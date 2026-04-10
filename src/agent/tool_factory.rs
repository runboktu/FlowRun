use std::sync::Arc;

use crate::agent::builtin_registry::BuiltinToolRegistry;
use crate::agent::tool_implementations::{HttpTool, PythonTool, ShellTool};
use crate::agent::types::ToolHandler;
use crate::core::types::{ToolSourceDefinition, ToolSourceType};
use crate::utils::error::WorkflowError;

pub fn create_tool_handler(
    def: &ToolSourceDefinition,
    registry: &BuiltinToolRegistry,
) -> Result<Arc<dyn ToolHandler>, WorkflowError> {
    match &def.source {
        ToolSourceType::Builtin => registry.lookup(&def.name).ok_or_else(|| {
            WorkflowError::Other(format!(
                "Unknown builtin tool: '{}'. Available: {}",
                def.name,
                registry.list_all().join(", ")
            ))
        }),

        ToolSourceType::Shell => {
            let command = def.command.clone().ok_or_else(|| {
                WorkflowError::Other(format!(
                    "Shell tool '{}' requires 'command' field",
                    def.name
                ))
            })?;
            Ok(Arc::new(ShellTool::new(command, def.timeout_secs)))
        }

        ToolSourceType::Http => {
            let url = def.url.clone().ok_or_else(|| {
                WorkflowError::Other(format!("HTTP tool '{}' requires 'url' field", def.name))
            })?;
            Ok(Arc::new(HttpTool::new(
                url,
                def.method.clone(),
                def.headers.clone(),
                def.body_template.clone(),
                def.timeout_secs,
            )))
        }

        ToolSourceType::Python => {
            let script = def.script.clone().ok_or_else(|| {
                WorkflowError::Other(format!(
                    "Python tool '{}' requires 'script' field",
                    def.name
                ))
            })?;
            Ok(Arc::new(PythonTool::new(script, def.timeout_secs)))
        }
    }
}
