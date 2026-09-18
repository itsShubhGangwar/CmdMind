/// Represents an individual persisted command history record stored in SQLite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    /// Unique auto-incrementing ID in SQLite
    pub id: i64,
    /// The user's original natural-language request
    pub request: String,
    /// The validated shell command
    pub command: String,
    /// Human-readable explanation / intent description
    pub explanation: String,
    /// Origin engine source ("Tier-1" or "Ollama")
    pub source: String,
    /// ISO 8601 (RFC 3339) timestamp of when the command was generated and validated
    pub created_at: String,
    /// Execution status ("validated", "executed", "failed", etc.)
    pub status: String,
}

impl HistoryEntry {
    /// Creates a new `HistoryEntry` instance with default status of "validated".
    pub fn new(
        id: i64,
        request: impl Into<String>,
        command: impl Into<String>,
        explanation: impl Into<String>,
        source: impl Into<String>,
        created_at: impl Into<String>,
    ) -> Self {
        Self::with_status(
            id,
            request,
            command,
            explanation,
            source,
            created_at,
            "validated",
        )
    }

    /// Creates a new `HistoryEntry` instance with an explicit status.
    pub fn with_status(
        id: i64,
        request: impl Into<String>,
        command: impl Into<String>,
        explanation: impl Into<String>,
        source: impl Into<String>,
        created_at: impl Into<String>,
        status: impl Into<String>,
    ) -> Self {
        Self {
            id,
            request: request.into(),
            command: command.into(),
            explanation: explanation.into(),
            source: source.into(),
            created_at: created_at.into(),
            status: status.into(),
        }
    }
}
