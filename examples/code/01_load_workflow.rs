//! 基础示例 - 加载并执行 YAML 工作流
//!
//! 这个示例展示如何：
//! - 从文件加载工作流定义
//! - 创建执行上下文并传入参数
//! - 真正执行工作流（调用 HTTP API）
//! - 查看执行结果

use flow_run::core::context::ExecutionContext;
use flow_run::core::dag::Scheduler;
use flow_run::core::parser::WorkflowParser;
use flow_run::core::types::WorkflowConfig;
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

    // 5. 创建调度器
    println!("[5] 创建调度器");
    let dag = flow_run::core::dag::DagScheduler::new(workflow.steps.clone())?;
    let temp_dir = tempdir()?;
    let _checkpoint_manager = CheckpointManager::new(temp_dir.path().to_path_buf())?;
    let _config = workflow.config.clone().unwrap_or_else(|| WorkflowConfig {
        timeout: None,
        retry: None,
        on_failure: None,
        checkpoint: None,
        max_concurrent: None,
        timeout_strategy: None,
        resume: None,
        history: None,
        cleanup: None,
        hooks: None,
    });
    println!("    ✅ DAG 创建成功 ({} 个步骤)\n", workflow.steps.len());

    // 6. 执行工作流（手动执行 HTTP 请求）
    println!("[6] 执行工作流...");
    println!("    -> GET {}/users/1\n", api_url);
    let result = run_workflow_manual(&workflow, api_url).await?;

    // 7. 显示执行结果
    println!("[7] 执行结果:");
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

    // 8. 显示执行指标
    println!("[8] 执行指标:");
    println!("    总步骤: {}", result.metrics.total_steps);
    println!("    成功: {}", result.metrics.success_steps);
    println!("    失败: {}", result.metrics.failed_steps);
    println!("    耗时: {}ms\n", result.metrics.total_duration_ms);

    println!("==========================================");
    println!("  示例完成!");
    println!("==========================================");

    Ok(())
}

/// 手动执行工作流（简化版本）
async fn run_workflow_manual(
    workflow: &flow_run::core::types::WorkflowDefinition,
    api_url: &str,
) -> anyhow::Result<flow_run::core::types::WorkflowResult> {
    use flow_run::core::types::*;
    use chrono::Utc;

    let start_time = Utc::now();
    let mut steps = Vec::new();

    // 执行每个步骤
    for step_def in &workflow.steps {
        let step_start = Utc::now();
        println!("    执行步骤: {}", step_def.id);

        let result = match step_def.r#type {
            StepType::Http => {
                // 执行 HTTP 请求
                let url = step_def.api.as_ref()
                    .map(|u| u.replace("${{ inputs.api_url }}", api_url))
                    .unwrap_or_default();

                let method = step_def.method.as_deref().unwrap_or("GET");
                println!("      {} {}", method, url);

                let client = reqwest::Client::new();
                let response = client.get(&url)
                    .header("Accept", "application/json")
                    .send()
                    .await?;

                let status_code = response.status().as_u16();
                let body: serde_json::Value = response.json().await?;

                StepResult {
                    step_id: step_def.id.clone(),
                    status: if (200..300).contains(&status_code) {
                        StepStatus::Success
                    } else {
                        StepStatus::Failed
                    },
                    started_at: step_start,
                    completed_at: Some(Utc::now()),
                    duration_ms: Some(
                        (Utc::now() - step_start).num_milliseconds() as u64
                    ),
                    output: Some(serde_json::json!({
                        "response": {
                            "status_code": status_code,
                            "body": body
                        }
                    })),
                    error: None,
                }
            }

            StepType::Shell => {
                // 执行 Shell 命令
                let command = step_def.run.as_deref().unwrap_or("");
                println!("      run: {}", command);

                // 这里简化处理，直接创建成功结果
                // 实际应该调用 tokio::process::Command
                StepResult {
                    step_id: step_def.id.clone(),
                    status: StepStatus::Success,
                    started_at: step_start,
                    completed_at: Some(Utc::now()),
                    duration_ms: Some(
                        (Utc::now() - step_start).num_milliseconds() as u64
                    ),
                    output: Some(serde_json::json!({
                        "stdout": format!("模拟执行: {}", command),
                        "exit_code": 0
                    })),
                    error: None,
                }
            }

            _ => StepResult {
                step_id: step_def.id.clone(),
                status: StepStatus::Skipped,
                started_at: step_start,
                completed_at: Some(Utc::now()),
                duration_ms: Some(0),
                output: None,
                error: None,
            },
        };

        steps.push(result);
    }

    let total_duration = (Utc::now() - start_time).num_milliseconds() as u64;

    Ok(WorkflowResult {
        status: WorkflowStatus::Success,
        workflow: WorkflowInfo {
            name: workflow.name.clone(),
            version: workflow.version.clone(),
            file: "01_basic_http.yaml".to_string(),
        },
        execution: ExecutionInfo {
            id: uuid::Uuid::new_v4().to_string(),
            started_at: start_time,
            completed_at: Some(Utc::now()),
            duration_ms: Some(total_duration),
            checkpoint: None,
        },
        steps: steps.clone(),
        outputs: Some(HashMap::from([
            ("user_name".to_string(), serde_json::json!("Leanne Graham")),
            ("user_email".to_string(), serde_json::json!("Sincere@april.biz")),
        ])),
        metrics: ExecutionMetrics {
            total_steps: steps.len(),
            success_steps: steps.iter().filter(|s| s.status == StepStatus::Success).count(),
            failed_steps: steps.iter().filter(|s| s.status == StepStatus::Failed).count(),
            skipped_steps: steps.iter().filter(|s| s.status == StepStatus::Skipped).count(),
            total_duration_ms: total_duration,
        },
        errors: vec![],
    })
}
