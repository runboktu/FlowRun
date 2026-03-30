use crate::core::types::*;
use std::path::Path;

/// 打印工作流执行结果
pub fn print_result(result: &WorkflowResult, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(result).unwrap_or_default()
        );
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
    println!(
        "  总步骤: {} | 成功: {} | 失败: {} | 跳过: {}",
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

/// 打印执行计划（dry-run 输出）
pub fn print_execution_plan(plan: &ExecutionPlan, json: bool, workflow_file: &Path) {
    if json {
        println!("{}", serde_json::to_string_pretty(plan).unwrap_or_default());
        return;
    }

    println!("══════════════════════════════════════════════");
    println!("  Dry Run: {}", plan.workflow_name);
    println!("══════════════════════════════════════════════");

    if let Some(desc) = &plan.workflow_description {
        println!("  描述: {}", desc);
    }
    if let Some(ver) = &plan.workflow_version {
        println!("  版本: {}", ver);
    }
    println!("  文件: {}", workflow_file.display());
    println!("  步骤: {} 个", plan.step_count);
    println!("  DAG 检查: 无循环依赖");

    if let Some(config) = &plan.config {
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
            println!(
                "  全局重试: max={}, strategy={:?}",
                retry.max_attempts, retry.strategy
            );
        }
    }

    if let Some(input_defs) = &plan.inputs {
        println!("\n── 输入参数 ──");
        for inp in input_defs {
            let req = if inp.required == Some(true) {
                "必填"
            } else {
                "可选"
            };
            println!(
                "  {} [{}]: {}",
                inp.name,
                req,
                inp.r#type.as_deref().unwrap_or("any")
            );
        }
        if !plan.provided_inputs.is_empty() {
            println!("  ─────────────");
            for (k, v) in &plan.provided_inputs {
                println!("  {} = {}", k, v);
            }
        }
    }

    if let Some(outputs) = &plan.outputs {
        println!("\n── 工作流输出 ──");
        for (key, expr) in outputs {
            println!("  {}: {}", key, expr);
        }
    }

    println!("\n── DAG 结构 ──");
    println!("  节点: {} | 边: {}", plan.step_count, plan.dag_edges.len());
    for edge in &plan.dag_edges {
        println!("  {} ──→ {}", edge.from, edge.to);
    }

    println!("\n── 拓扑排序（执行计划）──");
    println!("  共 {} 个批次", plan.batches.len());
    for (i, batch) in plan.batches.iter().enumerate() {
        let parallel_tag = if batch.len() > 1 { " (并行)" } else { "" };
        println!("  批次 {}:{} {} 个步骤", i + 1, parallel_tag, batch.len());
        for step_id in batch {
            println!("    ├─ {}", step_id);
        }
    }

    println!("\n══════════════════════════════════════════════");
    println!("  以上为模拟执行，未实际运行任何步骤");
    println!("══════════════════════════════════════════════");
}
