//! Supervisor 多智能体编排示例
//!
//! Pipeline 模式：Supervisor → Research → Math → Writer → Summary
//!
//! 运行：
//!   DEEPSEEK_API_KEY=sk-xxx cargo run --example 21_supervisor_pattern

use flow_run::core::runner::FlowRunner;
use std::collections::HashMap;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("flow_run=info")
        .init();

    let question = "FAANG 公司 2024 年总员工数是多少？计算出总人数的平方根";

    let runner = FlowRunner::from_file("examples/21_supervisor_pattern.yaml")?;
    let workflow = runner.workflow();

    println!("工作流: {} — {}", workflow.name, workflow.description.as_deref().unwrap_or(""));
    println!("步骤数: {}", workflow.steps.len());
    println!("问题: {}\n", question);

    let mut inputs = HashMap::new();
    inputs.insert("question".to_string(), serde_json::json!(question));

    let result = runner.run(inputs).await?;

    println!("\n状态: {:?}", result.status);
    for step in &result.steps {
        println!("  {} ({:?})", step.step_id, step.status);
        if let Some(output) = &step.output {
            if let Some(answer) = output.get("answer").and_then(|v| v.as_str()) {
                let preview: String = answer.chars().take(150).collect();
                println!("    {}", preview);
            }
        }
    }

    println!("\n耗时: {}ms", result.metrics.total_duration_ms);

    if let Some(outputs) = &result.outputs {
        println!("\n输出:");
        for (key, value) in outputs {
            let preview: String = value.to_string().chars().take(120).collect();
            println!("  {}: {}", key, preview);
        }
    }

    Ok(())
}
