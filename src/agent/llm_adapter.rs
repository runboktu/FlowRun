//! LLM 提供者 trait 定义

use async_trait::async_trait;
use crate::agent::types::LlmResponse;
use crate::agent::error::AgentError;
use crate::agent::types::Message;

/// LLM 提供者接口
/// 
/// 所有 LLM 实现必须实现此 trait
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// 同步调用 LLM
    /// 
    /// # Arguments
    /// - `messages`: 消息列表
    /// 
    /// # Returns
    /// - `Ok(LlmResponse)`: LLM 响应
    /// - `Err(AgentError)`: 调用失败
    async fn call(&self, messages: &[Message]) -> Result<LlmResponse, AgentError>;
}

/// Mock LLM 提供者（用于测试）
pub struct MockLlmProvider {
    responses: Vec<String>,
    call_count: std::sync::atomic::AtomicUsize,
}

impl MockLlmProvider {
    pub fn new(responses: Vec<String>) -> Self {
        Self {
            responses,
            call_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    pub fn call_count(&self) -> usize {
        self.call_count.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[async_trait]
impl LlmProvider for MockLlmProvider {
    async fn call(&self, _messages: &[Message]) -> Result<LlmResponse, AgentError> {
        let count = self.call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let response = self.responses
            .get(count % self.responses.len())
            .cloned()
            .unwrap_or_else(|| "<final_answer>Mock response</final_answer>".to_string());
        Ok(LlmResponse::success(response))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::types::Message;

    #[tokio::test]
    async fn test_mock_llm_provider() {
        let provider = MockLlmProvider::new(vec![
            "<thought>Thinking</thought><action>{\"name\":\"tool\",\"parameters\":{}}</action>".to_string(),
            "<thought>Done</thought><final_answer>Final answer</final_answer>".to_string(),
        ]);

        let messages = vec![Message::user("Hello")];
        
        let resp1 = provider.call(&messages).await.unwrap();
        assert!(resp1.success);
        assert!(resp1.content.contains("Thinking"));

        let resp2 = provider.call(&messages).await.unwrap();
        assert!(resp2.success);
        assert!(resp2.content.contains("Final answer"));

        let resp3 = provider.call(&messages).await.unwrap();
        assert!(resp3.success);
        assert!(resp3.content.contains("Thinking"));
    }
}
