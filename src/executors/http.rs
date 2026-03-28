use crate::core::context::ExecutionContext;
use crate::core::template::TemplateEngine;
use crate::core::types::*;
use crate::executors::Executor;
use crate::utils::error::WorkflowError;
use chrono::{DateTime, Duration, Utc};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::str::FromStr;
use std::time::Duration as StdDuration;
use tokio::sync::RwLock;
use tokio::time::sleep;

/// HTTP 执行器
///
/// 负责执行 HTTP 请求，支持：
/// - 多种 HTTP 方法（GET/POST/PUT/DELETE/PATCH）
/// - 模板表达式解析
/// - 响应缓存
/// - 请求重试
/// - 自定义请求头
/// - JSON 请求体
/// - 期望结果验证
pub struct HttpExecutor {
    /// HTTP 客户端
    client: reqwest::Client,
    /// 缓存存储 (缓存键 -> (响应体, 过期时间))
    /// 使用 RwLock 实现线程安全的内部可变性
    cache: RwLock<HashMap<String, (Value, DateTime<Utc>)>>,
    /// 最大缓存大小
    max_cache_size: Option<NonZeroUsize>,
    /// 模板引擎
    template_engine: TemplateEngine,
}

impl Default for HttpExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpExecutor {
    /// 创建默认的 HTTP 执行器
    ///
    /// 使用默认的 reqwest 客户端配置
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .build()
                .expect("Failed to create HTTP client"),
            cache: RwLock::new(HashMap::new()),
            max_cache_size: None,
            template_engine: TemplateEngine::new(),
        }
    }

    /// 创建带自定义客户端的 HTTP 执行器
    ///
    /// # 参数
    ///
    /// * `client` - 自定义的 reqwest 客户端
    ///
    /// # 返回
    ///
    /// 返回新创建的 HTTP 执行器
    pub fn with_client(client: reqwest::Client) -> Self {
        Self {
            client,
            cache: RwLock::new(HashMap::new()),
            max_cache_size: None,
            template_engine: TemplateEngine::new(),
        }
    }

    /// 设置最大缓存大小
    ///
    /// # 参数
    ///
    /// * `size` - 最大缓存条目数，如果设置为 0 则表示无限制
    ///
    /// # 返回
    ///
    /// 返回 self 以支持链式调用
    pub fn with_cache_size(mut self, size: usize) -> Self {
        self.max_cache_size = NonZeroUsize::new(size);
        self
    }

    /// 清理过期缓存
    ///
    /// 移除所有已过期的缓存条目，如果设置了最大缓存大小，还会移除超出限制的旧条目
    pub async fn cleanup_cache(&self) {
        let now = Utc::now();
        let mut cache = self.cache.write().await;

        // 移除过期的缓存条目
        cache.retain(|_, (_, expiry)| *expiry > now);

        // 如果设置了最大缓存大小，移除超出限制的条目
        if let Some(max_size) = self.max_cache_size {
            let max_size = max_size.get();
            if cache.len() > max_size {
                // 收集所有条目并按过期时间排序
                let mut entries: Vec<(String, DateTime<Utc>)> = cache
                    .iter()
                    .map(|(k, (_, e))| (k.clone(), *e))
                    .collect();
                entries.sort_by_key(|&(_, expiry)| expiry);

                // 保留最近的 max_size 个条目，移除其余的
                let to_remove = entries.len().saturating_sub(max_size);
                for (key, _) in entries.into_iter().take(to_remove) {
                    cache.remove(&key);
                }
            }
        }
    }

    /// 清空所有缓存
    pub async fn clear_cache(&self) {
        self.cache.write().await.clear();
    }

    /// 解析模板字符串
    ///
    /// 使用模板引擎解析模板表达式，替换变量占位符为实际值
    /// 支持复杂的表达式，包括路径访问、过滤器等
    ///
    /// # 参数
    ///
    /// * `template` - 模板字符串
    /// * `context` - 执行上下文
    ///
    /// # 返回
    ///
    /// 返回解析后的字符串
    fn resolve_template(
        &self,
        template: &str,
        context: &ExecutionContext,
    ) -> Result<String, WorkflowError> {
        // 将 ExecutionContext 转换为模板引擎需要的上下文格式
        let template_context = self.build_template_context(context);

        // 使用模板引擎解析
        self.template_engine
            .resolve_template(template, &template_context)
            .map_err(|e| WorkflowError::Other(format!("模板解析失败: {}", e)))
    }

    /// 构建 TemplateEngine 需要的上下文
    ///
    /// 将 ExecutionContext 转换为模板引擎可以使用的格式
    ///
    /// # 参数
    ///
    /// * `context` - 执行上下文
    ///
    /// # 返回
    ///
    /// 返回模板上下文映射
    fn build_template_context(&self, context: &ExecutionContext) -> HashMap<String, Value> {
        let mut template_context = HashMap::new();

        // 添加 inputs
        template_context.insert("inputs".to_string(), Value::Object(
            context
                .inputs
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        ));

        // 添加 variables
        template_context.insert("variables".to_string(), Value::Object(
            context
                .variables
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        ));

        // 添加 steps
        let mut steps_map = serde_json::Map::new();
        for (step_id, step_result) in &context.step_outputs {
            if let Some(output) = &step_result.output {
                steps_map.insert(step_id.clone(), output.clone());
            }
        }
        template_context.insert("steps".to_string(), Value::Object(steps_map));

        template_context
    }

    /// 生成缓存键
    ///
    /// 根据请求的 URL、方法和请求体生成唯一的缓存键
    ///
    /// # 参数
    ///
    /// * `url` - 请求 URL
    /// * `method` - HTTP 方法
    /// * `body` - 请求体（可选）
    ///
    /// # 返回
    ///
    /// 返回缓存键字符串
    fn generate_cache_key(
        &self,
        url: &str,
        method: &str,
        body: Option<&Value>,
    ) -> String {
        let body_str = body.map(|b| b.to_string()).unwrap_or_default();
        format!("{}:{}:{}", method, url, body_str)
    }

    /// 解析请求头
    ///
    /// 解析请求头配置，替换模板表达式
    ///
    /// # 参数
    ///
    /// * `headers` - 请求头映射
    /// * `context` - 执行上下文
    ///
    /// # 返回
    ///
    /// 返回解析后的请求头映射
    fn parse_headers(
        &self,
        headers: &HashMap<String, String>,
        context: &ExecutionContext,
    ) -> Result<HeaderMap, WorkflowError> {
        let mut header_map = HeaderMap::new();

        for (key, value) in headers {
            // 解析模板表达式
            let resolved_value = self.resolve_template(value, context)?;

            // 转换为 HeaderName 和 HeaderValue
            let header_name = HeaderName::from_str(key).map_err(|e| {
                WorkflowError::Other(format!("无效的请求头名称 '{}': {}", key, e))
            })?;

            let header_value = HeaderValue::from_str(&resolved_value).map_err(|e| {
                WorkflowError::Other(format!("无效的请求头值 '{}': {}", key, e))
            })?;

            header_map.insert(header_name, header_value);
        }

        Ok(header_map)
    }

    /// 验证期望结果
    ///
    /// 检查 HTTP 响是否符合期望配置
    ///
    /// # 参数
    ///
    /// * `expect` - 期望配置
    /// * `status_code` - HTTP 状态码
    /// * `body` - 响应体
    ///
    /// # 返回
    ///
    /// 如果验证通过则返回 Ok，否则返回错误
    fn validate_expect(
        &self,
        expect: &ExpectConfig,
        status_code: u16,
        body: &Value,
    ) -> Result<(), WorkflowError> {
        // 验证状态码
        if let Some(expected_status) = expect.status_code {
            if status_code != expected_status {
                return Err(WorkflowError::HttpRequestFailed {
                    status_code,
                    message: format!("期望状态码 {}, 实际 {}", expected_status, status_code),
                });
            }
        }

        // 验证响应体包含期望内容
        if let Some(expected_body) = &expect.body_contains {
            let body_str = serde_json::to_string(body).unwrap_or_default();
            if !body_str.contains(expected_body) {
                return Err(WorkflowError::Other(format!(
                    "响应体不包含期望内容: '{}'",
                    expected_body
                )));
            }
        }

        // TODO: 实现 JSON 路径验证
        if let Some(_json_path) = &expect.json_path {
            // 这里需要实现 JSON 路径解析和验证
            // 暂时跳过
        }

        Ok(())
    }

    /// 发送 HTTP 请求并处理重试
    ///
    /// # 参数
    ///
    /// * `request` - HTTP 请求构建器
    /// * `retry_config` - 重试配置
    /// * `step_id` - 步骤 ID（用于错误消息）
    ///
    /// # 返回
    ///
    /// 返回 HTTP 响应
    async fn send_request_with_retry(
        &self,
        request: reqwest::RequestBuilder,
        retry_config: Option<&RetryConfig>,
        step_id: &str,
    ) -> Result<reqwest::Response, WorkflowError> {
        let max_attempts = retry_config.map(|r| r.max_attempts).unwrap_or(1);

        for attempt in 1..=max_attempts {
            match request.try_clone().unwrap().send().await {
                Ok(response) => {
                    // 如果状态码表示成功，直接返回
                    if response.status().is_success() {
                        return Ok(response);
                    }

                    // 如果是最后一次尝试，返回错误
                    if attempt == max_attempts {
                        return Err(WorkflowError::HttpRequestFailed {
                            status_code: response.status().as_u16(),
                            message: format!("步骤 {}: HTTP 请求失败", step_id),
                        });
                    }

                    // 根据重试策略等待
                    if let Some(retry) = retry_config {
                        let delay = self.calculate_retry_delay(retry, attempt);
                        sleep(StdDuration::from_millis(delay as u64)).await;
                    }
                }
                Err(e) => {
                    // 如果是最后一次尝试，返回错误
                    if attempt == max_attempts {
                        return Err(WorkflowError::Other(format!(
                            "步骤 {}: HTTP 请求错误: {}",
                            step_id, e
                        )));
                    }

                    // 根据重试策略等待
                    if let Some(retry) = retry_config {
                        let delay = self.calculate_retry_delay(retry, attempt);
                        sleep(StdDuration::from_millis(delay as u64)).await;
                    }
                }
            }
        }

        Err(WorkflowError::Other(format!("步骤 {}: 超过最大重试次数", step_id)))
    }

    /// 计算重试延迟时间
    ///
    /// # 参数
    ///
    /// * `retry_config` - 重试配置
    /// * `attempt` - 当前尝试次数
    ///
    /// # 返回
    ///
    /// 返回延迟时间（毫秒）
    fn calculate_retry_delay(&self, retry_config: &RetryConfig, attempt: u32) -> f64 {
        let strategy = retry_config.strategy.as_ref().unwrap_or(&BackoffStrategy::Exponential);

        let initial_delay = retry_config.initial_delay.unwrap_or(1.0);
        let max_delay = retry_config.max_delay.unwrap_or(30.0);
        let factor = retry_config.factor.unwrap_or(2.0);

        let delay = match strategy {
            BackoffStrategy::Fixed => initial_delay,
            BackoffStrategy::Exponential => {
                initial_delay * factor.powi((attempt - 1) as i32)
            }
            BackoffStrategy::Fibonacci => {
                let mut a = 0.0;
                let mut b = 1.0;
                for _ in 1..attempt {
                    let temp = a + b;
                    a = b;
                    b = temp;
                }
                initial_delay * b
            }
        };

        // 应用最大延迟限制
        delay.min(max_delay)
    }

    /// 构建响应输出
    ///
    /// 将 HTTP 响应转换为结构化的 JSON 输出
    ///
    /// # 参数
    ///
    /// * `response` - HTTP 响应
    ///
    /// # 返回
    ///
    /// 返回结构化的响应 JSON
    async fn build_response_output(&self, response: reqwest::Response) -> Result<Value, WorkflowError> {
        let status_code = response.status().as_u16();

        // 收集响应头
        let mut headers_map = serde_json::Map::new();
        for (name, value) in response.headers() {
            if let Ok(value_str) = value.to_str() {
                headers_map.insert(name.as_str().to_string(), Value::String(value_str.to_string()));
            }
        }

        // 解析响应体
        let body_text = response.text().await.map_err(|e| {
            WorkflowError::Other(format!("读取响应体失败: {}", e))
        })?;

        // 尝试解析为 JSON，如果失败则作为字符串处理
        let body_value: Value = serde_json::from_str(&body_text).unwrap_or_else(|_| {
            Value::String(body_text)
        });

        Ok(serde_json::json!({
            "status_code": status_code,
            "headers": headers_map,
            "body": body_value,
        }))
    }
}

#[async_trait::async_trait]
impl Executor for HttpExecutor {
    /// 执行 HTTP 步骤
    ///
    /// 主要执行流程：
    /// 1. 解析模板表达式 (URL, headers, body)
    /// 2. 检查缓存 (如果配置了 cache)
    /// 3. 构建 HTTP 请求
    /// 4. 发送请求 (支持重试)
    /// 5. 解析响应
    /// 6. 验证期望结果 (expect 配置)
    /// 7. 缓存响应 (如果配置了 cache)
    /// 8. 返回 StepResult
    ///
    /// # 参数
    ///
    /// * `step` - 步骤定义
    /// * `context` - 执行上下文
    ///
    /// # 返回
    ///
    /// 返回步骤执行结果
    async fn execute(
        &self,
        step: &StepDefinition,
        context: &ExecutionContext,
    ) -> Result<StepResult, WorkflowError> {
        let step_id = &step.id;
        let started_at = Utc::now();

        // 获取 API URL
        let api_template = step.api.as_ref().ok_or_else(|| {
            WorkflowError::Other(format!("步骤 {} 缺少 api 字段", step_id))
        })?;
        let url = self.resolve_template(api_template, context)?;

        // 获取 HTTP 方法（默认 GET）
        let method = step.method.as_deref().unwrap_or("GET").to_uppercase();

        // 验证 HTTP 方法
        if !matches!(method.as_str(), "GET" | "POST" | "PUT" | "DELETE" | "PATCH") {
            return Err(WorkflowError::Other(format!(
                "步骤 {}: 不支持的 HTTP 方法 '{}'",
                step_id, method
            )));
        }

        // 解析请求头
        let headers_map = step.headers.as_ref().cloned().unwrap_or_default();
        let headers = self.parse_headers(&headers_map, context)?;

        // 解析请求体
        let body_value = step.body.as_ref().cloned();

        // 生成缓存键
        let cache_key = self.generate_cache_key(&url, &method, body_value.as_ref());

        // 检查缓存
        if let Some(_cache_config) = &step.cache {
            if let Some((cached_value, expiry)) = self.cache.read().await.get(&cache_key) {
                // 检查缓存是否过期
                if expiry > &Utc::now() {
                    let completed_at = Utc::now();
                    let duration_ms = (completed_at - started_at).num_milliseconds() as u64;

                    return Ok(StepResult::from_cache(step_id.clone(), cached_value.clone())
                        .with_timing(started_at, duration_ms));
                }
            }
        }

        // 构建 HTTP 请求
        let mut request_builder = self.client.request(method.parse().unwrap(), &url);

        // 添加请求头
        request_builder = request_builder.headers(headers);

        // 添加请求体
        if let Some(body) = body_value {
            request_builder = request_builder.json(&body);
        }

        // 发送请求（带重试）
        let response = self
            .send_request_with_retry(request_builder, step.retry.as_ref(), step_id)
            .await?;

        // 获取状态码
        let status_code = response.status().as_u16();

        // 构建响应输出
        let response_output = self.build_response_output(response).await?;

        // 验证期望结果
        if let Some(expect) = &step.expect {
            self.validate_expect(expect, status_code, &response_output)?;
        }

        // 缓存响应（如果配置了 cache）
        if let Some(cache_config) = &step.cache {
            let expiry = Utc::now() + Duration::seconds(cache_config.ttl as i64);
            self.cache.write().await.insert(cache_key, (response_output.clone(), expiry));
        }

        let completed_at = Utc::now();
        let duration_ms = (completed_at - started_at).num_milliseconds() as u64;

        Ok(StepResult {
            step_id: step_id.clone(),
            status: StepStatus::Success,
            started_at,
            completed_at: Some(completed_at),
            duration_ms: Some(duration_ms),
            output: Some(response_output),
            error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_generate_cache_key() {
        let executor = HttpExecutor::new();

        let key1 = executor.generate_cache_key("https://api.example.com", "GET", None);
        let key2 = executor.generate_cache_key("https://api.example.com", "GET", None);
        let key3 = executor.generate_cache_key("https://api.example.com", "POST", Some(&json!({"key":"value"})));

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_resolve_template() {
        let workflow = WorkflowDefinition {
            name: "测试工作流".to_string(),
            description: None,
            version: None,
            config: None,
            inputs: None,
            outputs: None,
            steps: vec![],
            on: None,
            trigger: None,
        };

        let mut inputs = HashMap::new();
        inputs.insert("api_url".to_string(), json!("https://api.example.com"));

        let mut context = ExecutionContext::new(&workflow, inputs);
        context.set_variable("token".to_string(), json!("secret-token"));

        let executor = HttpExecutor::new();

        let template = "https://${{ inputs.api_url }}/endpoint?token=${{ variables.token }}";
        let result = executor.resolve_template(template, &context).unwrap();
        assert_eq!(result, "https://https://api.example.com/endpoint?token=secret-token");
    }

    #[test]
    fn test_validate_expect() {
        let executor = HttpExecutor::new();

        let expect = ExpectConfig {
            status_code: Some(200),
            exit_code: None,
            body_contains: None,
            json_path: None,
        };

        let body = json!({"data": "test"});

        assert!(executor.validate_expect(&expect, 200, &body).is_ok());
        assert!(executor.validate_expect(&expect, 404, &body).is_err());

        let expect_with_body = ExpectConfig {
            status_code: None,
            exit_code: None,
            body_contains: Some("data".to_string()),
            json_path: None,
        };

        assert!(executor.validate_expect(&expect_with_body, 200, &body).is_ok());
        assert!(executor.validate_expect(&expect_with_body, 200, &json!({"other": "test"})).is_err());
    }

    #[test]
    fn test_calculate_retry_delay() {
        let executor = HttpExecutor::new();

        let retry_config = RetryConfig {
            max_attempts: 3,
            strategy: Some(BackoffStrategy::Exponential),
            initial_delay: Some(1.0),
            max_delay: Some(30.0),
            jitter: None,
            factor: Some(2.0),
        };

        let delay1 = executor.calculate_retry_delay(&retry_config, 1);
        let delay2 = executor.calculate_retry_delay(&retry_config, 2);
        let delay3 = executor.calculate_retry_delay(&retry_config, 3);

        assert_eq!(delay1, 1.0); // 1 * 2^0
        assert_eq!(delay2, 2.0); // 1 * 2^1
        assert_eq!(delay3, 4.0); // 1 * 2^2
    }

    #[test]
    fn test_parse_headers() {
        let workflow = WorkflowDefinition {
            name: "测试工作流".to_string(),
            description: None,
            version: None,
            config: None,
            inputs: None,
            outputs: None,
            steps: vec![],
            on: None,
            trigger: None,
        };

        let mut inputs = HashMap::new();
        inputs.insert("token".to_string(), json!("Bearer mytoken"));

        let context = ExecutionContext::new(&workflow, inputs);

        let executor = HttpExecutor::new();

        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "${{ inputs.token }}".to_string());
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        let header_map = executor.parse_headers(&headers, &context).unwrap();

        assert_eq!(
            header_map.get("Authorization").unwrap().to_str().unwrap(),
            "Bearer mytoken"
        );
        assert_eq!(
            header_map.get("Content-Type").unwrap().to_str().unwrap(),
            "application/json"
        );
    }

    #[tokio::test]
    async fn test_with_cache_size() {
        let executor = HttpExecutor::new().with_cache_size(2);

        // 验证 max_cache_size 已设置
        assert!(executor.max_cache_size.is_some());
        assert_eq!(executor.max_cache_size.unwrap().get(), 2);
    }

    #[test]
    fn test_build_template_context() {
        let workflow = WorkflowDefinition {
            name: "测试工作流".to_string(),
            description: None,
            version: None,
            config: None,
            inputs: None,
            outputs: None,
            steps: vec![],
            on: None,
            trigger: None,
        };

        let mut inputs = HashMap::new();
        inputs.insert("name".to_string(), json!("test"));

        let mut context = ExecutionContext::new(&workflow, inputs);
        context.set_variable("var".to_string(), json!("value"));

        let executor = HttpExecutor::new();
        let template_context = executor.build_template_context(&context);

        assert!(template_context.contains_key("inputs"));
        assert!(template_context.contains_key("variables"));
        assert!(template_context.contains_key("steps"));
    }
}
