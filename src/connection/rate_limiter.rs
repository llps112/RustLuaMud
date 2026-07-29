//! 令牌桶限速器
//!
//! 控制命令发送速率，替代 Lua 侧的 nums/burst_count 限速机制。
//! 桶容量（burst_size）允许突发，补充速率（refill_rate）控制长期平均速率。

use std::time::{Duration, Instant};

pub struct TokenBucket {
    /// 桶容量（最大突发令牌数）
    capacity: f64,
    /// 当前令牌数
    tokens: f64,
    /// 每秒补充令牌数
    refill_rate: f64,
    /// 上次补充时间
    last_refill: Instant,
    /// 最小发送间隔，作为令牌桶的下限保护
    min_interval: Duration,
}

impl TokenBucket {
    pub fn new(capacity: u64, refill_rate: u64, min_interval_ms: u64) -> Self {
        Self {
            capacity: capacity as f64,
            tokens: capacity as f64,
            refill_rate: refill_rate as f64,
            last_refill: Instant::now(),
            min_interval: Duration::from_millis(min_interval_ms),
        }
    }

    /// 尝试获取一个令牌，返回需要等待的时间
    pub fn acquire(&mut self) -> Duration {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.capacity);
        self.last_refill = now;

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            // 令牌充足时无需等待，允许突发（burst_size 控制突发上限）
            Duration::ZERO
        } else {
            // 令牌不足，等待补充；min_interval 作为下限防止速率过高
            let needed = 1.0 - self.tokens;
            let wait_secs = needed / self.refill_rate;
            let wait = Duration::from_secs_f64(wait_secs);
            wait.max(self.min_interval)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_burst() {
        let mut bucket = TokenBucket::new(5, 10, 50);
        // 令牌充足时无需等待，允许突发
        let wait = bucket.acquire();
        assert_eq!(wait, Duration::ZERO);
    }

    #[test]
    fn test_depleted_bucket_waits() {
        let mut bucket = TokenBucket::new(2, 10, 50);
        bucket.acquire();
        bucket.acquire();
        let wait = bucket.acquire();
        assert!(wait > Duration::from_millis(50));
    }

    #[test]
    fn test_refill_after_delay() {
        let mut bucket = TokenBucket::new(1, 10, 50);
        bucket.acquire();
        std::thread::sleep(Duration::from_millis(150));
        // 等待后令牌已补充，无需等待（允许突发）
        let wait = bucket.acquire();
        assert_eq!(wait, Duration::ZERO);
    }
}
