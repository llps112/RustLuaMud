//! 命令限速器：令牌桶 + 滑动窗口
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
//! ## TokenBucket（速率平滑）
//! - burst_size 条命令允许 0ms 间隔突发（初始满桶）
//! - 突发用完后强制 min_interval 间隔
//! - 令牌不足时预扣（余额可为负），因此任意 T 内发送条数 ≤ burst_size + cmds_per_sec×T
//! - 长期速率上界 = cmds_per_sec。min_interval 只约束非突发模式下的相邻间隔，
//!   不构成长期速率上限：当 min_interval > 1000/cmds_per_sec 时，富余令牌会
//!   累积到 burst_size 再以突发形式花掉，长期速率仍为 cmds_per_sec
//! - 单次突发安全公式：burst_size + 2×cmds_per_sec ≤ 60
//! - 突发能力在桶恢复满时重置（需空闲 (burst_size+1)/cmds_per_sec 秒）
//!
//! ## TokenBucket 的局限
//! 上述安全公式只约束「单次突发 + 随后 2 秒匀速」。若上层（如 GPS 寻路重试）
//! 在桶恢复满后再次突发，多次突发会跨服务端 drain 周期累积 cnt，仍可能超过 60。
//! 客户端无法得知服务端 drain 的精确相位，也无法读取当前 cnt，因此无法靠对齐 tick 解决。
//!
//! ## SlidingWindow（突发密度上限）
//! 不依赖 tick 对齐，约束「任意 window_duration 半开区间内发送条数 ≤ window_limit」。
//! 它封顶的是瞬时密度，不封顶长期速率：若长期速率超过服务端 drain 速率（40 条/2 秒），
//! cnt 仍会逐周期净增。两者分工：令牌桶把长期速率钉在 cmds_per_sec（应配为 20），
//! 滑动窗口把任意 window_duration 内的突发密度钉在 window_limit 以下。
//! 只覆盖 `send` 命令路径；`send_raw`（Lua SendPkt）绕过限速器直写，不计入窗口。
//!
//! 注意：window_limit = 60（雷劈阈值）**不等于**无条件安全。需要两个条件同时成立：
//! `2×cmds_per_sec ≤ 40`（长期速率不超过服务端每周期 drain 量），且
//! `burst_size + 2×cmds_per_sec ≤ 60`（令牌桶在任意 2 秒内的自身上界）。
//! 满足时组合限速器保证 cnt ≤ 60（详见 SlidingWindow 的「区间语义与局限」）。
//! window_limit 是第二道独立上界：调低它能在 burst_size/cmds_per_sec 配错时仍封顶
//! 突发密度，但它封顶密度而非长期速率，替代不了第一个条件。
//! 两条不等式均在配置解析时校验并告警。
//!
//! ## SafeLimiter（默认使用的组合限速器）
//! 两者串联，各自算出预计发送时刻并取较大值：令牌桶负责平滑速率与突发手感，
//! 滑动窗口负责兜底累积上限。
//! 长期速率 ≤ min(cmds_per_sec, window_limit/window_duration)。
//!
//! ## 调用方契约
//! `acquire()` 返回的 wait 必须被真实等待（sleep(wait) 后再发送）。两个子限速器
//! 都按「预计发送时刻 = acquire 时刻 + wait」记账，忽略 wait 会让令牌线性负债、
//! 窗口计数与实际错位，限速随之失效。

use std::collections::VecDeque;
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

    /// 尝试获取一个令牌，返回需要等待的时间。
    /// 调用方必须 sleep(wait) 后再发送，否则令牌记账会与实际发送时刻错位
    pub fn acquire(&mut self) -> Duration {
        let now = Instant::now();
        self.acquire_at(now).saturating_duration_since(now)
    }

    /// 以 `now` 为基准获取一个令牌，返回预计实际发送时刻。
    /// 拆出时钟参数便于组合限速器与测试共用同一时间基准。
    fn acquire_at(&mut self, now: Instant) -> Instant {
        // now 只前进不后退：拆出时钟参数后调用方（组合限速器、虚拟时钟测试）
        // 可能传入早于上次基准的时刻，若允许回退，同一段时间会被重复计入令牌补充
        if now > self.last_refill {
            let elapsed = now.duration_since(self.last_refill).as_secs_f64();
            self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.capacity);
            self.last_refill = now;
        }

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
                let since_last = now.saturating_duration_since(self.last_send);
                if since_last < self.min_interval {
                    self.min_interval - since_last
                } else {
                    Duration::ZERO
                }
            }
        } else {
            // 令牌不足，等待补充；min_interval 作为下限防止速率过高
            let needed = 1.0 - self.tokens;
            // 必须预扣这一枚令牌（余额可为负）：否则本次发送不记账，
            // 而补充出的令牌又会被下一次 acquire 消费，等于一个令牌放行两条命令。
            // 当 min_interval < 1000/refill_rate 时长期速率会突破 refill_rate，
            // 服务端 cnt 每个 drain 周期净增，最终必然雷劈
            self.tokens -= 1.0;
            let wait_secs = needed / self.refill_rate;
            let wait = Duration::from_secs_f64(wait_secs);
            wait.max(self.min_interval)
        };

        // last_send 记录预计的实际发送时间（acquire 时间 + wait），而非 acquire 时间
        // 这样下一次 acquire 才能正确计算距上次发送的间隔
        // 同样只前进不后退：回退会让下一次 min_interval 判断偏松
        let send_at = now + wait;
        if send_at > self.last_send {
            self.last_send = send_at;
        }
        send_at
    }

    /// 把内部记账的「预计发送时刻」推后到 `at`（只推后不提前）。
    ///
    /// `acquire_at` 返回的只是令牌桶自身的约束；若外层还叠加了其它等待，
    /// 实际发送时刻会晚于 last_send，导致下一次 min_interval 判断偏松。
    fn defer_last_send_to(&mut self, at: Instant) {
        if at > self.last_send {
            self.last_send = at;
        }
    }
}

/// 滑动窗口限速器：约束任意 `duration` 时间内的发送条数不超过 `limit`
///
/// 与令牌桶不同，滑动窗口不关心速率平滑，只做累积上限的硬兜底。
/// 队列中保存的是每条命令「预计实际发送时刻」（acquire 时刻 + 返回的 wait），
/// 因此调用方必须 sleep(wait) 后再发送，记账才与实际一致。
///
/// ## 区间语义与局限
/// 约束按半开区间 `[t, t + duration)` 成立。若 `limit` 条命令在同一时刻被放行，
/// 它们会在 `duration` 后同时到期，紧接着又可能有 `limit` 条排在同一时刻，
/// 因此**闭区间内可达 2×limit**。这是滑动窗口日志算法的固有性质，无法靠微调消除。
///
/// 服务端 `clear_cmd_count` 是某一瞬间的事件，落在边界上的命令算 drain 前还是
/// drain 后不可控，故本限速器**不构成对服务端 cnt 的无条件保证**：
/// - 它封顶突发密度，长期速率仍由令牌桶的 `cmds_per_sec` 决定
/// - 无条件安全要求 `2×cmds_per_sec ≤ 40`（服务端每周期 drain 量），
///   且 `burst_size + 2×cmds_per_sec ≤ 60`。该不等式由配置层告警强制提示
pub struct SlidingWindow {
    /// 已放行命令的预计发送时刻，单调不减
    timestamps: VecDeque<Instant>,
    /// 窗口内允许的最大命令数
    limit: usize,
    /// 窗口时长
    duration: Duration,
}

impl SlidingWindow {
    /// 预留容量上限：limit 可能来自配置的异常大值，VecDeque 预留溢出会 panic。
    /// 队列实际长度仍受 limit 约束，超出预留部分按需增长
    const MAX_PREALLOC: usize = 1024;

    /// 窗口时长上限：`acquire_at` 中的 `oldest + self.duration` 是 `Instant + Duration`，
    /// 溢出会 panic。本类型是 pub 的，不能只依赖 Session::new 的钳制
    /// （那里另有更严的 10 秒上限）。60 秒远超任何实际 drain 周期
    const MAX_WINDOW_MS: u64 = 60_000;

    pub fn new(limit: u64, duration_ms: u64) -> Self {
        let limit = limit.max(1) as usize;
        Self {
            timestamps: VecDeque::with_capacity(limit.min(Self::MAX_PREALLOC)),
            limit,
            duration: Duration::from_millis(duration_ms.clamp(1, Self::MAX_WINDOW_MS)),
        }
    }

    /// 获取一个发送配额，返回需要等待的时间
    pub fn acquire(&mut self) -> Duration {
        let now = Instant::now();
        self.acquire_at(now).saturating_duration_since(now)
    }

    /// 以 `now` 为基准获取一个发送配额，返回预计实际发送时刻
    fn acquire_at(&mut self, now: Instant) -> Instant {
        self.prune(now);

        // 窗口已满：最老一条滑出窗口后才能发送
        let mut send_at = now;
        if self.timestamps.len() >= self.limit {
            if let Some(oldest) = self.timestamps.pop_front() {
                let expire_at = oldest + self.duration;
                if expire_at > send_at {
                    send_at = expire_at;
                }
            }
        }
        // 过期裁剪依赖队列有序，钳制到不早于上一条的预计发送时刻
        if let Some(&last) = self.timestamps.back() {
            if last > send_at {
                send_at = last;
            }
        }
        self.timestamps.push_back(send_at);
        send_at
    }

    /// 把最新一条的预计发送时刻推后到 `at`（只推后不提前），用于组合限速器同步最终发送时刻
    fn defer_last_to(&mut self, at: Instant) {
        if let Some(last) = self.timestamps.back_mut() {
            if at > *last {
                *last = at;
            }
        }
    }

    /// 丢弃已滑出窗口的时间戳。`timestamps` 单调不减，从队首裁剪即可
    fn prune(&mut self, now: Instant) {
        while let Some(&front) = self.timestamps.front() {
            // front 可能是尚未到来的预计发送时刻，饱和减法得 0，不会误删
            if now.saturating_duration_since(front) >= self.duration {
                self.timestamps.pop_front();
            } else {
                break;
            }
        }
    }
}

/// 组合限速器：令牌桶（速率平滑）+ 滑动窗口（累积硬限制）
///
/// 两侧各自算出预计发送时刻，取较大值作为最终发送时刻，并回写给双方记账。
pub struct SafeLimiter {
    bucket: TokenBucket,
    window: SlidingWindow,
}

impl SafeLimiter {
    pub fn new(
        burst_size: u64,
        cmds_per_sec: u64,
        cmd_interval_ms: u64,
        window_limit: u64,
        window_duration_ms: u64,
    ) -> Self {
        Self {
            bucket: TokenBucket::new(burst_size, cmds_per_sec, cmd_interval_ms),
            window: SlidingWindow::new(window_limit, window_duration_ms),
        }
    }

    /// 获取一个发送配额，返回需要等待的时间。
    /// 调用方必须 sleep(wait) 后再发送（见模块级「调用方契约」）
    pub fn acquire(&mut self) -> Duration {
        let now = Instant::now();
        self.acquire_at(now).saturating_duration_since(now)
    }

    /// 以 `now` 为基准获取一个发送配额，返回预计实际发送时刻
    fn acquire_at(&mut self, now: Instant) -> Instant {
        let bucket_at = self.bucket.acquire_at(now);
        let window_at = self.window.acquire_at(now);
        let send_at = bucket_at.max(window_at);
        // 双方各自按自己的约束记账，统一到实际发送时刻，避免任一侧约束被放松
        self.bucket.defer_last_send_to(send_at);
        self.window.defer_last_to(send_at);
        send_at
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

    #[test]
    fn test_sliding_window_basic() {
        // limit=3、窗口 2 秒：前 3 条直接放行，第 4 条等最老一条滑出窗口
        let mut window = SlidingWindow::new(3, 2000);
        for i in 0..3 {
            assert_eq!(
                window.acquire(),
                Duration::ZERO,
                "cmd {} should pass",
                i + 1
            );
        }
        let wait = window.acquire();
        assert!(
            wait > Duration::from_millis(1900) && wait <= Duration::from_millis(2000),
            "cmd 4 should wait ~2s, got {:?}",
            wait
        );
    }

    #[test]
    fn test_sliding_window_expiry() {
        // 窗口过期后自动清理，重新恢复放行能力
        let mut window = SlidingWindow::new(2, 100);
        assert_eq!(window.acquire(), Duration::ZERO);
        assert_eq!(window.acquire(), Duration::ZERO);
        let wait = window.acquire();
        assert!(
            wait >= Duration::from_millis(90) && wait <= Duration::from_millis(100),
            "cmd 3 should wait ~100ms, got {:?}",
            wait
        );
        // 等三条记账（含第 3 条的预计发送时刻）全部滑出窗口
        std::thread::sleep(Duration::from_millis(250));
        assert_eq!(window.acquire(), Duration::ZERO);
        assert_eq!(window.acquire(), Duration::ZERO);
    }

    #[test]
    fn test_sliding_window_limit() {
        // 服务端雷劈阈值配置：60 条 / 2 秒
        let mut window = SlidingWindow::new(60, 2000);
        for i in 0..60 {
            assert_eq!(
                window.acquire(),
                Duration::ZERO,
                "cmd {} should pass",
                i + 1
            );
        }
        // 第 61 条被强制等待，不会突破窗口上限
        let wait = window.acquire();
        assert!(
            wait > Duration::from_millis(1900),
            "cmd 61 should be throttled, got {:?}",
            wait
        );
    }

    #[test]
    fn test_sliding_window_limit_zero_falls_back_to_one() {
        // limit=0 会被钳制到 1，避免除零语义与死锁
        let mut window = SlidingWindow::new(0, 100);
        assert_eq!(window.acquire(), Duration::ZERO);
        assert!(window.acquire() > Duration::ZERO);
    }

    #[test]
    fn test_sliding_window_huge_duration_is_clamped() {
        // SlidingWindow 是 pub 的，不能只靠 Session::new 的 clamp(2000, 10_000) 兜底。
        // 未钳制时 acquire_at 里的 `oldest + self.duration` 在 Windows（u64 的
        // 100ns 计时）上会溢出 panic；Linux 上不 panic，但会算出天文数字的等待，
        // 写入任务实质死锁。这里断言等待被钳制到上限内，两种平台都能拦住
        let mut window = SlidingWindow::new(2, u64::MAX);
        assert_eq!(window.acquire(), Duration::ZERO);
        assert_eq!(window.acquire(), Duration::ZERO);
        // 第三条需要计算到期时刻，时长未钳制时在此 panic 或返回巨大等待
        let wait = window.acquire();
        assert!(
            wait > Duration::ZERO && wait <= Duration::from_secs(60),
            "窗口时长未钳制，等待 {:?} 超出 60 秒上限",
            wait
        );
    }

    #[test]
    fn test_token_bucket_non_monotonic_now_does_not_over_refill() {
        // acquire_at 拆出时钟参数后，调用方（组合限速器、虚拟时钟）可能传入
        // 早于上次基准的时刻。若允许 last_refill 回退，同一段时间会被重复
        // 计入令牌补充，等于凭空放行一批免费命令
        let t0 = Instant::now();
        let later = t0 + Duration::from_millis(1000);
        let mut bucket = TokenBucket::new(2, 20, 50);
        // 在 later 上把令牌耗尽并转入负债
        bucket.acquire_at(later);
        bucket.acquire_at(later);
        bucket.acquire_at(later);
        // 插入一个明显更早的时刻
        bucket.acquire_at(t0);
        // 回到 later：基准若被回退，这 1 秒会被再算一次，
        // 桶会被重新填满并重置突发，本条将获得 wait=0 的免费放行
        let after = bucket.acquire_at(later);
        assert!(
            after > later,
            "非单调时钟导致同一段时间被重复补充令牌，命令被免费放行"
        );
    }

    #[test]
    fn test_safe_limiter_combined() {
        // 窗口成为约束方：令牌桶仍有突发余量，但被滑动窗口拦住
        let mut limiter = SafeLimiter::new(10, 20, 50, 3, 1000);
        for i in 0..3 {
            assert_eq!(limiter.acquire(), Duration::ZERO, "burst cmd {}", i + 1);
        }
        let wait = limiter.acquire();
        assert!(
            wait >= Duration::from_millis(900),
            "window should dominate, got {:?}",
            wait
        );

        // 令牌桶成为约束方：窗口宽松时等待时间由 min_interval 决定
        let mut limiter = SafeLimiter::new(2, 20, 50, 60, 2000);
        assert_eq!(limiter.acquire(), Duration::ZERO);
        assert_eq!(limiter.acquire(), Duration::ZERO);
        let wait = limiter.acquire();
        assert!(
            wait >= Duration::from_millis(49) && wait < Duration::from_millis(200),
            "bucket should dominate, got {:?}",
            wait
        );
        // 记账对齐：连续请求仍逐条等 min_interval，不会因两侧叠加而放松
        let wait = limiter.acquire();
        assert!(
            wait >= Duration::from_millis(49),
            "steady interval should hold, got {:?}",
            wait
        );
    }

    #[test]
    fn test_multiple_burst_accumulation() {
        // 复现 GPS 连续重启场景：三批次走路指令（22/22/24 条），
        // 批次之间角色处于 busy 等待，令牌桶恢复后重新突发。
        // 此处 cmd_interval_ms=20 短于令牌补充周期 1000/20=50ms，且突发 25 条，
        // 属于「单次突发安全公式已被突破」的配置（25 + (1000/20)*2 = 125 > 60），
        // 验证滑动窗口能把任意 2 秒内的发送条数兜底到 60 以下。
        let sends = drive_batches(&mut SafeLimiter::new(25, 20, 20, 60, 2000));
        assert_eq!(sends.len(), 68);
        assert_within_window(&sends, 60, Duration::from_millis(2000));
    }

    #[test]
    fn test_token_bucket_alone_breaks_window_limit() {
        // 对照实验：同一场景下去掉滑动窗口会突破 60 条/2 秒，
        // 即服务端 cnt 超过 3*CMDS_PER_TICK 的密度条件
        let sends = drive_batches(&mut TokenBucket::new(25, 20, 20));
        let peak = peak_within_window(&sends, Duration::from_millis(2000));
        assert!(
            peak > 60,
            "预期令牌桶单独使用时会突破阈值，实际峰值 {}",
            peak
        );
    }

    #[test]
    fn test_long_term_rate_capped_by_refill_rate() {
        // cmd_interval_ms=40 短于令牌补充周期 1000/20=50ms 时，
        // 长期速率仍不得超过 cmds_per_sec：否则服务端 cnt 每个 drain 周期净增，
        // 无论滑动窗口多严都必然雷劈
        let mut bucket = TokenBucket::new(15, 20, 40);
        let sends = drive_continuous(&mut bucket, 100);
        let total = *sends.last().unwrap();
        // 首 15 条为突发，余下 85 条受 20 令牌/秒限制，至少需要 85/20 = 4.25 秒
        assert!(
            total >= Duration::from_millis(4200),
            "100 条仅耗时 {:?}，长期速率突破 cmds_per_sec",
            total
        );
        // 也不能过度限流（预扣不应额外惩罚）
        assert!(
            total <= Duration::from_millis(4400),
            "100 条耗时 {:?}，速率低于预期",
            total
        );
    }

    #[test]
    fn test_server_cnt_never_exceeds_threshold() {
        // 端到端验证：用服务端 cmd.c 的 cnt 模型回放 GPS 三批次重启，
        // 使用 profiles/example.toml 的默认限速参数。
        // 客户端无法与服务端 tick 对齐，因此遍历 4 种 drain 相位。
        let sends = drive_batches(&mut SafeLimiter::new(15, 20, 50, 60, 2000));
        for phase_ms in [0, 500, 1000, 1500] {
            let peak = server_cnt_peak(&sends, Duration::from_millis(phase_ms));
            assert!(
                peak <= 60,
                "drain 相位 {}ms 时服务端 cnt 峰值 {}，会触发雷劈",
                phase_ms,
                peak
            );
        }
    }

    #[test]
    fn test_server_cnt_safe_when_interval_below_refill_period() {
        // cmd_interval_ms=40 < 1000/cmds_per_sec=50ms 是合法配置（clamp 范围 20~200），
        // 也是令牌预扣缺失时速率会翻倍到 ~29/s 的场景：服务端 cnt 逐周期净增，
        // 长时挂机必然雷劈。持续满载 400 条（约 20 秒）验证 cnt 始终安全。
        let sends = drive_continuous(&mut SafeLimiter::new(15, 20, 40, 60, 2000), 400);
        for phase_ms in [0, 500, 1000, 1500] {
            let peak = server_cnt_peak(&sends, Duration::from_millis(phase_ms));
            assert!(
                peak <= 60,
                "drain 相位 {}ms 时服务端 cnt 峰值 {}，长时挂机会雷劈",
                phase_ms,
                peak
            );
        }
    }

    #[test]
    fn test_server_cnt_safe_over_fine_drain_phase_sweep() {
        // 直接验证模块级安全断言：当 2×cmds_per_sec ≤ 40 且
        // burst_size + 2×cmds_per_sec ≤ 60 时，cnt 恒 ≤ 60。
        // 生产配置 15/20/50 + 60/2000 满足两条不等式（2×20=40、15+40=55 ≤ 60）。
        // 持续满载 600 条（约 30 秒、15 个 drain 周期），并按 100ms 粒度
        // 遍历 20 种 drain 相位——客户端无法得知服务端 tick 相位，
        // 4 点采样不足以证明「任意相位」安全
        let sends = drive_continuous(&mut SafeLimiter::new(15, 20, 50, 60, 2000), 600);
        for phase in (0..2000).step_by(100) {
            let peak = server_cnt_peak(&sends, Duration::from_millis(phase));
            assert!(
                peak <= 60,
                "drain 相位 {}ms 时服务端 cnt 峰值 {}，安全断言不成立",
                phase,
                peak
            );
        }
    }

    #[test]
    fn test_server_cnt_unsafe_when_precondition_violated() {
        // 反向验证安全断言的前提确实必要：cmds_per_sec=30 使
        // 2×cmds_per_sec=60 > 服务端每周期 drain 量 40，长期速率超过 drain 速率，
        // cnt 逐周期净增。滑动窗口 60/2000 无法阻止——它封顶密度而非长期速率
        let sends = drive_continuous(&mut SafeLimiter::new(15, 30, 30, 60, 2000), 600);
        let peak = (0..2000)
            .step_by(100)
            .map(|phase| server_cnt_peak(&sends, Duration::from_millis(phase)))
            .max()
            .unwrap();
        assert!(
            peak > 60,
            "预期违反前提时 cnt 会突破阈值，实际峰值 {}",
            peak
        );
    }

    /// GPS 三批次重启：(批次条数, 批次起始相对 t0 的毫秒偏移)
    const GPS_BATCHES: [(usize, u64); 3] = [(22, 0), (22, 800), (24, 1600)];

    /// 测试用统一接口：令牌桶与组合限速器都能被虚拟时钟驱动
    trait Limiter {
        fn acquire_at(&mut self, now: Instant) -> Instant;
    }

    impl Limiter for TokenBucket {
        fn acquire_at(&mut self, now: Instant) -> Instant {
            TokenBucket::acquire_at(self, now)
        }
    }

    impl Limiter for SafeLimiter {
        fn acquire_at(&mut self, now: Instant) -> Instant {
            SafeLimiter::acquire_at(self, now)
        }
    }

    /// 持续满载请求 count 条命令，返回每条相对 t0 的发送时刻
    fn drive_continuous(limiter: &mut impl Limiter, count: usize) -> Vec<Duration> {
        let t0 = Instant::now();
        let mut sends = Vec::with_capacity(count);
        let mut now = t0;
        for _ in 0..count {
            let send_at = limiter.acquire_at(now);
            sends.push(send_at.duration_since(t0));
            now = send_at;
        }
        sends
    }

    /// 按 GPS_BATCHES 驱动限速器，返回每条命令相对 t0 的发送时刻。
    ///
    /// 使用虚拟时钟（now = 上一条的发送时刻）精确复现调用方
    /// `acquire() -> sleep(wait) -> send()` 的真实时序，测试无需实际等待。
    fn drive_batches(limiter: &mut impl Limiter) -> Vec<Duration> {
        let t0 = Instant::now();
        let mut sends = Vec::new();
        let mut now = t0;
        for (count, start_ms) in GPS_BATCHES {
            // 批次起始不早于上一批次最后一条的发送时刻
            now = now.max(t0 + Duration::from_millis(start_ms));
            for _ in 0..count {
                let send_at = limiter.acquire_at(now);
                sends.push(send_at.duration_since(t0));
                now = send_at;
            }
        }
        sends
    }

    /// 按 LPC cmd.c 的计数模型回放发送序列，返回服务端 cnt 峰值。
    ///
    /// cnt 每条命令 +1，每 2 秒 drain 40（不低于 0）。`drain_phase` 指定首个 drain
    /// 时刻，用于覆盖客户端无法与服务端 tick 对齐的各种相位。
    fn server_cnt_peak(sends: &[Duration], drain_phase: Duration) -> u64 {
        const CMDS_PER_TICK: u64 = 20;
        let tick = Duration::from_secs(2);
        let mut cnt = 0u64;
        let mut peak = 0u64;
        let mut next_drain = drain_phase;
        for &t in sends {
            while t >= next_drain {
                cnt = cnt.saturating_sub(2 * CMDS_PER_TICK);
                next_drain += tick;
            }
            cnt += 1;
            peak = peak.max(cnt);
        }
        peak
    }

    /// 任意 `window` 时长内的最大发送条数
    fn peak_within_window(sends: &[Duration], window: Duration) -> usize {
        sends
            .iter()
            .map(|start| {
                sends
                    .iter()
                    .filter(|t| **t >= *start && **t < *start + window)
                    .count()
            })
            .max()
            .unwrap_or(0)
    }

    fn assert_within_window(sends: &[Duration], limit: usize, window: Duration) {
        for (i, &start) in sends.iter().enumerate() {
            let count = sends
                .iter()
                .filter(|t| **t >= start && **t < start + window)
                .count();
            assert!(
                count <= limit,
                "cmd {} 起 {:?} 内发送 {} 条，超过服务端雷劈阈值 {}",
                i + 1,
                window,
                count,
                limit
            );
        }
    }
}
