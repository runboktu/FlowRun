use crate::core::types::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// 运行上下文快照 — 失败时自动保存，供 `--from-step` 恢复
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunContext {
    /// 工作流文件绝对路径
    pub workflow_file: String,
    /// 保存时间
    pub saved_at: DateTime<Utc>,
    /// 输入参数
    pub inputs: HashMap<String, serde_json::Value>,
    /// 工作流变量
    pub variables: HashMap<String, serde_json::Value>,
    /// 所有已完成步骤的输出
    pub step_outputs: HashMap<StepId, StepResult>,
    /// 失败步骤信息（None 表示工作流成功完成）
    pub failed_step: Option<FailedStepInfo>,
}

/// 失败步骤信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailedStepInfo {
    /// 失败的步骤 ID
    pub step_id: String,
    /// 错误代码
    pub error_code: String,
    /// 错误消息
    pub error_message: String,
    /// 修复建议
    pub fix_suggestion: Option<String>,
}

impl RunContext {
    /// 保存运行上下文到文件
    ///
    /// 存储位置: `/tmp/flow-run-contexts/<hash>.json`
    /// 同一个工作流文件只保留一份最新上下文（覆盖写）
    pub fn save(
        workflow_file: &Path,
        inputs: HashMap<String, serde_json::Value>,
        variables: HashMap<String, serde_json::Value>,
        step_outputs: HashMap<StepId, StepResult>,
        failed_step: Option<FailedStepInfo>,
    ) -> Result<PathBuf, crate::utils::error::WorkflowError> {
        let path = Self::context_file_path(workflow_file);

        // 确保目录存在
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                crate::utils::error::WorkflowError::Other(format!(
                    "创建运行上下文目录失败: {}",
                    e
                ))
            })?;
        }

        let context = Self {
            workflow_file: workflow_file
                .canonicalize()
                .unwrap_or_else(|_| workflow_file.to_path_buf())
                .to_string_lossy()
                .to_string(),
            saved_at: Utc::now(),
            inputs,
            variables,
            step_outputs,
            failed_step,
        };

        let json = serde_json::to_string_pretty(&context).map_err(|e| {
            crate::utils::error::WorkflowError::Other(format!("序列化运行上下文失败: {}", e))
        })?;

        std::fs::write(&path, json).map_err(|e| {
            crate::utils::error::WorkflowError::Other(format!("写入运行上下文失败: {}", e))
        })?;

        tracing::info!("运行上下文已保存: {}", path.display());
        Ok(path)
    }

    /// 根据工作流文件路径加载对应的运行上下文
    pub fn load(workflow_file: &Path) -> Result<Self, crate::utils::error::WorkflowError> {
        let path = Self::context_file_path(workflow_file);

        if !path.exists() {
            return Err(crate::utils::error::WorkflowError::RunContextNotFound {
                workflow_file: workflow_file.to_string_lossy().to_string(),
            });
        }

        let content = std::fs::read_to_string(&path).map_err(|e| {
            crate::utils::error::WorkflowError::RunContextLoadFailed {
                path: path.display().to_string(),
                reason: e.to_string(),
            }
        })?;

        serde_json::from_str(&content).map_err(|e| {
            crate::utils::error::WorkflowError::RunContextLoadFailed {
                path: path.display().to_string(),
                reason: e.to_string(),
            }
        })
    }

    /// 检查是否存在对应的运行上下文
    pub fn exists(workflow_file: &Path) -> bool {
        Self::context_file_path(workflow_file).exists()
    }

    /// 计算上下文文件路径
    pub fn context_file_path(workflow_file: &Path) -> PathBuf {
        let hash = Self::workflow_file_hash(workflow_file);
        std::env::temp_dir()
            .join("flow-run-contexts")
            .join(format!("{}.json", hash))
    }

    /// 对工作流文件绝对路径生成 8 位 hex hash
    fn workflow_file_hash(path: &Path) -> String {
        let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        abs.to_string_lossy().to_string().hash(&mut hasher);
        format!("{:08x}", hasher.finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_save_and_load() {
        let dir = TempDir::new().unwrap();
        let workflow_file = dir.path().join("test.yaml");

        // 创建文件（让 canonicalize 工作）
        std::fs::write(&workflow_file, "name: test\nsteps: []\n").unwrap();

        let inputs = HashMap::from([
            ("key1".to_string(), serde_json::json!("value1")),
            ("key2".to_string(), serde_json::json!(42)),
        ]);
        let variables = HashMap::new();
        let step_outputs = HashMap::from([(
            "step1".to_string(),
            StepResult::success("step1", serde_json::json!({"result": "ok"})),
        )]);
        let failed_step = Some(FailedStepInfo {
            step_id: "step2".to_string(),
            error_code: "EXIT_1".to_string(),
            error_message: "command failed".to_string(),
            fix_suggestion: Some("check your command".to_string()),
        });

        let saved_path =
            RunContext::save(&workflow_file, inputs.clone(), variables.clone(), step_outputs.clone(), failed_step.clone())
                .unwrap();
        assert!(saved_path.exists());

        let loaded = RunContext::load(&workflow_file).unwrap();
        assert_eq!(loaded.inputs, inputs);
        assert_eq!(loaded.step_outputs.len(), 1);
        assert_eq!(loaded.failed_step.as_ref().unwrap().step_id, "step2");
    }

    #[test]
    fn test_load_not_found() {
        let result = RunContext::load(Path::new("/nonexistent/workflow.yaml"));
        assert!(result.is_err());
    }

    #[test]
    fn test_context_file_path_deterministic() {
        let dir = TempDir::new().unwrap();
        let workflow_file = dir.path().join("test.yaml");
        std::fs::write(&workflow_file, "name: test\nsteps: []\n").unwrap();

        let path1 = RunContext::context_file_path(&workflow_file);
        let path2 = RunContext::context_file_path(&workflow_file);
        assert_eq!(path1, path2);
    }
}
