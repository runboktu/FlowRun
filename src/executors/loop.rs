use crate::core::context::ExecutionContext;
use crate::core::types::*;
use crate::executors::Executor;
use crate::utils::error::WorkflowError;
use std::sync::Arc;

/// 循环执行器
///
/// 负责执行三种类型的循环：
/// - ForEach: 遍历数组，对每个元素执行一次循环体
/// - While: 条件循环，在条件为真时重复执行
/// - Range: 范围循环，在指定范围内迭代
pub struct LoopExecutor {
    /// 内部执行器（用于执行循环体）
    inner_executor: Arc<dyn Executor>,
}

impl LoopExecutor {
    /// 创建新的循环执行器
    ///
    /// # 参数
    ///
    /// * `inner_executor` - 内部执行器，用于执行循环体步骤
    ///
    /// # 返回
    ///
    /// 返回新创建的循环执行器
    pub fn new(inner_executor: Arc<dyn Executor>) -> Self {
        Self { inner_executor }
    }

    /// 执行循环步骤
    ///
    /// # 参数
    ///
    /// * `step` - 步骤定义
    /// * `context` - 执行上下文
    ///
    /// # 返回
    ///
    /// 返回循环执行结果，包含所有迭代的结果
    pub async fn execute(
        &self,
        step: &StepDefinition,
        context: &ExecutionContext,
    ) -> Result<StepResult, WorkflowError> {
        let step_id = &step.id;
        let started_at = chrono::Utc::now();

        // 获取循环配置
        let loop_config = step.r#loop.as_ref().ok_or_else(|| {
            WorkflowError::Other(format!("步骤 {} 缺少 loop 配置", step_id))
        })?;

        // 获取循环体步骤
        let do_steps = step.do_steps.as_ref().ok_or_else(|| {
            WorkflowError::Other(format!("步骤 {} 缺少 do_steps 配置", step_id))
        })?;

        // 根据循环类型执行
        let results = match loop_config {
            LoopConfig::ForEach { over, r#as } => {
                let items = self.evaluate_array(over, context)?;
                self.execute_for_each(do_steps, &items, r#as, context)
                    .await?
            }
            LoopConfig::While {
                condition,
                max_iterations,
            } => {
                self.execute_while(do_steps, condition, *max_iterations, context)
                    .await?
            }
            LoopConfig::Range { start, end } => {
                self.execute_range(do_steps, *start, *end, context)
                    .await?
            }
        };

        let completed_at = chrono::Utc::now();
        let duration_ms = (completed_at - started_at).num_milliseconds() as u64;

        // 构建循环响应
        let loop_response = LoopResponse {
            iterations: results.len(),
            results,
        };

        Ok(StepResult {
            step_id: step_id.clone(),
            status: StepStatus::Success,
            started_at,
            completed_at: Some(completed_at),
            duration_ms: Some(duration_ms),
            output: Some(serde_json::to_value(loop_response).map_err(|e| {
                WorkflowError::Other(format!("序列化循环结果失败: {}", e))
            })?),
            error: None,
        })
    }

    /// 执行 ForEach 循环
    ///
    /// 遍历数组，对每个元素执行一次循环体
    ///
    /// # 参数
    ///
    /// * `do_steps` - 循环体步骤
    /// * `items` - 要遍历的数组
    /// * `as_name` - 循环变量名
    /// * `context` - 父上下文
    ///
    /// # 返回
    ///
    /// 返回所有迭代的执行结果列表
    async fn execute_for_each(
        &self,
        do_steps: &[StepDefinition],
        items: &[serde_json::Value],
        as_name: &str,
        context: &ExecutionContext,
    ) -> Result<Vec<StepResult>, WorkflowError> {
        let mut results = Vec::new();

        for (index, item) in items.iter().enumerate() {
            // 创建独立的循环上下文
            let mut loop_context = self.create_loop_context(context);

            // 设置循环变量（使用 as_name 作为变量名）
            loop_context.set_variable(as_name.to_string(), item.clone());

            // 设置索引变量
            loop_context.set_variable("index".to_string(), serde_json::json!(index));

            // 执行循环体
            let iteration_result = self
                .execute_loop_body(do_steps, &loop_context)
                .await?;

            results.extend(iteration_result);

            // 检查是否需要中断（通过检查特殊变量）
            if let Some(break_loop) = loop_context.get_variable("break_loop") {
                if break_loop.as_bool().unwrap_or(false) {
                    break;
                }
            }
        }

        Ok(results)
    }

    /// 执行 While 循环
    ///
    /// 条件循环，在条件为真时重复执行
    ///
    /// # 参数
    ///
    /// * `do_steps` - 循环体步骤
    /// * `condition` - 条件表达式
    /// * `max_iterations` - 最大迭代次数（可选）
    /// * `context` - 父上下文
    ///
    /// # 返回
    ///
    /// 返回所有迭代的执行结果列表
    async fn execute_while(
        &self,
        do_steps: &[StepDefinition],
        condition: &str,
        max_iterations: Option<u32>,
        context: &ExecutionContext,
    ) -> Result<Vec<StepResult>, WorkflowError> {
        let mut results = Vec::new();
        let mut iteration_count = 0u32;

        loop {
            // 检查最大迭代次数
            if let Some(max) = max_iterations {
                if iteration_count >= max {
                    return Err(WorkflowError::MaxIterationsExceeded {
                        max_iterations: max,
                    });
                }
            }

            // 求值条件表达式
            let condition_value = self.evaluate_expression(condition, context)?;
            let should_continue = condition_value.as_bool().ok_or_else(|| {
                WorkflowError::ConditionError {
                    expression: condition.to_string(),
                    reason: "条件表达式的求值结果不是布尔值".to_string(),
                }
            })?;

            // 如果条件为假，退出循环
            if !should_continue {
                break;
            }

            // 创建独立的循环上下文
            let loop_context = self.create_loop_context(context);

            // 执行循环体
            let iteration_result = self
                .execute_loop_body(do_steps, &loop_context)
                .await?;

            results.extend(iteration_result);
            iteration_count += 1;

            // 检查是否需要中断（通过检查特殊变量）
            if let Some(break_loop) = loop_context.get_variable("break_loop") {
                if break_loop.as_bool().unwrap_or(false) {
                    break;
                }
            }
        }

        Ok(results)
    }

    /// 执行 Range 循环
    ///
    /// 在指定范围内迭代
    ///
    /// # 参数
    ///
    /// * `do_steps` - 循环体步骤
    /// * `start` - 起始值
    /// * `end` - 结束值
    /// * `context` - 父上下文
    ///
    /// # 返回
    ///
    /// 返回所有迭代的执行结果列表
    async fn execute_range(
        &self,
        do_steps: &[StepDefinition],
        start: i64,
        end: i64,
        context: &ExecutionContext,
    ) -> Result<Vec<StepResult>, WorkflowError> {
        let mut results = Vec::new();

        for i in start..end {
            // 创建独立的循环上下文
            let mut loop_context = self.create_loop_context(context);

            // 设置循环变量
            loop_context.set_variable("value".to_string(), serde_json::json!(i));
            loop_context.set_variable("index".to_string(), serde_json::json!(i - start));

            // 执行循环体
            let iteration_result = self
                .execute_loop_body(do_steps, &loop_context)
                .await?;

            results.extend(iteration_result);

            // 检查是否需要中断（通过检查特殊变量）
            if let Some(break_loop) = loop_context.get_variable("break_loop") {
                if break_loop.as_bool().unwrap_or(false) {
                    break;
                }
            }
        }

        Ok(results)
    }

    /// 创建独立的循环上下文
    ///
    /// 克隆父上下文，确保每次迭代都有独立的作用域
    ///
    /// # 参数
    ///
    /// * `parent_context` - 父上下文
    ///
    /// # 返回
    ///
    /// 返回新的循环上下文
    fn create_loop_context(&self, parent_context: &ExecutionContext) -> ExecutionContext {
        // 创建新的上下文（在实际实现中，应该创建一个克隆版本）
        // 由于 ExecutionContext 没有实现 Clone，这里需要手动处理
        // 简化实现：创建一个类似的上下文，复制必要的字段

        // 注意：这是一个简化实现，实际中应该实现 Clone trait
        // 或者使用 Arc 包装共享状态
        ExecutionContext {
            workflow_id: parent_context.workflow_id.clone(),
            workflow_name: parent_context.workflow_name.clone(),
            execution_id: parent_context.execution_id.clone(),
            started_at: parent_context.started_at,
            inputs: parent_context.inputs.clone(),
            step_outputs: parent_context.step_outputs.clone(),
            completed_steps: parent_context.completed_steps.clone(),
            failed_steps: parent_context.failed_steps.clone(),
            variables: parent_context.variables.clone(),
            current_batch: parent_context.current_batch,
        }
    }

    /// 执行循环体
    ///
    /// 执行循环体中的所有步骤
    ///
    /// # 参数
    ///
    /// * `do_steps` - 循环体步骤
    /// * `loop_context` - 循环上下文
    ///
    /// # 返回
    ///
    /// 返回所有步骤的执行结果列表
    async fn execute_loop_body(
        &self,
        do_steps: &[StepDefinition],
        loop_context: &ExecutionContext,
    ) -> Result<Vec<StepResult>, WorkflowError> {
        let mut results = Vec::new();

        // 依次执行循环体中的每个步骤
        for sub_step in do_steps {
            // 使用内部执行器执行步骤
            let result = self.inner_executor.execute(sub_step, loop_context).await?;
            results.push(result);
        }

        Ok(results)
    }

    /// 求值数组表达式
    ///
    /// 从上下文中求值数组表达式
    ///
    /// # 参数
    ///
    /// * `expression` - 数组表达式
    /// * `context` - 执行上下文
    ///
    /// # 返回
    ///
    /// 返回数组值
    fn evaluate_array(
        &self,
        expression: &str,
        context: &ExecutionContext,
    ) -> Result<Vec<serde_json::Value>, WorkflowError> {
        let value = context.evaluate(expression)?;

        match value {
            serde_json::Value::Array(arr) => Ok(arr),
            _ => Err(WorkflowError::Other(format!(
                "表达式 '{}' 的求值结果不是数组",
                expression
            ))),
        }
    }

    /// 求值表达式
    ///
    /// 使用模板引擎求值表达式
    ///
    /// # 参数
    ///
    /// * `expression` - 表达式字符串
    /// * `context` - 执行上下文
    ///
    /// # 返回
    ///
    /// 返回求值结果
    fn evaluate_expression(
        &self,
        expression: &str,
        context: &ExecutionContext,
    ) -> Result<serde_json::Value, WorkflowError> {
        context.evaluate(expression)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_loop_executor() {
        // 这个测试需要模拟一个 Executor
        // 在实际实现中，可以使用 mock 实例
        // 这里只是展示测试结构
    }

    #[test]
    fn test_loop_config_variants() {
        let foreach_config = LoopConfig::ForEach {
            over: "${{ inputs.items }}".to_string(),
            r#as: "item".to_string(),
        };
        let while_config = LoopConfig::While {
            condition: "${{ variables.counter < 10 }}".to_string(),
            max_iterations: Some(100),
        };
        let range_config = LoopConfig::Range {
            start: 0,
            end: 10,
        };

        // 验证各种循环配置可以正确创建
        match foreach_config {
            LoopConfig::ForEach { over, r#as } => {
                assert_eq!(over, "${{ inputs.items }}");
                assert_eq!(r#as, "item");
            }
            _ => panic!("错误的循环配置类型"),
        }

        match while_config {
            LoopConfig::While { condition, max_iterations } => {
                assert_eq!(condition, "${{ variables.counter < 10 }}");
                assert_eq!(max_iterations, Some(100));
            }
            _ => panic!("错误的循环配置类型"),
        }

        match range_config {
            LoopConfig::Range { start, end } => {
                assert_eq!(start, 0);
                assert_eq!(end, 10);
            }
            _ => panic!("错误的循环配置类型"),
        }
    }
}
