//! AI 作曲编曲工作流示例
//!
//! 展示如何用 FlowRunner 加载并运行 19_music_composer.yaml：
//!   Step 1: Agent 给歌词添加 Suno Tag
//!   Step 2: Agent 生成中文作曲提示词
//!   Step 3: Agent 生成英文作曲提示词（< 1000 字符）
//!   Step 4: Shell 汇总写入文件

use flow_run::FlowRunner;
use std::collections::HashMap;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("flow_run=info")
        .init();

    println!("══════════════════════════════════════════════");
    println!("  AI 作曲编曲工作流");
    println!("══════════════════════════════════════════════\n");

    let api_key = std::env::var("DEEPSEEK_API_KEY")
        .or_else(|_| std::env::var("OPENAI_API_KEY"))
        .ok();

    if api_key.is_none() {
        println!("⚠️  未设置 DEEPSEEK_API_KEY 或 OPENAI_API_KEY 环境变量");
        println!("    Agent 步骤将无法调用 LLM\n");
    }

    let runner = FlowRunner::from_file("examples/19_music_composer.yaml")?;
    println!("[1] 工作流: {} (v{})\n",
        runner.workflow().name,
        runner.workflow().version.as_deref().unwrap_or("?")
    );

    let lyrics = r#"南方的冬
总少了一片素白
常青的树
没等来雪的覆盖
故乡的雪
从视频里飘来
那片田野
想必又成了白色的海

候鸟在雨雾中寻找航线
行李箱滑过潮湿的路面
站台广播模糊了乡音的暖
玻璃窗映着三十岁的童年

记忆将雪花 变成泛黄旧片
雪球掷出笑声 在屋檐飞远
碎成星星点点 照亮异乡无眠
那片白茫茫 再也抓不回指尖

新雪覆盖旧雪 岁岁年年
埋着纸牌弹珠还有生锈的铁环
童年的口袋 装不下成年的思念
旅途中的我们 终将走散成虚线
列车穿过岁末 去向了从前
北风吹响风铃 把时光惊散
再也回不去 大雪纷飞的少年

新雪覆盖旧雪 岁岁年年
埋着纸牌蛋珠还有生锈的铁环
童年的口袋 装不下成年的思念
旅途中的我们 终将走散成虚线
列车穿过岁末 去向了从前
北风吹响风铃 把时光惊散
再也回不去 大雪纷飞的少年

列车穿过岁末 去向了从前
北风吹响风铃 把时光惊散
再也回不去 大雪纷飞的少年

列车穿过岁末 去向了从前
北风吹响风铃 把时光惊散
再也回不去 大雪纷飞的少年"#;

    let style = "heavy blues, saxophone Solo";
    let output_dir = "/tmp/music-output";

    let mut inputs = HashMap::new();
    inputs.insert("lyrics".to_string(), serde_json::json!(lyrics));
    inputs.insert("style".to_string(), serde_json::json!(style));
    inputs.insert("output_dir".to_string(), serde_json::json!(output_dir));

    println!("[2] 输入:");
    println!("    风格: {}", style);
    println!("    歌词: {}...", &lyrics[..lyrics.chars().take(30).collect::<String>().len()]);
    println!("    输出: {}\n", output_dir);

    println!("[3] 执行工作流...\n");
    let result = runner.run(inputs).await?;

    println!("[4] 执行结果:");
    println!("    状态: {:?}", result.status);
    println!("    耗时: {}ms", result.metrics.total_duration_ms);
    for step in &result.steps {
        println!("    [{}] {:?}", step.step_id, step.status);
    }
    println!();

    if let Some(outputs) = &result.outputs {
        println!("[5] 输出预览:");

        if let Some(tagged) = outputs.get("tagged_lyrics").and_then(|v| v.as_str()) {
            let preview: String = tagged.chars().take(200).collect();
            println!("\n── 歌词 Suno Tag ──\n{}\n", preview);
        }

        if let Some(cn) = outputs.get("chinese_prompt").and_then(|v| v.as_str()) {
            let preview: String = cn.chars().take(300).collect();
            println!("\n── 中文作曲提示词 ──\n{}\n", preview);
        }

        if let Some(en) = outputs.get("english_prompt").and_then(|v| v.as_str()) {
            println!("\n── 英文作曲提示词 ({} chars) ──\n{}\n", en.len(), en);
        }

        if let Some(path) = outputs.get("output_file").and_then(|v| v.as_str()) {
            println!("输出文件: {}", path);
        }
    }

    println!("\n══════════════════════════════════════════════");
    println!("  完成!");
    println!("══════════════════════════════════════════════");

    Ok(())
}
