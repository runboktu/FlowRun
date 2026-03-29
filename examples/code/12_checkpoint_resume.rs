use chrono::Duration;
use flow_run::core::context::ExecutionContext;
use flow_run::core::dag::{DagScheduler, Scheduler};
use flow_run::core::parser::WorkflowParser;
use flow_run::core::types::OnFailureStrategy;
use flow_run::utils::checkpoint::{Checkpoint, CheckpointManager, TimeoutContext};
use std::collections::HashMap;
use std::path::Path;
use tempfile::tempdir;

const FLAG_FILE: &str = "/tmp/checkpoint_resume_flag";
const DATA_FILE: &str = "/tmp/checkpoint_demo_data.txt";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("flow_run=info")
        .init();

    println!("==========================================");
    println!("  flow-run - Example 12: Checkpoint Resume");
    println!("==========================================\n");

    cleanup();

    let workflow_path = Path::new("examples/12_checkpoint_resume.yaml");
    println!("[加载] {:?}", workflow_path);
    let workflow = WorkflowParser::from_file(workflow_path)?;
    println!("  名称: {}", workflow.name);
    println!("  描述: {}", workflow.description.as_deref().unwrap_or("-"));

    let dag = DagScheduler::new(workflow.steps.clone())?;
    let batches = dag.topological_sort()?;
    println!("  步骤数: {}", workflow.steps.len());

    println!("\n[工作流结构]");
    for (i, batch) in batches.iter().enumerate() {
        let hint = if batch.len() > 1 { " (并行)" } else { " (串行)" };
        println!("  批次 {}: {:?}{}", i, batch, hint);
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
    println!("  阶段一：第一次运行（执行到一半失败）");
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

    println!("\n[3] 运行结果: {:?}", result1.status);
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
    println!("  阶段二：检查失败前的检查点");
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
    println!("    已完成: {:?}", cp.completed_steps);
    println!("    恢复后从 batch {} 之后继续\n", cp.current_batch);

    // ==========================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  阶段三：修复问题并 resume");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    println!("[6] 创建 FLAG 文件（模拟修复外部条件）");
    std::fs::write(FLAG_FILE, "ready")?;
    println!("    echo 'ready' > {}\n", FLAG_FILE);

    println!("[7] resume({}...)...", &good_checkpoint.id[..8]);
    let context2 = ExecutionContext::new(&workflow, HashMap::new());
    let scheduler2 = Scheduler::new(
        DagScheduler::new(workflow.steps.clone())?,
        config.clone(),
        CheckpointManager::new(checkpoint_dir.clone())?,
    );
    scheduler2.set_context(context2).await;

    let result2 = scheduler2.resume(&good_checkpoint.id).await?;

    println!("\n[8] 恢复后结果: {:?}", result2.status);
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
    println!("\n    assert 通过: 工作流恢复后成功完成 ✅\n");

    // ==========================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  阶段四：CheckpointManager 手动 API");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    println!("[9] 手动创建/保存/加载/验证");
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
    println!("    save -> load -> assert ✅\n");

    println!("[10] TimeoutContext");
    let mut tc = TimeoutContext::new(Duration::seconds(30));
    tc.record_step("x".into(), Duration::seconds(10), chrono::Utc::now());
    tc.update_step_elapsed(&"x".into(), Duration::seconds(8));
    println!("    step x: 8/10s, expired={}", tc.is_step_expired(&"x".into()));
    tc.update_step_elapsed(&"x".into(), Duration::seconds(3));
    println!("    step x: 11/10s, expired={}", tc.is_step_expired(&"x".into()));
    println!();

    println!("[11] 删除检查点");
    let before = checkpoint_manager.list()?.len();
    checkpoint_manager.delete(&id)?;
    let after = checkpoint_manager.list()?.len();
    println!("    {} -> {}\n", before, after);

    // cleanup();

    println!("==========================================");
    println!("  示例完成!");
    println!("==========================================");
    Ok(())
}

fn cleanup() {
    std::fs::remove_file(FLAG_FILE).ok();
    std::fs::remove_file(DATA_FILE).ok();
}
