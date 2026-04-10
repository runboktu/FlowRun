//! 自定义 Builtin 工具扩展示例
//!
//! 演示如何使用 FlowRunner::register_builtin() 注册自定义工具，
//! 然后在 YAML 中通过 `source: builtin` + `name: xxx` 引用。
//!
//! 无需 LLM API Key，纯工具步骤。
//!
//! 用法:
//!   cargo run --example 20_builtin_tool_extensibility

use flow_run::agent::tool_registry::FnTool;
use flow_run::FlowRunner;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("flow_run=info")
        .init();

    let workflow_path = Path::new("examples/20_builtin_tool_extensibility.yaml");

    let sample_text = "Hello   World!   This   is   a   test   text   with   lots   of   extra   whitespace   characters.";

    let runner = FlowRunner::from_file(workflow_path)?
        // 注册自定义 builtin 工具：check_char_count
        .register_builtin(
            "check_char_count",
            "检查文本字符数是否在限制内",
            Arc::new(FnTool(|args: String| async move {
                let parsed: serde_json::Value = serde_json::from_str(&args).unwrap_or_default();
                let text = parsed["text"].as_str().unwrap_or("");
                let max_chars = parsed["max_chars"].as_u64().unwrap_or(1000);
                let count = text.chars().count();
                let over_limit = count > max_chars as usize;
                format!(
                    "{{\"char_count\": {}, \"max_chars\": {}, \"over_limit\": {}}}",
                    count, max_chars, over_limit
                )
            })),
        )
        // 注册自定义 builtin 工具：strip_whitespace
        .register_builtin(
            "strip_whitespace",
            "压缩文本中的多余空白字符",
            Arc::new(FnTool(|args: String| async move {
                let parsed: serde_json::Value = serde_json::from_str(&args).unwrap_or_default();
                let text = parsed["text"].as_str().unwrap_or("");
                let result = text
                    .split_whitespace()
                    .collect::<Vec<&str>>()
                    .join(" ");
                format!("{{\"text\": \"{}\", \"char_count\": {}}}", result, result.chars().count())
            })),
        );

    println!("==========================================");
    println!("  自定义 Builtin 工具扩展示例");
    println!("==========================================");
    println!("工作流: {}", workflow_path.display());
    println!("输入文本: {}", sample_text);
    println!();

    let mut inputs = HashMap::new();
    inputs.insert("text".to_string(), Value::String(sample_text.to_string()));
    inputs.insert("max_chars".to_string(), Value::String("50".to_string()));

    let result = runner.run(inputs).await?;

    println!("执行状态: {:?}", result.status);
    println!(
        "步骤统计: 总计 {} | 成功 {} | 失败 {} | 耗时 {}ms",
        result.metrics.total_steps,
        result.metrics.success_steps,
        result.metrics.failed_steps,
        result.metrics.total_duration_ms
    );

    for step in &result.steps {
        println!("  - {}: {:?}", step.step_id, step.status);
        if let Some(output) = &step.output {
            if let Some(answer) = output.get("answer").and_then(|v| v.as_str()) {
                let preview: String = answer.chars().take(200).collect();
                println!("    answer: {}", preview);
            }
        }
    }

    if let Some(outputs) = &result.outputs {
        println!("\n工作流输出:");
        for (key, value) in outputs {
            let preview: String = value.to_string().chars().take(100).collect();
            println!("  {}: {}", key, preview);
        }
    }

    if !matches!(
        result.status,
        flow_run::core::types::WorkflowStatus::Success
    ) {
        return Err(anyhow::anyhow!("工作流执行失败"));
    }

    println!("\n==========================================");
    println!("  示例完成!");
    println!("==========================================");

    Ok(())
}
