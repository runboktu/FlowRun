use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::agent::types::{ToolHandler, ToolResult};
use crate::agent::tool_registry::FnTool;
use crate::utils::error::WorkflowError;

pub struct ShellTool {
    command_template: String,
    timeout: Duration,
}

impl ShellTool {
    pub fn new(command_template: String, timeout_secs: Option<u64>) -> Self {
        Self {
            command_template,
            timeout: Duration::from_secs(timeout_secs.unwrap_or(30)),
        }
    }
}

#[async_trait::async_trait]
impl ToolHandler for ShellTool {
    async fn execute(&self, args: &str) -> ToolResult {
        let command = match render_command_template(&self.command_template, args) {
            Ok(cmd) => cmd,
            Err(e) => return ToolResult::error(e),
        };

        let output = tokio::time::timeout(
            self.timeout,
            tokio::process::Command::new("sh")
                .arg("-c")
                .arg(&command)
                .output(),
        ).await;

        match output {
            Ok(Ok(output)) => {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    ToolResult::success(stdout)
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    ToolResult::error(format!("Exit {}: {}", output.status, stderr))
                }
            }
            Ok(Err(e)) => ToolResult::error(format!("Execution failed: {}", e)),
            Err(_) => ToolResult::error(format!("Timed out after {:?}", self.timeout)),
        }
    }
}

pub struct HttpTool {
    url_template: String,
    method: String,
    headers: HashMap<String, String>,
    body_template: Option<String>,
    timeout: Duration,
}

impl HttpTool {
    pub fn new(
        url_template: String,
        method: Option<String>,
        headers: Option<HashMap<String, String>>,
        body_template: Option<String>,
        timeout_secs: Option<u64>,
    ) -> Self {
        Self {
            url_template,
            method: method.unwrap_or_else(|| "GET".to_string()),
            headers: headers.unwrap_or_default(),
            body_template,
            timeout: Duration::from_secs(timeout_secs.unwrap_or(30)),
        }
    }
}

#[async_trait::async_trait]
impl ToolHandler for HttpTool {
    async fn execute(&self, args: &str) -> ToolResult {
        let params: HashMap<String, serde_json::Value> =
            serde_json::from_str(args).unwrap_or_default();

        let url = render_template(&self.url_template, &params);

        let client = reqwest::Client::new();
        let mut request = match self.method.to_uppercase().as_str() {
            "POST" => client.post(&url),
            "PUT" => client.put(&url),
            "DELETE" => client.delete(&url),
            "PATCH" => client.patch(&url),
            _ => client.get(&url),
        };

        for (key, value) in &self.headers {
            request = request.header(key, value);
        }

        if let Some(body_tpl) = &self.body_template {
            let body = render_template(body_tpl, &params);
            request = request
                .header("Content-Type", "application/json")
                .body(body);
        }

        let result = tokio::time::timeout(self.timeout, request.send()).await;

        match result {
            Ok(Ok(response)) => {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                if status.is_success() {
                    ToolResult::success(body)
                } else {
                    ToolResult::error(format!("HTTP {}: {}", status, body))
                }
            }
            Ok(Err(e)) => ToolResult::error(format!("HTTP request failed: {}", e)),
            Err(_) => ToolResult::error(format!("HTTP request timed out after {:?}", self.timeout)),
        }
    }
}

pub struct PythonTool {
    script: String,
    timeout: Duration,
}

impl PythonTool {
    pub fn new(script: String, timeout_secs: Option<u64>) -> Self {
        Self {
            script,
            timeout: Duration::from_secs(timeout_secs.unwrap_or(60)),
        }
    }
}

#[async_trait::async_trait]
impl ToolHandler for PythonTool {
    async fn execute(&self, args: &str) -> ToolResult {
        let output = tokio::time::timeout(
            self.timeout,
            tokio::process::Command::new("python3")
                .arg("-c")
                .arg(&self.script)
                .arg(args)
                .output(),
        ).await;

        match output {
            Ok(Ok(output)) => {
                if output.status.success() {
                    ToolResult::success(String::from_utf8_lossy(&output.stdout).to_string())
                } else {
                    ToolResult::error(String::from_utf8_lossy(&output.stderr).to_string())
                }
            }
            Ok(Err(e)) => ToolResult::error(format!("Python execution failed: {}", e)),
            Err(_) => ToolResult::error(format!("Python timed out after {:?}", self.timeout)),
        }
    }
}

pub fn create_builtin_tool(name: &str) -> Result<Arc<dyn ToolHandler>, WorkflowError> {
    match name {
        "read_file" => Ok(Arc::new(FnTool(|args: String| async move {
            let parsed: serde_json::Value = serde_json::from_str(&args).unwrap_or_default();
            let path = parsed["path"].as_str().unwrap_or("");
            tokio::fs::read_to_string(path)
                .await
                .unwrap_or_else(|e| format!("Error reading file: {}", e))
        }))),

        "write_file" => Ok(Arc::new(FnTool(|args: String| async move {
            let parsed: serde_json::Value = serde_json::from_str(&args).unwrap_or_default();
            let path = parsed["path"].as_str().unwrap_or("");
            let content = parsed["content"].as_str().unwrap_or("");
            match tokio::fs::write(path, content).await {
                Ok(()) => format!("Successfully wrote to {}", path),
                Err(e) => format!("Error writing file: {}", e),
            }
        }))),

        "list_directory" => Ok(Arc::new(FnTool(|args: String| async move {
            let parsed: serde_json::Value = serde_json::from_str(&args).unwrap_or_default();
            let path = parsed["path"].as_str().unwrap_or(".");
            match tokio::fs::read_dir(path).await {
                Ok(mut dir) => {
                    let mut entries = Vec::new();
                    while let Ok(Some(entry)) = dir.next_entry().await {
                        entries.push(entry.file_name().to_string_lossy().to_string());
                    }
                    entries.join("\n")
                }
                Err(e) => format!("Error listing directory: {}", e)
            }
        }))),

        "http_get" => Ok(Arc::new(FnTool(|args: String| async move {
            let parsed: serde_json::Value = serde_json::from_str(&args).unwrap_or_default();
            let url = parsed["url"].as_str().unwrap_or("");
            match reqwest::get(url).await {
                Ok(r) => r.text().await.unwrap_or_default(),
                Err(e) => format!("HTTP error: {}", e),
            }
        }))),

        _ => Err(WorkflowError::Other(format!(
            "Unknown builtin tool: '{}'. Available: read_file, write_file, list_directory, http_get",
            name
        ))),
    }
}

fn render_command_template(template: &str, args_json: &str) -> Result<String, String> {
    let args: HashMap<String, serde_json::Value> =
        serde_json::from_str(args_json)
            .map_err(|e| format!("Invalid JSON args: {}", e))?;

    let mut result = template.to_string();
    for (key, value) in &args {
        let str_val = match value {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        result = result.replace(&format!("{{{{{}}}}}", key), &str_val);
    }
    Ok(result)
}

fn render_template(template: &str, params: &HashMap<String, serde_json::Value>) -> String {
    let mut result = template.to_string();
    for (key, value) in params {
        let str_val = match value {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        result = result.replace(&format!("{{{{{}}}}}", key), &str_val);
    }
    result
}
