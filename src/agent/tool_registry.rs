//! 工具注册器
//! 
//! 翻译自 gtht-agent 的 tool_registry.hpp/cpp

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::agent::types::{ToolDescriptor, ToolHandler, ToolResult};
use tracing::{info, warn};

/// 工具注册器
/// 
/// 核心共享组件，Agent 和 ToolExecutor 都依赖它
pub struct ToolRegistry {
    tools: Arc<RwLock<HashMap<String, ToolDescriptor>>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 注册工具
    pub async fn register(&self, descriptor: ToolDescriptor) {
        let name = descriptor.name.clone();
        info!("[ToolRegistry] Registering tool: {}", name);
        self.tools.write().await.insert(name, descriptor);
    }

    /// 便捷注册方法
    pub async fn register_tool(
        &self,
        name: &str,
        description: &str,
        json_schema: Option<&str>,
        handler: Arc<dyn ToolHandler>,
    ) {
        let descriptor = ToolDescriptor {
            name: name.to_string(),
            description: description.to_string(),
            json_schema: json_schema.map(|s| s.to_string()),
            handler,
        };
        self.register(descriptor).await;
    }

    /// 检查工具是否存在
    pub async fn has_tool(&self, name: &str) -> bool {
        self.tools.read().await.contains_key(name)
    }

    /// 执行工具
    pub async fn execute(&self, name: &str, args: &str) -> ToolResult {
        let tools = self.tools.read().await;
        match tools.get(name) {
            Some(descriptor) => {
                info!(
                    "[ToolRegistry] Executing tool: {}\n  input: {}",
                    name,
                    preview(args),
                );
                let result = descriptor.handler.execute(args).await;
                info!(
                    "[ToolRegistry] Tool finished: {}\n  output: {}\n  is_error: {}",
                    name,
                    preview(&result.content),
                    result.is_error,
                );
                result
            }
            None => {
                warn!("[ToolRegistry] Tool not found: {}", name);
                ToolResult::error(format!("Tool '{}' not found", name))
            }
        }
    }

    /// 获取工具列表描述（用于系统提示）
    pub async fn get_tool_list(&self) -> String {
        let tools = self.tools.read().await;
        let mut result = String::new();
        for (name, desc) in tools.iter() {
            result.push_str(&format!(
                "name: {}\ndescription: {}\nschema: {}\n\n",
                name,
                desc.description,
                desc.json_schema.as_deref().unwrap_or("{}")
            ));
        }
        result
    }

    /// 获取工具描述符
    pub async fn get_descriptor(&self, name: &str) -> Option<ToolDescriptor> {
        self.tools.read().await.get(name).cloned()
    }

    /// 获取工具数量
    pub async fn tool_count(&self) -> usize {
        self.tools.read().await.len()
    }

    /// 获取所有工具名称
    pub async fn tool_names(&self) -> Vec<String> {
        self.tools.read().await.keys().cloned().collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn preview(text: &str) -> String {
    const LIMIT: usize = 500;
    let sanitized = text.replace('\n', "\\n");
    let mut chars = sanitized.chars();
    let preview: String = chars.by_ref().take(LIMIT).collect();
    if chars.next().is_some() {
        format!("{}...(truncated, {} chars total)", preview, sanitized.chars().count())
    } else {
        preview
    }
}

/// 简单闭包工具适配器
pub struct FnTool<F>(pub F);

#[async_trait::async_trait]
impl<F, Fut> ToolHandler for FnTool<F>
where
    F: Fn(String) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = String> + Send,
{
    async fn execute(&self, args: &str) -> ToolResult {
        let result = (self.0)(args.to_string()).await;
        ToolResult::success(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EchoTool;

    #[async_trait::async_trait]
    impl ToolHandler for EchoTool {
        async fn execute(&self, args: &str) -> ToolResult {
            ToolResult::success(format!("Echo: {}", args))
        }
    }

    #[tokio::test]
    async fn test_tool_registry() {
        let registry = ToolRegistry::new();

        registry.register_tool(
            "echo",
            "Echoes the input",
            Some(r#"{"type":"object","properties":{"message":{"type":"string"}}}"#),
            Arc::new(EchoTool),
        ).await;

        assert!(registry.has_tool("echo").await);
        assert!(!registry.has_tool("unknown").await);
        assert_eq!(registry.tool_count().await, 1);

        let result = registry.execute("echo", "Hello").await;
        assert!(!result.is_error);
        assert_eq!(result.content, "Echo: Hello");

        let result = registry.execute("unknown", "args").await;
        assert!(result.is_error);
        assert!(result.content.contains("not found"));
    }

    #[tokio::test]
    async fn test_tool_list() {
        let registry = ToolRegistry::new();

        registry.register_tool(
            "tool1",
            "First tool",
            None,
            Arc::new(EchoTool),
        ).await;

        registry.register_tool(
            "tool2",
            "Second tool",
            Some(r#"{"type":"object"}"#),
            Arc::new(EchoTool),
        ).await;

        let list = registry.get_tool_list().await;
        assert!(list.contains("tool1"));
        assert!(list.contains("tool2"));
        assert!(list.contains("First tool"));
        assert!(list.contains("Second tool"));
    }

    #[tokio::test]
    async fn test_fn_tool() {
        let registry = ToolRegistry::new();

        registry.register_tool(
            "greet",
            "Greets someone",
            None,
            Arc::new(FnTool(|args: String| async move {
                format!("Hello, {}!", args)
            })),
        ).await;

        let result = registry.execute("greet", "World").await;
        assert!(!result.is_error);
        assert_eq!(result.content, "Hello, World!");
    }
}
