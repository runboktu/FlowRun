use crate::core::types::*;
use crate::utils::error::WorkflowError;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// 工作流执行上下文
///
/// 存储工作流执行过程中的状态，包括输入、输出、变量、步骤状态等信息
pub struct ExecutionContext {
    /// 工作流 ID（从文件路径或定义生成）
    pub workflow_id: String,

    /// 工作流名称
    pub workflow_name: String,

    /// 执行 ID（唯一标识本次执行）
    pub execution_id: String,

    /// 开始时间
    pub started_at: DateTime<Utc>,

    /// 输入参数
    pub inputs: HashMap<String, Value>,

    /// 步骤输出（步骤 ID -> 执行结果）
    pub step_outputs: HashMap<StepId, StepResult>,

    /// 已完成的步骤 ID 集合
    pub completed_steps: HashSet<StepId>,

    /// 失败的步骤 ID 集合
    pub failed_steps: HashSet<StepId>,

    /// 自定义变量
    pub variables: HashMap<String, Value>,

    /// 当前批次（用于并行执行）
    pub current_batch: usize,
}

impl ExecutionContext {
    /// 创建新的执行上下文
    ///
    /// # 参数
    ///
    /// * `workflow` - 工作流定义
    /// * `inputs` - 输入参数映射
    ///
    /// # 返回
    ///
    /// 返回新创建的执行上下文
    ///
    /// # 示例
    ///
    /// ```no_run
    /// use flow_run::core::context::ExecutionContext;
    /// use flow_run::core::types::WorkflowDefinition;
    ///
    /// let workflow = WorkflowDefinition { /* ... */ };
    /// let inputs = HashMap::new();
    /// let context = ExecutionContext::new(&workflow, inputs);
    /// ```
    pub fn new(workflow: &WorkflowDefinition, inputs: HashMap<String, Value>) -> Self {
        // 从工作流名称生成 workflow_id（移除非法字符）
        let workflow_id = workflow
            .name
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect::<String>();

        // 生成唯一的执行 ID
        let execution_id = Uuid::new_v4().to_string();

        // 从工作流定义中获取变量
        let variables = workflow.variables.clone().unwrap_or_default();

        Self {
            workflow_id,
            workflow_name: workflow.name.clone(),
            execution_id,
            started_at: Utc::now(),
            inputs,
            step_outputs: HashMap::new(),
            completed_steps: HashSet::new(),
            failed_steps: HashSet::new(),
            variables,
            current_batch: 0,
        }
    }

    /// 创建空的执行上下文（用于初始化阶段）
    pub fn empty() -> Self {
        Self {
            workflow_id: String::new(),
            workflow_name: String::new(),
            execution_id: Uuid::new_v4().to_string(),
            started_at: Utc::now(),
            inputs: HashMap::new(),
            step_outputs: HashMap::new(),
            completed_steps: HashSet::new(),
            failed_steps: HashSet::new(),
            variables: HashMap::new(),
            current_batch: 0,
        }
    }

    /// 求值表达式
    ///
    /// 支持变量引用、路径解析等表达式求值
    ///
    /// # 参数
    ///
    /// * `expression` - 表达式字符串
    ///
    /// # 返回
    ///
    /// 返回求值结果
    ///
    /// # 示例
    ///
    /// ```no_run
    /// use flow_run::core::context::ExecutionContext;
    ///
    /// let context = ExecutionContext::new(&workflow, inputs);
    /// let value = context.evaluate("${{ inputs.api_url }}")?;
    /// ```
    pub fn evaluate(&self, expression: &str) -> Result<Value, WorkflowError> {
        let trimmed = expression.trim();

        // 检查是否为变量引用语法 ${{ ... }}
        if trimmed.starts_with("${{") && trimmed.ends_with("}}") {
            let inner = &trimmed[3..trimmed.len() - 2];
            let inner = inner.trim();

            // 解析路径表达式
            return self.resolve_path(inner);
        }

        // 检查是否为简单的变量引用
        if trimmed.contains('.') {
            return self.resolve_path(trimmed);
        }

        // 检查是否为变量名
        if let Some(value) = self.get_variable(trimmed) {
            return Ok(value);
        }

        // 检查是否为 inputs
        if trimmed.starts_with("inputs.") {
            return self.resolve_path(trimmed);
        }

        // 检查是否为 steps 引用
        if trimmed.starts_with("steps.") {
            return self.resolve_path(trimmed);
        }

        // 直接作为变量名查找
        if let Some(value) = self.get_variable(trimmed) {
            return Ok(value);
        }

        Err(WorkflowError::UndefinedVariable {
            variable: trimmed.to_string(),
        })
    }

    /// 获取变量值
    ///
    /// # 参数
    ///
    /// * `name` - 变量名
    ///
    /// # 返回
    ///
    /// 返回变量值，如果变量不存在则返回 None
    pub fn get_variable(&self, name: &str) -> Option<Value> {
        self.variables.get(name).cloned()
    }

    /// 设置变量值
    ///
    /// # 参数
    ///
    /// * `name` - 变量名
    /// * `value` - 变量值
    pub fn set_variable(&mut self, name: String, value: Value) {
        self.variables.insert(name, value);
    }

    /// 获取步骤输出
    ///
    /// # 参数
    ///
    /// * `step_id` - 步骤 ID
    ///
    /// # 返回
    ///
    /// 返回步骤执行结果，如果步骤不存在或未完成则返回错误
    pub fn get_step_output(&self, step_id: &str) -> Result<&StepResult, WorkflowError> {
        self.step_outputs
            .get(step_id)
            .ok_or_else(|| WorkflowError::StepNotFound {
                step_id: step_id.to_string(),
            })
    }

    /// 解析路径
    ///
    /// 支持点号分隔和数组索引的路径解析，例如：
    /// - `inputs.api_url`
    /// - `steps.deploy.response.body`
    /// - `steps.deploy.response.data[0].name`
    ///
    /// # 参数
    ///
    /// * `path` - 路径字符串
    ///
    /// # 返回
    ///
    /// 返回路径对应的值
    pub fn resolve_path(&self, path: &str) -> Result<Value, WorkflowError> {
        let parts: Vec<&str> = path.split('.').collect();

        if parts.is_empty() {
            return Err(WorkflowError::PathNotFound {
                path: path.to_string(),
            });
        }

        let current: &Value = match parts[0] {
            "inputs" => &Value::Object(
                self.inputs
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            ),
            "variables" => &Value::Object(
                self.variables
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            ),
            "steps" => {
                return self.resolve_steps_path(&parts[1..]);
            }
            _ => {
                if let Some(value) = self.get_variable(parts[0]) {
                    return self.navigate_value(&value, &parts[1..]);
                }
                return Err(WorkflowError::PathNotFound {
                    path: path.to_string(),
                });
            }
        };

        self.navigate_value(current, &parts[1..])
    }

    /// 解析 steps 路径
    fn resolve_steps_path(&self, path: &[&str]) -> Result<Value, WorkflowError> {
        if path.is_empty() {
            return Err(WorkflowError::PathNotFound {
                path: "steps".to_string(),
            });
        }

        // 获取步骤 ID
        let step_id = path[0];
        let step_result = self.get_step_output(step_id)?;

        // 获取步骤输出
        let output = step_result
            .output
            .as_ref()
            .ok_or_else(|| WorkflowError::PathNotFound {
                path: format!("steps.{}.output", step_id),
            })?;

        // 继续导航到后续路径
        self.navigate_value(output, &path[1..])
    }

    /// 在 JSON 值中导航路径
    fn navigate_value(&self, value: &Value, path: &[&str]) -> Result<Value, WorkflowError> {
        let mut current = value;

        for part in path {
            if part.is_empty() {
                continue;
            }

            current = match current {
                Value::Object(map) => {
                    // 尝试直接匹配键名
                    if let Some(v) = map.get(*part) {
                        v
                    }
                    // 处理数组索引语法，如 "data[0]"
                    else if part.contains('[') && part.contains(']') {
                        self.parse_array_access(map, part)?
                    } else {
                        return Err(WorkflowError::PathNotFound {
                            path: part.to_string(),
                        });
                    }
                }
                Value::Array(arr) => {
                    // 尝试解析为数字索引
                    if let Ok(index) = part.parse::<usize>() {
                        if index < arr.len() {
                            &arr[index]
                        } else {
                            return Err(WorkflowError::PathNotFound {
                                path: part.to_string(),
                            });
                        }
                    } else {
                        return Err(WorkflowError::PathNotFound {
                            path: part.to_string(),
                        });
                    }
                }
                _ => {
                    return Err(WorkflowError::PathNotFound {
                        path: part.to_string(),
                    });
                }
            };
        }

        Ok(current.clone())
    }

    /// 解析数组访问语法
    fn parse_array_access<'a>(
        &self,
        map: &'a serde_json::Map<String, Value>,
        part: &str,
    ) -> Result<&'a Value, WorkflowError> {
        let bracket_start = part.find('[').ok_or_else(|| WorkflowError::PathNotFound {
            path: part.to_string(),
        })?;

        let bracket_end = part.find(']').ok_or_else(|| WorkflowError::PathNotFound {
            path: part.to_string(),
        })?;

        let key = &part[..bracket_start];
        let index_str = &part[bracket_start + 1..bracket_end];

        // 获取数组
        let array =
            map.get(key)
                .and_then(|v| v.as_array())
                .ok_or_else(|| WorkflowError::PathNotFound {
                    path: key.to_string(),
                })?;

        // 解析索引
        let index: usize = index_str.parse().map_err(|_| WorkflowError::PathNotFound {
            path: part.to_string(),
        })?;

        // 检查索引范围
        if index >= array.len() {
            return Err(WorkflowError::PathNotFound {
                path: part.to_string(),
            });
        }

        Ok(&array[index])
    }

    /// 标记步骤为完成
    ///
    /// # 参数
    ///
    /// * `step_id` - 步骤 ID
    /// * `result` - 步骤执行结果
    pub fn mark_step_completed(&mut self, step_id: String, result: StepResult) {
        self.step_outputs.insert(step_id.clone(), result);
        self.completed_steps.insert(step_id);
    }

    /// 标记步骤为失败
    ///
    /// # 参数
    ///
    /// * `step_id` - 步骤 ID
    /// * `result` - 步骤执行结果
    pub fn mark_step_failed(&mut self, step_id: String, result: StepResult) {
        self.step_outputs.insert(step_id.clone(), result);
        self.failed_steps.insert(step_id);
    }

    /// 检查步骤是否已完成
    ///
    /// # 参数
    ///
    /// * `step_id` - 步骤 ID
    ///
    /// # 返回
    ///
    /// 返回步骤是否已完成（包括成功和失败）
    pub fn is_step_completed(&self, step_id: &str) -> bool {
        self.completed_steps.contains(step_id) || self.failed_steps.contains(step_id)
    }

    /// 检查步骤是否成功
    ///
    /// # 参数
    ///
    /// * `step_id` - 步骤 ID
    ///
    /// # 返回
    ///
    /// 返回步骤是否成功完成
    pub fn is_step_success(&self, step_id: &str) -> bool {
        self.completed_steps.contains(step_id)
    }

    /// 检查步骤是否失败
    ///
    /// # 参数
    ///
    /// * `step_id` - 步骤 ID
    ///
    /// # 返回
    ///
    /// 返回步骤是否失败
    pub fn is_step_failed(&self, step_id: &str) -> bool {
        self.failed_steps.contains(step_id)
    }

    /// 获取所有可执行步骤（满足依赖条件的步骤）
    ///
    /// # 参数
    ///
    /// * `workflow` - 工作流定义
    ///
    /// # 返回
    ///
    /// 返回可执行步骤的 ID 列表
    pub fn get_ready_steps(&self, workflow: &WorkflowDefinition) -> Vec<String> {
        workflow
            .steps
            .iter()
            .filter(|step| {
                // 步骤未完成
                !self.is_step_completed(&step.id) &&

                // 检查所有依赖是否已完成
                step.depends_on.as_ref().map_or(true, |deps| {
                    deps.iter().all(|dep_id| self.is_step_success(dep_id))
                })
            })
            .map(|step| step.id.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_new_context() {
        let workflow = WorkflowDefinition {
            name: "测试工作流".to_string(),
            description: None,
            version: None,
            config: None,
            inputs: None,
            outputs: None,
            steps: vec![],
            on: None,
            trigger: None,
        };

        let inputs = HashMap::new();
        let context = ExecutionContext::new(&workflow, inputs);

        assert_eq!(context.workflow_name, "测试工作流");
        assert!(!context.execution_id.is_empty());
        assert!(context.started_at <= Utc::now());
        assert!(context.step_outputs.is_empty());
        assert!(context.completed_steps.is_empty());
        assert!(context.failed_steps.is_empty());
    }

    #[test]
    fn test_variable_operations() {
        let workflow = WorkflowDefinition {
            name: "测试工作流".to_string(),
            description: None,
            version: None,
            config: None,
            inputs: None,
            outputs: None,
            steps: vec![],
            on: None,
            trigger: None,
        };

        let mut context = ExecutionContext::new(&workflow, HashMap::new());

        assert_eq!(context.get_variable("test_var"), None);

        context.set_variable("test_var".to_string(), json!("value1"));
        assert_eq!(context.get_variable("test_var"), Some(json!("value1")));

        context.set_variable("test_var".to_string(), json!("value2"));
        assert_eq!(context.get_variable("test_var"), Some(json!("value2")));
    }

    #[test]
    fn test_resolve_inputs() {
        let mut inputs = HashMap::new();
        inputs.insert("api_url".to_string(), json!("https://api.example.com"));
        inputs.insert("timeout".to_string(), json!(30));

        let workflow = WorkflowDefinition {
            name: "测试工作流".to_string(),
            description: None,
            version: None,
            config: None,
            inputs: None,
            outputs: None,
            steps: vec![],
            on: None,
            trigger: None,
        };

        let context = ExecutionContext::new(&workflow, inputs);

        assert_eq!(
            context.resolve_path("inputs.api_url").unwrap(),
            json!("https://api.example.com")
        );
        assert_eq!(context.resolve_path("inputs.timeout").unwrap(), json!(30));
    }

    #[test]
    fn test_resolve_steps_path() {
        let workflow = WorkflowDefinition {
            name: "测试工作流".to_string(),
            description: None,
            version: None,
            config: None,
            inputs: None,
            outputs: None,
            steps: vec![],
            on: None,
            trigger: None,
        };

        let mut context = ExecutionContext::new(&workflow, HashMap::new());

        let step_result = StepResult::success(
            "deploy",
            json!({
                "response": {
                    "status_code": 200,
                    "body": {
                        "data": [
                            {"id": 1, "name": "item1"},
                            {"id": 2, "name": "item2"}
                        ]
                    }
                }
            }),
        );
        context.mark_step_completed("deploy".to_string(), step_result);

        assert_eq!(
            context
                .resolve_path("steps.deploy.response.status_code")
                .unwrap(),
            json!(200)
        );
        assert_eq!(
            context
                .resolve_path("steps.deploy.response.body.data[0].name")
                .unwrap(),
            json!("item1")
        );
    }

    #[test]
    fn test_evaluate_expression() {
        let mut inputs = HashMap::new();
        inputs.insert("api_url".to_string(), json!("https://api.example.com"));

        let workflow = WorkflowDefinition {
            name: "测试工作流".to_string(),
            description: None,
            version: None,
            config: None,
            inputs: None,
            outputs: None,
            steps: vec![],
            on: None,
            trigger: None,
        };

        let context = ExecutionContext::new(&workflow, inputs);

        // 测试 ${{ }} 语法
        assert_eq!(
            context.evaluate("${{ inputs.api_url }}").unwrap(),
            json!("https://api.example.com")
        );

        // 测试简单路径
        assert_eq!(
            context.evaluate("inputs.api_url").unwrap(),
            json!("https://api.example.com")
        );
    }

    #[test]
    fn test_step_completion_tracking() {
        let workflow = WorkflowDefinition {
            name: "测试工作流".to_string(),
            description: None,
            version: None,
            config: None,
            inputs: None,
            outputs: None,
            steps: vec![],
            on: None,
            trigger: None,
        };

        let mut context = ExecutionContext::new(&workflow, HashMap::new());

        assert!(!context.is_step_completed("step1"));
        assert!(!context.is_step_success("step1"));
        assert!(!context.is_step_failed("step1"));

        let result = StepResult::success("step1", json!(true));
        context.mark_step_completed("step1".to_string(), result);

        assert!(context.is_step_completed("step1"));
        assert!(context.is_step_success("step1"));
        assert!(!context.is_step_failed("step1"));

        let failed_result = StepResult::failed(
            "step2",
            crate::core::types::StepError {
                code: "ERROR".to_string(),
                message: "Test error".to_string(),
                fix: None,
            },
        );
        context.mark_step_failed("step2".to_string(), failed_result);

        assert!(context.is_step_completed("step2"));
        assert!(!context.is_step_success("step2"));
        assert!(context.is_step_failed("step2"));
    }
}
