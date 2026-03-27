//! 中级示例 - 模板引擎
//!
//! 这个示例展示如何：
//! - 使用模板表达式求值
//! - 应用过滤器链
//! - 处理条件表达式

use flow_run::core::template::TemplateEngine;
use serde_json::json;
use std::collections::HashMap;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    println!("==========================================");
    println!("  flow-run - 中级示例：模板引擎");
    println!("==========================================\n");

    // 创建模板引擎
    let engine = TemplateEngine::new();

    // 创建测试上下文
    let mut context = HashMap::new();
    context.insert("inputs".to_string(), json!({
        "app_name": "myapp",
        "environment": "staging",
        "version": "v1.2.3"
    }));
    context.insert("steps".to_string(), json!({
        "deploy": {
            "response": {
                "body": {
                    "url": "https://myapp.example.com",
                    "id": "deploy_123"
                }
            }
        }
    }));
    context.insert("variables".to_string(), json!({
        "items": ["apple", "banana", "cherry"],
        "users": [
            {"name": "Alice", "status": "active"},
            {"name": "Bob", "status": "inactive"}
        ]
    }));

    // 基本路径访问
    println!("[1] 基本路径访问:");
    let paths = vec![
        "inputs.app_name",
        "inputs.environment",
        "steps.deploy.response.body.url",
        "variables.items[0]",
        "variables.users[1].name",
    ];
    for path in paths {
        let result = engine.evaluate(path, &context)?;
        println!("    {} -> {}", path, result);
    }
    println!();

    // 过滤器链
    println!("[2] 过滤器链:");
    let filters = vec![
        ("inputs.app_name | uppercase", "大写转换"),
        ("inputs.app_name | lowercase", "小写转换"),
        ("inputs.app_name | truncate(3)", "截断字符串"),
        ("variables.items | length", "数组长度"),
        ("variables.items | join(', ')", "数组拼接"),
        ("variables.items | slice(0, 2)", "数组切片"),
        ("variables.items | first", "首元素"),
    ];
    for (expr, desc) in filters {
        let result = engine.evaluate(expr, &context)?;
        println!("    {} ({}): {}", expr, desc, result);
    }
    println!();

    // 条件表达式
    println!("[3] 条件表达式:");
    let conditions = vec![
        "inputs.environment == 'staging'",
        "inputs.environment == 'production'",
        "inputs.missing || 'default_value'",
    ];
    for expr in conditions {
        let result = engine.evaluate(expr, &context)?;
        println!("    {} -> {}", expr, result);
    }
    println!();

    // 完整模板解析
    println!("[4] 完整模板解析:");
    let templates = vec![
        "Deploying ${{inputs.app_name}} version ${{inputs.version}}",
        "URL: ${{steps.deploy.response.body.url}}",
        "Items: ${{variables.items | join(', ')}}",
    ];
    for template in templates {
        let result = engine.resolve_template(template, &context)?;
        println!("    模板: {}", template);
        println!("    结果: {}\n", result);
    }

    println!("==========================================");
    println!("  示例完成!");
    println!("==========================================");

    Ok(())
}
