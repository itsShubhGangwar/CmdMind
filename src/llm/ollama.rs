use std::env;
use std::fmt;
use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::intent::{CommandPlan, CommandSource};

/// Default base URL for the local Ollama daemon.
pub const DEFAULT_OLLAMA_BASE_URL: &str = "http://localhost:11434";

/// Default model name for local command generation.
pub const DEFAULT_OLLAMA_MODEL: &str = "llama3.2:3b";

/// System prompt instructed to the local LLM.
/// Enforces single-command JSON generation targeted at macOS zsh.
pub const SYSTEM_PROMPT: &str = "\
You are CmdMind, an assistant that converts natural-language developer requests into safe shell commands.\n\
CmdMind targets macOS zsh.\n\
You must respond ONLY with a valid JSON object matching this schema:\n\
{\n\
  \"command\": \"<single macOS/zsh shell command>\",\n\
  \"explanation\": \"<short human-readable explanation of what the command does>\"\n\
}\n\
Rules:\n\
- Generate a single shell command appropriate for the user's request.\n\
- Never return markdown code blocks, conversational filler, or text outside the JSON object.\n\
- Do not invent mock output or execution results.\n\
- You must not execute commands.\n\
- Avoid obviously destructive commands where possible.\n\
- Use macOS/zsh-compatible shell syntax.";

/// Configuration settings for connecting to the Ollama HTTP API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OllamaConfig {
    /// Base URL of the Ollama server (e.g., "http://localhost:11434")
    pub base_url: String,
    /// Name of the model to use (e.g., "llama3.2:3b")
    pub model: String,
    /// Request timeout duration
    pub timeout: Duration,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self::from_env()
    }
}

impl OllamaConfig {
    /// Reads configuration from environment variables with sensible defaults:
    /// - `OLLAMA_BASE_URL` (defaults to `http://localhost:11434`)
    /// - `OLLAMA_MODEL` (defaults to `llama3.2:3b`)
    pub fn from_env() -> Self {
        let base_url = env::var("OLLAMA_BASE_URL")
            .unwrap_or_else(|_| DEFAULT_OLLAMA_BASE_URL.to_string())
            .trim_end_matches('/')
            .to_string();

        let model = env::var("OLLAMA_MODEL").unwrap_or_else(|_| DEFAULT_OLLAMA_MODEL.to_string());

        Self {
            base_url,
            model,
            timeout: Duration::from_secs(30),
        }
    }
}

/// Errors that can occur during communication or parsing with Ollama.
#[derive(Debug)]
pub enum OllamaError {
    Connection(String),
    Timeout(String),
    Http(u16, String),
    JsonParse(String),
    EmptyCommand,
    MissingCommandField,
}

impl fmt::Display for OllamaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OllamaError::Connection(msg) => write!(
                f,
                "Could not connect to Ollama. Ensure the Ollama server is running locally (e.g., 'ollama serve').\nDetails: {}",
                msg
            ),
            OllamaError::Timeout(msg) => write!(
                f,
                "Ollama request timed out while generating a response.\nDetails: {}",
                msg
            ),
            OllamaError::Http(code, body) => write!(
                f,
                "Ollama returned HTTP error {}: {}",
                code, body
            ),
            OllamaError::JsonParse(msg) => write!(
                f,
                "Failed to parse structured JSON from Ollama.\nDetails: {}",
                msg
            ),
            OllamaError::EmptyCommand => write!(
                f,
                "Ollama returned an empty shell command."
            ),
            OllamaError::MissingCommandField => write!(
                f,
                "Ollama JSON response was missing the 'command' field."
            ),
        }
    }
}

impl std::error::Error for OllamaError {}

/// Payload sent to Ollama's `/api/generate` endpoint.
#[derive(Debug, Serialize)]
struct GenerateRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    system: &'a str,
    stream: bool,
    format: &'a str,
}

/// Raw wrapper response from Ollama's `/api/generate` endpoint.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GenerateResponse {
    response: String,
    #[serde(default)]
    done: bool,
}

/// Expected JSON structure produced by the LLM.
#[derive(Debug, Deserialize)]
struct StructuredCommandOutput {
    command: Option<String>,
    explanation: Option<String>,
}

/// Client for communicating with the local Ollama server.
pub struct OllamaClient {
    client: Client,
    pub config: OllamaConfig,
}

impl OllamaClient {
    /// Creates a new `OllamaClient` with the given configuration.
    pub fn new(config: OllamaConfig) -> Result<Self, OllamaError> {
        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| OllamaError::Connection(e.to_string()))?;

        Ok(Self { client, config })
    }

    /// Sends a natural-language request to Ollama and returns a structured `CommandPlan`.
    pub async fn generate_plan(&self, request: &str) -> Result<CommandPlan, OllamaError> {
        let endpoint = format!("{}/api/generate", self.config.base_url);

        let payload = GenerateRequest {
            model: &self.config.model,
            prompt: request,
            system: SYSTEM_PROMPT,
            stream: false,
            format: "json",
        };

        let response = self
            .client
            .post(&endpoint)
            .json(&payload)
            .send()
            .await
            .map_err(|err| {
                if err.is_timeout() {
                    OllamaError::Timeout(err.to_string())
                } else if err.is_connect() {
                    OllamaError::Connection(err.to_string())
                } else {
                    OllamaError::Connection(err.to_string())
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(OllamaError::Http(status.as_u16(), body));
        }

        let gen_response: GenerateResponse = response.json().await.map_err(|err| {
            OllamaError::JsonParse(format!("Failed to parse Ollama API envelope: {}", err))
        })?;

        parse_llm_json(&gen_response.response)
    }
}

/// Convenience function to call Ollama using environment configuration.
pub async fn call_ollama(request: &str) -> Result<CommandPlan, OllamaError> {
    let config = OllamaConfig::from_env();
    let client = OllamaClient::new(config)?;
    client.generate_plan(request).await
}

/// Parses the inner raw LLM JSON string into a verified `CommandPlan`.
/// Handles raw JSON strings as well as markdown-fenced blocks.
pub fn parse_llm_json(raw_json: &str) -> Result<CommandPlan, OllamaError> {
    let trimmed = raw_json.trim();
    if trimmed.is_empty() {
        return Err(OllamaError::EmptyCommand);
    }

    // Strip markdown code fences if the model included them
    let clean_json = if trimmed.starts_with("```") {
        let without_prefix = trimmed.trim_start_matches('`');
        let content = without_prefix
            .strip_prefix("json")
            .unwrap_or(without_prefix);
        content.trim_end_matches('`').trim()
    } else {
        trimmed
    };

    let parsed: StructuredCommandOutput = serde_json::from_str(clean_json)
        .map_err(|e| OllamaError::JsonParse(format!("{}: raw output was: '{}'", e, trimmed)))?;

    let command = match parsed.command {
        Some(cmd) => {
            let trimmed_cmd = cmd.trim().to_string();
            if trimmed_cmd.is_empty() {
                return Err(OllamaError::EmptyCommand);
            }
            trimmed_cmd
        }
        None => return Err(OllamaError::MissingCommandField),
    };

    let explanation = parsed
        .explanation
        .map(|e| e.trim().to_string())
        .filter(|e| !e.is_empty())
        .unwrap_or_else(|| "Generated by Ollama".to_string());

    Ok(CommandPlan::new(
        command,
        explanation,
        CommandSource::Ollama,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_json() {
        let raw = r#"{
            "command": "find . -type f -name '*.py' -mtime -2",
            "explanation": "Find Python files modified within the last two days."
        }"#;

        let plan = parse_llm_json(raw).expect("expected successful parsing");
        assert_eq!(plan.command, "find . -type f -name '*.py' -mtime -2");
        assert_eq!(
            plan.explanation,
            "Find Python files modified within the last two days."
        );
        assert_eq!(plan.source, CommandSource::Ollama);
    }

    #[test]
    fn test_parse_markdown_fenced_json() {
        let raw = "```json\n{\n  \"command\": \"ls -lah\",\n  \"explanation\": \"List all files with details\"\n}\n```";
        let plan =
            parse_llm_json(raw).expect("expected successful parsing of markdown-fenced json");
        assert_eq!(plan.command, "ls -lah");
        assert_eq!(plan.explanation, "List all files with details");
        assert_eq!(plan.source, CommandSource::Ollama);
    }

    #[test]
    fn test_parse_missing_command_field() {
        let raw = r#"{
            "explanation": "Only an explanation without a command"
        }"#;

        let err = parse_llm_json(raw).unwrap_err();
        match err {
            OllamaError::MissingCommandField => (),
            other => panic!("expected MissingCommandField, got: {:?}", other),
        }
    }

    #[test]
    fn test_parse_empty_command() {
        let raw = r#"{
            "command": "   ",
            "explanation": "Valid explanation but empty command"
        }"#;

        let err = parse_llm_json(raw).unwrap_err();
        match err {
            OllamaError::EmptyCommand => (),
            other => panic!("expected EmptyCommand, got: {:?}", other),
        }
    }

    #[test]
    fn test_parse_malformed_json() {
        let raw = "This is not JSON at all";
        let err = parse_llm_json(raw).unwrap_err();
        match err {
            OllamaError::JsonParse(_) => (),
            other => panic!("expected JsonParse, got: {:?}", other),
        }
    }

    #[test]
    fn test_parse_empty_string() {
        let raw = "   ";
        let err = parse_llm_json(raw).unwrap_err();
        match err {
            OllamaError::EmptyCommand => (),
            other => panic!("expected EmptyCommand, got: {:?}", other),
        }
    }

    #[test]
    fn test_default_config() {
        let config = OllamaConfig::default();
        assert_eq!(config.base_url, DEFAULT_OLLAMA_BASE_URL);
        assert!(!config.model.is_empty());
    }
}
