# `--from-step` 从指定步骤继续执行

> 日期：2026-04-19
> 状态：已实施 → 迭代优化
> 范围：`src/utils/`、`src/cli/commands.rs`、`src/core/runner.rs`、`src/core/dag.rs`、`src/main.rs`

---

## 一、现状分析

### 问题场景

用户执行一个包含 10 个步骤的 YAML 工作流，第 7 步失败。当前行为：

1. 工作流中止，终端输出简单失败信息
2. 用户只能从头重新执行全部 10 步（已成功的步骤白跑一遍）
3. 或者配置 `on_failure: pause` + checkpoint 系统——但这需要预先配置，且依赖 `/tmp/flow-run-checkpoints` 目录

### 现有 resume 机制的局限

```
flow-run workflow.yaml resume --checkpoint-id cp_abc123
```

- **需要预先配置**：YAML 中必须声明 `config.checkpoint` 路径
- **依赖 checkpoint 目录**：checkpoint 文件必须存在于 `/tmp/flow-run-checkpoints`
- **恢复粒度粗**：按 batch（拓扑排序的批次）恢复，不是按 step
- **流程复杂**：需要先记住 checkpoint_id，再手动拼接 resume 命令

### 目标体验

```
$ flow-run workflow.yaml run

❌ 步骤执行失败: build_backend (第 5/10 步)

失败详情:
  步骤 ID:    build_backend
  错误代码:   EXIT_1
  错误信息:   error: failed to compile backend
  修复建议:   检查命令语法和执行环境

已完成步骤的输出已自动保存。

继续执行建议:
  修复问题后，从失败步骤重试:
    flow-run workflow.yaml run --from-step build_backend

  或从任意步骤开始:
    flow-run workflow.yaml run --from-step deploy

$ # 修复问题后...
$ flow-run workflow.yaml run --from-step build_backend

⏩ 从步骤 build_backend 继续执行 (已恢复 4 个前置步骤的输出)
  [⏩] checkout        (已跳过，使用上次输出)
  [⏩] install_deps    (已跳过，使用上次输出)
  [⏩] lint            (已跳过，使用上次输出)
  [⏩] build_frontend  (已跳过，使用上次输出)
  [OK] build_backend
  [OK] test
  ...
```

---

## 二、设计目标

| # | 目标 | 约束 |
|:--|:-----|:-----|
| 1 | **每个步骤执行后自动保存上下文** | 零配置，进程崩溃也不丢数据 |
| 2 | `--from-step <ID>` 从任意步骤继续 | 自动加载保存的上下文，前置步骤输出可正常引用 |
| 3 | 前置步骤标记为 Skipped（使用缓存） | 终端清晰展示哪些步骤用了缓存、哪些是真正执行的 |
| 4 | 失败时打印详细信息 + 建议命令 | 包含：step ID、错误码、错误消息、修复建议、重试命令 |
| 5 | 不影响正常 `run` 行为 | 不带 `--from-step` 时行为完全不变 |
| 6 | 向后兼容 | YAML 语法不变，所有现有测试通过 |

---

## 三、核心概念

### 3.1 `RunContext`（运行上下文快照）

**每个 batch 执行后自动保存**的 JSON 文件（不仅仅是失败时），包含继续执行所需的全部信息：

```
┌──────────────────────────────────────────────────────┐
│ RunContext                                            │
│                                                       │
│  workflow_file: String      // 工作流文件绝对路径       │
│  saved_at: DateTime<Utc>    // 保存时间               │
│                                                       │
│  inputs: HashMap<String, Value>       // 输入参数      │
│  variables: HashMap<String, Value>    // 工作流变量    │
│                                                       │
│  step_outputs: HashMap<StepId, StepResult>            │
│    // 所有已完成步骤的完整输出（包含 output、timing 等）│
│                                                       │
│  failed_step: FailedStepInfo | null   // 失败步骤信息  │
│                                                       │
└──────────────────────────────────────────────────────┘

FailedStepInfo:
  step_id: String
  error_code: String
  error_message: String
  fix_suggestion: Option<String>
```

### 3.2 存储位置

```
/tmp/flow-run-contexts/<workflow_file_hash>.json
```

- **key 是工作流文件的哈希**（不是 workflow name，因为 name 可能重复）
- 同一个 YAML 文件只有一份最新上下文（覆盖写）
- 路径示例：`/tmp/flow-run-contexts/a1b2c3d4.json`

### 3.3 数据流

```
═══ 正常执行 (run) — 每步保存上下文 ═══

FlowRunner::from_file("workflow.yaml")
  └─ FlowRunner::run(inputs)
       └─ Scheduler::run(workflow_path)
            │
            ├─ 执行 batch 0: [checkout]       ✅ 成功
            │    └─ 保存 step_output 到 context
            │    └─ ★ 保存 RunContext 到磁盘（增量：已包含 checkout 输出）
            │
            ├─ 执行 batch 1: [install]         ✅ 成功
            │    └─ 保存 step_output 到 context
            │    └─ ★ 保存 RunContext 到磁盘（增量：已包含 checkout + install 输出）
            │
            ├─ 执行 batch 2: [build]           ❌ 失败
            │    └─ ★ 保存 RunContext 到磁盘（含失败步骤信息）
            │
            └─ return WorkflowResult (status=Failed)
                 │
                 ▼
          main.rs 检测到失败
            └─ 打印详细失败信息 + --from-step 建议
               （上下文已在 Scheduler 内保存，无需再手动保存）


═══ 从步骤继续 (run --from-step build) ═══

main.rs 解析 --from-step build
  └─ FlowRunner::run_from_step("build", inputs)
       │
       ├─ 加载 RunContext: /tmp/flow-run-contexts/xxx.json
       │    └─ 恢复 inputs、variables、step_outputs 到 ExecutionContext
       │
       └─ Scheduler::run_from_step("build")
            │
            ├─ topological_sort() → batches: [[checkout], [install], [build], [test]]
            │
            ├─ 找到 build 在 batch 2 (index=2)
            │
            ├─ batch 0~1: 标记为 Skipped (使用恢复的输出)
            │    └─ StepResult::skipped("checkout", "使用缓存输出")
            │    └─ StepResult::skipped("install", "使用缓存输出")
            │
            └─ batch 2+: 正常执行（同样每步保存上下文）
                 └─ [build] ✅  → [test] ✅  → ... 完成
```

---

## 四、用户 API

### 4.1 CLI 用法

```bash
# 正常执行（每步自动保存上下文，失败时打印详细建议）
flow-run workflow.yaml run

# 从指定步骤继续（自动加载上次保存的上下文）
flow-run workflow.yaml run --from-step build_backend

# 从任意步骤开始（即使之前的步骤没有执行过也可以，只要依赖的步骤有输出即可）
flow-run workflow.yaml run --from-step deploy

# 结合 --input 覆盖参数
flow-run workflow.yaml run --from-step deploy --input env=production
```

### 4.2 Rust 库用户

```rust
// 正常执行（每步自动保存上下文）
let result = runner.run(inputs).await?;

// 从指定步骤继续
let result = runner.run_from_step("build_backend", inputs).await?;
```

### 4.3 YAML 侧

**零变化**。不需要在工作流 YAML 中配置任何东西。

---

## 五、改动清单

| 文件 | 改动 | 说明 |
|:-----|:-----|:-----|
| `src/utils/run_context.rs` | `RunContext.save()` 接收 `failed_step: Option` | failed_step 可为 None（正常保存） |
| `src/core/runner.rs` | `run()` 传入 `workflow_path` 到 Scheduler | Scheduler 需要知道文件路径以保存上下文 |
| `src/core/dag.rs` | `Scheduler` 新增 `workflow_path` 字段；`run()` 每 batch 后调用 `RunContext::save()`；`run_from_step()` 同样每步保存 | 核心改动 |
| `src/main.rs` | 移除 `save_run_context()` 函数；失败时只打印详情 + 建议 | 保存逻辑已移入 Scheduler |

---

## 六、关键设计变更（相对于初版）

### 6.1 保存时机：从"失败时保存"改为"每步保存"

**初版**：在 `main.rs` 中检测到 `WorkflowStatus::Failed` 后，才从 `result.steps` 中收集数据保存。

**优化后**：在 `Scheduler::run()` 的 batch 循环中，每执行完一个 batch 就保存一次 RunContext。

优势：
1. **进程崩溃也不丢数据** — 即使被 `kill -9`，前面步骤的输出已经在磁盘上
2. **可以从任意步骤恢复** — 不仅仅是失败步骤
3. **保存逻辑更内聚** — 不需要 main.rs 在事后反推哪些步骤成功了

### 6.2 Scheduler 需要 workflow_path

`Scheduler` 需要知道工作流文件路径才能计算 RunContext 的保存位置。

新增字段：
```rust
pub struct Scheduler {
    // ... 现有字段 ...
    workflow_path: Option<PathBuf>,  // ← 新增
}
```

`FlowRunner::run()` 和 `run_from_step()` 创建 Scheduler 时传入。

### 6.3 保存逻辑的位置

```
Scheduler::run() {
    for (batch_index, batch) in batches.iter().enumerate() {
        let batch_results = self.execute_batch(batch).await?;
        
        // 保存 step_outputs 到 context（已有逻辑）
        ...
        
        // ★ 新增：每步保存 RunContext
        self.save_run_context(&all_results, None);  // None = 无失败
        // 如果 has_failure:
        self.save_run_context(&all_results, Some(failed_info));
    }
}
```

### 6.4 main.rs 简化

移除 `save_run_context()` 函数。失败时只需打印详情 + 建议，因为上下文已经由 Scheduler 保存。

```rust
// 正常执行模式
let result = runner.run(input_map).await?;
if matches!(result.status, WorkflowStatus::Failed) {
    // 上下文已由 Scheduler 每步保存，只需打印
    print_failure_detail(&result, &cli.workflow);
    std::process::exit(1);
} else {
    print_result(&result, json);
}
```

---

## 七、不涉及改动的部分

- **YAML 语法**：零变化
- **`StepDefinition` / `WorkflowDefinition`**：类型定义不变
- **`resume` 命令**：完全不受影响，独立运作
- **`CheckpointManager`**：不被修改
- **Parser / Validation**：不变
- **所有 Executor**（HTTP/Shell/Loop/Condition/Workflow/Approve/Agent/Tool）：不变

---

## 八、边界情况处理

| 场景 | 处理 |
|:-----|:-----|
| `--from-step` 但没有保存的上下文 | 报错：`H001: 未找到运行上下文，请先正常执行工作流` |
| `--from-step` 指定不存在的 step ID | 报错：`H003: 步骤 'xxx' 不存在于工作流中。可用步骤: a, b, c` |
| `--from-step` 指定第一个步骤 | 等同于正常 run（没有步骤被跳过），但会用保存的 inputs |
| 前置步骤中有未保存输出的步骤 | 标记为 Skipped，output 设为 null，后续引用该步骤的模板可能失败 |
| 工作流 YAML 在两次运行之间被修改 | 上下文中的 step_outputs 可能与新的 YAML 不匹配 → 执行时报模板错误 |
| 进程在 batch 之间被 kill | 上一个 batch 的输出已保存，可从下一个 batch 的任意步骤恢复 |
| 成功完成后是否清理上下文 | 不清理，保留最后一次执行的上下文 |

---

## 九、总结

| 维度 | 设计决策 |
|:-----|:---------|
| 保存时机 | **每个 batch 执行后自动保存**，零配置 |
| 存储位置 | `/tmp/flow-run-contexts/<hash>.json` |
| 恢复粒度 | 精确到 step ID |
| 前置步骤处理 | Skipped + 保留缓存 output |
| inputs 合并 | CLI `--input` 优先，缺失的从 RunContext 取 |
| 与 resume 的关系 | 并行独立，互不影响 |
| 用户交互 | 失败时打印详细信息 + 可直接复制的 `--from-step` 命令 |
