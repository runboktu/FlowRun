# Builtin Tool 可扩展方案

> 日期：2026-04-10
> 状态：待实施
> 范围：`src/agent/`、`src/core/runner.rs`、`src/core/dag.rs`、`src/executors/`

---

## 一、现状分析

```
用户代码
  └─ FlowRunner::from_file("workflow.yaml")
       └─ FlowRunner::run(inputs)
            └─ Scheduler::new(dag, config, checkpoint)
                 └─ AgentExecutor::new(agent_manager)
                      └─ execute(step)
                           └─ create_tool_handler(tool_def)
                                └─ create_builtin_tool(name)  ← 硬编码 match
```

**问题**：`create_builtin_tool` 是一个 match 语句，`source: builtin` 的工具只有 4 个（read_file / write_file / list_directory / http_get）。库用户无法扩展，除非 fork 代码修改 match 分支。

---

## 二、设计目标

| # | 目标 | 约束 |
|:--|:-----|:-----|
| 1 | 库用户能注册自定义 builtin 工具 | 在 YAML 中用 `source: builtin` + `name: xxx` 即可使用 |
| 2 | 默认工具仍开箱即用 | 无需任何额外配置 |
| 3 | 注册是 per-FlowRunner 实例的 | 不同 runner 可注册不同工具集 |
| 4 | 支持工具同名覆盖 | 用户注册的同名工具优先于默认工具 |
| 5 | API 风格与现有 Builder 模式一致 | `FlowRunner::new().register_builtin_tool(...)` |
| 6 | 向后兼容 | 无 agent_tools 的旧 YAML 不受影响 |

---

## 三、核心概念

### 3.1 新增：`BuiltinToolRegistry`

```
┌────────────────────────────────────────────┐
│ BuiltinToolRegistry                         │
│                                             │
│  default_tools: HashMap<String, ToolDesc>   │  ← 框架内置的 4 个工具
│  custom_tools:   HashMap<String, ToolDesc>  │  ← 用户注册的工具
│                                             │
│  lookup(name) → 查 custom_tools 优先，      │
│                  再查 default_tools          │
└────────────────────────────────────────────┘
```

**两层查找**：用户工具覆盖默认工具（同名优先取 custom）。不需要报错冲突——覆盖是 feature，不是 bug。

### 3.2 数据流

```
用户代码
  FlowRunner::from_file("workflow.yaml")
    .register_builtin("check_char_count", desc, schema, handler)  ← 新增
    .register_builtin("strip_whitespace", desc, schema, handler)  ← 新增
  └─ FlowRunner.run(inputs)
       │
       │  builtin_registry: Arc<BuiltinToolRegistry>
       ▼
  Scheduler::new(dag, config, checkpoint, builtin_registry)   ← 新增参数
       │
       ├─ AgentExecutor::new(agent_manager, builtin_registry) ← 新增参数
       │     └─ execute(step)
       │          └─ create_tool_handler(tool_def, builtin_registry) ← 新增参数
       │               └─ source == Builtin
       │                    → builtin_registry.lookup(&name)?  ← 替代 match
       │
       └─ ToolExecutor::new(builtin_registry)                  ← 新增参数
             └─ execute(step)
                  └─ create_tool_handler(tool_def, builtin_registry) ← 同上
```

---

## 四、用户 API

### 4.1 Rust 库用户

```rust
use flow_run::{FlowRunner, BuiltinToolRegistry, ToolHandler, ToolResult};
use std::sync::Arc;

// 方式 A：FlowRunner Builder（推荐，最简单）
let runner = FlowRunner::from_file("workflow.yaml")?
    .register_builtin(
        "check_char_count",
        "检查文本字符数是否在限制内",
        Some(r#"{"type":"object","properties":{"text":{"type":"string"},"max_chars":{"type":"integer"}}}"#),
        Arc::new(MyCheckCharCountTool),
    )
    .register_builtin(
        "strip_whitespace",
        "压缩文本中的多余空白字符",
        Some(r#"{"type":"object","properties":{"text":{"type":"string"}}}"#),
        Arc::new(MyStripWhitespaceTool),
    );

runner.run(inputs).await?;
```

```rust
// 方式 B：先构造 Registry，再传入（适合注册大量工具或复用 registry）
let registry = BuiltinToolRegistry::new()           // 空 registry
    .with_defaults()                                // 含 4 个默认工具
    .register("check_char_count", desc, schema, handler)
    .register("strip_whitespace", desc, schema, handler);

let runner = FlowRunner::from_file("workflow.yaml")?
    .with_builtin_registry(registry);
```

### 4.2 YAML 侧（无任何变化）

```yaml
agent_tools:
  - name: check_char_count        # 自定义工具名
    source: builtin               # 同样写 builtin
  - name: read_file               # 默认工具名
    source: builtin               # 同样可用
```

**YAML 完全不变**——`source: builtin` + `name: xxx` 对使用者来说是同一件事，无需区分"默认"还是"自定义"。

---

## 五、改动清单

| 文件 | 改动 | 说明 |
|:-----|:-----|:-----|
| **新文件** `src/agent/builtin_registry.rs` | `BuiltinToolRegistry` struct | 两层 HashMap + lookup + register + with_defaults() |
| `src/agent/mod.rs` | +pub mod + pub use | 导出新模块 |
| `src/agent/tool_factory.rs` | `create_tool_handler` 增加参数 | `+ registry: &BuiltinToolRegistry`，builtin 分支从 registry 查找 |
| `src/agent/tool_implementations.rs` | 删除 `create_builtin_tool` 函数 | 职责转移到 `BuiltinToolRegistry::with_defaults()` |
| `src/core/runner.rs` | FlowRunner 增加字段和方法 | `builtin_registry: Arc<BuiltinToolRegistry>` + `register_builtin()` + `with_builtin_registry()` |
| `src/core/dag.rs` | Scheduler::new 增加参数 | `+ builtin_registry: Arc<BuiltinToolRegistry>`，传给 AgentExecutor 和 ToolExecutor |
| `src/executors/agent_executor.rs` | AgentExecutor 增加字段 | `builtin_registry: Arc<BuiltinToolRegistry>`，传给 `create_tool_handler` |
| `src/executors/tool_executor.rs` | ToolExecutor 增加字段 | 同上 |
| `src/lib.rs` | +pub use | 导出 `BuiltinToolRegistry` |

---

## 六、`BuiltinToolRegistry` 详细设计

```rust
pub struct BuiltinToolRegistry {
    /// 框架自带的默认工具（read_file 等）
    default_tools: HashMap<String, StoredTool>,
    /// 用户注册的自定义工具
    custom_tools: HashMap<String, StoredTool>,
}

/// 存储结构（与 ToolDescriptor 类似，但去掉 handler 的 Arc 嵌套）
struct StoredTool {
    name: String,
    description: String,
    json_schema: Option<String>,
    handler: Arc<dyn ToolHandler>,
}
```

**关键方法**：

| 方法 | 说明 |
|:-----|:-----|
| `new()` | 空 registry，无默认工具 |
| `with_defaults()` | 包含 4 个默认工具的 registry（read_file, write_file, list_directory, http_get） |
| `register(name, desc, schema, handler)` | 注册到 custom_tools，builder 模式返回 `&mut Self` |
| `lookup(name) → Option<Arc<dyn ToolHandler>>` | 先查 custom_tools，再查 default_tools |
| `list_all() → Vec<(String, String)>` | 返回所有工具名+描述（用于调试/日志） |
| `default_tool_names() → &[&str]` | 返回默认工具名列表 |

**创建时机**：
- `FlowRunner::new()` 内部自动调用 `BuiltinToolRegistry::with_defaults()` — 零配置即含默认工具
- 用户通过 `register_builtin()` 往 custom_tools 里追加

---

## 七、`FlowRunner` 改动

```rust
pub struct FlowRunner {
    workflow: WorkflowDefinition,
    checkpoint_dir: PathBuf,
    builtin_registry: Arc<BuiltinToolRegistry>,    // ← 新增
}

impl FlowRunner {
    pub fn new(workflow) -> Self {
        Self {
            ...
            builtin_registry: Arc::new(BuiltinToolRegistry::with_defaults()),
        }
    }

    /// 新增：builder 方法，注册单个自定义 builtin 工具
    pub fn register_builtin(
        mut self,
        name: &str,
        description: &str,
        json_schema: Option<&str>,
        handler: Arc<dyn ToolHandler>,
    ) -> Self {
        // Arc::get_mut 或 Arc::make_mut 实现
    }

    /// 新增：builder 方法，传入预构造的 registry
    pub fn with_builtin_registry(mut self, registry: BuiltinToolRegistry) -> Self {
        self.builtin_registry = Arc::new(registry);
        self
    }

    pub async fn run(&self, inputs) {
        // ... 现有逻辑 ...
        let scheduler = Scheduler::new(
            dag, config, checkpoint_manager,
            self.builtin_registry.clone(),    // ← 新增参数
        );
        // ...
    }
}
```

**内部可变性**：`register_builtin` 用 builder 模式（consume self → Self）解决。在 `.run()` 调用前都是独占所有权，`Arc::get_mut` 可用。或者用 `Arc::make_mut`。

---

## 八、`create_tool_handler` 改动

```rust
// 改前：
pub fn create_tool_handler(def: &ToolSourceDefinition) -> Result<Arc<dyn ToolHandler>, WorkflowError>

// 改后：
pub fn create_tool_handler(
    def: &ToolSourceDefinition,
    registry: &BuiltinToolRegistry,    // ← 新增参数
) -> Result<Arc<dyn ToolHandler>, WorkflowError> {
    match &def.source {
        ToolSourceType::Builtin => {
            // 改前：create_builtin_tool(&def.name)
            // 改后：从 registry 查找
            registry.lookup(&def.name)
                .ok_or_else(|| WorkflowError::Other(format!(
                    "Unknown builtin tool: '{}'. Available: {}",
                    def.name,
                    registry.list_all().iter()
                        .map(|(n, _)| n.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )))
        }
        ToolSourceType::Shell => { ... }  // 不变
        ToolSourceType::Http  => { ... }  // 不变
        ToolSourceType::Python => { ... } // 不变
    }
}
```

---

## 九、`Scheduler` / `AgentExecutor` / `ToolExecutor` 传导

```
Scheduler::new(dag, config, checkpoint, builtin_registry)
    │
    ├─ self.agent_executor = Arc::new(AgentExecutor::new(agent_manager, builtin_registry))
    └─ self.tool_executor  = Arc::new(ToolExecutor::new(builtin_registry))
```

三个 Executor 各新增一个 `builtin_registry: Arc<BuiltinToolRegistry>` 字段，在 `execute()` 中传给 `create_tool_handler`。

---

## 十、默认工具迁移

**从 `create_builtin_tool` 迁移到 `BuiltinToolRegistry::with_defaults()`**：

```rust
impl BuiltinToolRegistry {
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register("read_file", "Read file contents", schema, handler);
        registry.register("write_file", "Write content to file", schema, handler);
        registry.register("list_directory", "List directory contents", schema, handler);
        registry.register("http_get", "HTTP GET request", schema, handler);
        registry
    }
}
```

`tool_implementations.rs` 中删除 `create_builtin_tool` 函数。默认工具的 handler 实现保留在 `tool_implementations.rs` 中，由 `with_defaults()` 引用。

---

## 十一、对现有测试的影响

| 改动点 | 说明 |
|:-------|:-----|
| `FlowRunner` 测试 | `FlowRunner::new()` 现在含 `builtin_registry`，无需改测试（with_defaults 是透明的） |
| `AgentExecutor` 测试 | 构造时需传入 `Arc<BuiltinToolRegistry::with_defaults()>` |
| `ToolExecutor` 测试 | 同上 |
| `create_tool_handler` 测试 | 调用时需传入 `&BuiltinToolRegistry` |
| `Scheduler` 测试 | `Scheduler::new()` 增加参数 |

---

## 十二、不涉及改动的部分

- **YAML 语法**：不变，`source: builtin` + `name: xxx` 完全兼容
- **`ToolSourceDefinition` / `StepDefinition`**：类型定义不变
- **Parser / Validation**：不变（`validate_step_tools` 不关心工具名是否存在，只验证字段完整性）
- **`ToolRegistry` (per-agent)**：不变，仍然是 ReActAgent 内部独立的注册表
- **`ReActAgent` / `AgentManager`**：不变，工具注册到 per-agent registry 的流程不变

---

## 十三、总结

| 维度 | 设计决策 |
|:-----|:---------|
| 注册粒度 | Per-FlowRunner 实例（非全局） |
| 用户 API | Builder 模式 `register_builtin()` |
| YAML 侧 | 零变化，`source: builtin` 一视同仁 |
| 查找优先级 | custom_tools > default_tools |
| 默认工具 | `with_defaults()` 自动注册，用户零配置 |
| 传导方式 | `Arc<BuiltinToolRegistry>` 从 FlowRunner → Scheduler → Executor → create_tool_handler |
| 内部可变性 | Builder 模式 consume self，`Arc::make_mut` |
