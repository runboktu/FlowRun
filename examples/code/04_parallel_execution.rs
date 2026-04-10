//! 中级示例 - 并行执行与限流
//!
//! 这个示例展示如何：
//! - 使用 parallel 类型并行执行多个子步骤
//! - 通过 max_concurrent 控制并发数
//! - 在并行步骤完成后聚合结果

use flow_run::core::context::ExecutionContext;
use flow_run::core::dag::{DagScheduler, Scheduler};
use flow_run::agent::BuiltinToolRegistry;
use std::sync::Arc;
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
    println!("  flow-run - 中级示例：并行执行与限流");
    println!("==========================================\n");

    let workflow_path = Path::new("examples/04_intermediate_parallel.yaml");
    println!("[1] 从文件加载工作流: {:?}", workflow_path);
    let workflow = WorkflowParser::from_file(workflow_path)?;
    println!("    ✅ 加载成功!\n");

    println!("[2] 工作流信息:");
    println!("    名称: {}", workflow.name);
    println!("    描述: {}", workflow.description.as_deref().unwrap_or("无"));
    println!("    步骤数: {}\n", workflow.steps.len());

    println!("[3] 步骤配置:");
    for step in &workflow.steps {
        let max_concurrent = step.max_concurrent
            .map(|m| format!(" (max_concurrent: {})", m))
            .unwrap_or_default();
        let sub_count = step.steps.as_ref()
            .map(|s| s.len())
            .unwrap_or(0);
        if matches!(step.r#type, flow_run::core::types::StepType::Parallel) {
            println!("    - {} (parallel{}) - {} 个子步骤", step.id, max_concurrent, sub_count);
        } else {
            println!("    - {} ({:?})", step.id, step.r#type);
        }
    }
    println!();

    println!("[4] 输入参数:");
    let api_endpoints = "https://jsonplaceholder.typicode.com/posts/1,https://jsonplaceholder.typicode.com/posts/2,https://jsonplaceholder.typicode.com/posts/3";
    println!("    api_endpoints: {}\n", api_endpoints);

    let mut inputs = HashMap::new();
    inputs.insert("api_endpoints".to_string(), serde_json::json!(api_endpoints));

    println!("[5] 创建执行上下文");
    let context = ExecutionContext::new(&workflow, inputs);
    println!("    执行 ID: {}\n", context.execution_id);

    println!("[6] 创建 DAG 调度器");
    let dag = DagScheduler::new(workflow.steps.clone())?;
    let batches = dag.topological_sort()?;
    println!("    ✅ DAG 创建成功 ({} 个步骤, {} 批次)\n", workflow.steps.len(), batches.len());

    println!("[7] 创建 Scheduler");
    let temp_dir = tempdir()?;
    let checkpoint_manager = CheckpointManager::new(temp_dir.path().to_path_buf())?;
    let config = workflow.config.clone().unwrap_or_default();
    let scheduler = Scheduler::new(dag, config, checkpoint_manager, Arc::new(BuiltinToolRegistry::with_defaults()));
    scheduler.set_context(context).await;
    if let Some(outputs) = &workflow.outputs {
        let outputs_map: HashMap<String, String> = outputs
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        scheduler.set_outputs(outputs_map).await;
    }
    println!("    ✅ Scheduler 创建成功\n");

    println!("[8] 执行工作流...");
    let result = scheduler.run().await?;

    println!("[9] 执行结果:");
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
            if let Some(results) = output.get("results") {
                println!("        results: {:?}", results);
            }
        }
    }
    println!();

    println!("[10] 执行指标:");
    println!("    总步骤: {}", result.metrics.total_steps);
    println!("    成功: {}", result.metrics.success_steps);
    println!("    失败: {}", result.metrics.failed_steps);
    println!("    耗时: {}ms\n", result.metrics.total_duration_ms);

    if let Some(outputs) = &result.outputs {
        println!("[11] 工作流输出:");
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
