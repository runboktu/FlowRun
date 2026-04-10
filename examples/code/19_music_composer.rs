//! AI 作曲编曲工作流示例
//!
//! 用法:
//!   cargo run --example 19_music_composer -- \
//!     --lyrics lyrics.txt \
//!     --style "heavy blues, saxophone solo" \
//!     --output-dir /tmp/music-output
//!
//! 这个示例直接运行 `examples/19_music_composer.yaml`，
//! 由 YAML 自己声明 Agent step 的 `agent_tools` 和 Tool step 的 inline `tool`。

use anyhow::{Context, bail};
use flow_run::core::runner::FlowRunner;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

struct Cli {
    lyrics_path: String,
    style: String,
    output_dir: String,
}

fn parse_args() -> Result<Cli, anyhow::Error> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut lyrics_path = None;
    let mut style = None;
    let mut output_dir = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--lyrics" | "-l" => {
                let value = args.get(i + 1).context("--lyrics 缺少参数值")?;
                lyrics_path = Some(value.clone());
                i += 2;
            }
            "--style" | "-s" => {
                let value = args.get(i + 1).context("--style 缺少参数值")?;
                style = Some(value.clone());
                i += 2;
            }
            "--output-dir" | "-o" => {
                let value = args.get(i + 1).context("--output-dir 缺少参数值")?;
                output_dir = Some(value.clone());
                i += 2;
            }
            "--help" | "-h" => {
                println!(
                    "用法: 19_music_composer --lyrics <FILE> --style <STYLE> [--output-dir <DIR>]"
                );
                std::process::exit(0);
            }
            other => bail!("未知参数: {}", other),
        }
    }

    Ok(Cli {
        lyrics_path: lyrics_path.context("--lyrics 是必需参数")?,
        style: style.context("--style 是必需参数")?,
        output_dir: output_dir.unwrap_or_else(|| "/tmp/music-output".to_string()),
    })
}

fn get_required_env() -> Result<(), anyhow::Error> {
    std::env::var("DEEPSEEK_API_KEY")
        .or_else(|_| std::env::var("OPENAI_API_KEY"))
        .map(|_| ())
        .context("请先设置 DEEPSEEK_API_KEY 或 OPENAI_API_KEY")
}

fn value_as_str<'a>(outputs: &'a HashMap<String, Value>, key: &str) -> Option<&'a str> {
    outputs.get(key).and_then(|v| v.as_str())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("flow_run=info")
        .init();

    let cli = parse_args()?;
    get_required_env()?;

    let lyrics = fs::read_to_string(&cli.lyrics_path)
        .with_context(|| format!("无法读取歌词文件: {}", cli.lyrics_path))?;

    let workflow_path = Path::new("examples/19_music_composer.yaml");
    let runner = FlowRunner::from_file(workflow_path)
        .with_context(|| format!("无法加载工作流文件: {}", workflow_path.display()))?;

    let mut inputs = HashMap::new();
    inputs.insert("lyrics".to_string(), Value::String(lyrics));
    inputs.insert("style".to_string(), Value::String(cli.style.clone()));
    inputs.insert("output_dir".to_string(), Value::String(cli.output_dir.clone()));

    println!("==========================================");
    println!("  flow-run - AI 作曲编曲工作流");
    println!("==========================================");
    println!("工作流: {}", workflow_path.display());
    println!("歌词文件: {}", cli.lyrics_path);
    println!("风格: {}", cli.style);
    println!("输出目录: {}", cli.output_dir);
    println!();

    let result = runner.run(inputs).await?;

    println!("执行状态: {:?}", result.status);
    println!(
        "步骤统计: 总计 {} | 成功 {} | 失败 {} | 跳过 {} | 耗时 {}ms",
        result.metrics.total_steps,
        result.metrics.success_steps,
        result.metrics.failed_steps,
        result.metrics.skipped_steps,
        result.metrics.total_duration_ms
    );

    for step in &result.steps {
        println!("  - {}: {:?}", step.step_id, step.status);
        if let Some(error) = &step.error {
            println!("    错误: {}", error.message);
        }
    }

    println!();
    if let Some(outputs) = &result.outputs {
        if let Some(path) = value_as_str(outputs, "tagged_lyrics_path") {
            println!("tagged_lyrics: {}", path);
        }
        if let Some(path) = value_as_str(outputs, "chinese_prompt_path") {
            println!("compose_prompt_zh: {}", path);
        }
        if let Some(path) = value_as_str(outputs, "english_prompt_path") {
            println!("compose_prompt_en: {}", path);
        }
        if let Some(path) = value_as_str(outputs, "report_path") {
            println!("report: {}", path);
        }
        if let Some(chars) = outputs.get("english_char_count").and_then(|v| v.as_u64()) {
            println!("english_char_count: {}", chars);
        }
        println!();

        if let Some(report_path) = value_as_str(outputs, "report_path") {
            let report = fs::read_to_string(report_path)
                .with_context(|| format!("无法读取报告文件: {}", report_path))?;
            let preview: String = report.chars().take(400).collect();
            println!("报告预览:\n{}\n", preview);
        }
    }

    if result.status != flow_run::core::types::WorkflowStatus::Success {
        bail!("工作流执行失败");
    }

    println!("==========================================");
    println!("  示例完成!");
    println!("==========================================");

    Ok(())
}
