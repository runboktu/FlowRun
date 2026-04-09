//! AI 作曲编曲工作流 — 独立 CLI 工具
//!
//! 用法: cargo run --example 19_music_composer -- \
//!         --lyrics lyrics.txt --style "heavy blues, saxophone Solo"
//!
//! 流程:
//!   1. 解析 CLI 参数（lyrics 文件路径 / style 字符串 / output-dir）
//!   2. 解析 YAML 工作流 → DAG 拓扑排序
//!   3. 创建带自定义工具的 AgentManager（check_char_count / strip_whitespace）
//!   4. 按批次顺序执行：Agent 步骤 → Agent 步骤 → Shell 写文件
//!   5. 从 Agent 输出中提取 <chinese>/<english> 标签内容，汇总写入 result.md

use std::collections::HashMap;
use std::sync::Arc;

use flow_run::agent::{
    AgentManager, LlmProviderConfig, create_llm_provider, ToolRegistry,
    ToolDescriptor, ToolHandler, ToolResult,
};
use flow_run::core::context::ExecutionContext;
use flow_run::core::dag::DagScheduler;
use flow_run::core::parser::WorkflowParser;
use flow_run::core::template::TemplateEngine;

// ── CLI 参数解析 ──────────────────────────────────────────────

struct Cli {
    lyrics_path: String,
    style: String,
    output_dir: String,
}

fn parse_args() -> Result<Cli, Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut lyrics_path = None;
    let mut style = None;
    let mut output_dir = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--lyrics" | "-l" => {
                lyrics_path = Some(args[i + 1].clone());
                i += 2;
            }
            "--style" | "-s" => {
                style = Some(args[i + 1].clone());
                i += 2;
            }
            "--output-dir" | "-o" => {
                output_dir = Some(args[i + 1].clone());
                i += 2;
            }
            "--help" | "-h" => {
                eprintln!("用法: 19_music_composer --lyrics <FILE> --style <STYLE> [--output-dir <DIR>]");
                std::process::exit(0);
            }
            other => {
                return Err(format!("未知参数: {}", other).into());
            }
        }
    }

    Ok(Cli {
        lyrics_path: lyrics_path.ok_or("--lyrics 是必需参数")?,
        style: style.ok_or("--style 是必需参数")?,
        output_dir: output_dir.unwrap_or_else(|| "/tmp/music-output".to_string()),
    })
}

// ── 自定义工具 ────────────────────────────────────────────────

struct CheckCharCountTool;

#[async_trait::async_trait]
impl ToolHandler for CheckCharCountTool {
    async fn execute(&self, args: &str) -> ToolResult {
        let parsed: serde_json::Value = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return ToolResult::error(format!("JSON 解析失败: {}", e)),
        };

        let text = parsed["text"].as_str().unwrap_or("");
        let max_chars = parsed["max_chars"].as_u64().unwrap_or(1000) as usize;
        let count = text.chars().count();

        let result = serde_json::json!({
            "char_count": count,
            "max_chars": max_chars,
            "within_limit": count <= max_chars,
            "exceeded_by": if count > max_chars { count - max_chars } else { 0 },
        });
        println!("CheckCharCountTool {}", count);

        ToolResult::success(result.to_string())
    }
}

struct StripWhitespaceTool;

#[async_trait::async_trait]
impl ToolHandler for StripWhitespaceTool {
    async fn execute(&self, args: &str) -> ToolResult {
        let parsed: serde_json::Value = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return ToolResult::error(format!("JSON 解析失败: {}", e)),
        };

        let text = parsed["text"].as_str().unwrap_or("");
        let original_len = text.len();

        let cleaned = text
            .replace('\n', " ")
            .replace('\r', "")
            .replace('\t', " ")
            .split_whitespace()
            .collect::<Vec<&str>>()
            .join(" ");

        let result = serde_json::json!({
            "result": cleaned,
            "original_length": original_len,
            "cleaned_length": cleaned.len(),
            "removed_count": original_len - cleaned.len(),
        });
        println!("StripWhitespaceTool {}", cleaned.len());
        ToolResult::success(result.to_string())
    }
}

// ── XML 标签提取 ──────────────────────────────────────────────

fn extract_tag(content: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = content.find(&open)?;
    let end = content.find(&close)?;
    Some(content[start + open.len()..end].trim().to_string())
}

// ── 主流程 ────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("flow_run=info")
        .init();

    println!("══════════════════════════════════════════════");
    println!("  AI 作曲编曲工作流 v2");
    println!("══════════════════════════════════════════════\n");

    let cli = parse_args().map_err(|e| anyhow::anyhow!("{}", e))?;

    let lyrics = std::fs::read_to_string(&cli.lyrics_path)
        .map_err(|e| anyhow::anyhow!("无法读取歌词文件 '{}': {}", cli.lyrics_path, e))?;

    println!("[1] 输入:");
    println!("    歌词文件: {}", cli.lyrics_path);
    println!("    风格: {}", cli.style);
    println!("    输出: {}\n", cli.output_dir);

    let workflow = WorkflowParser::from_file("examples/19_music_composer.yaml")?;
    println!("[2] 工作流: {} (v{})",
        workflow.name,
        workflow.version.as_deref().unwrap_or("?")
    );

    let dag = DagScheduler::new(workflow.steps.clone())?;
    let batches = dag.topological_sort()?;
    println!("    DAG 批次: {:?}\n", batches);

    let mut inputs = HashMap::new();
    inputs.insert("lyrics".to_string(), serde_json::json!(lyrics));
    inputs.insert("style".to_string(), serde_json::json!(cli.style));
    inputs.insert("output_dir".to_string(), serde_json::json!(cli.output_dir));

    let context = ExecutionContext::new(&workflow, inputs);
    let ctx = Arc::new(tokio::sync::RwLock::new(context));

    let tool_registry = Arc::new(ToolRegistry::new());

    tool_registry.register(ToolDescriptor {
        name: "check_char_count".to_string(),
        description: "检查文本字符数是否在限制内".to_string(),
        json_schema: Some(r#"{"type":"object","properties":{"text":{"type":"string"},"max_chars":{"type":"integer","default":1000}}}"#.to_string()),
        handler: Arc::new(CheckCharCountTool),
    }).await;

    tool_registry.register(ToolDescriptor {
        name: "strip_whitespace".to_string(),
        description: "压缩文本中的多余空白字符（换行、Tab、连续空格压缩为单空格）".to_string(),
        json_schema: Some(r#"{"type":"object","properties":{"text":{"type":"string"}}}"#.to_string()),
        handler: Arc::new(StripWhitespaceTool),
    }).await;

    println!("[3] 已注册工具: {:?}\n", tool_registry.tool_names().await);

    let agent_manager = Arc::new(AgentManager::new(tool_registry));
    let api_key = std::env::var("DEEPSEEK_API_KEY")
        .or_else(|_| std::env::var("OPENAI_API_KEY"))
        .map_err(|_| anyhow::anyhow!("请设置 DEEPSEEK_API_KEY 或 OPENAI_API_KEY"))?;

    let provider_type = std::env::var("LLM_PROVIDER").unwrap_or_else(|_| "deepseek".to_string());
    let model = std::env::var("LLM_MODEL").ok();
    let base_url = std::env::var("LLM_BASE_URL").ok();

    let llm_config = LlmProviderConfig {
        r#type: provider_type,
        model,
        api_key,
        base_url,
    };
    let llm = create_llm_provider(&llm_config)?;

    let mut step_outputs: HashMap<String, serde_json::Value> = HashMap::new();

    println!("[4] 开始执行...\n");

    for (batch_idx, batch) in batches.iter().enumerate() {
        println!("── 批次 {} ──", batch_idx + 1);

        for step_id in batch {
            let step = dag.get_step(step_id)
                .ok_or_else(|| anyhow::anyhow!("步骤 {} 不存在", step_id))?
                .clone();

            println!("  [{}] ({:?})", step_id, step.r#type);

            match step.r#type {
                flow_run::core::types::StepType::Agent => {
                    execute_agent_step(&step, &ctx, &agent_manager, &llm, &mut step_outputs).await?;
                }
                flow_run::core::types::StepType::Shell => {
                    execute_shell_step(&step, &ctx, &cli.output_dir, &step_outputs).await?;
                }
                other => {
                    println!("    跳过不支持的步骤类型: {:?}", other);
                }
            }
        }
    }

    println!("\n══════════════════════════════════════════════");
    println!("  完成!");
    println!("══════════════════════════════════════════════");

    Ok(())
}

async fn execute_agent_step(
    step: &flow_run::core::types::StepDefinition,
    ctx: &Arc<tokio::sync::RwLock<ExecutionContext>>,
    agent_manager: &Arc<AgentManager>,
    llm: &Arc<dyn flow_run::agent::LlmProvider>,
    step_outputs: &mut HashMap<String, serde_json::Value>,
) -> Result<(), anyhow::Error> {
    let system_prompt = step.agent_system_prompt.as_deref().unwrap_or("");
    let input_template = step.agent_input.as_deref().unwrap_or("");
    let max_iterations = step.agent_max_iterations.unwrap_or(3);

    let session_id = agent_manager
        .create_session_with_llm(llm.clone(), Some(system_prompt))
        .await
        .map_err(|e| anyhow::anyhow!("创建 session 失败: {}", e))?;

    agent_manager.set_max_iterations(&session_id, max_iterations).await
        .map_err(|e| anyhow::anyhow!("设置 max_iterations 失败: {}", e))?;

    let template_ctx = {
        let ctx_guard = ctx.read().await;
        let mut tc = HashMap::new();
        tc.insert("inputs".to_string(), serde_json::to_value(&ctx_guard.inputs).unwrap_or_default());
        tc.insert("variables".to_string(), serde_json::to_value(&ctx_guard.variables).unwrap_or_default());

        for (sid, output) in step_outputs.iter() {
            tc.insert(format!("steps.{}.answer", sid), output.clone());
        }
        tc
    };

    let agent_input = TemplateEngine::new()
        .resolve_template(input_template, &template_ctx)
        .map_err(|e| anyhow::anyhow!("模板解析失败: {}", e))?;

    println!("    Agent 输入: {}...", &agent_input[..agent_input.chars().take(60).collect::<String>().len()]);

    let result = agent_manager
        .run_sync(&session_id, &agent_input, None)
        .await
        .map_err(|e| anyhow::anyhow!("Agent 执行失败: {}", e))?;

    agent_manager.destroy_session(&session_id).await;

    step_outputs.insert(step.id.clone(), serde_json::json!({ "answer": result.clone() }));

    {
        let mut ctx_guard = ctx.write().await;
        ctx_guard.step_outputs.insert(
            step.id.clone(),
            flow_run::core::types::StepResult::success(
                step.id.clone(),
                serde_json::json!({ "answer": result }),
            ),
        );
    }

    let preview: String = result.chars().take(100).collect();
    println!("    Agent 输出: {}...", preview);

    Ok(())
}

async fn execute_shell_step(
    _step: &flow_run::core::types::StepDefinition,
    _ctx: &Arc<tokio::sync::RwLock<ExecutionContext>>,
    output_dir: &str,
    step_outputs: &HashMap<String, serde_json::Value>,
) -> Result<(), anyhow::Error> {
    let tagged_lyrics = step_outputs
        .get("tag_lyrics")
        .and_then(|v| v.get("answer"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let compose_result = step_outputs
        .get("compose_prompt")
        .and_then(|v| v.get("answer"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let chinese = extract_tag(compose_result, "chinese").unwrap_or_default();
    let english = extract_tag(compose_result, "english").unwrap_or_default();
    let english_char_count = english.chars().count();

    let result_md = format!(
        r#"# AI 作曲编曲结果

## 一、歌词 Suno Tag 标注

{}

---

## 二、中文作曲提示词

{}

---

## 三、英文作曲提示词 ({} chars)

{}

---

## 四、英文提示词字符统计

- 字符数: {}
- 限制: 1000
- 状态: {}
"#,
        tagged_lyrics,
        chinese,
        english_char_count,
        english,
        english_char_count,
        if english_char_count <= 1000 { "✅ 通过" } else { "❌ 超限" },
    );

    std::fs::create_dir_all(output_dir)?;
    let output_path = format!("{}/result.md", output_dir);
    std::fs::write(&output_path, &result_md)?;

    println!("    输出文件: {}", output_path);
    println!("    英文字符数: {}/1000", english_char_count);

    Ok(())
}
