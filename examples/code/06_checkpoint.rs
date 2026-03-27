//! 高级示例 - 检查点管理
//!
//! 这个示例展示如何：
//! - 创建检查点
//! - 保存和加载检查点
//! - 从检查点恢复执行

use flow_run::utils::checkpoint::{Checkpoint, CheckpointManager, CheckpointStatus};
use flow_run::core::types::{StepResult, StepStatus, StepError};
use chrono::{Utc, Duration as ChronoDuration};
use std::collections::{HashMap, HashSet};
use tempfile::tempdir;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    println!("==========================================");
    println!("  flow-run - 高级示例：检查点管理");
    println!("==========================================\n");

    // 创建临时目录用于存储检查点
    let temp_dir = tempdir()?;
    let checkpoint_dir = temp_dir.path().to_path_buf();
    println!("[1] 检查点目录: {:?}\n", checkpoint_dir);

    // 创建检查点管理器
    let manager = CheckpointManager::new(checkpoint_dir)?;

    // 创建检查点
    println!("[2] 创建检查点:");
    let mut checkpoint = Checkpoint::new(
        "workflow-001".to_string(),
        "测试工作流".to_string(),
        Utc::now(),
        ChronoDuration::seconds(300),
    );
    println!("    ID: {}", checkpoint.id);
    println!("    工作流: {}", checkpoint.workflow_name);
    println!("    状态: {:?}\n", checkpoint.status);

    // 模拟步骤执行
    println!("[3] 模拟步骤执行:");
    let step1 = StepResult::success("step1", serde_json::json!({"data": "result1"}));
    let step2 = StepResult::success("step2", serde_json::json!({"data": "result2"}));
    let step3 = StepResult::failed("step3", StepError {
        code: "ERROR_001".to_string(),
        message: "步骤执行失败".to_string(),
        fix: Some("检查输入参数".to_string()),
    });

    checkpoint.mark_step_completed("step1".to_string());
    checkpoint.mark_step_completed("step2".to_string());
    checkpoint.mark_step_failed("step3".to_string());
    checkpoint.record_step_output("step1".to_string(), step1);
    checkpoint.record_step_output("step2".to_string(), step2);
    checkpoint.record_step_output("step3".to_string(), step3);

    println!("    已完成步骤: {:?}", checkpoint.completed_steps);
    println!("    失败步骤: {:?}\n", checkpoint.failed_steps);

    // 设置变量
    println!("[4] 设置变量:");
    checkpoint.set_variable("version".to_string(), serde_json::json!("1.0.0"));
    checkpoint.set_variable("env".to_string(), serde_json::json!("production"));
    println!("    version: {:?}", checkpoint.get_variable("version"));
    println!("    env: {:?}\n", checkpoint.get_variable("env"));

    // 保存检查点
    println!("[5] 保存检查点:");
    let checkpoint_id = manager.save(&mut checkpoint)?;
    println!("    ✅ 保存成功: {}\n", checkpoint_id);

    // 列出检查点
    println!("[6] 列出所有检查点:");
    let checkpoints = manager.list()?;
    for id in &checkpoints {
        println!("    - {}", id);
    }
    println!();

    // 加载检查点
    println!("[7] 加载检查点:");
    let loaded = manager.load(&checkpoint_id)?;
    println!("    ID: {}", loaded.id);
    println!("    工作流: {}", loaded.workflow_name);
    println!("    状态: {:?}", loaded.status);
    println!("    已完成步骤: {:?}", loaded.completed_steps);
    println!("    失败步骤: {:?}", loaded.failed_steps);
    println!();

    // 恢复执行（从第 4 步开始）
    println!("[8] 恢复执行:");
    let resume_from = loaded.current_batch + 1;
    println!("    从批次 {} 恢复执行", resume_from);
    println!("    已有输出: {} 个步骤", loaded.step_outputs.len());
    println!();

    // 更新检查点
    println!("[9] 更新检查点:");
    checkpoint.current_batch = 3;
    checkpoint.status = CheckpointStatus::Running;
    checkpoint.set_variable("updated".to_string(), serde_json::json!(true));
    manager.save(&mut checkpoint)?;
    println!("    ✅ 检查点已更新\n");

    // 删除检查点
    println!("[10] 删除检查点:");
    manager.delete(&checkpoint_id)?;
    println!("    ✅ 检查点已删除\n");

    println!("==========================================");
    println!("  示例完成!");
    println!("==========================================");

    Ok(())
}
