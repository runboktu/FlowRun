use chrono::Duration;
use flow_run::core::context::ExecutionContext;
use flow_run::core::dag::{DagScheduler, Scheduler};
use flow_run::core::parser::WorkflowParser;
use flow_run::core::types::OnFailureStrategy;
use flow_run::utils::checkpoint::{Checkpoint, CheckpointManager, TimeoutContext};
use std::collections::HashMap;
use tempfile::tempdir;

const FLAG_FILE: &str = "/tmp/checkpoint_resume_flag";
const DATA_FILE: &str = "/tmp/checkpoint_demo_data.txt";

const WORKFLOW_YAML: &str = r#"
name: "Checkpoint Resume 演示"
description: "工作流执行到一半失败，从检查点恢复后成功完成"

steps:
  - id: prepare_data
    type: shell
    run: |
      echo "[prepare_data] 准备数据..."
      echo "prepared_at=$(date +%s)" > /tmp/checkpoint_demo_data.txt
      echo "  -> 数据文件已创建"

  - id: process_data
    type: shell
    depends_on: [prepare_data]
    run: |
      echo "[process_data] 处理数据（需要外部条件满足）..."
      if [ -f /tmp/checkpoint_resume_flag ]; then
        echo "  -> 条件满足，继续处理"
        echo "processed=true" >> /tmp/checkpoint_demo_data.txt
        echo "  -> 处理完成"
      else
        echo "  -> 条件不满足（缺少 flag 文件），模拟失败"
        rm -f /tmp/checkpoint_demo_data.txt
        exit 1
      fi

  - id: validate_result
    type: shell
    depends_on: [process_data]
    run: |
      echo "[validate_result] 验证处理结果..."
      echo "  -> 数据内容:"
      cat /tmp/checkpoint_demo_data.txt
      echo "  -> 验证通过"

  - id: cleanup
    type: shell
    depends_on: [validate_result]
    run: |
      echo "[cleanup] 清理临时文件..."
      rm -f /tmp/checkpoint_demo_data.txt /tmp/checkpoint_resume_flag
      echo "  -> 清理完成，工作流结束！"

config:
  on_failure: pause
"#;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("flow_run=info")
        .init();

    println!("==========================================");
    println!("  flow-run - Checkpoint Resume 实战示例");
    println!("==========================================\n");

    cleanup();

    let workflow = WorkflowParser::from_str(WORKFLOW_YAML)?;
    let dag = DagScheduler::new(workflow.steps.clone())?;
    let batches = dag.topological_sort()?;

    println!("[工作流结构]");
    for (i, batch) in batches.iter().enumerate() {
        println!("  批次 {}: {:?}", i, batch);
    }
    println!();

    let temp_dir = tempdir()?;
    let checkpoint_dir = temp_dir.path().join("checkpoints");
    let checkpoint_manager = CheckpointManager::new(checkpoint_dir.clone())?;

    let mut config = workflow.config.clone().unwrap_or_default();
    config.checkpoint = Some(checkpoint_dir.to_string_lossy().to_string());
    config.on_failure = Some(OnFailureStrategy::Pause);

    // ==========================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  第一次运行：执行到一半失败（on_failure: pause）");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    println!("[1] FLAG 文件不存在 -> process_data 会失败\n");

    let context = ExecutionContext::new(&workflow, HashMap::new());
    let scheduler1 = Scheduler::new(
        DagScheduler::new(workflow.steps.clone())?,
        config.clone(),
        CheckpointManager::new(checkpoint_dir.clone())?,
    );
    scheduler1.set_context(context).await;

    println!("[2] 执行工作流...");
    let result1 = scheduler1.run().await?;

    println!("\n[3] 第一次运行结果:");
    println!("    状态: {:?}", result1.status);
    for step in &result1.steps {
        let icon = match step.status {
            flow_run::core::types::StepStatus::Success => "OK",
            flow_run::core::types::StepStatus::Failed => "FAIL",
            _ => "?",
        };
        println!("      [{}] {} ({:?})", icon, step.step_id, step.status);
    }

    // ==========================================================
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  查看检查点：找到失败前的快照");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let infos = checkpoint_manager.list_with_info()?;
    println!("[4] 已保存 {} 个检查点:", infos.len());
    for info in &infos {
        println!("    {}... | batch={} | ok={} fail={}",
            &info.id[..8], info.current_batch,
            info.completed_steps, info.failed_steps);
    }

    let good_checkpoint = infos
        .iter()
        .find(|i| i.failed_steps == 0)
        .expect("应该有一个全部成功的检查点（失败前的批次）");
    println!("\n[5] 选择恢复点: {}... (batch={}, 无失败步骤)",
        &good_checkpoint.id[..8], good_checkpoint.current_batch);

    let cp = checkpoint_manager.load(&good_checkpoint.id)?;
    println!("    已完成步骤: {:?}", cp.completed_steps);
    println!("    未执行步骤: 将从 batch {} 之后的批次继续",
        cp.current_batch);
    println!();

    // ==========================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  修复问题并从检查点恢复");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    println!("[6] 创建 FLAG 文件（模拟修复外部条件）");
    std::fs::write(FLAG_FILE, "ready")?;
    println!("    -> echo 'ready' > {}\n", FLAG_FILE);

    println!("[7] 创建新的 Scheduler 并 resume...");
    let context2 = ExecutionContext::new(&workflow, HashMap::new());
    let scheduler2 = Scheduler::new(
        DagScheduler::new(workflow.steps.clone())?,
        config.clone(),
        CheckpointManager::new(checkpoint_dir.clone())?,
    );
    scheduler2.set_context(context2).await;

    let result2 = scheduler2.resume(&good_checkpoint.id).await?;

    println!("\n[8] 恢复后运行结果:");
    println!("    状态: {:?}", result2.status);
    for step in &result2.steps {
        let icon = match step.status {
            flow_run::core::types::StepStatus::Success => "OK",
            flow_run::core::types::StepStatus::Failed => "FAIL",
            _ => "?",
        };
        println!("      [{}] {} ({:?})", icon, step.step_id, step.status);
    }
    println!("    总步骤: {}  成功: {}  失败: {}",
        result2.metrics.total_steps,
        result2.metrics.success_steps,
        result2.metrics.failed_steps);

    assert!(matches!(result2.status, flow_run::core::types::WorkflowStatus::Success),
        "恢复后工作流应该成功");
    println!("\n    断言通过: 工作流恢复后成功完成! ✅\n");

    // ==========================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  CheckpointManager 手动 API 补充演示");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    println!("[9] 手动创建检查点");
    let mut manual = Checkpoint::new(
        "manual_demo".into(), "手动演示".into(),
        chrono::Utc::now(), Duration::seconds(60),
    );
    manual.mark_step_completed("step_a".into());
    manual.set_variable("count".into(), serde_json::json!(42));
    manual.update_timeout(Duration::seconds(10));
    let id = checkpoint_manager.save(&mut manual)?;
    let loaded = checkpoint_manager.load(&id)?;
    assert!(loaded.is_step_completed(&"step_a".into()));
    assert_eq!(loaded.get_variable("count"), Some(&serde_json::json!(42)));
    assert_eq!(loaded.timeout_config.remaining_timeout, Duration::seconds(50));
    println!("    save -> load -> assert 通过 ✅\n");

    println!("[10] TimeoutContext");
    let mut tc = TimeoutContext::new(Duration::seconds(30));
    tc.record_step("x".into(), Duration::seconds(10), chrono::Utc::now());
    tc.update_step_elapsed(&"x".into(), Duration::seconds(8));
    println!("    step x: 8/10s elapsed, expired={}", tc.is_step_expired(&"x".into()));
    tc.update_step_elapsed(&"x".into(), Duration::seconds(3));
    println!("    step x: 11/10s elapsed, expired={}", tc.is_step_expired(&"x".into()));
    println!();

    println!("[11] 清理检查点");
    let before = checkpoint_manager.list()?.len();
    checkpoint_manager.delete(&id)?;
    let after = checkpoint_manager.list()?.len();
    println!("    {} -> {} (已删除手动检查点)\n", before, after);

    cleanup();

    println!("==========================================");
    println!("  示例完成!");
    println!("==========================================");
    Ok(())
}

fn cleanup() {
    std::fs::remove_file(FLAG_FILE).ok();
    std::fs::remove_file(DATA_FILE).ok();
}
