//! Supervisor 多智能体编排示例
//!
//! Fan-out / Fan-in 模式：
//!   Supervisor → [Research, Math] (并行) → Writer → Summary
//!
//! 运行：
//!   DEEPSEEK_API_KEY=sk-xxx cargo run --example 21_supervisor_pattern

use flow_run::core::context::ExecutionContext;
use flow_run::core::dag::{DagScheduler, Scheduler};
use flow_run::core::parser::WorkflowParser;
use flow_run::agent::BuiltinToolRegistry;
use flow_run::utils::checkpoint::CheckpointManager;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tempfile::tempdir;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("flow_run=info")
        .init();

    println!("╔══════════════════════════════════════════════════╗");
    println!("║  flow-run Supervisor 多智能体编排示例            ║");
    println!("║  Fan-out / Fan-in 多 Agent 协作模式             ║");
    println!("╚══════════════════════════════════════════════════╝\n");

    let api_key = std::env::var("DEEPSEEK_API_KEY")
        .or_else(|_| std::env::var("OPENAI_API_KEY"))
        .ok();

    if api_key.is_none() {
        println!("⚠️  未设置 DEEPSEEK_API_KEY 或 OPENAI_API_KEY");
        println!("    将仅展示工作流结构，Agent 步骤无法实际调用 LLM\n");
    }

    let workflow_path = Path::new("examples/21_supervisor_pattern.yaml");
    println!("[1] 加载 Supervisor 工作流: {:?}", workflow_path);
    let workflow = WorkflowParser::from_file(workflow_path)?;
    println!("    ✅ 加载成功");
    println!("    名称: {}", workflow.name);
    println!("    描述: {}", workflow.description.as_deref().unwrap_or(""));
    println!("    顶层步骤数: {}\n", workflow.steps.len());

    println!("[2] 验证工作流...");
    WorkflowParser::validate(&workflow)?;
    println!("    ✅ 验证通过\n");

    println!("[3] DAG 分析:");
    let dag = DagScheduler::new(workflow.steps.clone())?;

    match dag.has_cycle() {
        Ok(false) => println!("    ✅ 无循环依赖"),
        Ok(true) => {
            println!("    ❌ 循环依赖!");
            return Err(anyhow::anyhow!("循环依赖"));
        }
        Err(e) => {
            println!("    ❌ {}", e);
            return Err(e.into());
        }
    }

    let batches = dag.topological_sort()?;
    println!("    执行批次:");
    for (i, batch) in batches.iter().enumerate() {
        let parallel_marker = if batch.len() > 1 { " ← 并行" } else { "" };
        println!("      批次 {}: {:?}{}", i + 1, batch, parallel_marker);
    }
    println!();

    println!("[4] 工作流架构:");
    println!("    ┌──────────────┐");
    println!("    │  Supervisor   │ Phase 1: 分析规划");
    println!("    └──┬───────┬───┘");
    println!("       │       │");
    println!("       ▼       ▼");
    println!("    Research  Math     Phase 2: 并行执行");
    println!("       │       │");
    println!("       └───┬───┘");
    println!("           ▼");
    println!("        Writer            Phase 3: 汇聚撰写");
    println!("           ▼");
    println!("        Summary           Phase 4: 输出");
    println!();

    let question = "FAANG 公司 2024 年总员工数是多少？计算出总人数的平方根";
    println!("[5] 用户问题: {}\n", question);

    let mut inputs = HashMap::new();
    inputs.insert("question".to_string(), serde_json::json!(question));

    let context = ExecutionContext::new(&workflow, inputs);
    println!("    执行 ID: {}\n", context.execution_id);

    println!("[6] 开始执行...\n");
    let checkpoint_dir = tempdir()?;
    let checkpoint_manager = CheckpointManager::new(checkpoint_dir.path().to_path_buf())?;
    let config = workflow.config.clone().unwrap_or_default();
    let builtin_registry = Arc::new(BuiltinToolRegistry::with_defaults());
    let scheduler = Scheduler::new(dag, config, checkpoint_manager, builtin_registry);
    scheduler.set_context(context).await;

    if let Some(outputs) = &workflow.outputs {
        let outputs_map: HashMap<String, String> = outputs
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        scheduler.set_outputs(outputs_map).await;
    }

    let result = scheduler.run().await?;

    println!("\n[7] 执行结果:");
    println!("    状态: {:?}", result.status);

    for step in &result.steps {
        println!("    ── {} ({:?}) ──", step.step_id, step.status);
        if let Some(output) = &step.output {
            if let Some(answer) = output.get("answer").and_then(|v| v.as_str()) {
                let preview: String = answer.chars().take(120).collect();
                println!("       {}", preview);
            } else if let Some(stdout) = output.get("stdout").and_then(|v| v.as_str()) {
                let preview: String = stdout.chars().take(120).collect();
                println!("       {}", preview);
            }
        }
    }

    println!("\n[8] 执行指标:");
    println!("    总步骤: {}", result.metrics.total_steps);
    println!("    成功: {}", result.metrics.success_steps);
    println!("    失败: {}", result.metrics.failed_steps);
    println!("    跳过: {}", result.metrics.skipped_steps);
    println!("    耗时: {}ms\n", result.metrics.total_duration_ms);

    if let Some(outputs) = &result.outputs {
        println!("[9] 工作流输出:");
        for (key, value) in outputs {
            let preview: String = value.to_string().chars().take(100).collect();
            println!("    {}: {}", key, preview);
        }
        println!();
    }

    println!("╔══════════════════════════════════════════════════╗");
    println!("║  Supervisor 多智能体编排示例完成!                ║");
    println!("╚══════════════════════════════════════════════════╝");

    Ok(())
}
