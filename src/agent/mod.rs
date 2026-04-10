//! Agent 模块
//! 
//! 提供 ReAct（Reasoning + Acting）范式的 AI Agent 能力

pub mod types;
pub mod error;
pub mod llm_adapter;
pub mod util;
pub mod system_prompt;
pub mod tool_registry;
pub mod tool_implementations;
pub mod tool_factory;
pub mod response_parser;
pub mod react_agent;

pub use types::{
    AgentStatus, Message, MessageRole, LlmResponse, ToolResult,
    ToolDescriptor, ToolHandler, AgentCallback, ParserType, ParsedResponse,
    LlmChunk, TokenUsage, LlmStream,
    PROGRESS_TYPE_ITERATION_START, PROGRESS_TYPE_LLM_CALL,
    PROGRESS_TYPE_LLM_RESPONSE, PROGRESS_TYPE_TOOL_CALL,
    PROGRESS_TYPE_TOOL_RESULT, PROGRESS_TYPE_FINAL_ANSWER,
    PROGRESS_TYPE_ERROR,
};
pub use error::AgentError;
pub use llm_adapter::{LlmProvider, LlmProviderConfig, create_llm_provider, DeepSeekProvider, OpenAiCompatibleProvider};
pub use tool_registry::ToolRegistry;
pub use tool_factory::create_tool_handler;
pub use tool_implementations::{ShellTool, HttpTool, PythonTool};
pub use response_parser::{ResponseParser, XmlResponseParser, JsonResponseParser, create_parser};
pub use react_agent::{ReActAgent, AgentManager};
