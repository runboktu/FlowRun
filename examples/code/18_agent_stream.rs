use flow_run::core::context::ExecutionContext;
use flow_run::core::dag::{DagScheduler, Scheduler};
use flow_run::core::parser::WorkflowParser;
use flow_run::utils::checkpoint::CheckpointManager;
use std::collections::HashMap;
use std::path::Path;
use tempfile::tempdir;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("flow_run=info")
        .init();

    println!("==========================================");
    println!("  flow-run - Agent 流式输出示例");
    println!("==========================================\n");

    let workflow_path = Path::new("examples/18_agent_stream.yaml");
    println!("[1] 加载工作流: {:?}", workflow_path);
    let workflow = WorkflowParser::from_file(workflow_path)?;
    println!("    ✅ {}\n", workflow.name);

    let dag = DagScheduler::new(workflow.steps.clone())?;
    match dag.has_cycle() {
        Ok(false) => println!("    ✅ 无循环依赖\n"),
        Ok(true) => return Err(anyhow::anyhow!("工作流包含循环依赖")),
        Err(e) => return Err(e.into()),
    }

    let question = "用三句话解释什么是 Rust 的所有权系统？";
    let mut inputs = HashMap::new();
    inputs.insert("question".to_string(), serde_json::json!(question));
    println!("[2] 问题: {}\n", question);

    let context = ExecutionContext::new(&workflow, inputs);
    let checkpoint_dir = tempdir()?;
    let checkpoint_manager = CheckpointManager::new(checkpoint_dir.path().to_path_buf())?;
    let config = workflow.config.clone().unwrap_or_default();
    let scheduler = Scheduler::new(dag, config, checkpoint_manager);
    scheduler.set_context(context).await;

    println!("[3] 执行工作流（流式输出见下方）:");
    println!("    ──────────────────────────────────");
    let result = scheduler.run().await?;

    println!("\n    ──────────────────────────────────");
    println!("\n[4] 结果:");
    println!("    状态: {:?}", result.status);
    for step in &result.steps {
        if let Some(output) = &step.output {
            if let Some(answer) = output.get("answer").and_then(|v| v.as_str()) {
                println!("\n    完整答案:\n    {}", answer);
            }
        }
    }
    println!("\n    耗时: {}ms", result.metrics.total_duration_ms);
    println!("\n==========================================");
    println!("  示例完成!");
    println!("==========================================");

    Ok(())
}
