//! 基础示例 - DAG 调度器
//!
//! 这个示例展示如何：
//! - 创建 DAG 调度器
//! - 执行拓扑排序
//! - 查看并行批次

use flow_run::core::dag::DagScheduler;
use flow_run::core::parser::WorkflowParser;
use std::path::Path;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    println!("==========================================");
    println!("  flow-run - 基础示例：DAG 调度器");
    println!("==========================================\n");

    // 加载工作流定义
    let workflow_path = Path::new("examples/03_basic_dependencies.yaml");
    let workflow = WorkflowParser::from_file(workflow_path)?;

    println!("[1] 加载工作流: {}", workflow.name);
    println!("    步骤数: {}\n", workflow.steps.len());

    // 创建 DAG 调度器
    println!("[2] 创建 DAG 调度器");
    let dag = DagScheduler::new(workflow.steps.clone())?;
    println!("    ✅ DAG 创建成功\n");

    // 检查循环依赖
    println!("[3] 检查循环依赖:");
    match dag.has_cycle() {
        Ok(false) => println!("    ✅ 无循环依赖\n"),
        Ok(true) => println!("    ❌ 检测到循环依赖!\n"),
        Err(e) => println!("    ❌ 检查失败: {}\n", e),
    }

    // 执行拓扑排序
    println!("[4] 拓扑排序结果（执行批次）:");
    let batches = dag.topological_sort()?;
    for (i, batch) in batches.iter().enumerate() {
        println!("    批次 {}: {:?}", i + 1, batch);
    }
    println!();

    // 显示依赖关系
    println!("[5] 步骤依赖关系:");
    for step in &workflow.steps {
        let deps = step.depends_on.as_ref()
            .map(|d| d.join(", "))
            .unwrap_or_else(|| "无".to_string());
        println!("    {} -> [{}]", step.id, deps);
    }
    println!();

    println!("==========================================");
    println!("  示例完成!");
    println!("==========================================");

    Ok(())
}
