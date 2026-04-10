//! 基础示例 - 加载并执行 YAML 工作流
//!
//! 这个示例展示如何：
//! - 从文件加载工作流定义
//! - 创建执行上下文并传入参数
//! - 使用 Scheduler 真正执行工作流
//! - 查看执行结果

use flow_run::core::context::ExecutionContext;
use flow_run::core::dag::{DagScheduler, Scheduler};
use flow_run::agent::BuiltinToolRegistry;
use std::sync::Arc;
use flow_run::core::parser::WorkflowParser;
use flow_run::utils::checkpoint::CheckpointManager;
use std::collections::HashMap;
use std::path::Path;
use tempfile::tempdir;
use flow_run::FlowRunner;

// FlowRunner 版本
// #[tokio::main]
// async fn main() -> anyhow::Result<()> {
//     tracing_subscriber::fmt()
//         .with_env_filter("flow_run=info")
//         .init();
//
//     println!("==========================================");
//     println!("  flow-run - 基础示例：加载并执行工作流");
//     println!("==========================================\n");
//
//     // 1. 创建 FlowRunner（从文件加载）
//     println!("[1] 从文件创建工作流运行器");
//     let runner = FlowRunner::from_file("examples/01_basic_http.yaml")?;
//     println!("    ✅ 加载成功!\n");
//
//     // 2. 显示工作流信息
//     println!("[2] 工作流信息:");
//     println!("    名称: {}", runner.workflow().name);
//     println!("    步骤数: {}\n", runner.workflow().steps.len());
//
//     // 3. 创建输入参数
//     let api_url = "https://jsonplaceholder.typicode.com";
//     println!("[3] 输入参数:");
//     println!("    api_url: {}\n", api_url);
//
//     let mut inputs = HashMap::new();
//     inputs.insert("api_url".to_string(), serde_json::json!(api_url));
//
//     // 4. 执行工作流（一行搞定！）
//     println!("[4] 执行工作流...");
//     println!("    -> GET {}/users/1\n", api_url);
//     let result = runner.run(inputs).await?;
//
//     // 5. 显示执行结果
//     println!("[5] 执行结果:");
//     println!("    状态: {:?}", result.status);
//     println!("    步骤结果:");
//     for step in &result.steps {
//         println!("      - {}: {:?} {:?}", step.step_id, step.status, step.output);
//     }
//     println!();
//
//     println!("==========================================");
//     println!("  示例完成!");
//     println!("==========================================");
//
//     Ok(())
// }


#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("flow_run=info")
        .init();

    println!("==========================================");
    println!("  flow-run - 基础示例：加载并执行工作流");
    println!("==========================================\n");

    // 1. 从文件加载工作流
    let workflow_path = Path::new("examples/01_basic_http.yaml");
    println!("[1] 从文件加载工作流: {:?}", workflow_path);
    let workflow = WorkflowParser::from_file(workflow_path)?;
    println!("    ✅ 加载成功!\n");

    // 2. 显示工作流信息
    println!("[2] 工作流信息:");
    println!("    名称: {}", workflow.name);
    println!("    步骤数: {}\n", workflow.steps.len());

    // 3. 创建输入参数
    let api_url = "https://jsonplaceholder.typicode.com";
    println!("[3] 输入参数:");
    println!("    api_url: {}\n", api_url);

    let mut inputs = HashMap::new();
    inputs.insert("api_url".to_string(), serde_json::json!(api_url));

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
    let scheduler = Scheduler::new(dag, config, checkpoint_manager, Arc::new(BuiltinToolRegistry::with_defaults()));
    scheduler.set_context(context).await;
    println!("    ✅ Scheduler 创建成功\n");

    // 7. 使用 Scheduler 执行工作流
    println!("[7] 执行工作流...");
    println!("    -> GET {}/users/1\n", api_url);
    let result = scheduler.run().await?;

    // 8. 显示执行结果
    println!("[8] 执行结果:");
    println!("    状态: {:?}", result.status);
    println!("    步骤结果:");
    for step in &result.steps {
        println!("      - {}: {:?}", step.step_id, step.status);
        if let Some(output) = &step.output {
            let output_str = serde_json::to_string(output)?;
            if output_str.len() > 100 {
                println!("        输出: {}...", &output_str[..100]);
            } else {
                println!("        输出: {}", output_str);
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
