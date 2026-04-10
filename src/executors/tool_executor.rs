//! Tool 步骤执行器

use chrono::Utc;
use serde_json::Value;
use std::sync::Arc;

use crate::core::context::ExecutionContext;
use crate::core::types::*;
use crate::executors::Executor;
use crate::utils::error::WorkflowError;
use crate::agent::{create_tool_handler, BuiltinToolRegistry};

pub struct ToolExecutor {
    builtin_registry: Arc<BuiltinToolRegistry>,
}

impl ToolExecutor {
    pub fn new(builtin_registry: Arc<BuiltinToolRegistry>) -> Self {
        Self { builtin_registry }
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

        let args_template = step.tool_args.as_deref().unwrap_or("{}");
        let template_context = build_template_context(context);
        let args = crate::core::template::TemplateEngine::new()
            .resolve_template(args_template, &template_context)
            .map_err(|e| WorkflowError::Other(format!("Template error: {}", e)))?;

        let result = if let Some(tool_def) = &step.tool {
            let handler = create_tool_handler(tool_def, &self.builtin_registry)?;
            handler.execute(&args).await
        } else if let Some(tool_name) = &step.tool_name {
            return Err(WorkflowError::Other(format!(
                "Tool '{}' not found. Use inline 'tool:' definition instead of 'tool_name'.",
                tool_name
            )));
        } else {
            return Err(WorkflowError::Other(
                "Tool step must have 'tool' (inline definition)".to_string()
            ));
        };

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

    if let Some(loop_vars) = context.variables.get("loop") {
        ctx.insert("loop".to_string(), loop_vars.clone());
    }

    let mut steps = serde_json::Map::new();
    for (step_id, result) in &context.step_outputs {
        if let Some(output) = &result.output {
            steps.insert(step_id.clone(), output.clone());
        }
    }
    ctx.insert("steps".to_string(), Value::Object(steps));

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
    fn build_template_context_includes_step_outputs() {
        let workflow = WorkflowDefinition::default();
        let mut context = ExecutionContext::new(&workflow, HashMap::new());
        context.step_outputs.insert(
            "compose_prompt".to_string(),
            StepResult::success("compose_prompt", json!({ "answer": "<english>prompt</english>" })),
        );

        let template_context = build_template_context(&context);

        assert_eq!(
            template_context.get("steps"),
            Some(&json!({ "compose_prompt": { "answer": "<english>prompt</english>" } }))
        );
    }
}
