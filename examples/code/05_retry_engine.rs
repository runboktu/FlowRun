//! 中级示例 - 错误处理与重试
//!
//! 这个示例展示如何：
//! - 加载带重试配置的工作流
//! - 使用 Scheduler 执行工作流
//! - 观察重试机制和错误处理

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
    println!("  flow-run - 中级示例：错误处理与重试");
    println!("==========================================\n");

    // 1. 从文件加载工作流
    let workflow_path = Path::new("examples/05_intermediate_retry.yaml");
    println!("[1] 从文件加载工作流: {:?}", workflow_path);
    let workflow = WorkflowParser::from_file(workflow_path)?;
    println!("    ✅ 加载成功!\n");

    // 2. 显示工作流信息
    println!("[2] 工作流信息:");
    println!("    名称: {}", workflow.name);
    println!("    描述: {}", workflow.description.as_deref().unwrap_or("无"));
    println!("    步骤数: {}\n", workflow.steps.len());

    // 3. 显示步骤配置
    println!("[3] 步骤配置:");
    for step in &workflow.steps {
        println!("    - {} ({})", step.id, serde_json::to_string(&step.r#type).unwrap_or_default());
        if let Some(retry) = &step.retry {
            println!("      重试: {} 次, 策略: {:?}", retry.max_attempts, retry.strategy);
        }
        if let Some(timeout) = &step.timeout {
            println!("      超时: {}", timeout);
        }
    }
    println!();

    // 4. 创建 DAG 调度器
    println!("[4] DAG 分析:");
    let dag = DagScheduler::new(workflow.steps.clone())?;
    match dag.has_cycle() {
        Ok(false) => println!("    ✅ 无循环依赖"),
        Ok(true) => {
            println!("    ❌ 检测到循环依赖!");
            return Err(anyhow::anyhow!("工作流包含循环依赖"));
        }
        Err(e) => {
            println!("    ❌ 检查失败: {}", e);
            return Err(e.into());
        }
    }
    println!();

    // 5. 创建执行上下文
    println!("[5] 创建执行上下文");
    let context = ExecutionContext::new(&workflow, HashMap::new());
    println!("    执行 ID: {}\n", context.execution_id);

    // 6. 创建 Scheduler 并设置上下文
    println!("[6] 创建 Scheduler");
    let temp_dir = tempdir()?;
    let checkpoint_manager = CheckpointManager::new(temp_dir.path().to_path_buf())?;
    let config = workflow.config.clone().unwrap_or_default();
    let scheduler = Scheduler::new(dag, config, checkpoint_manager);
    scheduler.set_context(context).await;
    println!("    ✅ Scheduler 创建成功\n");

    // 7. 使用 Scheduler 执行工作流
    println!("[7] 执行工作流...");
    let result = scheduler.run().await?;

    // 8. 显示执行结果
    println!("[8] 执行结果:");
    println!("    状态: {:?}", result.status);
    println!("    步骤结果:");
    for step in &result.steps {
        println!("      - {}: {:?}", step.step_id, step.status);
        if let Some(output) = &step.output {
            if let Some(stdout) = output.get("stdout").and_then(|v| v.as_str()) {
                if !stdout.is_empty() {
                    println!("        stdout: {}", stdout.trim());
                }
            }
            if let Some(body) = output.get("response").and_then(|r| r.get("body")) {
                if let Some(title) = body.get("title").and_then(|t| t.as_str()) {
                    println!("        title: {}", title);
                }
            }
        }
        if let Some(error) = &step.error {
            println!("        错误: {}", error.message);
        }
    }
    println!();

    // 9. 显示执行指标
    println!("[9] 执行指标:");
    println!("    总步骤: {}", result.metrics.total_steps);
    println!("    成功: {}", result.metrics.success_steps);
    println!("    失败: {}", result.metrics.failed_steps);
    println!("    耗时: {}ms\n", result.metrics.total_duration_ms);

    // 10. 显示输出
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
