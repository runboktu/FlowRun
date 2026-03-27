//! flow-run: 专为 AI Agent 设计的声明式工作流引擎
//!
//! 核心功能:
//! - YAML 声明式工作流定义
//! - DAG 调度引擎，自动并行执行无依赖步骤
//! - 检查点与断点续跑
//! - 多种步骤执行器 (HTTP/Shell/Loop/Condition/Workflow/Approve)
//! - 模板表达式与过滤器链
//! - 重试引擎与错误处理

pub mod cli;
pub mod core;
pub mod executors;
pub mod utils;
