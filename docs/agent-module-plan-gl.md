# 为 FlowRun 增加 Agent 模块 — 实施计划

> 源码: `gtht-agent/src` (C++) → 目标: `flow-run/src/agent` (Rust)
> 忽略: `android/`, `ios/`, `harmony/` 移动端适配层

---

## 1. 源项目架构概览 (gtht-agent)

gtht-agent 是一个基于 **ReAct 范式** 的 C++ AI Agent 库，核心组件:

| 组件 | 源文件 | 职责 |
|:---|:---|:---|
| **Types** | `types.hpp` | 核心类型定义: `ToolDescriptor`, `AgentStatus`, `Message`, `LLMResponse`, `LLMFunction`, `AgentCallback` |
| **ToolRegistry** | `tool_registry.hpp/cpp` | 工具注册表: 注册/执行/查询工具，维护工具描述和 JSON Schema |
| **ResponseParser** | `response_parser.hpp/cpp` | LLM 响应解析器: XML 解析器 + JSON 解析器，工厂模式切换 |
| **ReActAgent** | `react_agent.hpp/cpp` | 核心推理循环: Thought → Action → Observation → Answer |
| **AgentManager** | `agent_manager.hpp.hpp` | 会话管理器: 单例模式，多会话隔离，线程安全 |
| **ThreadPool** | `thread_pool.hpp/cpp` | 线程池: 任务队列，异步执行 |
| **SystemPromptRenderer** | `system_prompt_renderer.hpp/cpp` | 模板渲染: 替换 `${user_prompt}` 和 `${tool_list}` 占位符 |
| **GTHTAgentUtil** | `gtht_agent_util.hpp/cpp` | 工具类: 构建进度数据 JSON、规范化响应字符串 |
| **Logger** | `logger.hpp/cpp` | 日志系统: 全局日志函数注入，4 级日志 |
| **UUIDGenerator** | `uuid_generator.hpp/cpp` | UUID 生成: 标准 UUID v4 格式 |
| **LLMAdapter** | `llm_adapter.hpp` | LLM 适配器: 仅占位，用户自行实现 `LLMFunction` |

### ReAct 循环核心流程

```
用户输入 → 构建 system prompt（注入工具列表）
         → 序列化消息历史为 JSON
         → 调用 LLMFunction
         → 解析响应（XML/JSON）
         ├─ 含 <final_answer> → 返回最终答案
         ├─ 含 <action>       → 解析工具名+参数 → 执行工具 → 得到 <observation> → 继续循环
         └─ 解析失败          → 返回原始 LLM 响应
```

---

## 2. 目标项目架构 (FlowRun)

FlowRun 是一个 Rust 声明式工作流引擎，当前模块结构:

```
src/
├── lib.rs          # 库入口
├── main.rs         # CLI 入口
├── cli/            # 命令行界面
├── core/           # 核心引擎 (DAG, parser, template, context, types)
├── executors/      # 步骤执行器 (http, shell, loop, condition, workflow, approve)
└── utils/          # 工具函数 (error, retry, checkpoint)
```

**已有依赖 (可复用)**:
- `tokio` — 异步运行时（替代 C++ ThreadPool）
- `serde_json` — JSON 处理（替代 nlohmann/json）
- `uuid` (v4) — UUID 生成（替代 UUIDGenerator）
- `regex` — 正则表达式
- `tracing` — 日志系统（替代自定义 Logger）
- `thiserror`/`anyhow` — 错误处理

---

## 3. 翻译映射方案

### 3.1 模块映射

| C++ 组件 | Rust 目标文件 | 翻译策略 |
|:---|:---|:---|
| `types.hpp` | `src/agent/types.rs` | 结构体 → Rust struct/enum，函数指针 → trait/closure |
| `tool_registry.hpp/cpp` | `src/agent/tool_registry.rs` | `unordered_map` → `HashMap`，`function` → 闭包 |
| `response_parser.hpp/cpp` | `src/agent/response_parser.rs` | 虚基类+工厂 → trait + enum，正则+XML标签提取 |
| `react_agent.hpp/cpp` | `src/agent/react_agent.rs` | ReAct 循环，`tokio::spawn` 替代 ThreadPool |
| `agent_manager.hpp/cpp` | `src/agent/agent_manager.rs` | 单例 → `Arc<Mutex<HashMap>>` 或普通结构体 |
| `system_prompt_renderer.hpp/cpp` | `src/agent/system_prompt.rs` | 简单字符串替换，可直接用 `str::replace` |
| `gtht_agent_util.hpp/cpp` | `src/agent/util.rs` | 进度数据构建 + 响应规范化 |
| `thread_pool.hpp/cpp` | **不翻译** | 直接使用 `tokio` runtime |
| `logger.hpp/cpp` | **不翻译** | 直接使用 `tracing` crate |
| `uuid_generator.hpp/cpp` | **不翻译** | 直接使用 `uuid` crate (已有依赖) |
| `llm_adapter.hpp` | `src/agent/llm_adapter.rs` | 定义 LLM trait，用户自行实现 |

### 3.2 核心类型翻译

#### `types.hpp` → `types.rs`

```rust
// AgentStatus 枚举
pub enum AgentStatus {
    IterationStart,
    LlmCall,
    LlmResponse,
    LlmChunk,
    ToolCall,
    ToolResult,
    IterationEnd,
    Retry,
    Unknown,
}

// Message 结构体
pub struct Message {
    pub role: MessageRole,  // enum: System, User, Assistant
    pub content: String,
}

// ToolDescriptor
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub json_schema: String,
    pub func: Box<dyn Fn(&str) -> String + Send + Sync>,
}

// LLMResponse
pub struct LlmResponse {
    pub content: String,
    pub success: bool,
}

// AgentCallback → 进度回调
pub type AgentCallback = Box<dyn Fn(String, AgentStatus) + Send + Sync>;

// LLMFunction → trait
pub trait LlmProvider: Send + Sync {
    fn call(&self, messages_json: &str) -> LlmResponse;
    fn call_stream(&self, session_id: &str, messages_json: &str) -> Result<(), AgentError>;
}
```

#### `response_parser.hpp` → `response_parser.rs`

```rust
pub enum ParserType {
    Xml,
    Json,
}

pub trait ResponseParser: Send + Sync {
    fn parse(&self, response: &str) -> ParseResult;
    fn parse_action(&self, action_str: &str) -> (String, String);
}

pub struct ParseResult {
    pub thought: Option<String>,
    pub action: Option<String>,
    pub final_answer: Option<String>,
}

pub struct XmlResponseParser { ... }
pub struct JsonResponseParser { ... }
```

#### `agent_manager.hpp` → `agent_manager.rs`

```rust
pub struct AgentManager {
    thread_pool_size: usize,
    sessions: Arc<Mutex<HashMap<String, Arc<ReActAgent>>>>,
    default_llm: Option<Arc<dyn LlmProvider>>,
}

impl AgentManager {
    pub fn new(thread_pool_size: usize) -> Self;
    pub fn create_session(&self, system_prompt: &str) -> String;
    pub fn destroy_session(&self, session_id: &str) -> bool;
    pub fn run_sync(&self, session_id: &str, user_input: &str, callback: AgentCallback);
    pub async fn run_async(&self, session_id: &str, user_input: &str, callback: AgentCallback);
    // ... 其他方法
}
```

### 3.3 关键翻译决策

| C++ 概念 | Rust 对应 | 说明 |
|:---|:---|:---|
| `std::function<>` | `Box<dyn Fn()>` 或泛型 | 工具函数和回调使用 trait object |
| `std::shared_ptr` | `Arc<T>` | 多线程共享所有权 |
| `std::mutex` | `Mutex<T>` / `tokio::sync::Mutex` | 同步用 `std::sync::Mutex`，异步场景用 tokio 的 |
| `std::unordered_map` | `HashMap` | 直接映射 |
| `std::string` | `String` / `&str` | 按所有权语义选择 |
| `std::thread` + ThreadPool | `tokio::spawn` | FlowRun 已依赖 tokio，无需自建线程池 |
| `nlohmann::json` | `serde_json::Value` | 直接映射 |
| `std::regex` | `regex::Regex` | FlowRun 已有 regex 依赖 |
| 自定义 Logger | `tracing::*` | FlowRun 已使用 tracing |
| 自定义 UUID | `uuid::Uuid::new_v4()` | FlowRun 已有 uuid 依赖 |
| 单例模式 (`static AgentManager`) | 普通结构体 + `Arc` | Rust 中避免全局单例，由调用方管理生命周期 |
| `virtual` + 工厂模式 | `trait` + `enum` | `ResponseParser` 用 trait，`ParserType` 用 enum |

### 3.4 异步模型转换

C++ 版本使用 ThreadPool 提交任务 + 回调，翻译为 Rust 时：

- `run_async` → `tokio::spawn` + `tokio::sync::mpsc` channel 回传进度
- `run_sync` → 直接同步执行（或 `block_on`）
- `run_internal_with_progress` → 保留进度回调语义，但用 `tokio::sync::watch` 或 `mpsc`

---

## 4. 文件清单与实施顺序

### 目标文件结构

```
src/agent/
├── mod.rs                 # 模块声明
├── types.rs               # 核心类型 (AgentStatus, Message, ToolDescriptor, LlmResponse, etc.)
├── error.rs               # Agent 错误类型 (thiserror)
├── llm_adapter.rs         # LlmProvider trait 定义
├── tool_registry.rs       # 工具注册表
├── response_parser.rs     # 响应解析器 (XML + JSON)
├── system_prompt.rs       # 系统提示词渲染
├── util.rs                # 工具函数 (进度数据构建、响应规范化)
└── react_agent.rs         # ReAct Agent 核心循环 + AgentManager
```

### 实施阶段

#### Phase 1: 基础类型层 (无依赖)

| # | 文件 | 源文件 | 工作量 | 说明 |
|:---|:---|:---|:---|:---|
| 1.1 | `src/agent/mod.rs` | — | 小 | 模块声明 + re-export |
| 1.2 | `src/agent/types.rs` | `types.hpp` | 中 | 所有核心类型定义 |
| 1.3 | `src/agent/error.rs` | — | 小 | `thiserror` 错误定义 |
| 1.4 | `src/agent/llm_adapter.rs` | `llm_adapter.hpp` | 小 | `LlmProvider` trait |

#### Phase 2: 基础设施层 (依赖 Phase 1)

| # | 文件 | 源文件 | 工作量 | 说明 |
|:---|:---|:---|:---|:---|
| 2.1 | `src/agent/util.rs` | `gtht_agent_util.cpp` | 中 | 进度数据 + 响应规范化 |
| 2.2 | `src/agent/system_prompt.rs` | `system_prompt_renderer.cpp` | 小 | 模板渲染 |
| 2.3 | `src/agent/tool_registry.rs` | `tool_registry.hpp/cpp` | 中 | 工具注册执行 |
| 2.4 | `src/agent/response_parser.rs` | `response_parser.hpp/cpp` | 大 | XML/JSON 双解析器 |

#### Phase 3: 核心业务层 (依赖 Phase 1+2)

| # | 文件 | 源文件 | 工作量 | 说明 |
|:---|:---|:---|:---|:---|
| 3.1 | `src/agent/react_agent.rs` | `react_agent.hpp/cpp` | 大 | ReAct 循环 + 会话管理 |

#### Phase 4: Executor 集成 (依赖 Phase 1+2+3)

| # | 文件 | 源文件 | 工作量 | 说明 |
|:---|:---|:---|:---|:---|
| 4.1 | `src/core/types.rs` 扩展 | — | 中 | 新增 `StepType::Agent`/`Tool`，扩展 `StepDefinition` 字段 |
| 4.2 | `src/core/context.rs` 扩展 | — | 小 | `ExecutionContext` 添加 `tool_registry`/`agent_manager` 字段 |
| 4.3 | `src/executors/agent_executor.rs` | — | 中 | Agent 步骤执行器，实现 `Executor` trait |
| 4.4 | `src/executors/tool_executor.rs` | — | 小 | Tool 步骤执行器，实现 `Executor` trait |
| 4.5 | `src/executors/mod.rs` 更新 | — | 小 | 注册 `pub mod agent; pub mod tool;` |

#### Phase 5: 验证与测试

| # | 任务 | 工作量 | 说明 |
|:---|:---|:---|:---|
| 5.1 | `src/lib.rs` 添加 `pub mod agent;` | 小 | 模块注册 |
| 5.2 | 编写 agent 模块单元测试 | 中 | types/tool_registry/response_parser/react_agent |
| 5.3 | 编写 executor 集成测试 | 中 | AgentExecutor + ToolExecutor + 混合 workflow |
| 5.4 | `cargo build` / `cargo test` | 小 | 编译验证 |

---

## 5. 各文件详细翻译说明

### 5.1 `types.rs` — 核心类型

**翻译来源**: `types.hpp`

| C++ 类型 | Rust 类型 | 说明 |
|:---|:---|:---|
| `ToolParam` | 不翻译 | C++ 版本中已弃用，工具参数通过 JSON Schema 描述 |
| `ToolDescriptor` | `pub struct ToolDescriptor` | 字段一致，`func` 改为 `Arc<dyn Fn(&str) -> String + Send + Sync>` |
| `AgentStatus` | `pub enum AgentStatus` | 直接映射，重命名为 Rust 惯用命名 |
| `Message` | `pub struct Message` | `role` 改为 `MessageRole` enum |
| `AgentCallback` | `pub type AgentCallback = Arc<dyn Fn(String, AgentStatus) + Send + Sync>` | 函数指针 → trait object |
| `LLMResponse` | `pub struct LlmResponse` | 直接映射 |
| `LLMFunction` | `pub trait LlmProvider` | 函数指针 → trait，支持未来扩展流式 |

**新增类型** (Rust 惯用增强):

```rust
pub enum MessageRole {
    System,
    User,
    Assistant,
}

pub struct ParseResult {
    pub thought: Option<String>,
    pub action: Option<String>,
    pub final_answer: Option<String>,
    pub has_action_or_answer: bool,  // 解析是否有效
}
```

### 5.2 `error.rs` — 错误类型

**无对应源文件**，新增文件，定义 Agent 模块专属错误:

```rust
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("Session not found: {0}")]
    SessionNotFound(String),
    #[error("LLM call failed: {0}")]
    LlmError(String),
    #[error("Tool not found: {0}")]
    ToolNotFound(String),
    #[error("Parse error: {0}")]
    ParseError(String),
    #[error("Max iterations reached")]
    MaxIterationsReached,
    #[error("Response format invalid")]
    InvalidResponseFormat,
}
```

### 5.3 `llm_adapter.rs` — LLM 适配器

**翻译来源**: `llm_adapter.hpp` (仅占位)

定义 LLM 提供者 trait，解耦 Agent 与具体 LLM 实现:

```rust
pub trait LlmProvider: Send + Sync {
    /// 同步调用 LLM
    fn call(&self, messages_json: &str) -> LlmResponse;
}
```

### 5.4 `util.rs` — 工具函数

**翻译来源**: `gtht_agent_util.cpp`

| C++ 方法 | Rust 函数 | 说明 |
|:---|:---|:---|
| `build_progress_data()` | `pub fn build_progress_data()` | 构建 `{"session_id", "iteration", "data": {...}}` JSON |
| `build_llm_response_data()` | `pub fn build_llm_response_data()` | 构建 LLM 响应进度数据 |
| `normalize_response()` | `pub fn normalize_response()` | trim → 去外层引号 → 反转义 |

进度类型常量使用 Rust `const &str`:

```rust
pub const PROGRESS_TYPE_ITERATION_START: &str = "iteration_start";
pub const PROGRESS_TYPE_LLM_CALL: &str = "llm_call";
// ... 其余类似
```

### 5.5 `system_prompt.rs` — 系统提示词渲染

**翻译来源**: `system_prompt_renderer.cpp`

极简翻译，Rust 原生 `str::replace` 即可:

```rust
pub fn render_system_prompt(template: &str, user_prompt: &str, tool_list: &str) -> String {
    template
        .replace("${user_prompt}", user_prompt)
        .replace("${tool_list}", tool_list)
}
```

### 5.6 `tool_registry.rs` — 工具注册表

**翻译来源**: `tool_registry.hpp/cpp`

```rust
pub struct ToolRegistry {
    tools: HashMap<String, ToolDescriptor>,
}

impl ToolRegistry {
    pub fn new() -> Self;
    pub fn register(&mut self, descriptor: ToolDescriptor);
    pub fn register_tool(&mut self, name: &str, desc: &str, schema: &str, func: Arc<dyn Fn(&str)->String+Send+Sync>);
    pub fn execute(&self, tool_name: &str, args: &str) -> Result<String, AgentError>;
    pub fn has_tool(&self, tool_name: &str) -> bool;
    pub fn get_tool_list(&self) -> String;
    pub fn get_descriptor(&self, tool_name: &str) -> Option<&ToolDescriptor>;
}
```

关键差异:
- `execute` 返回 `Result<String, AgentError>` 而非 JSON 错误字符串
- `get_tool_list()` 格式保持与 C++ 一致（name/description/schema 格式）

### 5.7 `response_parser.rs` — 响应解析器

**翻译来源**: `response_parser.hpp/cpp` (最大文件)

```
ResponseParser trait
├── XmlResponseParser
│   ├── parse()           — 提取 <thought>, <action>, <final_answer>
│   ├── parse_action()    — JSON 格式: {"name":"...", "parameters":{...}}
│   └── extract_last_complete_tag() — 从后往前搜索最后一个完整标签
└── JsonResponseParser
    ├── parse()           — 从 JSON 对象提取 thought/action/final_answer
    └── parse_action()    — 解析 action 字符串/对象
```

关键翻译点:
- `extract_last_complete_tag()`: C++ 用 `rfind` 从后搜索 + `tolower`，Rust 用 `.to_lowercase()` + `.rfind()`
- `parse_action()`: 优先尝试 JSON 解析 (`serde_json::from_str`)，失败则用正则
- `remove_backslash_quotes()`: 简单的 `str::replace("\\\"", "\"")` 即可
- `normalize_response()`: 已在 `util.rs` 中

### 5.8 `react_agent.rs` — ReAct Agent + AgentManager

**翻译来源**: `react_agent.hpp/cpp` + `agent_manager.hpp/cpp`

此文件包含两个主要结构体:

#### ReActAgent

```rust
pub struct ReActAgent {
    session_id: String,
    messages: Vec<Message>,
    tool_registry: Arc<Mutex<ToolRegistry>>,
    llm_provider: Arc<dyn LlmProvider>,
    system_prompt_template: String,
    user_prompt: String,
    parser: Box<dyn ResponseParser>,
    max_iterations: u32,  // 默认 10
}
```

核心方法:
| C++ 方法 | Rust 方法 | 说明 |
|:---|:---|:---|
| `run()` | `pub fn run(&mut self, user_input: &str) -> Result<String, AgentError>` | 同步运行 |
| `run_async()` | `pub async fn run_async(...)` | tokio spawn 异步 |
| `run_internal()` | `fn run_internal(&mut self, ...) -> Result<String, AgentError>` | 内部 ReAct 循环 |
| `run_internal_with_progress()` | `fn run_with_progress(&mut self, ..., callback)` | 带进度回调的循环 |
| `set_system_prompt()` | `pub fn set_user_prompt(&mut self, prompt: &str)` | |
| `register_tool()` | `pub fn register_tool(...)` | |
| `clear_history()` | `pub fn clear_history(&mut self)` | |
| `get_history()` | `pub fn history(&self) -> &[Message]` | |

#### AgentManager

```rust
pub struct AgentManager {
    sessions: Arc<Mutex<HashMap<String, Arc<Mutex<ReActAgent>>>>>,
    default_llm: Option<Arc<dyn LlmProvider>>,
}
```

**不采用单例模式**，改为普通结构体，由调用方持有和管理。

核心方法:
| C++ 方法 | Rust 方法 |
|:---|:---|
| `create_session()` | `pub fn create_session(&self, system_prompt: &str) -> String` |
| `destroy_session()` | `pub fn destroy_session(&self, session_id: &str) -> bool` |
| `session_exists()` | `pub fn session_exists(&self, session_id: &str) -> bool` |
| `run_sync()` | `pub fn run_sync(&self, session_id: &str, input: &str, callback: AgentCallback)` |
| `run_async()` | `pub async fn run_async(...)` |
| `set_llm_function()` | `pub fn set_llm_provider(&self, session_id: &str, provider: Arc<dyn LlmProvider>)` |
| `register_tool()` | `pub fn register_tool(&self, session_id: &str, ...)` |
| `set_max_iterations()` | `pub fn set_max_iterations(&self, session_id: &str, max: u32)` |
| `clear_history()` | `pub fn clear_history(&self, session_id: &str)` |
| `get_all_session_ids()` | `pub fn session_ids(&self) -> Vec<String>` |
| `get_session_count()` | `pub fn session_count(&self) -> usize` |

---

## 6. 不翻译的组件 (直接复用 Rust 生态)

| C++ 组件 | Rust 替代 | 说明 |
|:---|:---|:---|
| `ThreadPool` | `tokio::runtime::Handle` + `tokio::spawn` | FlowRun 已全面使用 tokio |
| `Logger` | `tracing::{info!, debug!, warn!, error!}` | FlowRun 已配置 tracing-subscriber |
| `UUIDGenerator` | `uuid::Uuid::new_v4().to_string()` | Cargo.toml 已有 `uuid = "1.6"` |
| `nlohmann/json` | `serde_json::Value` / `serde_json::json!` | 已有依赖 |
| `std::regex` | `regex::Regex` | 已有依赖 |

---

## 7. 依赖变更

无需新增依赖。所有功能可由以下已有 crate 实现:

- `tokio` — 异步运行时
- `serde` + `serde_json` — 序列化
- `regex` — 正则匹配
- `uuid` — 会话 ID
- `tracing` — 日志
- `thiserror` + `anyhow` — 错误处理

---

## 8. 翻译中的注意事项

### 8.1 保持语义一致

- ReAct 循环的核心逻辑 **严格保持一致**: system prompt 模板、XML 标签格式、工具调用 JSON 格式 (`{"name":"...", "parameters":{...}}`)
- 错误处理路径保持一致: LLM 失败 → continue、工具不存在 → continue、达到最大迭代 → 返回错误

### 8.2 Rust 惯用改造

- 错误返回: `Result<T, AgentError>` 替代 JSON 错误字符串
- 回调: `Arc<dyn Fn(...)>` 替代 `std::function`
- 线程安全: `Arc<Mutex<>>` 替代裸 `std::mutex`
- 不用单例: `AgentManager` 作为普通结构体由调用方管理

### 8.3 与 FlowRun 现有模块的集成

- `agent` 模块为独立模块，不依赖 `core/`, `executors/`, `cli/`
- 后续可通过 `executors` 中的步骤类型调用 agent，或在 agent 中调用 workflow
- `src/lib.rs` 仅需添加 `pub mod agent;`

---

## 9. Executor 集成设计

### 9.1 背景

Agent 模块完成后，需要与 FlowRun 的 workflow 执行引擎深度集成。核心思路:

1. **Agent 作为 Executor 的一种** — Step 可以直接调用 Agent 执行推理
2. **ToolExecutor** — Step 可以直接调用 ToolRegistry 中注册的工具，使 Step 更灵活

### 9.2 新增 StepType

在 `src/core/types.rs` 的 `StepType` enum 中新增两种类型:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepType {
    Http,
    Shell,
    Parallel,
    Loop,
    Condition,
    Workflow,
    Approve,
    // ===== 新增 =====
    Agent,   // Agent 推理步骤
    Tool,    // 工具调用步骤
}
```

### 9.3 ToolRegistry 共享设计

**核心问题**: ToolRegistry 中的工具既需要被 Agent 使用，也需要被 ToolExecutor 使用。

**设计方案**: Workflow 级别共享 ToolRegistry

```rust
// ExecutionContext 扩展
pub struct ExecutionContext {
    // ... 现有字段 ...
    
    /// 工具注册表（Workflow 级共享）
    tool_registry: Option<Arc<Mutex<ToolRegistry>>>,
    
    /// Agent 管理器（Workflow 级共享）
    agent_manager: Option<Arc<AgentManager>>,
}
```

**注册时机**:
- Workflow 启动前，通过 `WorkflowConfig` 或 hooks 注册工具到 `ToolRegistry`
- AgentExecutor 和 ToolExecutor 从 `ExecutionContext` 获取共享的 `ToolRegistry`

### 9.4 StepDefinition 扩展

为新增的两种 StepType 添加配置字段:

```rust
pub struct StepDefinition {
    // ... 现有字段 ...
    
    // ===== Agent 步骤新增字段 =====
    /// Agent 会话 ID（可选，不提供则自动创建临时会话）
    pub session_id: Option<String>,
    /// Agent 系统提示词（创建新会话时使用）
    pub agent_system_prompt: Option<String>,
    /// Agent 输入（模板表达式）
    pub agent_input: Option<String>,
    /// Agent 最大迭代次数
    pub agent_max_iterations: Option<u32>,
    /// 进度回调配置
    pub progress_callback: Option<ProgressCallbackConfig>,
    
    // ===== Tool 步骤新增字段 =====
    /// 工具名称
    pub tool_name: Option<String>,
    /// 工具参数（JSON 或模板表达式）
    pub tool_args: Option<String>,
}

pub struct ProgressCallbackConfig {
    /// 回调类型: "log" | "http" | "webhook"
    pub r#type: String,
    /// 回调目标（URL 或日志级别）
    pub target: Option<String>,
}
```

### 9.5 AgentExecutor 设计

**文件**: `src/executors/agent_executor.rs`

```rust
pub struct AgentExecutor {
    agent_manager: Arc<AgentManager>,
}

impl AgentExecutor {
    pub fn new(agent_manager: Arc<AgentManager>) -> Self;
}

#[async_trait::async_trait]
impl Executor for AgentExecutor {
    async fn execute(
        &self,
        step: &StepDefinition,
        context: &ExecutionContext,
    ) -> Result<StepResult, WorkflowError>;
}
```

**执行流程**:

```
StepDefinition (type: agent)
  ↓
获取或创建 Agent 会话
  ├─ 有 session_id → 使用已有会话
  └─ 无 session_id → 创建临时会话（workflow 结束时销毁）
  ↓
设置 LLM Provider（从 ExecutionContext 获取）
  ↓
注入 ToolRegistry（共享）
  ↓
渲染 agent_input（模板引擎）
  ↓
调用 Agent.run_sync / run_async
  ↓
返回 StepResult（包含 final_answer）
```

**YAML 示例**:

```yaml
steps:
  - id: analyze_data
    name: "AI 分析数据"
    type: agent
    agent_system_prompt: "You are a data analyst..."
    agent_input: "${{ steps.fetch_data.output.body }}"
    agent_max_iterations: 5
    depends_on: [fetch_data]
```

### 9.6 ToolExecutor 设计

**文件**: `src/executors/tool_executor.rs`

```rust
pub struct ToolExecutor;

impl ToolExecutor {
    pub fn new() -> Self;
}

#[async_trait::async_trait]
impl Executor for ToolExecutor {
    async fn execute(
        &self,
        step: &StepDefinition,
        context: &ExecutionContext,
    ) -> Result<StepResult, WorkflowError>;
}
```

**执行流程**:

```
StepDefinition (type: tool)
  ↓
从 ExecutionContext 获取 ToolRegistry
  ↓
渲染 tool_args（模板引擎）
  ↓
ToolRegistry.execute(tool_name, tool_args)
  ↓
返回 StepResult（包含工具返回值）
```

**YAML 示例**:

```yaml
steps:
  - id: read_file
    name: "读取文件"
    type: tool
    tool_name: "read_file"
    tool_args: '{"path": "${{ inputs.file_path }}"}'
    depends_on: []
```

### 9.7 工具注册配置

在 Workflow YAML 中提供工具注册入口:

```yaml
# WorkflowConfig 扩展
config:
  # ... 现有配置 ...
  
  # 工具注册配置
  tools:
    - name: read_file
      description: "Read file content"
      json_schema: '{"type":"object","properties":{"path":{"type":"string"}}}'
      # 工具来源: "builtin" | "plugin" | "custom"
      source: builtin
    - name: http_request
      description: "Make HTTP request"
      json_schema: '{"type":"object","properties":{"url":{"type":"string"},"method":{"type":"string"}}}'
      source: builtin
    - name: custom_tool
      description: "Custom tool"
      json_schema: '{"type":"object"}'
      source: custom
      # 自定义工具通过代码注册，YAML 仅声明
```

**内置工具** (builtin):
- `read_file` — 读取本地文件
- `write_file` — 写入本地文件
- `http_request` — HTTP 请求（与 HttpExecutor 功能重叠，但可作为工具供 Agent 使用）
- `execute_shell` — Shell 命令（与 ShellExecutor 功能重叠）
- `query_workflow` — 查询 workflow 状态/输出

### 9.8 架构集成图

```
┌─────────────────────────────────────────────────────────────┐
│                    Workflow Execution                        │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│   ExecutionContext                                           │
│   ┌────────────────────────────────────────────────────┐    │
│   │  tool_registry: Arc<Mutex<ToolRegistry>>           │    │
│   │  agent_manager: Arc<AgentManager>                  │◄───┤── Workflow 级共享
│   └────────────────────────────────────────────────────┘    │
│                                                              │
│   ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│   │ HttpExecutor │  │ ShellExecutor│  │ AgentExecutor│      │
│   └──────────────┘  └──────────────┘  └──────┬───────┘      │
│                                              │              │
│   ┌──────────────┐  ┌──────────────┐         │              │
│   │ LoopExecutor │  │ToolExecutor │◄────────┤              │
│   └──────────────┘  └──────┬───────┘         │              │
│                            │                 │              │
│                            ▼                 ▼              │
│                   ┌────────────────────────────┐            │
│                   │      ToolRegistry          │            │
│                   │  (工具注册 + 执行)          │            │
│                   └────────────────────────────┘            │
│                            │                                │
│                            ▼                                │
│                   ┌────────────────────────────┐            │
│                   │       ReActAgent           │            │
│                   │  (推理 → 调用工具)          │            │
│                   └────────────────────────────┘            │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 9.9 新增文件清单

| 文件 | 说明 |
|:---|:---|
| `src/executors/agent_executor.rs` | Agent 步骤执行器 |
| `src/executors/tool_executor.rs` | 工具步骤执行器 |

**executors/mod.rs 更新**:

```rust
pub mod http;
pub mod shell;
pub mod r#loop;
pub mod condition;
pub mod workflow;
pub mod approve;
pub mod agent;    // 新增
pub mod tool;     // 新增
```

---

## 10. 预估工作量

| 阶段 | 文件数 | 预估代码行 | 难度 |
|:---|:---|:---|:---|
| Phase 1: 基础类型 | 4 | ~200 | 低 |
| Phase 2: 基础设施 | 4 | ~400 | 中 |
| Phase 3: 核心业务 | 1 | ~500 | 高 |
| Phase 4: Executor 集成 | 2 | ~300 | 中 |
| Phase 5: 验证测试 | — | ~200 | 低 |
| **总计** | **11 个新文件** | **~1600 行** | — |

> 注: Executor 集成包含 `agent_executor.rs` 和 `tool_executor.rs`，以及 `StepType`/`StepDefinition`/`ExecutionContext` 的扩展。

---

## 11. 验收标准

1. `cargo build` 无错误无警告
2. `cargo test` 通过 (含 agent 模块单元测试)
3. ReAct 循环行为与 C++ 版本一致:
   - 相同 system prompt 模板
   - 相同 XML 标签解析
   - 相同工具调用 JSON 格式
   - 相同进度回调数据结构
4. 示例代码可运行: 创建会话 → 注册工具 → 设置 LLM → 同步/异步运行
5. **Executor 集成验证**:
   - Agent 步骤可以在 YAML workflow 中定义并执行
   - Tool 步骤可以直接调用 ToolRegistry 中的工具
   - Agent 和 ToolExecutor 共享同一 ToolRegistry
6. **端到端测试**:
   ```yaml
   # 示例 workflow: agent + tool 混合使用
   config:
     tools:
       - name: read_file
         source: builtin
   
   steps:
     - id: read_config
       type: tool
       tool_name: read_file
       tool_args: '{"path": "./config.json"}'
     
     - id: analyze
       type: agent
       agent_input: "${{ steps.read_config.output }}"
       agent_max_iterations: 3
       depends_on: [read_config]
   ```
