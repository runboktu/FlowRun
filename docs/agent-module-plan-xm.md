# Agent 模块实现计划

## 1. 概述

将 `gtht-agent`（C++ ReAct Agent）的核心功能翻译为 Rust 模块，集成到 `flow-run` 工作流引擎中。

### 目标
- 提供 ReAct（Reasoning + Acting）范式的 AI Agent 能力
- **共享工具系统**：`ToolRegistry` 作为核心组件，同时供 Agent 和独立的 Tool 步骤使用
- 作为新的步骤类型 `agent` 和 `tool` 集成到工作流引擎
- Agent 和 Tool 都是 Executor 的具体实现

### 设计原则
- **Rust 惯用法**：使用 trait、async/await、所有权模型
- **异步优先**：基于 tokio 异步运行时
- **可扩展性**：LLM 接口和工具系统通过 trait 抽象
- **线程安全**：利用 Rust 的类型系统保证安全
- **组件复用**：ToolRegistry 是全局共享的，Agent 和 ToolExecutor 都依赖它

---

## 2. 模块结构

```
src/
├── agent/
│   ├── mod.rs              # 模块导出
│   ├── types.rs            # 类型定义（Message, ToolDescriptor, AgentStatus 等）
│   ├── tool_registry.rs    # 工具注册器（核心共享组件）
│   ├── response_parser.rs  # 响应解析器（XML/JSON）
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

### 架构关系

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

**关键点：**
- `ToolRegistry` 是全局共享组件
- `AgentExecutor` 通过 `ReActAgent` 使用 `ToolRegistry`
- `ToolExecutor` 直接使用 `ToolRegistry`
- 两者可以复用相同的工具定义

---

## 3. 详细设计

### 3.1 类型定义 (`agent/types.rs`)

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

/// 消息结构
pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

pub enum MessageRole {
    System,
    User,
    Assistant,
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
    pub handler: Box<dyn ToolHandler>,
}

/// 工具结果
pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
}
```

### 3.2 核心 Trait

#### LLM Provider Trait
```rust
/// LLM 提供者接口
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// 同步调用 LLM
    async fn call(&self, messages: &[Message]) -> Result<LlmResponse>;
    
    /// 流式调用（可选）
    async fn call_streaming(
        &self,
        messages: &[Message],
        callback: impl Fn(String) + Send + 'static,
    ) -> Result<LlmResponse> {
        // 默认实现：调用 call
        self.call(messages).await
    }
}
```

#### Tool Handler Trait
```rust
/// 工具处理器接口
#[async_trait]
pub trait ToolHandler: Send + Sync {
    /// 执行工具
    async fn execute(&self, args: &str) -> ToolResult;
}

/// 闭包工具适配器
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

### 3.3 响应解析器 (`agent/response_parser.rs`)

```rust
/// 解析器类型
pub enum ParserType {
    Xml,
    Json,
}

/// 响应解析器 trait
pub trait ResponseParser: Send + Sync {
    fn parse(&self, response: &str) -> ParsedResponse;
    fn parse_action(&self, action: &str) -> (String, String);
}

/// 解析结果
pub struct ParsedResponse {
    pub thought: Option<String>,
    pub action: Option<String>,
    pub final_answer: Option<String>,
}

/// XML 解析器
pub struct XmlParser;

/// JSON 解析器
pub struct JsonParser;

/// 解析器工厂
pub fn create_parser(parser_type: ParserType) -> Box<dyn ResponseParser> {
    match parser_type {
        ParserType::Xml => Box::new(XmlParser),
        ParserType::Json => Box::new(JsonParser),
    }
}
```

### 3.4 Tool Registry (`agent/tool_registry.rs`)

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 工具注册器
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
}
```

### 3.5 ReAct Agent (`agent/react_agent.rs`)

```rust
pub struct ReActAgent {
    session_id: String,
    messages: Vec<Message>,
    tool_registry: Arc<ToolRegistry>,
    llm_provider: Arc<dyn LlmProvider>,
    parser: Box<dyn ResponseParser>,
    max_iterations: usize,
    system_prompt_template: String,
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
        }
    }

    /// 同步运行 Agent
    pub async fn run(&mut self, user_input: &str) -> Result<String> {
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
        for iteration in 0..self.max_iterations {
            // 调用 LLM
            let response = self.llm_provider.call(&self.messages).await?;
            
            if !response.success {
                return Err(anyhow::anyhow!("LLM call failed: {}", response.content));
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
                
                let result = self.tool_registry.execute(&tool_name, &args).await;
                
                self.messages.push(Message {
                    role: MessageRole::User,
                    content: format!("<observation>{}</observation>", result.content),
                });
            }
        }

        Err(anyhow::anyhow!("Max iterations reached"))
    }

    /// 带进度回调的运行
    pub async fn run_with_callback(
        &mut self,
        user_input: &str,
        callback: impl Fn(AgentEvent) + Send + 'static,
    ) -> Result<String> {
        // 类似 run()，但在每个阶段调用 callback
        // ...
    }

    /// 渲染系统提示
    async fn render_system_prompt(&self) -> String {
        let tool_list = self.tool_registry.get_tool_list().await;
        self.system_prompt_template
            .replace("${tool_list}", &tool_list)
    }
}
```

### 3.6 Session Manager (`agent/session_manager.rs`)

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// 会话管理器（单例）
pub struct SessionManager {
    llm_provider: Arc<dyn LlmProvider>,
    tool_registry: Arc<ToolRegistry>,
    sessions: Arc<RwLock<HashMap<String, ReActAgent>>>,
}

impl SessionManager {
    pub fn new(llm_provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            llm_provider,
            tool_registry: Arc::new(ToolRegistry::new()),
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

    /// 同步运行
    pub async fn run_sync(&self, session_id: &str, user_input: &str) -> Result<String> {
        let mut sessions = self.sessions.write().await;
        match sessions.get_mut(session_id) {
            Some(agent) => agent.run(user_input).await,
            None => Err(anyhow::anyhow!("Session not found: {}", session_id)),
        }
    }

    /// 注册全局工具
    pub async fn register_tool(&self, descriptor: ToolDescriptor) {
        self.tool_registry.register(descriptor).await;
    }
}
```

### 3.7 Agent 步骤执行器 (`executors/agent.rs`)

```rust
/// Agent 步骤配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStepConfig {
    /// 用户输入/提示
    pub input: String,
    /// LLM 提供者配置
    pub llm_provider: LlmProviderConfig,
    /// 工具列表（引用已注册的工具或内联定义）
    pub tools: Option<Vec<String>>,
    /// 最大迭代次数
    pub max_iterations: Option<usize>,
    /// 系统提示模板
    pub system_prompt: Option<String>,
    /// 响应解析器类型
    pub parser_type: Option<String>,
}

/// Agent 步骤执行器
pub struct AgentExecutor {
    session_manager: Arc<SessionManager>,
    tool_registry: Arc<ToolRegistry>,
}

#[async_trait]
impl Executor for AgentExecutor {
    async fn execute(
        &self,
        step: &StepDefinition,
        context: &ExecutionContext,
    ) -> Result<StepResult, WorkflowError> {
        // 1. 解析 Agent 配置
        let config: AgentStepConfig = parse_agent_config(step)?;

        // 2. 创建或复用会话
        let session_id = self.session_manager
            .create_session(config.system_prompt)
            .await;

        // 3. 如果指定了工具列表，为该会话过滤可用工具
        if let Some(tool_names) = config.tools {
            // Agent 会自动使用 ToolRegistry 中已注册的工具
            // 这里可以设置会话级别的工具过滤
        }

        // 4. 运行 Agent（自动使用共享的 ToolRegistry）
        let result = self.session_manager
            .run_sync(&session_id, &config.input)
            .await?;

        // 5. 返回结果
        Ok(StepResult::success(
            step.id.clone(),
            serde_json::json!({ "answer": result }),
        ))
    }
}
```

### 3.8 Tool 步骤执行器 (`executors/tool.rs`)

```rust
/// Tool 步骤配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStepConfig {
    /// 要执行的工具名称
    pub tool: String,
    /// 工具参数（支持模板表达式）
    pub args: serde_json::Value,
    /// 超时时间（秒）
    pub timeout: Option<u64>,
}

/// Tool 步骤执行器
///
/// 直接调用 ToolRegistry 中已注册的工具，无需 Agent 推理循环
pub struct ToolExecutor {
    tool_registry: Arc<ToolRegistry>,
}

impl ToolExecutor {
    pub fn new(tool_registry: Arc<ToolRegistry>) -> Self {
        Self { tool_registry }
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

        // 1. 解析 Tool 配置
        let config: ToolStepConfig = parse_tool_config(step)?;

        // 2. 解析参数中的模板表达式
        let resolved_args = resolve_templates(&config.args, context)?;
        let args_str = serde_json::to_string(&resolved_args)?;

        // 3. 检查工具是否存在
        if !self.tool_registry.has_tool(&config.tool).await {
            return Err(WorkflowError::Other(format!(
                "Tool '{}' not found in registry",
                config.tool
            )));
        }

        // 4. 执行工具
        let result = if let Some(timeout_secs) = config.timeout {
            // 带超时的执行
            tokio::time::timeout(
                std::time::Duration::from_secs(timeout_secs),
                self.tool_registry.execute(&config.tool, &args_str),
            )
            .await
            .map_err(|_| WorkflowError::Other(format!("Tool '{}' timed out", config.tool)))?
        } else {
            self.tool_registry.execute(&config.tool, &args_str).await
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
            // 尝试解析为 JSON，如果失败则作为字符串
            let output = serde_json::from_str::<serde_json::Value>(&result.content)
                .unwrap_or_else(|_| serde_json::Value::String(result.content));

            Ok(StepResult::success(step.id.clone(), output)
                .with_timing(started_at, duration_ms))
        }
    }
}
```

### 3.9 工具注册的两种方式

工具可以通过两种方式注册到 `ToolRegistry`：

#### 方式 1：全局配置（推荐）
在工作流全局配置中定义工具，所有步骤共享：

```yaml
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

steps:
  # ToolExecutor 直接调用工具
  - id: read_config
    type: tool
    tool: read_file
    args:
      path: "/etc/config.yaml"

  # AgentExecutor 也可以使用相同的工具
  - id: ai_analysis
    type: agent
    input: "分析配置文件..."
    llm_provider: ...
    # Agent 自动可以使用所有已注册的工具
```

---

## 4. YAML 配置示例

### 4.1 完整示例：共享工具的 Agent 和 Tool 步骤

```yaml
name: "工具增强的 AI 工作流"
description: "演示 ToolRegistry 共享给 Agent 和 ToolExecutor"

# 全局工具定义（注册到 ToolRegistry）
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
    implementation: shell
    config:
      run: "echo '${{ args.content }}' > ${{ args.path }}"

steps:
  # 步骤 1: 直接使用 ToolExecutor 调用工具
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
    input: |
      根据以下信息诊断问题：
      配置内容: ${{ steps.read_config.output }}
      相关问题: ${{ steps.search_issues.output }}
    llm_provider:
      type: openai
      model: gpt-4
      api_key: "${{ inputs.openai_api_key }}"
    max_iterations: 5
    # Agent 自动可以使用 search_docs, read_file, write_file 等工具

  # 步骤 4: ToolExecutor 写入修复结果
  - id: save_fix
    name: "保存修复方案"
    type: tool
    tool: write_file
    args:
      path: "/tmp/fix_report.txt"
      content: "${{ steps.ai_diagnosis.output.answer }}"

outputs:
  diagnosis: "${{ steps.ai_diagnosis.output.answer }}"
  report_path: "/tmp/fix_report.txt"
```

### 4.2 ToolExecutor 单独使用示例

```yaml
name: "简单的工具调用工作流"

tools:
  - name: http_get
    description: "发送 HTTP GET 请求"
    implementation: http
    config:
      method: GET

  - name: transform_json
    description: "转换 JSON 数据"
    implementation: inline_script
    config:
      language: python
      script: |
        import json
        data = json.loads(args['input'])
        # 转换逻辑
        result = {"processed": data}
        print(json.dumps(result))

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
```

### 4.3 AgentExecutor 单独使用示例

```yaml
name: "纯 Agent 工作流"

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
    implementation: inline_script
    config:
      language: python
      script: |
        result = eval(args['expression'])
        print(result)

steps:
  - id: math_agent
    type: agent
    input: "计算 (100 * 2 + 50) / 3 的结果"
    llm_provider:
      type: openai
      model: gpt-4
    max_iterations: 3
    system_prompt: "你是一个数学助手，使用 calculator 工具进行计算。"
```

---

## 5. 实现步骤

### Phase 1: 基础类型和接口
- [ ] 创建 `src/agent/mod.rs` 模块结构
- [ ] 实现 `types.rs` - 所有基础类型定义
- [ ] 实现 `LlmProvider` 和 `ToolHandler` trait

### Phase 2: 核心组件 - ToolRegistry（优先实现）
- [ ] 实现 `tool_registry.rs` - 工具注册器（**核心共享组件**）
- [ ] 实现 `response_parser.rs` - XML/JSON 解析器
- [ ] 单元测试

### Phase 3: ToolExecutor（独立于 Agent）
- [ ] 实现 `executors/tool.rs` - Tool 步骤执行器
- [ ] 扩展 `StepType` 枚举添加 `Tool` 类型
- [ ] 更新 YAML 解析器支持工具定义和 Tool 步骤
- [ ] **验证：ToolExecutor 可独立工作**

### Phase 4: Agent 核心
- [ ] 实现 `react_agent.rs` - ReAct 循环
- [ ] 实现 `session_manager.rs` - 会话管理
- [ ] 集成测试

### Phase 5: AgentExecutor 集成
- [ ] 实现 `executors/agent.rs` - Agent 步骤执行器
- [ ] 扩展 `StepType` 枚举添加 `Agent` 类型
- [ ] 更新 YAML 解析器支持 Agent 步骤配置
- [ ] **验证：Agent 和 Tool 共享 ToolRegistry**

### Phase 6: 示例和文档
- [ ] 创建 Agent 工作流示例
- [ ] 创建 Tool 工作流示例
- [ ] 创建 Agent + Tool 混合工作流示例
- [ ] 编写使用文档
- [ ] 性能测试

---

## 6. 依赖

需要添加到 `Cargo.toml`:

```toml
[dependencies]
# 现有依赖...

# Agent 模块需要的额外依赖
async-trait = "0.1"  # 已有
uuid = { version = "1.6", features = ["v4"] }  # 已有
```

---

## 7. 注意事项

### C++ → Rust 映射

| C++ | Rust | 说明 |
|-----|------|------|
| `std::shared_ptr<ThreadPool>` | tokio async tasks | 异步运行时替代线程池 |
| `std::mutex` | `tokio::sync::RwLock` | 异步锁 |
| `std::function<...>` | `Box<dyn Fn(...) -> ...>` 或 trait | 闭包/trait 对象 |
| `std::unordered_map` | `HashMap` | 哈希映射 |
| 单例模式 | 依赖注入 | 更符合 Rust 惯用法 |

### 关键设计决策

1. **不使用线程池**：tokio 运行时已经提供了任务调度
2. **trait 抽象**：LLM 和工具通过 trait 接口解耦
3. **异步优先**：所有可能阻塞的操作都使用 async
4. **所有权清晰**：使用 `Arc` 共享不可变数据，`RwLock` 保护可变状态
5. **ToolRegistry 共享**：全局唯一的工具注册中心，Agent 和 ToolExecutor 都依赖它
6. **ToolExecutor 独立**：可在不使用 Agent 的情况下直接调用工具，更轻量

---

## 8. 测试策略

### 单元测试
- ResponseParser: XML/JSON 解析正确性
- ToolRegistry: 工具注册和执行
- ReActAgent: 单轮/多轮对话逻辑

### 集成测试
- SessionManager: 会话生命周期管理
- AgentExecutor: 与工作流引擎集成

### 端到端测试
- 使用 mock LLM provider 测试完整流程
- 实际 LLM 调用测试（可选）

---

## 9. 时间估算

- Phase 1: 1-2 天
- Phase 2: 2-3 天
- Phase 3: 2-3 天（ToolExecutor）
- Phase 4: 3-4 天（Agent 核心）
- Phase 5: 2-3 天（AgentExecutor 集成）
- Phase 6: 1-2 天（示例和文档）

**总计: 11-17 天**

### 优先级建议

**高优先级（先实现）：**
- Phase 1-3: 基础设施 + ToolExecutor
- 这样即使 Agent 部分延迟，ToolExecutor 也能独立使用

**中优先级：**
- Phase 4-5: Agent 核心和集成

**低优先级：**
- Phase 6: 文档和示例
