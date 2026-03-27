use crate::core::types::BackoffStrategy;
use crate::utils::error::RetryError;
use rand::Rng;
use std::time::Duration;
use std::ops::Fn;

/// 重试策略配置
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// 最大重试次数
    pub max_attempts: u32,
    /// 退避策略
    pub strategy: BackoffStrategy,
    /// 初始延迟
    pub initial_delay: Duration,
    /// 最大延迟
    pub max_delay: Duration,
    /// 是否启用抖动
    pub jitter: bool,
    /// 可重试的 HTTP 状态码
    pub retryable_status_codes: Vec<u16>,
    /// 可重试的错误类型
    pub retryable_errors: Vec<String>,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            strategy: BackoffStrategy::Exponential { factor: Some(2.0) },
            initial_delay: Duration::from_millis(1000),
            max_delay: Duration::from_secs(30),
            jitter: true,
            retryable_status_codes: vec![408, 429, 500, 502, 503, 504],
            retryable_errors: vec![
                "NetworkError".to_string(),
                "Timeout".to_string(),
                "ConnectionReset".to_string(),
            ],
        }
    }
}

/// 重试引擎
#[derive(Debug, Clone)]
pub struct RetryEngine {
    /// 重试策略
    pub policy: RetryPolicy,
}

impl RetryEngine {
    /// 创建新的重试引擎
    pub fn new(policy: RetryPolicy) -> Self {
        Self { policy }
    }

    /// 使用默认策略创建重试引擎
    pub fn default_policy() -> Self {
        Self::new(RetryPolicy::default())
    }

    /// 执行带重试的操作
    ///
    /// # 参数
    /// * `operation` - 要执行的操作，返回 Result<T, RetryError>
    ///
    /// # 返回
    /// 操作的结果，如果所有重试都失败则返回最后一个错误
    pub async fn execute<T, F, Fut>(&self, mut operation: F) -> Result<T, RetryError>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, RetryError>>,
    {
        let mut last_error = None;

        for attempt in 0..self.policy.max_attempts {
            match operation().await {
                Ok(result) => {
                    if attempt > 0 {
                        tracing::debug!(
                            "操作在第 {} 次尝试后成功",
                            attempt + 1
                        );
                    }
                    return Ok(result);
                }
                Err(err) => {
                    last_error = Some(err.clone());

                    // 检查是否可重试
                    if !self.is_retryable(&err) {
                        tracing::debug!("错误不可重试: {:?}", err);
                        return Err(err);
                    }

                    // 如果是最后一次尝试，直接返回错误
                    if attempt + 1 >= self.policy.max_attempts {
                        tracing::warn!(
                            "已达到最大重试次数 ({}), 放弃重试",
                            self.policy.max_attempts
                        );
                        return Err(err);
                    }

                    // 计算延迟并等待
                    let delay = self.calculate_delay(attempt);
                    tracing::info!(
                        "第 {} 次尝试失败，{:?} 后重试: {:?}",
                        attempt + 1,
                        delay,
                        err
                    );

                    tokio::time::sleep(delay).await;
                }
            }
        }

        Err(last_error.unwrap_or(RetryError::MaxAttemptsExceeded))
    }

    /// 计算重试延迟时间
    ///
    /// # 参数
    /// * `attempt` - 当前尝试次数（从 0 开始）
    ///
    /// # 返回
    /// 计算出的延迟时间
    pub fn calculate_delay(&self, attempt: u32) -> Duration {
        let base_delay = match &self.policy.strategy {
            BackoffStrategy::Fixed => self.policy.initial_delay,
            BackoffStrategy::Exponential { factor } => {
                let factor = factor.unwrap_or(2.0);
                let multiplier = factor.powi(attempt as i32);
                let delay_ms = self.policy.initial_delay.as_millis() as f64 * multiplier;
                Duration::from_millis(delay_ms as u64)
            }
            BackoffStrategy::Fibonacci => {
                let fib = self.fibonacci(attempt as usize + 1);
                let delay_ms = self.policy.initial_delay.as_millis() as u64 * fib as u64;
                Duration::from_millis(delay_ms)
            }
        };

        // 应用最大延迟限制
        let delay = base_delay.min(self.policy.max_delay);

        // 应用抖动（随机化延迟，避免惊群效应）
        if self.policy.jitter {
            let mut rng = rand::thread_rng();
            let jitter_factor = rng.gen_range(0.8..=1.2);
            let jittered_ms = (delay.as_millis() as f64 * jitter_factor) as u64;
            Duration::from_millis(jittered_ms)
        } else {
            delay
        }
    }

    /// 判断错误是否可重试
    ///
    /// # 参数
    /// * `error` - 重试错误
    ///
    /// # 返回
    /// true 表示可重试，false 表示不可重试
    pub fn is_retryable(&self, error: &RetryError) -> bool {
        match error {
            RetryError::HttpStatus(status) => {
                self.policy.retryable_status_codes.contains(status)
            }
            RetryError::Network(err) => {
                // 检查错误类型是否在可重试列表中
                self.policy.retryable_errors.iter().any(|retryable_type| {
                    err.contains(retryable_type)
                })
            }
            RetryError::MaxAttemptsExceeded => false,
            RetryError::NotRetryable(_) => false,
        }
    }

    /// 计算斐波那契数列的第 n 项
    ///
    /// # 参数
    /// * `n` - 要计算的项数（从 1 开始）
    ///
    /// # 返回
    /// 斐波那契数列的第 n 项
    ///
    /// # 示例
    /// ```
    /// let engine = RetryEngine::default_policy();
    /// assert_eq!(engine.fibonacci(1), 1);
    /// assert_eq!(engine.fibonacci(2), 1);
    /// assert_eq!(engine.fibonacci(3), 2);
    /// assert_eq!(engine.fibonacci(4), 3);
    /// assert_eq!(engine.fibonacci(5), 5);
    /// ```
    pub fn fibonacci(&self, n: usize) -> u64 {
        if n <= 1 {
            return n as u64;
        }

        let mut a = 0u64;
        let mut b = 1u64;

        for _ in 2..=n {
            let temp = a + b;
            a = b;
            b = temp;
        }

        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fibonacci() {
        let engine = RetryEngine::default_policy();
        assert_eq!(engine.fibonacci(1), 1);
        assert_eq!(engine.fibonacci(2), 1);
        assert_eq!(engine.fibonacci(3), 2);
        assert_eq!(engine.fibonacci(4), 3);
        assert_eq!(engine.fibonacci(5), 5);
        assert_eq!(engine.fibonacci(6), 8);
        assert_eq!(engine.fibonacci(10), 55);
    }

    #[test]
    fn test_calculate_delay_fixed() {
        let policy = RetryPolicy {
            max_attempts: 3,
            strategy: BackoffStrategy::Fixed,
            initial_delay: Duration::from_millis(1000),
            max_delay: Duration::from_secs(30),
            jitter: false,
            retryable_status_codes: vec![],
            retryable_errors: vec![],
        };
        let engine = RetryEngine::new(policy);

        assert_eq!(engine.calculate_delay(0), Duration::from_millis(1000));
        assert_eq!(engine.calculate_delay(1), Duration::from_millis(1000));
        assert_eq!(engine.calculate_delay(2), Duration::from_millis(1000));
    }

    #[test]
    fn test_calculate_delay_exponential() {
        let policy = RetryPolicy {
            max_attempts: 3,
            strategy: BackoffStrategy::Exponential { factor: Some(2.0) },
            initial_delay: Duration::from_millis(1000),
            max_delay: Duration::from_secs(30),
            jitter: false,
            retryable_status_codes: vec![],
            retryable_errors: vec![],
        };
        let engine = RetryEngine::new(policy);

        assert_eq!(engine.calculate_delay(0), Duration::from_millis(1000));
        assert_eq!(engine.calculate_delay(1), Duration::from_millis(2000));
        assert_eq!(engine.calculate_delay(2), Duration::from_millis(4000));
    }

    #[test]
    fn test_calculate_delay_fibonacci() {
        let policy = RetryPolicy {
            max_attempts: 3,
            strategy: BackoffStrategy::Fibonacci,
            initial_delay: Duration::from_millis(1000),
            max_delay: Duration::from_secs(30),
            jitter: false,
            retryable_status_codes: vec![],
            retryable_errors: vec![],
        };
        let engine = RetryEngine::new(policy);

        assert_eq!(engine.calculate_delay(0), Duration::from_millis(1000)); // fib(1) = 1
        assert_eq!(engine.calculate_delay(1), Duration::from_millis(1000)); // fib(2) = 1
        assert_eq!(engine.calculate_delay(2), Duration::from_millis(2000)); // fib(3) = 2
        assert_eq!(engine.calculate_delay(3), Duration::from_millis(3000)); // fib(4) = 3
    }

    #[test]
    fn test_max_delay_limit() {
        let policy = RetryPolicy {
            max_attempts: 10,
            strategy: BackoffStrategy::Exponential { factor: Some(10.0) },
            initial_delay: Duration::from_millis(1000),
            max_delay: Duration::from_millis(5000), // 小的最大延迟
            jitter: false,
            retryable_status_codes: vec![],
            retryable_errors: vec![],
        };
        let engine = RetryEngine::new(policy);

        // 即使指数增长很快，也不会超过最大延迟
        assert_eq!(engine.calculate_delay(0), Duration::from_millis(1000));
        assert_eq!(engine.calculate_delay(1), Duration::from_millis(5000)); // 被 max_delay 限制
        assert_eq!(engine.calculate_delay(2), Duration::from_millis(5000));
    }

    #[test]
    fn test_is_retryable() {
        let policy = RetryPolicy {
            max_attempts: 3,
            strategy: BackoffStrategy::Fixed,
            initial_delay: Duration::from_millis(1000),
            max_delay: Duration::from_secs(30),
            jitter: false,
            retryable_status_codes: vec![500, 503],
            retryable_errors: vec!["NetworkError".to_string()],
        };
        let engine = RetryEngine::new(policy);

        // 可重试的 HTTP 状态码
        assert!(engine.is_retryable(&RetryError::HttpStatus(500)));
        assert!(engine.is_retryable(&RetryError::HttpStatus(503)));

        // 不可重试的 HTTP 状态码
        assert!(!engine.is_retryable(&RetryError::HttpStatus(404)));
        assert!(!engine.is_retryable(&RetryError::HttpStatus(400)));

        // 可重试的网络错误
        assert!(engine.is_retryable(&RetryError::Network(
            "ConnectionReset NetworkError".to_string()
        )));

        // 不可重试的错误
        assert!(!engine.is_retryable(&RetryError::MaxAttemptsExceeded));
        assert!(!engine.is_retryable(&RetryError::NotRetryable("InvalidConfig".to_string())));
    }

    #[tokio::test]
    async fn test_execute_success_on_first_attempt() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};

        let engine = RetryEngine::default_policy();
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let result = engine.execute(move || {
            let counter = counter_clone.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok::<i32, RetryError>(42)
            }
        }).await;

        assert_eq!(result, Ok(42));
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_execute_success_after_retry() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};

        let engine = RetryEngine::new(RetryPolicy {
            max_attempts: 5,
            strategy: BackoffStrategy::Fixed,
            initial_delay: Duration::from_millis(10),
            max_delay: Duration::from_millis(100),
            jitter: false,
            retryable_status_codes: vec![500],
            retryable_errors: vec![],
        });
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let result = engine.execute(move || {
            let counter = counter_clone.clone();
            async move {
                let current = counter.fetch_add(1, Ordering::SeqCst) + 1;
                if current < 3 {
                    Err(RetryError::HttpStatus(500))
                } else {
                    Ok::<i32, RetryError>(42)
                }
            }
        }).await;

        assert_eq!(result, Ok(42));
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_execute_retries() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};

        let engine = RetryEngine::new(RetryPolicy {
            max_attempts: 2,
            strategy: BackoffStrategy::Fixed,
            initial_delay: Duration::from_millis(10),
            max_delay: Duration::from_millis(100),
            jitter: false,
            retryable_status_codes: vec![500],
            retryable_errors: vec![],
        });
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let result = engine.execute(move || {
            let counter = counter_clone.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Err::<i32, RetryError>(RetryError::HttpStatus(500))
            }
        }).await;

        assert_eq!(result, Err(RetryError::HttpStatus(500)));
        assert_eq!(counter.load(Ordering::SeqCst), 2); // 尝试了 max_attempts 次
    }

    #[tokio::test]
    async fn test_execute_not_retryable() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};

        let engine = RetryEngine::new(RetryPolicy {
            max_attempts: 10,
            strategy: BackoffStrategy::Fixed,
            initial_delay: Duration::from_millis(10),
            max_delay: Duration::from_millis(100),
            jitter: false,
            retryable_status_codes: vec![500],
            retryable_errors: vec![],
        });
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let result = engine.execute(move || {
            let counter = counter_clone.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Err::<i32, RetryError>(RetryError::HttpStatus(404)) // 不可重试
            }
        }).await;

        assert_eq!(result, Err(RetryError::HttpStatus(404)));
        assert_eq!(counter.load(Ordering::SeqCst), 1); // 只尝试一次
    }
}
