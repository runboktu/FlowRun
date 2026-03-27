use crate::core::context::ExecutionContext;
use crate::core::types::*;
use crate::executors::Executor;
use crate::utils::error::WorkflowError;
use std::sync::Arc;

/// 条件执行器
///
/// 负责执行条件分支，根据条件表达式的求值结果执行 then_steps 或 else_steps
pub struct ConditionExecutor {
    /// Then 分支执行器
    then_executor: Arc<dyn Executor>,
    /// Else 分支执行器（可选）
    else_executor: Option<Arc<dyn Executor>>,
}

impl ConditionExecutor {
    /// 创建新的条件执行器
    ///
    /// # 参数
    ///
    /// * `then_executor` - Then 分支执行器
    /// * `else_executor` - Else 分支执行器（可选）
    ///
    /// # 返回
    ///
    /// 返回新创建的条件执行器
    pub fn new(
        then_executor: Arc<dyn Executor>,
        else_executor: Option<Arc<dyn Executor>>,
    ) -> Self {
        Self {
            then_executor,
            else_executor,
        }
    }

    /// 执行条件步骤
    ///
    /// # 参数
    ///
    /// * `step` - 步骤定义
    /// * `context` - 执行上下文
    ///
    /// # 返回
    ///
    /// 返回条件执行结果，包含执行的分支和步骤结果
    pub async fn execute(
        &self,
        step: &StepDefinition,
        context: &ExecutionContext,
    ) -> Result<StepResult, WorkflowError> {
        let step_id = &step.id;
        let started_at = chrono::Utc::now();

        // 获取条件表达式
        let expression = step.expression.as_ref().ok_or_else(|| {
            WorkflowError::Other(format!("步骤 {} 缺少 expression 字段", step_id))
        })?;

        // 求值条件表达式
        let condition_value = self.evaluate_expression(expression, context)?;
        let should_execute_then = condition_value.as_bool().ok_or_else(|| {
            WorkflowError::ConditionError {
                expression: expression.to_string(),
                reason: "条件表达式的求值结果不是布尔值".to_string(),
            }
        })?;

        // 根据条件选择执行分支
        let (branch_name, branch_steps) = if should_execute_then {
            ("then".to_string(), step.then_steps.as_ref())
        } else {
            ("else".to_string(), step.else_steps.as_ref())
        };

        // 执行分支步骤
        let results = if let Some(steps) = branch_steps {
            if steps.is_empty() {
                // 分支为空，返回空结果
                vec![]
            } else {
                // 使用对应的执行器执行步骤
                let executor = if should_execute_then {
                    &self.then_executor
                } else {
                    self.else_executor.as_ref().ok_or_else(|| {
                        WorkflowError::Other(format!("步骤 {} 缺少 else 执行器", step_id))
                    })?
                };

                self.execute_branch(executor, steps, context).await?
            }
        } else {
            // 没有对应的分支，返回空结果
            vec![]
        };

        let completed_at = chrono::Utc::now();
        let duration_ms = (completed_at - started_at).num_milliseconds() as u64;

        // 构建分支响应
        let branch_response = BranchResponse {
            branch: branch_name,
            results,
        };

        Ok(StepResult {
            step_id: step_id.clone(),
            status: StepStatus::Success,
            started_at,
            completed_at: Some(completed_at),
            duration_ms: Some(duration_ms),
            output: Some(serde_json::to_value(branch_response).map_err(|e| {
                WorkflowError::Other(format!("序列化分支结果失败: {}", e))
            })?),
            error: None,
        })
    }

    /// 求值表达式
    ///
    /// 使用模板引擎求值条件表达式
    ///
    /// # 参数
    ///
    /// * `expression` - 条件表达式字符串
    /// * `context` - 执行上下文
    ///
    /// # 返回
    ///
    /// 返回求值结果
    ///
    /// # 示例
    ///
    /// ```no_run
    /// use flow_run::core::context::ExecutionContext;
    /// use flow_run::executors::condition::ConditionExecutor;
    ///
    /// let context = ExecutionContext::new(&workflow, inputs);
    /// let value = condition_executor.evaluate_expression("${{ inputs.enabled }}", &context)?;
    /// ```
    fn evaluate_expression(
        &self,
        expression: &str,
        context: &ExecutionContext,
    ) -> Result<serde_json::Value, WorkflowError> {
        context.evaluate(expression)
    }

    /// 执行分支
    ///
    /// 执行分支中的所有步骤
    ///
    /// # 参数
    ///
    /// * `executor` - 执行器
    /// * `steps` - 分支步骤列表
    /// * `context` - 执行上下文
    ///
    /// # 返回
    ///
    /// 返回所有步骤的执行结果列表
    async fn execute_branch(
        &self,
        executor: &Arc<dyn Executor>,
        steps: &[StepDefinition],
        context: &ExecutionContext,
    ) -> Result<Vec<StepResult>, WorkflowError> {
        let mut results = Vec::new();

        // 依次执行分支中的每个步骤
        for step in steps {
            let result = executor.execute(step, context).await?;
            let is_failed = result.status == StepStatus::Failed;
            results.push(result);

            // 如果步骤失败，停止执行后续步骤
            if is_failed {
                return Ok(results);
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_condition_executor() {
        // 这个测试需要模拟一个 Executor
        // 在实际实现中，可以使用 mock 实例
        // 这里只是展示测试结构
    }

    #[test]
    fn test_branch_response_serialization() {
        // 测试分支响应可以正确序列化
        let response = BranchResponse {
            branch: "then".to_string(),
            results: vec![],
        };

        let json_value = serde_json::to_value(&response);
        assert!(json_value.is_ok());

        if let Ok(value) = json_value {
            assert_eq!(value["branch"], "then");
            assert!(value["results"].is_array());
        }
    }

    #[test]
    fn test_evaluate_boolean_condition() {
        // 测试布尔条件求值
        // 这个测试需要完整的上下文和执行器设置
        // 这里只是展示测试结构
    }
}
