# Agent 模块实现计划（合并版）

> 源码: `gtht-agent/src` (C++) → 目标: `flow-run/src/agent` (Rust)
> 忽略: `android/`, `ios/`, `harmony/` 移动端适配层

---

## 1. 概述与目标

将 `gtht-agent`（C++ ReAct Agent）的核心功能翻译为 Rust 模块，集成到 `flow-run` 工作流引擎中。

### 核心目标

1. **ReAct Agent 能力**：提供 Thought → Action → Observation → Answer 推理循环
2. **共享工具系统**：`ToolRegistry` 作为核心组件，同时供 Agent 和独立的 Tool 步骤使用
3. **Executor 集成**：新增 `AgentExecutor` 和 `ToolExecutor` 两种步骤执行器
4. **Rust 惯用设计**：使用 trait、async/await、所有权模型

### 设计原则

- **异步优先**：基于 tokio 异步运行时（替代 C++ ThreadPool）
- **trait 抽象**：LLM 接口和工具系统通过 trait 解耦
- **组件复用**：ToolRegistry 是全局共享的，Agent 和 ToolExecutor 都依赖它
- **线程安全**：使用 `Arc` + `RwLock` 保证并发安全

---

## 2. 源项目架构概览 (gtht-agent)

gtht-agent 是一个基于 **ReAct 范式** 的 C++ AI Agent 库，核心组件:

| 组件 | 源文件 | 职责 | 翻译策略 |
|:---|:---|:---|:---|
| **Types** | `types.hpp` | 核心类型: `ToolDescriptor`, `AgentStatus`, `Message`, `LLMResponse` | 直接映射为 Rust struct/enum |
| **ToolRegistry** | `tool_registry.hpp/cpp` | 工具注册/执行/查询 | `HashMap` + trait 闭包 |
| **ResponseParser** | `response_parser.hpp/cpp` | XML/JSON 响应解析，工厂模式 | trait + enum，正则+XML标签提取 |
| **ReActAgent** | `react_agent.hpp/cpp` | ReAct 推理循环核心 | tokio::spawn 替代 ThreadPool |
| **AgentManager** | `agent_manager.hpp/cpp` | 会话管理，单例模式 | 普通结构体 + `Arc<RwLock<HashMap>>` |
| **ThreadPool** | `thread_pool.hpp/cpp` | 线程池 | **不翻译** - 直接使用 tokio |
| **Logger** | `logger.hpp/cpp` | 日志系统 | **不翻译** - 直接使用 tracing |
| **UUIDGenerator** | `uuid_generator.hpp/cpp` | UUID 生成 | **不翻译** - 直接使用 uuid crate |
| **SystemPromptRenderer** | `system_prompt_renderer.hpp/cpp` | 模板渲染 | 简单 `str::replace` |
| **GTHTAgentUtil** | `gtht_agent_util.hpp/cpp` | 工具函数 | 进度数据构建 + 响应规范化 |

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

## 3. 目标项目架构 (FlowRun)

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

## 4. 模块结构设计

### 4.1 文件结构

```
src/
├── agent/
│   ├── mod.rs                 # 模块声明 + re-export
│   ├── types.rs               # 核心类型 (AgentStatus, Message, ToolDescriptor, LlmResponse, etc.)
│   ├── error.rs               # Agent 错误类型 (thiserror)
│   ├── llm_adapter.rs         # LlmProvider trait 定义
│   ├── tool_registry.rs       # 工具注册表（核心共享组件）
│   ├── response_parser.rs     # 响应解析器 (XML + JSON)
│   ├── system_prompt.rs       # 系统提示词渲染
│   ├── util.rs                # 工具函数 (进度数据构建、响应规范化)
│   └── react_agent.rs         # ReAct Agent 核心循环 + AgentManager
├── executors/
│   ├── mod.rs                 # 现有执行器模块
│   ├── http.rs, shell.rs, ... # 现有执行器
│   ├── agent_executor.rs      # Agent 步骤执行器（新增）
│   └── tool_executor.rs       # Tool 步骤执行器（新增）
└── ...
```

### 4.2 架构关系图

```
┌─────────────────────────────────────────────────────────────────────┐
│                         WorkflowEngine                              │
└─────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────┐
│                      Executor (trait)                               │
├──────────┬──────────┬──────────┬──────────┬──────────┬──────────────┤
│   Http   │  Shell   │  Loop    │Condition │  Agent   │    Tool      │
│ Executor │ Executor │ Executor │ Executor │ Executor │  Executor    │
└──────────┴──────────┴──────────┴──────────┴────┬─────┴──────┬───────┘
                                                 │            │
                                                 ▼            │
                                        ┌─────────────┐      │
                                        │ ReActAgent  │      │
                                        │ (ReAct循环)  │      │
                                        └──────┬──────┘      │
                                               │             │
                                               ▼             ▼
                                      ┌─────────────────────────────┐
                                      │   ToolRegistry (共享组件)    │
                                      │   - register_tool()         │
                                      │   - execute_tool()          │
                                      │   - get_tool_list()         │
                                      └─────────────────────────────┘
```

**关键点：**
- `ToolRegistry` 是全局共享组件
- `AgentExecutor` 通过 `ReActAgent` 使用 `ToolRegistry`
- `ToolExecutor` 直接使用 `ToolRegistry`
- 两者可以复用相同的工具定义

---

## 5. 核心类型翻译

### 5.1 `types.hpp` → `types.rs`

```rust
/// Agent 状态枚举
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
pub enum MessageRole {
    System,
    User,
    Assistant,
}

/// 消息结构
pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

/// LLM 响应
pub struct LlmResponse {
    pub content: String,
    pub success: bool,
}

/// 工具描述符
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub json_schema: Option<String>,
    pub handler: Arc<dyn ToolHandler>,
}

/// 工具处理器 trait
#[async_trait]
pub trait ToolHandler: Send + Sync {
    async fn execute(&self, args: &str) -> ToolResult;
}

/// 工具结果
pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
}

/// Agent 进度回调
pub type AgentCallback = Arc<dyn Fn(String, AgentStatus) + Send + Sync>;
```

### 5.2 C++ → Rust 类型映射

| C++ 类型 | Rust 类型 | 说明 |
|:---|:---|:---|
| `std::function<LLMResponse(string)>` | `trait LlmProvider` | 函数指针 → trait |
| `std::function<string(string)>` | `trait ToolHandler` | 异步 trait |
| `std::shared_ptr<T>` | `Arc<T>` | 多线程共享所有权 |
| `std::mutex` | `tokio::sync::RwLock` | 异步锁 |
| `std::unordered_map` | `HashMap` | 直接映射 |
| `std::thread` + ThreadPool | `tokio::spawn` | 异步任务 |
| `nlohmann::json` | `serde_json::Value` | JSON 处理 |
| 自定义 Logger | `tracing::*` | 日志系统 |
| 自定义 UUID | `uuid::Uuid::new_v4()` | UUID 生成 |
| 单例 AgentManager | 普通结构体 + `Arc` | 依赖注入 |

---

## 6. 核心 Trait 设计

### 6.1 LLM Provider Trait

```rust
/// LLM 提供者接口
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// 同步调用 LLM
    async fn call(&self, messages: &[Message]) -> Result<LlmResponse, AgentError>;
    
    /// 流式调用（可选，默认实现为同步调用）
    async fn call_streaming(
        &self,
        messages: &[Message],
        callback: impl Fn(String) + Send + 'static,
    ) -> Result<LlmResponse, AgentError> {
        self.call(messages).await
    }
}
```

### 6.2 Tool Handler Trait

```rust
/// 工具处理器接口
#[async_trait]
pub trait ToolHandler: Send + Sync {
    /// 执行工具
    async fn execute(&self, args: &str) -> ToolResult;
}

/// 闭包工具适配器（便捷实现）
pub struct FnTool<F>(pub F);

#[async_trait]
impl<F, Fut> ToolHandler for FnTool<F>
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

### 6.3 Response Parser Trait

```rust
/// 解析器类型
pub enum ParserType {
    Xml,
    Json,
}

/// 解析结果
pub struct ParsedResponse {
    pub thought: Option<String>,
    pub action: Option<String>,
    pub final_answer: Option<String>,
}

/// 响应解析器 trait
pub trait ResponseParser: Send + Sync {
    fn parse(&self, response: &str) -> ParsedResponse;
    fn parse_action(&self, action: &str) -> (String, String);
}

/// 解析器工厂
pub fn create_parser(parser_type: ParserType) -> Box<dyn ResponseParser> {
    match parser_type {
        ParserType::Xml => Box::new(XmlResponseParser::new()),
        ParserType::Json => Box::new(JsonResponseParser::new()),
    }
}
```

---

## 7. 核心组件实现

### 7.1 ToolRegistry (`agent/tool_registry.rs`)

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

    /// 便捷注册方法
    pub async fn register_tool(
        &self,
        name: &str,
        description: &str,
        json_schema: Option<&str>,
        handler: Arc<dyn ToolHandler>,
    ) {
        let descriptor = ToolDescriptor {
            name: name.to_string(),
            description: description.to_string(),
            json_schema: json_schema.map(|s| s.to_string()),
            handler,
        };
        self.register(descriptor).await;
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
                "name: {}\ndescription: {}\nschema: {}\n\n",
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

### 7.2 Response Parser (`agent/response_parser.rs`)

```rust
/// XML 解析器实现
pub struct XmlResponseParser;

impl XmlResponseParser {
    pub fn new() -> Self {
        Self
    }

    /// 从字符串中提取最后一个完整的标签内容
    fn extract_last_complete_tag(&self, s: &str, tag_name: &str) -> Option<String> {
        let open_tag = format!("<{}>", tag_name);
        let close_tag = format!("</{}>", tag_name);
        let s_lower = s.to_lowercase();
        
        let mut pos = s_lower.rfind(&open_tag.to_lowercase());
        while let Some(p) = pos {
            let content_start = p + open_tag.len();
            if let Some(close_pos) = s_lower.find(&close_tag.to_lowercase(), content_start) {
                let content = &s[content_start..close_pos];
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
            if p == 0 {
                break;
            }
            pos = s_lower[..p].rfind(&open_tag.to_lowercase());
        }
        None
    }
}

impl ResponseParser for XmlResponseParser {
    fn parse(&self, response: &str) -> ParsedResponse {
        let normalized = normalize_response(response);
        ParsedResponse {
            thought: self.extract_last_complete_tag(&normalized, "thought"),
            action: self.extract_last_complete_tag(&normalized, "action"),
            final_answer: self.extract_last_complete_tag(&normalized, "final_answer"),
        }
    }

    fn parse_action(&self, action_str: &str) -> (String, String) {
        let trimmed = remove_backslash_quotes(action_str).trim().to_string();
        
        // 尝试 JSON 解析: {"name":"...", "parameters":{...}}
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&trimmed) {
            if let (Some(name), Some(params)) = (json.get("name"), json.get("parameters")) {
                if let (Some(name_str), Ok(params_str)) = (name.as_str(), serde_json::to_string(params)) {
                    return (name_str.to_string(), params_str);
                }
            }
        }
        
        (String::new(), String::new())
    }
}

/// JSON 解析器实现
pub struct JsonResponseParser;

impl JsonResponseParser {
    pub fn new() -> Self {
        Self
    }
}

impl ResponseParser for JsonResponseParser {
    fn parse(&self, response: &str) -> ParsedResponse {
        // JSON 解析逻辑
        // ...
        ParsedResponse {
            thought: None,
            action: None,
            final_answer: None,
        }
    }

    fn parse_action(&self, action_str: &str) -> (String, String) {
        // JSON 解析逻辑
        (String::new(), String::new())
    }
}

/// 辅助函数：移除反斜杠引号
fn remove_backslash_quotes(s: &str) -> String {
    s.replace("\\\"", "\"")
}

/// 辅助函数：规范化响应
fn normalize_response(s: &str) -> String {
    s.trim()
        .trim_matches('"')
        .replace("\\n", "\n")
        .replace("\\t", "\t")
}
```

### 7.3 ReAct Agent (`agent/react_agent.rs`)

```rust
/// ReAct Agent 核心结构
pub struct ReActAgent {
    session_id: String,
    messages: Vec<Message>,
    tool_registry: Arc<ToolRegistry>,
    llm_provider: Arc<dyn LlmProvider>,
    parser: Box<dyn ResponseParser>,
    max_iterations: u32,
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

    /// 设置用户提示
    pub fn set_user_prompt(&mut self, prompt: &str) {
        self.user_prompt = prompt.to_string();
    }

    /// 设置最大迭代次数
    pub fn set_max_iterations(&mut self, max: u32) {
        self.max_iterations = max;
    }

    /// 注册工具
    pub async fn register_tool(&self, descriptor: ToolDescriptor) {
        self.tool_registry.register(descriptor).await;
    }

    /// 清空历史
    pub fn clear_history(&mut self) {
        self.messages.clear();
    }

    /// 获取历史
    pub fn history(&self) -> &[Message] {
        &self.messages
    }

    /// 同步运行 Agent
    pub async fn run(&mut self, user_input: &str) -> Result<String, AgentError> {
        self.run_internal(user_input).await
    }

    /// 带进度回调的运行
    pub async fn run_with_callback(
        &mut self,
        user_input: &str,
        callback: AgentCallback,
    ) -> Result<String, AgentError> {
        self.run_internal_with_progress(user_input, callback).await
    }

    /// 内部 ReAct 循环
    async fn run_internal(&mut self, user_input: &str) -> Result<String, AgentError> {
        // 1. 构建系统提示
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
        let mut iteration_count = 0;
        while iteration_count < self.max_iterations {
            // 序列化消息
            let messages_json = serde_json::to_string(
                &self.messages.iter().map(|m| {
                    serde_json::json!({
                        "role": match m.role {
                            MessageRole::System => "system",
                            MessageRole::User => "user",
                            MessageRole::Assistant => "assistant",
                        },
                        "content": m.content,
                    })
                }).collect::<Vec<_>>()
            ).map_err(|e| AgentError::SerializationError(e.to_string()))?;

            // 调用 LLM
            let response = self.llm_provider.call(&self.messages).await?;
            
            if !response.success {
                return Err(AgentError::LlmError(response.content));
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
                let (tool_name, args_str) = self.parser.parse_action(&action);
                
                if !self.tool_registry.has_tool(&tool_name).await {
                    let error_msg = format!("Tool '{}' not found", tool_name);
                    self.messages.push(Message {
                        role: MessageRole::User,
                        content: format!("<observation>{}</observation>", error_msg),
                    });
                    continue;
                }

                let result = self.tool_registry.execute(&tool_name, &args_str).await;
                self.messages.push(Message {
                    role: MessageRole::User,
                    content: format!("<observation>{}</observation>", result.content),
                });
            } else {
                // 没有 action 也没有 final_answer，返回原始响应
                return Ok(response.content);
            }

            iteration_count += 1;
        }

        Err(AgentError::MaxIterationsReached)
    }

    /// 带进度回调的内部循环
    async fn run_internal_with_progress(
        &mut self,
        user_input: &str,
        callback: AgentCallback,
    ) -> Result<String, AgentError> {
        // 类似 run_internal，但在每个阶段调用 callback
        // ...
        todo!()
    }

    /// 渲染系统提示
    async fn render_system_prompt(&self) -> String {
        let tool_list = self.tool_registry.get_tool_list().await;
        self.system_prompt_template
            .replace("${user_prompt}", &self.user_prompt)
            .replace("${tool_list}", &tool_list)
    }
}

/// 默认系统提示模板
const DEFAULT_SYSTEM_PROMPT: &str = r#"
你需要解决一个问题。为此，你需要将问题分解为多个步骤。对于每个步骤，首先使用 <thought> 思考要做什么，然后使用可用工具之一决定一个 <action>。接着，你将根据你的行动从环境/工具中收到一个 <observation>。持续这个思考和行动的过程，直到你有足够的信息来提供 <final_answer>。

所有步骤请严格使用以下 XML 标签格式输出：
- <question> 用户问题
- <thought> 思考
- <action> 采取的工具操作
- <observation> 工具或环境返回的结果
- <final_answer> 最终答案

请严格遵守：
- 你每次回答都必须包括两个标签，第一个是 <thought>，第二个是 <action> 或 <final_answer>
- 输出 <action> 后立即停止生成，等待真实的 <observation>，擅自生成 <observation> 将导致错误

${user_prompt}

本次任务可用工具：
${tool_list}
"#;
```

### 7.4 AgentManager (`agent/react_agent.rs` 内)

```rust
/// 会话管理器（非单例，由调用方管理）
pub struct AgentManager {
    sessions: Arc<RwLock<HashMap<String, ReActAgent>>>,
    default_llm: Option<Arc<dyn LlmProvider>>,
    tool_registry: Arc<ToolRegistry>,
}

impl AgentManager {
    pub fn new(tool_registry: Arc<ToolRegistry>) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            default_llm: None,
            tool_registry,
        }
    }

    /// 设置默认 LLM Provider
    pub fn set_default_llm(&mut self, provider: Arc<dyn LlmProvider>) {
        self.default_llm = Some(provider);
    }

    /// 创建会话
    pub async fn create_session(&self, system_prompt: Option<&str>) -> Result<String, AgentError> {
        let session_id = uuid::Uuid::new_v4().to_string();
        let llm = self.default_llm.clone()
            .ok_or_else(|| AgentError::ConfigError("No LLM provider set".to_string()))?;

        let mut agent = ReActAgent::new(
            session_id.clone(),
            self.tool_registry.clone(),
            llm,
        );

        if let Some(prompt) = system_prompt {
            agent.set_user_prompt(prompt);
        }

        self.sessions.write().await.insert(session_id.clone(), agent);
        Ok(session_id)
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
    pub async fn run_sync(
        &self,
        session_id: &str,
        user_input: &str,
        callback: Option<AgentCallback>,
    ) -> Result<String, AgentError> {
        let mut sessions = self.sessions.write().await;
        let agent = sessions.get_mut(session_id)
            .ok_or_else(|| AgentError::SessionNotFound(session_id.to_string()))?;

        if let Some(cb) = callback {
            agent.run_with_callback(user_input, cb).await
        } else {
            agent.run(user_input).await
        }
    }

    /// 异步运行（tokio::spawn）
    pub async fn run_async(
        &self,
        session_id: &str,
        user_input: &str,
        callback: AgentCallback,
    ) -> Result<(), AgentError> {
        let sessions = self.sessions.clone();
        let session_id = session_id.to_string();
        let user_input = user_input.to_string();

        tokio::spawn(async move {
            let mut sessions = sessions.write().await;
            if let Some(agent) = sessions.get_mut(&session_id) {
                let _ = agent.run_with_callback(&user_input, callback).await;
            }
        });

        Ok(())
    }

    /// 设置会话的 LLM Provider
    pub async fn set_llm_provider(
        &self,
        session_id: &str,
        provider: Arc<dyn LlmProvider>,
    ) -> Result<(), AgentError> {
        let mut sessions = self.sessions.write().await;
        let agent = sessions.get_mut(session_id)
            .ok_or_else(|| AgentError::SessionNotFound(session_id.to_string()))?;
        // 需要在 ReActAgent 中添加 set_llm_provider 方法
        todo!()
    }

    /// 注册工具（全局）
    pub async fn register_tool(&self, descriptor: ToolDescriptor) {
        self.tool_registry.register(descriptor).await;
    }

    /// 为会话注册工具
    pub async fn register_tool_for_session(
        &self,
        session_id: &str,
        descriptor: ToolDescriptor,
    ) -> Result<(), AgentError> {
        // 全局注册，所有会话共享
        self.tool_registry.register(descriptor).await;
        Ok(())
    }

    /// 设置最大迭代次数
    pub async fn set_max_iterations(
        &self,
        session_id: &str,
        max: u32,
    ) -> Result<(), AgentError> {
        let mut sessions = self.sessions.write().await;
        let agent = sessions.get_mut(session_id)
            .ok_or_else(|| AgentError::SessionNotFound(session_id.to_string()))?;
        agent.set_max_iterations(max);
        Ok(())
    }

    /// 清空会话历史
    pub async fn clear_history(&self, session_id: &str) -> Result<(), AgentError> {
        let mut sessions = self.sessions.write().await;
        let agent = sessions.get_mut(session_id)
            .ok_or_else(|| AgentError::SessionNotFound(session_id.to_string()))?;
        agent.clear_history();
        Ok(())
    }

    /// 获取所有会话 ID
    pub async fn session_ids(&self) -> Vec<String> {
        self.sessions.read().await.keys().cloned().collect()
    }

    /// 获取会话数量
    pub async fn session_count(&self) -> usize {
        self.sessions.read().await.len()
    }
}
```

---

## 8. Executor 集成设计

### 8.1 StepType 扩展

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

### 8.2 StepDefinition 扩展

```rust
pub struct StepDefinition {
    // ... 现有字段 ...
    
    // ===== Agent 步骤新增字段 =====
    /// Agent 系统提示词
    pub agent_system_prompt: Option<String>,
    /// Agent 输入（模板表达式）
    pub agent_input: Option<String>,
    /// Agent 最大迭代次数
    pub agent_max_iterations: Option<u32>,
    
    // ===== Tool 步骤新增字段 =====
    /// 工具名称
    pub tool_name: Option<String>,
    /// 工具参数（JSON 字符串或模板表达式）
    pub tool_args: Option<String>,
}
```

### 8.3 ExecutionContext 扩展

```rust
pub struct ExecutionContext {
    // ... 现有字段 ...
    
    /// 工具注册表（Workflow 级共享）
    pub tool_registry: Option<Arc<ToolRegistry>>,
    
    /// Agent 管理器（Workflow 级共享）
    pub agent_manager: Option<Arc<AgentManager>>,
}
```

### 8.4 AgentExecutor (`executors/agent_executor.rs`)

```rust
pub struct AgentExecutor {
    agent_manager: Arc<AgentManager>,
}

impl AgentExecutor {
    pub fn new(agent_manager: Arc<AgentManager>) -> Self {
        Self { agent_manager }
    }
}

#[async_trait::async_trait]
impl Executor for AgentExecutor {
    async fn execute(
        &self,
        step: &StepDefinition,
        context: &ExecutionContext,
    ) -> Result<StepResult, WorkflowError> {
        let started_at = chrono::Utc::now();

        // 1. 创建会话
        let session_id = self.agent_manager
            .create_session(step.agent_system_prompt.as_deref())
            .await
            .map_err(|e| WorkflowError::Other(format!("Failed to create session: {}", e)))?;

        // 2. 解析输入（模板表达式）
        let input_template = step.agent_input.as_deref()
            .ok_or_else(|| WorkflowError::Other("Missing agent_input".to_string()))?;
        let input = context.resolve_template(input_template)?;

        // 3. 设置最大迭代次数
        if let Some(max_iter) = step.agent_max_iterations {
            self.agent_manager.set_max_iterations(&session_id, max_iter).await
                .map_err(|e| WorkflowError::Other(format!("Failed to set max iterations: {}", e)))?;
        }

        // 4. 运行 Agent
        let result = self.agent_manager
            .run_sync(&session_id, &input, None)
            .await
            .map_err(|e| WorkflowError::Other(format!("Agent execution failed: {}", e)))?;

        // 5. 销毁会话
        self.agent_manager.destroy_session(&session_id).await;

        let completed_at = chrono::Utc::now();
        let duration_ms = (completed_at - started_at).num_milliseconds() as u64;

        Ok(StepResult::success(
            step.id.clone(),
            serde_json::json!({ "answer": result }),
        ).with_timing(started_at, duration_ms))
    }
}
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

### 8.5 ToolExecutor (`executors/tool_executor.rs`)

```rust
pub struct ToolExecutor {
    tool_registry: Arc<ToolRegistry>,
}

impl ToolExecutor {
    pub fn new(tool_registry: Arc<ToolRegistry>) -> Self {
        Self { tool_registry }
    }
}

#[async_trait::async_trait]
impl Executor for ToolExecutor {
    async fn execute(
        &self,
        step: &StepDefinition,
        context: &ExecutionContext,
    ) -> Result<StepResult, WorkflowError> {
        let started_at = chrono::Utc::now();

        // 1. 获取工具名称
        let tool_name = step.tool_name.as_deref()
            .ok_or_else(|| WorkflowError::Other("Missing tool_name".to_string()))?;

        // 2. 解析工具参数（模板表达式）
        let args_template = step.tool_args.as_deref().unwrap_or("{}");
        let args = context.resolve_template(args_template)?;

        // 3. 检查工具是否存在
        if !self.tool_registry.has_tool(tool_name).await {
            return Err(WorkflowError::Other(format!(
                "Tool '{}' not found in registry",
                tool_name
            )));
        }

        // 4. 执行工具
        let result = self.tool_registry.execute(tool_name, &args).await;

        // 5. 构建结果
        let completed_at = chrono::Utc::now();
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
            // 尝试解析为 JSON
            let output = serde_json::from_str::<serde_json::Value>(&result.content)
                .unwrap_or_else(|_| serde_json::Value::String(result.content));

            Ok(StepResult::success(step.id.clone(), output)
                .with_timing(started_at, duration_ms))
        }
    }
}
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

### 8.6 executors/mod.rs 更新

```rust
pub mod http;
pub mod shell;
pub mod r#loop;
pub mod condition;
pub mod workflow;
pub mod approve;
pub mod agent_executor;    // 新增
pub mod tool_executor;     // 新增
```

---

## 9. 工具注册配置

### 9.1 Workflow YAML 中的工具定义

在 Workflow 级别配置工具，所有步骤共享:

```yaml
name: "工具增强的 AI 工作流"

# 全局工具定义（注册到 ToolRegistry）
tools:
  - name: search_docs
    description: "搜索技术文档"
    json_schema: |
      {
        "type": "object",
        "properties": {
          "query": {"type": "string"}
        }
      }
    # 工具实现方式
    source: builtin  # builtin | custom

  - name: read_file
    description: "读取文件内容"
    json_schema: |
      {
        "type": "object",
        "properties": {
          "path": {"type": "string"}
        }
      }
    source: builtin

steps:
  # ToolExecutor 直接调用工具
  - id: read_config
    type: tool
    tool_name: read_file
    tool_args: '{"path": "/etc/config.yaml"}'

  # AgentExecutor 也可以使用相同的工具
  - id: ai_analysis
    type: agent
    agent_input: "分析配置文件..."
    # Agent 自动可以使用所有已注册的工具
```

### 9.2 内置工具 (builtin)

| 工具名 | 说明 |
|:---|:---|
| `read_file` | 读取本地文件 |
| `write_file` | 写入本地文件 |
| `http_request` | HTTP 请求 |
| `execute_shell` | Shell 命令执行 |
| `query_workflow` | 查询 workflow 状态/输出 |

---

## 10. 错误处理

### 10.1 AgentError 定义 (`agent/error.rs`)

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
    
    #[error("Serialization error: {0}")]
    SerializationError(String),
    
    #[error("Configuration error: {0}")]
    ConfigError(String),
}
```

---

## 11. 不翻译的组件

| C++ 组件 | Rust 替代 | 说明 |
|:---|:---|:---|
| `ThreadPool` | `tokio::runtime::Handle` + `tokio::spawn` | FlowRun 已全面使用 tokio |
| `Logger` | `tracing::{info!, debug!, warn!, error!}` | FlowRun 已配置 tracing-subscriber |
| `UUIDGenerator` | `uuid::Uuid::new_v4().to_string()` | Cargo.toml 已有 `uuid = "1.6"` |
| `nlohmann/json` | `serde_json::Value` / `serde_json::json!` | 已有依赖 |
| `std::regex` | `regex::Regex` | 已有依赖 |

---

## 12. 实施阶段

### Phase 1: 基础类型层 (无依赖)

| # | 文件 | 源文件 | 说明 |
|:---|:---|:---|:---|
| 1.1 | `src/agent/mod.rs` | — | 模块声明 + re-export |
| 1.2 | `src/agent/types.rs` | `types.hpp` | 所有核心类型定义 |
| 1.3 | `src/agent/error.rs` | — | `thiserror` 错误定义 |
| 1.4 | `src/agent/llm_adapter.rs` | `llm_adapter.hpp` | `LlmProvider` trait |

### Phase 2: 基础设施层 (依赖 Phase 1)

| # | 文件 | 源文件 | 说明 |
|:---|:---|:---|:---|
| 2.1 | `src/agent/util.rs` | `gtht_agent_util.cpp` | 进度数据 + 响应规范化 |
| 2.2 | `src/agent/system_prompt.rs` | `system_prompt_renderer.cpp` | 模板渲染 |
| 2.3 | `src/agent/tool_registry.rs` | `tool_registry.hpp/cpp` | 工具注册执行（**核心共享组件**）|
| 2.4 | `src/agent/response_parser.rs` | `response_parser.hpp/cpp` | XML/JSON 双解析器 |

### Phase 3: ToolExecutor (依赖 Phase 2)

| # | 文件 | 说明 |
|:---|:---|:---|
| 3.1 | `src/executors/tool_executor.rs` | Tool 步骤执行器 |
| 3.2 | `src/core/types.rs` 扩展 | 新增 `StepType::Tool` |
| 3.3 | `src/executors/mod.rs` 更新 | 注册 `pub mod tool_executor;` |
| 3.4 | YAML 解析器更新 | 支持工具定义和 Tool 步骤 |

**验证：ToolExecutor 可独立工作**

### Phase 4: Agent 核心层 (依赖 Phase 1+2)

| # | 文件 | 源文件 | 说明 |
|:---|:---|:---|:---|
| 4.1 | `src/agent/react_agent.rs` | `react_agent.hpp/cpp` + `agent_manager.hpp/cpp` | ReAct 循环 + 会话管理 |

### Phase 5: AgentExecutor 集成 (依赖 Phase 4)

| # | 文件 | 说明 |
|:---|:---|:---|
| 5.1 | `src/executors/agent_executor.rs` | Agent 步骤执行器 |
| 5.2 | `src/core/types.rs` 扩展 | 新增 `StepType::Agent` |
| 5.3 | `src/core/context.rs` 扩展 | 添加 `tool_registry`/`agent_manager` 字段 |
| 5.4 | `src/executors/mod.rs` 更新 | 注册 `pub mod agent_executor;` |

**验证：Agent 和 Tool 共享 ToolRegistry**

### Phase 6: 验证与测试

| # | 任务 | 说明 |
|:---|:---|:---|
| 6.1 | `src/lib.rs` 添加 `pub mod agent;` | 模块注册 |
| 6.2 | 编写 agent 模块单元测试 | types/tool_registry/response_parser/react_agent |
| 6.3 | 编写 executor 集成测试 | AgentExecutor + ToolExecutor + 混合 workflow |
| 6.4 | 创建示例工作流 | Agent + Tool 混合使用 |
| 6.5 | `cargo build` / `cargo test` | 编译验证 |

---

## 13. 依赖变更

无需新增依赖。所有功能可由以下已有 crate 实现:

- `tokio` — 异步运行时
- `serde` + `serde_json` — 序列化
- `regex` — 正则匹配
- `uuid` — 会话 ID
- `tracing` — 日志
- `thiserror` + `anyhow` — 错误处理
- `async-trait` — 异步 trait 支持

---

## 14. 预估工作量

| 阶段 | 文件数 | 预估代码行 | 难度 | 预估时间 |
|:---|:---|:---|:---|:---|
| Phase 1: 基础类型 | 4 | ~200 | 低 | 1-2 天 |
| Phase 2: 基础设施 | 4 | ~400 | 中 | 2-3 天 |
| Phase 3: ToolExecutor | 4 | ~200 | 中 | 2-3 天 |
| Phase 4: Agent 核心 | 1 | ~500 | 高 | 3-4 天 |
| Phase 5: AgentExecutor | 4 | ~300 | 中 | 2-3 天 |
| Phase 6: 验证测试 | — | ~200 | 低 | 1-2 天 |
| **总计** | **17 个文件** | **~1800 行** | — | **11-17 天** |

### 优先级建议

**高优先级（先实现）：**
- Phase 1-3: 基础设施 + ToolExecutor
- 这样即使 Agent 部分延迟，ToolExecutor 也能独立使用

**中优先级：**
- Phase 4-5: Agent 核心和集成

**低优先级：**
- Phase 6: 文档和示例

---

## 15. 验收标准

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

---

## 16. 附录：完整 YAML 示例

### 16.1 Agent + Tool 混合使用

```yaml
name: "工具增强的 AI 工作流"
description: "演示 ToolRegistry 共享给 Agent 和 ToolExecutor"

# 全局工具定义
tools:
  - name: search_docs
    description: "搜索技术文档"
    json_schema: '{"type":"object","properties":{"query":{"type":"string"}}}'
    source: builtin

  - name: read_file
    description: "读取文件内容"
    json_schema: '{"type":"object","properties":{"path":{"type":"string"}}}'
    source: builtin

  - name: write_file
    description: "写入文件"
    json_schema: '{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}}}'
    source: builtin

steps:
  # 步骤 1: 直接使用 ToolExecutor 调用工具
  - id: read_config
    name: "读取配置文件"
    type: tool
    tool_name: read_file
    tool_args: '{"path": "/etc/app/config.yaml"}'

  # 步骤 2: Agent 使用所有已注册的工具进行推理
  - id: ai_diagnosis
    name: "AI 诊断"
    type: agent
    agent_input: |
      根据以下信息诊断问题：
      配置内容: ${{ steps.read_config.output }}
    agent_max_iterations: 5
    depends_on: [read_config]

  # 步骤 3: ToolExecutor 写入修复结果
  - id: save_fix
    name: "保存修复方案"
    type: tool
    tool_name: write_file
    tool_args: '{"path": "/tmp/fix_report.txt", "content": "${{ steps.ai_diagnosis.output.answer }}"}'
    depends_on: [ai_diagnosis]

outputs:
  diagnosis: "${{ steps.ai_diagnosis.output.answer }}"
  report_path: "/tmp/fix_report.txt"
```
