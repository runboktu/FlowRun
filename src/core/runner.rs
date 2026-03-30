use crate::core::context::ExecutionContext;
use crate::core::dag::{DagScheduler, Scheduler};
use crate::core::parser::WorkflowParser;
use crate::core::types::*;
use crate::utils::checkpoint::CheckpointManager;
use crate::utils::error::WorkflowError;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 工作流执行器 — 库的高层 API 入口
///
/// 封装了 DagScheduler + Scheduler + CheckpointManager 的组装逻辑，
/// 外部应用只需 `FlowRunner::from_file()` + `runner.run()` 即可执行工作流。
pub struct FlowRunner {
    workflow: WorkflowDefinition,
    checkpoint_dir: PathBuf,
}

impl FlowRunner {
    /// 从工作流定义创建
    pub fn new(workflow: WorkflowDefinition) -> Self {
        Self {
            workflow,
            checkpoint_dir: std::env::temp_dir().join(format!("flow-run-{}", std::process::id())),
        }
    }

    /// 从 YAML 文件创建
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, WorkflowError> {
        let workflow = WorkflowParser::from_file(path)?;
        Ok(Self::new(workflow))
    }

    /// 设置检查点目录（builder 模式）
    pub fn with_checkpoint_dir(mut self, dir: PathBuf) -> Self {
        self.checkpoint_dir = dir;
        self
    }

    /// 执行工作流
    pub async fn run(&self, inputs: HashMap<String, serde_json::Value>) -> Result<WorkflowResult, WorkflowError> {
        let dag = DagScheduler::new(self.workflow.steps.clone())?;
        let checkpoint_manager = CheckpointManager::new(self.checkpoint_dir.clone())?;
        let config = self.workflow.config.clone().unwrap_or_default();
        let scheduler = Scheduler::new(dag, config, checkpoint_manager);

        let context = ExecutionContext::new(&self.workflow, inputs);
        scheduler.set_context(context).await;

        if let Some(outputs) = &self.workflow.outputs {
            let outputs_map: HashMap<String, String> = outputs
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            scheduler.set_outputs(outputs_map).await;
        }

        Ok(scheduler.run().await?)
    }

    /// 从检查点恢复执行
    pub async fn resume(
        &self,
        checkpoint_id: &str,
        inputs: HashMap<String, serde_json::Value>,
    ) -> Result<WorkflowResult, WorkflowError> {
        let dag = DagScheduler::new(self.workflow.steps.clone())?;
        let checkpoint_manager = CheckpointManager::new(self.checkpoint_dir.clone())?;
        let config = self.workflow.config.clone().unwrap_or_default();
        let scheduler = Scheduler::new(dag, config, checkpoint_manager);

        let context = ExecutionContext::new(&self.workflow, inputs);
        scheduler.set_context(context).await;

        if let Some(outputs) = &self.workflow.outputs {
            let outputs_map: HashMap<String, String> = outputs
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            scheduler.set_outputs(outputs_map).await;
        }

        Ok(scheduler.resume(checkpoint_id).await?)
    }

    /// 生成执行计划（dry-run 的数据层）
    pub fn plan(&self, provided_inputs: &[(String, String)]) -> Result<ExecutionPlan, WorkflowError> {
        let dag = DagScheduler::new(self.workflow.steps.clone())?;
        let batches = dag.topological_sort()?;

        let mut dag_edges = Vec::new();
        for step in &self.workflow.steps {
            if let Some(deps) = &step.depends_on {
                for dep in deps {
                    dag_edges.push(DagEdge {
                        from: dep.clone(),
                        to: step.id.clone(),
                    });
                }
            }
        }

        Ok(ExecutionPlan {
            workflow_name: self.workflow.name.clone(),
            workflow_version: self.workflow.version.clone(),
            workflow_description: self.workflow.description.clone(),
            step_count: self.workflow.steps.len(),
            has_cycle: false,
            config: self.workflow.config.clone(),
            inputs: self.workflow.inputs.clone(),
            provided_inputs: provided_inputs.to_vec(),
            outputs: self.workflow.outputs.clone(),
            batches,
            dag_edges,
        })
    }

    /// 获取工作流定义的引用
    pub fn workflow(&self) -> &WorkflowDefinition {
        &self.workflow
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runner_from_str() {
        let yaml = r#"
name: "test_workflow"
steps:
  - id: "step1"
    type: "shell"
    run: "echo hello"
"#;
        let workflow = WorkflowParser::from_str(yaml).unwrap();
        let runner = FlowRunner::new(workflow);
        assert_eq!(runner.workflow().name, "test_workflow");
    }

    #[test]
    fn test_runner_plan() {
        let yaml = r#"
name: "test_workflow"
steps:
  - id: "step1"
    type: "shell"
    run: "echo hello"
  - id: "step2"
    type: "shell"
    run: "echo world"
    depends_on: ["step1"]
"#;
        let workflow = WorkflowParser::from_str(yaml).unwrap();
        let runner = FlowRunner::new(workflow);

        let plan = runner.plan(&[]).unwrap();
        assert_eq!(plan.workflow_name, "test_workflow");
        assert_eq!(plan.step_count, 2);
        assert_eq!(plan.batches.len(), 2);
        assert_eq!(plan.dag_edges.len(), 1);
        assert_eq!(plan.dag_edges[0].from, "step1");
        assert_eq!(plan.dag_edges[0].to, "step2");
    }

    #[test]
    fn test_runner_with_checkpoint_dir() {
        let yaml = r#"
name: "test_workflow"
steps:
  - id: "step1"
    type: "shell"
    run: "echo hello"
"#;
        let workflow = WorkflowParser::from_str(yaml).unwrap();
        let runner = FlowRunner::new(workflow)
            .with_checkpoint_dir(PathBuf::from("/tmp/custom-checkpoints"));
        assert_eq!(runner.checkpoint_dir, PathBuf::from("/tmp/custom-checkpoints"));
    }
}
