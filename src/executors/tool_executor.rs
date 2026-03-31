//! Tool 步骤执行器

use std::sync::Arc;
use chrono::Utc;
use serde_json::Value;

use crate::core::context::ExecutionContext;
use crate::core::types::*;
use crate::executors::Executor;
use crate::utils::error::WorkflowError;
use crate::agent::ToolRegistry;

/// Tool 步骤执行器
pub struct ToolExecutor {
    tool_registry: Arc<ToolRegistry>,
}

impl ToolExecutor {
    pub fn new(tool_registry: Arc<ToolRegistry>) -> Self {
        Self { tool_registry }
    }
}

#[async_trait::async_trait]
impl Executor for ToolExecutor {
    async fn execute(
        &self,
        step: &StepDefinition,
        context: &ExecutionContext,
    ) -> Result<StepResult, WorkflowError> {
        let started_at = Utc::now();
        let step_id = &step.id;

        let tool_name = step.tool_name.as_deref()
            .ok_or_else(|| WorkflowError::Other("Missing tool_name".to_string()))?;

        let args_template = step.tool_args.as_deref().unwrap_or("{}");
        let template_context = build_template_context(context);
        let args = crate::core::template::TemplateEngine::new()
            .resolve_template(args_template, &template_context)
            .map_err(|e| WorkflowError::Other(format!("Template error: {}", e)))?;

        if !self.tool_registry.has_tool(tool_name).await {
            return Err(WorkflowError::Other(format!(
                "Tool '{}' not found in registry",
                tool_name
            )));
        }

        let result = self.tool_registry.execute(tool_name, &args).await;

        let completed_at = Utc::now();
        let duration_ms = (completed_at - started_at).num_milliseconds() as u64;

        if result.is_error {
            Ok(StepResult::failed(
                step_id.clone(),
                StepError {
                    code: "TOOL_ERROR".to_string(),
                    message: result.content,
                    fix: None,
                },
            ))
        } else {
            let output: Value = serde_json::from_str(&result.content)
                .unwrap_or_else(|_| Value::String(result.content));

            Ok(StepResult::success(step_id.clone(), output)
                .with_timing(started_at, duration_ms))
        }
    }
}

fn build_template_context(context: &ExecutionContext) -> std::collections::HashMap<String, Value> {
    let mut ctx = std::collections::HashMap::new();
    ctx.insert("inputs".to_string(), serde_json::to_value(&context.inputs).unwrap_or_default());
    ctx.insert("variables".to_string(), serde_json::to_value(&context.variables).unwrap_or_default());
    ctx
}
