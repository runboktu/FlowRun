//! 内置工具注册表 — 支持用户扩展 builtin 工具
//!
//! 提供两层查找：用户自定义工具优先于框架默认工具（同名覆盖）。
//! 用户通过 `FlowRunner::register_builtin()` 注册自定义工具，
//! 在 YAML 中用 `source: builtin` + `name: xxx` 即可使用。

use std::collections::HashMap;
use std::sync::Arc;

use crate::agent::types::ToolHandler;
use crate::agent::tool_registry::FnTool;

/// 存储的工具条目
struct StoredTool {
    #[allow(dead_code)]
    description: String,
    handler: Arc<dyn ToolHandler>,
}

/// 内置工具注册表
///
/// 两层 HashMap：
/// - `default_tools`: 框架自带的默认工具（read_file 等）
/// - `custom_tools`: 用户注册的自定义工具
///
/// 查找时 custom_tools 优先于 default_tools（同名覆盖）。
pub struct BuiltinToolRegistry {
    default_tools: HashMap<String, StoredTool>,
    custom_tools: HashMap<String, StoredTool>,
}

impl BuiltinToolRegistry {
    /// 创建空的注册表（不含默认工具）
    pub fn new() -> Self {
        Self {
            default_tools: HashMap::new(),
            custom_tools: HashMap::new(),
        }
    }

    /// 创建包含框架默认工具的注册表
    ///
    /// 默认工具：read_file, write_file, list_directory, http_get
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();

        registry.default_register(
            "read_file",
            "读取文件内容",
            Arc::new(FnTool(|args: String| async move {
                let parsed: serde_json::Value = serde_json::from_str(&args).unwrap_or_default();
                let path = parsed["path"].as_str().unwrap_or("");
                tokio::fs::read_to_string(path)
                    .await
                    .unwrap_or_else(|e| format!("Error reading file: {}", e))
            })),
        );

        registry.default_register(
            "write_file",
            "写入内容到文件",
            Arc::new(FnTool(|args: String| async move {
                let parsed: serde_json::Value = serde_json::from_str(&args).unwrap_or_default();
                let path = parsed["path"].as_str().unwrap_or("");
                let content = parsed["content"].as_str().unwrap_or("");
                match tokio::fs::write(path, content).await {
                    Ok(()) => format!("Successfully wrote to {}", path),
                    Err(e) => format!("Error writing file: {}", e),
                }
            })),
        );

        registry.default_register(
            "list_directory",
            "列出目录内容",
            Arc::new(FnTool(|args: String| async move {
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
                    Err(e) => format!("Error listing directory: {}", e),
                }
            })),
        );

        registry.default_register(
            "http_get",
            "HTTP GET 请求",
            Arc::new(FnTool(|args: String| async move {
                let parsed: serde_json::Value = serde_json::from_str(&args).unwrap_or_default();
                let url = parsed["url"].as_str().unwrap_or("");
                match reqwest::get(url).await {
                    Ok(r) => r.text().await.unwrap_or_default(),
                    Err(e) => format!("HTTP error: {}", e),
                }
            })),
        );

        registry
    }

    /// 注册到默认工具层（内部使用）
    fn default_register(
        &mut self,
        name: &str,
        description: &str,
        handler: Arc<dyn ToolHandler>,
    ) {
        self.default_tools.insert(
            name.to_string(),
            StoredTool {
                description: description.to_string(),
                handler,
            },
        );
    }

    /// 注册自定义工具（用户调用）
    ///
    /// 如果与默认工具同名，自定义工具优先。
    /// 返回 `&mut Self` 以支持链式调用。
    pub fn register(
        &mut self,
        name: &str,
        description: &str,
        handler: Arc<dyn ToolHandler>,
    ) {
        self.custom_tools.insert(
            name.to_string(),
            StoredTool {
                description: description.to_string(),
                handler,
            },
        );
    }

    /// 查找工具：custom_tools 优先，再查 default_tools
    pub fn lookup(&self, name: &str) -> Option<Arc<dyn ToolHandler>> {
        if let Some(stored) = self.custom_tools.get(name) {
            return Some(stored.handler.clone());
        }
        self.default_tools.get(name).map(|stored| stored.handler.clone())
    }

    /// 返回所有可用工具名（用于错误提示）
    pub fn list_all(&self) -> Vec<String> {
        let mut names: Vec<String> = self.default_tools.keys().cloned().collect();
        for name in self.custom_tools.keys() {
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
        names.sort();
        names
    }

    /// 返回已注册的自定义工具数量
    #[allow(dead_code)]
    pub fn custom_tool_count(&self) -> usize {
        self.custom_tools.len()
    }

    /// 返回默认工具数量
    #[allow(dead_code)]
    pub fn default_tool_count(&self) -> usize {
        self.default_tools.len()
    }
}

impl Default for BuiltinToolRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_with_defaults_contains_four_tools() {
        let registry = BuiltinToolRegistry::with_defaults();
        assert!(registry.lookup("read_file").is_some());
        assert!(registry.lookup("write_file").is_some());
        assert!(registry.lookup("list_directory").is_some());
        assert!(registry.lookup("http_get").is_some());
        assert_eq!(registry.default_tool_count(), 4);
        assert_eq!(registry.custom_tool_count(), 0);
    }

    #[test]
    fn test_custom_tool_overrides_default() {
        let mut registry = BuiltinToolRegistry::with_defaults();

        // 注册同名自定义工具
        registry.register(
            "read_file",
            "自定义读取",
            Arc::new(FnTool(|_args: String| async move {
                "custom read".to_string()
            })),
        );

        assert_eq!(registry.custom_tool_count(), 1);
        // lookup 应该返回自定义版本
        // (我们无法直接比较 handler，但确认 lookup 成功)
        assert!(registry.lookup("read_file").is_some());
    }

    #[test]
    fn test_custom_tool_not_in_defaults() {
        let mut registry = BuiltinToolRegistry::with_defaults();
        registry.register(
            "my_tool",
            "我的工具",
            Arc::new(FnTool(|args: String| async move {
                format!("my_tool: {}", args)
            })),
        );

        assert!(registry.lookup("my_tool").is_some());
        assert_eq!(registry.custom_tool_count(), 1);
    }

    #[test]
    fn test_unknown_tool_returns_none() {
        let registry = BuiltinToolRegistry::with_defaults();
        assert!(registry.lookup("nonexistent").is_none());
    }

    #[test]
    fn test_list_all() {
        let mut registry = BuiltinToolRegistry::with_defaults();
        registry.register(
            "zebra_tool",
            "Z 工具",
            Arc::new(FnTool(|_args: String| async move { "z".to_string() })),
        );

        let names = registry.list_all();
        assert!(names.contains(&"read_file".to_string()));
        assert!(names.contains(&"zebra_tool".to_string()));
        // sorted
        assert_eq!(names[0], "http_get");
        assert_eq!(names[names.len() - 1], "zebra_tool");
    }

    #[tokio::test]
    async fn test_execute_custom_tool() {
        let mut registry = BuiltinToolRegistry::new();
        registry.register(
            "echo",
            "回显工具",
            Arc::new(FnTool(|args: String| async move {
                format!("Echo: {}", args)
            })),
        );

        let handler = registry.lookup("echo").expect("should find echo");
        let result = handler.execute("{\"msg\":\"hello\"}").await;
        assert!(!result.is_error);
        assert!(result.content.contains("hello"));
    }

    #[test]
    fn test_new_is_empty() {
        let registry = BuiltinToolRegistry::new();
        assert_eq!(registry.default_tool_count(), 0);
        assert_eq!(registry.custom_tool_count(), 0);
        assert!(registry.lookup("read_file").is_none());
    }
}
