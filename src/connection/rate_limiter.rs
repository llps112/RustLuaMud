//! 令牌桶限速器
//!
//! 控制命令发送速率，替代 Lua 侧的 nums/burst_count 限速机制。
//!
//! 服务端限速机制（LPC cmd.c）：
//! - cnt 计数器，每条命令 +1
//! - 每 2 秒 drain 40（clear_cmd_count: cnt -= 40，不低于 0）
//! - cnt > 60（3*CMDS_PER_TICK）→ 雷劈/unconscious/踢出
//! - cnt > 20（CMDS_PER_TICK）→ 小惩罚（扣气）
//!
//! 等效令牌桶：容量 60，每 2 秒补充 40
//!
//! 客户端限速策略：
//! - burst_size 条命令允许 0ms 间隔突发（初始满桶）
//! - 突发用完后强制 min_interval 间隔，确保 2 秒内命令数 ≤ 60
//! - 安全公式：burst_size + (1000/min_interval)*2 ≤ 60
//! - 突发能力在桶恢复满时重置（需空闲 burst_size/cmds_per_sec 秒）

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
    /// 上次实际发送时间，用于强制最小间隔
    last_send: Instant,
    /// 突发剩余次数：> 0 时跳过 min_interval，允许 0ms 间隔发送
    /// 初始 = capacity，每次突发 -1
    /// 只在 tokens 恢复到 capacity 时重置为 capacity
    burst_remaining: u64,
}

impl TokenBucket {
    pub fn new(capacity: u64, refill_rate: u64, min_interval_ms: u64) -> Self {
        Self {
            capacity: capacity as f64,
            tokens: capacity as f64,
            refill_rate: refill_rate as f64,
            last_refill: Instant::now(),
            min_interval: Duration::from_millis(min_interval_ms),
            // 初始化到 1 秒前，确保首次发送不受 min_interval 限制
            last_send: Instant::now() - Duration::from_secs(1),
            burst_remaining: capacity,
        }
    }

    /// 尝试获取一个令牌，返回需要等待的时间
    pub fn acquire(&mut self) -> Duration {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.capacity);
        self.last_refill = now;

        // 桶满时恢复突发能力
        if self.tokens >= self.capacity {
            self.burst_remaining = self.capacity as u64;
        }

        let wait = if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            if self.burst_remaining > 0 {
                // 突发模式：跳过 min_interval，允许 0ms 间隔
                self.burst_remaining -= 1;
                Duration::ZERO
            } else {
                // 匀速模式：强制 min_interval，防止连续无间隔命令超过服务端阈值
                let since_last = now.duration_since(self.last_send);
                if since_last < self.min_interval {
                    self.min_interval - since_last
                } else {
                    Duration::ZERO
                }
            }
        } else {
            // 令牌不足，等待补充；min_interval 作为下限防止速率过高
            let needed = 1.0 - self.tokens;
            let wait_secs = needed / self.refill_rate;
            let wait = Duration::from_secs_f64(wait_secs);
            wait.max(self.min_interval)
        };

        // last_send 记录预计的实际发送时间（acquire 时间 + wait），而非 acquire 时间
        // 这样下一次 acquire 才能正确计算距上次发送的间隔
        self.last_send = now + wait;
        wait
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_burst() {
        let mut bucket = TokenBucket::new(5, 10, 50);
        // 首次发送无需等待
        let wait = bucket.acquire();
        assert_eq!(wait, Duration::ZERO);
    }

    #[test]
    fn test_burst_capacity() {
        // burst=3, cmds_per_sec=20, min_interval=50ms
        // 前 3 条应立即发送（突发模式），第 4 条开始强制 50ms 间隔
        let mut bucket = TokenBucket::new(3, 20, 50);
        assert_eq!(bucket.acquire(), Duration::ZERO); // 突发 1
        assert_eq!(bucket.acquire(), Duration::ZERO); // 突发 2
        assert_eq!(bucket.acquire(), Duration::ZERO); // 突发 3
        let wait = bucket.acquire(); // 突发用完，强制 50ms
        assert!(wait >= Duration::from_millis(49));
    }

    #[test]
    fn test_depleted_bucket_waits() {
        let mut bucket = TokenBucket::new(2, 10, 50);
        bucket.acquire();
        bucket.acquire();
        let wait = bucket.acquire();
        assert!(wait >= Duration::from_millis(50));
    }

    #[test]
    fn test_refill_after_delay() {
        let mut bucket = TokenBucket::new(1, 10, 50);
        bucket.acquire();
        std::thread::sleep(Duration::from_millis(150));
        // 等待后令牌已补充且突发能力恢复
        let wait = bucket.acquire();
        assert_eq!(wait, Duration::ZERO);
    }

    #[test]
    fn test_burst_reset_on_full_refill() {
        // burst=2, cmds_per_sec=100, min_interval=50ms
        // 突发 2 条后需等桶恢复满（2/100=20ms）才能再次突发
        let mut bucket = TokenBucket::new(2, 100, 50);
        assert_eq!(bucket.acquire(), Duration::ZERO); // 突发 1
        assert_eq!(bucket.acquire(), Duration::ZERO); // 突发 2
        let wait = bucket.acquire(); // 突发用完，强制 50ms
        assert!(wait >= Duration::from_millis(49));

        // 等待桶恢复满（20ms 即可，但多等一些确保）
        std::thread::sleep(Duration::from_millis(50));
        // 桶已满，突发能力恢复
        let wait = bucket.acquire();
        assert_eq!(wait, Duration::ZERO); // 再次突发
    }

    #[test]
    fn test_no_micro_burst_after_burst_exhausted() {
        // 验证突发用完后无微突发：严格 50ms 间隔
        // burst=1, cmds_per_sec=20, min_interval=50ms
        let mut bucket = TokenBucket::new(1, 20, 50);
        let wait1 = bucket.acquire(); // 突发 1
        assert_eq!(wait1, Duration::ZERO);
        // 突发用完，后续严格 50ms 间隔
        let wait2 = bucket.acquire();
        assert!(wait2 >= Duration::from_millis(49));
    }

    #[test]
    fn test_steady_state_after_burst() {
        // 模拟连线场景：burst=20, cmds_per_sec=20, min_interval=50ms
        // 前 20 条突发，之后严格 50ms 间隔
        let mut bucket = TokenBucket::new(20, 20, 50);
        let mut burst_count = 0;
        let mut interval_count = 0;

        for i in 0..30 {
            let wait = bucket.acquire();
            if i < 20 {
                // 前 20 条应立即发送
                assert_eq!(wait, Duration::ZERO, "cmd {} should be burst", i + 1);
                burst_count += 1;
            } else {
                // 后 10 条应等 ~50ms
                assert!(
                    wait >= Duration::from_millis(49),
                    "cmd {} should wait, got {:?}",
                    i + 1,
                    wait
                );
                interval_count += 1;
            }
        }
        assert_eq!(burst_count, 20);
        assert_eq!(interval_count, 10);
    }
}
