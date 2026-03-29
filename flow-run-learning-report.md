# flow-run 学习指南

## 1. 项目简介

**flow-run** 是一个用 Rust 编写的**声明式工作流引擎**，专为 AI Agent 设计。它用 YAML 定义工作流，通过 DAG（有向无环图）调度引擎自动解析步骤依赖、并行执行无依赖步骤，并提供检查点断点续跑、条件分支、循环、模板表达式等能力。

**解决的核心问题：**
- Agent 执行多步骤任务时缺乏编排能力
- 失败后需从头重试，浪费已完成的工作
- 无法并行执行无依赖步骤
- 缺乏条件分支和循环支持

**技术栈：** Rust 2021 Edition / Tokio（异步运行时）/ Clap（CLI）/ Serde（序列化）/ reqwest（HTTP）

---

## 2. 项目结构一览

```
flow-run/
├── Cargo.toml                    # Rust 项目配置与依赖
├── flow-run-design.md            # 详细设计文档（强烈建议先读这个）
│
├── src/
│   ├── main.rs                   # CLI 入口，命令分发
│   ├── lib.rs                    # 库入口，导出 4 个子模块
│   │
│   ├── core/                     # 核心引擎（YAML 解析 → DAG 调度 → 执行）
│   │   ├── types.rs              # 所有类型定义（~767 行，最重要的文件）
│   │   ├── parser.rs             # YAML 解析 + 验证（唯一性、依赖、循环检测）
│   │   ├── dag.rs                # DAG 调度器 + Scheduler 执行引擎（~896 行）
│   │   ├── context.rs            # 执行上下文（输入/输出/变量/状态追踪）
│   │   └── template.rs           # 模板表达式引擎（${{...}} 语法 + 过滤器）
│   │
│   ├── executors/                # 步骤执行器
│   │   ├── mod.rs                # Executor trait 定义
│   │   ├── http.rs               # HTTP 请求执行器
│   │   ├── shell.rs              # Shell 命令执行器
│   │   ├── loop.rs               # 循环执行器
│   │   ├── condition.rs          # 条件分支执行器
│   │   ├── workflow.rs           # 子工作流执行器（含 WorkflowRunner trait）
│   │   └── approve.rs            # 人工审批执行器
│   │
│   ├── cli/                      # 命令行接口
│   │   ├── mod.rs
│   │   └── commands.rs           # Clap 定义所有子命令（run/resume/validate/...）
│   │
│   └── utils/                    # 工具模块
│       ├── mod.rs
│       ├── error.rs              # 统一错误类型（分类编码：A/B/C/D/E/F/G）
│       ├── retry.rs              # 重试引擎（固定/指数/斐波那契退避 + 抖动）
│       └── checkpoint.rs         # 检查点管理（保存/加载/超时上下文）
│
└── examples/                     # YAML 工作流示例（11 个，从基础到综合）
    ├── 01_basic_http.yaml        # HTTP 请求
    ├── 02_basic_shell.yaml       # Shell 命令
    ├── 03_basic_dependencies.yaml # 步骤依赖
    ├── 04_intermediate_parallel.yaml  # 并行执行
    ├── 05_intermediate_retry.yaml     # 重试
    ├── 06_intermediate_templates.yaml # 模板表达式
    ├── 07_advanced_loop.yaml          # 循环
    ├── 08_advanced_condition.yaml     # 条件分支
    ├── 09_advanced_subworkflow.yaml   # 子工作流
    ├── 10_advanced_approval.yaml      # 人工审批
    ├── 11_comprehensive_cicd.yaml     # 综合 CI/CD
    └── code/                           # Rust 代码示例（7 个）
```

---

## 3. 核心概念速览

### 3.1 YAML 工作流定义

一个 flow-run 工作流就是一个 YAML 文件，包含以下顶层字段：

```yaml
name: deploy-application          # 工作流名称
description: 自动化部署工作流     # 描述（可选）
version: "1.0"                    # 版本（可选）

config:                           # 全局配置
  timeout: 300s                   #   总超时
  retry:                          #   全局重试策略
    max_attempts: 3
    strategy: exponential
  on_failure: pause               #   失败策略: abort / pause / continue
  checkpoint: /tmp/deploy.state   #   检查点路径
  max_concurrent: 5               #   最大并发数

inputs:                           # 输入参数定义
  - name: app_name
    type: string
    required: true
  - name: environment
    type: string
    default: staging
    enum: [staging, production]

outputs:                          # 输出定义
  deployment_id: ${{steps.deploy.response.body.id}}

steps:                            # 步骤列表
  - id: fetch_data
    type: http
    api: https://api.example.com/data
    method: GET
```

### 3.2 七种步骤类型

| 类型 | 用途 | 关键字段 |
|:---|:---|:---|
| `http` | HTTP API 调用 | `api`, `method`, `headers`, `body`, `cache` |
| `shell` | 执行 Shell 命令 | `run`, `env`, `safe_mode`, `allowed_commands` |
| `parallel` | 并行执行子步骤 | `steps`, `max_concurrent`, `rate_limit` |
| `loop` | 循环执行 | `loop`（forEach/while/range）, `do_steps` |
| `condition` | 条件分支 | `expression`, `then_steps`, `else_steps` |
| `workflow` | 子工作流 | `workflow`(路径), `inputs`, `error_strategy` |
| `approve` | 人工审批 | `message`, `approvers`, `auto_approve_on` |

### 3.3 依赖与并行

步骤之间通过 `depends_on` 声明依赖。DAG 调度器自动进行拓扑排序，将步骤分批执行——**同一批次内无依赖关系的步骤并行执行**。

```yaml
steps:
  - id: fetch_data          # 批次 1
    type: http
    api: https://api.example.com/data

  - id: prepare_env         # 批次 1（与 fetch_data 并行）
    type: shell
    run: "mkdir -p /tmp/output"

  - id: process_data        # 批次 2（依赖 fetch_data）
    type: shell
    run: "cat /tmp/data | jq"
    depends_on: [fetch_data]

  - id: save_result         # 批次 3（依赖 process_data 和 prepare_env）
    type: shell
    run: "cp /tmp/data /output"
    depends_on: [process_data, prepare_env]
```

### 3.4 模板表达式

使用 `${{...}}` 语法引用变量、步骤输出，并支持过滤器链：

```yaml
# 变量引用
api: ${{ inputs.api_url }}

# 步骤输出引用（嵌套路径 + 数组索引）
item: ${{ steps.fetch.outputs.data.items[0].name }}

# 过滤器链（管道符串联）
message: ${{ steps.check.outputs.result | uppercase | truncate(50) }}

# 默认值
fallback: ${{ steps.optional.value | default("unknown") }}

# 条件表达式
env: ${{ inputs.environment || "staging" }}
```

**内置过滤器：** `uppercase`, `lowercase`, `capitalize`, `trim`, `default(val)`, `to_json`, `from_json`, `length`, `slice(s,e)`, `first`, `last`, `join(sep)`, `split(sep)`, `replace(old,new)`, `regex_extract(pat)`, `truncate(n)`, `base64_encode`, `base64_decode`, `format_timestamp`, `format_duration`

---

## 4. 架构与执行流程

### 4.1 整体流水线

```
YAML 文件 → Parser → Validator → DAG Scheduler → Executor(s) → Result
              │         │           │               │
              │         │           │               ├─ HTTP Executor
              │         │           │               ├─ Shell Executor
              │         │           │               ├─ Parallel Executor
              │         │           │               ├─ Loop Executor
              │         │           │               ├─ Condition Executor
              │         │           │               ├─ Workflow Executor
              │         │           │               └─ Approve Executor
              │         │           │
              │         │           └─ 检查点管理（每批次后保存）
              │         │
              │         └─ 循环依赖检测（DFS）
              └─ serde_yaml 反序列化
```

### 4.2 关键数据流

1. **Parser** (`core/parser.rs`) 将 YAML 解析为 `WorkflowDefinition` 结构体
2. **DagScheduler** (`core/dag.rs`) 从 `WorkflowDefinition` 构建邻接表和入度表
3. **topological_sort** 执行 Kahn 算法，将步骤分成多个批次（`Vec<Vec<StepId>>`）
4. **Scheduler::run** 按批次执行，每个批次内通过 `tokio::spawn` 并行执行，用 `Semaphore` 控制并发
5. 每个步骤的结果写入 `ExecutionContext`，后续步骤可以通过模板引擎引用前序步骤的输出

### 4.3 核心结构体关系

```
WorkflowDefinition（顶层定义）
├── config: WorkflowConfig
├── inputs: Vec<InputDefinition>
├── outputs: HashMap<String, String>
├── steps: Vec<StepDefinition>     ←── 每个步骤可以包含子步骤
│   ├── id, name, type
│   ├── depends_on: Vec<String>
│   ├── steps (parallel 子步骤)
│   ├── then_steps / else_steps (condition)
│   ├── do_steps (loop 循环体)
│   └── workflow (子工作流路径)
├── on: HooksConfig
└── trigger: Vec<TriggerConfig>

ExecutionContext（运行时状态）
├── inputs: HashMap<String, Value>
├── step_outputs: HashMap<StepId, StepResult>  ←── 步骤间数据传递的核心
├── completed_steps: HashSet<StepId>
├── failed_steps: HashSet<StepId>
└── variables: HashMap<String, Value>

StepResult（单步执行结果）
├── step_id, status, started_at, completed_at, duration_ms
├── output: Option<Value>           ←── 被后续步骤通过模板引用
└── error: Option<StepError>
```

---

## 5. 核心模块详解

### 5.1 类型系统 (`core/types.rs`)

这是整个项目最重要的文件（~767 行），定义了所有数据结构。关键类型：

| 类型 | 说明 |
|:---|:---|
| `WorkflowDefinition` | 工作流顶层定义 |
| `StepDefinition` | 步骤定义（一个大 struct，包含所有步骤类型的字段） |
| `StepType` | 步骤类型枚举：Http/Shell/Parallel/Loop/Condition/Workflow/Approve |
| `WorkflowConfig` | 全局配置（超时、重试、并发、检查点、恢复策略等） |
| `StepResult` | 步骤执行结果（提供 `success()`/`failed()`/`skipped()` 构造方法） |
| `WorkflowResult` | 工作流最终结果（状态、指标、步骤结果列表、输出、错误） |
| `LoopConfig` | 循环配置（ForEach/While/Range 三种模式） |
| `OnFailureStrategy` | 失败策略：Abort（中止）/ Pause（暂停保存检查点）/ Continue（继续） |

`StepDefinition` 使用一个扁平的大 struct 来兼容所有步骤类型——每种类型只使用自己关心的字段，其他字段为 `None`。这是一种常见的 Rust 模式，虽然字段多但避免了 enum 嵌套的复杂性。

### 5.2 YAML 解析与验证 (`core/parser.rs`)

`WorkflowParser` 提供 3 个核心方法：

```rust
// 从文件加载
WorkflowParser::from_file("workflow.yaml")?;
// 从字符串解析
WorkflowParser::from_str(yaml_content)?;
// 验证工作流定义
WorkflowParser::validate(&workflow)?;
```

验证包括三步：
1. **步骤 ID 唯一性检查** — 递归检查所有子步骤（parallel 的 steps、condition 的 then/else_steps、loop 的 do_steps）
2. **依赖关系有效性** — 确保每个 `depends_on` 引用的步骤 ID 确实存在
3. **循环依赖检测** — 使用 DFS 在递归栈中检测回边

### 5.3 DAG 调度器 (`core/dag.rs`)

**DagScheduler** 负责构建依赖图和拓扑排序：
- 维护 `adjacency`（邻接表）和 `in_degree`（入度表）
- `topological_sort()` 返回 `Vec<Vec<StepId>>`，即分批的执行计划
- 使用 Kahn 算法（BFS 层序），每层的节点构成一个批次

**Scheduler** 是真正的执行引擎：
- `run()` — 从头执行工作流
- `resume()` — 从检查点恢复执行
- `execute_batch()` — 用 `Semaphore` 控制并发的 `tokio::spawn` 并行执行
- `execute_step()` — 根据步骤类型分发到不同的执行方法

### 5.4 执行上下文 (`core/context.rs`)

`ExecutionContext` 是运行时的"共享状态仓库"：
- 存储所有输入参数、步骤输出、变量
- 提供 `evaluate()` 方法求值 `${{...}}` 表达式
- 提供 `resolve_path()` 方法解析点号分隔的路径（如 `steps.deploy.response.body.data[0]`）
- 追踪步骤完成/失败状态

### 5.5 模板表达式引擎 (`core/template.rs`)

`TemplateEngine` 处理 `${{...}}` 模板表达式，支持：
- **路径访问**：`inputs.api_url`、`steps.fetch.response.body.data[0].name`
- **过滤器链**：`value | uppercase | truncate(10)`
- **条件表达式**：`inputs.env || "staging"`、`inputs.count == 10`
- **18 种内置过滤器**

实现要点：
- 用 `Regex` 匹配 `${{...}}` 模式
- `find_operator()` 方法智能识别 `||` 和 `==` 操作符，忽略引号内和括号内的同名字符
- `navigate_path()` 返回 `Value::Null` 而非报错，配合 `default` 和 `||` 使用

### 5.6 步骤执行器 (`executors/`)

每个执行器实现 `Executor` trait（除 `WorkflowExecutor` 使用 `WorkflowRunner` trait）：

| 执行器 | 文件 | 职责 |
|:---|:---|:---|
| HTTP Executor | `executors/http.rs` | 构建请求、发送、解析响应、期望验证 |
| Shell Executor | `executors/shell.rs` | 执行 `sh -c`、环境变量注入、安全模式检查 |
| Loop Executor | `executors/loop.rs` | ForEach/While/Range 三种循环模式 |
| Condition Executor | `executors/condition.rs` | 求值表达式，执行 then/else 分支 |
| Workflow Executor | `executors/workflow.rs` | 加载子工作流、准备输入、上下文隔离 |
| Approve Executor | `executors/approve.rs` | 人工审批（发送通知、轮询结果、超时处理） |

### 5.7 重试引擎 (`utils/retry.rs`)

`RetryEngine` 提供带退避策略的自动重试：
- 三种退避策略：Fixed（固定）、Exponential（指数，`delay = initial * factor^attempt`）、Fibonacci（斐波那契）
- 抖动（jitter）避免惊群效应：在计算出的延迟基础上乘以 0.8~1.2 的随机因子
- 可配置可重试的 HTTP 状态码（默认 408/429/500/502/503/504）和错误类型
- 最大延迟上限（默认 30s）

### 5.8 检查点系统 (`utils/checkpoint.rs`)

检查点实现断点续跑：
- `Checkpoint` 结构体保存完整的执行状态（已完成步骤、失败步骤、步骤输出、变量、当前批次、超时上下文）
- `CheckpointManager` 提供保存（JSON 文件）、加载、列出、删除操作
- `TimeoutContext` 追踪工作流级和步骤级的超时消耗，恢复时可以继承剩余超时

### 5.9 错误体系 (`utils/error.rs`)

错误类型使用字母编码分类，便于 Agent 解析：

| 前缀 | 类别 | 示例 |
|:---|:---|:---|
| A | 工作流错误 | A001 文件不存在、A004 循环依赖 |
| B | 执行错误 | B001 HTTP 失败、B003 超时 |
| C | 检查点错误 | C001 检查点不存在 |
| D | 模板错误 | D002 变量未定义、D005 过滤器不存在 |
| E | 审批错误 | E001 审批被拒绝 |
| F | 钩子错误 | F001 钩子超时 |
| G | 触发器错误 | G001 Webhook 签名无效 |

---

## 6. CLI 使用方法

```
flow-run <WORKFLOW_FILE> [OPTIONS] <SUBCOMMAND>

子命令：
  run         执行工作流
  resume      从检查点恢复
  validate    验证工作流定义
  dry-run     模拟执行
  checkpoint  检查点管理（list/show/clean）
  history     查看执行历史
  schema      输出 JSON Schema
```

```bash
# 执行工作流
flow-run workflow.yaml run --input key=value --json

# 验证工作流（显示 DAG 结构）
flow-run workflow.yaml validate --show-dag

# 试运行
flow-run workflow.yaml dry-run

# 从检查点恢复
flow-run workflow.yaml resume --checkpoint_id cp_xxx

# 列出检查点
flow-run workflow.yaml checkpoint list --verbose
```

---

## 7. 学习路径建议

### 第一阶段：理解设计（读 design.md）

先读 `flow-run-design.md`，里面有完整的架构图、数据结构设计、伪代码示例。这是最快建立全局认知的方式。

### 第二阶段：跑通示例

按顺序运行 `examples/` 下的 YAML 示例：

```bash
# 基础
cargo run -- examples/01_basic_http.yaml validate
cargo run -- examples/02_basic_shell.yaml validate
cargo run -- examples/03_basic_dependencies.yaml validate

# 中级
cargo run -- examples/04_intermediate_parallel.yaml validate
cargo run -- examples/05_intermediate_retry.yaml validate
cargo run -- examples/06_intermediate_templates.yaml validate

# 高级
cargo run -- examples/07_advanced_loop.yaml validate
cargo run -- examples/08_advanced_condition.yaml validate
cargo run -- examples/09_advanced_subworkflow.yaml validate
cargo run -- examples/10_advanced_approval.yaml validate

# 综合
cargo run -- examples/11_comprehensive_cicd.yaml validate
```

### 第三阶段：读核心源码（按依赖顺序）

1. **`src/core/types.rs`** — 所有类型定义，理解数据模型
2. **`src/utils/error.rs`** — 错误体系，理解错误分类
3. **`src/core/parser.rs`** — YAML 解析和验证逻辑
4. **`src/core/context.rs`** — 执行上下文和表达式求值
5. **`src/core/template.rs`** — 模板引擎和过滤器系统
6. **`src/core/dag.rs`** — DAG 调度器 + Scheduler 执行引擎
7. **`src/utils/retry.rs`** — 重试引擎
8. **`src/utils/checkpoint.rs`** — 检查点系统
9. **`src/executors/*.rs`** — 各类步骤执行器
10. **`src/cli/commands.rs`** — CLI 命令定义
11. **`src/main.rs`** — 入口函数，串联以上所有模块

### 第四阶段：看 Rust 代码示例

`examples/code/` 下有 7 个 Rust 代码示例，展示如何作为库使用 flow-run：
- `01_load_workflow` — 加载工作流
- `02_execution_context` — 执行上下文
- `03_dag_scheduler` — DAG 调度
- `04_template_engine` — 模板引擎
- `05_retry_engine` — 重试引擎
- `06_checkpoint` — 检查点
- `07_full_execution` — 完整执行

### 第五阶段：运行测试

```bash
cargo test          # 运行所有测试
cargo test -- --nocapture  # 显示测试输出
```

---

## 8. 关键设计决策解读

### 8.1 为什么用 Rust？

- AI Agent 需要非交互式、结构化输出的工具
- Rust 提供内存安全、零成本异步（tokio）、单二进制部署
- JSON 原生输出，无需 `--json` 标志

### 8.2 为什么用扁平 struct 而非 enum 嵌套？

`StepDefinition` 是一个大 struct，包含所有步骤类型的字段。这简化了 YAML 反序列化——Serde 可以直接将 YAML 映射到 struct，不需要复杂的 tag 解析。缺点是字段多（约 30 个），但通过 `Option<T>` 保证类型安全。

### 8.3 检查点如何实现断点续跑？

每次执行完一个批次后，Scheduler 将当前状态序列化为 JSON 保存。恢复时，加载检查点、跳过已完成的批次、从下一个批次继续执行。`TimeoutContext` 保存已消耗时间，恢复时继承剩余超时而非重新计时。

### 8.4 模板引擎为何返回 Null 而非报错？

当路径不存在时（如 `inputs.missing_field`），模板引擎返回 `Value::Null` 而非报错。这使得 `default` 过滤器和 `||` 操作符可以优雅地处理缺失值——这是 Agent 友好的设计，避免因非关键字段缺失而中断整个工作流。

---

## 9. 常见 YAML 模式速查

### HTTP 请求 + 结果引用
```yaml
- id: fetch
  type: http
  api: ${{ inputs.api_url }}/users
  method: GET
- id: process
  type: shell
  run: echo ${{ steps.fetch.response.body.name }}
  depends_on: [fetch]
```

### 并行执行
```yaml
- id: parallel_tasks
  type: parallel
  max_concurrent: 10
  rate_limit:
    requests_per_second: 5
    burst: 10
  steps:
    - id: task_1
      type: http
      api: https://api.example.com/1
    - id: task_2
      type: http
      api: https://api.example.com/2
```

### 条件分支
```yaml
- id: deploy
  type: condition
  expression: inputs.environment == 'production'
  then_steps:
    - id: prod_deploy
      type: shell
      run: ./deploy-prod.sh
  else_steps:
    - id: dev_deploy
      type: shell
      run: ./deploy-dev.sh
```

### 循环
```yaml
- id: process_items
  type: loop
  loop:
    for_each:
      over: ${{ steps.fetch.outputs.data.items }}
      as: item
  do_steps:
    - id: process
      type: shell
      run: echo "Processing ${{ variables.item.name }}"
```

### 子工作流
```yaml
- id: run_tests
  type: workflow
  workflow: ./test-suite.yaml
  inputs:
    test_env: ${{ inputs.environment }}
  error_strategy: continue
  timeout: 120s
```

### 人工审批
```yaml
- id: approve_deploy
  type: approve
  message: "确认部署 ${{ inputs.version}} 到生产环境？"
  approvers: [team-leads@company.com]
  timeout: 3600s
  auto_approve_on:
    - condition: "${{ inputs.environment == 'staging' }}"
      reason: "staging 自动通过"



## 代码详细解释

这段代码是 `DagScheduler::new` 方法中的一部分，用于**初始化 DAG（有向无环图）的数据结构**：

```rust
for step_id in &step_ids {
    adjacency.insert(step_id.clone(), Vec::new());  // 邻接表
    in_degree.insert(step_id.clone(), 0);           // 入度表
}
```
# dag 详细分析
## 两个核心数据结构

### 1. `adjacency: HashMap<StepId, Vec<StepId>>` - 邻接表

```rust
adjacency.insert(step_id.clone(), Vec::new());
```

- **含义**：记录"从当前步骤可以到达哪些后续步骤"
- **初始化**：每个步骤都初始化一个空的 `Vec`
- **示例**：如果 `step1 -> step2`，则 `adjacency["step1"] = ["step2"]`

### 2. `in_degree: HashMap<StepId, usize>` - 入度表

```rust
in_degree.insert(step_id.clone(), 0);
```

- **含义**：记录"有多少个前置步骤依赖当前步骤"
- **初始化**：每个步骤入度初始化为 `0`
- **示例**：如果 `step1 -> step2`，则 `in_degree["step2"] = 1`

## 完整流程图解

假设有以下依赖关系：
```
step1 → step2 → step4
    ↘ step3 ↗
```

### 初始化后：

| step_id | adjacency | in_degree |
|---------|-----------|-----------|
| step1 | `[]` | 0 |
| step2 | `[]` | 0 |
| step3 | `[]` | 0 |
| step4 | `[]` | 0 |

### 处理 `depends_on` 后：

```rust
for step in &steps {
    if let Some(deps) = &step.depends_on {
        for dep in deps {
            // dep -> step.id 的边
            adjacency.entry(dep.clone()).or_default().push(step.id.clone());
            *in_degree.entry(step.id.clone()).or_insert(0) += 1;
        }
    }
}
```

| step_id | adjacency | in_degree |
|---------|-----------|-----------|
| step1 | `[step2, step3]` | 0 |
| step2 | `[step4]` | 1 |
| step3 | `[step4]` | 1 |
| step4 | `[]` | 2 |



## 为什么需要这两个数据结构？

这是**拓扑排序（Kahn 算法）**的核心：

1. **in_degree = 0** 的步骤没有前置依赖，可以立即执行
2. 执行完一个步骤后，将其从 `adjacency` 中的邻居的 `in_degree` 减 1
3. 当某个步骤的 `in_degree` 变为 0 时，加入执行队列

```rust
// topological_sort 中的使用
for (step_id, &deg) in &in_degree {
    if deg == 0 {
        queue.push_back(step_id.clone());  // 可以立即执行
    }
}
```

## 总结

这段代码的目的是：
1. **收集所有步骤 ID**
2. **初始化邻接表和入度表**（每个步骤都是孤立的，入度为 0）
3. 后续代码会根据 `depends_on` 填充这两个表，构建完整的依赖图

这个解释 添加到注释
```

是的，这里是**手动实现的拓扑排序**，使用的是经典的 **Kahn 算法**（BFS 版本）。

## 算法流程

```rust
pub fn topological_sort(&self) -> Result<Vec<Vec<StepId>>, WorkflowError> {
    // 1. 先检测循环依赖
    if self.has_cycle()? {
        return Err(WorkflowError::CycleDetected);
    }

    // 2. 初始化：复制入度表，找到所有入度为 0 的步骤
    let mut in_degree = self.in_degree.clone();
    let mut queue: VecDeque<StepId> = VecDeque::new();
    let mut batches: Vec<Vec<StepId>> = Vec::new();

    for (step_id, &deg) in &in_degree {
        if deg == 0 {
            queue.push_back(step_id.clone());
        }
    }

    // 3. 执行拓扑排序（Kahn 算法的核心循环）
    while !queue.is_empty() {
        // 取出当前批次（所有入度为 0 的步骤可以并行执行）
        let current_batch: Vec<StepId> = queue.drain(..).collect();

        // 将当前批次的后续步骤入度减 1
        for step_id in &current_batch {
            if let Some(neighbors) = self.adjacency.get(step_id) {
                for neighbor in neighbors {
                    if let Some(deg) = in_degree.get_mut(neighbor) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(neighbor.clone());
                        }
                    }
                }
            }
        }

        batches.push(current_batch);
    }

    Ok(batches)
}
```

## 关键特性

这个实现返回的是**分批次的拓扑排序** `Vec<Vec<StepId>>`，而不仅仅是一个扁平的序列：

| 批次 | 包含步骤 | 说明 |
|------|----------|------|
| `batches[0]` | 入度为 0 的步骤 | 可以立即并行执行 |
| `batches[1]` | 依赖第一批次完成的步骤 | |
| ... | ... | |

## 示例

```
step1 → step2 → step4
    ↘ step3 ↗
```

拓扑排序结果：`[["step1"], ["step2", "step3"], ["step4"]]`

- 第一批：`step1`（无依赖，立即执行）
- 第二批：`step2`, `step3`（并行执行，都依赖 `step1`）
- 第三批：`step4`（依赖 `step2` 和 `step3`）

## 为什么手动实现？

1. **分批执行**：需要支持并行执行同一层级的步骤
2. **循环检测**：在排序前先检测循环依赖
3. **与工作流引擎集成**：返回结果直接用于 `Scheduler::run()` 的批次执行

---

## 10. 步骤间结果传递机制详解

这是 flow-run 最核心的运行时机制——前一个步骤的执行结果如何被后一个步骤引用。

### 10.1 整体数据流

```
步骤 A 执行完毕
       │
       ▼
StepResult { step_id: "A", output: Some({...}), status, ... }
       │
       ▼  Scheduler.run() 中，每个批次执行后写入
       │
ExecutionContext.step_outputs["A"] = StepResult(...)
       │
       ▼  步骤 B 执行前，构建模板上下文
       │
template_ctx = {
    "inputs":   { "api_url": "https://..." },
    "steps":    { "A": <StepResult.output 的值> },  ← 关键：只取 output 字段
    "variables": { ... }
}
       │
       ▼  TemplateEngine 解析 B 的 run/api 字段中的 ${{ steps.A.xxx }}
       │
${{ steps.A.response.body.title }}  →  沿 JSON 路径逐层取值
       │
步骤 B 拿到步骤 A 的输出数据
```

### 10.2 StepResult.output 的 JSON 结构

不同步骤类型的 `output` 字段有不同结构：

**Shell 步骤** — stdout 原始输出：
```json
{ "stdout": "目录已创建\n配置文件已写入\n" }
```

**HTTP 步骤** — 包装为 response 结构：
```json
{
  "response": {
    "status_code": 200,
    "body": { "id": 1, "title": "...", "userId": 1 }
  }
}
```

**Parallel 步骤** — 子步骤 output 的数组：
```json
{
  "results": [
    { "response": { "status_code": 200, "body": {...} } },
    { "response": { "status_code": 200, "body": {...} } },
    { "response": { "status_code": 200, "body": {...} } }
  ]
}
```

**Workflow（子工作流）步骤** — 包含状态和输出：
```json
{
  "workflow": "examples/sub.yaml",
  "status": "Success",
  "outputs": { "artifact_path": "/tmp/build/app.tar.gz" },
  "metrics": { "total_steps": 3, "success_steps": 3, ... }
}
```

### 10.3 写入：结果何时存入上下文

在 `Scheduler::run()` 中（`src/core/dag.rs:215-218`），每个批次的所有步骤执行完毕后，立即写入共享上下文：

```rust
for result in &batch_results {
    let mut ctx = self.context.write().await;
    ctx.step_outputs.insert(result.step_id.clone(), result.clone());
}
```

`step_outputs` 是 `HashMap<StepId, StepResult>`，key 是步骤 ID，value 是完整的 `StepResult`（包含 status、output、error、duration_ms 等）。

**写入时机很重要**：同一批次内的并行步骤，后完成的会覆盖先完成的（通常不会冲突因为 ID 不同）。但不同批次之间是严格有序的——批次 N 的结果在批次 N+1 执行前已全部写入。

### 10.4 读取：模板上下文如何构建

当步骤 B 准备执行时（`execute_shell_step` 或 `execute_http_step`），会从 `ExecutionContext` 构建模板上下文（`src/core/dag.rs:490-503`）：

```rust
// 构建 steps 上下文：只包含 output 字段，便于模板直接访问
let mut steps_ctx = serde_json::Map::new();
for (step_id, result) in &ctx.step_outputs {
    if let Some(output) = &result.output {
        steps_ctx.insert(step_id.clone(), output.clone());
    }
}
template_ctx.insert("steps".to_string(), serde_json::Value::Object(steps_ctx));
```

关键设计：
- 只取 `result.output`，不包含 `status`、`error`、`duration_ms` 等元数据
- 如果步骤失败且 `output` 为 `None`，该步骤 ID 不会出现在模板上下文中
- 模板上下文还包含 `inputs`（输入参数）和 `variables`（工作流变量 + 循环变量）

### 10.5 解析：`${{ steps.A.x.y }}` 如何解析

当步骤 B 的 `run` 字段包含 `${{ steps.fetch_data.response.body.title }}` 时：

1. **正则提取**：`TemplateEngine` 用 `\$\{\{([^}]+)\}\}` 提取内部表达式 `steps.fetch_data.response.body.title`

2. **操作符检测**（优先级从高到低）：
   - `||` 默认值操作符：`inputs.env || "staging"` → 如果左侧为 Null/空串，返回右侧
   - `==` 相等比较：`inputs.risk_level == 'high'` → 返回 `true`/`false`
   - `|` 过滤器链：`value | uppercase | truncate(10)` → 依次应用过滤器

3. **路径解析**：按 `.` 分割为 `["steps", "fetch_data", "response", "body", "title"]`
   - `resolve_path()` 先从 context 取根键 `steps` → 得到 steps_ctx 对象
   - `navigate_path()` 逐层导航：
     - `steps_ctx["fetch_data"]` → HTTP 步骤的 output：`{"response": {"status_code": 200, "body": {...}}}`
     - `["response"]` → `{"status_code": 200, "body": {...}}`
     - `["body"]` → `{"id": 1, "title": "..."}`
     - `["title"]` → `"..."`

4. **数组索引**：`variables.items[0].name`
   - `items[0]` 先按 `[` 分割，取字段 `items`（空串时直接取数组），再按数字索引取元素

5. **缺失路径返回 Null**：路径不存在时返回 `Value::Null`，而非报错。这使得 `||` 和 `default()` 过滤器可以优雅处理缺失值。

### 10.6 两套路径解析系统的差异

| 特性 | `TemplateEngine.resolve_path()` | `ExecutionContext.resolve_path()` |
|:---|:---|:---|
| 所在文件 | `src/core/template.rs` | `src/core/context.rs` |
| 路径不存在时 | 返回 `Value::Null`（友好） | 返回 `WorkflowError::PathNotFound`（报错） |
| 使用场景 | 步骤执行器解析 `run`/`api` 模板 | `evaluate()` 方法、output 解析 |
| 数组越界 | 返回 `Value::Null` | 返回 `PathNotFound` |
| 支持过滤器 | 是 | 否 |

### 10.7 特殊场景

**Parallel 步骤的子步骤间传递**：在 `execute_parallel_step()` 中（`src/core/dag.rs:550-553`），每个子步骤执行后**立即**写入上下文：

```rust
{
    let mut ctx = context.write().await;
    ctx.step_outputs.insert(sub_step.id.clone(), result.clone());
}
```

这意味着并行组内的后续子步骤可以引用同组内先完成的子步骤输出。

**循环步骤的变量传递**：循环变量通过 `context.variables["loop"]` 传递，在构建模板上下文时会被提取到顶层：

```rust
if let Some(loop_vars) = ctx.variables.get("loop") {
    template_ctx.insert("loop".to_string(), loop_vars.clone());
}
```

所以循环体内可以用 `${{ variables.loop.current }}` 或 `${{ loop.current }}` 访问当前迭代变量。

**子工作流的上下文隔离**：子工作流创建独立的 `ExecutionContext`，默认透传父工作流的 inputs（可通过 `passthrough_vars` 指定变量、`isolation: true` 完全隔离）。子工作流的输出以包装结构返回给父工作流。

### 10.8 完整示例：HTTP → Shell 结果传递

```yaml
steps:
  - id: fetch
    type: http
    api: https://api.example.com/users/1
    method: GET
    # output = {"response": {"status_code": 200, "body": {"name": "Alice", "email": "a@b.com"}}}

  - id: display
    type: shell
    depends_on: [fetch]
    # 模板解析链:
    #   steps.fetch → 取 fetch 步骤的 output
    #   .response → {"status_code": 200, "body": {...}}
    #   .body → {"name": "Alice", "email": "a@b.com"}
    #   .name → "Alice"
    run: echo "用户名: ${{ steps.fetch.response.body.name }}"
```
