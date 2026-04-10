//! ReAct Agent 核心实现
//! 
//! 翻译自 gtht-agent 的 react_agent.hpp/cpp + agent_manager.hpp/cpp

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;
use tracing::info;

use crate::agent::types::{
    Message, AgentStatus, AgentCallback, 
    ToolDescriptor, ParserType,
};
use crate::agent::error::AgentError;
use crate::agent::llm_adapter::LlmProvider;
use crate::agent::tool_registry::ToolRegistry;
use crate::agent::response_parser::{ResponseParser, create_parser};
use crate::agent::system_prompt::{render_system_prompt, DEFAULT_SYSTEM_PROMPT};
use tokio_stream::StreamExt;

/// ReAct Agent
pub struct ReActAgent {
    session_id: String,
    messages: Vec<Message>,
    tool_registry: Arc<ToolRegistry>,
    llm_provider: Arc<dyn LlmProvider>,
    parser: Box<dyn ResponseParser>,
    max_iterations: u32,
    system_prompt_template: String,
    user_prompt: String,
}

impl ReActAgent {
    pub fn new(
        session_id: String,
        llm_provider: Arc<dyn LlmProvider>,
    ) -> Self {
        Self {
            session_id,
            messages: Vec::new(),
            tool_registry: Arc::new(ToolRegistry::new()),
            llm_provider,
            parser: create_parser(ParserType::Xml),
            max_iterations: 10,
            system_prompt_template: DEFAULT_SYSTEM_PROMPT.to_string(),
            user_prompt: String::new(),
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn set_user_prompt(&mut self, prompt: &str) {
        self.user_prompt = prompt.to_string();
    }

    pub fn set_max_iterations(&mut self, max: u32) {
        self.max_iterations = max;
    }

    pub fn set_parser_type(&mut self, parser_type: ParserType) {
        self.parser = create_parser(parser_type);
    }

    pub fn clear_history(&mut self) {
        self.messages.clear();
    }

    pub fn history(&self) -> &[Message] {
        &self.messages
    }

    pub async fn register_tool(&self, descriptor: ToolDescriptor) {
        self.tool_registry.register(descriptor).await;
    }

    async fn render_system_prompt(&self) -> String {
        let tool_list = self.tool_registry.get_tool_list().await;
        render_system_prompt(&self.system_prompt_template, &self.user_prompt, &tool_list)
    }

    /// 同步运行 Agent
    pub async fn run(&mut self, user_input: &str) -> Result<String, AgentError> {
        self.run_internal(user_input).await
    }

    /// 带进度回调的运行
    pub async fn run_with_callback(
        &mut self,
        user_input: &str,
        callback: AgentCallback,
    ) -> Result<String, AgentError> {
        self.run_internal_with_progress(user_input, callback).await
    }

    /// 流式运行 Agent
    pub async fn run_stream(
        &mut self,
        user_input: &str,
        callback: AgentCallback,
    ) -> Result<String, AgentError> {
        self.run_internal_stream(user_input, callback).await
    }

    async fn run_internal(&mut self, user_input: &str) -> Result<String, AgentError> {
        self.messages.clear();
        
        let system_prompt = self.render_system_prompt().await;
        self.messages.push(Message::system(system_prompt));
        self.messages.push(Message::user(format!("<question>{}</question>", user_input)));

        let mut iteration_count = 0;
        while iteration_count < self.max_iterations {
            let response = self.llm_provider.call(&self.messages).await?;
            
            if !response.success {
                return Err(AgentError::LlmError(response.content));
            }

            self.messages.push(Message::assistant(response.content.clone()));
            let parsed = self.parser.parse(&response.content);

            if let Some(final_answer) = parsed.final_answer {
                return Ok(final_answer);
            }

            if let Some(action) = parsed.action {
                let (tool_name, args_str) = self.parser.parse_action(&action);
                
                if !self.tool_registry.has_tool(&tool_name).await {
                    let error_msg = format!("Tool '{}' not found", tool_name);
                    self.messages.push(Message::user(format!("<observation>{}</observation>", error_msg)));
                    continue;
                }

                let result = self.tool_registry.execute(&tool_name, &args_str).await;
                self.messages.push(Message::user(format!("<observation>{}</observation>", result.content)));
            } else {
                return Ok(response.content);
            }

            iteration_count += 1;
        }

        Err(AgentError::MaxIterationsReached)
    }

    async fn run_internal_with_progress(
        &mut self,
        user_input: &str,
        callback: AgentCallback,
    ) -> Result<String, AgentError> {
        self.messages.clear();
        
        let system_prompt = self.render_system_prompt().await;
        self.messages.push(Message::system(system_prompt));
        self.messages.push(Message::user(format!("<question>{}</question>", user_input)));

        callback(
            serde_json::json!({"type": "iteration_start"}).to_string(),
            AgentStatus::IterationStart,
        );

        let mut iteration_count = 0;
        while iteration_count < self.max_iterations {
            iteration_count += 1;

            callback(
                serde_json::json!({"type": "llm_call", "iteration": iteration_count}).to_string(),
                AgentStatus::LlmCall,
            );

            let response = self.llm_provider.call(&self.messages).await?;

            callback(
                serde_json::json!({
                    "type": "llm_response",
                    "content": response.content,
                    "success": response.success,
                    "iteration": iteration_count,
                }).to_string(),
                AgentStatus::LlmResponse,
            );

            if !response.success {
                callback(
                    serde_json::json!({"type": "error", "content": response.content}).to_string(),
                    AgentStatus::Retry,
                );
                continue;
            }

            self.messages.push(Message::assistant(response.content.clone()));
            let parsed = self.parser.parse(&response.content);

            if let Some(final_answer) = &parsed.final_answer {
                callback(
                    serde_json::json!({"type": "final_answer", "content": final_answer}).to_string(),
                    AgentStatus::IterationEnd,
                );
                return Ok(final_answer.clone());
            }

            if let Some(action) = &parsed.action {
                let (tool_name, args_str) = self.parser.parse_action(action);

                callback(
                    serde_json::json!({
                        "type": "tool_call",
                        "tool_name": tool_name,
                        "args": args_str,
                        "iteration": iteration_count,
                    }).to_string(),
                    AgentStatus::ToolCall,
                );

                if !self.tool_registry.has_tool(&tool_name).await {
                    let error_msg = format!("Tool '{}' not found", tool_name);
                    callback(
                        serde_json::json!({"type": "error", "content": error_msg}).to_string(),
                        AgentStatus::Retry,
                    );
                    self.messages.push(Message::user(format!("<observation>{}</observation>", error_msg)));
                    continue;
                }

                let result = self.tool_registry.execute(&tool_name, &args_str).await;

                callback(
                    serde_json::json!({
                        "type": "tool_result",
                        "content": result.content,
                        "is_error": result.is_error,
                    }).to_string(),
                    AgentStatus::ToolResult,
                );

                self.messages.push(Message::user(format!("<observation>{}</observation>", result.content)));
            } else {
                return Ok(response.content);
            }
        }

        callback(
            serde_json::json!({"type": "error", "content": "Max iterations reached"}).to_string(),
            AgentStatus::IterationEnd,
        );
        Err(AgentError::MaxIterationsReached)
    }

    async fn run_internal_stream(
        &mut self,
        user_input: &str,
        callback: AgentCallback,
    ) -> Result<String, AgentError> {
        self.messages.clear();

        let system_prompt = self.render_system_prompt().await;
        self.messages.push(Message::system(system_prompt));
        self.messages.push(Message::user(format!("<question>{}</question>", user_input)));

        callback(
            serde_json::json!({"type": "iteration_start"}).to_string(),
            AgentStatus::IterationStart,
        );

        let mut iteration_count = 0;
        while iteration_count < self.max_iterations {
            iteration_count += 1;

            callback(
                serde_json::json!({"type": "llm_call", "iteration": iteration_count}).to_string(),
                AgentStatus::LlmCall,
            );

            let messages = self.messages.clone();
            let mut stream = self.llm_provider.call_stream(messages);
            let mut accumulator = String::new();

            while let Some(chunk_result) = stream.next().await {
                let chunk = chunk_result?;
                accumulator.push_str(&chunk.delta);

                callback(
                    serde_json::json!({
                        "type": "llm_chunk",
                        "delta": chunk.delta,
                        "accumulated_length": accumulator.len(),
                        "iteration": iteration_count,
                    }).to_string(),
                    AgentStatus::LlmChunk,
                );

                if chunk.done {
                    if let Some(usage) = chunk.usage {
                        callback(
                            serde_json::json!({
                                "type": "token_usage",
                                "prompt_tokens": usage.prompt_tokens,
                                "completion_tokens": usage.completion_tokens,
                                "total_tokens": usage.total_tokens,
                            }).to_string(),
                            AgentStatus::LlmResponse,
                        );
                    }
                    break;
                }
            }

            self.messages.push(Message::assistant(accumulator.clone()));
            let parsed = self.parser.parse(&accumulator);

            if let Some(final_answer) = &parsed.final_answer {
                callback(
                    serde_json::json!({"type": "final_answer", "content": final_answer}).to_string(),
                    AgentStatus::IterationEnd,
                );
                return Ok(final_answer.clone());
            }

            if let Some(action) = &parsed.action {
                let (tool_name, args_str) = self.parser.parse_action(action);

                callback(
                    serde_json::json!({
                        "type": "tool_call",
                        "tool_name": tool_name,
                        "args": args_str,
                        "iteration": iteration_count,
                    }).to_string(),
                    AgentStatus::ToolCall,
                );

                if !self.tool_registry.has_tool(&tool_name).await {
                    let error_msg = format!("Tool '{}' not found", tool_name);
                    callback(
                        serde_json::json!({"type": "error", "content": error_msg}).to_string(),
                        AgentStatus::Retry,
                    );
                    self.messages.push(Message::user(format!("<observation>{}</observation>", error_msg)));
                    continue;
                }

                let result = self.tool_registry.execute(&tool_name, &args_str).await;

                callback(
                    serde_json::json!({
                        "type": "tool_result",
                        "content": result.content,
                        "is_error": result.is_error,
                    }).to_string(),
                    AgentStatus::ToolResult,
                );

                self.messages.push(Message::user(format!("<observation>{}</observation>", result.content)));
            } else {
                return Ok(accumulator);
            }
        }

        callback(
            serde_json::json!({"type": "error", "content": "Max iterations reached"}).to_string(),
            AgentStatus::IterationEnd,
        );
        Err(AgentError::MaxIterationsReached)
    }
}

/// 会话管理器
pub struct AgentManager {
    sessions: Arc<RwLock<HashMap<String, ReActAgent>>>,
    default_llm: Option<Arc<dyn LlmProvider>>,
}

impl AgentManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            default_llm: None,
        }
    }

    pub fn with_llm(mut self, provider: Arc<dyn LlmProvider>) -> Self {
        self.default_llm = Some(provider);
        self
    }

    pub async fn create_session(&self, user_prompt: Option<&str>) -> Result<String, AgentError> {
        let session_id = Uuid::new_v4().to_string();
        let llm = self.default_llm.clone()
            .ok_or_else(|| AgentError::ConfigError("No LLM provider set".to_string()))?;

        let mut agent = ReActAgent::new(session_id.clone(), llm);

        if let Some(prompt) = user_prompt {
            agent.set_user_prompt(prompt);
        }

        info!("[AgentManager] Created session: {}", session_id);
        self.sessions.write().await.insert(session_id.clone(), agent);
        Ok(session_id)
    }

    pub async fn create_session_with_llm(
        &self,
        llm: Arc<dyn LlmProvider>,
        user_prompt: Option<&str>,
    ) -> Result<String, AgentError> {
        let session_id = Uuid::new_v4().to_string();

        let mut agent = ReActAgent::new(session_id.clone(), llm);

        if let Some(prompt) = user_prompt {
            agent.set_user_prompt(prompt);
        }

        info!("[AgentManager] Created session: {}", session_id);
        self.sessions.write().await.insert(session_id.clone(), agent);
        Ok(session_id)
    }

    pub async fn destroy_session(&self, session_id: &str) -> bool {
        let removed = self.sessions.write().await.remove(session_id).is_some();
        if removed {
            info!("[AgentManager] Destroyed session: {}", session_id);
        }
        removed
    }

    pub async fn session_exists(&self, session_id: &str) -> bool {
        self.sessions.read().await.contains_key(session_id)
    }

    pub async fn run_sync(
        &self,
        session_id: &str,
        user_input: &str,
        callback: Option<AgentCallback>,
    ) -> Result<String, AgentError> {
        let mut sessions = self.sessions.write().await;
        let agent = sessions.get_mut(session_id)
            .ok_or_else(|| AgentError::SessionNotFound(session_id.to_string()))?;

        if let Some(cb) = callback {
            agent.run_with_callback(user_input, cb).await
        } else {
            agent.run(user_input).await
        }
    }

    pub async fn run_sync_stream(
        &self,
        session_id: &str,
        user_input: &str,
        callback: AgentCallback,
    ) -> Result<String, AgentError> {
        let mut sessions = self.sessions.write().await;
        let agent = sessions.get_mut(session_id)
            .ok_or_else(|| AgentError::SessionNotFound(session_id.to_string()))?;
        agent.run_stream(user_input, callback).await
    }

    pub async fn set_max_iterations(&self, session_id: &str, max: u32) -> Result<(), AgentError> {
        let mut sessions = self.sessions.write().await;
        let agent = sessions.get_mut(session_id)
            .ok_or_else(|| AgentError::SessionNotFound(session_id.to_string()))?;
        agent.set_max_iterations(max);
        Ok(())
    }

    pub async fn clear_history(&self, session_id: &str) -> Result<(), AgentError> {
        let mut sessions = self.sessions.write().await;
        let agent = sessions.get_mut(session_id)
            .ok_or_else(|| AgentError::SessionNotFound(session_id.to_string()))?;
        agent.clear_history();
        Ok(())
    }

    pub async fn register_tool(&self, session_id: &str, descriptor: ToolDescriptor) -> Result<(), AgentError> {
        let sessions = self.sessions.read().await;
        let agent = sessions.get(session_id)
            .ok_or_else(|| AgentError::SessionNotFound(session_id.to_string()))?;
        agent.register_tool(descriptor).await;
        Ok(())
    }

    pub async fn session_ids(&self) -> Vec<String> {
        self.sessions.read().await.keys().cloned().collect()
    }

    pub async fn session_count(&self) -> usize {
        self.sessions.read().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::llm_adapter::MockLlmProvider;
    use crate::agent::tool_registry::FnTool;

    #[tokio::test]
    async fn test_react_agent_basic() {
        let llm = Arc::new(MockLlmProvider::new(vec![
            r#"<thought>I should use echo tool</thought><action>{"name":"echo","parameters":{"message":"hello"}}</action>"#.to_string(),
            r#"<thought>Done</thought><final_answer>The result is Echo: hello</final_answer>"#.to_string(),
        ]));

        let mut agent = ReActAgent::new(
            "test-session".to_string(),
            llm,
        );

        agent.register_tool(ToolDescriptor {
            name: "echo".to_string(),
            description: "Echoes input".to_string(),
            json_schema: None,
            handler: Arc::new(FnTool(|args: String| async move {
                format!("Echo: {}", args)
            })),
        }).await;

        let result = agent.run("Echo hello").await.unwrap();
        assert!(result.contains("Echo: hello"));
    }

    #[tokio::test]
    async fn test_agent_manager() {
        let llm = Arc::new(MockLlmProvider::new(vec![
            r#"<final_answer>Hello!</final_answer>"#.to_string(),
        ]));

        let manager = AgentManager::new().with_llm(llm);
        let session_id = manager.create_session(Some("You are helpful")).await.unwrap();

        assert!(manager.session_exists(&session_id).await);
        assert_eq!(manager.session_count().await, 1);

        let result = manager.run_sync(&session_id, "Hi", None).await.unwrap();
        assert_eq!(result, "Hello!");

        assert!(manager.destroy_session(&session_id).await);
        assert!(!manager.session_exists(&session_id).await);
    }
}
