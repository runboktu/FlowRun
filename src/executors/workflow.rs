use crate::core::context::ExecutionContext;
use crate::core::types::*;
use crate::utils::error::WorkflowError;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

/// 工作流运行器 Trait
///
/// 定义了工作流执行的核心接口，允许子工作流调用父工作流的执行能力
#[async_trait::async_trait]
pub trait WorkflowRunner: Send + Sync {
    /// 执行工作流
    ///
    /// # 参数
    ///
    /// * `workflow_path` - 工作流文件路径
    /// * `inputs` - 输入参数
    /// * `timeout` - 超时时间
    ///
    /// # 返回
    ///
    /// 返回工作流执行结果
    async fn run_workflow(
        &self,
        workflow_path: &str,
        inputs: HashMap<String, serde_json::Value>,
        timeout: Option<Duration>,
    ) -> Result<WorkflowResult, WorkflowError>;
}

/// 子工作流执行器
///
/// 负责执行子工作流，包括路径解析、输入准备、错误处理等
pub struct WorkflowExecutor {
    /// 工作流运行器（使用 Arc 共享）
    pub runner: Arc<dyn WorkflowRunner>,
}

impl WorkflowExecutor {
    /// 创建新的子工作流执行器
    ///
    /// # 参数
    ///
    /// * `runner` - 工作流运行器
    pub fn new(runner: Arc<dyn WorkflowRunner>) -> Self {
        Self { runner }
    }

    /// 执行子工作流步骤
    ///
    /// # 参数
    ///
    /// * `step` - 步骤定义
    /// * `context` - 执行上下文
    ///
    /// # 返回
    ///
    /// 返回步骤执行结果
    pub async fn execute(
        &self,
        step: &StepDefinition,
        context: &ExecutionContext,
    ) -> Result<StepResult, WorkflowError> {
        let step_id = &step.id;
        let started_at = chrono::Utc::now();

        // 解析子工作流路径
        let workflow_path = self.resolve_workflow_path(step)?;

        // 准备子工作流输入
        let inputs = self.prepare_inputs(step, context)?;

        // 解析超时时间
        let timeout_duration = self.parse_timeout(step.timeout.as_deref())?;

        // 执行子工作流
        let result = match timeout_duration {
            Some(duration) => {
                // 使用超时控制
                let timeout_future = timeout(duration, self.runner.run_workflow(
                    &workflow_path,
                    inputs.clone(),
                    Some(duration),
                ));

                match timeout_future.await {
                    Ok(Ok(workflow_result)) => Ok(workflow_result),
                    Ok(Err(e)) => Err(e),
                    Err(_) => Err(WorkflowError::Timeout {
                        timeout_ms: duration.as_millis() as u64,
                    }),
                }
            }
            None => {
                // 无超时限制
                self.runner.run_workflow(&workflow_path, inputs, None).await
            }
        };

        // 根据错误策略处理错误
        match result {
            Ok(workflow_result) => {
                // 子工作流成功执行
                let completed_at = chrono::Utc::now();
                let duration_ms = (completed_at - started_at).num_milliseconds() as u64;

                // 构建输出
                let output = serde_json::json!({
                    "workflow": workflow_path,
                    "status": workflow_result.status,
                    "outputs": workflow_result.outputs.unwrap_or_default(),
                    "metrics": workflow_result.metrics,
                });

                Ok(StepResult {
                    step_id: step_id.clone(),
                    status: StepStatus::Success,
                    started_at,
                    completed_at: Some(completed_at),
                    duration_ms: Some(duration_ms),
                    output: Some(output),
                    error: None,
                })
            }
            Err(e) => {
                // 子工作流执行失败，根据错误策略处理
                self.handle_error(step, step_id, &e, started_at).await
            }
        }
    }

    /// 解析子工作流路径
    ///
    /// 支持相对路径和绝对路径
    ///
    /// # 参数
    ///
    /// * `step` - 步骤定义
    ///
    /// # 返回
    ///
    /// 返回解析后的工作流路径
    fn resolve_workflow_path(&self, step: &StepDefinition) -> Result<String, WorkflowError> {
        let workflow = step.workflow.as_ref().ok_or_else(|| {
            WorkflowError::Other(format!("步骤 {} 缺少 workflow 字段", step.id))
        })?;

        // 检查路径是否存在
        if !Path::new(workflow).exists() {
            return Err(WorkflowError::WorkflowFileNotFound {
                path: workflow.clone(),
            });
        }

        // 返回规范化的绝对路径
        let path = Path::new(workflow)
            .canonicalize()
            .map_err(|e| WorkflowError::Other(format!("路径解析失败: {}", e)))?;

        Ok(path.to_string_lossy().to_string())
    }

    /// 准备子工作流输入
    ///
    /// 根据步骤配置准备输入参数，支持模板表达式
    ///
    /// # 参数
    ///
    /// * `step` - 步骤定义
    /// * `context` - 执行上下文
    ///
    /// # 返回
    ///
    /// 返回准备好的输入参数
    fn prepare_inputs(
        &self,
        step: &StepDefinition,
        context: &ExecutionContext,
    ) -> Result<HashMap<String, serde_json::Value>, WorkflowError> {
        let mut inputs = HashMap::new();

        // 从步骤配置中获取输入定义
        if let Some(step_inputs) = &step.inputs {
            for (key, value_expr) in step_inputs {
                // 尝试求值表达式
                let value = context.evaluate(value_expr)?;
                inputs.insert(key.clone(), value);
            }
        }

        // 处理上下文隔离
        let isolation = step.isolation.unwrap_or(false);
        if !isolation {
            // 如果未隔离，透传父工作流的变量
            if let Some(passthrough_vars) = &step.passthrough_vars {
                // 只透传指定的变量
                for var_name in passthrough_vars {
                    if let Some(value) = context.get_variable(var_name) {
                        inputs.insert(var_name.clone(), value);
                    }
                }
            } else {
                // 透传所有父工作流输入
                for (key, value) in &context.inputs {
                    inputs.insert(key.clone(), value.clone());
                }
            }
        }

        Ok(inputs)
    }

    /// 根据错误策略处理错误
    ///
    /// # 参数
    ///
    /// * `step` - 步骤定义
    /// * `step_id` - 步骤 ID
    /// * `error` - 错误信息
    /// * `started_at` - 开始时间
    ///
    /// # 返回
    ///
    /// 返回步骤执行结果
    async fn handle_error(
        &self,
        step: &StepDefinition,
        step_id: &str,
        error: &WorkflowError,
        started_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<StepResult, WorkflowError> {
        let error_strategy = step
            .error_strategy
            .as_ref()
            .unwrap_or(&SubWorkflowErrorStrategy::Propagate);

        match error_strategy {
            SubWorkflowErrorStrategy::Propagate => {
                // 向上传播，父工作流失败
                Err(WorkflowError::Other(error.to_string()))
            }
            SubWorkflowErrorStrategy::Continue => {
                // 子工作流失败，父工作流继续，标记步骤为失败
                let completed_at = chrono::Utc::now();
                let duration_ms = (completed_at - started_at).num_milliseconds() as u64;

                Ok(StepResult {
                    step_id: step_id.to_string(),
                    status: StepStatus::Failed,
                    started_at,
                    completed_at: Some(completed_at),
                    duration_ms: Some(duration_ms),
                    output: Some(serde_json::json!({
                        "error": error.to_string(),
                        "strategy": "continue"
                    })),
                    error: Some(StepError {
                        code: "SUB_WORKFLOW_FAILED".to_string(),
                        message: error.to_string(),
                        fix: Some("检查子工作流日志".to_string()),
                    }),
                })
            }
            SubWorkflowErrorStrategy::Retry => {
                // 重试子工作流
                // 注意：这里简单实现，实际应用中应该使用重试配置
                Err(WorkflowError::Other(error.to_string()))
            }
            SubWorkflowErrorStrategy::Ignore => {
                // 忽略错误，标记为 skipped
                let completed_at = chrono::Utc::now();
                let duration_ms = (completed_at - started_at).num_milliseconds() as u64;

                Ok(StepResult::skipped(
                    step_id,
                    format!("子工作流失败但被忽略: {}", error),
                ))
            }
        }
    }

    /// 解析超时时间
    ///
    /// 支持秒、毫秒、分钟等单位
    ///
    /// # 参数
    ///
    /// * `timeout_str` - 超时时间字符串
    ///
    /// # 返回
    ///
    /// 返回解析后的 Duration
    fn parse_timeout(&self, timeout_str: Option<&str>) -> Result<Option<Duration>, WorkflowError> {
        if let Some(s) = timeout_str {
            let s = s.trim().to_lowercase();
            if s.is_empty() {
                return Ok(None);
            }

            // 解析数字和单位
            if let Some(pos) = s.chars().position(|c| !c.is_ascii_digit()) {
                let num = s[..pos]
                    .parse::<u64>()
                    .map_err(|_| WorkflowError::Other(format!("无效的超时时间: {}", s)))?;

                let unit = &s[pos..];
                let duration = match unit {
                    "s" | "sec" | "second" | "seconds" => Duration::from_secs(num),
                    "ms" | "millisec" | "millisecond" | "milliseconds" => {
                        Duration::from_millis(num)
                    }
                    "m" | "min" | "minute" | "minutes" => Duration::from_secs(num * 60),
                    "h" | "hour" | "hours" => Duration::from_secs(num * 3600),
                    _ => {
                        return Err(WorkflowError::Other(format!(
                            "不支持的时间单位: {}",
                            unit
                        )))
                    }
                };

                Ok(Some(duration))
            } else {
                // 默认单位为秒
                let num = s
                    .parse::<u64>()
                    .map_err(|_| WorkflowError::Other(format!("无效的超时时间: {}", s)))?;
                Ok(Some(Duration::from_secs(num)))
            }
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_timeout() {
        let executor = WorkflowExecutor::new(Arc::new(MockRunner));

        // 测试秒
        let result = executor.parse_timeout(Some("30")).unwrap();
        assert_eq!(result, Some(Duration::from_secs(30)));

        // 测试毫秒
        let result = executor.parse_timeout(Some("500ms")).unwrap();
        assert_eq!(result, Some(Duration::from_millis(500)));

        // 测试分钟
        let result = executor.parse_timeout(Some("5min")).unwrap();
        assert_eq!(result, Some(Duration::from_secs(300)));

        // 测试小时
        let result = executor.parse_timeout(Some("2h")).unwrap();
        assert_eq!(result, Some(Duration::from_secs(7200)));

        // 测试空值
        let result = executor.parse_timeout(None).unwrap();
        assert_eq!(result, None);
    }

    // Mock Runner 用于测试
    struct MockRunner;

    #[async_trait::async_trait]
    impl WorkflowRunner for MockRunner {
        async fn run_workflow(
            &self,
            _workflow_path: &str,
            _inputs: HashMap<String, serde_json::Value>,
            _timeout: Option<Duration>,
        ) -> Result<WorkflowResult, WorkflowError> {
            Ok(WorkflowResult {
                status: WorkflowStatus::Success,
                workflow: WorkflowInfo {
                    name: "mock".to_string(),
                    version: None,
                    file: "mock.yml".to_string(),
                },
                execution: ExecutionInfo {
                    id: "test".to_string(),
                    started_at: chrono::Utc::now(),
                    completed_at: Some(chrono::Utc::now()),
                    duration_ms: Some(0),
                    checkpoint: None,
                },
                steps: vec![],
                outputs: Some(HashMap::new()),
                metrics: ExecutionMetrics {
                    total_steps: 0,
                    success_steps: 0,
                    failed_steps: 0,
                    skipped_steps: 0,
                    total_duration_ms: 0,
                },
                errors: vec![],
            })
        }
    }
}
