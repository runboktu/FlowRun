//! 高级示例 - 子工作流组合
//!
//! 这个示例展示如何：
//! - 加载包含子工作流的工作流
//! - 使用 Scheduler 执行工作流
//! - 观察子工作流的执行和参数传递

use flow_run::core::context::ExecutionContext;
use flow_run::core::dag::{DagScheduler, Scheduler};
use flow_run::agent::BuiltinToolRegistry;
use flow_run::core::parser::WorkflowParser;
use flow_run::executors::workflow::{WorkflowExecutor, WorkflowRunner};
use flow_run::utils::checkpoint::CheckpointManager;
use flow_run::utils::error::WorkflowError;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;

/// 实际的工作流运行器实现
struct RealWorkflowRunner;

#[async_trait::async_trait]
impl WorkflowRunner for RealWorkflowRunner {
    async fn run_workflow(
        &self,
        workflow_path: &str,
        inputs: HashMap<String, serde_json::Value>,
        _timeout: Option<Duration>,
    ) -> Result<flow_run::core::types::WorkflowResult, WorkflowError> {
        // 解析子工作流
        let workflow = WorkflowParser::from_file(workflow_path)?;

        // 创建执行上下文
        let context = ExecutionContext::new(&workflow, inputs);

        // 创建 DAG 调度器
        let dag = DagScheduler::new(workflow.steps.clone())?;

        // 创建检查点管理器
        let temp_dir = tempdir().map_err(|e| WorkflowError::Other(e.to_string()))?;
        let checkpoint_manager = CheckpointManager::new(temp_dir.path().to_path_buf())?;

        // 创建子工作流的 WorkflowRunner（递归支持）
        let sub_runner = Arc::new(RealWorkflowRunner);
        let workflow_executor = Arc::new(WorkflowExecutor::new(sub_runner));

        // 创建调度器
        let config = workflow.config.unwrap_or_default();
        let scheduler = Scheduler::with_workflow_executor(dag, config, checkpoint_manager, workflow_executor, Arc::new(BuiltinToolRegistry::with_defaults()));
        scheduler.set_context(context).await;
        if let Some(outputs) = &workflow.outputs {
            let outputs_map: HashMap<String, String> = outputs
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            scheduler.set_outputs(outputs_map).await;
        }

        scheduler.run().await
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("flow_run=info")
        .init();

    println!("==========================================");
    println!("  flow-run - 高级示例：子工作流组合");
    println!("==========================================\n");

    // 1. 从文件加载工作流
    let workflow_path = Path::new("examples/09_advanced_subworkflow.yaml");
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
        if let Some(sub_workflow) = &step.workflow {
            println!("      子工作流: {}", sub_workflow);
        }
    }
    println!();

    // 4. 创建 DAG 调度器并分析依赖
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

    // 5. 创建输入参数
    println!("[5] 输入参数:");
    let project_name = "my-awesome-app";
    let deploy_env = "staging";
    println!("    project_name: {}", project_name);
    println!("    deploy_env: {}\n", deploy_env);

    let mut inputs = HashMap::new();
    inputs.insert("project_name".to_string(), serde_json::json!(project_name));
    inputs.insert("deploy_env".to_string(), serde_json::json!(deploy_env));

    // 6. 创建执行上下文
    println!("[6] 创建执行上下文");
    let context = ExecutionContext::new(&workflow, inputs);
    println!("    执行 ID: {}\n", context.execution_id);

    // 7. 创建 Scheduler（带 WorkflowRunner）
    println!("[7] 创建 Scheduler");
    let temp_dir = tempdir()?;
    let checkpoint_manager = CheckpointManager::new(temp_dir.path().to_path_buf())?;
    let config = workflow.config.clone().unwrap_or_default();

    // 创建实际的 WorkflowRunner
    let runner = Arc::new(RealWorkflowRunner);
    let workflow_executor = Arc::new(WorkflowExecutor::new(runner));

    let scheduler = Scheduler::with_workflow_executor(dag, config, checkpoint_manager, workflow_executor, Arc::new(BuiltinToolRegistry::with_defaults()));
    scheduler.set_context(context).await;
    if let Some(outputs) = &workflow.outputs {
        let outputs_map: HashMap<String, String> = outputs
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        scheduler.set_outputs(outputs_map).await;
    }
    println!("    ✅ Scheduler 创建成功\n");

    // 8. 使用 Scheduler 执行工作流
    println!("[8] 执行工作流...");
    let result = scheduler.run().await?;

    // 9. 显示执行结果
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
        }
    }
    println!();

    // 10. 显示执行指标
    println!("[10] 执行指标:");
    println!("    总步骤: {}", result.metrics.total_steps);
    println!("    成功: {}", result.metrics.success_steps);
    println!("    失败: {}", result.metrics.failed_steps);
    println!("    耗时: {}ms\n", result.metrics.total_duration_ms);

    // 11. 显示输出
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
