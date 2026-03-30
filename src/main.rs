use clap::Parser;
use flow_run::cli::commands::{Args, CheckpointAction, CleanStrategy, Commands};
use flow_run::cli::output::{print_execution_plan, print_result};
use flow_run::core::runner::FlowRunner;
use flow_run::core::types::WorkflowStatus;
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
            let runner = FlowRunner::from_file(&cli.workflow)?;
            if dry_run {
                let plan = runner.plan(&input)?;
                print_execution_plan(&plan, json, &cli.workflow);
            } else {
                let input_map = build_input_map(&input);
                let result = runner.run(input_map).await?;
                print_result(&result, json);
                if !matches!(result.status, WorkflowStatus::Success) {
                    std::process::exit(1);
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
                    println!("清理状态为 {} 的检查点", status);
                }
                CleanStrategy::Keep { count } => {
                    println!("保留最近 {} 个检查点", count);
                }
            }
        }
    }
}
