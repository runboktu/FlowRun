//! LLM 提供者 trait 定义

use async_trait::async_trait;
use crate::agent::types::{LlmResponse, LlmChunk, LlmStream, Message};
use crate::agent::error::AgentError;
use std::sync::Arc;
use tokio_stream::StreamExt;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct LlmProviderConfig {
    pub r#type: String,
    pub model: Option<String>,
    pub api_key: String,
    pub base_url: Option<String>,
}

pub fn create_llm_provider(config: &LlmProviderConfig) -> Result<Arc<dyn LlmProvider>, AgentError> {
    match config.r#type.as_str() {
        "deepseek" => {
            let provider = DeepSeekProvider::new(
                &config.api_key,
                config.model.as_deref().unwrap_or("deepseek-chat"),
                config.base_url.as_deref().unwrap_or("https://api.deepseek.com/v1"),
            );
            Ok(Arc::new(provider))
        }
        "openai" => {
            let provider = OpenAiCompatibleProvider::new(
                &config.api_key,
                config.model.as_deref().unwrap_or("gpt-3.5-turbo"),
                config.base_url.as_deref().unwrap_or("https://api.openai.com/v1"),
            );
            Ok(Arc::new(provider))
        }
        other => Err(AgentError::ConfigError(format!(
            "Unsupported LLM provider type: {}", other
        ))),
    }
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn call(&self, messages: &[Message]) -> Result<LlmResponse, AgentError>;

    fn call_stream(&self, messages: Vec<Message>) -> LlmStream;
}

pub struct OpenAiCompatibleProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl OpenAiCompatibleProvider {
    pub fn new(api_key: &str, model: &str, base_url: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleProvider {
    async fn call(&self, messages: &[Message]) -> Result<LlmResponse, AgentError> {
        let url = format!("{}/chat/completions", self.base_url);
        let body = serde_json::json!({
            "model": self.model,
            "messages": messages.iter().map(|m| {
                serde_json::json!({
                    "role": m.role.to_string(),
                    "content": m.content,
                })
            }).collect::<Vec<_>>(),
        });

        let response = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AgentError::LlmError(format!("HTTP request failed: {}", e)))?;

        let status = response.status();
        let text = response.text().await
            .map_err(|e| AgentError::LlmError(format!("Failed to read response body: {}", e)))?;

        if !status.is_success() {
            return Err(AgentError::LlmError(format!(
                "API error ({}): {}", status, text
            )));
        }

        let json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| AgentError::LlmError(format!("Failed to parse response: {}", e)))?;

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| AgentError::LlmError("No content in response".to_string()))?
            .to_string();

        Ok(LlmResponse::success(content))
    }

    fn call_stream(&self, messages: Vec<Message>) -> LlmStream {
        let client = self.client.clone();
        let api_key = self.api_key.clone();
        let model = self.model.clone();
        let base_url = self.base_url.clone();

        Box::pin(async_stream::stream! {
            let url = format!("{}/chat/completions", base_url);
            let body = serde_json::json!({
                "model": model,
                "stream": true,
                "messages": messages.iter().map(|m| {
                    serde_json::json!({
                        "role": m.role.to_string(),
                        "content": m.content,
                    })
                }).collect::<Vec<_>>(),
            });

            let response = client
                .post(&url)
                .header("Authorization", format!("Bearer {}", api_key))
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await
                .map_err(|e| AgentError::LlmError(format!("HTTP request failed: {}", e)));

            let response = match response {
                Ok(r) => r,
                Err(e) => { yield Err(e); return; }
            };

            if !response.status().is_success() {
                let text = response.text().await.unwrap_or_default();
                yield Err(AgentError::LlmError(format!("API error: {}", text)));
                return;
            }

            let mut stream = response.bytes_stream();
            let mut sse_buffer = String::new();

            while let Some(bytes_result) = stream.next().await {
                let bytes = match bytes_result {
                    Ok(b) => b,
                    Err(e) => { yield Err(AgentError::LlmError(format!("Stream error: {}", e))); return; }
                };
                sse_buffer.push_str(&String::from_utf8_lossy(&bytes));

                while let Some(pos) = sse_buffer.find("\n\n") {
                    let frame = sse_buffer[..pos].to_string();
                    sse_buffer = sse_buffer[pos + 2..].to_string();

                    if let Some(chunk) = parse_sse_frame(&frame, &api_key)? {
                        yield Ok(chunk);
                    }
                }
            }
        })
    }
}

pub type DeepSeekProvider = OpenAiCompatibleProvider;

fn parse_sse_frame(frame: &str, _api_key: &str) -> Result<Option<LlmChunk>, AgentError> {
    let data_line = frame.lines()
        .find(|line| line.starts_with("data: "))
        .map(|line| line.trim_start_matches("data: ").trim());

    let data = match data_line {
        Some(d) => d,
        None => return Ok(None),
    };

    if data == "[DONE]" {
        return Ok(Some(LlmChunk {
            delta: String::new(),
            done: true,
            usage: None,
        }));
    }

    let json: serde_json::Value = match serde_json::from_str(data) {
        Ok(j) => j,
        Err(_) => return Ok(None),
    };

    let delta = json["choices"][0]["delta"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();

    let finish_reason = json["choices"][0]["finish_reason"].as_str();
    let done = finish_reason == Some("stop") || finish_reason == Some("end_turn");

    let usage = if done {
        json.get("usage").map(|u| {
            crate::agent::types::TokenUsage {
                prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0) as u32,
                completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0) as u32,
                total_tokens: u["total_tokens"].as_u64().unwrap_or(0) as u32,
            }
        })
    } else {
        None
    };

    Ok(Some(LlmChunk { delta, done, usage }))
}

pub struct MockLlmProvider {
    responses: Vec<String>,
    call_count: Arc<std::sync::atomic::AtomicUsize>,
}

impl MockLlmProvider {
    pub fn new(responses: Vec<String>) -> Self {
        Self {
            responses,
            call_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
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

    fn call_stream(&self, _messages: Vec<Message>) -> LlmStream {
        let responses = self.responses.clone();
        let call_count = self.call_count.clone();

        Box::pin(async_stream::stream! {
            let count = call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let response: String = responses
                .get(count % responses.len())
                .cloned()
                .unwrap_or_else(|| "<final_answer>Mock response</final_answer>".to_string());

            let words: Vec<&str> = response.split_whitespace().collect();
            for (i, word) in words.iter().enumerate() {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                let delta: String = if i == 0 { word.to_string() } else { format!(" {}", word) };
                yield Ok(LlmChunk {
                    delta,
                    done: false,
                    usage: None,
                });
            }
            yield Ok(LlmChunk {
                delta: String::new(),
                done: true,
                usage: None,
            });
        })
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

    #[tokio::test]
    async fn test_mock_llm_provider_stream() {
        use tokio_stream::StreamExt;

        let provider = MockLlmProvider::new(vec![
            "<final_answer>Hello world</final_answer>".to_string(),
        ]);

        let messages = vec![Message::user("Hi")];
        let mut stream = provider.call_stream(messages);

        let mut accumulated = String::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.unwrap();
            accumulated.push_str(&chunk.delta);
            if chunk.done {
                break;
            }
        }

        assert!(accumulated.contains("Hello"));
        assert!(accumulated.contains("world"));
    }
}
