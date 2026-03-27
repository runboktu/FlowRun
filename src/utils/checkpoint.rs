use crate::core::types::{StepId, StepResult};
use crate::utils::error::CheckpointError;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

/// 检查点状态枚举
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointStatus {
    /// 运行中
    Running,
    /// 已暂停
    Paused,
    /// 已完成
    Completed,
    /// 失败
    Failed,
}

/// 步骤超时信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepTimeoutInfo {
    /// 步骤 ID
    pub step_id: StepId,
    /// 步骤超时时间
    pub timeout: Duration,
    /// 步骤开始时间
    pub started_at: DateTime<Utc>,
    /// 步骤已执行时间
    pub elapsed: Duration,
}

/// 超时上下文
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutContext {
    /// 原始超时时间
    pub original_timeout: Duration,
    /// 已经过的时间
    pub elapsed_time: Duration,
    /// 剩余超时时间
    pub remaining_timeout: Duration,
    /// 每个步骤的超时信息
    pub step_timeouts: HashMap<StepId, StepTimeoutInfo>,
}

impl TimeoutContext {
    /// 创建新的超时上下文
    pub fn new(timeout: Duration) -> Self {
        Self {
            original_timeout: timeout,
            elapsed_time: Duration::zero(),
            remaining_timeout: timeout,
            step_timeouts: HashMap::new(),
        }
    }

    /// 更新已过时间并计算剩余时间
    pub fn update_elapsed(&mut self, additional_time: Duration) {
        self.elapsed_time = self.elapsed_time + additional_time;
        self.remaining_timeout = self.original_timeout - self.elapsed_time;
    }

    /// 检查是否已超时
    pub fn is_expired(&self) -> bool {
        self.remaining_timeout <= Duration::zero()
    }

    /// 记录步骤超时信息
    pub fn record_step(&mut self, step_id: StepId, timeout: Duration, started_at: DateTime<Utc>) {
        self.step_timeouts.insert(
            step_id.clone(),
            StepTimeoutInfo {
                step_id,
                timeout,
                started_at,
                elapsed: Duration::zero(),
            },
        );
    }

    /// 更新步骤执行时间
    pub fn update_step_elapsed(&mut self, step_id: &StepId, additional_time: Duration) {
        if let Some(step_info) = self.step_timeouts.get_mut(step_id) {
            step_info.elapsed = step_info.elapsed + additional_time;
        }
    }

    /// 检查步骤是否已超时
    pub fn is_step_expired(&self, step_id: &StepId) -> bool {
        if let Some(step_info) = self.step_timeouts.get(step_id) {
            step_info.elapsed > step_info.timeout
        } else {
            false
        }
    }
}

/// 检查点结构体
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    /// 检查点 ID
    pub id: String,
    /// 工作流 ID
    pub workflow_id: String,
    /// 工作流名称
    pub workflow_name: String,
    /// 工作流开始时间
    pub started_at: DateTime<Utc>,
    /// 检查点创建时间
    pub checkpoint_at: DateTime<Utc>,
    /// 检查点状态
    pub status: CheckpointStatus,
    /// 已完成的步骤集合
    pub completed_steps: HashSet<StepId>,
    /// 失败的步骤集合
    pub failed_steps: HashSet<StepId>,
    /// 当前批次
    pub current_batch: usize,
    /// 步骤输出结果
    pub step_outputs: HashMap<StepId, StepResult>,
    /// 工作流变量
    pub variables: HashMap<String, serde_json::Value>,
    /// 超时配置上下文
    pub timeout_config: TimeoutContext,
}

impl Checkpoint {
    /// 创建新的检查点
    pub fn new(
        workflow_id: String,
        workflow_name: String,
        started_at: DateTime<Utc>,
        timeout: Duration,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: String::new(),
            workflow_id,
            workflow_name,
            started_at,
            checkpoint_at: now,
            status: CheckpointStatus::Running,
            completed_steps: HashSet::new(),
            failed_steps: HashSet::new(),
            current_batch: 0,
            step_outputs: HashMap::new(),
            variables: HashMap::new(),
            timeout_config: TimeoutContext::new(timeout),
        }
    }

    /// 标记步骤为已完成
    pub fn mark_step_completed(&mut self, step_id: StepId) {
        self.completed_steps.insert(step_id);
    }

    /// 标记步骤为失败
    pub fn mark_step_failed(&mut self, step_id: StepId) {
        self.failed_steps.insert(step_id);
    }

    /// 记录步骤输出
    pub fn record_step_output(&mut self, step_id: StepId, output: StepResult) {
        self.step_outputs.insert(step_id, output);
    }

    /// 设置变量
    pub fn set_variable(&mut self, key: String, value: serde_json::Value) {
        self.variables.insert(key, value);
    }

    /// 获取变量
    pub fn get_variable(&self, key: &str) -> Option<&serde_json::Value> {
        self.variables.get(key)
    }

    /// 增加当前批次
    pub fn increment_batch(&mut self) {
        self.current_batch += 1;
    }

    /// 更新检查点时间戳
    pub fn update_timestamp(&mut self) {
        self.checkpoint_at = Utc::now();
    }

    /// 更新超时上下文
    pub fn update_timeout(&mut self, elapsed_time: Duration) {
        self.timeout_config.update_elapsed(elapsed_time);
    }

    /// 获取步骤输出
    pub fn get_step_output(&self, step_id: &StepId) -> Option<&StepResult> {
        self.step_outputs.get(step_id)
    }

    /// 检查步骤是否已完成
    pub fn is_step_completed(&self, step_id: &StepId) -> bool {
        self.completed_steps.contains(step_id)
    }

    /// 检查步骤是否已失败
    pub fn is_step_failed(&self, step_id: &StepId) -> bool {
        self.failed_steps.contains(step_id)
    }
}

/// 检查点管理器
#[derive(Debug, Clone)]
pub struct CheckpointManager {
    /// 检查点文件基础目录
    pub base_dir: PathBuf,
}

impl CheckpointManager {
    /// 创建新的检查点管理器
    pub fn new(base_dir: PathBuf) -> Result<Self, CheckpointError> {
        fs::create_dir_all(&base_dir)?;
        Ok(Self { base_dir })
    }

    /// 生成唯一的检查点 ID
    pub fn generate_id() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    /// 获取检查点文件路径
    fn get_checkpoint_path(&self, checkpoint_id: &str) -> PathBuf {
        self.base_dir.join(format!("{}.json", checkpoint_id))
    }

    /// 保存检查点
    pub fn save(&self, context: &mut Checkpoint) -> Result<String, CheckpointError> {
        if context.id.is_empty() {
            context.id = Self::generate_id();
        }

        context.update_timestamp();

        let checkpoint_path = self.get_checkpoint_path(&context.id);
        let json = serde_json::to_string_pretty(context)?;

        fs::write(&checkpoint_path, json)?;

        tracing::info!("检查点已保存: {}", context.id);

        Ok(context.id.clone())
    }

    /// 加载检查点
    pub fn load(&self, checkpoint_id: &str) -> Result<Checkpoint, CheckpointError> {
        let checkpoint_path = self.get_checkpoint_path(checkpoint_id);

        if !checkpoint_path.exists() {
            return Err(CheckpointError::NotFound(checkpoint_id.to_string()));
        }

        let json = fs::read_to_string(&checkpoint_path)?;
        let checkpoint: Checkpoint = serde_json::from_str(&json)?;

        tracing::info!("检查点已加载: {}", checkpoint_id);

        Ok(checkpoint)
    }

    /// 列出所有检查点
    pub fn list(&self) -> Result<Vec<String>, CheckpointError> {
        let mut checkpoints = Vec::new();

        let entries = fs::read_dir(&self.base_dir)?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    checkpoints.push(stem.to_string());
                }
            }
        }

        checkpoints.sort();
        checkpoints.reverse();

        tracing::info!("找到 {} 个检查点", checkpoints.len());

        Ok(checkpoints)
    }

    /// 删除检查点
    pub fn delete(&self, checkpoint_id: &str) -> Result<(), CheckpointError> {
        let checkpoint_path = self.get_checkpoint_path(checkpoint_id);

        if !checkpoint_path.exists() {
            return Err(CheckpointError::NotFound(checkpoint_id.to_string()));
        }

        fs::remove_file(&checkpoint_path)?;

        tracing::info!("检查点已删除: {}", checkpoint_id);

        Ok(())
    }

    /// 列出检查点详细信息
    pub fn list_with_info(&self) -> Result<Vec<CheckpointInfo>, CheckpointError> {
        let checkpoint_ids = self.list()?;
        let mut infos = Vec::new();

        for id in checkpoint_ids {
            match self.load(&id) {
                Ok(checkpoint) => {
                    infos.push(CheckpointInfo {
                        id: checkpoint.id.clone(),
                        workflow_id: checkpoint.workflow_id,
                        workflow_name: checkpoint.workflow_name,
                        status: checkpoint.status,
                        started_at: checkpoint.started_at,
                        checkpoint_at: checkpoint.checkpoint_at,
                        completed_steps: checkpoint.completed_steps.len(),
                        failed_steps: checkpoint.failed_steps.len(),
                        current_batch: checkpoint.current_batch,
                    });
                }
                Err(e) => {
                    tracing::warn!("无法加载检查点 {}: {:?}", id, e);
                }
            }
        }

        Ok(infos)
    }
}

/// 检查点信息摘要
#[derive(Debug, Clone)]
pub struct CheckpointInfo {
    /// 检查点 ID
    pub id: String,
    /// 工作流 ID
    pub workflow_id: String,
    /// 工作流名称
    pub workflow_name: String,
    /// 检查点状态
    pub status: CheckpointStatus,
    /// 工作流开始时间
    pub started_at: DateTime<Utc>,
    /// 检查点时间
    pub checkpoint_at: DateTime<Utc>,
    /// 已完成步骤数
    pub completed_steps: usize,
    /// 失败步骤数
    pub failed_steps: usize,
    /// 当前批次
    pub current_batch: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timeout_context_new() {
        let timeout = Duration::seconds(60);
        let context = TimeoutContext::new(timeout);

        assert_eq!(context.original_timeout, timeout);
        assert_eq!(context.elapsed_time, Duration::zero());
        assert_eq!(context.remaining_timeout, timeout);
        assert!(context.step_timeouts.is_empty());
    }

    #[test]
    fn test_timeout_context_update_elapsed() {
        let timeout = Duration::seconds(60);
        let mut context = TimeoutContext::new(timeout);

        context.update_elapsed(Duration::seconds(10));

        assert_eq!(context.elapsed_time, Duration::seconds(10));
        assert_eq!(context.remaining_timeout, Duration::seconds(50));
        assert!(!context.is_expired());

        context.update_elapsed(Duration::seconds(50));

        assert_eq!(context.elapsed_time, Duration::seconds(60));
        assert_eq!(context.remaining_timeout, Duration::zero());
        assert!(context.is_expired());
    }

    #[test]
    fn test_timeout_context_step_timeout() {
        let timeout = Duration::seconds(60);
        let mut context = TimeoutContext::new(timeout);

        let step_id = "step1".to_string();
        let step_timeout = Duration::seconds(10);
        let started_at = Utc::now();

        context.record_step(step_id.clone(), step_timeout, started_at);

        assert!(!context.is_step_expired(&step_id));

        context.update_step_elapsed(&step_id, Duration::seconds(5));
        assert!(!context.is_step_expired(&step_id));

        context.update_step_elapsed(&step_id, Duration::seconds(6));
        assert!(context.is_step_expired(&step_id));
    }

    #[test]
    fn test_checkpoint_new() {
        let workflow_id = "wf1".to_string();
        let workflow_name = "Test Workflow".to_string();
        let started_at = Utc::now();
        let timeout = Duration::seconds(60);

        let checkpoint = Checkpoint::new(
            workflow_id.clone(),
            workflow_name.clone(),
            started_at,
            timeout,
        );

        assert!(checkpoint.id.is_empty());
        assert_eq!(checkpoint.workflow_id, workflow_id);
        assert_eq!(checkpoint.workflow_name, workflow_name);
        assert_eq!(checkpoint.started_at, started_at);
        assert_eq!(checkpoint.status, CheckpointStatus::Running);
        assert!(checkpoint.completed_steps.is_empty());
        assert!(checkpoint.failed_steps.is_empty());
        assert_eq!(checkpoint.current_batch, 0);
    }

    #[test]
    fn test_checkpoint_mark_steps() {
        let mut checkpoint = Checkpoint::new(
            "wf1".to_string(),
            "Test Workflow".to_string(),
            Utc::now(),
            Duration::seconds(60),
        );

        checkpoint.mark_step_completed("step1".to_string());
        checkpoint.mark_step_completed("step2".to_string());
        checkpoint.mark_step_failed("step3".to_string());

        assert!(checkpoint.is_step_completed(&"step1".to_string()));
        assert!(checkpoint.is_step_completed(&"step2".to_string()));
        assert!(!checkpoint.is_step_completed(&"step3".to_string()));

        assert!(!checkpoint.is_step_failed(&"step1".to_string()));
        assert!(!checkpoint.is_step_failed(&"step2".to_string()));
        assert!(checkpoint.is_step_failed(&"step3".to_string()));

        assert_eq!(checkpoint.completed_steps.len(), 2);
        assert_eq!(checkpoint.failed_steps.len(), 1);
    }

    #[test]
    fn test_checkpoint_variables() {
        let mut checkpoint = Checkpoint::new(
            "wf1".to_string(),
            "Test Workflow".to_string(),
            Utc::now(),
            Duration::seconds(60),
        );

        checkpoint.set_variable("key1".to_string(), serde_json::json!("value1"));
        checkpoint.set_variable("key2".to_string(), serde_json::json!(42));

        assert_eq!(
            checkpoint.get_variable("key1"),
            Some(&serde_json::json!("value1"))
        );
        assert_eq!(
            checkpoint.get_variable("key2"),
            Some(&serde_json::json!(42))
        );
        assert_eq!(checkpoint.get_variable("key3"), None);
    }

    #[test]
    fn test_checkpoint_manager_save_and_load() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = CheckpointManager::new(temp_dir.path().to_path_buf()).unwrap();

        let mut checkpoint = Checkpoint::new(
            "wf1".to_string(),
            "Test Workflow".to_string(),
            Utc::now(),
            Duration::seconds(60),
        );
        checkpoint.mark_step_completed("step1".to_string());
        checkpoint.set_variable("key1".to_string(), serde_json::json!("value1"));

        let checkpoint_id = manager.save(&mut checkpoint).unwrap();
        assert!(!checkpoint_id.is_empty());

        let loaded_checkpoint = manager.load(&checkpoint_id).unwrap();
        assert_eq!(loaded_checkpoint.id, checkpoint_id);
        assert_eq!(loaded_checkpoint.workflow_id, "wf1");
        assert!(loaded_checkpoint.is_step_completed(&"step1".to_string()));
        assert_eq!(
            loaded_checkpoint.get_variable("key1"),
            Some(&serde_json::json!("value1"))
        );
    }

    #[test]
    fn test_checkpoint_manager_list() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = CheckpointManager::new(temp_dir.path().to_path_buf()).unwrap();

        let mut checkpoint1 = Checkpoint::new(
            "wf1".to_string(),
            "Workflow 1".to_string(),
            Utc::now(),
            Duration::seconds(60),
        );
        checkpoint1.mark_step_completed("step1".to_string());
        manager.save(&mut checkpoint1).unwrap();

        let mut checkpoint2 = Checkpoint::new(
            "wf2".to_string(),
            "Workflow 2".to_string(),
            Utc::now(),
            Duration::seconds(60),
        );
        checkpoint2.mark_step_completed("step2".to_string());
        manager.save(&mut checkpoint2).unwrap();

        let checkpoints = manager.list().unwrap();
        assert_eq!(checkpoints.len(), 2);
    }

    #[test]
    fn test_checkpoint_manager_delete() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = CheckpointManager::new(temp_dir.path().to_path_buf()).unwrap();

        let mut checkpoint = Checkpoint::new(
            "wf1".to_string(),
            "Test Workflow".to_string(),
            Utc::now(),
            Duration::seconds(60),
        );
        let checkpoint_id = manager.save(&mut checkpoint).unwrap();

        assert!(manager.load(&checkpoint_id).is_ok());

        manager.delete(&checkpoint_id).unwrap();

        assert!(manager.load(&checkpoint_id).is_err());
    }

    #[test]
    fn test_checkpoint_manager_load_not_found() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = CheckpointManager::new(temp_dir.path().to_path_buf()).unwrap();

        let result = manager.load("nonexistent");
        assert!(matches!(result, Err(CheckpointError::NotFound(_))));
    }

    #[test]
    fn test_checkpoint_manager_delete_not_found() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = CheckpointManager::new(temp_dir.path().to_path_buf()).unwrap();

        let result = manager.delete("nonexistent");
        assert!(matches!(result, Err(CheckpointError::NotFound(_))));
    }
}
