//! 基础示例 - 步骤依赖和数据流
//!
//! 这个示例展示如何：
//! - 加载包含依赖关系的工作流
//! - DAG 自动并行调度（无依赖的步骤并行）
//! - 通过模板表达式在步骤间传递数据
//! - 查看执行批次（并行 vs 串行）

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
    println!("  flow-run - 基础示例：步骤依赖和数据流");
    println!("==========================================\n");

    let workflow_path = Path::new("examples/03_basic_dependencies.yaml");
    println!("[1] 从文件加载工作流: {:?}", workflow_path);
    let workflow = WorkflowParser::from_file(workflow_path)?;
    println!("    ✅ 加载成功!\n");

    println!("[2] 工作流信息:");
    println!("    名称: {}", workflow.name);
    println!("    描述: {}", workflow.description.as_deref().unwrap_or("无"));
    println!("    步骤数: {}\n", workflow.steps.len());

    println!("[3] 步骤依赖关系:");
    let dag = DagScheduler::new(workflow.steps.clone())?;
    for step in &workflow.steps {
        let deps = step.depends_on.as_ref()
            .map(|d| d.join(", "))
            .unwrap_or_else(|| "无".to_string());
        println!("    - {} ({:?}) -> [{}]", step.id, step.r#type, deps);
    }

    let batches = dag.topological_sort()?;
    println!("\n    执行批次:");
    for (i, batch) in batches.iter().enumerate() {
        let count = batch.len();
        let parallel_hint = if count > 1 { " (可并行)" } else { "" };
        println!("    批次 {}: {:?}{}", i + 1, batch, parallel_hint);
    }
    println!();

    println!("[4] 输入参数:");
    let source_url = "https://jsonplaceholder.typicode.com/posts/1";
    let target_path = "/tmp/output/data.json";
    println!("    source_url: {}", source_url);
    println!("    target_path: {}\n", target_path);

    let mut inputs = HashMap::new();
    inputs.insert("source_url".to_string(), serde_json::json!(source_url));
    inputs.insert("target_path".to_string(), serde_json::json!(target_path));

    println!("[5] 创建执行上下文");
    let context = ExecutionContext::new(&workflow, inputs);
    println!("    执行 ID: {}\n", context.execution_id);

    println!("[6] 创建 Scheduler");
    let temp_dir = tempdir()?;
    let checkpoint_manager = CheckpointManager::new(temp_dir.path().to_path_buf())?;
    let config = workflow.config.clone().unwrap_or_default();
    let scheduler = Scheduler::new(dag, config, checkpoint_manager);
    scheduler.set_context(context).await;
    if let Some(outputs) = &workflow.outputs {
        let outputs_map: HashMap<String, String> = outputs
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        scheduler.set_outputs(outputs_map).await;
    }
    println!("    ✅ Scheduler 创建成功\n");

    println!("[7] 执行工作流...");
    let result = scheduler.run().await?;

    println!("[8] 执行结果:");
    println!("    状态: {:?}", result.status);
    println!("    步骤结果:");
    for step in &result.steps {
        println!("      - {}: {:?}", step.step_id, step.status);
        if let Some(output) = &step.output {
            if let Some(stdout) = output.get("stdout").and_then(|v| v.as_str()) {
                if !stdout.is_empty() {
                    println!("        stdout: {}", stdout.replace('\n', " ").trim());
                }
            }
        }
    }
    println!();

    println!("[9] 执行指标:");
    println!("    总步骤: {}", result.metrics.total_steps);
    println!("    成功: {}", result.metrics.success_steps);
    println!("    失败: {}", result.metrics.failed_steps);
    println!("    耗时: {}ms\n", result.metrics.total_duration_ms);

    if let Some(outputs) = &result.outputs {
        println!("[10] 工作流输出:");
        for (key, value) in outputs {
            println!("    {}: {}", key, value);
        }
        println!();
    }

    println!("==========================================");
    println!("  示例完成!");
    println!("==========================================");

    Ok(())
}
