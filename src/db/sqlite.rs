use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::Utc;
use rusqlite::{params, Connection};

use super::models::HistoryEntry;
use crate::security::ValidatedPlan;

/// Standard database filename.
pub const DEFAULT_DB_FILENAME: &str = "cmdmind.db";

/// Errors occurring during SQLite database operations.
#[derive(Debug)]
pub enum DbError {
    Connection(String),
    Query(String),
    Io(String),
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DbError::Connection(msg) => write!(f, "Failed to connect to SQLite database: {}", msg),
            DbError::Query(msg) => write!(f, "SQLite query execution failed: {}", msg),
            DbError::Io(msg) => write!(f, "Database filesystem I/O error: {}", msg),
        }
    }
}

impl std::error::Error for DbError {}

impl From<rusqlite::Error> for DbError {
    fn from(err: rusqlite::Error) -> Self {
        DbError::Query(err.to_string())
    }
}

/// Encapsulates access to the local SQLite database for CmdMind.
///
/// SQLite is used exclusively for local command persistence. It contains no execution capabilities.
#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    /// Opens or creates an SQLite database at the specified filesystem path.
    pub fn open(path: &Path) -> Result<Self, DbError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                DbError::Io(format!(
                    "Failed to create database directory {}: {}",
                    parent.display(),
                    e
                ))
            })?;
        }

        let conn = Connection::open(path).map_err(|e| {
            DbError::Connection(format!(
                "Could not open SQLite database at {}: {}",
                path.display(),
                e
            ))
        })?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.initialize()?;
        Ok(db)
    }

    /// Opens an in-memory SQLite database, primarily for isolated, high-speed unit testing.
    #[allow(dead_code)]
    pub fn open_in_memory() -> Result<Self, DbError> {
        let conn = Connection::open_in_memory()
            .map_err(|e| DbError::Connection(format!("Failed to open in-memory SQLite: {}", e)))?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.initialize()?;
        Ok(db)
    }

    /// Resolves the default portable application data location and opens the database:
    /// - Checks `CMDMIND_DB_PATH` environment variable for user overrides.
    /// - Otherwise uses platform standard:
    ///   - macOS: `~/Library/Application Support/cmdmind/cmdmind.db`
    ///   - Linux: `~/.local/share/cmdmind/cmdmind.db`
    ///   - Windows: `%LOCALAPPDATA%\cmdmind\cmdmind.db`
    pub fn open_default() -> Result<Self, DbError> {
        let path = Self::resolve_default_path();
        Self::open(&path)
    }

    /// Resolves the platform-specific database path.
    pub fn resolve_default_path() -> PathBuf {
        if let Ok(custom_path) = env::var("CMDMIND_DB_PATH") {
            if !custom_path.trim().is_empty() {
                return PathBuf::from(custom_path.trim());
            }
        }

        if let Some(data_dir) = dirs::data_local_dir() {
            data_dir.join("cmdmind").join(DEFAULT_DB_FILENAME)
        } else if let Some(home_dir) = dirs::home_dir() {
            home_dir.join(".cmdmind").join(DEFAULT_DB_FILENAME)
        } else {
            PathBuf::from(DEFAULT_DB_FILENAME)
        }
    }

    /// Initializes the database schema idempotently using `CREATE TABLE IF NOT EXISTS`.
    pub fn initialize(&self) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS command_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                request TEXT NOT NULL,
                command TEXT NOT NULL,
                explanation TEXT NOT NULL,
                source TEXT NOT NULL,
                created_at TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'validated'
            );",
            [],
        )?;
        // Ensure status column exists if the table was created in an earlier phase
        let _ = conn.execute(
            "ALTER TABLE command_history ADD COLUMN status TEXT NOT NULL DEFAULT 'validated';",
            [],
        );
        Ok(())
    }

    /// Saves a successfully validated command plan into the SQLite database with an execution status.
    ///
    /// Security Boundary:
    /// This method accepts `&ValidatedPlan` exclusively. Raw, unvalidated `CommandPlan` instances
    /// cannot be saved, guaranteeing that rejected or untrusted commands never enter history.
    ///
    /// SQL Safety:
    /// Uses bound parameterized queries to prevent any SQL injection or escaping bugs.
    pub fn save_command_with_status(
        &self,
        request: &str,
        plan: &ValidatedPlan,
        status: &str,
    ) -> Result<HistoryEntry, DbError> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        let source_str = plan.source().to_string();

        conn.execute(
            "INSERT INTO command_history (request, command, explanation, source, created_at, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6);",
            params![request, plan.command(), plan.explanation(), source_str, now, status],
        )?;

        let id = conn.last_insert_rowid();

        Ok(HistoryEntry::with_status(
            id,
            request,
            plan.command(),
            plan.explanation(),
            source_str,
            now,
            status,
        ))
    }

    /// Saves a successfully validated command plan into the SQLite database with default status 'validated'.
    pub fn save_command(
        &self,
        request: &str,
        plan: &ValidatedPlan,
    ) -> Result<HistoryEntry, DbError> {
        self.save_command_with_status(request, plan, "validated")
    }

    /// Retrieves the most recent command history entries in descending order of creation.
    pub fn get_recent_history(&self, limit: usize) -> Result<Vec<HistoryEntry>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, request, command, explanation, source, created_at, status
             FROM command_history
             ORDER BY id DESC
             LIMIT ?1;",
        )?;

        let entries_iter = stmt.query_map(params![limit as i64], |row| {
            Ok(HistoryEntry::with_status(
                row.get(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)
                    .unwrap_or_else(|_| "validated".to_string()),
            ))
        })?;

        let mut entries = Vec::new();
        for entry in entries_iter {
            entries.push(entry?);
        }

        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::{CommandPlan, CommandSource};
    use crate::security::validate;

    fn make_validated(cmd: &str, explanation: &str, source: CommandSource) -> ValidatedPlan {
        let plan = CommandPlan::new(cmd, explanation, source);
        validate(plan).expect("command must validate")
    }

    #[test]
    fn test_initialize_and_open_in_memory() {
        let db = Database::open_in_memory().expect("in-memory database should open");
        let history = db
            .get_recent_history(10)
            .expect("querying history succeeds");
        assert!(history.is_empty());
    }

    #[test]
    fn test_insert_and_retrieve_validated_plan() {
        let db = Database::open_in_memory().expect("db open");
        let plan = make_validated("ls", "List files", CommandSource::Tier1);

        let saved = db.save_command("list files", &plan).expect("save succeeds");
        assert_eq!(saved.id, 1);
        assert_eq!(saved.request, "list files");
        assert_eq!(saved.command, "ls");
        assert_eq!(saved.source, "Tier-1");

        let recent = db.get_recent_history(5).expect("retrieve succeeds");
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].command, "ls");
        assert_eq!(recent[0].request, "list files");
    }

    #[test]
    fn test_multiple_sources_and_ordering() {
        let db = Database::open_in_memory().expect("db open");

        let tier1_plan = make_validated(
            "find . -type f -name '*.pdf'",
            "Find PDF files",
            CommandSource::Tier1,
        );
        db.save_command("find all pdf files", &tier1_plan)
            .expect("saved tier-1");

        let ollama_plan = make_validated(
            "find . -type f -name '*.py' -mtime -2",
            "Find recent Python files",
            CommandSource::Ollama,
        );
        db.save_command("find python files modified recently", &ollama_plan)
            .expect("saved ollama");

        let recent = db.get_recent_history(10).expect("retrieve succeeds");
        assert_eq!(recent.len(), 2);

        // Most recent first (DESC by id)
        assert_eq!(recent[0].source, "Ollama");
        assert_eq!(recent[0].command, "find . -type f -name '*.py' -mtime -2");
        assert_eq!(recent[1].source, "Tier-1");
        assert_eq!(recent[1].command, "find . -type f -name '*.pdf'");
    }

    #[test]
    fn test_parameterized_special_characters() {
        let db = Database::open_in_memory().expect("db open");

        // Request containing single quotes, double quotes, unicode, spaces
        let complex_request = "find 'quoted \"complex\"' 🐍 files & scripts";
        let plan = make_validated(
            "find . -name '*quoted*'",
            "Search for quoted",
            CommandSource::Tier1,
        );

        let saved = db
            .save_command(complex_request, &plan)
            .expect("save with quotes and unicode succeeds");
        assert_eq!(saved.request, complex_request);

        let recent = db.get_recent_history(1).expect("retrieve succeeds");
        assert_eq!(recent[0].request, complex_request);
        assert_eq!(recent[0].command, "find . -name '*quoted*'");
    }

    #[test]
    fn test_persistence_across_reopen() {
        let temp_dir = std::env::temp_dir().join(format!(
            "cmdmind_test_{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let db_path = temp_dir.join("test_history.db");

        {
            let db = Database::open(&db_path).expect("open file db");
            let plan = make_validated("git status", "Show git status", CommandSource::Tier1);
            db.save_command("check git status", &plan)
                .expect("save entry");
        } // database connection closed here

        {
            // Reopen the database from disk
            let db_reopened = Database::open(&db_path).expect("reopen file db");
            let history = db_reopened
                .get_recent_history(10)
                .expect("read reopened db");
            assert_eq!(history.len(), 1);
            assert_eq!(history[0].command, "git status");
            assert_eq!(history[0].request, "check git status");
        }

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_rejected_command_not_saved() {
        let db = Database::open_in_memory().expect("db open");

        // An untrusted dangerous plan
        let dangerous_plan = CommandPlan::new("rm -rf /", "Delete root", CommandSource::Tier1);

        // Security validation fails!
        let validation_result = validate(dangerous_plan);
        assert!(validation_result.is_err(), "must fail validation");

        // Because validation failed, no ValidatedPlan exists to pass into save_command!
        // Database remains completely empty.
        let history = db.get_recent_history(10).expect("retrieve succeeds");
        assert!(
            history.is_empty(),
            "database must not record rejected commands"
        );
    }

    // --- Type-State Enforcement Test ---

    #[test]
    fn test_type_state_prevents_unvalidated_plan() {
        let db = Database::open_in_memory().expect("db open");
        let raw_plan = CommandPlan::new("ls", "List files", CommandSource::Tier1);

        // Passing &raw_plan to db.save_command() is a COMPILE-TIME type error!
        // Only &ValidatedPlan can be saved:
        let validated = validate(raw_plan).expect("validates");
        let result = db.save_command("list files", &validated);
        assert!(result.is_ok());
    }

    #[test]
    fn test_save_command_with_status_executed_and_failed() {
        let db = Database::open_in_memory().expect("db open");
        let plan1 = make_validated("git status", "Show git status", CommandSource::Tier1);
        db.save_command_with_status("check git status", &plan1, "executed")
            .expect("save executed");

        let plan2 = make_validated("ls nonexistent", "List files", CommandSource::Tier1);
        db.save_command_with_status("list nonexistent", &plan2, "failed")
            .expect("save failed");

        let history = db.get_recent_history(10).expect("retrieve");
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].status, "failed");
        assert_eq!(history[1].status, "executed");
    }
}
