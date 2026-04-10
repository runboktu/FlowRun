//! Agent 工作流示例 - DeepSeek + Shell 混合
//!
//! 这个示例展示如何：
//! - 加载包含 agent 步骤的工作流
//! - 配置环境变量（DEEPSEEK_API_KEY）
//! - 执行 Shell → Agent → Shell 的三步工作流
//! - 观察步骤间数据传递

use flow_run::core::context::ExecutionContext;
use flow_run::core::dag::{DagScheduler, Scheduler};
use flow_run::core::parser::WorkflowParser;
use flow_run::agent::BuiltinToolRegistry;
use flow_run::utils::checkpoint::CheckpointManager;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tempfile::tempdir;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("flow_run=info")
        .init();

    println!("==========================================");
    println!("  flow-run - Agent 工作流示例");
    println!("==========================================\n");

    // 1. 检查环境变量
    let api_key = std::env::var("DEEPSEEK_API_KEY")
        .or_else(|_| std::env::var("OPENAI_API_KEY"))
        .ok();

    if api_key.is_none() {
        println!("⚠️  未设置 DEEPSEEK_API_KEY 或 OPENAI_API_KEY 环境变量");
        println!("    Agent 步骤将无法调用 LLM，仅展示工作流结构分析\n");
    }

    // 2. 从文件加载工作流
    let workflow_path = Path::new("examples/16_agent_deepseek.yaml");
    println!("[1] 从文件加载工作流: {:?}", workflow_path);
    let workflow = WorkflowParser::from_file(workflow_path)?;
    println!("    ✅ 加载成功!\n");

    // 3. 显示工作流信息
    println!("[2] 工作流信息:");
    println!("    名称: {}", workflow.name);
    println!("    描述: {}", workflow.description.as_deref().unwrap_or("无"));
    println!("    步骤数: {}\n", workflow.steps.len());

    // 4. DAG 分析
    println!("[3] DAG 分析:");
    let dag = DagScheduler::new(workflow.steps.clone())?;

    match dag.has_cycle() {
        Ok(false) => println!("    ✅ 无循环依赖"),
        Ok(true) => {
            println!("    ❌ 检测到循环依赖!");
            return Err(anyhow::anyhow!("工作流包含循环依赖"));
        }
        Err(e) => {
            println!("    ❌ 检查失败: {}", e);
            return Err(e.into());
        }
    }

    println!("\n    步骤依赖关系:");
    for step in &workflow.steps {
        let deps = step.depends_on.as_ref()
            .map(|d| d.join(", "))
            .unwrap_or_else(|| "无".to_string());
        println!("      {} ({:?}) -> [{}]", step.id, step.r#type, deps);
    }

    let batches = dag.topological_sort()?;
    println!("\n    执行批次:");
    for (i, batch) in batches.iter().enumerate() {
        println!("      批次 {}: {:?}", i + 1, batch);
    }
    println!();

    // 5. 创建输入参数
    let work_dir = tempdir()?;
    let question = "用Rust 写一个 计算 1 + 2 + 3 + ... + 100 的程序";

    println!("[4] 输入参数:");
    println!("    work_dir: {:?}", work_dir.path());
    println!("    question: {}\n", question);

    let mut inputs = HashMap::new();
    inputs.insert("work_dir".to_string(), serde_json::json!("/Users/mingshu/workspace/code/ai/cli/flow-run"));
    inputs.insert("question".to_string(), serde_json::json!(question));

    // 6. 创建执行上下文
    println!("[5] 创建执行上下文");
    let context = ExecutionContext::new(&workflow, inputs);
    println!("    执行 ID: {}\n", context.execution_id);

    // 7. 创建 Scheduler
    println!("[6] 创建 Scheduler");
    let checkpoint_dir = tempdir()?;
    let checkpoint_manager = CheckpointManager::new(checkpoint_dir.path().to_path_buf())?;
    let config = workflow.config.clone().unwrap_or_default();
    let builtin_registry = Arc::new(BuiltinToolRegistry::with_defaults());
    let scheduler = Scheduler::new(dag, config, checkpoint_manager, builtin_registry);
    scheduler.set_context(context).await;
    println!("    ✅ Scheduler 创建成功\n");

    // 8. 执行工作流
    println!("[7] 执行工作流...");
    let result = scheduler.run().await?;

    // 9. 显示执行结果
    println!("[8] 执行结果:");
    println!("    状态: {:?}", result.status);
    println!("    步骤结果:");
    for step in &result.steps {
        println!("      - {}: {:?}", step.step_id, step.status);
        if let Some(output) = &step.output {
            if let Some(answer) = output.get("answer").and_then(|v| v.as_str()) {
                let preview: String = answer.chars().take(80).collect();
                println!("        answer: {}...", preview);
            }
        }
    }
    println!();

    // 10. 显示指标
    println!("[9] 执行指标:");
    println!("    总步骤: {}", result.metrics.total_steps);
    println!("    成功: {}", result.metrics.success_steps);
    println!("    失败: {}", result.metrics.failed_steps);
    println!("    耗时: {}ms\n", result.metrics.total_duration_ms);

    // 11. 显示输出
    if let Some(outputs) = &result.outputs {
        println!("[10] 工作流输出:");
        for (key, value) in outputs {
            let preview: String = value.to_string().chars().take(60).collect();
            println!("    {}: {}", key, preview);
        }
        println!();
    }

    println!("==========================================");
    println!("  示例完成!");
    println!("==========================================");

    Ok(())
}
