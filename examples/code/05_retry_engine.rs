//! 中级示例 - 重试引擎
//!
//! 这个示例展示如何：
//! - 创建重试策略
//! - 执行带重试的操作
//! - 使用不同的退避策略

use flow_run::core::types::BackoffStrategy;
use flow_run::utils::retry::{RetryEngine, RetryPolicy};
use flow_run::utils::error::RetryError;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    println!("==========================================");
    println!("  flow-run - 中级示例：重试引擎");
    println!("==========================================\n");

    // 示例 1：固定延迟重试
    println!("[1] 固定延迟重试策略:");
    let policy1 = RetryPolicy {
        max_attempts: 3,
        strategy: BackoffStrategy::Fixed,
        initial_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(5),
        jitter: false,
        retryable_status_codes: vec![500, 502, 503],
        retryable_errors: vec!["NetworkError".to_string()],
    };
    let engine1 = RetryEngine::new(policy1);
    let counter1 = Arc::new(AtomicU32::new(0));
    let counter1_clone = counter1.clone();

    let result1 = engine1.execute(move || {
        let counter = counter1_clone.clone();
        async move {
            let attempt = counter.fetch_add(1, Ordering::SeqCst) + 1;
            println!("    尝试 #{}", attempt);
            if attempt < 3 {
                Err(RetryError::HttpStatus(500))
            } else {
                Ok::<String, RetryError>("成功!".to_string())
            }
        }
    }).await;

    println!("    结果: {:?}\n", result1);

    // 示例 2：指数退避重试
    println!("[2] 指数退避重试策略:");
    let policy2 = RetryPolicy {
        max_attempts: 4,
        strategy: BackoffStrategy::Exponential { factor: Some(2.0) },
        initial_delay: Duration::from_millis(50),
        max_delay: Duration::from_secs(1),
        jitter: true,
        retryable_status_codes: vec![500],
        retryable_errors: vec![],
    };
    let engine2 = RetryEngine::new(policy2);

    // 显示延迟计算
    println!("    延迟计算:");
    for attempt in 0..4 {
        let delay = engine2.calculate_delay(attempt);
        println!("    尝试 #{} 延迟: {:?}", attempt + 1, delay);
    }
    println!();

    // 示例 3：斐波那契退避
    println!("[3] 斐波那契退避策略:");
    let policy3 = RetryPolicy {
        max_attempts: 5,
        strategy: BackoffStrategy::Fibonacci,
        initial_delay: Duration::from_millis(10),
        max_delay: Duration::from_secs(1),
        jitter: false,
        retryable_status_codes: vec![],
        retryable_errors: vec![],
    };
    let engine3 = RetryEngine::new(policy3);

    println!("    延迟计算:");
    for attempt in 0..5 {
        let delay = engine3.calculate_delay(attempt);
        println!("    尝试 #{} 延迟: {:?}", attempt + 1, delay);
    }
    println!();

    // 示例 4：不可重试的错误
    println!("[4] 不可重试的错误:");
    let policy4 = RetryPolicy {
        max_attempts: 3,
        strategy: BackoffStrategy::Fixed,
        initial_delay: Duration::from_millis(10),
        max_delay: Duration::from_secs(1),
        jitter: false,
        retryable_status_codes: vec![500], // 只重试 500
        retryable_errors: vec![],
    };
    let engine4 = RetryEngine::new(policy4);
    let counter4 = Arc::new(AtomicU32::new(0));
    let counter4_clone = counter4.clone();

    let result4 = engine4.execute(move || {
        let counter = counter4_clone.clone();
        async move {
            let attempt = counter.fetch_add(1, Ordering::SeqCst) + 1;
            println!("    尝试 #{}", attempt);
            Err::<(), RetryError>(RetryError::HttpStatus(404)) // 404 不可重试
        }
    }).await;

    println!("    结果: {:?}", result4);
    println!("    总尝试次数: {}\n", counter4.load(Ordering::SeqCst));

    println!("==========================================");
    println!("  示例完成!");
    println!("==========================================");

    Ok(())
}
