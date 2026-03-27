use clap::Parser;
use flow_run::cli::commands::{Args, Commands};
use flow_run::core::parser::WorkflowParser;

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
        Commands::Run { mode: _, input, json, dry_run } => {
            let workflow = WorkflowParser::from_file(&cli.workflow)?;

            if json {
                println!("{}", serde_json::to_string_pretty(&workflow)?);
            } else {
                if dry_run {
                    println!("试运行模式: {}", workflow.name);
                } else {
                    println!("工作流: {}", workflow.name);
                    println!("步骤数: {}", workflow.steps.len());
                    println!("输入参数: {:?}", input);
                }
            }
        }

        Commands::Resume { checkpoint_id, input, json } => {
            println!("从检查点恢复: {}", checkpoint_id);
            println!("输入参数: {:?}", input);
            if json {
                println!("{{\"checkpoint_id\": \"{}\", \"status\": \"resumed\"}}", checkpoint_id);
            }
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
        }

        Commands::DryRun { input, json } => {
            let workflow = WorkflowParser::from_file(&cli.workflow)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&workflow)?);
            } else {
                println!("试运行模式: {}", workflow.name);
                println!("步骤数: {}", workflow.steps.len());
                println!("输入参数: {:?}", input);
            }
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
            } else {
                if pretty {
                    println!("{}", serde_json::to_string_pretty(&serde_json::from_str::<serde_json::Value>(schema)?)?);
                } else {
                    println!("{}", schema);
                }
            }
        }
    }

    Ok(())
}
