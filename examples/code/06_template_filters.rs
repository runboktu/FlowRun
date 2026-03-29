//! 中级示例 - 模板表达式与过滤器
//!
//! 这个示例展示如何：
//! - 使用 ${{ }} 模板语法引用变量
//! - 使用内置过滤器（uppercase, lowercase, truncate 等）
//! - 使用条件默认值（|| 操作符）
//! - 路径访问和数组索引

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
    println!("  flow-run - 中级示例：模板表达式与过滤器");
    println!("==========================================\n");

    let workflow_path = Path::new("examples/06_intermediate_templates.yaml");
    println!("[1] 从文件加载工作流: {:?}", workflow_path);
    let workflow = WorkflowParser::from_file(workflow_path)?;
    println!("    ✅ 加载成功!\n");

    println!("[2] 工作流信息:");
    println!("    名称: {}", workflow.name);
    println!("    描述: {}", workflow.description.as_deref().unwrap_or("无"));
    println!("    步骤数: {}\n", workflow.steps.len());

    println!("[3] 模板表达式预览:");
    for step in &workflow.steps {
        if let Some(run) = &step.run {
            let template_count = run.matches("${{").count();
            if template_count > 0 {
                println!("    - {} ({} 个模板引用)", step.id, template_count);
            } else {
                println!("    - {} (无模板引用)", step.id);
            }
        }
    }
    println!();

    println!("[4] 输入参数:");
    let username = "Samantha";
    println!("    username: {}", username);
    println!("    environment: (使用默认值 staging)\n");

    let mut inputs = HashMap::new();
    inputs.insert("username".to_string(), serde_json::json!(username));

    println!("[5] 创建执行上下文");
    let context = ExecutionContext::new(&workflow, inputs);
    println!("    执行 ID: {}\n", context.execution_id);

    println!("[6] 创建 DAG 调度器");
    let dag = DagScheduler::new(workflow.steps.clone())?;
    let batches = dag.topological_sort()?;
    println!("    ✅ DAG 创建成功 ({} 个步骤, {} 个批次)\n", workflow.steps.len(), batches.len());

    println!("[7] 创建 Scheduler");
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
            if let Some(branch) = output.get("branch").and_then(|v| v.as_str()) {
                println!("        branch: {}", branch);
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
