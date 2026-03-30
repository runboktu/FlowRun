# Library/CLI Separation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Separate `flow-run` into a reusable library (`FlowRunner` API) and a thin CLI shell, enabling other Rust applications to embed the workflow engine.

**Architecture:** Introduce `FlowRunner` struct in `core/runner.rs` as the public API entry point. Move output formatting to `cli/output.rs`. Make `clap` an optional dependency. `main.rs` becomes ~80 lines of pure CLI glue.

**Tech Stack:** Rust, clap (optional feature), serde, tokio

---

## Current State

```
src/
├── lib.rs          # Re-exports 4 modules, no public API
├── main.rs         # 505 lines: CLI + execution logic + formatting mixed
├── cli/
│   ├── mod.rs      # pub mod commands
│   └── commands.rs # clap definitions (Args, Commands, etc.)
├── core/
│   ├── context.rs  # ExecutionContext
│   ├── dag.rs      # DagScheduler + Scheduler (run/resume logic)
│   ├── parser.rs   # WorkflowParser
│   ├── template.rs # TemplateEngine
│   └── types.rs    # All data structures
├── executors/      # Step executors (http, shell, etc.)
└── utils/
    ├── checkpoint.rs
    ├── error.rs
    └── retry.rs
```

## Target State

```
src/
├── lib.rs          # Re-exports + pub mod runner
├── main.rs         # ~80 lines: parse args → call API → format output
├── cli/
│   ├── mod.rs      # pub mod commands + pub mod output
│   ├── commands.rs # Unchanged
│   └── output.rs   # NEW: print_result, print_execution_plan
├── core/
│   ├── mod.rs      # + pub mod runner
│   ├── runner.rs   # NEW: FlowRunner struct
│   ├── context.rs  # Unchanged
│   ├── dag.rs      # Unchanged
│   ├── parser.rs   # Unchanged
│   ├── template.rs # Unchanged
│   └── types.rs    # + ExecutionPlan, DagEdge structs
├── executors/      # Unchanged
└── utils/          # Unchanged
```

## Dependencies Change

```toml
# Before: clap is mandatory
clap = { version = "4.4", features = ["derive"] }

# After: clap is optional (CLI-only)
[features]
default = ["cli"]
cli = ["clap", "tracing-subscriber"]

[dependencies]
clap = { version = "4.4", features = ["derive"], optional = true }
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"], optional = true }
```

## Library User Experience (Target)

```rust
use flow_run::core::runner::FlowRunner;
use std::collections::HashMap;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let runner = FlowRunner::from_file("workflow.yaml")?;

    let mut inputs = HashMap::new();
    inputs.insert("api_url".to_string(), serde_json::json!("https://api.example.com"));

    let result = runner.run(inputs).await?;
    println!("Status: {:?}", result.status);
    Ok(())
}
```

---

## Implementation Tasks

### Task 1: Add `ExecutionPlan` and `DagEdge` to `types.rs`

**Files:**
- Modify: `src/core/types.rs` (append after `ExecutionMetrics`)

**Step 1: Add structs to types.rs**

```rust
/// DAG 边（依赖关系）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagEdge {
    /// 源步骤 ID
    pub from: StepId,
    /// 目标步骤 ID
    pub to: StepId,
}

/// 执行计划（dry-run 产出的结构化数据）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    /// 工作流名称
    pub workflow_name: String,
    /// 工作流版本
    pub workflow_version: Option<String>,
    /// 工作流描述
    pub workflow_description: Option<String>,
    /// 步骤总数
    pub step_count: usize,
    /// 是否有循环依赖
    pub has_cycle: bool,
    /// 全局配置
    pub config: Option<WorkflowConfig>,
    /// 输入参数定义
    pub inputs: Option<Vec<InputDefinition>>,
    /// 实际传入的输入参数
    pub provided_inputs: Vec<(String, String)>,
    /// 输出定义
    pub outputs: Option<HashMap<String, String>>,
    /// 拓扑排序批次
    pub batches: Vec<Vec<StepId>>,
    /// DAG 边列表
    pub dag_edges: Vec<DagEdge>,
}
```

**Step 2: Verify compilation**

```bash
cargo check
```

**Step 3: Commit**

```bash
git add src/core/types.rs
git commit -m "feat(types): add ExecutionPlan and DagEdge structs for dry-run data"
```

---

### Task 2: Create `src/core/runner.rs` — FlowRunner

**Files:**
- Create: `src/core/runner.rs`
- Modify: `src/core/mod.rs` (add `pub mod runner;`)

**Step 1: Write failing test for FlowRunner**

Create `src/core/runner.rs` with initial test:

```rust
use crate::core::context::ExecutionContext;
use crate::core::dag::{DagScheduler, Scheduler};
use crate::core::parser::WorkflowParser;
use crate::core::types::*;
use crate::utils::checkpoint::CheckpointManager;
use crate::utils::error::WorkflowError;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 工作流执行器 — 库的高层 API 入口
///
/// 封装了 DagScheduler + Scheduler + CheckpointManager 的组装逻辑，
/// 外部应用只需 `FlowRunner::from_file()` + `runner.run()` 即可执行工作流。
pub struct FlowRunner {
    workflow: WorkflowDefinition,
    checkpoint_dir: PathBuf,
}

impl FlowRunner {
    /// 从工作流定义创建
    pub fn new(workflow: WorkflowDefinition) -> Self {
        Self {
            workflow,
            checkpoint_dir: std::env::temp_dir().join(format!("flow-run-{}", std::process::id())),
        }
    }

    /// 从 YAML 文件创建
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, WorkflowError> {
        let workflow = WorkflowParser::from_file(path)?;
        Ok(Self::new(workflow))
    }

    /// 设置检查点目录（builder 模式）
    pub fn with_checkpoint_dir(mut self, dir: PathBuf) -> Self {
        self.checkpoint_dir = dir;
        self
    }

    /// 执行工作流
    pub async fn run(&self, inputs: HashMap<String, serde_json::Value>) -> Result<WorkflowResult, WorkflowError> {
        let dag = DagScheduler::new(self.workflow.steps.clone())?;
        let checkpoint_manager = CheckpointManager::new(self.checkpoint_dir.clone())?;
        let config = self.workflow.config.clone().unwrap_or_default();
        let scheduler = Scheduler::new(dag, config, checkpoint_manager);

        let context = ExecutionContext::new(&self.workflow, inputs);
        scheduler.set_context(context).await;

        if let Some(outputs) = &self.workflow.outputs {
            let outputs_map: HashMap<String, String> = outputs
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            scheduler.set_outputs(outputs_map).await;
        }

        Ok(scheduler.run().await?)
    }

    /// 从检查点恢复执行
    pub async fn resume(
        &self,
        checkpoint_id: &str,
        inputs: HashMap<String, serde_json::Value>,
    ) -> Result<WorkflowResult, WorkflowError> {
        let dag = DagScheduler::new(self.workflow.steps.clone())?;
        let checkpoint_manager = CheckpointManager::new(self.checkpoint_dir.clone())?;
        let config = self.workflow.config.clone().unwrap_or_default();
        let scheduler = Scheduler::new(dag, config, checkpoint_manager);

        let context = ExecutionContext::new(&self.workflow, inputs);
        scheduler.set_context(context).await;

        if let Some(outputs) = &self.workflow.outputs {
            let outputs_map: HashMap<String, String> = outputs
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            scheduler.set_outputs(outputs_map).await;
        }

        Ok(scheduler.resume(checkpoint_id).await?)
    }

    /// 生成执行计划（dry-run 的数据层）
    pub fn plan(&self, provided_inputs: &[(String, String)]) -> Result<ExecutionPlan, WorkflowError> {
        let dag = DagScheduler::new(self.workflow.steps.clone())?;
        let batches = dag.topological_sort()?;

        // 构建 DAG 边
        let mut dag_edges = Vec::new();
        for step in &self.workflow.steps {
            if let Some(deps) = &step.depends_on {
                for dep in deps {
                    dag_edges.push(DagEdge {
                        from: dep.clone(),
                        to: step.id.clone(),
                    });
                }
            }
        }

        Ok(ExecutionPlan {
            workflow_name: self.workflow.name.clone(),
            workflow_version: self.workflow.version.clone(),
            workflow_description: self.workflow.description.clone(),
            step_count: self.workflow.steps.len(),
            has_cycle: false, // topological_sort 已经检查过
            config: self.workflow.config.clone(),
            inputs: self.workflow.inputs.clone(),
            provided_inputs: provided_inputs.to_vec(),
            outputs: self.workflow.outputs.clone(),
            batches,
            dag_edges,
        })
    }

    /// 获取工作流定义的引用
    pub fn workflow(&self) -> &WorkflowDefinition {
        &self.workflow
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runner_from_str() {
        let yaml = r#"
name: "test_workflow"
steps:
  - id: "step1"
    type: "shell"
    run: "echo hello"
"#;
        let workflow = WorkflowParser::from_str(yaml).unwrap();
        let runner = FlowRunner::new(workflow);
        assert_eq!(runner.workflow().name, "test_workflow");
    }

    #[test]
    fn test_runner_plan() {
        let yaml = r#"
name: "test_workflow"
steps:
  - id: "step1"
    type: "shell"
    run: "echo hello"
  - id: "step2"
    type: "shell"
    run: "echo world"
    depends_on: ["step1"]
"#;
        let workflow = WorkflowParser::from_str(yaml).unwrap();
        let runner = FlowRunner::new(workflow);

        let plan = runner.plan(&[]).unwrap();
        assert_eq!(plan.workflow_name, "test_workflow");
        assert_eq!(plan.step_count, 2);
        assert_eq!(plan.batches.len(), 2);
        assert_eq!(plan.dag_edges.len(), 1);
        assert_eq!(plan.dag_edges[0].from, "step1");
        assert_eq!(plan.dag_edges[0].to, "step2");
    }
}
```

**Step 2: Add to core/mod.rs**

```rust
pub mod dag;
pub mod parser;
pub mod template;
pub mod context;
pub mod types;
pub mod runner;  // NEW
```

**Step 3: Run tests**

```bash
cargo test -p flow-run core::runner
```

**Step 4: Commit**

```bash
git add src/core/runner.rs src/core/mod.rs
git commit -m "feat(core): add FlowRunner struct with run/resume/plan API"
```

---

### Task 3: Create `src/cli/output.rs` — Output Formatting

**Files:**
- Create: `src/cli/output.rs`
- Modify: `src/cli/mod.rs` (add `pub mod output;`)

**Step 1: Create output.rs with formatting functions**

Extract `print_result` and `dry_run` formatting from `main.rs`:

```rust
use crate::core::types::*;

/// 打印工作流执行结果
pub fn print_result(result: &WorkflowResult, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(result).unwrap_or_default());
        return;
    }

    println!("\n执行结果: {:?}", result.status);
    println!("步骤结果:");
    for step in &result.steps {
        let icon = match step.status {
            StepStatus::Success => "OK",
            StepStatus::Failed => "FAIL",
            StepStatus::Skipped => "SKIP",
            StepStatus::Pending => "PEND",
            StepStatus::Running => "RUN",
        };
        print!("  [{}] {}", icon, step.step_id);
        if let Some(output) = &step.output {
            if let Some(stdout) = output.get("stdout").and_then(|v| v.as_str()) {
                let trimmed = stdout.replace('\n', " ").trim().to_string();
                if !trimmed.is_empty() {
                    print!("  {}", &trimmed[..trimmed.len().min(120)]);
                }
            }
            if let Some(status_code) = output.get("status_code") {
                print!("  status={}", status_code);
            }
        }
        println!();
    }

    println!("\n指标:");
    println!("  总步骤: {} | 成功: {} | 失败: {} | 跳过: {}",
        result.metrics.total_steps,
        result.metrics.success_steps,
        result.metrics.failed_steps,
        result.metrics.skipped_steps,
    );
    println!("  耗时: {}ms", result.metrics.total_duration_ms);

    if let Some(outputs) = &result.outputs {
        if !outputs.is_empty() {
            println!("\n工作流输出:");
            for (key, value) in outputs {
                println!("  {}: {}", key, value);
            }
        }
    }
}

/// 打印执行计划（dry-run 输出）
pub fn print_execution_plan(plan: &ExecutionPlan, json: bool, workflow_file: &std::path::Path) {
    if json {
        println!("{}", serde_json::to_string_pretty(plan).unwrap_or_default());
        return;
    }

    println!("══════════════════════════════════════════════");
    println!("  Dry Run: {}", plan.workflow_name);
    println!("══════════════════════════════════════════════");

    if let Some(desc) = &plan.workflow_description {
        println!("  描述: {}", desc);
    }
    if let Some(ver) = &plan.workflow_version {
        println!("  版本: {}", ver);
    }
    println!("  文件: {}", workflow_file.display());
    println!("  步骤: {} 个", plan.step_count);
    println!("  DAG 检查: 无循环依赖");

    if let Some(config) = &plan.config {
        println!("\n── 全局配置 ──");
        if let Some(timeout) = &config.timeout {
            println!("  超时: {}", timeout);
        }
        if let Some(failure) = &config.on_failure {
            println!("  失败策略: {:?}", failure);
        }
        if let Some(cp) = &config.checkpoint {
            println!("  检查点: {}", cp);
        }
        if let Some(max) = config.max_concurrent {
            println!("  最大并发: {}", max);
        }
        if let Some(retry) = &config.retry {
            println!("  全局重试: max={}, strategy={:?}", retry.max_attempts, retry.strategy);
        }
    }

    if let Some(input_defs) = &plan.inputs {
        println!("\n── 输入参数 ──");
        for inp in input_defs {
            let req = if inp.required == Some(true) { "必填" } else { "可选" };
            println!("  {} [{}]: {}", inp.name, req, inp.r#type.as_deref().unwrap_or("any"));
        }
        if !plan.provided_inputs.is_empty() {
            println!("  ─────────────");
            for (k, v) in &plan.provided_inputs {
                println!("  {} = {}", k, v);
            }
        }
    }

    if let Some(outputs) = &plan.outputs {
        println!("\n── 工作流输出 ──");
        for (key, expr) in outputs {
            println!("  {}: {}", key, expr);
        }
    }

    println!("\n── DAG 结构 ──");
    println!("  节点: {} | 边: {}", plan.step_count, plan.dag_edges.len());
    for edge in &plan.dag_edges {
        println!("  {} ──→ {}", edge.from, edge.to);
    }

    println!("\n── 拓扑排序（执行计划）──");
    println!("  共 {} 个批次", plan.batches.len());
    for (i, batch) in plan.batches.iter().enumerate() {
        let parallel_tag = if batch.len() > 1 { " (并行)" } else { "" };
        println!("  批次 {}:{} {} 个步骤", i + 1, parallel_tag, batch.len());
        for step_id in batch {
            println!("    ├─ {}", step_id);
        }
    }

    println!("\n══════════════════════════════════════════════");
    println!("  以上为模拟执行，未实际运行任何步骤");
    println!("══════════════════════════════════════════════");
}
```

**Step 2: Add to cli/mod.rs**

```rust
pub mod commands;
pub mod output;  // NEW
```

**Step 3: Verify compilation**

```bash
cargo check
```

**Step 4: Commit**

```bash
git add src/cli/output.rs src/cli/mod.rs
git commit -m "feat(cli): add output.rs for result and plan formatting"
```

---

### Task 4: Rewrite `main.rs` — Thin CLI Shell

**Files:**
- Modify: `src/main.rs`

**Step 1: Rewrite main.rs**

Replace the entire file with thin CLI glue:

```rust
use clap::Parser;
use flow_run::cli::commands::{Args, CheckpointAction, CleanStrategy, Commands};
use flow_run::cli::output::{print_execution_plan, print_result};
use flow_run::core::runner::FlowRunner;
use flow_run::core::types::WorkflowStatus;
use std::collections::HashMap;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Args::parse();

    match cli.command {
        Commands::Run { input, json, dry_run, .. } => {
            let runner = FlowRunner::from_file(&cli.workflow)?;
            if dry_run {
                let plan = runner.plan(&input)?;
                print_execution_plan(&plan, json, &cli.workflow);
            } else {
                let input_map = build_input_map(&input);
                let result = runner.run(input_map).await?;
                print_result(&result, json);
                if !matches!(result.status, WorkflowStatus::Success) {
                    std::process::exit(1);
                }
            }
        }

        Commands::Resume { checkpoint_id, input, json } => {
            let runner = FlowRunner::from_file(&cli.workflow)?;
            let input_map = build_input_map(&input);
            let result = runner.resume(&checkpoint_id, input_map).await?;
            print_result(&result, json);
            if !matches!(result.status, WorkflowStatus::Success) {
                std::process::exit(1);
            }
        }

        Commands::DryRun { input, json } => {
            let runner = FlowRunner::from_file(&cli.workflow)?;
            let plan = runner.plan(&input)?;
            print_execution_plan(&plan, json, &cli.workflow);
        }

        Commands::Validate { show_dag, json } => {
            match FlowRunner::from_file(&cli.workflow) {
                Ok(runner) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(runner.workflow())?);
                    } else {
                        println!("✅ 工作流验证通过: {}", runner.workflow().name);
                        if show_dag {
                            println!("步骤:");
                            for step in &runner.workflow().steps {
                                println!("  - {} ({})", step.id, serde_json::to_string(&step.r#type).unwrap_or_default());
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("❌ 验证失败: {}", e);
                    std::process::exit(1);
                }
            }
        }

        Commands::Checkpoint { action } => {
            handle_checkpoint_action(action);
        }

        Commands::History { limit, status, failed, json } => {
            if json {
                println!("{{\"history\": []}}");
            } else {
                println!("执行历史:");
                println!("  限制: {}", limit);
                if let Some(s) = status {
                    println!("  状态过滤: {}", s);
                }
                if failed {
                    println!("  只显示失败");
                }
            }
        }

        Commands::Schema { output, pretty } => {
            let schema = r#"{"$schema": "http://json-schema.org/draft-07/schema#", "type": "object"}"#;
            if let Some(path) = output {
                std::fs::write(&path, schema)?;
                println!("Schema 已写入: {}", path.display());
            } else if pretty {
                println!("{}", serde_json::to_string_pretty(&serde_json::from_str::<serde_json::Value>(schema)?)?);
            } else {
                println!("{}", schema);
            }
        }
    }

    Ok(())
}

fn build_input_map(inputs: &[(String, String)]) -> HashMap<String, serde_json::Value> {
    let mut map = HashMap::new();
    for (k, v) in inputs {
        map.insert(k.clone(), serde_json::json!(v));
    }
    map
}

fn handle_checkpoint_action(action: CheckpointAction) {
    match action {
        CheckpointAction::List { verbose, status, json } => {
            if json {
                println!("{{\"checkpoints\": []}}");
            } else {
                println!("检查点列表:");
                if verbose {
                    println!("  (详细模式)");
                }
                if let Some(s) = status {
                    println!("  状态过滤: {}", s);
                }
            }
        }
        CheckpointAction::Show { id, steps, json } => {
            if json {
                println!("{{\"id\": \"{}\"}}", id);
            } else {
                println!("检查点详情: {}", id);
                if steps {
                    println!("  (显示步骤信息)");
                }
            }
        }
        CheckpointAction::Clean { strategy } => {
            match strategy {
                CleanStrategy::Id { ids } => {
                    for id in ids {
                        println!("清理检查点: {}", id);
                    }
                }
                CleanStrategy::All { confirm } => {
                    if confirm {
                        println!("清理所有检查点");
                    } else {
                        println!("需要 --confirm 确认清理操作");
                    }
                }
                CleanStrategy::OlderThan { days } => {
                    println!("清理超过 {} 天的检查点", days);
                }
                CleanStrategy::Status { status } => {
                    println!("清理状态为 {} 的检查点", status);
                }
                CleanStrategy::Keep { count } => {
                    println!("保留最近 {} 个检查点", count);
                }
            }
        }
    }
}
```

**Step 2: Verify compilation**

```bash
cargo check
```

**Step 3: Run all tests**

```bash
cargo test
```

**Step 4: Test CLI manually**

```bash
cargo run -- examples/01_basic_http.yaml dry-run
cargo run -- examples/02_basic_shell.yaml run --input project_name=test
```

**Step 5: Commit**

```bash
git add src/main.rs
git commit -m "refactor: slim main.rs to thin CLI shell using FlowRunner API"
```

---

### Task 5: Make `clap` and `tracing-subscriber` Optional

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/lib.rs` (conditional re-export of cli module)

**Step 1: Update Cargo.toml**

```toml
[features]
default = ["cli"]
cli = ["clap", "tracing-subscriber"]

[dependencies]
# CLI 框架 (optional)
clap = { version = "4.4", features = ["derive"], optional = true }

# 日志
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"], optional = true }
```

**Step 2: Update lib.rs with conditional compilation**

```rust
//! flow-run: 专为 AI Agent 设计的声明式工作流引擎
//!
//! 核心功能:
//! - YAML 声明式工作流定义
//! - DAG 调度引擎，自动并行执行无依赖步骤
//! - 检查点与断点续跑
//! - 多种步骤执行器 (HTTP/Shell/Loop/Condition/Workflow/Approve)
//! - 模板表达式与过滤器链
//! - 重试引擎与错误处理

#[cfg(feature = "cli")]
pub mod cli;
pub mod core;
pub mod executors;
pub mod utils;

// Re-export 核心类型，方便库用户使用
pub use core::runner::FlowRunner;
pub use core::types::*;
pub use core::parser::WorkflowParser;
pub use utils::error::WorkflowError;
```

**Step 3: Verify library builds without CLI feature**

```bash
cargo check --no-default-features
```

**Step 4: Verify CLI still works**

```bash
cargo check --features cli
cargo run -- examples/01_basic_http.yaml dry-run
```

**Step 5: Run all tests**

```bash
cargo test
```

**Step 6: Commit**

```bash
git add Cargo.toml src/lib.rs
git commit -m "feat: make clap/tracing-subscriber optional, add feature 'cli'"
```

---

### Task 6: Verify End-to-End

**Step 1: Full test suite**

```bash
cargo test --all-features
```

**Step 2: Test library usage (create temp example)**

Create `/tmp/test_flow_run/Cargo.toml`:

```toml
[package]
name = "test-flow-run"
version = "0.1.0"
edition = "2021"

[dependencies]
flow-run = { path = "/Users/mingshu/workspace/code/ai/cli/flow-run", default-features = false }
tokio = { version = "1", features = ["full"] }
serde_json = "1"
```

Create `/tmp/test_flow_run/src/main.rs`:

```rust
use flow_run::FlowRunner;
use std::collections::HashMap;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let runner = FlowRunner::from_file("workflow.yaml")?;

    let mut inputs = HashMap::new();
    inputs.insert("name".to_string(), serde_json::json!("world"));

    let result = runner.run(inputs).await?;
    println!("Status: {:?}", result.status);

    Ok(())
}
```

```bash
cd /tmp/test_flow_run && cargo check
```

**Step 3: Final commit if any fixes needed**

---

## Rollback Plan

If issues arise:

```bash
git log --oneline  # Find commit before refactoring
git revert <commit-hash>  # Or reset to pre-refactor state
```

## Success Criteria

- [ ] `cargo test --all-features` passes
- [ ] `cargo check --no-default-features` succeeds (library-only build)
- [ ] `cargo run -- examples/01_basic_http.yaml dry-run` works
- [ ] `main.rs` is < 100 lines
- [ ] External app can use `FlowRunner` without pulling in `clap`
