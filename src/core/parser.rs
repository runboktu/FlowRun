use crate::core::types::*;
use crate::utils::error::WorkflowError;
use serde_yaml;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// 工作流解析器
///
/// 负责从 YAML 文件或字符串解析工作流定义，并进行验证
pub struct WorkflowParser;

impl WorkflowParser {
    /// 从文件加载工作流定义
    ///
    /// # 参数
    ///
    /// * `path` - YAML 文件路径
    ///
    /// # 返回
    ///
    /// 成功返回解析后的工作流定义，失败返回错误
    ///
    /// # 示例
    ///
    /// ```no_run
    /// use flow_run::core::parser::WorkflowParser;
    ///
    /// let workflow = WorkflowParser::from_file("workflow.yaml")?;
    /// ```
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<WorkflowDefinition, WorkflowError> {
        let path = path.as_ref();

        // 检查文件是否存在
        if !path.exists() {
            return Err(WorkflowError::WorkflowFileNotFound {
                path: path.to_string_lossy().to_string(),
            });
        }

        // 读取文件内容
        let content = fs::read_to_string(path).map_err(|e| WorkflowError::Io(e))?;

        // 解析 YAML
        Self::from_str(&content)
    }

    /// 从字符串解析工作流定义
    ///
    /// # 参数
    ///
    /// * `content` - YAML 格式的工作流定义字符串
    ///
    /// # 返回
    ///
    /// 成功返回解析后的工作流定义，失败返回错误
    ///
    /// # 示例
    ///
    /// ```
    /// use flow_run::core::parser::WorkflowParser;
    ///
    /// let yaml = r#"
    /// name: "示例工作流"
    /// steps: []
    /// "#;
    /// let workflow = WorkflowParser::from_str(yaml)?;
    /// ```
    pub fn from_str(content: &str) -> Result<WorkflowDefinition, WorkflowError> {
        // 使用 serde_yaml 反序列化
        serde_yaml::from_str(content).map_err(|e| WorkflowError::YamlParseError {
            reason: e.to_string(),
        })
    }

    /// 验证工作流定义
    ///
    /// 检查工作流定义的完整性，包括：
    /// - 步骤 ID 唯一性
    /// - 依赖关系有效性
    /// - 循环依赖检测
    /// - 必填字段检查
    ///
    /// # 参数
    ///
    /// * `workflow` - 待验证的工作流定义
    ///
    /// # 返回
    ///
    /// 验证通过返回 Ok()，失败返回错误信息
    ///
    /// # 示例
    ///
    /// ```no_run
    /// use flow_run::core::parser::WorkflowParser;
    ///
    /// let workflow = WorkflowParser::from_file("workflow.yaml")?;
    /// WorkflowParser::validate(&workflow)?;
    /// ```
    pub fn validate(workflow: &WorkflowDefinition) -> Result<(), WorkflowError> {
        // 检查步骤 ID 唯一性
        Self::validate_step_ids(&workflow.steps)?;

        // 检查依赖关系有效性
        Self::validate_dependencies(&workflow.steps)?;

        // 检查循环依赖
        Self::validate_no_cycles(&workflow.steps)?;

        Ok(())
    }

    /// 验证步骤 ID 唯一性
    fn validate_step_ids(steps: &[StepDefinition]) -> Result<(), WorkflowError> {
        let mut seen = HashSet::new();

        for step in steps {
            if !seen.insert(&step.id) {
                return Err(WorkflowError::SchemaValidationError {
                    message: format!("重复的步骤 ID: {}", step.id),
                });
            }

            // 递归验证子步骤
            if let Some(sub_steps) = &step.steps {
                Self::validate_step_ids(sub_steps)?;
            }
            if let Some(then_steps) = &step.then_steps {
                Self::validate_step_ids(then_steps)?;
            }
            if let Some(else_steps) = &step.else_steps {
                Self::validate_step_ids(else_steps)?;
            }
            if let Some(do_steps) = &step.do_steps {
                Self::validate_step_ids(do_steps)?;
            }
        }

        Ok(())
    }

    /// 验证依赖关系有效性
    fn validate_dependencies(steps: &[StepDefinition]) -> Result<(), WorkflowError> {
        // 收集所有步骤 ID
        let mut all_step_ids: HashSet<String> = HashSet::new();
        Self::collect_step_ids(steps, &mut all_step_ids);

        // 验证每个步骤的依赖
        for step in steps {
            if let Some(depends_on) = &step.depends_on {
                for dep_id in depends_on {
                    if !all_step_ids.contains(dep_id) {
                        return Err(WorkflowError::SchemaValidationError {
                            message: format!("步骤 {} 依赖的步骤 {} 不存在", step.id, dep_id),
                        });
                    }
                }
            }

            // 递归验证子步骤
            if let Some(sub_steps) = &step.steps {
                Self::validate_dependencies(sub_steps)?;
            }
        }

        Ok(())
    }

    /// 收集所有步骤 ID（包括子步骤）
    fn collect_step_ids(steps: &[StepDefinition], all_ids: &mut HashSet<String>) {
        for step in steps {
            all_ids.insert(step.id.clone());

            if let Some(sub_steps) = &step.steps {
                Self::collect_step_ids(sub_steps, all_ids);
            }
            if let Some(then_steps) = &step.then_steps {
                Self::collect_step_ids(then_steps, all_ids);
            }
            if let Some(else_steps) = &step.else_steps {
                Self::collect_step_ids(else_steps, all_ids);
            }
            if let Some(do_steps) = &step.do_steps {
                Self::collect_step_ids(do_steps, all_ids);
            }
        }
    }

    /// 检查循环依赖
    ///
    /// 使用深度优先搜索（DFS）检测循环依赖
    fn validate_no_cycles(steps: &[StepDefinition]) -> Result<(), WorkflowError> {
        // 构建依赖图
        let mut graph: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();

        for step in steps {
            let step_id = step.id.clone();

            // 初始化节点
            graph.entry(step_id.clone()).or_insert_with(Vec::new);

            // 添加依赖边（被依赖的步骤指向当前步骤）
            if let Some(depends_on) = &step.depends_on {
                for dep_id in depends_on {
                    graph
                        .entry(dep_id.clone())
                        .or_insert_with(Vec::new)
                        .push(step_id.clone());
                }
            }
        }

        // 检测循环（DFS）
        let mut visited = HashSet::new();
        let mut recursion_stack = HashSet::new();

        for step_id in graph.keys() {
            if !visited.contains(step_id) {
                if Self::dfs_has_cycle(step_id, &graph, &mut visited, &mut recursion_stack)? {
                    return Err(WorkflowError::CycleDetected);
                }
            }
        }

        Ok(())
    }

    /// DFS 检测循环依赖
    fn dfs_has_cycle(
        node: &str,
        graph: &std::collections::HashMap<String, Vec<String>>,
        visited: &mut HashSet<String>,
        recursion_stack: &mut HashSet<String>,
    ) -> Result<bool, WorkflowError> {
        // 标记为已访问
        visited.insert(node.to_string());
        // 加入递归栈
        recursion_stack.insert(node.to_string());

        // 遍历所有邻居
        if let Some(neighbors) = graph.get(node) {
            for neighbor in neighbors {
                // 如果不在已访问集合中，递归访问
                if !visited.contains(neighbor) {
                    if Self::dfs_has_cycle(neighbor, graph, visited, recursion_stack)? {
                        return Ok(true);
                    }
                }
                // 如果在递归栈中，说明存在循环
                else if recursion_stack.contains(neighbor) {
                    return Ok(true);
                }
            }
        }

        // 从递归栈中移除
        recursion_stack.remove(node);

        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_str_valid() {
        let yaml = r#"
name: "测试工作流"
version: "1.0.0"
steps:
  - id: "step1"
    type: "http"
    api: "https://example.com/api"
    method: "GET"
"#;

        let result = WorkflowParser::from_str(yaml);
        assert!(result.is_ok());

        let workflow = result.unwrap();
        assert_eq!(workflow.name, "测试工作流");
        assert_eq!(workflow.version, Some("1.0.0".to_string()));
        assert_eq!(workflow.steps.len(), 1);
        assert_eq!(workflow.steps[0].id, "step1");
    }

    #[test]
    fn test_validate_unique_step_ids() {
        let mut workflow = WorkflowDefinition {
            name: "测试工作流".to_string(),
            description: None,
            version: None,
            config: None,
            inputs: None,
            outputs: None,
            steps: vec![
                StepDefinition {
                    id: "step1".to_string(),
                    r#type: StepType::Http,
                    name: None,
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
                    run: None,
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
                    id: "step1".to_string(),
                    r#type: StepType::Http,
                    name: None,
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
                    run: None,
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
            ],
            on: None,
            trigger: None,
            variables: None,
        };

        let result = WorkflowParser::validate(&workflow);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_dependencies() {
        let workflow = WorkflowDefinition {
            name: "测试工作流".to_string(),
            description: None,
            version: None,
            config: None,
            inputs: None,
            outputs: None,
            steps: vec![
                StepDefinition {
                    id: "step1".to_string(),
                    r#type: StepType::Http,
                    name: None,
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
                    run: None,
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
                    r#type: StepType::Http,
                    name: None,
                    depends_on: Some(vec!["step1".to_string(), "step3".to_string()]),
                    expect: None,
                    retry: None,
                    timeout: None,
                    hooks: None,
                    api: None,
                    method: None,
                    headers: None,
                    body: None,
                    cache: None,
                    run: None,
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
            ],
            on: None,
            trigger: None,
            variables: None,
        };

        let result = WorkflowParser::validate(&workflow);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_no_cycles() {
        let workflow = WorkflowDefinition {
            name: "测试工作流".to_string(),
            description: None,
            version: None,
            config: None,
            inputs: None,
            outputs: None,
            steps: vec![
                StepDefinition {
                    id: "step1".to_string(),
                    r#type: StepType::Http,
                    name: None,
                    depends_on: Some(vec!["step3".to_string()]),
                    expect: None,
                    retry: None,
                    timeout: None,
                    hooks: None,
                    api: None,
                    method: None,
                    headers: None,
                    body: None,
                    cache: None,
                    run: None,
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
                    r#type: StepType::Http,
                    name: None,
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
                    run: None,
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
                    id: "step3".to_string(),
                    r#type: StepType::Http,
                    name: None,
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
                    run: None,
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
            ],
            on: None,
            trigger: None,
            variables: None,
        };

        let result = WorkflowParser::validate(&workflow);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), WorkflowError::CycleDetected));
    }

    #[test]
    fn test_validate_valid_workflow() {
        let workflow = WorkflowDefinition {
            name: "测试工作流".to_string(),
            description: None,
            version: None,
            config: None,
            inputs: None,
            outputs: None,
            steps: vec![
                StepDefinition {
                    id: "step1".to_string(),
                    r#type: StepType::Http,
                    name: None,
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
                    run: None,
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
                    r#type: StepType::Http,
                    name: None,
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
                    run: None,
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
                    id: "step3".to_string(),
                    r#type: StepType::Shell,
                    name: None,
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
                    run: None,
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
            ],
            on: None,
            trigger: None,
            variables: None,
        };

        let result = WorkflowParser::validate(&workflow);
        assert!(result.is_ok());
    }
}
