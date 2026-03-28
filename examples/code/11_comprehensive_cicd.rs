//! 综合示例 - 加载运行完整 CI/CD 工作流
//!
//! 这个示例展示如何：
//! - 加载复杂的 CI/CD 工作流定义
//! - 传入多个输入参数（仓库地址、分支、部署环境等）
//! - 创建 DAG 调度器并执行
//! - 查看执行结果和输出

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
    println!("  flow-run - 综合示例：完整 CI/CD 流水线");
    println!("==========================================\n");

    let workflow_path = Path::new("examples/11_comprehensive_cicd.yaml");
    println!("[1] 从文件加载工作流：{:?}", workflow_path);
    let workflow = WorkflowParser::from_file(workflow_path)?;
    println!("    ✅ 加载成功!\n");

    println!("[2] 工作流信息:");
    println!("    名称：{}", workflow.name);
    if let Some(desc) = &workflow.description {
        println!("    描述：{}", desc);
    }
    if let Some(version) = &workflow.version {
        println!("    版本：{}", version);
    }
    println!("    步骤数：{}\n", workflow.steps.len());

    if let Some(inputs_def) = &workflow.inputs {
        if !inputs_def.is_empty() {
            println!("[3] 输入参数定义:");
            for input in inputs_def {
                let required = if input.required.unwrap_or(false) { "必需" } else { "可选" };
                let input_type = input.r#type.as_deref().unwrap_or("any");
                println!("    - {}: {} ({})", input.name, input_type, required);
                if let Some(desc) = &input.description {
                    println!("      {}", desc);
                }
            }
            println!();
        }
    }

    println!("[4] 输入参数值:");
    let repo_url = "https://github.com/example/myapp.git";
    let branch = "main";
    let deploy_env = "staging";
    let skip_tests = false;
    println!("    repo_url: {}", repo_url);
    println!("    branch: {}", branch);
    println!("    deploy_env: {}", deploy_env);
    println!("    skip_tests: {}\n", skip_tests);

    let mut inputs = HashMap::new();
    inputs.insert("repo_url".to_string(), serde_json::json!(repo_url));
    inputs.insert("branch".to_string(), serde_json::json!(branch));
    inputs.insert("deploy_env".to_string(), serde_json::json!(deploy_env));
    inputs.insert("skip_tests".to_string(), serde_json::json!(skip_tests));

    println!("[5] 创建执行上下文");
    let context = ExecutionContext::new(&workflow, inputs);
    println!("    执行 ID: {}\n", context.execution_id);

    println!("[6] 创建 DAG 调度器");
    let dag = DagScheduler::new(workflow.steps.clone())?;
    println!("    ✅ DAG 创建成功 ({} 个步骤)\n", workflow.steps.len());

    println!("[7] 创建 Scheduler");
    let temp_dir = tempdir()?;
    let checkpoint_manager = CheckpointManager::new(temp_dir.path().to_path_buf())?;
    let workflow_config = workflow.config.clone().unwrap_or_default();
    let scheduler = Scheduler::new(dag, workflow_config, checkpoint_manager);
    scheduler.set_context(context).await;
    println!("    ✅ Scheduler 创建成功\n");

    println!("[8] 执行工作流...");
    println!("    -> CI/CD 流水线启动...\n");
    let result = scheduler.run().await?;

    println!("[9] 执行结果:");
    println!("    状态：{:?}", result.status);
    println!("    步骤结果:");
    for step in &result.steps {
        println!("      - {}: {:?}", step.step_id, step.status);
        if let Some(output) = &step.output {
            let output_str = serde_json::to_string(output)?;
            if output_str.len() > 100 {
                println!("        输出：{}...", &output_str[..100]);
            } else {
                println!("        输出：{}", output_str);
            }
        }
    }
    println!();

    println!("[10] 执行指标:");
    println!("    总步骤：{}", result.metrics.total_steps);
    println!("    成功：{}", result.metrics.success_steps);
    println!("    失败：{}", result.metrics.failed_steps);
    println!("    跳过：{}", result.metrics.skipped_steps);
    println!("    耗时：{}ms\n", result.metrics.total_duration_ms);

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
