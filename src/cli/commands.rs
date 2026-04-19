use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// flow-run - 专为 AI Agent 设计的声明式工作流引擎
#[derive(Parser, Debug)]
#[command(name = "flow-run")]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// 工作流定义文件路径（YAML 或 JSON）
    #[arg(value_name = "WORKFLOW_FILE")]
    pub workflow: PathBuf,

    /// 启用详细日志输出
    #[arg(short, long)]
    pub verbose: bool,

    /// 指定配置文件
    #[arg(short = 'C', long)]
    pub config: Option<PathBuf>,

    /// 子命令
    #[command(subcommand)]
    pub command: Commands,
}

/// CLI 子命令
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// 执行工作流
    Run {
        /// 工作流执行模式
        #[command(flatten)]
        mode: ExecutionMode,

        /// 输入参数（格式：--input key=value，可多次使用）
        #[arg(short, long, value_parser = parse_key_value)]
        input: Vec<(String, String)>,

        /// 以 JSON 格式输出结果
        #[arg(long)]
        json: bool,

        /// 模拟执行（不实际运行步骤）
        #[arg(long)]
        dry_run: bool,

        /// 从指定步骤继续执行（使用上次失败时保存的上下文）
        #[arg(long, value_name = "STEP_ID")]
        from_step: Option<String>,
    },

    /// 从检查点恢复工作流执行
    Resume {
        /// 检查点 ID
        #[arg(long)]
        checkpoint_id: String,

        /// 覆盖输入参数（格式：--input key=value）
        #[arg(short, long, value_parser = parse_key_value)]
        input: Vec<(String, String)>,

        /// 以 JSON 格式输出结果
        #[arg(long)]
        json: bool,
    },

    /// 验证工作流定义
    Validate {
        /// 显示 DAG（有向无环图）结构
        #[arg(long)]
        show_dag: bool,

        /// 以 JSON 格式输出验证结果
        #[arg(long)]
        json: bool,
    },

    /// 模拟执行工作流
    DryRun {
        /// 输入参数（格式：--input key=value）
        #[arg(short, long, value_parser = parse_key_value)]
        input: Vec<(String, String)>,

        /// 以 JSON 格式输出结果
        #[arg(long)]
        json: bool,
    },

    /// 检查点管理
    Checkpoint {
        /// 检查点子命令
        #[command(subcommand)]
        action: CheckpointAction,
    },

    /// 查看执行历史
    History {
        /// 最大显示条数
        #[arg(short, long, default_value = "20")]
        limit: usize,

        /// 过滤状态
        #[arg(long)]
        status: Option<String>,

        /// 只显示失败的执行
        #[arg(long)]
        failed: bool,

        /// 以 JSON 格式输出
        #[arg(long)]
        json: bool,
    },

    /// 输出工作流定义的 JSON Schema
    Schema {
        /// 输出到文件
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// 美化输出
        #[arg(long)]
        pretty: bool,
    },
}

/// 工作流执行模式
#[derive(clap::Args, Debug)]
pub struct ExecutionMode {
    /// 普通执行模式（默认）
    #[arg(long, default_value = "false")]
    pub normal: bool,

    /// 异步执行模式
    #[arg(long, default_value = "false")]
    pub async_mode: bool,

    /// 守护进程模式
    #[arg(long, default_value = "false")]
    pub daemon: bool,
}

/// 检查点操作
#[derive(Subcommand, Debug)]
pub enum CheckpointAction {
    /// 列出所有检查点
    List {
        /// 显示详细信息
        #[arg(short, long)]
        verbose: bool,

        /// 按状态过滤
        #[arg(long)]
        status: Option<String>,

        /// 以 JSON 格式输出
        #[arg(long)]
        json: bool,
    },

    /// 显示检查点详情
    Show {
        /// 检查点 ID
        #[arg(value_name = "CHECKPOINT_ID")]
        id: String,

        /// 显示步骤详情
        #[arg(short, long)]
        steps: bool,

        /// 以 JSON 格式输出
        #[arg(long)]
        json: bool,
    },

    /// 清理检查点
    Clean {
        /// 清理策略
        #[command(subcommand)]
        strategy: CleanStrategy,
    },
}

/// 清理策略
#[derive(Subcommand, Debug)]
pub enum CleanStrategy {
    /// 清理指定检查点
    Id {
        /// 检查点 ID（可多个）
        #[arg(value_name = "CHECKPOINT_IDS")]
        ids: Vec<String>,
    },

    /// 清理所有检查点
    All {
        /// 确认清理操作
        #[arg(long)]
        confirm: bool,
    },

    /// 清理超过指定天数的检查点
    OlderThan {
        /// 天数
        #[arg(short, long)]
        days: u32,
    },

    /// 清理特定状态的检查点
    Status {
        /// 状态（如：paused, failed, completed）
        #[arg(value_name = "STATUS")]
        status: String,
    },

    /// 清理指定数量之前的检查点
    Keep {
        /// 保留的检查点数量
        #[arg(short = 'n', long)]
        count: usize,
    },
}

/// 解析 key=value 格式的参数
fn parse_key_value(s: &str) -> anyhow::Result<(String, String)> {
    let parts: Vec<&str> = s.splitn(2, '=').collect();
    if parts.len() != 2 {
        anyhow::bail!("参数格式错误：'{}'，应为 key=value", s);
    }
    if parts[0].is_empty() {
        anyhow::bail!("参数格式错误：'{}'，key 不能为空", s);
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

impl Args {
    /// 解析并返回工作流执行参数
    pub fn parse_workflow_args(&self) -> anyhow::Result<WorkflowArgs> {
        let (inputs, json, dry_run, from_step) = match &self.command {
            Commands::Run {
                input,
                json,
                dry_run,
                from_step,
                ..
            } => (input.clone(), *json, *dry_run, from_step.clone()),
            Commands::Resume { input, json, .. } => (input.clone(), *json, false, None),
            Commands::DryRun { input, json, .. } => (input.clone(), *json, true, None),
            _ => return Ok(WorkflowArgs::default()),
        };

        Ok(WorkflowArgs {
            workflow_file: self.workflow.clone(),
            inputs,
            json,
            dry_run,
            verbose: self.verbose,
            from_step,
        })
    }
}

/// 工作流执行参数
#[derive(Debug, Clone)]
pub struct WorkflowArgs {
    /// 工作流文件路径
    pub workflow_file: PathBuf,
    /// 输入参数
    pub inputs: Vec<(String, String)>,
    /// 是否输出 JSON 格式
    pub json: bool,
    /// 是否为模拟执行
    pub dry_run: bool,
    /// 是否启用详细日志
    pub verbose: bool,
    /// 从指定步骤继续执行
    pub from_step: Option<String>,
}

impl Default for WorkflowArgs {
    fn default() -> Self {
        Self {
            workflow_file: PathBuf::new(),
            inputs: Vec::new(),
            json: false,
            dry_run: false,
            verbose: false,
            from_step: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_key_value() {
        let result = parse_key_value("key=value").unwrap();
        assert_eq!(result, (String::from("key"), String::from("value")));

        let result = parse_key_value("name=John Doe").unwrap();
        assert_eq!(result, (String::from("name"), String::from("John Doe")));

        assert!(parse_key_value("invalid").is_err());
        assert!(parse_key_value("=").is_err());
    }
}
