use crate::agent::builtin_registry::BuiltinToolRegistry;
use crate::agent::ToolHandler;
use crate::core::context::ExecutionContext;
use crate::core::dag::{DagScheduler, Scheduler};
use crate::core::parser::WorkflowParser;
use crate::core::types::*;
use crate::utils::checkpoint::CheckpointManager;
use crate::utils::error::WorkflowError;
use crate::utils::run_context::RunContext;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 工作流执行器 — 库的高层 API 入口
///
/// 封装了 DagScheduler + Scheduler + CheckpointManager 的组装逻辑，
/// 外部应用只需 `FlowRunner::from_file()` + `runner.run()` 即可执行工作流。
pub struct FlowRunner {
    workflow: WorkflowDefinition,
    workflow_path: PathBuf,
    checkpoint_dir: PathBuf,
    builtin_registry: Arc<BuiltinToolRegistry>,
}

impl FlowRunner {
    /// 从工作流定义创建
    pub fn new(workflow: WorkflowDefinition) -> Self {
        Self {
            workflow,
            workflow_path: PathBuf::new(),
            checkpoint_dir: std::env::temp_dir().join(format!("flow-run-{}", std::process::id())),
            builtin_registry: Arc::new(BuiltinToolRegistry::with_defaults()),
        }
    }

    /// 从 YAML 文件创建
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, WorkflowError> {
        let workflow = WorkflowParser::from_file(&path)?;
        Ok(Self {
            workflow,
            workflow_path: path.as_ref().to_path_buf(),
            checkpoint_dir: std::env::temp_dir().join(format!("flow-run-{}", std::process::id())),
            builtin_registry: Arc::new(BuiltinToolRegistry::with_defaults()),
        })
    }

    /// 设置检查点目录（builder 模式）
    pub fn with_checkpoint_dir(mut self, dir: PathBuf) -> Self {
        self.checkpoint_dir = dir;
        self
    }

    /// 注册自定义 builtin 工具（builder 模式）
    ///
    /// 注册后可在 YAML 中通过 `source: builtin` + `name: xxx` 使用。
    /// 如果与默认工具同名，自定义工具优先。
    pub fn register_builtin(
        mut self,
        name: &str,
        description: &str,
        handler: Arc<dyn ToolHandler>,
    ) -> Self {
        let registry = Arc::get_mut(&mut self.builtin_registry)
            .expect("register_builtin must be called before run() — Arc should be uniquely owned");
        registry.register(name, description, handler);
        self
    }

    /// 传入预构造的 BuiltinToolRegistry（builder 模式）
    pub fn with_builtin_registry(mut self, registry: BuiltinToolRegistry) -> Self {
        self.builtin_registry = Arc::new(registry);
        self
    }

    /// 执行工作流
    pub async fn run(&self, inputs: HashMap<String, serde_json::Value>) -> Result<WorkflowResult, WorkflowError> {
        let dag = DagScheduler::new(self.workflow.steps.clone())?;
        let checkpoint_manager = CheckpointManager::new(self.checkpoint_dir.clone())?;
        let config = self.workflow.config.clone().unwrap_or_default();
        let scheduler = Scheduler::new_with_workflow_path(
            dag, config, checkpoint_manager, self.builtin_registry.clone(), Some(self.workflow_path.clone()),
        );

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
        let scheduler = Scheduler::new(dag, config, checkpoint_manager, self.builtin_registry.clone());

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

        // 构建 DAG 边
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
            has_cycle: false, // topological_sort 已经检查过
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

    /// 获取工作流文件路径
    pub fn workflow_path(&self) -> &Path {
        &self.workflow_path
    }

    /// 从指定步骤继续执行（加载上次失败时保存的上下文）
    pub async fn run_from_step(
        &self,
        from_step_id: &str,
        inputs: HashMap<String, serde_json::Value>,
    ) -> Result<(WorkflowResult, usize), WorkflowError> {
        // 加载保存的运行上下文
        let saved_context = RunContext::load(&self.workflow_path)?;

        // 合并 inputs：CLI 传入的优先
        let mut merged_inputs = saved_context.inputs.clone();
        for (k, v) in inputs {
            merged_inputs.insert(k, v);
        }

        let dag = DagScheduler::new(self.workflow.steps.clone())?;
        let checkpoint_manager = CheckpointManager::new(self.checkpoint_dir.clone())?;
        let config = self.workflow.config.clone().unwrap_or_default();
        let scheduler = Scheduler::new_with_workflow_path(
            dag, config, checkpoint_manager, self.builtin_registry.clone(), Some(self.workflow_path.clone()),
        );

        let context = ExecutionContext::new(&self.workflow, merged_inputs);
        scheduler.set_context(context).await;

        if let Some(outputs) = &self.workflow.outputs {
            let outputs_map: HashMap<String, String> = outputs
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            scheduler.set_outputs(outputs_map).await;
        }

        scheduler
            .run_from_step(
                from_step_id,
                saved_context.step_outputs,
                saved_context.variables,
            )
            .await
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
