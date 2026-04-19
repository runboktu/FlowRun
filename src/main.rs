use clap::Parser;
use flow_run::cli::commands::{Args, CheckpointAction, CleanStrategy, Commands};
use flow_run::cli::output::{print_execution_plan, print_result};
use flow_run::core::runner::FlowRunner;
use flow_run::core::types::{StepStatus, WorkflowStatus};
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
        Commands::Run { input, json, dry_run, from_step, .. } => {
            let runner = FlowRunner::from_file(&cli.workflow)?;
            if dry_run {
                let plan = runner.plan(&input)?;
                print_execution_plan(&plan, json, &cli.workflow);
            } else if let Some(step_id) = from_step {
                // ── --from-step 模式：从指定步骤继续执行 ──
                let input_map = build_input_map(&input);
                match runner.run_from_step(&step_id, input_map).await {
                    Ok((result, skipped)) => {
                        if !json {
                            println!("⏩ 从步骤 {} 继续执行 (已恢复 {} 个前置步骤的输出)\n", step_id, skipped);
                        }
                        print_result(&result, json);
                        if !matches!(result.status, WorkflowStatus::Success) {
                            std::process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("❌ {}", e);
                        std::process::exit(1);
                    }
                }
            } else {
                // ── 正常执行模式 ──
                let input_map = build_input_map(&input);
                let result = runner.run(input_map).await?;

                if matches!(result.status, WorkflowStatus::Failed) {
                    print_failure_detail(&result, &cli.workflow);
                    std::process::exit(1);
                } else {
                    print_result(&result, json);
                }
            }
        }

        Commands::Resume { checkpoint_id, input, json } => {
            let checkpoint_dir = std::env::temp_dir().join("flow-run-checkpoints");
            std::fs::create_dir_all(&checkpoint_dir)?;
            let runner = FlowRunner::from_file(&cli.workflow)?
                .with_checkpoint_dir(checkpoint_dir);
            let input_map = build_input_map(&input);
            let result = runner.resume(&checkpoint_id, input_map).await?;
            print_result(&result, json);
            if !matches!(result.status, WorkflowStatus::Success) {
                std::process::exit(1);
            }
        }

        Commands::DryRun { input, json } => {
            let runner = FlowRunner::from_file(&cli.workflow)?;
            let plan = runner.plan(&input)?;
            print_execution_plan(&plan, json, &cli.workflow);
        }

        Commands::Validate { show_dag, json } => {
            match FlowRunner::from_file(&cli.workflow) {
                Ok(runner) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(runner.workflow())?);
                    } else {
                        println!("✅ 工作流验证通过: {}", runner.workflow().name);
                        if show_dag {
                            println!("步骤:");
                            for step in &runner.workflow().steps {
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
        }

        Commands::Checkpoint { action } => {
            handle_checkpoint_action(action);
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
        }

        Commands::Schema { output, pretty } => {
            let schema = r#"{"$schema": "http://json-schema.org/draft-07/schema#", "type": "object"}"#;
            if let Some(path) = output {
                std::fs::write(&path, schema)?;
                println!("Schema 已写入: {}", path.display());
            } else if pretty {
                println!("{}", serde_json::to_string_pretty(&serde_json::from_str::<serde_json::Value>(schema)?)?);
            } else {
                println!("{}", schema);
            }
        }
    }

    Ok(())
}

fn build_input_map(inputs: &[(String, String)]) -> HashMap<String, serde_json::Value> {
    let mut map = HashMap::new();
    for (k, v) in inputs {
        map.insert(k.clone(), serde_json::json!(v));
    }
    map
}

fn handle_checkpoint_action(action: CheckpointAction) {
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
                    println!("按状态清理: {:?}", status);
                }
                CleanStrategy::Keep { count } => {
                    println!("保留最近 {} 个检查点", count);
                }
            }
        }
    }
}

fn print_failure_detail(
    result: &flow_run::core::types::WorkflowResult,
    workflow_file: &std::path::Path,
) {
    let failed_steps: Vec<_> = result
        .steps
        .iter()
        .filter(|s| s.status == StepStatus::Failed)
        .collect();

    let success_count = result.steps.iter().filter(|s| s.status == StepStatus::Success).count();
    let total = result.steps.len();

    if let Some(failed) = failed_steps.first() {
        eprintln!();
        eprintln!("❌ 步骤执行失败: {} (第 {}/{} 步)", failed.step_id, success_count + 1, total);
        eprintln!();
        eprintln!("失败详情:");
        eprintln!("  步骤 ID:    {}", failed.step_id);
        if let Some(err) = &failed.error {
            eprintln!("  错误代码:   {}", err.code);
            eprintln!("  错误信息:   {}", err.message);
            if let Some(fix) = &err.fix {
                eprintln!("  修复建议:   {}", fix);
            }
        }
    }

    eprintln!();
    eprintln!("已完成步骤的输出已自动保存。");

    eprintln!();
    eprintln!("继续执行建议:");
    if let Some(failed) = failed_steps.first() {
        eprintln!("  修复问题后，从失败步骤重试:");
        eprintln!("    flow-run {} run --from-step {}", workflow_file.display(), failed.step_id);
    }

    let failed_id = failed_steps.first().map(|f| f.step_id.as_str()).unwrap_or("");
    let later_steps: Vec<&str> = result
        .steps
        .iter()
        .filter(|s| s.status == StepStatus::Pending || s.step_id != failed_id)
        .filter(|s| !result.steps.iter().take_while(|x| x.step_id != failed_id).any(|x| x.step_id == s.step_id))
        .map(|s| s.step_id.as_str())
        .collect();

    if !later_steps.is_empty() {
        eprintln!();
        eprintln!("  或从其他步骤开始:");
        for step_id in later_steps.iter().take(5) {
            eprintln!("    flow-run {} run --from-step {}", workflow_file.display(), step_id);
        }
    }
}
