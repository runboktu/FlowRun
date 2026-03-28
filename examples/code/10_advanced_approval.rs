//! 高级示例 - 加载运行人工审批工作流
//!
//! 这个示例展示如何：
//! - 加载包含审批步骤的工作流
//! - 传入风险等级等输入参数
//! - 低风险自动通过审批
//! - 高风险时通过共享存储模拟人工审批
//! - 创建 DAG 调度器并执行

use flow_run::core::context::ExecutionContext;
use flow_run::core::dag::{DagScheduler, Scheduler};
use flow_run::core::parser::WorkflowParser;
use flow_run::core::types::ApprovalStatus;
use flow_run::executors::approve::{ApproveExecutor, ApprovalStore, InMemoryApprovalStore};
use flow_run::utils::checkpoint::CheckpointManager;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("flow_run=info")
        .init();

    println!("==========================================");
    println!("  flow-run - 高级示例：人工审批流程");
    println!("==========================================\n");

    let workflow_path = Path::new("examples/10_advanced_approval.yaml");
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
    let change_request_id = "CR-2024-042";
    let risk_level = "high";
    println!("    change_request_id: {}", change_request_id);
    println!("    risk_level: {} (高风险，需要人工审批)\n", risk_level);

    let mut inputs = HashMap::new();
    inputs.insert("change_request_id".to_string(), serde_json::json!(change_request_id));
    inputs.insert("risk_level".to_string(), serde_json::json!(risk_level));

    println!("[5] 创建执行上下文");
    let context = ExecutionContext::new(&workflow, inputs);
    println!("    执行 ID: {}\n", context.execution_id);

    println!("[6] 创建 DAG 调度器");
    let dag = DagScheduler::new(workflow.steps.clone())?;
    println!("    ✅ DAG 创建成功 ({} 个步骤)\n", workflow.steps.len());

    // 创建共享审批存储，用于模拟人工审批
    let approval_store = Arc::new(InMemoryApprovalStore::new());
    let approve_executor = ApproveExecutor::with_components(
        approval_store.clone(),
        Arc::new(flow_run::executors::approve::SimpleApprovalNotifier),
    );

    println!("[7] 创建 Scheduler");
    let temp_dir = tempdir()?;
    let checkpoint_manager = CheckpointManager::new(temp_dir.path().to_path_buf())?;
    let workflow_config = workflow.config.clone().unwrap_or_default();
    let mut scheduler = Scheduler::new(dag, workflow_config, checkpoint_manager);
    scheduler.set_context(context).await;
    scheduler.set_approve_executor(approve_executor);
    if let Some(outputs) = &workflow.outputs {
        let outputs_map: HashMap<String, String> = outputs
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        scheduler.set_outputs(outputs_map).await;
    }
    println!("    ✅ Scheduler 创建成功\n");

    // 启动异步任务：2 秒后模拟人工批准
    let store_clone = approval_store.clone();
    let step_id = "approval_gate".to_string();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(2)).await;
        store_clone
            .save_approval(
                &step_id,
                ApprovalStatus::Approved,
                Some("admin@example.com".to_string()),
                Some("审批通过：变更合理，同意执行".to_string()),
            )
            .await
            .ok();
    });

    println!("[8] 执行工作流...");
    println!("    -> 审批流程启动（高风险，等待人工审批，2 秒后自动批准）...\n");
    let result = scheduler.run().await?;

    println!("[9] 执行结果:");
    println!("    状态：{:?}", result.status);
    println!("    步骤结果:");
    for step in &result.steps {
        println!("      - {}: {:?}", step.step_id, step.status);
        if let Some(output) = &step.output {
            let output_str = serde_json::to_string(output)?;
            if output_str.len() > 100 {
                let end = output_str.char_indices()
                    .map(|(i, _)| i)
                    .nth(100)
                    .unwrap_or(output_str.len());
                println!("        输出：{}...", &output_str[..end]);
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
