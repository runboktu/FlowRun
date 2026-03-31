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

    if let Some(outputs) = &workflow.outputs {
        println!("\n── 工作流输出 ──");
        for (key, expr) in outputs {
            println!("  {}: {}", key, expr);
        }
    }

    println!("\n── 步骤列表 ──");
    for step in &workflow.steps {
        let deps = step.depends_on.as_ref()
            .map(|d| d.join(", "))
            .unwrap_or_else(|| "无".to_string());
        print!("  {} ({:?})", step.id, step.r#type);
        if let Some(name) = &step.name {
            print!(" - {}", name);
        }
        print!("  [依赖: {}]", deps);
        if let Some(timeout) = &step.timeout {
            print!("  [超时: {}]", timeout);
        }
        if let Some(retry) = &step.retry {
            print!("  [重试: max={}, strategy={:?}]", retry.max_attempts, retry.strategy);
        }
        println!();

        match step.r#type {
            flow_run::core::types::StepType::Http => {
                if let Some(api) = &step.api {
                    println!("    API: {} {}", step.method.as_deref().unwrap_or("GET"), api);
                }
            }
            flow_run::core::types::StepType::Shell => {
                if let Some(cmd) = &step.run {
                    let preview = if cmd.chars().count() > 80 {
                        let end: String = cmd.chars().take(77).collect();
                        format!("{}...", end)
                    } else {
                        cmd.clone()
                    };
                    println!("    命令: {}", preview);
                }
            }
            flow_run::core::types::StepType::Parallel => {
                if let Some(sub_steps) = &step.steps {
                    println!("    子步骤: {} 个", sub_steps.len());
                    for sub in sub_steps {
                        println!("      - {} ({:?})", sub.id, sub.r#type);
                    }
                }
                if let Some(max) = step.max_concurrent {
                    println!("    最大并发: {}", max);
                }
            }
            flow_run::core::types::StepType::Loop => {
                if let Some(loop_cfg) = &step.r#loop {
                    println!("    循环: {:?}", loop_cfg);
                }
            }
            flow_run::core::types::StepType::Condition => {
                if let Some(expr) = &step.expression {
                    println!("    条件: {}", expr);
                }
                if let Some(then) = &step.then_steps {
                    println!("    then 分支: {} 个步骤", then.len());
                }
                if let Some(else_) = &step.else_steps {
                    println!("    else 分支: {} 个步骤", else_.len());
                }
            }
            flow_run::core::types::StepType::Workflow => {
                if let Some(wf) = &step.workflow {
                    println!("    子工作流: {}", wf);
                }
            }
            flow_run::core::types::StepType::Approve => {
                if let Some(approvers) = &step.approvers {
                    println!("    审批人: {:?}", approvers);
                }
            }
            flow_run::core::types::StepType::Agent => {
                if let Some(input) = &step.agent_input {
                    println!("    Agent 输入: {}", input);
                }
                if let Some(max_iter) = &step.agent_max_iterations {
                    println!("    最大迭代次数: {}", max_iter);
                }
            }
            flow_run::core::types::StepType::Tool => {
                if let Some(tool_name) = &step.tool_name {
                    println!("    工具: {}", tool_name);
                }
                if let Some(args) = &step.tool_args {
                    println!("    参数: {}", args);
                }
            }
        }
    }

    let edge_count: usize = workflow.steps.iter()
        .filter_map(|s| s.depends_on.as_ref().map(|d| d.len()))
        .sum();

    println!("\n── DAG 结构 ──");
    println!("  节点: {} | 边: {}", workflow.steps.len(), edge_count);
    for step in &workflow.steps {
        if let Some(deps) = &step.depends_on {
            for dep in deps {
                println!("  {} ──→ {}", dep, step.id);
            }
        }
    }

    println!("\n── 拓扑排序（执行计划）──");
    println!("  共 {} 个批次", batches.len());
    for (i, batch) in batches.iter().enumerate() {
        let parallel_tag = if batch.len() > 1 { " (并行)" } else { "" };
        println!("  批次 {}:{} {} 个步骤", i + 1, parallel_tag, batch.len());
        for step_id in batch {
            let step = workflow.steps.iter().find(|s| &s.id == step_id);
            if let Some(s) = step {
                let name_suffix = s.name.as_ref().map(|n| format!(" - {}", n)).unwrap_or_default();
                    let out_edges: Vec<&str> = workflow.steps.iter()
                        .filter_map(|s| {
                            let is_child = s.depends_on.as_ref()
                                .map_or(false, |d| d.iter().any(|dep| *dep == *step_id));
                            if is_child { Some(s.id.as_str()) } else { None }
                        })
                        .collect();
                    let out_str = if out_edges.is_empty() { "无".to_string() } else { out_edges.join(", ") };
                    println!("    ├─ {}{:?}{} [out→ {}]",
                        s.id, s.r#type, name_suffix, out_str
                    );
            }
        }
    }

    println!("\n══════════════════════════════════════════════");
    println!("  以上为模拟执行，未实际运行任何步骤");
    println!("══════════════════════════════════════════════");

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
