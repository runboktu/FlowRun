use crate::core::context::ExecutionContext;
use crate::core::types::*;
use crate::executors::workflow::{WorkflowExecutor, WorkflowRunner};
use crate::executors::approve::ApproveExecutor;
use crate::utils::checkpoint::CheckpointManager;
use crate::utils::error::WorkflowError;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, Semaphore};

#[derive(Debug, Clone)]
pub struct DagScheduler {
    steps: Vec<StepDefinition>,
    adjacency: HashMap<StepId, Vec<StepId>>,
    in_degree: HashMap<StepId, usize>,
}

impl DagScheduler {
    pub fn new(steps: Vec<StepDefinition>) -> Result<Self, WorkflowError> {
        let mut adjacency: HashMap<StepId, Vec<StepId>> = HashMap::new();
        let mut in_degree: HashMap<StepId, usize> = HashMap::new();

        let step_ids: Vec<String> = steps.iter().map(|s| s.id.clone()).collect();

        for step_id in &step_ids {
            adjacency.insert(step_id.clone(), Vec::new());
            in_degree.insert(step_id.clone(), 0);
        }

        for step in &steps {
            if let Some(deps) = &step.depends_on {
                for dep in deps {
                    if !step_ids.contains(dep) {
                        return Err(WorkflowError::StepNotFound { step_id: dep.clone() });
                    }
                    adjacency.entry(dep.clone()).or_default().push(step.id.clone());
                    *in_degree.entry(step.id.clone()).or_insert(0) += 1;
                }
            }
        }

        Ok(Self {
            steps,
            adjacency,
            in_degree,
        })
    }

    pub fn topological_sort(&self) -> Result<Vec<Vec<StepId>>, WorkflowError> {
        if self.has_cycle()? {
            return Err(WorkflowError::CycleDetected);
        }

        let mut in_degree = self.in_degree.clone();
        let mut queue: VecDeque<StepId> = VecDeque::new();
        let mut batches: Vec<Vec<StepId>> = Vec::new();

        for (step_id, &deg) in &in_degree {
            if deg == 0 {
                queue.push_back(step_id.clone());
            }
        }

        while !queue.is_empty() {
            let batch_size = queue.len();
            let mut current_batch: Vec<StepId> = Vec::with_capacity(batch_size);

            for _ in 0..batch_size {
                if let Some(step_id) = queue.pop_front() {
                    current_batch.push(step_id.clone());

                    if let Some(neighbors) = self.adjacency.get(&step_id) {
                        for neighbor in neighbors {
                            if let Some(deg) = in_degree.get_mut(neighbor) {
                                *deg -= 1;
                                if *deg == 0 {
                                    queue.push_back(neighbor.clone());
                                }
                            }
                        }
                    }
                }
            }

            if !current_batch.is_empty() {
                batches.push(current_batch);
            }
        }

        Ok(batches)
    }

    pub fn has_cycle(&self) -> Result<bool, WorkflowError> {
        let mut visited: HashSet<StepId> = HashSet::new();
        let mut recursion_stack: HashSet<StepId> = HashSet::new();

        for step in &self.steps {
            if !visited.contains(&step.id) {
                if self.dfs_cycle_check(&step.id, &mut visited, &mut recursion_stack)? {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    fn dfs_cycle_check(
        &self,
        step_id: &StepId,
        visited: &mut HashSet<StepId>,
        recursion_stack: &mut HashSet<StepId>,
    ) -> Result<bool, WorkflowError> {
        visited.insert(step_id.clone());
        recursion_stack.insert(step_id.clone());

        if let Some(neighbors) = self.adjacency.get(step_id) {
            for neighbor in neighbors {
                if !visited.contains(neighbor) {
                    if self.dfs_cycle_check(neighbor, visited, recursion_stack)? {
                        return Ok(true);
                    }
                } else if recursion_stack.contains(neighbor) {
                    return Ok(true);
                }
            }
        }

        recursion_stack.remove(step_id);
        Ok(false)
    }

    pub fn get_step(&self, step_id: &StepId) -> Option<&StepDefinition> {
        self.steps.iter().find(|s| &s.id == step_id)
    }

    pub fn get_steps(&self) -> &[StepDefinition] {
        &self.steps
    }
}

pub struct Scheduler {
    dag: DagScheduler,
    context: Arc<RwLock<ExecutionContext>>,
    config: WorkflowConfig,
    checkpoint_manager: CheckpointManager,
    workflow_executor: Arc<WorkflowExecutor>,
    approve_executor: Arc<ApproveExecutor>,
}

impl Scheduler {
    pub fn new(
        dag: DagScheduler,
        config: WorkflowConfig,
        checkpoint_manager: CheckpointManager,
    ) -> Self {
        // 创建一个空的调度器，稍后初始化 workflow_executor
        Self {
            dag,
            context: Arc::new(RwLock::new(ExecutionContext::empty())),
            config,
            checkpoint_manager,
            workflow_executor: Arc::new(WorkflowExecutor::new(Arc::new(NullWorkflowRunner))),
            approve_executor: Arc::new(ApproveExecutor::new()),
        }
    }

    pub fn with_workflow_executor(
        dag: DagScheduler,
        config: WorkflowConfig,
        checkpoint_manager: CheckpointManager,
        workflow_executor: Arc<WorkflowExecutor>,
    ) -> Self {
        Self {
            dag,
            context: Arc::new(RwLock::new(ExecutionContext::empty())),
            config,
            checkpoint_manager,
            workflow_executor,
            approve_executor: Arc::new(ApproveExecutor::new()),
        }
    }

    pub async fn run(&self) -> Result<WorkflowResult, WorkflowError> {
        let start_time = Utc::now();
        let execution_id = format!("exec_{}", start_time.timestamp_millis());

        let batches = self.dag.topological_sort()?;

        let mut all_results: Vec<StepResult> = Vec::new();
        let mut errors: Vec<StepError> = Vec::new();

        for (batch_index, batch) in batches.iter().enumerate() {
            let batch_results = self.execute_batch(batch).await?;

            for result in &batch_results {
                let mut ctx = self.context.write().await;
                ctx.step_outputs.insert(result.step_id.clone(), result.clone());
            }

            let has_failure = batch_results.iter().any(|r| r.status == StepStatus::Failed);
            all_results.extend(batch_results);

            if let Some(checkpoint_path) = &self.config.checkpoint {
                if !checkpoint_path.is_empty() {
                    let ctx = self.context.read().await;
                    let mut checkpoint = crate::utils::checkpoint::Checkpoint::new(
                        execution_id.clone(),
                        "workflow".to_string(),
                        start_time,
                        ChronoDuration::seconds(300),
                    );

                    // 设置检查点状态
                    for result in &all_results {
                        if result.status == StepStatus::Success {
                            checkpoint.mark_step_completed(result.step_id.clone());
                        } else if result.status == StepStatus::Failed {
                            checkpoint.mark_step_failed(result.step_id.clone());
                        }
                        checkpoint.record_step_output(result.step_id.clone(), result.clone());
                    }

                    // 复制变量
                    for (key, value) in ctx.variables.iter() {
                        checkpoint.set_variable(key.clone(), value.clone());
                    }

                    checkpoint.current_batch = batch_index;

                    // 保存检查点
                    let _ = self.checkpoint_manager.save(&mut checkpoint);
                }
            }

            if let Some(on_failure) = &self.config.on_failure {
                if has_failure {
                    let batch_errors: Vec<StepError> = all_results
                        .iter()
                        .filter(|r| r.status == StepStatus::Failed)
                        .filter_map(|r| r.error.clone())
                        .collect();

                    match on_failure {
                        OnFailureStrategy::Abort => {
                            errors.extend(batch_errors);
                            return self.build_result(execution_id, start_time, all_results, errors);
                        }
                        OnFailureStrategy::Pause => {
                            errors.extend(batch_errors);
                            return self.build_result(execution_id, start_time, all_results, errors);
                        }
                        OnFailureStrategy::Continue => {
                            errors.extend(batch_errors);
                        }
                    }
                }
            }
        }

        self.build_result(execution_id, start_time, all_results, errors)
    }

    async fn execute_batch(&self, batch: &[StepId]) -> Result<Vec<StepResult>, WorkflowError> {
        let max_concurrent = self.config.max_concurrent.unwrap_or(4);
        let semaphore = Arc::new(Semaphore::new(max_concurrent));
        let mut tasks = Vec::new();

        for step_id in batch {
            if let Some(step_def) = self.dag.get_step(step_id) {
                let step_def = step_def.clone();
                let semaphore = Arc::clone(&semaphore);
                let context = Arc::clone(&self.context);
                let workflow_executor = Arc::clone(&self.workflow_executor);
                let approve_executor = Arc::clone(&self.approve_executor);

                let task = tokio::spawn(async move {
                    let _permit = semaphore.acquire().await.unwrap();
                    Self::execute_step(&step_def, &context, workflow_executor, approve_executor).await
                });

                tasks.push(task);
            }
        }

        let mut results = Vec::new();
        for task in tasks {
            let result = task.await.map_err(|e| {
                WorkflowError::Other(format!("任务执行失败: {}", e))
            })?;
            results.push(result?);
        }

        Ok(results)
    }

    /// 执行单个步骤（返回 boxed future 以支持递归调用）
    fn execute_step<'a>(
        step_def: &'a StepDefinition,
        context: &'a Arc<RwLock<ExecutionContext>>,
        workflow_executor: Arc<WorkflowExecutor>,
        approve_executor: Arc<ApproveExecutor>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<StepResult, WorkflowError>> + Send + 'a>> {
        Box::pin(async move {
            let start_time = Utc::now();

            let result = match &step_def.r#type {
                StepType::Http => {
                    Self::execute_http_step(step_def, context).await
                }
                StepType::Shell => {
                    Self::execute_shell_step(step_def, context).await
                }
                StepType::Parallel => {
                    Self::execute_parallel_step(step_def, context, workflow_executor.clone(), approve_executor.clone()).await
                }
                StepType::Loop => {
                    Self::execute_loop_step(step_def, context, workflow_executor.clone(), approve_executor.clone()).await
                }
                StepType::Condition => {
                    Self::execute_condition_step(step_def, context, workflow_executor.clone(), approve_executor.clone()).await
                }
                StepType::Workflow => {
                    Self::execute_workflow_step(step_def, context, workflow_executor).await
                }
                StepType::Approve => {
                    Self::execute_approve_step(step_def, context, approve_executor).await
                }
            };

            result.map(|mut r| {
                let duration_ms = Utc::now()
                    .signed_duration_since(start_time)
                    .num_milliseconds() as u64;
                r.started_at = start_time;
                r.completed_at = Some(Utc::now());
                r.duration_ms = Some(duration_ms);
                r
            })
        })
    }

    async fn execute_http_step(
        step_def: &StepDefinition,
        _context: &Arc<RwLock<ExecutionContext>>,
    ) -> Result<StepResult, WorkflowError> {
        let url = step_def.api.as_ref().ok_or_else(|| {
            WorkflowError::Other(format!("步骤 {} 缺少 API 地址", step_def.id))
        })?;

        let method = step_def.method.as_deref().unwrap_or("GET");

        let client = reqwest::Client::new();
        let request = match method.to_uppercase().as_str() {
            "GET" => client.get(url),
            "POST" => client.post(url),
            "PUT" => client.put(url),
            "DELETE" => client.delete(url),
            "PATCH" => client.patch(url),
            _ => return Err(WorkflowError::Other(format!("不支持的 HTTP 方法: {}", method))),
        };

        let mut request_builder = request;
        if let Some(headers) = &step_def.headers {
            for (key, value) in headers {
                request_builder = request_builder.header(key, value);
            }
        }

        if let Some(body) = &step_def.body {
            request_builder = request_builder.json(body);
        }

        let response = request_builder.send().await.map_err(|e| {
            WorkflowError::HttpRequestFailed {
                status_code: 0,
                message: e.to_string(),
            }
        })?;

        let status_code = response.status().as_u16();
        let body_text = response.text().await.map_err(|e| {
            WorkflowError::HttpRequestFailed {
                status_code,
                message: format!("读取响应失败: {}", e),
            }
        })?;

        if !(200..300).contains(&status_code) {
            return Ok(StepResult::failed(
                &step_def.id,
                StepError {
                    code: format!("HTTP_{}", status_code),
                    message: format!("HTTP 请求失败: {}", body_text),
                    fix: None,
                },
            ));
        }

        let output: serde_json::Value = serde_json::from_str(&body_text).unwrap_or_else(|_| {
            serde_json::json!({ "text": body_text })
        });

        Ok(StepResult::success(&step_def.id, output))
    }

    async fn execute_shell_step(
        step_def: &StepDefinition,
        _context: &Arc<RwLock<ExecutionContext>>,
    ) -> Result<StepResult, WorkflowError> {
        let command = step_def.run.as_ref().ok_or_else(|| {
            WorkflowError::Other(format!("步骤 {} 缺少 run 命令", step_def.id))
        })?;

        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .output()
            .await
            .map_err(|e| WorkflowError::Other(format!("执行命令失败: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Ok(StepResult::failed(
                &step_def.id,
                StepError {
                    code: format!("EXIT_{}", output.status.code().unwrap_or(-1)),
                    message: stderr,
                    fix: Some("检查命令语法和执行环境".to_string()),
                },
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let output_value = serde_json::json!({ "stdout": stdout });

        Ok(StepResult::success(&step_def.id, output_value))
    }

    async fn execute_parallel_step(
        step_def: &StepDefinition,
        context: &Arc<RwLock<ExecutionContext>>,
        workflow_executor: Arc<WorkflowExecutor>,
        approve_executor: Arc<ApproveExecutor>,
    ) -> Result<StepResult, WorkflowError> {
        let steps = step_def.steps.as_ref().ok_or_else(|| {
            WorkflowError::Other(format!("步骤 {} 缺少子步骤", step_def.id))
        })?;

        let mut results = Vec::new();
        for sub_step in steps {
            let result = Self::execute_step(sub_step, context, workflow_executor.clone(), approve_executor.clone()).await?;
            results.push(result);
        }

        let output = serde_json::json!({
            "results": results.iter().map(|r| &r.output).collect::<Vec<_>>()
        });

        Ok(StepResult::success(&step_def.id, output))
    }

    async fn execute_loop_step(
        step_def: &StepDefinition,
        context: &Arc<RwLock<ExecutionContext>>,
        workflow_executor: Arc<WorkflowExecutor>,
        approve_executor: Arc<ApproveExecutor>,
    ) -> Result<StepResult, WorkflowError> {
        let _loop_config = step_def.r#loop.as_ref().ok_or_else(|| {
            WorkflowError::Other(format!("步骤 {} 缺少循环配置", step_def.id))
        })?;

        let do_steps = step_def.do_steps.as_ref().ok_or_else(|| {
            WorkflowError::Other(format!("步骤 {} 缺少循环体步骤", step_def.id))
        })?;

        let mut results = Vec::new();
        for sub_step in do_steps {
            let result = Self::execute_step(sub_step, context, workflow_executor.clone(), approve_executor.clone()).await?;
            results.push(result);
        }

        let output = serde_json::json!({
            "iterations": results.len(),
            "results": results.iter().map(|r| &r.output).collect::<Vec<_>>()
        });

        Ok(StepResult::success(&step_def.id, output))
    }

    async fn execute_condition_step(
        step_def: &StepDefinition,
        context: &Arc<RwLock<ExecutionContext>>,
        workflow_executor: Arc<WorkflowExecutor>,
        approve_executor: Arc<ApproveExecutor>,
    ) -> Result<StepResult, WorkflowError> {
        let _expression = step_def.expression.as_ref().ok_or_else(|| {
            WorkflowError::Other(format!("步骤 {} 缺少条件表达式", step_def.id))
        })?;

        let then_steps = step_def.then_steps.as_deref().unwrap_or(&[]);
        let _else_steps = step_def.else_steps.as_deref().unwrap_or(&[]);

        let selected_steps = then_steps;

        let mut results = Vec::new();
        for sub_step in selected_steps {
            let result = Self::execute_step(sub_step, context, workflow_executor.clone(), approve_executor.clone()).await?;
            results.push(result);
        }

        let output = serde_json::json!({
            "branch": "then",
            "results": results.iter().map(|r| &r.output).collect::<Vec<_>>()
        });

        Ok(StepResult::success(&step_def.id, output))
    }

    async fn execute_workflow_step(
        step_def: &StepDefinition,
        context: &Arc<RwLock<ExecutionContext>>,
        workflow_executor: Arc<WorkflowExecutor>,
    ) -> Result<StepResult, WorkflowError> {
        let ctx = context.read().await;
        workflow_executor.execute(step_def, &ctx).await
    }

    async fn execute_approve_step(
        step_def: &StepDefinition,
        context: &Arc<RwLock<ExecutionContext>>,
        approve_executor: Arc<ApproveExecutor>,
    ) -> Result<StepResult, WorkflowError> {
        let ctx = context.read().await;
        approve_executor.execute(step_def, &ctx).await
    }

    pub async fn resume(
        &self,
        checkpoint_id: &str,
    ) -> Result<WorkflowResult, WorkflowError> {
        let start_time = Utc::now();
        let execution_id = format!("exec_{}_resumed", start_time.timestamp_millis());

        let checkpoint_data = self.checkpoint_manager.load(checkpoint_id)?;

        let mut ctx = self.context.write().await;
        ctx.variables = checkpoint_data.variables.clone();
        let step_outputs = checkpoint_data.step_outputs.clone();
        for (step_id, result) in step_outputs {
            ctx.step_outputs.insert(step_id, result);
        }

        let batches = self.dag.topological_sort()?;
        let mut all_results: Vec<StepResult> = checkpoint_data
            .step_outputs
            .values()
            .cloned()
            .collect();
        let mut errors: Vec<StepError> = Vec::new();

        for batch_index in (checkpoint_data.current_batch + 1)..batches.len() {
            let batch = &batches[batch_index];
            let batch_results = self.execute_batch(batch).await?;

            for result in &batch_results {
                let mut ctx = self.context.write().await;
                ctx.step_outputs.insert(result.step_id.clone(), result.clone());
            }

            all_results.extend(batch_results);

            if let Some(checkpoint_path) = &self.config.checkpoint {
                if !checkpoint_path.is_empty() {
                    let ctx = self.context.read().await;
                    let mut checkpoint = crate::utils::checkpoint::Checkpoint::new(
                        execution_id.clone(),
                        "workflow".to_string(),
                        start_time,
                        ChronoDuration::seconds(300),
                    );

                    // 设置检查点状态
                    for result in &all_results {
                        if result.status == StepStatus::Success {
                            checkpoint.mark_step_completed(result.step_id.clone());
                        } else if result.status == StepStatus::Failed {
                            checkpoint.mark_step_failed(result.step_id.clone());
                        }
                        checkpoint.record_step_output(result.step_id.clone(), result.clone());
                    }

                    // 复制变量
                    for (key, value) in ctx.variables.iter() {
                        checkpoint.set_variable(key.clone(), value.clone());
                    }

                    checkpoint.current_batch = batch_index;

                    // 保存检查点
                    let _ = self.checkpoint_manager.save(&mut checkpoint);
                }
            }
        }

        self.build_result(execution_id, start_time, all_results, errors)
    }

    fn build_result(
        &self,
        execution_id: String,
        start_time: DateTime<Utc>,
        results: Vec<StepResult>,
        errors: Vec<StepError>,
    ) -> Result<WorkflowResult, WorkflowError> {
        let completed_at = Some(Utc::now());
        let duration_ms = completed_at
            .map(|t| t.signed_duration_since(start_time).num_milliseconds() as u64);

        let total_steps = results.len();
        let success_steps = results
            .iter()
            .filter(|r| r.status == StepStatus::Success)
            .count();
        let failed_steps = results
            .iter()
            .filter(|r| r.status == StepStatus::Failed)
            .count();
        let skipped_steps = results
            .iter()
            .filter(|r| r.status == StepStatus::Skipped)
            .count();

        let status = if failed_steps > 0 {
            WorkflowStatus::Failed
        } else {
            WorkflowStatus::Success
        };

        let metrics = ExecutionMetrics {
            total_steps,
            success_steps,
            failed_steps,
            skipped_steps,
            total_duration_ms: duration_ms.unwrap_or(0),
        };

        let execution_info = ExecutionInfo {
            id: execution_id,
            started_at: start_time,
            completed_at,
            duration_ms,
            checkpoint: self.config.checkpoint.clone(),
        };

        Ok(WorkflowResult {
            status,
            workflow: WorkflowInfo {
                name: "workflow".to_string(),
                version: None,
                file: "unknown".to_string(),
            },
            execution: execution_info,
            steps: results,
            outputs: None,
            metrics,
            errors,
        })
    }
}

#[async_trait::async_trait]
impl WorkflowRunner for Scheduler {
    async fn run_workflow(
        &self,
        workflow_path: &str,
        inputs: HashMap<String, serde_json::Value>,
        _timeout: Option<Duration>,
    ) -> Result<WorkflowResult, WorkflowError> {
        use crate::core::parser::WorkflowParser;

        // 解析工作流定义
        let workflow_def = WorkflowParser::from_file(workflow_path)?;

        // 创建新的调度器来运行子工作流
        let sub_dag = DagScheduler::new(workflow_def.steps.clone())?;
        let sub_scheduler = Scheduler::new(
            sub_dag,
            self.config.clone(),
            self.checkpoint_manager.clone(),
        );

        // 设置子工作流的上下文
        let mut ctx = self.context.write().await;
        ctx.inputs = inputs;

        // 运行子工作流
        sub_scheduler.run().await
    }
}

/// 空的 WorkflowRunner 实现（占位符）
///
/// 在初始化阶段使用，稍后会被替换
struct NullWorkflowRunner;

#[async_trait::async_trait]
impl WorkflowRunner for NullWorkflowRunner {
    async fn run_workflow(
        &self,
        _workflow_path: &str,
        _inputs: HashMap<String, serde_json::Value>,
        _timeout: Option<Duration>,
    ) -> Result<WorkflowResult, WorkflowError> {
        Err(WorkflowError::Other(
            "NullWorkflowRunner 不应该被调用".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dag_scheduler_creation() {
        let steps = vec![
            StepDefinition {
                id: "step1".to_string(),
                name: None,
                r#type: StepType::Shell,
                depends_on: None,
                expect: None,
                retry: None,
                timeout: None,
                hooks: None,
                api: None,
                method: None,
                headers: None,
                body: None,
                cache: None,
                run: Some("echo hello".to_string()),
                env: None,
                safe_mode: None,
                allowed_commands: None,
                steps: None,
                max_concurrent: None,
                rate_limit: None,
                r#loop: None,
                do_steps: None,
                expression: None,
                then_steps: None,
                else_steps: None,
                workflow: None,
                inputs: None,
                error_strategy: None,
                isolation: None,
                passthrough_vars: None,
                message: None,
                approvers: None,
                require_comment: None,
                on_timeout: None,
                auto_approve_on: None,
            },
            StepDefinition {
                id: "step2".to_string(),
                name: None,
                r#type: StepType::Shell,
                depends_on: Some(vec!["step1".to_string()]),
                expect: None,
                retry: None,
                timeout: None,
                hooks: None,
                api: None,
                method: None,
                headers: None,
                body: None,
                cache: None,
                run: Some("echo world".to_string()),
                env: None,
                safe_mode: None,
                allowed_commands: None,
                steps: None,
                max_concurrent: None,
                rate_limit: None,
                r#loop: None,
                do_steps: None,
                expression: None,
                then_steps: None,
                else_steps: None,
                workflow: None,
                inputs: None,
                error_strategy: None,
                isolation: None,
                passthrough_vars: None,
                message: None,
                approvers: None,
                require_comment: None,
                on_timeout: None,
                auto_approve_on: None,
            },
        ];

        let scheduler = DagScheduler::new(steps).unwrap();
        let batches = scheduler.topological_sort().unwrap();

        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0], vec!["step1"]);
        assert_eq!(batches[1], vec!["step2"]);
    }

    #[test]
    fn test_cycle_detection() {
        let steps = vec![
            StepDefinition {
                id: "step1".to_string(),
                name: None,
                r#type: StepType::Shell,
                depends_on: Some(vec!["step2".to_string()]),
                expect: None,
                retry: None,
                timeout: None,
                hooks: None,
                api: None,
                method: None,
                headers: None,
                body: None,
                cache: None,
                run: Some("echo hello".to_string()),
                env: None,
                safe_mode: None,
                allowed_commands: None,
                steps: None,
                max_concurrent: None,
                rate_limit: None,
                r#loop: None,
                do_steps: None,
                expression: None,
                then_steps: None,
                else_steps: None,
                workflow: None,
                inputs: None,
                error_strategy: None,
                isolation: None,
                passthrough_vars: None,
                message: None,
                approvers: None,
                require_comment: None,
                on_timeout: None,
                auto_approve_on: None,
            },
            StepDefinition {
                id: "step2".to_string(),
                name: None,
                r#type: StepType::Shell,
                depends_on: Some(vec!["step1".to_string()]),
                expect: None,
                retry: None,
                timeout: None,
                hooks: None,
                api: None,
                method: None,
                headers: None,
                body: None,
                cache: None,
                run: Some("echo world".to_string()),
                env: None,
                safe_mode: None,
                allowed_commands: None,
                steps: None,
                max_concurrent: None,
                rate_limit: None,
                r#loop: None,
                do_steps: None,
                expression: None,
                then_steps: None,
                else_steps: None,
                workflow: None,
                inputs: None,
                error_strategy: None,
                isolation: None,
                passthrough_vars: None,
                message: None,
                approvers: None,
                require_comment: None,
                on_timeout: None,
                auto_approve_on: None,
            },
        ];

        let scheduler = DagScheduler::new(steps).unwrap();
        assert!(scheduler.has_cycle().unwrap());
    }
}
