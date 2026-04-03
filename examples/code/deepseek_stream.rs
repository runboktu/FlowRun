//! DeepSeek 流式调用示例
//!
//! 这个示例展示如何：
//! - 直接使用 DeepSeekProvider 的 call_stream 方法
//! - 消费 LlmChunk stream，实时打印生成的内容
//! - 累积完整响应

use flow_run::agent::{DeepSeekProvider, LlmProvider, LlmProviderConfig};
use flow_run::agent::types::Message;
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("flow_run=info")
        .init();

    println!("==========================================");
    println!("  flow-run - DeepSeek 流式调用示例");
    println!("==========================================\n");

    // 1. 检查环境变量
    let api_key = std::env::var("DEEPSEEK_API_KEY")
        .or_else(|_| std::env::var("OPENAI_API_KEY"))
        .map_err(|_| anyhow::anyhow!("DEEPSEEK_API_KEY or OPENAI_API_KEY not set"))?;

    // 2. 创建 DeepSeek provider（两种方式）
    println!("[1] 创建 DeepSeek Provider");

    // 方式 A：直接使用 DeepSeekProvider 构造函数
    let provider = DeepSeekProvider::new(
        &api_key,
        "deepseek-chat",
        "https://api.deepseek.com/v1",
    );

    // 方式 B：通过工厂函数（支持从配置创建）
    let _config = LlmProviderConfig {
        r#type: "deepseek".to_string(),
        model: Some("deepseek-chat".to_string()),
        api_key: api_key.clone(),
        base_url: Some("https://api.deepseek.com/v1".to_string()),
    };
    let _provider_from_config = flow_run::agent::create_llm_provider(&_config)?;

    println!("    ✅ Provider 创建成功\n");

    // 3. 构建消息
    println!("[2] 构建消息:");
    let messages = vec![
        Message::system("You are a helpful assistant. Reply in Chinese."),
        Message::user("用三句话解释什么是 Rust 的所有权系统？"),
    ];
    println!("    system: You are a helpful assistant. Reply in Chinese.");
    println!("    user: 用三句话解释什么是 Rust 的所有权系统？\n");

    // 4. 流式调用
    println!("[3] 流式调用开始（实时输出）:");
    println!("    ──────────────────────────────────");

    let stream = provider.call_stream(messages);
    let mut accumulator = String::new();
    let mut chunk_count = 0;

    tokio::pin!(stream);
    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result?;
        chunk_count += 1;

        // 实时打印增量内容
        if !chunk.delta.is_empty() {
            print!("{}", chunk.delta);
            use std::io::Write;
            std::io::stdout().flush().ok();
        }

        accumulator.push_str(&chunk.delta);

        if chunk.done {
            if let Some(usage) = &chunk.usage {
                println!();
                println!("    ──────────────────────────────────");
                println!("    Token 使用: prompt={}, completion={}, total={}",
                    usage.prompt_tokens, usage.completion_tokens, usage.total_tokens);
            }
            break;
        }
    }

    println!();
    println!("\n    ──────────────────────────────────");
    println!("\n[4] 统计信息:");
    println!("    总 chunk 数: {}", chunk_count);
    println!("    累积字符数: {}", accumulator.chars().count());
    println!("\n    完整响应:");
    println!("    {}", accumulator);
    println!();

    println!("==========================================");
    println!("  示例完成!");
    println!("==========================================");

    Ok(())
}
