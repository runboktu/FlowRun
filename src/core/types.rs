use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 工作流步骤的唯一标识符
pub type StepId = String;

/// 工作流定义的顶层结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    /// 工作流名称
    pub name: String,
    /// 工作流描述
    pub description: Option<String>,
    /// 工作流版本
    pub version: Option<String>,
    /// 全局配置
    pub config: Option<WorkflowConfig>,
    /// 输入参数定义
    pub inputs: Option<Vec<InputDefinition>>,
    /// 输出定义
    pub outputs: Option<HashMap<String, String>>,
    /// 步骤列表
    pub steps: Vec<StepDefinition>,
    /// 钩子配置
    pub on: Option<HooksConfig>,
    /// 触发器配置
    pub trigger: Option<Vec<TriggerConfig>>,
    /// 工作流变量
    pub variables: Option<HashMap<String, serde_json::Value>>,
}

/// 工作流全局配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowConfig {
    /// 全局超时时间（秒）
    pub timeout: Option<String>,
    /// 重试策略
    pub retry: Option<RetryConfig>,
    /// 失败处理策略
    pub on_failure: Option<OnFailureStrategy>,
    /// 检查点文件路径
    pub checkpoint: Option<String>,
    /// 最大并发数
    pub max_concurrent: Option<usize>,
    /// 超时策略
    pub timeout_strategy: Option<TimeoutStrategy>,
    /// 恢复配置
    pub resume: Option<ResumeConfig>,
    /// 历史记录配置
    pub history: Option<HistoryConfig>,
    /// 清理配置
    pub cleanup: Option<CleanupConfig>,
    /// 钩子配置
    pub hooks: Option<HookGlobalConfig>,
}

impl Default for WorkflowConfig {
    fn default() -> Self {
        Self {
            timeout: None,
            retry: None,
            on_failure: None,
            checkpoint: None,
            max_concurrent: None,
            timeout_strategy: None,
            resume: None,
            history: None,
            cleanup: None,
            hooks: None,
        }
    }
}

/// 重试配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// 最大重试次数
    pub max_attempts: u32,
    /// 退避策略
    pub strategy: Option<BackoffStrategy>,
    /// 初始延迟（秒）
    pub initial_delay: Option<f64>,
    /// 最大延迟（秒）
    pub max_delay: Option<f64>,
    /// 是否启用抖动
    pub jitter: Option<bool>,
    /// 指数退避因子（仅 exponential 策略生效）
    pub factor: Option<f64>,
}

/// 退避策略枚举
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackoffStrategy {
    /// 固定延迟
    Fixed,
    /// 指数退避
    Exponential,
    /// 斐波那契退避
    Fibonacci,
}

/// 失败处理策略
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnFailureStrategy {
    /// 中止执行
    Abort,
    /// 暂停（保存检查点）
    Pause,
    /// 继续执行
    Continue,
}

/// 超时策略
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeoutStrategy {
    /// 严格超时
    Hard,
    /// 软超时（可续期）
    Soft,
}

/// 恢复配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeConfig {
    /// 超时模式
    pub timeout_mode: TimeoutMode,
    /// 宽限期（秒）
    pub grace_period: Option<String>,
}

/// 超时模式
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeoutMode {
    /// 继承剩余超时
    Inherit,
    /// 重新计时
    Reset,
}

/// 历史记录配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryConfig {
    /// 保留天数
    pub retention_days: Option<u32>,
    /// 保留的成功执行数
    pub keep_successful: Option<u32>,
    /// 保留的失败执行数
    pub keep_failed: Option<u32>,
    /// 保留的暂停执行数
    pub keep_paused: Option<u32>,
}

/// 清理配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupConfig {
    /// 清理计划（cron 表达式）
    pub schedule: Option<String>,
    /// 最大检查点数量
    pub max_checkpoints: Option<u32>,
}

/// 钩子全局配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookGlobalConfig {
    /// 钩子失败时是否继续
    pub continue_on_error: Option<bool>,
    /// 单个钩子超时
    pub timeout: Option<String>,
    /// 是否并行执行钩子
    pub parallel: Option<bool>,
    /// 钩子失败是否重试
    pub retry_on_failure: Option<bool>,
}

/// 输入参数定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputDefinition {
    /// 参数名称
    pub name: String,
    /// 参数类型
    pub r#type: Option<String>,
    /// 是否必需
    pub required: Option<bool>,
    /// 默认值
    pub default: Option<serde_json::Value>,
    /// 枚举值
    pub r#enum: Option<Vec<String>>,
    /// 正则验证
    pub regex: Option<String>,
    /// 描述
    pub description: Option<String>,
}

/// 步骤定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepDefinition {
    /// 步骤 ID
    pub id: String,
    /// 步骤名称
    pub name: Option<String>,
    /// 步骤类型
    pub r#type: StepType,
    /// 依赖的步骤
    pub depends_on: Option<Vec<String>>,
    /// 期望结果验证
    pub expect: Option<ExpectConfig>,
    /// 步骤级重试配置
    pub retry: Option<RetryConfig>,
    /// 超时时间
    pub timeout: Option<String>,
    /// 钩子配置
    pub hooks: Option<StepHooksConfig>,

    // HTTP 步骤特有字段
    /// HTTP API 地址
    pub api: Option<String>,
    /// HTTP 方法
    pub method: Option<String>,
    /// HTTP 请求头
    pub headers: Option<HashMap<String, String>>,
    /// HTTP 请求体
    pub body: Option<serde_json::Value>,
    /// 缓存配置
    pub cache: Option<CacheConfig>,

    // Shell 步骤特有字段
    /// Shell 命令
    pub run: Option<String>,
    /// 环境变量
    pub env: Option<HashMap<String, String>>,
    /// 安全模式
    pub safe_mode: Option<SafeMode>,
    /// 允许的命令
    pub allowed_commands: Option<Vec<String>>,

    // Parallel 步骤特有字段
    /// 子步骤
    pub steps: Option<Vec<StepDefinition>>,
    /// 步骤级最大并发数
    pub max_concurrent: Option<usize>,
    /// 速率限制配置
    pub rate_limit: Option<RateLimitConfig>,

    // Loop 步骤特有字段
    /// 循环配置
    pub r#loop: Option<LoopConfig>,
    /// 循环体步骤
    pub do_steps: Option<Vec<StepDefinition>>,

    // Condition 步骤特有字段
    /// 条件表达式
    pub expression: Option<String>,
    /// Then 分支
    pub then_steps: Option<Vec<StepDefinition>>,
    /// Else 分支
    pub else_steps: Option<Vec<StepDefinition>>,

    // Workflow 步骤特有字段
    /// 子工作流文件路径
    pub workflow: Option<String>,
    /// 子工作流输入
    pub inputs: Option<HashMap<String, String>>,
    /// 错误策略
    pub error_strategy: Option<SubWorkflowErrorStrategy>,
    /// 是否隔离上下文
    pub isolation: Option<bool>,
    /// 透传变量
    pub passthrough_vars: Option<Vec<String>>,

    // Approve 步骤特有字段
    /// 审批消息
    pub message: Option<String>,
    /// 审批人列表
    pub approvers: Option<Vec<String>>,
    /// 是否需要审批意见
    pub require_comment: Option<bool>,
    /// 超时处理策略
    pub on_timeout: Option<ApprovalTimeoutStrategy>,
    /// 自动审批条件
    pub auto_approve_on: Option<Vec<AutoApproveRule>>,
}

/// 步骤类型枚举
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepType {
    /// HTTP 请求
    Http,
    /// Shell 命令
    Shell,
    /// 并行执行
    Parallel,
    /// 循环
    Loop,
    /// 条件分支
    Condition,
    /// 子工作流
    Workflow,
    /// 人工审批
    Approve,
}

/// 安全模式
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafeMode {
    /// 严格模式
    Strict,
    /// 警告模式
    Warn,
    /// 无限制
    None,
}

/// 期望结果配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectConfig {
    /// 期望的状态码（HTTP）
    pub status_code: Option<u16>,
    /// 期望的退出码（Shell）
    pub exit_code: Option<i32>,
    /// 期望的响应体匹配
    pub body_contains: Option<String>,
    /// 期望的 JSON 路径存在
    pub json_path: Option<String>,
}

/// 缓存配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// 缓存 TTL（秒）
    pub ttl: u64,
}

/// 速率限制配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// 每秒请求数
    pub requests_per_second: f64,
    /// 突发容量
    pub burst: u32,
}

/// 循环配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LoopConfig {
    /// ForEach 循环
    ForEach {
        /// 遍历的数组表达式
        over: String,
        /// 循环变量名
        r#as: String,
    },
    /// While 循环
    While {
        /// 条件表达式
        condition: String,
        /// 最大迭代次数
        max_iterations: Option<u32>,
    },
    /// Range 循环
    Range {
        /// 起始值
        start: i64,
        /// 结束值
        end: i64,
    },
}

/// 子工作流错误策略
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubWorkflowErrorStrategy {
    /// 向上传播
    Propagate,
    /// 继续执行
    Continue,
    /// 重试
    Retry,
    /// 忽略
    Ignore,
}

/// 审批超时策略
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalTimeoutStrategy {
    /// 中止
    Abort,
    /// 暂停
    Pause,
    /// 继续
    Continue,
}

/// 自动审批规则
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoApproveRule {
    /// 条件表达式
    pub condition: String,
    /// 自动审批原因
    pub reason: String,
}

/// 步骤钩子配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepHooksConfig {
    /// 执行前钩子
    pub before: Option<Vec<HookAction>>,
    /// 执行后钩子
    pub after: Option<Vec<HookAction>>,
    /// 成功时钩子
    pub on_success: Option<Vec<HookAction>>,
    /// 失败时钩子
    pub on_error: Option<Vec<HookAction>>,
}

/// 全局钩子配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HooksConfig {
    /// 工作流开始时
    pub workflow_start: Option<Vec<HookAction>>,
    /// 工作流成功时
    pub workflow_success: Option<Vec<HookAction>>,
    /// 工作流失败时
    pub workflow_failure: Option<Vec<HookAction>>,
    /// 工作流暂停时
    pub workflow_pause: Option<Vec<HookAction>>,
    /// 工作流恢复时
    pub workflow_resume: Option<Vec<HookAction>>,
}

/// 钩子动作
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HookAction {
    /// 执行命令
    Run { run: String },
    /// HTTP 请求
    Http { http: HttpHookConfig },
}

/// HTTP 钩子配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpHookConfig {
    /// API 地址
    pub api: String,
    /// HTTP 方法
    pub method: Option<String>,
    /// 请求体
    pub body: Option<serde_json::Value>,
}

/// 触发器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerConfig {
    /// 触发器类型
    pub r#type: TriggerType,
    /// Cron 计划
    pub schedule: Option<String>,
    /// 时区
    pub timezone: Option<String>,
    /// Webhook 路径
    pub path: Option<String>,
    /// Webhook 密钥
    pub secret: Option<String>,
    /// 文件监听路径
    pub patterns: Option<Vec<String>>,
    /// 防抖时间
    pub debounce: Option<String>,
    /// 事件类型
    pub event: Option<String>,
    /// 源工作流
    pub source_workflow: Option<String>,
    /// 触发器输入
    pub inputs: Option<HashMap<String, String>>,
}

/// 触发器类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerType {
    /// Cron 定时
    Cron,
    /// Webhook
    Webhook,
    /// 文件监听
    FileWatch,
    /// 工作流事件
    WorkflowEvent,
}

/// 步骤执行状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    /// 成功
    Success,
    /// 失败
    Failed,
    /// 跳过
    Skipped,
    /// 运行中
    Running,
    /// 等待中
    Pending,
}

/// 步骤执行结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    /// 步骤 ID
    pub step_id: StepId,
    /// 执行状态
    pub status: StepStatus,
    /// 开始时间
    pub started_at: DateTime<Utc>,
    /// 完成时间
    pub completed_at: Option<DateTime<Utc>>,
    /// 耗时（毫秒）
    pub duration_ms: Option<u64>,
    /// 步骤输出
    pub output: Option<serde_json::Value>,
    /// 错误信息
    pub error: Option<StepError>,
}

/// 步骤错误信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepError {
    /// 错误代码
    pub code: String,
    /// 错误消息
    pub message: String,
    /// 修复建议
    pub fix: Option<String>,
}

/// HTTP 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse {
    /// 状态码
    pub status_code: u16,
    /// 响应头
    pub headers: HashMap<String, String>,
    /// 响应体
    pub body: serde_json::Value,
}

/// Shell 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellResponse {
    /// 退出码
    pub exit_code: i32,
    /// 标准输出
    pub stdout: String,
    /// 标准错误
    pub stderr: String,
}

/// 循环响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopResponse {
    /// 迭代次数
    pub iterations: usize,
    /// 每次迭代的结果
    pub results: Vec<StepResult>,
}

/// 分支响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchResponse {
    /// 执行的分支
    pub branch: String,
    /// 分支内的步骤结果
    pub results: Vec<StepResult>,
}

/// 工作流响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowResponse {
    /// 子工作流输出
    pub outputs: HashMap<String, serde_json::Value>,
    /// 子工作流指标
    pub metrics: Option<ExecutionMetrics>,
}

/// 审批响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalResponse {
    /// 审批状态
    pub status: ApprovalStatus,
    /// 审批人
    pub approved_by: String,
    /// 审批意见
    pub comment: Option<String>,
}

/// 审批状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    /// 等待中
    Pending,
    /// 已批准
    Approved,
    /// 已拒绝
    Rejected,
    /// 已超时
    TimedOut,
    /// 自动批准
    AutoApproved,
}

/// 工作流执行状态
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStatus {
    /// 成功
    Success,
    /// 暂停
    Paused,
    /// 失败
    Failed,
    /// 试运行
    DryRun,
}

/// 工作流执行结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowResult {
    /// 执行状态
    pub status: WorkflowStatus,
    /// 工作流信息
    pub workflow: WorkflowInfo,
    /// 执行信息
    pub execution: ExecutionInfo,
    /// 步骤结果列表
    pub steps: Vec<StepResult>,
    /// 输出
    pub outputs: Option<HashMap<String, serde_json::Value>>,
    /// 执行指标
    pub metrics: ExecutionMetrics,
    /// 错误列表
    pub errors: Vec<StepError>,
}

/// 工作流信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowInfo {
    /// 工作流名称
    pub name: String,
    /// 工作流版本
    pub version: Option<String>,
    /// 工作流文件
    pub file: String,
}

/// 执行信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionInfo {
    /// 执行 ID
    pub id: String,
    /// 开始时间
    pub started_at: DateTime<Utc>,
    /// 完成时间
    pub completed_at: Option<DateTime<Utc>>,
    /// 耗时（毫秒）
    pub duration_ms: Option<u64>,
    /// 检查点路径
    pub checkpoint: Option<String>,
}

/// 执行指标
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionMetrics {
    /// 总步骤数
    pub total_steps: usize,
    /// 成功步骤数
    pub success_steps: usize,
    /// 失败步骤数
    pub failed_steps: usize,
    /// 跳过步骤数
    pub skipped_steps: usize,
    /// 总耗时（毫秒）
    pub total_duration_ms: u64,
}

/// 速率限制配置（步骤级）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepRateLimitConfig {
    /// 每秒请求数
    pub requests_per_second: f64,
    /// 突发容量
    pub burst: u32,
}

/// 并发限制器配置
#[derive(Debug, Clone)]
pub struct ConcurrencyLimitConfig {
    /// 最大并发数
    pub max_concurrent: usize,
    /// 速率限制
    pub rate_limit: Option<StepRateLimitConfig>,
}

impl StepResult {
    /// 创建成功的步骤结果
    pub fn success(step_id: impl Into<String>, output: serde_json::Value) -> Self {
        let now = Utc::now();
        Self {
            step_id: step_id.into(),
            status: StepStatus::Success,
            started_at: now,
            completed_at: Some(now),
            duration_ms: Some(0),
            output: Some(output),
            error: None,
        }
    }

    /// 创建失败的步骤结果
    pub fn failed(step_id: impl Into<String>, error: StepError) -> Self {
        let now = Utc::now();
        Self {
            step_id: step_id.into(),
            status: StepStatus::Failed,
            started_at: now,
            completed_at: Some(now),
            duration_ms: Some(0),
            output: None,
            error: Some(error),
        }
    }

    /// 创建跳过的步骤结果
    pub fn skipped(step_id: impl Into<String>, reason: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            step_id: step_id.into(),
            status: StepStatus::Skipped,
            started_at: now,
            completed_at: Some(now),
            duration_ms: Some(0),
            output: Some(serde_json::json!({ "reason": reason.into() })),
            error: None,
        }
    }

    /// 创建带警告的失败结果
    pub fn failed_with_warning(
        step_id: impl Into<String>,
        error: StepError,
        warning: impl Into<String>,
    ) -> Self {
        let mut result = Self::failed(step_id, error);
        result.output = Some(serde_json::json!({ "warning": warning.into() }));
        result
    }

    /// 从缓存创建结果
    pub fn from_cache(step_id: impl Into<String>, cached: serde_json::Value) -> Self {
        let now = Utc::now();
        Self {
            step_id: step_id.into(),
            status: StepStatus::Success,
            started_at: now,
            completed_at: Some(now),
            duration_ms: Some(0),
            output: Some(serde_json::json!({ "cached": true, "data": cached })),
            error: None,
        }
    }

    /// 设置执行时间
    pub fn with_timing(mut self, started_at: DateTime<Utc>, duration_ms: u64) -> Self {
        self.started_at = started_at;
        self.completed_at = Some(started_at + chrono::Duration::milliseconds(duration_ms as i64));
        self.duration_ms = Some(duration_ms);
        self
    }
}
