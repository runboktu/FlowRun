//! Agent 模块错误类型定义

use thiserror::Error;

/// Agent 模块错误类型
#[derive(Debug, Error)]
pub enum AgentError {
    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("LLM call failed: {0}")]
    LlmError(String),

    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Max iterations reached")]
    MaxIterationsReached,

    #[error("Response format invalid")]
    InvalidResponseFormat,

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_error_display() {
        let err = AgentError::SessionNotFound("session-123".to_string());
        assert_eq!(err.to_string(), "Session not found: session-123");

        let err = AgentError::LlmError("API key invalid".to_string());
        assert_eq!(err.to_string(), "LLM call failed: API key invalid");

        let err = AgentError::MaxIterationsReached;
        assert_eq!(err.to_string(), "Max iterations reached");
    }
}
