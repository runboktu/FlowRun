# Agent 模块实施计划（合并版）

> 源码: `gtht-agent/src` (C++) → 目标: `flow-run/src/agent` (Rust)
> 忽略: `android/`, `ios/`, `harmony/` 移动端适配层

---

## 1. 概述

### 1.1 目标

将 `gtht-agent`（C++ ReAct Agent）的核心功能翻译为 Rust 模块，集成到 `flow-run` 工作流引擎中：

- 提供 **ReAct（Reasoning + Acting）** 范式的 AI Agent 能力
- **共享工具系统**：`ToolRegistry` 作为核心组件，同时供 Agent 和独立的 Tool 步骤使用
- 作为新的步骤类型 `agent` 和 `tool` 集成到工作流引擎
- Agent 和 Tool 都是 Executor 的具体实现

### 1.2 设计原则

| 原则 | 说明 |
|:---|:---|
| **Rust 惯用法** | 使用 trait、async/await、所有权模型 |
| **异步优先** | 基于 tokio 异步运行时，无自建线程池 |
| **可扩展性** | LLM 接口和工具系统通过 trait 抽象 |
| **线程安全** | 利用 Rust 类型系统，使用 `Arc<RwLock<>>` |
| **组件复用** | ToolRegistry 全局共享，Agent 和 ToolExecutor 都依赖它 |
| **独立优先** | ToolExecutor 可独立于 Agent 工作，降低耦合 |

---

## 2. 源项目架构概览 (gtht-agent)

gtht-agent 是一个基于 **ReAct 范式** 的 C++ AI Agent 库，核心组件：

| 组件 | 源文件 | 职责 |
|:---|:---|:---|
| **Types** | `types.hpp` | 核心类型定义: `ToolDescriptor`, `AgentStatus`, `Message`, `LLMResponse`, `LLMFunction`, `AgentCallback` |
| **ToolRegistry** | `tool_registry.hpp/cpp` | 工具注册表: 注册/执行/查询工具，维护工具描述和 JSON Schema |
| **ResponseParser** | `response_parser.hpp/cpp` | LLM 响应解析器: XML 解析器 + JSON 解析器，工厂模式切换 |
| **ReActAgent** | `react_agent.hpp/cpp` | 核心推理循环: Thought → Action → Observation → Answer |
| **AgentManager** | `agent_manager.hpp/cpp` | 会话管理器: 单例模式，多会话隔离，线程安全 |
| **ThreadPool** | `thread_pool.hpp/cpp` | 线程池: 任务队列，异步执行 |
| **SystemPromptRenderer** | `system_prompt_renderer.hpp/cpp` | 模板渲染: 替换 `${user_prompt}` 和 `${tool_list}` 占位符 |
| **GTHTAgentUtil** | `gtht_agent_util.hpp/cpp` | 工具类: 构建进度数据 JSON、规范化响应字符串 |
| **Logger** | `logger.hpp/cpp` | 日志系统: 全局日志函数注入，4 级日志 |
| **UUIDGenerator** | `uuid_generator.hpp/cpp` | UUID 生成: 标准 UUID v4 格式 |
| **LLMAdapter** | `llm_adapter.hpp` | LLM 适配器: 仅占位，用户自行实现 `LLMFunction` |

### 2.1 ReAct 循环核心流程

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

## 3. 目标模块结构

```
src/
├── agent/
│   ├── mod.rs              # 模块导出
│   ├── types.rs            # 类型定义（Message, ToolDescriptor, AgentStatus 等）
│   ├── error.rs            # Agent 错误类型（thiserror）
│   ├── llm_adapter.rs      # LLM Provider trait
│   ├── tool_registry.rs    # 工具注册器（核心共享组件）
│   ├── tool_handler.rs     # ToolHandler trait（工具抽象）
│   ├── response_parser.rs  # 响应解析器（XML/JSON）
│   ├── system_prompt.rs    # 系统提示词渲染
│   ├── util.rs             # 工具函数（进度数据、响应规范化）
│   ├── react_agent.rs      # ReAct Agent 核心逻辑
│   └── session_manager.rs  # 会话管理器（替代 AgentManager）
├── executors/
│   ├── mod.rs              # 现有执行器模块
│   ├── http.rs             # HTTP 执行器
│   ├── shell.rs            # Shell 执行器
│   ├── ...                 # 其他现有执行器
│   ├── agent.rs            # Agent 步骤执行器（新增）
│   └── tool.rs             # Tool 步骤执行器（新增）
└── ...
```

### 3.1 架构关系图

```
┌─────────────────────────────────────────────────────────────┐
│                      WorkflowEngine                         │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                    Executor (trait)                         │
├─────────────┬─────────────┬─────────────┬───────────────────┤
│   Http      │   Shell     │   Agent     │   Tool            │
│  Executor   │  Executor   │  Executor   │  Executor         │
└─────────────┴─────────────┴──────┬──────┴─────────┬─────────┘
                                   │                │
                                   ▼                │
                          ┌────────────────┐        │
                          │ ReActAgent     │        │
                          │ (ReAct 循环)   │        │
                          └────────┬───────┘        │
                                   │                │
                                   ▼                ▼
                          ┌────────────────────────────────────┐
                          │     ToolRegistry (共享组件)         │
                          │  - register_tool()                 │
                          │  - execute_tool()                  │
                          │  - get_tool_list()                 │
                          └────────────────────────────────────┘
```

**关键设计**：
- `ToolRegistry` 是全局共享组件
- `AgentExecutor` 通过 `ReActAgent` 使用 `ToolRegistry`
- `ToolExecutor` 直接使用 `ToolRegistry`
- 两者可以复用相同的工具定义

---

## 4. 翻译映射方案

### 4.1 模块映射

| C++ 组件 | Rust 目标文件 | 翻译策略 |
|:---|:---|:---|
| `types.hpp` | `src/agent/types.rs` | 结构体 → Rust struct/enum，函数指针 → trait/closure |
| `tool_registry.hpp/cpp` | `src/agent/tool_registry.rs` | `unordered_map` → `HashMap`，`function` → `ToolHandler` trait |
| `response_parser.hpp/cpp` | `src/agent/response_parser.rs` | 虚基类+工厂 → trait + enum，正则+XML标签提取 |
| `react_agent.hpp/cpp` | `src/agent/react_agent.rs` | ReAct 循环，`tokio::spawn` 替代 ThreadPool |
| `agent_manager.hpp/cpp` | `src/agent/session_manager.rs` | 单例 → 普通结构体，重命名为 SessionManager |
| `system_prompt_renderer.hpp/cpp` | `src/agent/system_prompt.rs` | 简单字符串替换，`str::replace` |
| `gtht_agent_util.hpp/cpp` | `src/agent/util.rs` | 进度数据构建 + 响应规范化 |
| `thread_pool.hpp/cpp` | **不翻译** | 直接使用 `tokio` runtime |
| `logger.hpp/cpp` | **不翻译** | 直接使用 `tracing` crate |
| `uuid_generator.hpp/cpp` | **不翻译** | 直接使用 `uuid` crate |
| `llm_adapter.hpp` | `src/agent/llm_adapter.rs` | 定义 `LlmProvider` trait |
| — | `src/agent/tool_handler.rs` | **新增**：工具处理器 trait 抽象 |
| — | `src/agent/error.rs` | **新增**：Agent 错误类型 |

### 4.2 关键翻译决策

| C++ 概念 | Rust 对应 | 说明 |
|:---|:---|:---|
| `std::function<>` | `Box<dyn Fn()>` 或 `ToolHandler` trait | 工具使用 trait 抽象，回调使用闭包 |
| `std::shared_ptr` | `Arc<T>` | 多线程共享所有权 |
| `std::mutex` | `tokio::sync::RwLock` | 异步场景用 RwLock |
| `std::unordered_map` | `HashMap` | 直接映射 |
| `std::string` | `String` / `&str` | 按所有权语义选择 |
| `std::thread` + ThreadPool | `tokio::spawn` | 无需自建线程池 |
| `nlohmann::json` | `serde_json::Value` | 直接映射 |
| `std::regex` | `regex::Regex` | 已有依赖 |
| 自定义 Logger | `tracing::*` | 已配置 tracing-subscriber |
| 自定义 UUID | `uuid::Uuid::new_v4()` | 已有依赖 |
| 单例模式 | 普通结构体 + `Arc` | 依赖注入，由调用方管理 |
| `virtual` + 工厂模式 | `trait` + `enum` | `ResponseParser` 用 trait |

---

## 5. 核心类型设计

### 5.1 `types.rs` — 核心类型

```rust
/// Agent 状态枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// 消息角色
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageRole {
    System,
    User,
    Assistant,
}

/// 消息结构
#[derive(Debug, Clone)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

/// LLM 响应
#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: String,
    pub success: bool,
}

/// 工具描述符
#[derive(Debug, Clone)]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub json_schema: Option<String>,
    pub handler: Arc<dyn ToolHandler>,
}

/// 工具执行结果
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
}

/// 解析结果
#[derive(Debug, Clone, Default)]
pub struct ParsedResponse {
    pub thought: Option<String>,
    pub action: Option<String>,
    pub final_answer: Option<String>,
}

/// 进度事件
#[derive(Debug, Clone)]
pub struct AgentEvent {
    pub session_id: String,
    pub iteration: usize,
    pub status: AgentStatus,
    pub data: serde_json::Value,
}
```

### 5.2 `error.rs` — 错误类型

```rust
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("Session not found: {0}")]
    SessionNotFound(String),
    
    #[error("LLM call failed: {0}")]
    LlmError(String),
    
    #[error("Tool not found: {0}")]
    ToolNotFound(String),
    
    #[error("Tool execution failed: {0}")]
    ToolError(String),
    
    #[error("Parse error: {0}")]
    ParseError(String),
    
    #[error("Max iterations reached")]
    MaxIterationsReached,
    
    #[error("Response format invalid")]
    InvalidResponseFormat,
    
    #[error("{0}")]
    Other(String),
}
```

### 5.3 `tool_handler.rs` — 工具处理器 Trait

```rust
use async_trait::async_trait;

/// 工具处理器接口
#[async_trait]
pub trait ToolHandler: Send + Sync {
    /// 执行工具
    async fn execute(&self, args: &str) -> ToolResult;
}

/// 闭包工具适配器（同步）
pub struct FnTool<F>(pub F);

#[async_trait]
impl<F> ToolHandler for FnTool<F>
where
    F: Fn(String) -> String + Send + Sync,
{
    async fn execute(&self, args: &str) -> ToolResult {
        let result = (self.0)(args.to_string());
        ToolResult {
            content: result,
            is_error: false,
        }
    }
}

/// 异步闭包工具适配器
pub struct AsyncFnTool<F>(pub F);

#[async_trait]
impl<F, Fut> ToolHandler for AsyncFnTool<F>
where
    F: Fn(String) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = String> + Send,
{
    async fn execute(&self, args: &str) -> ToolResult {
        let result = (self.0)(args.to_string()).await;
        ToolResult {
            content: result,
            is_error: false,
        }
    }
}
```

### 5.4 `llm_adapter.rs` — LLM Provider Trait

```rust
use async_trait::async_trait;

/// LLM 提供者接口
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// 同步调用 LLM
    async fn call(&self, messages: &[Message]) -> Result<LlmResponse, AgentError>;
    
    /// 流式调用（可选，默认调用 call）
    async fn call_streaming(
        &self,
        messages: &[Message],
        callback: impl Fn(String) + Send + 'static,
    ) -> Result<LlmResponse, AgentError> {
        self.call(messages).await
    }
}
```

---

## 6. 核心组件设计

### 6.1 Tool Registry (`tool_registry.rs`)

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 工具注册器（核心共享组件）
pub struct ToolRegistry {
    tools: Arc<RwLock<HashMap<String, ToolDescriptor>>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 注册工具
    pub async fn register(&self, descriptor: ToolDescriptor) {
        let name = descriptor.name.clone();
        self.tools.write().await.insert(name, descriptor);
    }

    /// 批量注册工具
    pub async fn register_all(&self, descriptors: Vec<ToolDescriptor>) {
        let mut tools = self.tools.write().await;
        for desc in descriptors {
            tools.insert(desc.name.clone(), desc);
        }
    }

    /// 检查工具是否存在
    pub async fn has_tool(&self, name: &str) -> bool {
        self.tools.read().await.contains_key(name)
    }

    /// 执行工具
    pub async fn execute(&self, name: &str, args: &str) -> ToolResult {
        let tools = self.tools.read().await;
        match tools.get(name) {
            Some(descriptor) => descriptor.handler.execute(args).await,
            None => ToolResult {
                content: format!("Tool '{}' not found", name),
                is_error: true,
            },
        }
    }

    /// 获取工具列表描述（用于系统提示）
    pub async fn get_tool_list(&self) -> String {
        let tools = self.tools.read().await;
        let mut result = String::new();
        for (name, desc) in tools.iter() {
            result.push_str(&format!(
                "- {}: {}\n  Schema: {}\n",
                name,
                desc.description,
                desc.json_schema.as_deref().unwrap_or("{}")
            ));
        }
        result
    }

    /// 获取工具描述符
    pub async fn get_descriptor(&self, name: &str) -> Option<ToolDescriptor> {
        self.tools.read().await.get(name).cloned()
    }
}
```

### 6.2 Response Parser (`response_parser.rs`)

```rust
/// 解析器类型
#[derive(Debug, Clone, Copy)]
pub enum ParserType {
    Xml,
    Json,
}

/// 响应解析器 trait
pub trait ResponseParser: Send + Sync {
    fn parse(&self, response: &str) -> ParsedResponse;
    fn parse_action(&self, action: &str) -> (String, String);  // (tool_name, args_json)
}

/// XML 解析器
pub struct XmlParser;

impl ResponseParser for XmlParser {
    fn parse(&self, response: &str) -> ParsedResponse {
        // 提取 <thought>, <action>, <final_answer> 标签
        // 使用 extract_last_complete_tag 从后搜索
        let normalized = normalize_response(response);
        ParsedResponse {
            thought: extract_last_complete_tag(&normalized, "thought"),
            action: extract_last_complete_tag(&normalized, "action"),
            final_answer: extract_last_complete_tag(&normalized, "final_answer"),
        }
    }

    fn parse_action(&self, action: &str) -> (String, String) {
        // 优先尝试 JSON 解析: {"name": "...", "parameters": {...}}
        // 失败则返回 ("", "")
        match serde_json::from_str::<serde_json::Value>(action) {
            Ok(json) if json.is_object() => {
                let name = json.get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let params = json.get("parameters")
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                (name, params)
            }
            _ => ("", "".to_string()),
        }
    }
}

/// JSON 解析器
pub struct JsonParser;

impl ResponseParser for JsonParser {
    fn parse(&self, response: &str) -> ParsedResponse {
        // 从 JSON 对象提取 thought/action/final_answer
        // ...
    }

    fn parse_action(&self, action: &str) -> (String, String) {
        // 解析 action 字符串/对象
        // ...
    }
}

/// 解析器工厂
pub fn create_parser(parser_type: ParserType) -> Box<dyn ResponseParser> {
    match parser_type {
        ParserType::Xml => Box::new(XmlParser),
        ParserType::Json => Box::new(JsonParser),
    }
}

/// 辅助函数：提取最后一个完整标签
fn extract_last_complete_tag(s: &str, tag_name: &str) -> Option<String> {
    let open_tag = format!("<{}>", tag_name);
    let close_tag = format!("</{}>", tag_name);
    let s_lower = s.to_lowercase();
    
    if let Some(pos) = s_lower.rfind(&open_tag) {
        let content_start = pos + open_tag.len();
        if let Some(close_pos) = s_lower.find(&close_tag, content_start) {
            return Some(s[content_start..close_pos].trim().to_string());
        }
    }
    None
}
```

### 6.3 ReAct Agent (`react_agent.rs`)

```rust
pub struct ReActAgent {
    session_id: String,
    messages: Vec<Message>,
    tool_registry: Arc<ToolRegistry>,
    llm_provider: Arc<dyn LlmProvider>,
    parser: Box<dyn ResponseParser>,
    max_iterations: usize,
    system_prompt_template: String,
    user_prompt: String,
}

impl ReActAgent {
    pub fn new(
        session_id: String,
        tool_registry: Arc<ToolRegistry>,
        llm_provider: Arc<dyn LlmProvider>,
    ) -> Self {
        Self {
            session_id,
            messages: Vec::new(),
            tool_registry,
            llm_provider,
            parser: create_parser(ParserType::Xml),
            max_iterations: 10,
            system_prompt_template: DEFAULT_SYSTEM_PROMPT.to_string(),
            user_prompt: String::new(),
        }
    }

    /// 设置系统提示
    pub fn set_system_prompt(&mut self, prompt: &str) {
        self.user_prompt = prompt.to_string();
    }

    /// 设置最大迭代次数
    pub fn set_max_iterations(&mut self, max: usize) {
        self.max_iterations = max;
    }

    /// 同步运行 Agent
    pub async fn run(&mut self, user_input: &str) -> Result<String, AgentError> {
        // 1. 添加系统提示
        let system_prompt = self.render_system_prompt().await;
        self.messages.push(Message {
            role: MessageRole::System,
            content: system_prompt,
        });

        // 2. 添加用户输入
        self.messages.push(Message {
            role: MessageRole::User,
            content: format!("<question>{}</question>", user_input),
        });

        // 3. ReAct 循环
        for _iteration in 0..self.max_iterations {
            // 调用 LLM
            let response = self.llm_provider.call(&self.messages).await?;
            
            if !response.success {
                self.messages.push(Message {
                    role: MessageRole::Assistant,
                    content: format!("LLM Error: {}", response.content),
                });
                continue;
            }

            self.messages.push(Message {
                role: MessageRole::Assistant,
                content: response.content.clone(),
            });

            // 解析响应
            let parsed = self.parser.parse(&response.content);

            // 检查最终答案
            if let Some(final_answer) = parsed.final_answer {
                return Ok(final_answer);
            }

            // 执行工具
            if let Some(action) = parsed.action {
                let (tool_name, args) = self.parser.parse_action(&action);
                
                if !tool_name.is_empty() {
                    let result = self.tool_registry.execute(&tool_name, &args).await;
                    
                    self.messages.push(Message {
                        role: MessageRole::User,
                        content: format!("<observation>{}</observation>", result.content),
                    });
                }
            }
        }

        Err(AgentError::MaxIterationsReached)
    }

    /// 带进度回调的运行
    pub async fn run_with_callback(
        &mut self,
        user_input: &str,
        callback: impl Fn(AgentEvent) + Send + 'static,
    ) -> Result<String, AgentError> {
        // 类似 run()，但在每个阶段触发 callback
        // 触发时机: IterationStart, LlmCall, LlmResponse, ToolCall, ToolResult, IterationEnd
        // ...
    }

    /// 渲染系统提示
    async fn render_system_prompt(&self) -> String {
        let tool_list = self.tool_registry.get_tool_list().await;
        self.system_prompt_template
            .replace("${user_prompt}", &self.user_prompt)
            .replace("${tool_list}", &tool_list)
    }

    /// 获取历史记录
    pub fn history(&self) -> &[Message] {
        &self.messages
    }

    /// 清空历史
    pub fn clear_history(&mut self) {
        self.messages.clear();
    }
}

const DEFAULT_SYSTEM_PROMPT: &str = r#"
你需要解决一个问题。为此，你需要将问题分解为多个步骤。

所有步骤请严格使用以下 XML 标签格式输出：
- <question> 用户问题
- <thought> 思考
- <action> 采取的工具操作
- <observation> 工具或环境返回的结果
- <final_answer> 最终答案

请严格遵守：
- 你每次回答都必须包括两个标签，第一个是 <thought>，第二个是 <action> 或 <final_answer>
- 输出 <action> 后立即停止生成，等待真实的 <observation>

${user_prompt}

本次任务可用工具：
${tool_list}
"#;
```

### 6.4 Session Manager (`session_manager.rs`)

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// 会话管理器
pub struct SessionManager {
    llm_provider: Arc<dyn LlmProvider>,
    tool_registry: Arc<ToolRegistry>,
    sessions: Arc<RwLock<HashMap<String, ReActAgent>>>,
}

impl SessionManager {
    pub fn new(
        llm_provider: Arc<dyn LlmProvider>,
        tool_registry: Arc<ToolRegistry>,
    ) -> Self {
        Self {
            llm_provider,
            tool_registry,
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 创建会话
    pub async fn create_session(&self, system_prompt: Option<String>) -> String {
        let session_id = Uuid::new_v4().to_string();
        let mut agent = ReActAgent::new(
            session_id.clone(),
            self.tool_registry.clone(),
            self.llm_provider.clone(),
        );
        
        if let Some(prompt) = system_prompt {
            agent.set_system_prompt(&prompt);
        }

        self.sessions.write().await.insert(session_id.clone(), agent);
        session_id
    }

    /// 销毁会话
    pub async fn destroy_session(&self, session_id: &str) -> bool {
        self.sessions.write().await.remove(session_id).is_some()
    }

    /// 检查会话是否存在
    pub async fn session_exists(&self, session_id: &str) -> bool {
        self.sessions.read().await.contains_key(session_id)
    }

    /// 同步运行
    pub async fn run_sync(&self, session_id: &str, user_input: &str) -> Result<String, AgentError> {
        let mut sessions = self.sessions.write().await;
        match sessions.get_mut(session_id) {
            Some(agent) => agent.run(user_input).await,
            None => Err(AgentError::SessionNotFound(session_id.to_string())),
        }
    }

    /// 带回调的异步运行
    pub async fn run_with_callback(
        &self,
        session_id: &str,
        user_input: &str,
        callback: impl Fn(AgentEvent) + Send + 'static,
    ) -> Result<String, AgentError> {
        let mut sessions = self.sessions.write().await;
        match sessions.get_mut(session_id) {
            Some(agent) => agent.run_with_callback(user_input, callback).await,
            None => Err(AgentError::SessionNotFound(session_id.to_string())),
        }
    }

    /// 获取所有会话 ID
    pub async fn session_ids(&self) -> Vec<String> {
        self.sessions.read().await.keys().cloned().collect()
    }

    /// 获取会话数量
    pub async fn session_count(&self) -> usize {
        self.sessions.read().await.len()
    }

    /// 清空会话历史
    pub async fn clear_history(&self, session_id: &str) -> Result<(), AgentError> {
        let mut sessions = self.sessions.write().await;
        match sessions.get_mut(session_id) {
            Some(agent) => {
                agent.clear_history();
                Ok(())
            }
            None => Err(AgentError::SessionNotFound(session_id.to_string())),
        }
    }

    /// 设置会话最大迭代次数
    pub async fn set_max_iterations(&self, session_id: &str, max: usize) -> Result<(), AgentError> {
        let mut sessions = self.sessions.write().await;
        match sessions.get_mut(session_id) {
            Some(agent) => {
                agent.set_max_iterations(max);
                Ok(())
            }
            None => Err(AgentError::SessionNotFound(session_id.to_string())),
        }
    }
}
```

---

## 7. Executor 集成设计

### 7.1 StepType 扩展

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

### 7.2 StepDefinition 扩展

```rust
pub struct StepDefinition {
    // ... 现有字段 ...
    
    // ===== Agent 步骤新增字段 =====
    /// Agent 输入（模板表达式）
    pub agent_input: Option<String>,
    /// 系统提示词
    pub system_prompt: Option<String>,
    /// 最大迭代次数
    pub max_iterations: Option<usize>,
    /// 解析器类型: "xml" | "json"
    pub parser_type: Option<String>,
    
    // ===== Tool 步骤新增字段 =====
    /// 工具名称
    pub tool: Option<String>,
    /// 工具参数（JSON 或模板表达式）
    pub args: Option<serde_json::Value>,
    /// 超时时间（秒）
    pub timeout: Option<u64>,
}
```

### 7.3 ExecutionContext 扩展

```rust
pub struct ExecutionContext {
    // ... 现有字段 ...
    
    /// 工具注册表（Workflow 级共享）
    pub tool_registry: Option<Arc<ToolRegistry>>,
    
    /// 会话管理器（Workflow 级共享）
    pub session_manager: Option<Arc<SessionManager>>,
}
```

### 7.4 ToolExecutor (`executors/tool.rs`)

```rust
/// Tool 步骤执行器
///
/// 直接调用 ToolRegistry 中已注册的工具，无需 Agent 推理循环
pub struct ToolExecutor;

impl ToolExecutor {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Executor for ToolExecutor {
    async fn execute(
        &self,
        step: &StepDefinition,
        context: &ExecutionContext,
    ) -> Result<StepResult, WorkflowError> {
        let started_at = Utc::now();

        // 1. 解析配置
        let tool_name = step.tool.as_ref()
            .ok_or_else(|| WorkflowError::Other("tool name required".into()))?;
        let args = step.args.as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default())
            .unwrap_or_default();

        // 2. 获取 ToolRegistry
        let tool_registry = context.tool_registry.as_ref()
            .ok_or_else(|| WorkflowError::Other("ToolRegistry not initialized".into()))?;

        // 3. 检查工具是否存在
        if !tool_registry.has_tool(tool_name).await {
            return Err(WorkflowError::Other(format!(
                "Tool '{}' not found in registry",
                tool_name
            )));
        }

        // 4. 执行工具（可选超时）
        let result = if let Some(timeout_secs) = step.timeout {
            tokio::time::timeout(
                std::time::Duration::from_secs(timeout_secs),
                tool_registry.execute(tool_name, &args),
            )
            .await
            .map_err(|_| WorkflowError::Other(format!("Tool '{}' timed out", tool_name)))??
        } else {
            tool_registry.execute(tool_name, &args).await
        };

        // 5. 构建结果
        let completed_at = Utc::now();
        let duration_ms = (completed_at - started_at).num_milliseconds() as u64;

        if result.is_error {
            Ok(StepResult::failed(
                step.id.clone(),
                StepError {
                    code: "TOOL_ERROR".to_string(),
                    message: result.content,
                    fix: None,
                },
            ))
        } else {
            let output = serde_json::from_str::<serde_json::Value>(&result.content)
                .unwrap_or_else(|_| serde_json::Value::String(result.content));
            Ok(StepResult::success(step.id.clone(), output)
                .with_timing(started_at, duration_ms))
        }
    }
}
```

### 7.5 AgentExecutor (`executors/agent.rs`)

```rust
/// Agent 步骤执行器
pub struct AgentExecutor;

impl AgentExecutor {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Executor for AgentExecutor {
    async fn execute(
        &self,
        step: &StepDefinition,
        context: &ExecutionContext,
    ) -> Result<StepResult, WorkflowError> {
        let started_at = Utc::now();

        // 1. 解析配置
        let user_input = step.agent_input.as_ref()
            .ok_or_else(|| WorkflowError::Other("agent_input required".into()))?;

        // 2. 获取 SessionManager
        let session_manager = context.session_manager.as_ref()
            .ok_or_else(|| WorkflowError::Other("SessionManager not initialized".into()))?;

        // 3. 创建会话
        let session_id = session_manager
            .create_session(step.system_prompt.clone())
            .await;

        // 4. 设置最大迭代次数
        if let Some(max_iter) = step.max_iterations {
            session_manager.set_max_iterations(&session_id, max_iter).await?;
        }

        // 5. 运行 Agent
        let result = session_manager.run_sync(&session_id, user_input).await;

        // 6. 销毁临时会话
        session_manager.destroy_session(&session_id).await;

        // 7. 构建结果
        let completed_at = Utc::now();
        let duration_ms = (completed_at - started_at).num_milliseconds() as u64;

        match result {
            Ok(answer) => Ok(StepResult::success(
                step.id.clone(),
                serde_json::json!({ "answer": answer }),
            ).with_timing(started_at, duration_ms)),
            Err(e) => Ok(StepResult::failed(
                step.id.clone(),
                StepError {
                    code: "AGENT_ERROR".to_string(),
                    message: e.to_string(),
                    fix: None,
                },
            )),
        }
    }
}
```

---

## 8. 工具注册配置

### 8.1 YAML 配置格式

在 Workflow YAML 中提供工具注册入口：

```yaml
# WorkflowConfig 扩展
config:
  # ... 现有配置 ...
  
  # 工具注册配置
  tools:
    # HTTP 工具
    - name: search_docs
      description: "搜索技术文档"
      schema: |
        {
          "type": "object",
          "properties": {
            "query": {"type": "string", "description": "搜索关键词"}
          },
          "required": ["query"]
        }
      implementation: http
      config:
        api: "https://api.example.com/search"
        method: POST
        headers:
          Content-Type: application/json
        body:
          query: "${{ args.query }}"
    
    # Shell 工具
    - name: read_file
      description: "读取文件内容"
      schema: |
        {
          "type": "object",
          "properties": {
            "path": {"type": "string", "description": "文件路径"}
          },
          "required": ["path"]
        }
      implementation: shell
      config:
        run: "cat ${{ args.path }}"
    
    # 内置工具
    - name: write_file
      description: "写入文件"
      schema: |
        {
          "type": "object",
          "properties": {
            "path": {"type": "string"},
            "content": {"type": "string"}
          },
          "required": ["path", "content"]
        }
      implementation: builtin
      config:
        handler: file_write
    
    # 自定义工具（代码注册）
    - name: custom_tool
      description: "自定义工具"
      schema: '{"type": "object"}'
      implementation: custom
      # 自定义工具通过代码注册，YAML 仅声明
```

### 8.2 工具实现类型

| implementation | 说明 | 配置字段 |
|:---|:---|:---|
| `http` | HTTP 请求工具 | `api`, `method`, `headers`, `body` |
| `shell` | Shell 命令工具 | `run`, `env` |
| `builtin` | 内置工具 | `handler` (处理器名) |
| `custom` | 自定义工具 | 通过代码注册 |

### 8.3 内置工具列表

| 工具名 | 描述 |
|:---|:---|
| `read_file` | 读取本地文件 |
| `write_file` | 写入本地文件 |
| `http_request` | HTTP 请求 |
| `execute_shell` | Shell 命令 |
| `query_workflow` | 查询 workflow 状态/输出 |

---

## 9. YAML 配置示例

### 9.1 完整示例：Agent + Tool 混合工作流

```yaml
name: "工具增强的 AI 工作流"
description: "演示 ToolRegistry 共享给 Agent 和 ToolExecutor"
version: "1.0.0"

# 全局工具定义（注册到 ToolRegistry）
config:
  tools:
    - name: search_docs
      description: "搜索技术文档"
      schema: |
        {
          "type": "object",
          "properties": {
            "query": {"type": "string"}
          }
        }
      implementation: http
      config:
        api: "https://api.example.com/search"
        method: POST
        body:
          query: "${{ args.query }}"

    - name: read_file
      description: "读取文件内容"
      schema: |
        {
          "type": "object",
          "properties": {
            "path": {"type": "string"}
          }
        }
      implementation: shell
      config:
        run: "cat ${{ args.path }}"

    - name: write_file
      description: "写入文件"
      schema: |
        {
          "type": "object",
          "properties": {
            "path": {"type": "string"},
            "content": {"type": "string"}
          }
        }
      implementation: builtin
      config:
        handler: file_write

steps:
  # 步骤 1: ToolExecutor 直接调用工具
  - id: read_config
    name: "读取配置文件"
    type: tool
    tool: read_file
    args:
      path: "/etc/app/config.yaml"

  # 步骤 2: 另一个 ToolExecutor 调用
  - id: search_issues
    name: "搜索相关问题"
    type: tool
    tool: search_docs
    args:
      query: "config error timeout"

  # 步骤 3: Agent 使用所有已注册的工具进行推理
  - id: ai_diagnosis
    name: "AI 诊断"
    type: agent
    agent_input: |
      根据以下信息诊断问题：
      配置内容: ${{ steps.read_config.output }}
      相关问题: ${{ steps.search_issues.output }}
    system_prompt: "你是一个运维专家，帮助诊断系统问题。"
    max_iterations: 5
    depends_on: [read_config, search_issues]

  # 步骤 4: ToolExecutor 写入修复结果
  - id: save_fix
    name: "保存修复方案"
    type: tool
    tool: write_file
    args:
      path: "/tmp/fix_report.txt"
      content: "${{ steps.ai_diagnosis.output.answer }}"
    depends_on: [ai_diagnosis]

outputs:
  diagnosis: "${{ steps.ai_diagnosis.output.answer }}"
  report_path: "/tmp/fix_report.txt"
```

### 9.2 ToolExecutor 单独使用示例

```yaml
name: "简单的工具调用工作流"

config:
  tools:
    - name: http_get
      description: "发送 HTTP GET 请求"
      implementation: http
      config:
        method: GET

    - name: transform_json
      description: "转换 JSON 数据"
      implementation: shell
      config:
        run: |
          echo '${{ args.input }}' | python3 -c "
          import sys, json
          data = json.load(sys.stdin)
          result = {'processed': data}
          print(json.dumps(result))
          "

steps:
  - id: fetch_data
    type: tool
    tool: http_get
    args:
      url: "https://api.example.com/data"

  - id: process_data
    type: tool
    tool: transform_json
    args:
      input: "${{ steps.fetch_data.output.body }}"
    depends_on: [fetch_data]
```

### 9.3 AgentExecutor 单独使用示例

```yaml
name: "纯 Agent 工作流"

config:
  tools:
    - name: calculator
      description: "执行数学计算"
      schema: |
        {
          "type": "object",
          "properties": {
            "expression": {"type": "string"}
          }
        }
      implementation: shell
      config:
        run: "python3 -c \"print(${{ args.expression }})\""

steps:
  - id: math_agent
    type: agent
    agent_input: "计算 (100 * 2 + 50) / 3 的结果"
    system_prompt: "你是一个数学助手，使用 calculator 工具进行计算。"
    max_iterations: 3
```

---

## 10. 实施阶段

### Phase 1: 基础类型和接口 (1-2 天)

| 任务 | 文件 | 说明 |
|:---|:---|:---|
| 模块结构 | `src/agent/mod.rs` | 模块声明 + re-export |
| 类型定义 | `src/agent/types.rs` | 所有基础类型 |
| 错误类型 | `src/agent/error.rs` | `thiserror` 错误定义 |
| LLM trait | `src/agent/llm_adapter.rs` | `LlmProvider` trait |
| 工具 trait | `src/agent/tool_handler.rs` | `ToolHandler` trait + 闭包适配器 |

### Phase 2: 核心组件 - ToolRegistry (2-3 天) 【高优先级】

| 任务 | 文件 | 说明 |
|:---|:---|:---|
| 工具注册器 | `src/agent/tool_registry.rs` | **核心共享组件** |
| 响应解析器 | `src/agent/response_parser.rs` | XML/JSON 双解析器 |
| 系统提示渲染 | `src/agent/system_prompt.rs` | 模板渲染 |
| 工具函数 | `src/agent/util.rs` | 进度数据 + 响应规范化 |
| 单元测试 | — | ToolRegistry, ResponseParser |

### Phase 3: ToolExecutor 独立实现 (2-3 天) 【高优先级】

| 任务 | 文件 | 说明 |
|:---|:---|:---|
| Tool 执行器 | `src/executors/tool.rs` | Tool 步骤执行器 |
| StepType 扩展 | `src/core/types.rs` | 新增 `Tool` 类型 |
| ExecutionContext 扩展 | `src/core/context.rs` | 添加 `tool_registry` 字段 |
| YAML 解析器更新 | `src/core/parser.rs` | 支持工具定义和 Tool 步骤 |
| **验证** | — | **ToolExecutor 可独立工作** |

### Phase 4: Agent 核心 (3-4 天) 【中优先级】

| 任务 | 文件 | 说明 |
|:---|:---|:---|
| ReAct Agent | `src/agent/react_agent.rs` | ReAct 循环 |
| 会话管理器 | `src/agent/session_manager.rs` | 会话生命周期管理 |
| 集成测试 | — | ReActAgent + ToolRegistry |

### Phase 5: AgentExecutor 集成 (2-3 天) 【中优先级】

| 任务 | 文件 | 说明 |
|:---|:---|:---|
| Agent 执行器 | `src/executors/agent.rs` | Agent 步骤执行器 |
| StepType 扩展 | `src/core/types.rs` | 新增 `Agent` 类型 |
| YAML 解析器更新 | `src/core/parser.rs` | 支持 Agent 步骤配置 |
| **验证** | — | **Agent 和 Tool 共享 ToolRegistry** |

### Phase 6: 示例和文档 (1-2 天) 【低优先级】

| 任务 | 说明 |
|:---|:---|
| Agent 工作流示例 | YAML + Rust 代码 |
| Tool 工作流示例 | YAML + Rust 代码 |
| Agent + Tool 混合示例 | 端到端演示 |
| 使用文档 | API 文档 + 集成指南 |
| 性能测试 | 压力测试 + 基准测试 |

---

## 11. 工作量估算

| 阶段 | 文件数 | 预估代码行 | 时间 | 优先级 |
|:---|:---|:---|:---|:---|
| Phase 1: 基础类型 | 5 | ~250 | 1-2 天 | 高 |
| Phase 2: ToolRegistry | 4 | ~350 | 2-3 天 | 高 |
| Phase 3: ToolExecutor | 4 | ~300 | 2-3 天 | 高 |
| Phase 4: Agent 核心 | 2 | ~400 | 3-4 天 | 中 |
| Phase 5: AgentExecutor | 3 | ~250 | 2-3 天 | 中 |
| Phase 6: 示例文档 | — | ~200 | 1-2 天 | 低 |
| **总计** | **12 个新文件** | **~1750 行** | **11-17 天** | — |

### 优先级建议

**高优先级（先实现）**：
- Phase 1-3: 基础设施 + ToolExecutor
- 这样即使 Agent 部分延迟，ToolExecutor 也能独立使用

**中优先级**：
- Phase 4-5: Agent 核心和集成

**低优先级**：
- Phase 6: 文档和示例

---

## 12. 验收标准

### 12.1 编译与测试

1. `cargo build` 无错误无警告
2. `cargo test` 通过 (含 agent 模块单元测试)
3. `cargo clippy` 无严重警告

### 12.2 Agent 核心功能

4. ReAct 循环行为与 C++ 版本一致:
   - 相同 system prompt 模板
   - 相同 XML 标签解析
   - 相同工具调用 JSON 格式 (`{"name":"...", "parameters":{...}}`)
   - 相同进度回调数据结构

### 12.3 Executor 集成

5. **ToolExecutor 独立工作**:
   - 可以在 YAML 中定义 `type: tool` 步骤
   - 工具可通过 YAML 配置注册
   - 工具执行结果正确返回

6. **AgentExecutor 集成**:
   - 可以在 YAML 中定义 `type: agent` 步骤
   - Agent 自动使用已注册的工具
   - Agent 和 ToolExecutor 共享同一 ToolRegistry

### 12.4 端到端测试

7. 运行完整示例工作流：
   ```yaml
   config:
     tools:
       - name: read_file
         implementation: shell
         config:
           run: "cat ${{ args.path }}"
   
   steps:
     - id: read_config
       type: tool
       tool: read_file
       args:
         path: "./config.json"
     
     - id: analyze
       type: agent
       agent_input: "${{ steps.read_config.output }}"
       max_iterations: 3
       depends_on: [read_config]
   ```

---

## 13. 依赖变更

无需新增依赖。所有功能可由以下已有 crate 实现：

| Crate | 用途 |
|:---|:---|
| `tokio` | 异步运行时 |
| `serde` + `serde_json` | 序列化 |
| `regex` | 正则匹配 |
| `uuid` | 会话 ID |
| `tracing` | 日志 |
| `thiserror` + `anyhow` | 错误处理 |
| `async-trait` | 异步 trait |

---

## 14. 风险与缓解

| 风险 | 影响 | 缓解措施 |
|:---|:---|:---|
| LLM API 调用延迟 | Agent 响应慢 | 支持流式响应 + 超时配置 |
| 工具执行失败 | Agent 循环中断 | 错误作为 observation 返回，继续推理 |
| XML 解析不稳定 | 无法提取 action | 支持多解析器（XML/JSON） |
| 会话状态丢失 | 上下文中断 | 支持会话持久化（后续扩展） |