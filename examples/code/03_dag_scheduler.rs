//! 基础示例 - 步骤依赖和数据流
//!
//! 这个示例展示如何：
//! - 加载具有依赖关系的工作流
//! - 查看 DAG 拓扑排序结果
//! - 使用 Scheduler 执行工作流
//! - 观察并行执行的批次

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

    // 1. 从文件加载工作流
    let workflow_path = Path::new("examples/03_basic_dependencies.yaml");
    println!("[1] 从文件加载工作流: {:?}", workflow_path);
    let workflow = WorkflowParser::from_file(workflow_path)?;
    println!("    ✅ 加载成功!\n");

    // 2. 显示工作流信息
    println!("[2] 工作流信息:");
    println!("    名称: {}", workflow.name);
    println!("    描述: {}", workflow.description.as_deref().unwrap_or("无"));
    println!("    步骤数: {}\n", workflow.steps.len());

    // 3. 创建 DAG 调度器并分析依赖
    println!("[3] DAG 分析:");
    let dag = DagScheduler::new(workflow.steps.clone())?;

    // 检查循环依赖
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

    // 显示依赖关系
    println!("\n    步骤依赖关系:");
    for step in &workflow.steps {
        let deps = step.depends_on.as_ref()
            .map(|d| d.join(", "))
            .unwrap_or_else(|| "无".to_string());
        println!("      {} -> [{}]", step.id, deps);
    }

    // 执行拓扑排序
    let batches = dag.topological_sort()?;
    println!("\n    执行批次:");
    for (i, batch) in batches.iter().enumerate() {
        println!("      批次 {}: {:?}", i + 1, batch);
    }
    println!();

    // 4. 创建输入参数
    let source_url = "https://jsonplaceholder.typicode.com/posts/1";
    let target_path = "/tmp/flow-run-output/result.txt";
    println!("[4] 输入参数:");
    println!("    source_url: {}", source_url);
    println!("    target_path: {}\n", target_path);

    let mut inputs = HashMap::new();
    inputs.insert("source_url".to_string(), serde_json::json!(source_url));
    inputs.insert("target_path".to_string(), serde_json::json!(target_path));

    // 5. 创建执行上下文
    println!("[5] 创建执行上下文");
    let context = ExecutionContext::new(&workflow, inputs);
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
    println!("    批次 1: fetch_data + prepare_env (并行)");
    println!("    批次 2: process_data");
    println!("    批次 3: save_result\n");
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
                    let display = if stdout.len() > 60 {
                        format!("{}...", &stdout[..60])
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
