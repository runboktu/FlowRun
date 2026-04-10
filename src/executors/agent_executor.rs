//! Agent 步骤执行器

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use chrono::Utc;

use crate::core::context::ExecutionContext;
use crate::core::types::*;
use crate::executors::Executor;
use crate::utils::error::WorkflowError;
use crate::agent::{AgentManager, AgentCallback, AgentStatus, LlmProviderConfig, create_llm_provider};
use crate::agent::types::ToolDescriptor;
use crate::agent::create_tool_handler;

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

        if let Some(tool_defs) = &step.agent_tools {
            for tool_def in tool_defs {
                let handler = create_tool_handler(tool_def)?;
                let descriptor = ToolDescriptor {
                    name: tool_def.name.clone(),
                    description: tool_def.description.clone().unwrap_or_default(),
                    json_schema: tool_def.json_schema.clone(),
                    handler,
                };
                self.agent_manager
                    .register_tool(&session_id, descriptor)
                    .await
                    .map_err(|e| WorkflowError::Other(
                        format!("Failed to register tool '{}': {}", tool_def.name, e)
                    ))?;
            }
            tracing::info!(
                "[AgentExecutor] Registered {} tools for agent step '{}'",
                tool_defs.len(), step_id
            );
        }

        let input = resolve_agent_input(step, context)?;

        if let Some(max_iter) = step.agent_max_iterations {
            self.agent_manager.set_max_iterations(&session_id, max_iter).await
                .map_err(|e| WorkflowError::Other(format!("Failed to set max iterations: {}", e)))?;
        }

        let use_stream = step.agent_stream.unwrap_or(false);
        let result = if use_stream {
            let callback = build_stream_callback(step_id.clone());
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

fn resolve_agent_input(
    step: &StepDefinition,
    context: &ExecutionContext,
) -> Result<String, WorkflowError> {
    let input_template = step.agent_input.as_deref()
        .ok_or_else(|| WorkflowError::Other("Missing agent_input".to_string()))?;
    let template_context = build_template_context(context);
    crate::core::template::TemplateEngine::new()
        .resolve_template(input_template, &template_context)
        .map_err(|e| WorkflowError::Other(format!("Template error: {}", e)))
}

fn build_stream_callback(step_id: String) -> AgentCallback {
    let first_chunk = Arc::new(AtomicBool::new(true));
    Arc::new(move |data: String, status: AgentStatus| {
        match status {
            AgentStatus::IterationStart => {
                eprintln!("\n    [Agent Stream:{}] starting", step_id);
            }
            AgentStatus::LlmCall => {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&data) {
                    let iteration = json.get("iteration").and_then(|v| v.as_u64()).unwrap_or(0);
                    eprintln!("    [Agent Stream:{}] iteration {} llm_call", step_id, iteration);
                }
            }
            AgentStatus::LlmChunk => {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&data) {
                    if let Some(delta) = json.get("delta").and_then(|v| v.as_str()) {
                        if !delta.is_empty() {
                            if first_chunk.load(Ordering::SeqCst) {
                                first_chunk.store(false, Ordering::SeqCst);
                                eprintln!("    [Agent Stream:{}] output:", step_id);
                            }
                            eprint!("{}", delta);
                            let _ = std::io::Write::flush(&mut std::io::stderr());
                        }
                    }
                }
            }
            AgentStatus::ToolCall => {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&data) {
                    let tool_name = json.get("tool_name").and_then(|v| v.as_str()).unwrap_or("unknown");
                    eprintln!("\n    [Agent Stream:{}] tool_call {}", step_id, tool_name);
                }
            }
            AgentStatus::ToolResult => {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&data) {
                    let is_error = json.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false);
                    eprintln!("    [Agent Stream:{}] tool_result is_error={}", step_id, is_error);
                }
            }
            AgentStatus::LlmResponse => {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&data) {
                    let prompt = json.get("prompt_tokens").and_then(|v| v.as_u64());
                    let completion = json.get("completion_tokens").and_then(|v| v.as_u64());
                    let total = json.get("total_tokens").and_then(|v| v.as_u64());
                    if let (Some(prompt), Some(completion), Some(total)) = (prompt, completion, total) {
                        eprintln!(
                            "\n    [Agent Stream:{}] token_usage prompt={}, completion={}, total={}",
                            step_id, prompt, completion, total
                        );
                    }
                }
            }
            AgentStatus::IterationEnd => {
                eprintln!("\n    [Agent Stream:{}] finished", step_id);
            }
            AgentStatus::Retry | AgentStatus::Unknown => {}
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

    if let Some(loop_vars) = context.variables.get("loop") {
        ctx.insert("loop".to_string(), loop_vars.clone());
    }

    let mut steps = serde_json::Map::new();
    for (step_id, result) in &context.step_outputs {
        if let Some(output) = &result.output {
            steps.insert(step_id.clone(), output.clone());
        }
    }
    ctx.insert("steps".to_string(), serde_json::Value::Object(steps));

    ctx
}

#[cfg(test)]
mod tests {
    use super::build_template_context;
    use crate::core::context::ExecutionContext;
    use crate::core::types::{StepResult, WorkflowDefinition};
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn build_template_context_includes_steps_and_loop() {
        let workflow = WorkflowDefinition::default();
        let mut context = ExecutionContext::new(&workflow, HashMap::new());
        context.variables.insert(
            "loop".to_string(),
            json!({ "current": 3, "index": 0 }),
        );
        context.step_outputs.insert(
            "tag_lyrics".to_string(),
            StepResult::success("tag_lyrics", json!({ "answer": "tagged" })),
        );

        let template_context = build_template_context(&context);

        assert_eq!(template_context.get("loop"), Some(&json!({ "current": 3, "index": 0 })));
        assert_eq!(
            template_context.get("steps"),
            Some(&json!({ "tag_lyrics": { "answer": "tagged" } }))
        );
    }
}
