# Per-Step 工具注册方案

> 日期：2026-04-09
> 状态：待实施
> 范围：`src/agent/`、`src/executors/`、`src/core/types.rs`、`src/core/parser.rs`

---

## 一、背景与目标

### 1.1 现状问题

当前 `ReActAgent` 的工具通过共享的 `ToolRegistry` 注入，链路如下：

```
Scheduler::new()
  → ToolRegistry::new()           // 空 registry
  → AgentManager::new(tool_registry)  // 共享同一个 registry
  → AgentExecutor::new(agent_manager)
  → ToolExecutor::new(tool_registry)
```

所有 Agent session 共享 `AgentManager.tool_registry`，工具对全局可见，没有隔离。
而且 `ToolRegistry` 始终为空 — 没有任何地方往里注册工具。

### 1.2 目标

1. **工具声明下放到每个 Step 级别** — Agent Step 声明 `agent_tools`（多个），Tool Step 声明 `tool`（单个）
2. **工具仅注册到对应 Agent 实例** — 每个 Agent session 拥有独立的 `ToolRegistry`，session 销毁后释放
3. **AgentManager 不再持有全局 tool_registry** — 只管理 session 生命周期，工具注册下沉到 `ReActAgent`
4. **取消全局工具注册** — 不需要 `WorkflowDefinition.tools`，所有工具都在 Step 内声明
5. **向后兼容** — 无 `agent_tools` 的旧 YAML 仍然正常运行（Agent 无工具可用，纯 LLM 推理）

---

## 二、架构设计

### 2.1 核心思路

```
之前：AgentManager 持有全局 ToolRegistry → 所有 Agent session 共享
之后：每个 ReActAgent 自带独立 ToolRegistry → AgentManager 只管 session 生命周期
```

### 2.2 改动总览

```
┌─────────────────────────────────────────────────────────┐
│  StepDefinition                                          │
│  + agent_tools: Option<Vec<ToolSourceDefinition>>        │
│  + tool: Option<ToolSourceDefinition>                    │
└──────────────────────┬──────────────────────────────────┘
                       │
          ┌────────────┴────────────┐
          ▼                         ▼
  ┌───────────────┐         ┌───────────────┐
  │ Agent Step    │         │ Tool Step     │
  │ agent_tools[] │         │ tool { ... }  │
  └───────┬───────┘         └───────┬───────┘
          │                         │
          ▼                         ▼
  AgentExecutor.execute()    ToolExecutor.execute()
          │                         │
          ▼                         ▼
  ① ReActAgent::new()        create_tool_handler()
     (内部 new ToolRegistry)       ↓
  ② agent.register_tool()    就地执行，丢弃
     × N 个工具
  ③ agent.run()
  ④ 销毁 session
     (ToolRegistry 随之释放)
```

---

## 三、YAML 语法

### 3.1 Agent Step — `agent_tools` 字段

Agent Step 通过 `agent_tools` 声明该 Agent 可用的工具列表（0~N 个）：

```yaml
steps:
  - id: data_analyst
    type: agent
    agent_system_prompt: "你是数据分析专家"
    agent_input: "请分析 /data/sales.csv"
    agent_max_iterations: 15

    # 该 Agent 独享的工具列表
    agent_tools:
      # 内置工具
      - name: read_file
        source: builtin

      # Shell 工具
      - name: grep_data
        description: "在文件中搜索关键词"
        json_schema: |
          {
            "type": "object",
            "properties": {
              "pattern": { "type": "string" },
              "path":    { "type": "string" }
            }
          }
        source: shell
        command: "grep -rn '{{pattern}}' '{{path}}'"

      # HTTP 工具
      - name: query_api
        description: "查询外部数据服务"
        json_schema: |
          {
            "type": "object",
            "properties": {
              "endpoint": { "type": "string" }
            }
          }
        source: http
        url: "https://api.example.com/{{endpoint}}"
        method: POST
        headers:
          Authorization: "Bearer xxx"
        body_template: |
          {"query": {}}

      # Python 工具
      - name: plot_chart
        description: "用 matplotlib 画图表"
        json_schema: |
          {
            "type": "object",
            "properties": {
              "csv_path": { "type": "string" },
              "chart_type": { "type": "string" }
            }
          }
        source: python
        timeout_secs: 30
        script: |
          import pandas as pd, matplotlib.pyplot as plt, json, sys
          args = json.loads(sys.argv[1])
          df = pd.read_csv(args["csv_path"])
          df.plot(kind=args.get("chart_type", "line"))
          out = "/tmp/chart.png"
          plt.savefig(out)
          print(out)
```

**无 `agent_tools` 字段的 Agent Step**：Agent 没有工具，纯 LLM 推理（向后兼容）。

### 3.2 Tool Step — `tool` 字段

Tool Step 通过 `tool` 字段内联声明一个工具定义：

```yaml
steps:
  # 内联工具定义
  - id: fetch_data
    type: tool
    tool:
      name: http_get
      description: "获取数据"
      source: http
      url: "https://api.example.com/data"
      method: GET
    tool_args: '{}'

  # Python 内联工具
  - id: analyze
    type: tool
    tool:
      name: csv_analysis
      source: python
      script: |
        import pandas as pd, json, sys
        args = json.loads(sys.argv[1])
        df = pd.read_csv(args["path"])
        print(df.describe().to_json())
    tool_args: '{"path": "/data/sales.csv"}'
    depends_on: [fetch_data]

  # Shell 内联工具
  - id: count_lines
    type: tool
    tool:
      name: wc_tool
      source: shell
      command: "wc -l '{{path}}'"
    tool_args: '{"path": "/data/sales.csv"}'
    depends_on: [fetch_data]
```

### 3.3 `ToolSourceDefinition` 完整字段

每个工具声明的完整字段：

| 字段 | 类型 | 必填 | 说明 |
|:---|:---|:---|:---|
| `name` | string | ✅ | 工具名称（在当前 step 内唯一） |
| `description` | string | ❌ | 工具描述（给 LLM 看，影响 LLM 决策是否调用） |
| `json_schema` | string | ❌ | 参数 JSON Schema（给 LLM 看的参数说明） |
| `source` | enum | ✅ | `builtin` / `shell` / `http` / `python` |
| `command` | string | shell 必填 | Shell 命令模板，支持 `{{param}}` 占位符 |
| `url` | string | http 必填 | HTTP URL 模板，支持 `{{param}}` 占位符 |
| `method` | string | ❌ | HTTP 方法，默认 `GET` |
| `headers` | map | ❌ | HTTP 请求头 |
| `body_template` | string | ❌ | HTTP Body 模板 |
| `script` | string | python 必填 | Python 脚本内容 |
| `timeout_secs` | u64 | ❌ | 超时秒数，默认 30 |
| `allow_failure` | bool | ❌ | 工具失败时是否中断 Agent 循环，默认 false |

---

## 四、Rust 类型改动

### 4.1 新增：`ToolSourceDefinition` + `ToolSourceType`

文件：`src/core/types.rs`

```rust
/// 工具来源类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ToolSourceType {
    /// 内置工具（Rust 实现，按名称匹配）
    Builtin,
    /// Shell 命令执行
    Shell,
    /// HTTP API 调用
    Http,
    /// Python 脚本执行
    Python,
}

/// 工具来源定义
///
/// 在 YAML 的 agent_tools[] 或 tool 字段中声明。
/// 描述一个工具的元信息和实现来源。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSourceDefinition {
    /// 工具名称
    pub name: String,

    /// 工具描述（给 LLM 看的）
    #[serde(default)]
    pub description: Option<String>,

    /// JSON Schema（描述参数结构，给 LLM 看的）
    #[serde(default)]
    pub json_schema: Option<String>,

    /// 工具来源类型
    pub source: ToolSourceType,

    // ─── Shell 来源字段 ───
    #[serde(default)]
    pub command: Option<String>,

    // ─── HTTP 来源字段 ───
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub body_template: Option<String>,

    // ─── Python 来源字段 ───
    #[serde(default)]
    pub script: Option<String>,

    // ─── 安全配置 ───
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub allow_failure: Option<bool>,
}
```

### 4.2 修改：`StepDefinition` 新增两个字段

文件：`src/core/types.rs`

```rust
pub struct StepDefinition {
    // ... 现有字段全部保留 ...

    // ─── 新增 ───

    /// Agent 步骤：该 Agent 独享的工具列表
    /// 仅 type: agent 时有效
    #[serde(default)]
    pub agent_tools: Option<Vec<ToolSourceDefinition>>,

    /// Tool 步骤：内联工具定义
    /// 仅 type: tool 时有效，与 tool_name 互斥
    /// tool 有值时优先使用内联定义，tool_name 回退到全局查找
    #[serde(default)]
    pub tool: Option<ToolSourceDefinition>,
}
```

`StepDefinition::default()` 同步新增：

```rust
agent_tools: None,
tool: None,
```

### 4.3 修改：`AgentManager` 移除 `tool_registry` 字段

文件：`src/agent/react_agent.rs`

#### 改前：

```rust
pub struct AgentManager {
    sessions: Arc<RwLock<HashMap<String, ReActAgent>>>,
    default_llm: Option<Arc<dyn LlmProvider>>,
    tool_registry: Arc<ToolRegistry>,        // ← 删除此字段
}

impl AgentManager {
    pub fn new(tool_registry: Arc<ToolRegistry>) -> Self { ... }
}
```

#### 改后：

```rust
pub struct AgentManager {
    sessions: Arc<RwLock<HashMap<String, ReActAgent>>>,
    default_llm: Option<Arc<dyn LlmProvider>>,
    // 不再有 tool_registry 字段
}

impl AgentManager {
    /// 创建 AgentManager（不需要 ToolRegistry 参数）
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            default_llm: None,
        }
    }

    // with_llm 保持不变
    pub fn with_llm(mut self, provider: Arc<dyn LlmProvider>) -> Self {
        self.default_llm = Some(provider);
        self
    }
}
```

#### `create_session` / `create_session_with_llm` 改动：

每个 session 创建时，ReActAgent 内部自行 new 一个空的 ToolRegistry：

```rust
pub async fn create_session(&self, user_prompt: Option<&str>) -> Result<String, AgentError> {
    let session_id = Uuid::new_v4().to_string();
    let llm = self.default_llm.clone()
        .ok_or_else(|| AgentError::ConfigError("No LLM provider set".to_string()))?;

    // ReActAgent::new 内部自行创建空的 ToolRegistry
    let mut agent = ReActAgent::new(session_id.clone(), llm);

    if let Some(prompt) = user_prompt {
        agent.set_user_prompt(prompt);
    }

    info!("[AgentManager] Created session: {}", session_id);
    self.sessions.write().await.insert(session_id.clone(), agent);
    Ok(session_id)
}

pub async fn create_session_with_llm(
    &self,
    llm: Arc<dyn LlmProvider>,
    user_prompt: Option<&str>,
) -> Result<String, AgentError> {
    let session_id = Uuid::new_v4().to_string();

    let mut agent = ReActAgent::new(session_id.clone(), llm);

    if let Some(prompt) = user_prompt {
        agent.set_user_prompt(prompt);
    }

    info!("[AgentManager] Created session: {}", session_id);
    self.sessions.write().await.insert(session_id.clone(), agent);
    Ok(session_id)
}
```

#### `register_tool` 改动 — 增加 `session_id` 参数：

```rust
// 改前：
pub async fn register_tool(&self, descriptor: ToolDescriptor) {
    self.tool_registry.register(descriptor).await;
}

// 改后：根据 session_id 找到对应的 ReActAgent，往其内部 registry 注册
pub async fn register_tool(&self, session_id: &str, descriptor: ToolDescriptor) -> Result<(), AgentError> {
    let sessions = self.sessions.read().await;
    let agent = sessions.get(session_id)
        .ok_or_else(|| AgentError::SessionNotFound(session_id.to_string()))?;
    agent.register_tool(descriptor).await;
    Ok(())
}
```

#### 删除 `tool_registry()` getter：

```rust
// 删除此方法
// pub fn tool_registry(&self) -> &Arc<ToolRegistry> {
//     &self.tool_registry
// }
```

### 4.4 `ReActAgent::new` 改动 — 内部自行创建 ToolRegistry

文件：`src/agent/react_agent.rs`

```rust
// 改前：
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

// 改后：
pub fn new(
    session_id: String,
    llm_provider: Arc<dyn LlmProvider>,
) -> Self {
    Self {
        session_id,
        messages: Vec::new(),
        tool_registry: Arc::new(ToolRegistry::new()),   // 内部自行创建
        llm_provider,
        parser: create_parser(ParserType::Xml),
        max_iterations: 10,
        system_prompt_template: DEFAULT_SYSTEM_PROMPT.to_string(),
        user_prompt: String::new(),
    }
}
```

`ReActAgent` 的 `register_tool`、`tool_registry` 其他用法不变 — 只是 registry 的创建者从外部传入变成了内部 new。

### 4.5 `Scheduler::new` 改动 — 不再创建共享 ToolRegistry

文件：`src/core/dag.rs`

```rust
// 改前：
impl Scheduler {
    pub fn new(
        dag: DagScheduler,
        config: WorkflowConfig,
        checkpoint_manager: CheckpointManager,
    ) -> Self {
        let tool_registry = Arc::new(crate::agent::ToolRegistry::new());
        let agent_manager = Arc::new(crate::agent::AgentManager::new(tool_registry.clone()));

        Self {
            dag,
            context: Arc::new(RwLock::new(ExecutionContext::empty())),
            config,
            checkpoint_manager,
            workflow_executor: Arc::new(WorkflowExecutor::new(Arc::new(NullWorkflowRunner))),
            approve_executor: Arc::new(ApproveExecutor::new()),
            agent_executor: Arc::new(AgentExecutor::new(agent_manager)),
            tool_executor: Arc::new(ToolExecutor::new(tool_registry)),
            workflow_outputs: Arc::new(RwLock::new(None)),
        }
    }
}

// 改后：
impl Scheduler {
    pub fn new(
        dag: DagScheduler,
        config: WorkflowConfig,
        checkpoint_manager: CheckpointManager,
    ) -> Self {
        let agent_manager = Arc::new(crate::agent::AgentManager::new());  // 无参构造

        Self {
            dag,
            context: Arc::new(RwLock::new(ExecutionContext::empty())),
            config,
            checkpoint_manager,
            workflow_executor: Arc::new(WorkflowExecutor::new(Arc::new(NullWorkflowRunner))),
            approve_executor: Arc::new(ApproveExecutor::new()),
            agent_executor: Arc::new(AgentExecutor::new(agent_manager)),
            tool_executor: Arc::new(ToolExecutor::new()),  // 无需共享 registry
            workflow_outputs: Arc::new(RwLock::new(None)),
        }
    }
}
```

`with_workflow_executor` 方法同步改动。

---

## 五、执行器改动

### 5.1 `AgentExecutor` — Per-Step 工具注册

文件：`src/executors/agent_executor.rs`

核心改动：`execute()` 中，创建 session 后，根据 `step.agent_tools` 往该 session 注册工具。

```rust
#[async_trait::async_trait]
impl Executor for AgentExecutor {
    async fn execute(
        &self,
        step: &StepDefinition,
        context: &ExecutionContext,
    ) -> Result<StepResult, WorkflowError> {
        let started_at = Utc::now();
        let step_id = &step.id;

        let agent_system_prompt = step.agent_system_prompt.as_deref();
        let llm_config = parse_llm_provider_config(step, context)?;
        let llm = create_llm_provider(&llm_config)
            .map_err(|e| WorkflowError::Other(format!("Failed to create LLM provider: {}", e)))?;

        // 创建 session（ReActAgent 内部自带空 ToolRegistry）
        let session_id = self.agent_manager
            .create_session_with_llm(llm, agent_system_prompt)
            .await
            .map_err(|e| WorkflowError::Other(format!("Failed to create session: {}", e)))?;

        // ═══ 核心新增：注册 Per-Step 工具 ═══
        if let Some(tool_defs) = &step.agent_tools {
            for tool_def in tool_defs {
                let handler = create_tool_handler(tool_def)?;
                let descriptor = ToolDescriptor {
                    name: tool_def.name.clone(),
                    description: tool_def.description.clone().unwrap_or_default(),
                    json_schema: tool_def.json_schema.clone(),
                    handler,
                };
                self.agent_manager
                    .register_tool(&session_id, descriptor)
                    .await
                    .map_err(|e| WorkflowError::Other(
                        format!("Failed to register tool '{}': {}", tool_def.name, e)
                    ))?;
            }
            tracing::info!(
                "[AgentExecutor] Registered {} tools for agent step '{}'",
                tool_defs.len(), step_id
            );
        }

        // 以下逻辑不变
        let input = resolve_agent_input(step, context)?;

        if let Some(max_iter) = step.agent_max_iterations {
            self.agent_manager.set_max_iterations(&session_id, max_iter).await
                .map_err(|e| WorkflowError::Other(format!("Failed to set max iterations: {}", e)))?;
        }

        let use_stream = step.agent_stream.unwrap_or(false);
        let result = if use_stream {
            let callback = build_stream_callback();
            self.agent_manager
                .run_sync_stream(&session_id, &input, callback)
                .await
                .map_err(|e| WorkflowError::Other(format!("Agent stream execution failed: {}", e)))?
        } else {
            self.agent_manager
                .run_sync(&session_id, &input, None)
                .await
                .map_err(|e| WorkflowError::Other(format!("Agent execution failed: {}", e)))?
        };

        self.agent_manager.destroy_session(&session_id).await;

        let completed_at = Utc::now();
        let duration_ms = (completed_at - started_at).num_milliseconds() as u64;

        Ok(StepResult::success(
            step_id.clone(),
            serde_json::json!({ "answer": result }),
        ).with_timing(started_at, duration_ms))
    }
}
```

提取 `resolve_agent_input` 辅助函数：

```rust
fn resolve_agent_input(
    step: &StepDefinition,
    context: &ExecutionContext,
) -> Result<String, WorkflowError> {
    let input_template = step.agent_input.as_deref()
        .ok_or_else(|| WorkflowError::Other("Missing agent_input".to_string()))?;
    let template_context = build_template_context(context);
    crate::core::template::TemplateEngine::new()
        .resolve_template(input_template, &template_context)
        .map_err(|e| WorkflowError::Other(format!("Template error: {}", e)))
}
```

### 5.2 `ToolExecutor` — 支持内联工具

文件：`src/executors/tool_executor.rs`

```rust
/// Tool 步骤执行器
pub struct ToolExecutor {
    // 不再持有全局 tool_registry
}

impl ToolExecutor {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait::async_trait]
impl Executor for ToolExecutor {
    async fn execute(
        &self,
        step: &StepDefinition,
        context: &ExecutionContext,
    ) -> Result<StepResult, WorkflowError> {
        let started_at = Utc::now();
        let step_id = &step.id;

        let args_template = step.tool_args.as_deref().unwrap_or("{}");
        let template_context = build_template_context(context);
        let args = crate::core::template::TemplateEngine::new()
            .resolve_template(args_template, &template_context)
            .map_err(|e| WorkflowError::Other(format!("Template error: {}", e)))?;

        // ═══ 核心分支：内联工具 vs tool_name 引用 ═══
        let result = if let Some(tool_def) = &step.tool {
            // ── 内联模式：就地创建 handler，执行，丢弃 ──
            let handler = create_tool_handler(tool_def)?;
            handler.execute(&args).await
        } else if let Some(tool_name) = &step.tool_name {
            // ── 兼容模式：通过 tool_name 引用（需 FlowRunner 注入全局 registry，或废弃） ──
            return Err(WorkflowError::Other(format!(
                "Tool '{}' not found. Use inline 'tool:' definition instead of 'tool_name'.",
                tool_name
            )));
        } else {
            return Err(WorkflowError::Other(
                "Tool step must have 'tool' (inline definition)".to_string()
            ));
        };

        let completed_at = Utc::now();
        let duration_ms = (completed_at - started_at).num_milliseconds() as u64;

        if result.is_error {
            Ok(StepResult::failed(
                step_id.clone(),
                StepError { code: "TOOL_ERROR".into(), message: result.content, fix: None },
            ))
        } else {
            let output: Value = serde_json::from_str(&result.content)
                .unwrap_or_else(|_| Value::String(result.content));
            Ok(StepResult::success(step_id.clone(), output).with_timing(started_at, duration_ms))
        }
    }
}
```

> **注意**：`tool_name` 引用全局注册表的方式在本次改造中标记为废弃。
> 如果后续需要恢复，可以通过 FlowRunner 注入全局 ToolRegistry 到 ToolExecutor。

---

## 六、工具工厂

### 6.1 新文件：`src/agent/tool_factory.rs`

统一的工具创建入口，AgentExecutor 和 ToolExecutor 共用：

```rust
//! 工具工厂 — 根据 ToolSourceDefinition 创建 ToolHandler 实例

use std::sync::Arc;
use crate::core::types::{ToolSourceDefinition, ToolSourceType};
use crate::agent::types::ToolHandler;
use crate::utils::error::WorkflowError;

/// 根据 ToolSourceDefinition 创建 ToolHandler
pub fn create_tool_handler(def: &ToolSourceDefinition) -> Result<Arc<dyn ToolHandler>, WorkflowError> {
    match &def.source {
        ToolSourceType::Builtin => create_builtin_tool(&def.name),

        ToolSourceType::Shell => {
            let command = def.command.clone().ok_or_else(|| WorkflowError::Other(
                format!("Shell tool '{}' requires 'command' field", def.name)
            ))?;
            Ok(Arc::new(ShellTool::new(command, def.timeout_secs)))
        }

        ToolSourceType::Http => {
            let url = def.url.clone().ok_or_else(|| WorkflowError::Other(
                format!("HTTP tool '{}' requires 'url' field", def.name)
            ))?;
            Ok(Arc::new(HttpTool::new(
                url,
                def.method.clone(),
                def.headers.clone(),
                def.body_template.clone(),
                def.timeout_secs,
            )))
        }

        ToolSourceType::Python => {
            let script = def.script.clone().ok_or_else(|| WorkflowError::Other(
                format!("Python tool '{}' requires 'script' field", def.name)
            ))?;
            Ok(Arc::new(PythonTool::new(script, def.timeout_secs)))
        }
    }
}
```

### 6.2 新文件：`src/agent/tool_implementations.rs`

四种 ToolHandler 实现：

#### ShellTool

```rust
use std::time::Duration;
use crate::agent::types::{ToolHandler, ToolResult};

/// Shell 命令工具
pub struct ShellTool {
    command_template: String,
    timeout: Duration,
}

impl ShellTool {
    pub fn new(command_template: String, timeout_secs: Option<u64>) -> Self {
        Self {
            command_template,
            timeout: Duration::from_secs(timeout_secs.unwrap_or(30)),
        }
    }
}

#[async_trait::async_trait]
impl ToolHandler for ShellTool {
    async fn execute(&self, args: &str) -> ToolResult {
        let command = match render_command_template(&self.command_template, args) {
            Ok(cmd) => cmd,
            Err(e) => return ToolResult::error(e),
        };

        let output = tokio::time::timeout(
            self.timeout,
            tokio::process::Command::new("sh")
                .arg("-c")
                .arg(&command)
                .output(),
        ).await;

        match output {
            Ok(Ok(output)) => {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    ToolResult::success(stdout)
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    ToolResult::error(format!("Exit {}: {}", output.status, stderr))
                }
            }
            Ok(Err(e)) => ToolResult::error(format!("Execution failed: {}", e)),
            Err(_) => ToolResult::error(format!("Timed out after {:?}", self.timeout)),
        }
    }
}
```

#### HttpTool

```rust
/// HTTP API 工具
pub struct HttpTool {
    url_template: String,
    method: String,
    headers: HashMap<String, String>,
    body_template: Option<String>,
    timeout: Duration,
}

impl HttpTool {
    pub fn new(
        url_template: String,
        method: Option<String>,
        headers: Option<HashMap<String, String>>,
        body_template: Option<String>,
        timeout_secs: Option<u64>,
    ) -> Self {
        Self {
            url_template,
            method: method.unwrap_or_else(|| "GET".to_string()),
            headers: headers.unwrap_or_default(),
            body_template,
            timeout: Duration::from_secs(timeout_secs.unwrap_or(30)),
        }
    }
}

#[async_trait::async_trait]
impl ToolHandler for HttpTool {
    async fn execute(&self, args: &str) -> ToolResult {
        let params: HashMap<String, serde_json::Value> =
            serde_json::from_str(args).unwrap_or_default();

        let url = render_template(&self.url_template, &params);

        let client = reqwest::Client::new();
        let mut request = match self.method.to_uppercase().as_str() {
            "POST" => client.post(&url),
            "PUT" => client.put(&url),
            "DELETE" => client.delete(&url),
            "PATCH" => client.patch(&url),
            _ => client.get(&url),
        };

        for (key, value) in &self.headers {
            request = request.header(key, value);
        }

        if let Some(body_tpl) = &self.body_template {
            let body = render_template(body_tpl, &params);
            request = request
                .header("Content-Type", "application/json")
                .body(body);
        }

        let result = tokio::time::timeout(self.timeout, request.send()).await;

        match result {
            Ok(Ok(response)) => {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                if status.is_success() {
                    ToolResult::success(body)
                } else {
                    ToolResult::error(format!("HTTP {}: {}", status, body))
                }
            }
            Ok(Err(e)) => ToolResult::error(format!("HTTP request failed: {}", e)),
            Err(_) => ToolResult::error(format!("HTTP request timed out after {:?}", self.timeout)),
        }
    }
}
```

#### PythonTool

```rust
/// Python 脚本工具
/// 执行 python3 -c <script> <args_json>
/// 脚本通过 print() 输出结果，通过 sys.argv[1] 获取参数
pub struct PythonTool {
    script: String,
    timeout: Duration,
}

impl PythonTool {
    pub fn new(script: String, timeout_secs: Option<u64>) -> Self {
        Self {
            script,
            timeout: Duration::from_secs(timeout_secs.unwrap_or(60)),
        }
    }
}

#[async_trait::async_trait]
impl ToolHandler for PythonTool {
    async fn execute(&self, args: &str) -> ToolResult {
        let output = tokio::time::timeout(
            self.timeout,
            tokio::process::Command::new("python3")
                .arg("-c")
                .arg(&self.script)
                .arg(args)
                .output(),
        ).await;

        match output {
            Ok(Ok(output)) => {
                if output.status.success() {
                    ToolResult::success(String::from_utf8_lossy(&output.stdout).to_string())
                } else {
                    ToolResult::error(String::from_utf8_lossy(&output.stderr).to_string())
                }
            }
            Ok(Err(e)) => ToolResult::error(format!("Python execution failed: {}", e)),
            Err(_) => ToolResult::error(format!("Python timed out after {:?}", self.timeout)),
        }
    }
}
```

#### Builtin 工具

```rust
/// 内置工具 — 按 name 匹配 Rust 原生实现
fn create_builtin_tool(name: &str) -> Result<Arc<dyn ToolHandler>, WorkflowError> {
    match name {
        "read_file" => Ok(Arc::new(FnTool(|args: String| async move {
            let parsed: serde_json::Value = serde_json::from_str(&args).unwrap_or_default();
            let path = parsed["path"].as_str().unwrap_or("");
            tokio::fs::read_to_string(path)
                .await
                .unwrap_or_else(|e| format!("Error reading file: {}", e))
        }))),

        "write_file" => Ok(Arc::new(FnTool(|args: String| async move {
            let parsed: serde_json::Value = serde_json::from_str(&args).unwrap_or_default();
            let path = parsed["path"].as_str().unwrap_or("");
            let content = parsed["content"].as_str().unwrap_or("");
            match tokio::fs::write(path, content).await {
                Ok(()) => format!("Successfully wrote to {}", path),
                Err(e) => format!("Error writing file: {}", e),
            }
        }))),

        "list_directory" => Ok(Arc::new(FnTool(|args: String| async move {
            let parsed: serde_json::Value = serde_json::from_str(&args).unwrap_or_default();
            let path = parsed["path"].as_str().unwrap_or(".");
            match tokio::fs::read_dir(path).await {
                Ok(mut dir) => {
                    let mut entries = Vec::new();
                    while let Ok(Some(entry)) = dir.next_entry().await {
                        entries.push(entry.file_name().to_string_lossy().to_string());
                    }
                    entries.join("\n")
                }
                Err(e) => format!("Error listing directory: {}", e)
            }
        }))),

        "http_get" => Ok(Arc::new(FnTool(|args: String| async move {
            let parsed: serde_json::Value = serde_json::from_str(&args).unwrap_or_default();
            let url = parsed["url"].as_str().unwrap_or("");
            match reqwest::get(url).await {
                Ok(r) => r.text().await.unwrap_or_default(),
                Err(e) => format!("HTTP error: {}", e),
            }
        }))),

        _ => Err(WorkflowError::Other(format!(
            "Unknown builtin tool: '{}'. Available: read_file, write_file, list_directory, http_get",
            name
        ))),
    }
}
```

#### 模板渲染辅助

```rust
/// 渲染命令模板：将 {{param}} 替换为参数值
fn render_command_template(template: &str, args_json: &str) -> Result<String, String> {
    let args: HashMap<String, serde_json::Value> =
        serde_json::from_str(args_json)
            .map_err(|e| format!("Invalid JSON args: {}", e))?;

    let mut result = template.to_string();
    for (key, value) in &args {
        let str_val = match value {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        result = result.replace(&format!("{{{{{}}}}}", key), &str_val);
    }
    Ok(result)
}

/// 通用模板渲染（用于 URL、Body 等）
fn render_template(template: &str, params: &HashMap<String, serde_json::Value>) -> String {
    let mut result = template.to_string();
    for (key, value) in params {
        let str_val = match value {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        result = result.replace(&format!("{{{{{}}}}}", key), &str_val);
    }
    result
}
```

---

## 七、验证增强

文件：`src/core/parser.rs`

```rust
impl WorkflowParser {
    pub fn validate(workflow: &WorkflowDefinition) -> Result<(), WorkflowError> {
        Self::validate_step_ids(&workflow.steps)?;
        Self::validate_dependencies(&workflow.steps)?;
        Self::validate_no_cycles(&workflow.steps)?;
        Self::validate_step_tools(&workflow.steps)?;  // 新增
        Ok(())
    }

    /// 递归验证步骤中的工具声明
    fn validate_step_tools(steps: &[StepDefinition]) -> Result<(), WorkflowError> {
        for step in steps {
            match &step.r#type {
                StepType::Agent => {
                    if let Some(tools) = &step.agent_tools {
                        Self::validate_tool_definitions(tools, &format!("agent step '{}'", step.id))?;
                    }
                }
                StepType::Tool => {
                    if let Some(tool_def) = &step.tool {
                        Self::validate_single_tool(tool_def, &format!("tool step '{}'", step.id))?;
                    }
                    // tool 和 tool_name 都没有时报错
                    if step.tool.is_none() && step.tool_name.is_none() {
                        return Err(WorkflowError::SchemaValidationError {
                            message: format!(
                                "Tool step '{}' must have 'tool' (inline definition)",
                                step.id
                            ),
                        });
                    }
                }
                _ => {}
            }

            // 递归子步骤
            if let Some(sub_steps) = &step.steps {
                Self::validate_step_tools(sub_steps)?;
            }
            if let Some(then_steps) = &step.then_steps {
                Self::validate_step_tools(then_steps)?;
            }
            if let Some(else_steps) = &step.else_steps {
                Self::validate_step_tools(else_steps)?;
            }
            if let Some(do_steps) = &step.do_steps {
                Self::validate_step_tools(do_steps)?;
            }
        }
        Ok(())
    }

    /// 验证工具列表（名称唯一性 + 逐个验证）
    fn validate_tool_definitions(
        tools: &[ToolSourceDefinition],
        context: &str,
    ) -> Result<(), WorkflowError> {
        let mut names = std::collections::HashSet::new();
        for tool in tools {
            if !names.insert(&tool.name) {
                return Err(WorkflowError::SchemaValidationError {
                    message: format!("Duplicate tool name '{}' in {}", tool.name, context),
                });
            }
            Self::validate_single_tool(tool, context)?;
        }
        Ok(())
    }

    /// 验证单个工具定义的必填字段
    fn validate_single_tool(
        tool: &ToolSourceDefinition,
        context: &str,
    ) -> Result<(), WorkflowError> {
        match &tool.source {
            ToolSourceType::Builtin => { /* 只需 name，无需额外字段 */ }
            ToolSourceType::Shell if tool.command.is_none() => {
                return Err(WorkflowError::SchemaValidationError {
                    message: format!("Shell tool '{}' in {} requires 'command'", tool.name, context),
                });
            }
            ToolSourceType::Http if tool.url.is_none() => {
                return Err(WorkflowError::SchemaValidationError {
                    message: format!("HTTP tool '{}' in {} requires 'url'", tool.name, context),
                });
            }
            ToolSourceType::Python if tool.script.is_none() => {
                return Err(WorkflowError::SchemaValidationError {
                    message: format!("Python tool '{}' in {} requires 'script'", tool.name, context),
                });
            }
            _ => {}
        }
        Ok(())
    }
}
```

---

## 八、module 导出

### `src/agent/mod.rs` 新增：

```rust
pub mod tool_factory;
pub mod tool_implementations;

pub use tool_factory::create_tool_handler;
pub use tool_implementations::{ShellTool, HttpTool, PythonTool};
```

### `src/lib.rs` 新增 re-export：

```rust
pub use core::types::{ToolSourceDefinition, ToolSourceType};
```

---

## 九、完整数据流图

### 9.1 Agent Step 执行

```
YAML
  steps:
    - id: analyst
      type: agent
      agent_tools:
        - name: read_file
          source: builtin
        - name: search
          source: shell
          command: "grep '{{query}}' /data/**"
      agent_input: "分析数据"
      agent_system_prompt: "你是分析师"

                    │ 解析
                    ▼
StepDefinition {
  type: Agent,
  agent_tools: Some([
    ToolSourceDefinition { name: "read_file", source: Builtin },
    ToolSourceDefinition { name: "search", source: Shell, command: Some(...) },
  ]),
  ...
}

                    │ AgentExecutor.execute()
                    ▼
① self.agent_manager.create_session_with_llm(llm, prompt)
   → ReActAgent::new(session_id, llm)
      内部: self.tool_registry = Arc::new(ToolRegistry::new())   // 空 registry

② for tool_def in step.agent_tools:
     handler = create_tool_handler(tool_def)
     self.agent_manager.register_tool(&session_id, ToolDescriptor { ... })
       → sessions.get(session_id).register_tool(descriptor)
         → agent.tool_registry.register(descriptor)

③ 此时 agent 的 registry 中有: read_file(Builtin), search(Shell)

④ agent.run(input)
   → LLM 返回 <action>{"name":"search","parameters":{"query":"error"}}</action>
   → tool_registry.has_tool("search") = true
   → tool_registry.execute("search", '{"query":"error"}')
     → ShellTool.execute()
       → sh -c "grep 'error' /data/**"
       → ToolResult::success(stdout)

⑤ self.agent_manager.destroy_session(session_id)
   → ReActAgent 从 HashMap 移除，Arc<ToolRegistry> 引用计数归零，释放
```

### 9.2 Tool Step 执行

```
YAML
  steps:
    - id: export
      type: tool
      tool:
        name: csv_export
        source: python
        script: "import pandas as pd..."
      tool_args: '{"path": "/tmp/out.csv"}'

                    │ ToolExecutor.execute()
                    ▼
① step.tool.is_some() → 内联模式

② handler = create_tool_handler(step.tool)
   → source == Python → PythonTool::new(script, timeout)

③ handler.execute(tool_args)
   → python3 -c <script> '{"path": "/tmp/out.csv"}'
   → ToolResult::success(stdout)

④ 返回 StepResult
   handler 被 drop，无残留
```

---

## 十、受影响的现有测试

需要同步修改的测试：

| 文件 | 改动 |
|:---|:---|
| `src/agent/react_agent.rs` tests | `ReActAgent::new` 不再需要 `tool_registry` 参数 |
| `src/agent/react_agent.rs` tests | `AgentManager::new()` 不再需要参数 |
| `src/core/dag.rs` tests | `Scheduler::new` 不再需要传入 registry |
| `src/core/parser.rs` tests | `StepDefinition::default()` 新增 `agent_tools: None, tool: None` |
| `src/executors/tool_executor.rs` tests | `ToolExecutor::new()` 无参构造 |

### react_agent.rs 测试改动示例

```rust
// 改前：
let tool_registry = Arc::new(ToolRegistry::new());
tool_registry.register_tool("echo", ...).await;
let mut agent = ReActAgent::new("test-session".to_string(), tool_registry, llm);

// 改后：
let mut agent = ReActAgent::new("test-session".to_string(), llm);
agent.register_tool(ToolDescriptor {
    name: "echo".to_string(),
    description: "Echoes input".to_string(),
    json_schema: None,
    handler: Arc::new(FnTool(|args: String| async move {
        format!("Echo: {}", args)
    })),
}).await;
```

---

## 十一、文件改动清单

| 文件 | 改动类型 | 行数估计 |
|:---|:---|:---|
| `src/core/types.rs` | 新增 `ToolSourceDefinition`、`ToolSourceType`；`StepDefinition` 加 2 字段 | +70 |
| `src/core/parser.rs` | 新增 `validate_step_tools` 系列方法 | +70 |
| `src/agent/react_agent.rs` | `ReActAgent::new` 去掉 registry 参数；`AgentManager` 去掉 `tool_registry` 字段；`register_tool` 加 `session_id` | ~50 改动 |
| `src/executors/agent_executor.rs` | 创建 session 后注册 Per-Step 工具 | +40 改动 |
| `src/executors/tool_executor.rs` | 去掉 `tool_registry` 字段，支持内联 tool | ~30 改动 |
| `src/core/dag.rs` | `Scheduler::new` 去掉共享 registry | ~15 改动 |
| **新文件** `src/agent/tool_factory.rs` | `create_tool_handler` 工厂函数 | +50 |
| **新文件** `src/agent/tool_implementations.rs` | ShellTool, HttpTool, PythonTool, Builtin | +300 |
| `src/agent/mod.rs` | pub mod + pub use | +5 |
| `src/lib.rs` | re-export | +2 |

**总计**：~630 行新增/修改。
