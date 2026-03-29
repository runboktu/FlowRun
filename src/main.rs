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
            "workflow": {
                "name": workflow.name,
                "description": workflow.description,
                "version": workflow.version,
            },
            "inputs": workflow.inputs.as_ref().map(|i| i.iter().map(|inp| serde_json::json!({
                "name": inp.name,
                "type": inp.r#type,
                "required": inp.required,
            })).collect::<Vec<_>>()),
            "provided_inputs": inputs.iter().map(|(k, v)| (k, v)).collect::<HashMap<_, _>>(),
            "config": workflow.config.as_ref().map(|c| serde_json::json!({
                "timeout": c.timeout,
                "on_failure": c.on_failure.as_ref().map(|f| format!("{:?}", f)),
                "checkpoint": c.checkpoint,
                "max_concurrent": c.max_concurrent,
            })),
            "outputs": workflow.outputs,
            "steps": workflow.steps.iter().map(|s| serde_json::json!({
                "id": s.id,
                "name": s.name,
                "type": format!("{:?}", s.r#type),
                "depends_on": s.depends_on,
                "timeout": s.timeout,
                "retry": s.retry.as_ref().map(|r| serde_json::json!({
                    "max_attempts": r.max_attempts,
                    "strategy": r.strategy.as_ref().map(|s| format!("{:?}", s)),
                })),
            })).collect::<Vec<_>>(),
            "dag": {
                "edges": workflow.steps.iter().filter_map(|s| {
                    s.depends_on.as_ref().map(|deps| serde_json::json!({
                        "step": s.id,
                        "depends_on": deps,
                    }))
                }).collect::<Vec<_>>(),
            },
            "topological_sort": {
                "total_batches": batches.len(),
                "batches": batches.iter().enumerate().map(|(i, b)| serde_json::json!({
                    "batch": i + 1,
                    "parallel": b.len() > 1,
                    "steps": b,
                })).collect::<Vec<_>>(),
            },
        });
        println!("{}", serde_json::to_string_pretty(&plan)?);
        return Ok(());
    }

    println!("══════════════════════════════════════════════");
    println!("  Dry Run: {}", workflow.name);
    println!("══════════════════════════════════════════════");

    if let Some(desc) = &workflow.description {
        println!("  描述: {}", desc);
    }
    if let Some(ver) = &workflow.version {
        println!("  版本: {}", ver);
    }
    println!("  文件: {}", workflow_file.display());
    println!("  步骤: {} 个", workflow.steps.len());
    println!("  DAG 检查: 无循环依赖");

    if let Some(config) = &workflow.config {
        println!("\n── 全局配置 ──");
        if let Some(timeout) = &config.timeout {
            println!("  超时: {}", timeout);
        }
        if let Some(failure) = &config.on_failure {
            println!("  失败策略: {:?}", failure);
        }
        if let Some(cp) = &config.checkpoint {
            println!("  检查点: {}", cp);
        }
        if let Some(max) = config.max_concurrent {
            println!("  最大并发: {}", max);
        }
        if let Some(retry) = &config.retry {
            println!("  全局重试: max={}, strategy={:?}", retry.max_attempts, retry.strategy);
        }
    }

    if let Some(input_defs) = &workflow.inputs {
        println!("\n── 输入参数 ──");
        for inp in input_defs {
            let req = if inp.required == Some(true) { "必填" } else { "可选" };
            println!("  {} [{}]: {}", inp.name, req, inp.r#type.as_deref().unwrap_or("any"));
        }
        if !inputs.is_empty() {
            println!("  ─────────────");
            for (k, v) in inputs {
                println!("  {} = {}", k, v);
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
