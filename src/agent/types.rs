//! Agent 模块核心类型定义
//! 
//! 翻译自 gtht-agent 的 types.hpp

use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Agent 状态枚举
/// 
/// 用于进度回调，标识 Agent 执行的各个阶段
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    /// 迭代开始
    IterationStart,
    /// LLM 调用中
    LlmCall,
    /// LLM 响应收到
    LlmResponse,
    /// LLM 流式响应 chunk
    LlmChunk,
    /// 工具调用中
    ToolCall,
    /// 工具结果返回
    ToolResult,
    /// 迭代结束
    IterationEnd,
    /// 重试
    Retry,
    /// 未知状态
    Unknown,
}

impl std::fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentStatus::IterationStart => write!(f, "iteration_start"),
            AgentStatus::LlmCall => write!(f, "llm_call"),
            AgentStatus::LlmResponse => write!(f, "llm_response"),
            AgentStatus::LlmChunk => write!(f, "llm_chunk"),
            AgentStatus::ToolCall => write!(f, "tool_call"),
            AgentStatus::ToolResult => write!(f, "tool_result"),
            AgentStatus::IterationEnd => write!(f, "iteration_end"),
            AgentStatus::Retry => write!(f, "retry"),
            AgentStatus::Unknown => write!(f, "unknown"),
        }
    }
}

/// 消息角色枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    /// 系统消息
    System,
    /// 用户消息
    User,
    /// 助手消息
    Assistant,
}

impl std::fmt::Display for MessageRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageRole::System => write!(f, "system"),
            MessageRole::User => write!(f, "user"),
            MessageRole::Assistant => write!(f, "assistant"),
        }
    }
}

/// 消息结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// 消息角色
    pub role: MessageRole,
    /// 消息内容
    pub content: String,
}

impl Message {
    /// 创建系统消息
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: content.into(),
        }
    }

    /// 创建用户消息
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
        }
    }

    /// 创建助手消息
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
        }
    }

    /// 转换为 JSON 格式（用于序列化给 LLM）
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "role": self.role.to_string(),
            "content": self.content,
        })
    }
}

/// LLM 响应结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    /// 响应内容
    pub content: String,
    /// 是否成功
    pub success: bool,
}

impl LlmResponse {
    /// 创建成功响应
    pub fn success(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            success: true,
        }
    }

    /// 创建失败响应
    pub fn error(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            success: false,
        }
    }
}

/// 工具结果结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// 结果内容
    pub content: String,
    /// 是否为错误
    pub is_error: bool,
}

impl ToolResult {
    /// 创建成功结果
    pub fn success(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
        }
    }

    /// 创建错误结果
    pub fn error(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
        }
    }
}

/// 工具处理器 trait
/// 
/// 所有工具必须实现此 trait
#[async_trait::async_trait]
pub trait ToolHandler: Send + Sync {
    /// 执行工具
    /// 
    /// # 参数
    /// - `args`: JSON 格式的参数字符串
    /// 
    /// # 返回
    /// - `ToolResult`: 工具执行结果
    async fn execute(&self, args: &str) -> ToolResult;
}

/// 工具描述符
/// 
/// 描述一个工具的元信息和执行逻辑
pub struct ToolDescriptor {
    /// 工具名称
    pub name: String,
    /// 工具描述
    pub description: String,
    /// JSON Schema（可选）
    pub json_schema: Option<String>,
    /// 工具处理器
    pub handler: Arc<dyn ToolHandler>,
}

impl std::fmt::Debug for ToolDescriptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolDescriptor")
            .field("name", &self.name)
            .field("description", &self.description)
            .field("json_schema", &self.json_schema)
            .field("handler", &"<ToolHandler>")
            .finish()
    }
}

impl Clone for ToolDescriptor {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            description: self.description.clone(),
            json_schema: self.json_schema.clone(),
            handler: self.handler.clone(),
        }
    }
}

/// Agent 进度回调类型
pub type AgentCallback = Arc<dyn Fn(String, AgentStatus) + Send + Sync>;

/// 进度数据类型常量
pub const PROGRESS_TYPE_ITERATION_START: &str = "iteration_start";
pub const PROGRESS_TYPE_LLM_CALL: &str = "llm_call";
pub const PROGRESS_TYPE_LLM_RESPONSE: &str = "llm_response";
pub const PROGRESS_TYPE_TOOL_CALL: &str = "tool_call";
pub const PROGRESS_TYPE_TOOL_RESULT: &str = "tool_result";
pub const PROGRESS_TYPE_FINAL_ANSWER: &str = "final_answer";
pub const PROGRESS_TYPE_ERROR: &str = "error";

/// 解析器类型枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParserType {
    /// XML 格式解析器
    Xml,
    /// JSON 格式解析器
    Json,
}

/// 解析结果结构
#[derive(Debug, Clone)]
pub struct ParsedResponse {
    /// 思考内容
    pub thought: Option<String>,
    /// 动作内容
    pub action: Option<String>,
    /// 最终答案
    pub final_answer: Option<String>,
}

impl ParsedResponse {
    /// 检查是否包含有效的动作或最终答案
    pub fn has_action_or_answer(&self) -> bool {
        self.action.is_some() || self.final_answer.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_status_display() {
        assert_eq!(AgentStatus::IterationStart.to_string(), "iteration_start");
        assert_eq!(AgentStatus::LlmCall.to_string(), "llm_call");
        assert_eq!(AgentStatus::ToolCall.to_string(), "tool_call");
    }

    #[test]
    fn test_message_role_display() {
        assert_eq!(MessageRole::System.to_string(), "system");
        assert_eq!(MessageRole::User.to_string(), "user");
        assert_eq!(MessageRole::Assistant.to_string(), "assistant");
    }

    #[test]
    fn test_message_creation() {
        let msg = Message::system("You are a helpful assistant.");
        assert_eq!(msg.role, MessageRole::System);
        assert_eq!(msg.content, "You are a helpful assistant.");

        let msg = Message::user("Hello!");
        assert_eq!(msg.role, MessageRole::User);
        assert_eq!(msg.content, "Hello!");
    }

    #[test]
    fn test_message_to_json() {
        let msg = Message::user("Hello!");
        let json = msg.to_json();
        assert_eq!(json["role"], "user");
        assert_eq!(json["content"], "Hello!");
    }

    #[test]
    fn test_llm_response() {
        let resp = LlmResponse::success("Response content");
        assert!(resp.success);
        assert_eq!(resp.content, "Response content");

        let resp = LlmResponse::error("Error occurred");
        assert!(!resp.success);
        assert_eq!(resp.content, "Error occurred");
    }

    #[test]
    fn test_tool_result() {
        let result = ToolResult::success("Tool output");
        assert!(!result.is_error);
        assert_eq!(result.content, "Tool output");

        let result = ToolResult::error("Tool failed");
        assert!(result.is_error);
        assert_eq!(result.content, "Tool failed");
    }

    #[test]
    fn test_parsed_response() {
        let parsed = ParsedResponse {
            thought: Some("Thinking...".to_string()),
            action: Some(r#"{"name":"tool","parameters":{}}"#.to_string()),
            final_answer: None,
        };
        assert!(parsed.has_action_or_answer());
        assert!(parsed.action.is_some());
        assert!(parsed.final_answer.is_none());

        let parsed = ParsedResponse {
            thought: None,
            action: None,
            final_answer: Some("Final answer".to_string()),
        };
        assert!(parsed.has_action_or_answer());
    }
}
