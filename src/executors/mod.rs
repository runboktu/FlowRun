use crate::core::context::ExecutionContext;
use crate::core::types::*;
use crate::utils::error::WorkflowError;

pub mod http;
pub mod shell;
pub mod r#loop;
pub mod condition;
pub mod workflow;
pub mod approve;

/// Executor trait 的定义
///
/// 所有执行器必须实现此 trait
#[async_trait::async_trait]
pub trait Executor: Send + Sync {
    /// 执行步骤
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
    ) -> Result<StepResult, WorkflowError>;
}
