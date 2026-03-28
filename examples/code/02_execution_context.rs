//! 基础示例 - Shell 命令执行
//!
//! 这个示例展示如何：
//! - 加载 Shell 命令工作流
//! - 传入 project_name 和 environment 参数
//! - 使用 Scheduler 执行工作流
//! - 查看执行结果

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
    println!("  flow-run - 基础示例：Shell 命令执行");
    println!("==========================================\n");

    // 1. 从文件加载工作流
    let workflow_path = Path::new("examples/02_basic_shell.yaml");
    println!("[1] 从文件加载工作流: {:?}", workflow_path);
    let workflow = WorkflowParser::from_file(workflow_path)?;
    println!("    ✅ 加载成功!\n");

    // 2. 显示工作流信息
    println!("[2] 工作流信息:");
    println!("    名称: {}", workflow.name);
    println!("    描述: {}", workflow.description.as_deref().unwrap_or("无"));
    println!("    步骤数: {}\n", workflow.steps.len());

    // 3. 创建输入参数
    let project_name = "demo-project";
    let environment = "production";
    println!("[3] 输入参数:");
    println!("    project_name: {}", project_name);
    println!("    environment: {}\n", environment);

    let mut inputs = HashMap::new();
    inputs.insert("project_name".to_string(), serde_json::json!(project_name));
    inputs.insert("environment".to_string(), serde_json::json!(environment));

    // 4. 创建执行上下文
    println!("[4] 创建执行上下文");
    let context = ExecutionContext::new(&workflow, inputs);
    println!("    执行 ID: {}\n", context.execution_id);

    // 5. 创建 DAG 调度器
    println!("[5] 创建 DAG 调度器");
    let dag = DagScheduler::new(workflow.steps.clone())?;
    println!("    ✅ DAG 创建成功 ({} 个步骤)\n", workflow.steps.len());

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
    println!("    -> mkdir -p /tmp/{}", project_name);
    println!("    -> 创建配置文件");
    println!("    -> 验证配置文件\n");
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
                    let display = if stdout.len() > 80 {
                        format!("{}...", &stdout[..80])
                    } else {
                        stdout.to_string()
                    };
                    println!("        stdout: {}", display.replace('\n', " "));
                }
            }
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
