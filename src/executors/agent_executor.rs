//! Agent 步骤执行器

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use chrono::Utc;

use crate::core::context::ExecutionContext;
use crate::core::types::*;
use crate::executors::Executor;
use crate::utils::error::WorkflowError;
use crate::agent::{AgentManager, AgentCallback, AgentStatus, LlmProviderConfig, create_llm_provider};

/// Agent 步骤执行器
pub struct AgentExecutor {
    agent_manager: Arc<AgentManager>,
}

impl AgentExecutor {
    pub fn new(agent_manager: Arc<AgentManager>) -> Self {
        Self { agent_manager }
    }
}

#[async_trait::async_trait]
impl Executor for AgentExecutor {
    async fn execute(
        &self,
        step: &StepDefinition,
        context: &ExecutionContext,
    ) -> Result<StepResult, WorkflowError> {
        let started_at = Utc::now();
        let step_id = &step.id;

        let agent_system_prompt = step.agent_system_prompt.as_deref();

        let llm_config = parse_llm_provider_config(step, context)?;
        let llm = create_llm_provider(&llm_config)
            .map_err(|e| WorkflowError::Other(format!("Failed to create LLM provider: {}", e)))?;

        let session_id = self.agent_manager
            .create_session_with_llm(llm, agent_system_prompt)
            .await
            .map_err(|e| WorkflowError::Other(format!("Failed to create session: {}", e)))?;

        let input_template = step.agent_input.as_deref()
            .ok_or_else(|| WorkflowError::Other("Missing agent_input".to_string()))?;

        let template_context = build_template_context(context);
        let input = crate::core::template::TemplateEngine::new()
            .resolve_template(input_template, &template_context)
            .map_err(|e| WorkflowError::Other(format!("Template error: {}", e)))?;

        if let Some(max_iter) = step.agent_max_iterations {
            self.agent_manager.set_max_iterations(&session_id, max_iter).await
                .map_err(|e| WorkflowError::Other(format!("Failed to set max iterations: {}", e)))?;
        }

        let use_stream = step.agent_stream.unwrap_or(false);
        let result = if use_stream {
            let callback = build_stream_callback();
            self.agent_manager
                .run_sync_stream(&session_id, &input, callback)
                .await
                .map_err(|e| WorkflowError::Other(format!("Agent stream execution failed: {}", e)))?
        } else {
            self.agent_manager
                .run_sync(&session_id, &input, None)
                .await
                .map_err(|e| WorkflowError::Other(format!("Agent execution failed: {}", e)))?
        };

        self.agent_manager.destroy_session(&session_id).await;

        let completed_at = Utc::now();
        let duration_ms = (completed_at - started_at).num_milliseconds() as u64;

        Ok(StepResult::success(
            step_id.clone(),
            serde_json::json!({ "answer": result }),
        ).with_timing(started_at, duration_ms))
    }
}

fn build_stream_callback() -> AgentCallback {
    let first_chunk = Arc::new(AtomicBool::new(true));
    Arc::new(move |data: String, status: AgentStatus| {
        if status == AgentStatus::LlmChunk {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&data) {
                if let Some(delta) = json.get("delta").and_then(|v| v.as_str()) {
                    if !delta.is_empty() {
                        if first_chunk.load(Ordering::SeqCst) {
                            first_chunk.store(false, Ordering::SeqCst);
                            eprintln!("\n    [Agent Stream]");
                        }
                        eprint!("{}", delta);
                        let _ = std::io::Write::flush(&mut std::io::stderr());
                    }
                }
            }
        } else if status == AgentStatus::LlmResponse {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&data) {
                if let Some(usage) = json.get("token_usage") {
                    eprintln!("\n    [Token Usage] prompt={}, completion={}, total={}",
                        usage.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                        usage.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                        usage.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                    );
                }
            }
        }
    })
}

fn parse_llm_provider_config(
    _step: &StepDefinition,
    _context: &ExecutionContext,
) -> Result<LlmProviderConfig, WorkflowError> {
    let api_key = std::env::var("DEEPSEEK_API_KEY")
        .or_else(|_| std::env::var("OPENAI_API_KEY"))
        .map_err(|_| WorkflowError::Other(
            "DEEPSEEK_API_KEY or OPENAI_API_KEY environment variable not set".to_string()
        ))?;

    let provider_type = std::env::var("LLM_PROVIDER")
        .unwrap_or_else(|_| "deepseek".to_string());

    let model = std::env::var("LLM_MODEL").ok();
    let base_url = std::env::var("LLM_BASE_URL").ok();

    Ok(LlmProviderConfig {
        r#type: provider_type,
        model,
        api_key,
        base_url,
    })
}

fn build_template_context(context: &ExecutionContext) -> std::collections::HashMap<String, serde_json::Value> {
    let mut ctx = std::collections::HashMap::new();
    ctx.insert("inputs".to_string(), serde_json::to_value(&context.inputs).unwrap_or_default());
    ctx.insert("variables".to_string(), serde_json::to_value(&context.variables).unwrap_or_default());
    ctx
}
