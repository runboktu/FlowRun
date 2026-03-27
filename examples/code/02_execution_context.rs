//! 基础示例 - 创建执行上下文
//!
//! 这个示例展示如何：
//! - 创建 ExecutionContext
//! - 设置输入参数
//! - 使用模板表达式求值

use flow_run::core::context::ExecutionContext;
use flow_run::core::parser::WorkflowParser;
use std::collections::HashMap;
use std::path::Path;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    println!("==========================================");
    println!("  flow-run - 基础示例：执行上下文");
    println!("==========================================\n");

    // 加载工作流定义
    let workflow_path = Path::new("examples/01_basic_http.yaml");
    let workflow = WorkflowParser::from_file(workflow_path)?;

    // 创建输入参数
    let mut inputs = HashMap::new();
    inputs.insert(
        "api_url".to_string(),
        serde_json::json!("https://jsonplaceholder.typicode.com"),
    );

    // 创建执行上下文
    println!("[1] 创建执行上下文");
    let mut context = ExecutionContext::new(&workflow, inputs);
    println!("    执行 ID: {}", context.execution_id);
    println!("    工作流: {}", context.workflow_name);
    println!("    开始时间: {}\n", context.started_at);

    // 显示输入参数
    println!("[2] 输入参数:");
    for (key, value) in &context.inputs {
        println!("    {}: {}", key, value);
    }
    println!();

    // 模板表达式求值
    println!("[3] 模板表达式求值:");
    let expressions = vec![
        "${{ inputs.api_url }}",
        "${{ inputs.api_url }}/users/1",
        "${{ inputs.api_url || 'https://default.api.com' }}",
    ];

    for expr in expressions {
        match context.evaluate(expr) {
            Ok(value) => println!("    {} -> {}", expr, value),
            Err(e) => println!("    {} -> 错误: {}", expr, e),
        }
    }
    println!();

    // 设置变量
    println!("[4] 设置变量:");
    context.set_variable("version".to_string(), serde_json::json!("1.0.0"));
    context.set_variable("env".to_string(), serde_json::json!("production"));
    println!("    version: {:?}", context.get_variable("version"));
    println!("    env: {:?}", context.get_variable("env"));
    println!();

    println!("==========================================");
    println!("  示例完成!");
    println!("==========================================");

    Ok(())
}
