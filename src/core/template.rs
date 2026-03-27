//! 模板表达式引擎
//!
//! 支持 ${{...}} 语法的模板解析，包括：
//! - 嵌套路径访问 (如 steps.deploy.response.body.url)
//! - 数组索引访问 (如 items[0].name)
//! - 过滤器链 (如 ${{ steps.data | uppercase | truncate(10) }})
//! - 条件表达式 (如 ${{ inputs.env || "staging" }})

use crate::utils::error::TemplateError;
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;

/// 模板引擎，负责解析和求值模板表达式
pub struct TemplateEngine {
    /// 模板表达式正则: ${{ ... }}
    template_regex: Regex,
    /// 过滤器注册表
    filters: HashMap<String, FilterFunc>,
}

/// 过滤器函数类型
type FilterFunc = Box<dyn Fn(&Value, &[&str]) -> Result<Value, TemplateError> + Send + Sync>;

impl TemplateEngine {
    /// 创建新的模板引擎实例
    pub fn new() -> Self {
        let template_regex = Regex::new(r"\$\{\{([^}]+)\}\}").expect("模板正则表达式编译失败");

        let mut engine = Self {
            template_regex,
            filters: HashMap::new(),
        };

        // 注册内置过滤器
        engine.register_builtin_filters();
        engine
    }

    /// 注册内置过滤器
    fn register_builtin_filters(&mut self) {
        // 大小写转换
        self.filters.insert(
            "uppercase".to_string(),
            Box::new(|val, _| {
                Ok(Value::String(
                    val.as_str()
                        .ok_or_else(|| {
                            TemplateError::TypeError("uppercase 需要字符串".to_string())
                        })?
                        .to_uppercase(),
                ))
            }),
        );

        self.filters.insert(
            "lowercase".to_string(),
            Box::new(|val, _| {
                Ok(Value::String(
                    val.as_str()
                        .ok_or_else(|| {
                            TemplateError::TypeError("lowercase 需要字符串".to_string())
                        })?
                        .to_lowercase(),
                ))
            }),
        );

        self.filters.insert(
            "capitalize".to_string(),
            Box::new(|val, _| {
                let s = val
                    .as_str()
                    .ok_or_else(|| TemplateError::TypeError("capitalize 需要字符串".to_string()))?;
                let mut chars = s.chars();
                let capitalized = match chars.next() {
                    None => String::new(),
                    Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                };
                Ok(Value::String(capitalized))
            }),
        );

        // 去除首尾空格
        self.filters.insert(
            "trim".to_string(),
            Box::new(|val, _| {
                Ok(Value::String(
                    val.as_str()
                        .ok_or_else(|| TemplateError::TypeError("trim 需要字符串".to_string()))?
                        .trim()
                        .to_string(),
                ))
            }),
        );

        // 默认值
        self.filters.insert(
            "default".to_string(),
            Box::new(|val, args| {
                if val.is_null() || val.as_str() == Some("") {
                    let default_val = args.first().ok_or_else(|| {
                        TemplateError::TypeError("default 需要默认值参数".to_string())
                    })?;
                    Ok(Value::String(default_val.to_string()))
                } else {
                    Ok(val.clone())
                }
            }),
        );

        // JSON 序列化
        self.filters.insert(
            "to_json".to_string(),
            Box::new(|val, _| {
                serde_json::to_string(val)
                    .map(Value::String)
                    .map_err(|e| TemplateError::TypeError(format!("to_json 失败: {}", e)))
            }),
        );

        // JSON 反序列化
        self.filters.insert(
            "from_json".to_string(),
            Box::new(|val, _| {
                let s = val
                    .as_str()
                    .ok_or_else(|| TemplateError::TypeError("from_json 需要字符串".to_string()))?;
                serde_json::from_str(s)
                    .map_err(|e| TemplateError::TypeError(format!("from_json 失败: {}", e)))
            }),
        );

        // 长度
        self.filters.insert(
            "length".to_string(),
            Box::new(|val, _| {
                let len = if let Some(arr) = val.as_array() {
                    arr.len()
                } else if let Some(s) = val.as_str() {
                    s.len()
                } else {
                    return Err(TemplateError::TypeError(
                        "length 需要数组或字符串".to_string(),
                    ));
                };
                Ok(Value::Number(serde_json::Number::from(len)))
            }),
        );

        // 数组切片
        self.filters.insert(
            "slice".to_string(),
            Box::new(|val, args| {
                let arr = val
                    .as_array()
                    .ok_or_else(|| TemplateError::TypeError("slice 需要数组".to_string()))?;
                let start: usize = args.first().and_then(|s| s.parse().ok()).unwrap_or(0);
                let end: usize = args
                    .get(1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(arr.len());
                let sliced: Vec<Value> = arr[start..end.min(arr.len())].to_vec();
                Ok(Value::Array(sliced))
            }),
        );

        // 取第一个元素
        self.filters.insert(
            "first".to_string(),
            Box::new(|val, _| {
                val.as_array()
                    .and_then(|arr| arr.first())
                    .cloned()
                    .ok_or_else(|| TemplateError::TypeError("first 需要非空数组".to_string()))
            }),
        );

        // 取最后一个元素
        self.filters.insert(
            "last".to_string(),
            Box::new(|val, _| {
                val.as_array()
                    .and_then(|arr| arr.last())
                    .cloned()
                    .ok_or_else(|| TemplateError::TypeError("last 需要非空数组".to_string()))
            }),
        );

        // 数组拼接字符串
        self.filters.insert(
            "join".to_string(),
            Box::new(|val, args| {
                let arr = val
                    .as_array()
                    .ok_or_else(|| TemplateError::TypeError("join 需要数组".to_string()))?;
                let sep = args.first().copied().unwrap_or(",");
                let strings: Vec<String> = arr
                    .iter()
                    .map(|v| match v {
                        Value::String(s) => s.clone(),
                        _ => v.to_string(),
                    })
                    .collect();
                Ok(Value::String(strings.join(sep)))
            }),
        );

        // 字符串分割数组
        self.filters.insert(
            "split".to_string(),
            Box::new(|val, args| {
                let s = val
                    .as_str()
                    .ok_or_else(|| TemplateError::TypeError("split 需要字符串".to_string()))?;
                let sep = args.first().copied().unwrap_or(",");
                let parts: Vec<Value> =
                    s.split(sep).map(|p| Value::String(p.to_string())).collect();
                Ok(Value::Array(parts))
            }),
        );

        // 字符串替换
        self.filters.insert(
            "replace".to_string(),
            Box::new(|val, args| {
                let s = val
                    .as_str()
                    .ok_or_else(|| TemplateError::TypeError("replace 需要字符串".to_string()))?;
                let old = args.first().ok_or_else(|| {
                    TemplateError::TypeError("replace 需要旧字符串参数".to_string())
                })?;
                let new = args.get(1).copied().unwrap_or("");
                Ok(Value::String(s.replace(old, new)))
            }),
        );

        // 正则提取
        self.filters.insert(
            "regex_extract".to_string(),
            Box::new(|val, args| {
                let s = val.as_str().ok_or_else(|| {
                    TemplateError::TypeError("regex_extract 需要字符串".to_string())
                })?;
                let pattern = args.first().ok_or_else(|| {
                    TemplateError::TypeError("regex_extract 需要正则参数".to_string())
                })?;
                let re = Regex::new(pattern)
                    .map_err(|e| TemplateError::SyntaxError(format!("正则错误: {}", e)))?;
                match re.captures(s) {
                    Some(caps) => Ok(Value::String(
                        caps.get(1)
                            .or_else(|| caps.get(0))
                            .map(|m| m.as_str().to_string())
                            .unwrap_or_default(),
                    )),
                    None => Ok(Value::Null),
                }
            }),
        );

        // 截断字符串
        self.filters.insert(
            "truncate".to_string(),
            Box::new(|val, args| {
                let s = val
                    .as_str()
                    .ok_or_else(|| TemplateError::TypeError("truncate 需要字符串".to_string()))?;
                let n: usize = args.first().and_then(|s| s.parse().ok()).unwrap_or(100);
                let truncated = if s.len() > n {
                    format!("{}...", &s[..n])
                } else {
                    s.to_string()
                };
                Ok(Value::String(truncated))
            }),
        );

        // Base64 编码
        self.filters.insert(
            "base64_encode".to_string(),
            Box::new(|val, _| {
                let s = val.as_str().ok_or_else(|| {
                    TemplateError::TypeError("base64_encode 需要字符串".to_string())
                })?;
                Ok(Value::String(base64_encode(s)))
            }),
        );

        // Base64 解码
        self.filters.insert(
            "base64_decode".to_string(),
            Box::new(|val, _| {
                let s = val.as_str().ok_or_else(|| {
                    TemplateError::TypeError("base64_decode 需要字符串".to_string())
                })?;
                base64_decode(s)
                    .map(Value::String)
                    .map_err(|e| TemplateError::TypeError(format!("base64_decode 失败: {}", e)))
            }),
        );

        // 格式化时间戳
        self.filters.insert(
            "format_timestamp".to_string(),
            Box::new(|val, _| {
                let ts = val.as_i64().ok_or_else(|| {
                    TemplateError::TypeError("format_timestamp 需要数字".to_string())
                })?;
                let datetime = chrono::DateTime::from_timestamp(ts, 0)
                    .ok_or_else(|| TemplateError::TypeError("无效时间戳".to_string()))?;
                Ok(Value::String(
                    datetime.format("%Y-%m-%d %H:%M:%S").to_string(),
                ))
            }),
        );

        // 格式化毫秒
        self.filters.insert(
            "format_duration".to_string(),
            Box::new(|val, _| {
                let ms = val.as_u64().ok_or_else(|| {
                    TemplateError::TypeError("format_duration 需要数字".to_string())
                })?;
                let seconds = ms / 1000;
                let minutes = seconds / 60;
                let hours = minutes / 60;
                let formatted = if hours > 0 {
                    format!("{}h {}m {}s", hours, minutes % 60, seconds % 60)
                } else if minutes > 0 {
                    format!("{}m {}s", minutes, seconds % 60)
                } else {
                    format!("{}s", seconds)
                };
                Ok(Value::String(formatted))
            }),
        );
    }

    /// 解析模板表达式，替换所有 ${{...}} 为实际值
    ///
    /// # 参数
    ///
    /// * `template` - 包含模板表达式的字符串
    /// * `context` - 变量上下文
    ///
    /// # 返回
    ///
    /// 返回替换后的字符串
    pub fn resolve_template(
        &self,
        template: &str,
        context: &HashMap<String, Value>,
    ) -> Result<String, TemplateError> {
        let mut result = template.to_string();

        for cap in self.template_regex.captures_iter(template) {
            let full_match = &cap[0];
            let expression = cap[1].trim();
            let value = self.evaluate(expression, context)?;
            let replacement = match &value {
                Value::String(s) => s.clone(),
                _ => value.to_string(),
            };
            result = result.replace(full_match, &replacement);
        }

        Ok(result)
    }

    /// 求值单个表达式
    ///
    /// 支持：
    /// - 路径访问: steps.deploy.response.body
    /// - 数组索引: items[0].name
    /// - 过滤器: expression | filter1 | filter2(arg)
    /// - 条件表达式: expression || default_value
    /// - 布尔比较: expression == value
    ///
    /// # 参数
    ///
    /// * `expression` - 表达式字符串
    /// * `context` - 变量上下文
    ///
    /// # 返回
    ///
    /// 返回求值结果
    pub fn evaluate(
        &self,
        expression: &str,
        context: &HashMap<String, Value>,
    ) -> Result<Value, TemplateError> {
        let expression = expression.trim();

        // 处理条件表达式 (||)
        if let Some(or_pos) = self.find_operator(expression, "||") {
            let left = &expression[..or_pos];
            let right = &expression[or_pos + 2..].trim();
            let left_val = self.evaluate(left.trim(), context)?;
            if left_val.is_null() || left_val.as_str() == Some("") {
                return self.evaluate(right, context);
            }
            return Ok(left_val);
        }

        // 处理布尔比较 (==)
        if let Some(eq_pos) = self.find_operator(expression, "==") {
            let left = &expression[..eq_pos];
            let right = &expression[eq_pos + 2..].trim();
            let left_val = self.evaluate(left.trim(), context)?;
            let right_val = self.evaluate(right, context)?;
            return Ok(Value::Bool(left_val == right_val));
        }

        // 处理过滤器链 (|)
        let parts: Vec<&str> = expression.split('|').map(|s| s.trim()).collect();
        if parts.len() > 1 {
            let mut value = self.evaluate(parts[0], context)?;
            for filter_expr in &parts[1..] {
                value = self.apply_filter(&value, filter_expr)?;
            }
            return Ok(value);
        }

        // 解析路径
        self.resolve_path(expression, context)
    }

    /// 查找操作符位置（忽略引号内和嵌套括号内的操作符）
    fn find_operator(&self, expression: &str, op: &str) -> Option<usize> {
        let mut depth = 0;
        let mut in_quotes = false;
        let mut quote_char = '"';
        let bytes = expression.as_bytes();

        for i in 0..expression.len() {
            let ch = bytes[i] as char;
            if ch == '"' || ch == '\'' {
                if !in_quotes {
                    in_quotes = true;
                    quote_char = ch;
                } else if ch == quote_char {
                    in_quotes = false;
                }
            } else if !in_quotes {
                if ch == '(' || ch == '[' || ch == '{' {
                    depth += 1;
                } else if ch == ')' || ch == ']' || ch == '}' {
                    depth -= 1;
                } else if depth == 0 && expression[i..].starts_with(op) {
                    return Some(i);
                }
            }
        }
        None
    }

    /// 应用过滤器
    fn apply_filter(&self, value: &Value, filter_expr: &str) -> Result<Value, TemplateError> {
        // 解析过滤器名和参数
        let (filter_name, args) = if let Some(paren_pos) = filter_expr.find('(') {
            let name = filter_expr[..paren_pos].trim();
            let args_str = filter_expr[paren_pos + 1..].trim_end_matches(')');
            let args: Vec<&str> = args_str
                .split(',')
                .map(|s| s.trim().trim_matches('"'))
                .collect();
            (name, args)
        } else {
            (filter_expr, vec![])
        };

        // 查找并执行过滤器
        let filter_func = self
            .filters
            .get(filter_name)
            .ok_or_else(|| TemplateError::FilterNotFound(filter_name.to_string()))?;

        filter_func(value, &args)
    }

    /// 解析路径表达式
    ///
    /// 支持引号字符串（返回为字符串值）和路径表达式（返回为变量值）
    fn resolve_path(
        &self,
        path: &str,
        context: &HashMap<String, Value>,
    ) -> Result<Value, TemplateError> {
        // 处理引号字符串
        if (path.starts_with('"') && path.ends_with('"'))
            || (path.starts_with('\'') && path.ends_with('\''))
        {
            return Ok(Value::String(path[1..path.len() - 1].to_string()));
        }

        let parts: Vec<&str> = path.split('.').collect();
        if parts.is_empty() {
            return Err(TemplateError::SyntaxError("空路径".to_string()));
        }

        // 获取根变量
        let root_key = parts[0];
        let mut current = match context.get(root_key) {
            Some(v) => v.clone(),
            None => return Ok(Value::Null), // 根变量不存在返回 Null
        };

        // 遍历路径
        for part in &parts[1..] {
            current = self.navigate_path(&current, part)?;
        }

        Ok(current)
    }

    /// 在 JSON 值中导航路径
    ///
    /// 返回 Null 表示路径不存在（用于 default 和 || 操作符）
    fn navigate_path(&self, value: &Value, path_part: &str) -> Result<Value, TemplateError> {
        // 检查是否包含数组索引
        if let Some(bracket_pos) = path_part.find('[') {
            let field = &path_part[..bracket_pos];
            let index_str = &path_part[bracket_pos + 1..path_part.len() - 1];

            // 先访问字段（不存在则返回 Null）
            let obj = if field.is_empty() {
                value.clone()
            } else {
                match value.get(field) {
                    Some(v) => v.clone(),
                    None => return Ok(Value::Null),
                }
            };

            // 再访问数组索引（越界则返回 Null）
            let index: usize = index_str
                .parse()
                .map_err(|_| TemplateError::SyntaxError(format!("无效数组索引: {}", index_str)))?;
            match obj.as_array().and_then(|arr| arr.get(index)) {
                Some(v) => Ok(v.clone()),
                None => Ok(Value::Null),
            }
        } else {
            // 字段不存在则返回 Null，而不是报错
            match value.get(path_part) {
                Some(v) => Ok(v.clone()),
                None => Ok(Value::Null),
            }
        }
    }
}

impl Default for TemplateEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Base64 编码 (简化实现)
fn base64_encode(input: &str) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = input.as_bytes();
    let mut result = String::with_capacity((bytes.len() + 2) / 3 * 4);

    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };

        let triple = (b0 << 16) | (b1 << 8) | b2;

        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        result.push(if chunk.len() > 1 {
            CHARS[((triple >> 6) & 0x3F) as usize] as char
        } else {
            '='
        });
        result.push(if chunk.len() > 2 {
            CHARS[(triple & 0x3F) as usize] as char
        } else {
            '='
        });
    }

    result
}

/// Base64 解码 (简化实现)
fn base64_decode(input: &str) -> Result<String, String> {
    let input = input.trim_end_matches('=');
    let mut bytes = Vec::with_capacity(input.len() * 3 / 4);

    for chunk in input.as_bytes().chunks(4) {
        let mut triple = 0u32;
        for (i, &b) in chunk.iter().enumerate() {
            let val = match b {
                b'A'..=b'Z' => (b - b'A') as u32,
                b'a'..=b'z' => (b - b'a' + 26) as u32,
                b'0'..=b'9' => (b - b'0' + 52) as u32,
                b'+' => 62,
                b'/' => 63,
                _ => return Err(format!("无效 Base64 字符: {}", b as char)),
            };
            triple |= val << (18 - i * 6);
        }

        bytes.push((triple >> 16) as u8);
        if chunk.len() > 2 {
            bytes.push((triple >> 8) as u8);
        }
        if chunk.len() > 3 {
            bytes.push(triple as u8);
        }
    }

    String::from_utf8(bytes).map_err(|e| format!("UTF-8 解码失败: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_test_context() -> HashMap<String, Value> {
        let mut ctx = HashMap::new();
        ctx.insert(
            "inputs".to_string(),
            json!({
                "app_name": "myapp",
                "environment": "staging",
                "version": "v1.2.3"
            }),
        );
        ctx.insert(
            "steps".to_string(),
            json!({
                "deploy": {
                    "response": {
                        "body": {
                            "url": "https://myapp.example.com",
                            "id": "deploy_123"
                        }
                    }
                }
            }),
        );
        ctx.insert(
            "variables".to_string(),
            json!({
                "items": ["apple", "banana", "cherry"],
                "users": [
                    {"name": "Alice", "status": "active"},
                    {"name": "Bob", "status": "inactive"}
                ]
            }),
        );
        ctx
    }

    #[test]
    fn test_simple_path_resolution() {
        let engine = TemplateEngine::new();
        let ctx = create_test_context();

        let result = engine.evaluate("inputs.app_name", &ctx).unwrap();
        assert_eq!(result, json!("myapp"));
    }

    #[test]
    fn test_nested_path_resolution() {
        let engine = TemplateEngine::new();
        let ctx = create_test_context();

        let result = engine
            .evaluate("steps.deploy.response.body.url", &ctx)
            .unwrap();
        assert_eq!(result, json!("https://myapp.example.com"));
    }

    #[test]
    fn test_array_index_access() {
        let engine = TemplateEngine::new();
        let ctx = create_test_context();

        let result = engine.evaluate("variables.items[0]", &ctx).unwrap();
        assert_eq!(result, json!("apple"));

        let result = engine.evaluate("variables.items[2]", &ctx).unwrap();
        assert_eq!(result, json!("cherry"));
    }

    #[test]
    fn test_uppercase_filter() {
        let engine = TemplateEngine::new();
        let ctx = create_test_context();

        let result = engine
            .evaluate("inputs.app_name | uppercase", &ctx)
            .unwrap();
        assert_eq!(result, json!("MYAPP"));
    }

    #[test]
    fn test_filter_chain() {
        let engine = TemplateEngine::new();
        let ctx = create_test_context();

        let result = engine
            .evaluate("inputs.app_name | uppercase | truncate(2)", &ctx)
            .unwrap();
        assert_eq!(result, json!("MY..."));
    }

    #[test]
    fn test_default_filter() {
        let engine = TemplateEngine::new();
        let ctx = create_test_context();

        let result = engine
            .evaluate("inputs.missing | default(\"fallback\")", &ctx)
            .unwrap();
        assert_eq!(result, json!("fallback"));
    }

    #[test]
    fn test_or_operator() {
        let engine = TemplateEngine::new();
        let ctx = create_test_context();

        let result = engine
            .evaluate("inputs.missing || \"default_value\"", &ctx)
            .unwrap();
        assert_eq!(result, json!("default_value"));
    }

    #[test]
    fn test_equality_operator() {
        let engine = TemplateEngine::new();
        let ctx = create_test_context();

        let result = engine
            .evaluate("inputs.environment == \"staging\"", &ctx)
            .unwrap();
        assert_eq!(result, json!(true));

        let result = engine
            .evaluate("inputs.environment == \"production\"", &ctx)
            .unwrap();
        assert_eq!(result, json!(false));
    }

    #[test]
    fn test_resolve_template() {
        let engine = TemplateEngine::new();
        let ctx = create_test_context();

        let template =
            "Deploying ${{inputs.app_name}} version ${{inputs.version}} to ${{inputs.environment}}";
        let result = engine.resolve_template(template, &ctx).unwrap();
        assert_eq!(result, "Deploying myapp version v1.2.3 to staging");
    }

    #[test]
    fn test_length_filter() {
        let engine = TemplateEngine::new();
        let ctx = create_test_context();

        let result = engine.evaluate("variables.items | length", &ctx).unwrap();
        assert_eq!(result, json!(3));
    }

    #[test]
    fn test_join_filter() {
        let engine = TemplateEngine::new();
        let ctx = create_test_context();

        let result = engine
            .evaluate("variables.items | join(\"-\")", &ctx)
            .unwrap();
        assert_eq!(result, json!("apple-banana-cherry"));
    }

    #[test]
    fn test_slice_filter() {
        let engine = TemplateEngine::new();
        let ctx = create_test_context();

        let result = engine
            .evaluate("variables.items | slice(0, 2)", &ctx)
            .unwrap();
        assert_eq!(result, json!(["apple", "banana"]));
    }
}
