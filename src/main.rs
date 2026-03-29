use clap::Parser;
use flow_run::cli::commands::{Args, Commands};
use flow_run::core::context::ExecutionContext;
use flow_run::core::dag::{DagScheduler, Scheduler};
use flow_run::core::parser::WorkflowParser;
use flow_run::core::types::{StepStatus, WorkflowStatus};
use flow_run::utils::checkpoint::CheckpointManager;
use std::collections::HashMap;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Args::parse();

    match cli.command {
        Commands::Run { input, json, dry_run, .. } => {
            let workflow = WorkflowParser::from_file(&cli.workflow)?;
            if dry_run {
                dry_run_workflow(&workflow, &cli.workflow, &input, json)
            } else {
                let result = run_workflow(&workflow, &input).await?;
                print_result(&result, json);
                if !matches!(result.status, WorkflowStatus::Success) {
                    std::process::exit(1);
                }
                Ok(())
            }
        }

        Commands::Resume { checkpoint_id, input, json } => {
            let workflow = WorkflowParser::from_file(&cli.workflow)?;
            let result = resume_workflow(&workflow, &checkpoint_id, &input).await?;
            print_result(&result, json);
            if !matches!(result.status, WorkflowStatus::Success) {
                std::process::exit(1);
            }
            Ok(())
        }

        Commands::DryRun { input, json } => {
            let workflow = WorkflowParser::from_file(&cli.workflow)?;
            dry_run_workflow(&workflow, &cli.workflow, &input, json)
        }

        Commands::Validate { show_dag, json } => {
            match WorkflowParser::from_file(&cli.workflow) {
                Ok(workflow) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&workflow)?);
                    } else {
                        println!("✅ 工作流验证通过: {}", workflow.name);
                        if show_dag {
                            println!("步骤:");
                            for step in &workflow.steps {
                                println!("  - {} ({})", step.id, serde_json::to_string(&step.r#type).unwrap_or_default());
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("❌ 验证失败: {}", e);
                    std::process::exit(1);
                }
            }
            Ok(())
        }

        Commands::Checkpoint { action } => {
            use flow_run::cli::commands::CheckpointAction;
            match action {
                CheckpointAction::List { verbose, status, json } => {
                    if json {
                        println!("{{\"checkpoints\": []}}");
                    } else {
                        println!("检查点列表:");
                        if verbose {
                            println!("  (详细模式)");
                        }
                        if let Some(s) = status {
                            println!("  状态过滤: {}", s);
                        }
                    }
                }
                CheckpointAction::Show { id, steps, json } => {
                    if json {
                        println!("{{\"id\": \"{}\"}}", id);
                    } else {
                        println!("检查点详情: {}", id);
                        if steps {
                            println!("  (显示步骤信息)");
                        }
                    }
                }
                CheckpointAction::Clean { strategy } => {
                    use flow_run::cli::commands::CleanStrategy;
                    match strategy {
                        CleanStrategy::Id { ids } => {
                            for id in ids {
                                println!("清理检查点: {}", id);
                            }
                        }
                        CleanStrategy::All { confirm } => {
                            if confirm {
                                println!("清理所有检查点");
                            } else {
                                println!("需要 --confirm 确认清理操作");
                            }
                        }
                        CleanStrategy::OlderThan { days } => {
                            println!("清理超过 {} 天的检查点", days);
                        }
                        CleanStrategy::Status { status } => {
                            println!("清理状态为 {} 的检查点", status);
                        }
                        CleanStrategy::Keep { count } => {
                            println!("保留最近 {} 个检查点", count);
                        }
                    }
                }
            }
            Ok(())
        }

        Commands::History { limit, status, failed, json } => {
            if json {
                println!("{{\"history\": []}}");
            } else {
                println!("执行历史:");
                println!("  限制: {}", limit);
                if let Some(s) = status {
                    println!("  状态过滤: {}", s);
                }
                if failed {
                    println!("  只显示失败");
                }
            }
            Ok(())
        }

        Commands::Schema { output, pretty } => {
            let schema = r#"{"$schema": "http://json-schema.org/draft-07/schema#", "type": "object"}"#;
            if let Some(path) = output {
                std::fs::write(&path, schema)?;
                println!("Schema 已写入: {}", path.display());
            } else {
                if pretty {
                    println!("{}", serde_json::to_string_pretty(&serde_json::from_str::<serde_json::Value>(schema)?)?);
                } else {
                    println!("{}", schema);
                }
            }
            Ok(())
        }
    }
}

async fn run_workflow(
    workflow: &flow_run::core::types::WorkflowDefinition,
    inputs: &[(String, String)],
) -> anyhow::Result<flow_run::core::types::WorkflowResult> {
    let dag = DagScheduler::new(workflow.steps.clone())?;
    let checkpoint_dir = std::env::temp_dir().join(format!("flow-run-{}", std::process::id()));
    let checkpoint_manager = CheckpointManager::new(checkpoint_dir)?;
    let config = workflow.config.clone().unwrap_or_default();
    let scheduler = Scheduler::new(dag, config, checkpoint_manager);

    let mut input_map = HashMap::new();
    for (k, v) in inputs {
        input_map.insert(k.clone(), serde_json::json!(v));
    }
    let context = ExecutionContext::new(workflow, input_map);
    scheduler.set_context(context).await;

    if let Some(outputs) = &workflow.outputs {
        let outputs_map: HashMap<String, String> = outputs
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        scheduler.set_outputs(outputs_map).await;
    }

    Ok(scheduler.run().await?)
}

async fn resume_workflow(
    workflow: &flow_run::core::types::WorkflowDefinition,
    checkpoint_id: &str,
    inputs: &[(String, String)],
) -> anyhow::Result<flow_run::core::types::WorkflowResult> {
    let dag = DagScheduler::new(workflow.steps.clone())?;
    let checkpoint_dir = std::env::temp_dir().join("flow-run-checkpoints");
    std::fs::create_dir_all(&checkpoint_dir)?;
    let checkpoint_manager = CheckpointManager::new(checkpoint_dir)?;

    let config = workflow.config.clone().unwrap_or_default();
    let scheduler = Scheduler::new(dag, config, checkpoint_manager);

    let mut input_map = HashMap::new();
    for (k, v) in inputs {
        input_map.insert(k.clone(), serde_json::json!(v));
    }
    let context = ExecutionContext::new(workflow, input_map);
    scheduler.set_context(context).await;

    if let Some(outputs) = &workflow.outputs {
        let outputs_map: HashMap<String, String> = outputs
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        scheduler.set_outputs(outputs_map).await;
    }

    Ok(scheduler.resume(checkpoint_id).await?)
}

fn dry_run_workflow(
    workflow: &flow_run::core::types::WorkflowDefinition,
    workflow_file: &std::path::Path,
    inputs: &[(String, String)],
    json: bool,
) -> anyhow::Result<()> {
    let dag = DagScheduler::new(workflow.steps.clone())?;
    let batches = dag.topological_sort()?;

    if json {
        let plan = serde_json::json!({
            "name": workflow.name,
            "steps": workflow.steps.len(),
            "batches": batches,
            "inputs": inputs.iter().map(|(k, v)| (k, v)).collect::<HashMap<_, _>>(),
        });
        println!("{}", serde_json::to_string_pretty(&plan)?);
    } else {
        println!("工作流: {}", workflow.name);
        println!("文件: {}", workflow_file.display());
        println!("步骤数: {}", workflow.steps.len());
        println!("输入参数: {:?}", inputs);
        println!("\n执行计划:");
        for (i, batch) in batches.iter().enumerate() {
            println!("  批次 {}:", i + 1);
            for step_id in batch {
                let step = workflow.steps.iter().find(|s| &s.id == step_id);
                if let Some(s) = step {
                    println!("    - {} ({:?})", s.id, s.r#type);
                }
            }
        }
    }
    Ok(())
}

fn print_result(result: &flow_run::core::types::WorkflowResult, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(result).unwrap_or_default());
        return;
    }

    println!("\n执行结果: {:?}", result.status);
    println!("步骤结果:");
    for step in &result.steps {
        let icon = match step.status {
            StepStatus::Success => "OK",
            StepStatus::Failed => "FAIL",
            StepStatus::Skipped => "SKIP",
            StepStatus::Pending => "PEND",
            StepStatus::Running => "RUN",
        };
        print!("  [{}] {}", icon, step.step_id);
        if let Some(output) = &step.output {
            if let Some(stdout) = output.get("stdout").and_then(|v| v.as_str()) {
                let trimmed = stdout.replace('\n', " ").trim().to_string();
                if !trimmed.is_empty() {
                    print!("  {}", &trimmed[..trimmed.len().min(120)]);
                }
            }
            if let Some(status_code) = output.get("status_code") {
                print!("  status={}", status_code);
            }
        }
        println!();
    }

    println!("\n指标:");
    println!("  总步骤: {} | 成功: {} | 失败: {} | 跳过: {}",
        result.metrics.total_steps,
        result.metrics.success_steps,
        result.metrics.failed_steps,
        result.metrics.skipped_steps,
    );
    println!("  耗时: {}ms", result.metrics.total_duration_ms);

    if let Some(outputs) = &result.outputs {
        if !outputs.is_empty() {
            println!("\n工作流输出:");
            for (key, value) in outputs {
                println!("  {}: {}", key, value);
            }
        }
    }
}
