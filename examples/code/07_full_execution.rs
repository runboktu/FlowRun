//! 综合示例 - 完整工作流执行
//!
//! 这个示例展示如何：
//! - 加载 YAML 工作流
//! - 创建执行上下文
//! - 使用 DAG 调度器执行工作流
//! - 处理执行结果

use flow_run::core::context::ExecutionContext;
use flow_run::core::dag::DagScheduler;
use flow_run::core::parser::WorkflowParser;
use flow_run::core::types::{WorkflowConfig, WorkflowResult, WorkflowStatus};
use std::collections::HashMap;
use std::path::Path;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("flow_run=info")
        .init();

    println!("==========================================");
    println!("  flow-run - 综合示例：完整工作流执行");
    println!("==========================================\n");

    // 1. 加载工作流定义
    println!("[1] 加载工作流定义");
    let workflow_path = Path::new("examples/02_basic_shell.yaml");
    let workflow = WorkflowParser::from_file(workflow_path)?;
    println!("    工作流: {}", workflow.name);
    println!("    步骤数: {}\n", workflow.steps.len());

    // 2. 创建输入参数
    println!("[2] 创建输入参数");
    let mut inputs = HashMap::new();
    inputs.insert("project_name".to_string(), serde_json::json!("demo-project"));
    inputs.insert("environment".to_string(), serde_json::json!("development"));
    for (key, value) in &inputs {
        println!("    {}: {}", key, value);
    }
    println!();

    // 3. 创建执行上下文
    println!("[3] 创建执行上下文");
    let context = ExecutionContext::new(&workflow, inputs);
    println!("    执行 ID: {}", context.execution_id);
    println!("    工作流: {}", context.workflow_name);
    println!("    开始时间: {}\n", context.started_at);

    // 4. 创建 DAG 调度器
    println!("[4] 创建 DAG 调度器");
    let dag = DagScheduler::new(workflow.steps.clone())?;

    // 检查循环依赖
    if dag.has_cycle()? {
        eprintln!("    ❌ 检测到循环依赖!");
        return Err(anyhow::anyhow!("工作流包含循环依赖"));
    }
    println!("    ✅ 无循环依赖\n");

    // 显示执行批次
    let batches = dag.topological_sort()?;
    println!("[5] 执行计划 ({} 个批次):", batches.len());
    for (i, batch) in batches.iter().enumerate() {
        println!("    批次 {}: {:?}", i + 1, batch);
    }
    println!();

    // 5. 显示工作流配置
    println!("[6] 工作流配置:");
    if let Some(config) = &workflow.config {
        if let Some(timeout) = &config.timeout {
            println!("    超时: {}", timeout);
        }
        if let Some(max_concurrent) = config.max_concurrent {
            println!("    最大并发: {}", max_concurrent);
        }
        if let Some(on_failure) = &config.on_failure {
            println!("    失败策略: {:?}", on_failure);
        }
    } else {
        println!("    使用默认配置");
    }
    println!();

    // 6. 模拟执行结果
    println!("[7] 模拟执行结果:");
    let mock_result = create_mock_result(&workflow.name, &workflow.steps);
    println!("    状态: {:?}", mock_result.status);
    println!("    步骤结果:");
    for step in &mock_result.steps {
        println!("      - {}: {:?}", step.step_id, step.status);
    }
    println!();

    // 7. 显示执行指标
    println!("[8] 执行指标:");
    println!("    总步骤: {}", mock_result.metrics.total_steps);
    println!("    成功: {}", mock_result.metrics.success_steps);
    println!("    失败: {}", mock_result.metrics.failed_steps);
    println!("    跳过: {}", mock_result.metrics.skipped_steps);
    println!("    耗时: {}ms\n", mock_result.metrics.total_duration_ms);

    println!("==========================================");
    println!("  示例完成!");
    println!("==========================================");

    Ok(())
}

fn create_mock_result(
    workflow_name: &str,
    steps: &[flow_run::core::types::StepDefinition],
) -> WorkflowResult {
    use flow_run::core::types::*;
    use chrono::Utc;

    let now = Utc::now();
    let step_results: Vec<StepResult> = steps
        .iter()
        .map(|step| StepResult {
            step_id: step.id.clone(),
            status: StepStatus::Success,
            started_at: now,
            completed_at: Some(now),
            duration_ms: Some(100),
            output: Some(serde_json::json!({"mock": true})),
            error: None,
        })
        .collect();

    WorkflowResult {
        status: WorkflowStatus::Success,
        workflow: WorkflowInfo {
            name: workflow_name.to_string(),
            version: Some("1.0.0".to_string()),
            file: "mock.yaml".to_string(),
        },
        execution: ExecutionInfo {
            id: "mock-execution".to_string(),
            started_at: now,
            completed_at: Some(now),
            duration_ms: Some(steps.len() as u64 * 100),
            checkpoint: None,
        },
        steps: step_results,
        outputs: Some(HashMap::from([
            ("status".to_string(), serde_json::json!("success")),
        ])),
        metrics: ExecutionMetrics {
            total_steps: steps.len(),
            success_steps: steps.len(),
            failed_steps: 0,
            skipped_steps: 0,
            total_duration_ms: steps.len() as u64 * 100,
        },
        errors: vec![],
    }
}
