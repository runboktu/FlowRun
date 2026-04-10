use crate::core::context::ExecutionContext;
use crate::core::template::TemplateEngine;
use crate::core::types::*;
use crate::utils::error::WorkflowError;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{timeout, sleep};

/// 审批存储 Trait
///
/// 定义了审批状态存储的接口，用于持久化审批状态
#[async_trait::async_trait]
pub trait ApprovalStore: Send + Sync {
    /// 保存审批状态
    async fn save_approval(
        &self,
        step_id: &str,
        status: ApprovalStatus,
        approved_by: Option<String>,
        comment: Option<String>,
    ) -> Result<(), WorkflowError>;

    /// 获取审批状态
    async fn get_approval(&self, step_id: &str) -> Result<Option<ApprovalData>, WorkflowError>;
}

/// 审批数据
#[derive(Debug, Clone)]
pub struct ApprovalData {
    /// 步骤 ID
    pub step_id: String,
    /// 审批状态
    pub status: ApprovalStatus,
    /// 审批人
    pub approved_by: Option<String>,
    /// 审批意见
    pub comment: Option<String>,
    /// 创建时间
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// 审批通知发送器 Trait
///
/// 定义了发送审批通知的接口
#[async_trait::async_trait]
pub trait ApprovalNotifier: Send + Sync {
    /// 发送审批通知
    async fn send_notification(
        &self,
        step: &StepDefinition,
        context: &ExecutionContext,
    ) -> Result<(), WorkflowError>;
}

/// 内存审批存储（用于测试）
#[derive(Debug, Default)]
pub struct InMemoryApprovalStore {
    approvals: Arc<tokio::sync::RwLock<HashMap<String, ApprovalData>>>,
}

impl InMemoryApprovalStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait::async_trait]
impl ApprovalStore for InMemoryApprovalStore {
    async fn save_approval(
        &self,
        step_id: &str,
        status: ApprovalStatus,
        approved_by: Option<String>,
        comment: Option<String>,
    ) -> Result<(), WorkflowError> {
        let mut approvals = self.approvals.write().await;
        approvals.insert(
            step_id.to_string(),
            ApprovalData {
                step_id: step_id.to_string(),
                status,
                approved_by,
                comment,
                created_at: chrono::Utc::now(),
            },
        );
        Ok(())
    }

    async fn get_approval(&self, step_id: &str) -> Result<Option<ApprovalData>, WorkflowError> {
        let approvals = self.approvals.read().await;
        Ok(approvals.get(step_id).cloned())
    }
}

/// 简单的通知发送器（用于测试）
#[derive(Debug, Default)]
pub struct SimpleApprovalNotifier;

#[async_trait::async_trait]
impl ApprovalNotifier for SimpleApprovalNotifier {
    async fn send_notification(
        &self,
        step: &StepDefinition,
        _context: &ExecutionContext,
    ) -> Result<(), WorkflowError> {
        // 在实际应用中，这里应该发送邮件、Slack 消息等
        eprintln!(
            "发送审批通知: 步骤 {} - {}",
            step.id,
            step.message.as_deref().unwrap_or("需要审批")
        );
        Ok(())
    }
}

/// 人工审批执行器
///
/// 负责执行人工审批步骤，包括自动审批检查、通知发送、超时处理等
pub struct ApproveExecutor {
    /// 审批存储
    store: Arc<dyn ApprovalStore>,
    /// 通知发送器
    notifier: Arc<dyn ApprovalNotifier>,
}

impl Default for ApproveExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl ApproveExecutor {
    /// 创建新的审批执行器
    pub fn new() -> Self {
        Self {
            store: Arc::new(InMemoryApprovalStore::new()),
            notifier: Arc::new(SimpleApprovalNotifier),
        }
    }

    pub fn store(&self) -> Arc<dyn ApprovalStore> {
        Arc::clone(&self.store)
    }

    /// 创建带自定义存储和通知器的审批执行器
    pub fn with_components(
        store: Arc<dyn ApprovalStore>,
        notifier: Arc<dyn ApprovalNotifier>,
    ) -> Self {
        Self { store, notifier }
    }

    /// 执行审批步骤
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

        // 检查自动审批条件
        if let Some(auto_approve_result) = self.check_auto_approve(step, context).await? {
            return Ok(self.build_success_result(
                step_id,
                started_at,
                auto_approve_result.status,
                auto_approve_result.approved_by,
                auto_approve_result.comment,
            ));
        }

        // 发送审批通知
        self.send_notification(step, context).await?;

        // 初始化审批状态为 Pending
        self.store
            .save_approval(step_id, ApprovalStatus::Pending, None, None)
            .await?;

        // 解析超时时间
        let timeout_duration = self.parse_timeout(step.timeout.as_deref())?;

        // 等待审批
        let approval_result = match timeout_duration {
            Some(duration) => {
                let timeout_future = timeout(duration, self.wait_for_approval(step, step_id));
                match timeout_future.await {
                    Ok(Ok(result)) => result,
                    Ok(Err(e)) => return Err(e),
                    Err(_) => {
                        // 超时，根据超时策略处理
                        self.handle_timeout(step, step_id).await?
                    }
                }
            }
            None => self.wait_for_approval(step, step_id).await?,
        };

        // 验证审批结果
        if approval_result.status == ApprovalStatus::Rejected {
            return Err(WorkflowError::ApprovalRejected {
                step_id: step_id.to_string(),
                reason: approval_result
                    .comment
                    .unwrap_or_else(|| "审批被拒绝".to_string()),
            });
        }

        if approval_result.status == ApprovalStatus::TimedOut {
            return Err(WorkflowError::ApprovalTimeout {
                step_id: step_id.to_string(),
            });
        }

        Ok(self.build_success_result(
            step_id,
            started_at,
            approval_result.status,
            approval_result.approved_by,
            approval_result.comment,
        ))
    }

    /// 检查自动审批条件
    ///
    /// # 参数
    ///
    /// * `step` - 步骤定义
    /// * `context` - 执行上下文
    ///
    /// # 返回
    ///
    /// 如果满足自动审批条件，返回审批结果；否则返回 None
    async fn check_auto_approve(
        &self,
        step: &StepDefinition,
        context: &ExecutionContext,
    ) -> Result<Option<ApprovalResult>, WorkflowError> {
        if let Some(auto_approve_rules) = &step.auto_approve_on {
            for rule in auto_approve_rules {
                // 评估条件表达式
                if self.evaluate_condition(&rule.condition, context)? {
                    // 满足自动审批条件
                    return Ok(Some(ApprovalResult {
                        status: ApprovalStatus::AutoApproved,
                        approved_by: Some("system".to_string()),
                        comment: Some(rule.reason.clone()),
                    }));
                }
            }
        }

        Ok(None)
    }

    /// 发送审批通知
    ///
    /// # 参数
    ///
    /// * `step` - 步骤定义
    /// * `context` - 执行上下文
    async fn send_notification(
        &self,
        step: &StepDefinition,
        context: &ExecutionContext,
    ) -> Result<(), WorkflowError> {
        self.notifier.send_notification(step, context).await
    }

    /// 等待审批
    ///
    /// 通过轮询检查审批状态
    ///
    /// # 参数
    ///
    /// * `step` - 步骤定义
    /// * `step_id` - 步骤 ID
    ///
    /// # 返回
    ///
    /// 返回审批结果
    async fn wait_for_approval(
        &self,
        step: &StepDefinition,
        step_id: &str,
    ) -> Result<ApprovalResult, WorkflowError> {
        let check_interval = Duration::from_secs(5);

        loop {
            // 获取审批状态
            if let Some(approval) = self.store.get_approval(step_id).await? {
                match approval.status {
                    ApprovalStatus::Approved => {
                        // 验证是否需要审批意见
                        let has_comment = approval.comment.as_ref().map_or(false, |c| !c.is_empty());
                        if step.require_comment.unwrap_or(false) && !has_comment {
                            continue;
                        }
                        return Ok(ApprovalResult {
                            status: ApprovalStatus::Approved,
                            approved_by: approval.approved_by,
                            comment: approval.comment,
                        });
                    }
                    ApprovalStatus::Rejected => {
                        return Ok(ApprovalResult {
                            status: ApprovalStatus::Rejected,
                            approved_by: approval.approved_by,
                            comment: approval.comment,
                        });
                    }
                    ApprovalStatus::AutoApproved => {
                        return Ok(ApprovalResult {
                            status: ApprovalStatus::AutoApproved,
                            approved_by: approval.approved_by,
                            comment: approval.comment,
                        });
                    }
                    _ => {
                        // 继续等待
                    }
                }
            }

            sleep(check_interval).await;
        }
    }

    /// 处理审批超时
    ///
    /// # 参数
    ///
    /// * `step` - 步骤定义
    /// * `step_id` - 步骤 ID
    ///
    /// # 返回
    ///
    /// 返回审批结果或错误
    async fn handle_timeout(
        &self,
        step: &StepDefinition,
        step_id: &str,
    ) -> Result<ApprovalResult, WorkflowError> {
        let timeout_strategy = step
            .on_timeout
            .as_ref()
            .unwrap_or(&ApprovalTimeoutStrategy::Abort);

        match timeout_strategy {
            ApprovalTimeoutStrategy::Abort => {
                self.store
                    .save_approval(
                        step_id,
                        ApprovalStatus::TimedOut,
                        Some("system".to_string()),
                        Some("审批超时".to_string()),
                    )
                    .await?;
                Ok(ApprovalResult {
                    status: ApprovalStatus::TimedOut,
                    approved_by: Some("system".to_string()),
                    comment: Some("审批超时".to_string()),
                })
            }
            ApprovalTimeoutStrategy::Pause => {
                self.store
                    .save_approval(
                        step_id,
                        ApprovalStatus::TimedOut,
                        Some("system".to_string()),
                        Some("审批超时，已暂停".to_string()),
                    )
                    .await?;
                Ok(ApprovalResult {
                    status: ApprovalStatus::TimedOut,
                    approved_by: Some("system".to_string()),
                    comment: Some("审批超时，已暂停".to_string()),
                })
            }
            ApprovalTimeoutStrategy::Continue => {
                self.store
                    .save_approval(
                        step_id,
                        ApprovalStatus::TimedOut,
                        Some("system".to_string()),
                        Some("审批超时，继续执行".to_string()),
                    )
                    .await?;
                Ok(ApprovalResult {
                    status: ApprovalStatus::AutoApproved,
                    approved_by: Some("system".to_string()),
                    comment: Some("审批超时，自动批准".to_string()),
                })
            }
        }
    }

    /// 评估条件表达式
    ///
    /// # 参数
    ///
    /// * `condition` - 条件表达式
    /// * `context` - 执行上下文
    ///
    /// # 返回
    ///
    /// 返回条件是否为真
    fn evaluate_condition(
        &self,
        condition: &str,
        context: &ExecutionContext,
    ) -> Result<bool, WorkflowError> {
        let mut template_ctx = HashMap::new();
        template_ctx.insert("inputs".to_string(), serde_json::to_value(&context.inputs).unwrap_or_default());
        template_ctx.insert("variables".to_string(), serde_json::to_value(&context.variables).unwrap_or_default());

        let steps_map: HashMap<String, serde_json::Value> = context
            .step_outputs
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::to_value(v).unwrap_or_default()))
            .collect();
        template_ctx.insert("steps".to_string(), serde_json::json!(steps_map));

        let engine = TemplateEngine::new();

        let condition = condition.trim();
        let condition = if condition.starts_with("${{") && condition.ends_with("}}") {
            &condition[3..condition.len() - 2]
        } else {
            condition
        };

        let value = engine.evaluate(condition, &template_ctx).map_err(|e| {
            WorkflowError::Other(format!("条件求值失败: {}", e))
        })?;

        match value {
            serde_json::Value::Bool(b) => Ok(b),
            serde_json::Value::String(s) => Ok(!s.is_empty()),
            serde_json::Value::Number(n) => Ok(n.as_i64().unwrap_or(0) != 0),
            serde_json::Value::Array(arr) => Ok(!arr.is_empty()),
            serde_json::Value::Object(obj) => Ok(!obj.is_empty()),
            serde_json::Value::Null => Ok(false),
        }
    }

    /// 解析超时时间
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
                let num = s
                    .parse::<u64>()
                    .map_err(|_| WorkflowError::Other(format!("无效的超时时间: {}", s)))?;
                Ok(Some(Duration::from_secs(num)))
            }
        } else {
            Ok(None)
        }
    }

    /// 构建成功的审批结果
    ///
    /// # 参数
    ///
    /// * `step_id` - 步骤 ID
    /// * `started_at` - 开始时间
    /// * `status` - 审批状态
    /// * `approved_by` - 审批人
    /// * `comment` - 审批意见
    ///
    /// # 返回
    ///
    /// 返回步骤执行结果
    fn build_success_result(
        &self,
        step_id: &str,
        started_at: chrono::DateTime<chrono::Utc>,
        status: ApprovalStatus,
        approved_by: Option<String>,
        comment: Option<String>,
    ) -> StepResult {
        let completed_at = chrono::Utc::now();
        let duration_ms = (completed_at - started_at).num_milliseconds() as u64;

        let output = serde_json::json!({
            "status": status,
            "approved_by": approved_by,
            "comment": comment,
        });

        StepResult {
            step_id: step_id.to_string(),
            status: StepStatus::Success,
            started_at,
            completed_at: Some(completed_at),
            duration_ms: Some(duration_ms),
            output: Some(output),
            error: None,
        }
    }
}

/// 审批结果
#[derive(Debug)]
struct ApprovalResult {
    /// 审批状态
    pub status: ApprovalStatus,
    /// 审批人
    pub approved_by: Option<String>,
    /// 审批意见
    pub comment: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_parse_timeout() {
        let executor = ApproveExecutor::new();

        let result = executor.parse_timeout(Some("30")).unwrap();
        assert_eq!(result, Some(Duration::from_secs(30)));

        let result = executor.parse_timeout(Some("500ms")).unwrap();
        assert_eq!(result, Some(Duration::from_millis(500)));

        let result = executor.parse_timeout(None).unwrap();
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_evaluate_condition() {
        let executor = ApproveExecutor::new();

        let mut inputs = HashMap::new();
        inputs.insert("flag".to_string(), json!(true));

        let workflow = WorkflowDefinition {
            name: "test".to_string(),
            description: None,
            version: None,
            config: None,
            inputs: None,
            outputs: None,
            steps: vec![],
            on: None,
            trigger: None,
            variables: None,
        };

        let context = ExecutionContext::new(&workflow, inputs);

        let result = executor.evaluate_condition("${{ inputs.flag }}", &context).unwrap();
        assert!(result);

        let result = executor
            .evaluate_condition("${{ inputs.flag }}", &context)
            .unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn test_auto_approve() {
        let store = Arc::new(InMemoryApprovalStore::new());
        let notifier = Arc::new(SimpleApprovalNotifier);
        let executor = ApproveExecutor::with_components(store, notifier);

        let mut inputs = HashMap::new();
        inputs.insert("auto_approve".to_string(), json!(true));

        let workflow = WorkflowDefinition {
            name: "test".to_string(),
            description: None,
            version: None,
            config: None,
            inputs: None,
            outputs: None,
            steps: vec![],
            on: None,
            trigger: None,
            variables: None,
        };

        let context = ExecutionContext::new(&workflow, inputs);

        let step = StepDefinition {
            id: "test_step".to_string(),
            name: None,
            r#type: StepType::Approve,
            depends_on: None,
            expect: None,
            retry: None,
            timeout: None,
            hooks: None,
            api: None,
            method: None,
            headers: None,
            body: None,
            cache: None,
            run: None,
            env: None,
            safe_mode: None,
            allowed_commands: None,
            steps: None,
            max_concurrent: None,
            rate_limit: None,
            r#loop: None,
            do_steps: None,
            expression: None,
            then_steps: None,
            else_steps: None,
            workflow: None,
            inputs: None,
            error_strategy: None,
            isolation: None,
            passthrough_vars: None,
            message: Some("Test approval".to_string()),
            approvers: Some(vec!["user1".to_string()]),
            require_comment: Some(false),
            on_timeout: Some(ApprovalTimeoutStrategy::Abort),
            auto_approve_on: Some(vec![AutoApproveRule {
                condition: "${{ inputs.auto_approve }}".to_string(),
                reason: "自动测试批准".to_string(),
            }]),
            agent_system_prompt: None,
            agent_input: None,
            agent_max_iterations: None,
            agent_stream: None,
            tool_name: None,
            tool_args: None,
            agent_tools: None,
            tool: None,
        };

        let result = executor.execute(&step, &context).await.unwrap();
        assert_eq!(result.status, StepStatus::Success);
    }
}
