/// 工作流引擎统一错误类型
#[derive(Debug, thiserror::Error)]
pub enum WorkflowError {
    // ========== Axxx - 工作流错误 ==========
    /// A001: 工作流文件不存在
    #[error("A001: 工作流文件不存在: {path}")]
    WorkflowFileNotFound { path: String },

    /// A002: YAML 解析失败
    #[error("A002: YAML 解析失败: {reason}")]
    YamlParseError { reason: String },

    /// A003: Schema 验证失败
    #[error("A003: Schema 验证失败: {message}")]
    SchemaValidationError { message: String },

    /// A004: 循环依赖
    #[error("A004: 检测到循环依赖")]
    CycleDetected,

    /// A005: 步骤未定义
    #[error("A005: 步骤未定义: {step_id}")]
    StepNotFound { step_id: String },

    /// A006: 工作流版本不兼容
    #[error("A006: 工作流版本不兼容: {version}")]
    IncompatibleVersion { version: String },

    /// A007: 工作流文件语法变更
    #[error("A007: 工作流文件语法变更，需重新验证")]
    SyntaxChanged,

    // ========== Bxxx - 执行错误 ==========
    /// B001: HTTP 请求失败
    #[error("B001: HTTP 请求失败: {status_code} - {message}")]
    HttpRequestFailed { status_code: u16, message: String },

    /// B002: Shell 命令失败
    #[error("B002: Shell 命令失败: 退出码 {exit_code}, 错误: {stderr}")]
    ShellCommandFailed { exit_code: i32, stderr: String },

    /// B003: 超时
    #[error("B003: 操作超时 ({timeout_ms}ms)")]
    Timeout { timeout_ms: u64 },

    /// B004: 条件表达式错误
    #[error("B004: 条件表达式错误: {expression} - {reason}")]
    ConditionError { expression: String, reason: String },

    /// B005: 循环超过最大迭代次数
    #[error("B005: 循环超过最大迭代次数: {max_iterations}")]
    MaxIterationsExceeded { max_iterations: u32 },

    /// B006: 子工作流失败
    #[error("B006: 子工作流失败: {workflow} - {reason}")]
    SubWorkflowFailed { workflow: String, reason: String },

    /// B007: 并发限制
    #[error("B007: 并发限制: 当前 {current}，最大 {max}")]
    ConcurrencyLimit { current: usize, max: usize },

    /// B008: 速率限制
    #[error("B008: 速率限制: {message}")]
    RateLimitExceeded { message: String },

    // ========== Cxxx - 检查点错误 ==========
    /// C001: 检查点不存在
    #[error("C001: 检查点不存在: {checkpoint_id}")]
    CheckpointNotFound { checkpoint_id: String },

    /// C002: 检查点损坏
    #[error("C002: 检查点损坏: {checkpoint_id}")]
    CheckpointCorrupted { checkpoint_id: String },

    /// C003: 检查点写入失败
    #[error("C003: 检查点写入失败: {path} - {reason}")]
    CheckpointWriteFailed { path: String, reason: String },

    /// C004: 检查点版本不兼容
    #[error("C004: 检查点版本不兼容: {version}")]
    CheckpointVersionIncompatible { version: String },

    /// C005: 超时已过期
    #[error("C005: 超时已过期，无法恢复: {checkpoint_id}")]
    TimeoutExpired { checkpoint_id: String },

    // ========== Dxxx - 模板错误 ==========
    /// D001: 模板语法错误
    #[error("D001: 模板语法错误: {expression}")]
    TemplateSyntaxError { expression: String },

    /// D002: 变量未定义
    #[error("D002: 变量未定义: {variable}")]
    UndefinedVariable { variable: String },

    /// D003: 路径不存在
    #[error("D003: 路径不存在: {path}")]
    PathNotFound { path: String },

    /// D004: 类型不匹配
    #[error("D004: 类型不匹配: 期望 {expected}，实际 {actual}")]
    TypeMismatch { expected: String, actual: String },

    /// D005: 过滤器不存在
    #[error("D005: 过滤器不存在: {filter}")]
    FilterNotFound { filter: String },

    // ========== Exxx - 审批错误 ==========
    /// E001: 审批被拒绝
    #[error("E001: 审批被拒绝: {step_id} - {reason}")]
    ApprovalRejected { step_id: String, reason: String },

    /// E002: 审批超时
    #[error("E002: 审批超时: {step_id}")]
    ApprovalTimeout { step_id: String },

    /// E003: 审批人未授权
    #[error("E003: 审批人未授权: {approver}")]
    ApproverUnauthorized { approver: String },

    /// E004: 审批服务不可用
    #[error("E004: 审批服务不可用: {reason}")]
    ApprovalServiceUnavailable { reason: String },

    /// E005: 自动审批条件无效
    #[error("E005: 自动审批条件无效: {condition}")]
    InvalidAutoApproveCondition { condition: String },

    // ========== Fxxx - 钩子错误 ==========
    /// F001: 钩子执行超时
    #[error("F001: 钩子执行超时: {hook}")]
    HookTimeout { hook: String },

    /// F002: 钩子命令失败
    #[error("F002: 钩子命令失败: {command} - {reason}")]
    HookCommandFailed { command: String, reason: String },

    /// F003: 钩子 HTTP 请求失败
    #[error("F003: 钩子 HTTP 请求失败: {url} - {reason}")]
    HookHttpRequestFailed { url: String, reason: String },

    /// F004: 钩子配置无效
    #[error("F004: 钩子配置无效: {reason}")]
    InvalidHookConfig { reason: String },

    // ========== Gxxx - 触发器错误 ==========
    /// G001: Webhook 签名验证失败
    #[error("G001: Webhook 签名验证失败")]
    WebhookSignatureInvalid,

    /// G002: Cron 表达式无效
    #[error("G002: Cron 表达式无效: {expression}")]
    InvalidCronExpression { expression: String },

    /// G003: 文件监听路径不存在
    #[error("G003: 文件监听路径不存在: {path}")]
    FileWatchPathNotFound { path: String },

    /// G004: 触发器已禁用
    #[error("G004: 触发器已禁用: {trigger_id}")]
    TriggerDisabled { trigger_id: String },

    /// G005: 工作流不存在
    #[error("G005: 工作流不存在: {workflow}")]
    WorkflowNotFound { workflow: String },

    // ========== Hxxx - 运行上下文错误 ==========
    /// H001: 运行上下文不存在（--from-step 找不到保存的上下文）
    #[error("H001: 未找到运行上下文，请先正常执行工作流: {workflow_file}")]
    RunContextNotFound { workflow_file: String },

    /// H002: 运行上下文加载失败
    #[error("H002: 运行上下文加载失败: {path} - {reason}")]
    RunContextLoadFailed { path: String, reason: String },

    /// H003: 指定的步骤 ID 不存在于工作流中
    #[error("H003: 步骤 '{step_id}' 不存在于工作流中。可用步骤: {available}")]
    FromStepNotFound { step_id: String, available: String },

    // ========== 通用错误 ==========
    /// IO 错误
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    /// JSON 序列化/反序列化错误
    #[error("JSON 错误: {0}")]
    Json(#[from] serde_json::Error),

    /// YAML 解析错误
    #[error("YAML 错误: {0}")]
    Yaml(#[from] serde_yaml::Error),

    /// HTTP 客户端错误
    #[error("HTTP 客户端错误: {0}")]
    HttpClient(#[from] reqwest::Error),

    /// 正则表达式错误
    #[error("正则表达式错误: {0}")]
    Regex(#[from] regex::Error),

    /// 通用错误
    #[error("{0}")]
    Other(String),
}

impl From<CheckpointError> for WorkflowError {
    fn from(err: CheckpointError) -> Self {
        match err {
            CheckpointError::NotFound(id) => {
                WorkflowError::CheckpointNotFound { checkpoint_id: id }
            }
            CheckpointError::Corrupted(id) => {
                WorkflowError::CheckpointCorrupted { checkpoint_id: id }
            }
            CheckpointError::Io(e) => WorkflowError::Io(e),
            CheckpointError::Json(e) => WorkflowError::Json(e),
        }
    }
}

/// 重试错误类型
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum RetryError {
    /// HTTP 状态码错误
    #[error("HTTP 状态码: {0}")]
    HttpStatus(u16),

    /// 网络错误
    #[error("网络错误: {0}")]
    Network(String),

    /// 超过最大重试次数
    #[error("超过最大重试次数")]
    MaxAttemptsExceeded,

    /// 不可重试的错误
    #[error("不可重试: {0}")]
    NotRetryable(String),
}

/// 检查点错误类型
#[derive(Debug, thiserror::Error)]
pub enum CheckpointError {
    /// 检查点不存在
    #[error("检查点不存在: {0}")]
    NotFound(String),

    /// 检查点损坏
    #[error("检查点损坏: {0}")]
    Corrupted(String),

    /// IO 错误
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    /// JSON 错误
    #[error("JSON 错误: {0}")]
    Json(#[from] serde_json::Error),
}

/// 模板错误类型
#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    /// 语法错误
    #[error("语法错误: {0}")]
    SyntaxError(String),

    /// 变量未定义
    #[error("变量未定义: {0}")]
    UndefinedVariable(String),

    /// 过滤器不存在
    #[error("过滤器不存在: {0}")]
    FilterNotFound(String),

    /// 类型错误
    #[error("类型错误: {0}")]
    TypeError(String),

    /// 路径不存在
    #[error("路径不存在: {0}")]
    PathNotFound(String),
}

/// 触发器错误类型
#[derive(Debug, thiserror::Error)]
pub enum TriggerError {
    /// 触发器未找到
    #[error("触发器未找到: {0}")]
    NotFound(String),

    /// 触发器已禁用
    #[error("触发器已禁用: {0}")]
    Disabled(String),

    /// Cron 表达式无效
    #[error("Cron 表达式无效: {0}")]
    InvalidCron(String),

    /// 文件监听错误
    #[error("文件监听错误: {0}")]
    FileWatchError(String),

    /// IO 错误
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

/// 速率限制错误类型
#[derive(Debug, thiserror::Error)]
pub enum RateLimitError {
    /// 被限流
    #[error("被限流")]
    Limited,

    /// 内部错误
    #[error("内部错误: {0}")]
    Internal(String),
}
