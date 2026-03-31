//! Agent 步骤执行器

use std::sync::Arc;
use chrono::Utc;

use crate::core::context::ExecutionContext;
use crate::core::types::*;
use crate::executors::Executor;
use crate::utils::error::WorkflowError;
use crate::agent::AgentManager;

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
        let session_id = self.agent_manager
            .create_session(agent_system_prompt)
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

        let result = self.agent_manager
            .run_sync(&session_id, &input, None)
            .await
            .map_err(|e| WorkflowError::Other(format!("Agent execution failed: {}", e)))?;

        self.agent_manager.destroy_session(&session_id).await;

        let completed_at = Utc::now();
        let duration_ms = (completed_at - started_at).num_milliseconds() as u64;

        Ok(StepResult::success(
            step_id.clone(),
            serde_json::json!({ "answer": result }),
        ).with_timing(started_at, duration_ms))
    }
}

fn build_template_context(context: &ExecutionContext) -> std::collections::HashMap<String, serde_json::Value> {
    let mut ctx = std::collections::HashMap::new();
    ctx.insert("inputs".to_string(), serde_json::to_value(&context.inputs).unwrap_or_default());
    ctx.insert("variables".to_string(), serde_json::to_value(&context.variables).unwrap_or_default());
    ctx
}
