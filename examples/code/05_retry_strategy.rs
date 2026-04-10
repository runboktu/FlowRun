//! 中级示例 - 错误处理与重试策略
//!
//! 这个示例展示如何：
//! - 配置步骤级重试机制
//! - 使用不同的退避策略（fixed / exponential）
//! - 期望结果验证（expect）
//! - 失败后继续执行后续步骤

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
    println!("  flow-run - 中级示例：错误处理与重试策略");
    println!("==========================================\n");

    let workflow_path = Path::new("examples/05_intermediate_retry.yaml");
    println!("[1] 从文件加载工作流: {:?}", workflow_path);
    let workflow = WorkflowParser::from_file(workflow_path)?;
    println!("    ✅ 加载成功!\n");

    println!("[2] 工作流信息:");
    println!("    名称: {}", workflow.name);
    println!("    描述: {}", workflow.description.as_deref().unwrap_or("无"));
    println!("    步骤数: {}\n", workflow.steps.len());

    println!("[3] 重试配置:");
    for step in &workflow.steps {
        if let Some(retry) = &step.retry {
            let strategy = retry.strategy.as_ref()
                .map(|s| format!("{:?}", s))
                .unwrap_or_else(|| "无".to_string());
            let expect = step.expect.as_ref().map(|e| {
                format!("status_code={:?}", e.status_code)
            });
            let expect_str = expect.as_deref().unwrap_or("无");
            println!("    - {}: max_attempts={}, strategy={}, expect={}",
                step.id, retry.max_attempts, strategy, expect_str);
        } else {
            println!("    - {}: 无重试配置", step.id);
        }
    }
    println!();

    println!("[4] 创建 Scheduler");
    let dag = DagScheduler::new(workflow.steps.clone())?;
    let temp_dir = tempdir()?;
    let checkpoint_manager = CheckpointManager::new(temp_dir.path().to_path_buf())?;
    let config = workflow.config.clone().unwrap_or_default();
    println!("    config.timeout: {:?}\n", config.timeout);
    let scheduler = Scheduler::new(dag, config, checkpoint_manager, Arc::new(BuiltinToolRegistry::with_defaults()));

    let mut inputs = HashMap::new();
    inputs.insert("project_name".to_string(), serde_json::json!("test-app"));
    let context = ExecutionContext::new(&workflow, inputs);
    scheduler.set_context(context).await;
    if let Some(outputs) = &workflow.outputs {
        let outputs_map: HashMap<String, String> = outputs
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        scheduler.set_outputs(outputs_map).await;
    }
    println!("    ✅ Scheduler 创建成功\n");

    println!("[5] 执行工作流...");
    println!("    -> fetch_with_retry 会请求 404 接口并重试...\n");
    let result = scheduler.run().await?;

    println!("[6] 执行结果:");
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
            if let Some(error) = &step.error {
                println!("        错误: [{}] {}", error.code, error.message);
            }
        }
    }
    println!();

    println!("[7] 执行指标:");
    println!("    总步骤: {}", result.metrics.total_steps);
    println!("    成功: {}", result.metrics.success_steps);
    println!("    失败: {}", result.metrics.failed_steps);
    println!("    耗时: {}ms\n", result.metrics.total_duration_ms);

    if let Some(outputs) = &result.outputs {
        println!("[8] 工作流输出:");
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
