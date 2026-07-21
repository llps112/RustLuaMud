//! 服务器响应追踪器
//!
//! 通过追踪"最后收到服务器输出的时间"来判断服务器是否响应正常，
//! 用于解决服务器间歇性无响应时指令堆积导致的 LPC 雷劈问题。
//!
//! # 设计原理
//!
//! MUD 服务器 LPC 限速机制（`/feature/alias.c`）：
//! - 每 2 秒（1 tick）清除 40 条指令计数（`2 * CMDS_PER_TICK`）
//! - `cnt > 20`：扣血
//! - `cnt > 60`（`3 * CMDS_PER_TICK`）：雷劈昏迷 + 强制踢出
//!
//! 客户端限速 20 条/秒，若服务器卡顿 N 秒，堆积指令 = 20*N：
//! - N=2.5s → 50 条（扣血，但不雷劈，留 10 条余量）
//! - N=3.0s → 60 条（雷劈线！）
//!
//! 因此 `pause_timeout` 设为 2.5s：服务器无输出超过此值时暂停发送，
//! 确保堆积指令不超过 50 条，避免雷劈。

use std::time::{Duration, Instant};

/// 服务器响应追踪器配置
///
/// 所有时间阈值均可通过 `set_config` 动态调整。
#[derive(Debug, Clone)]
pub struct ServerWatchConfig {
    /// 服务器无输出超过此时间，判定为"可能无响应"，输出警告日志
    ///
    /// 设为 2.0s：提前 0.5s 预警，给 `pause_timeout` 留缓冲。
    pub warn_timeout: Duration,

    /// 服务器无输出超过此时间，暂停发送指令
    ///
    /// 设为 2.5s：20条/秒 × 2.5s = 50条 < 60条（雷劈线）。
    pub pause_timeout: Duration,

    /// 暂停后，重新检查服务器就绪状态的间隔
    ///
    /// 设为 0.5s：在暂停期间每隔 0.5s 重新检查一次，平衡响应速度和 CPU 开销。
    pub check_interval: Duration,

    /// 最大等待时间：超过此时间清空队列，防止永久卡死
    ///
    /// 设为 30.0s：超过此值认为服务器已断连或严重故障，放弃堆积的指令。
    pub max_wait_time: Duration,

    /// DEBUG 日志输出间隔（秒），防止刷屏
    pub debug_interval: Duration,
}

impl Default for ServerWatchConfig {
    fn default() -> Self {
        Self {
            warn_timeout: Duration::from_millis(2000),
            pause_timeout: Duration::from_millis(2500),
            check_interval: Duration::from_millis(500),
            max_wait_time: Duration::from_secs(30),
            debug_interval: Duration::from_secs(5),
        }
    }
}

/// 服务器响应追踪器统计信息
#[derive(Debug, Clone, Default)]
pub struct ServerWatchStats {
    /// 总暂停次数
    pub total_paused: u64,
    /// 总放弃次数（超时清空队列）
    pub total_aborted: u64,
}

/// 服务器响应追踪器
///
/// # 线程安全
///
/// 该结构体本身不是线程安全的，调用方需通过 `Rc<RefCell<>>` 或 `Mutex` 保护。
/// 在 `LuaEngine` 中通过 `Rc<RefCell<ServerWatch>>` 共享给 Lua API 闭包。
#[derive(Debug)]
pub struct ServerWatch {
    /// 最后收到服务器输出的时间
    last_output: Instant,

    /// 配置参数
    config: ServerWatchConfig,

    /// 统计信息
    stats: ServerWatchStats,

    /// 上次警告日志的时间（用于节流，None 表示尚未输出过警告）
    last_warn_time: Option<Instant>,

    /// 上次 DEBUG 日志的时间（用于节流，None 表示尚未输出过 DEBUG）
    last_debug_time: Option<Instant>,
}

impl Default for ServerWatch {
    fn default() -> Self {
        Self::new()
    }
}

impl ServerWatch {
    /// 创建新的服务器响应追踪器
    ///
    /// 初始化 `last_output` 为当前时间，表示"刚收到输出"，
    /// 避免引擎启动后立即触发暂停。
    pub fn new() -> Self {
        Self {
            last_output: Instant::now(),
            config: ServerWatchConfig::default(),
            stats: ServerWatchStats::default(),
            last_warn_time: None,
            last_debug_time: None,
        }
    }

    /// 标记"收到服务器输出"
    ///
    /// 应在 Rust 端 `process_output_inner` 中每行服务器输出时调用，
    /// 更新 `last_output` 时间戳并重置警告节流状态。
    pub fn on_output(&mut self) {
        self.last_output = Instant::now();
        self.last_warn_time = None;
        self.last_debug_time = None;
    }

    /// 获取距离上次服务器输出的时间
    pub fn get_elapsed(&self) -> Duration {
        self.last_output.elapsed()
    }

    /// 检查服务器是否就绪（可以发送指令）
    ///
    /// # 返回值
    ///
    /// - `true`：就绪，可以发送指令
    /// - `false`：未就绪，应暂停发送
    ///
    /// # 日志输出
    ///
    /// - `elapsed > warn_timeout`：每 `debug_interval` 输出一次警告日志
    /// - `elapsed > pause_timeout`：每 `debug_interval` 输出一次 DEBUG 日志
    ///
    /// # 注意
    ///
    /// 此方法会更新内部的日志节流时间戳，因此不是纯查询方法。
    /// 日志输出通过返回的 `ServerWatchLog` 结构体交给调用方处理，
    /// 避免在 Rust 端直接依赖 Lua 的 `Note` 函数。
    pub fn is_ready(&mut self) -> ServerWatchReadiness {
        let elapsed = self.get_elapsed();

        // 警告级别：超过 warn_timeout 但未超过 pause_timeout
        if elapsed > self.config.warn_timeout && elapsed <= self.config.pause_timeout {
            let should_log = self
                .last_warn_time
                .map(|t| t.elapsed() >= self.config.debug_interval)
                .unwrap_or(true);
            if should_log {
                self.last_warn_time = Some(Instant::now());
                return ServerWatchReadiness {
                    is_ready: true,
                    elapsed,
                    log: Some(ServerWatchLog::Warn(elapsed)),
                };
            }
        }

        // 暂停级别：超过 pause_timeout
        if elapsed > self.config.pause_timeout {
            let should_log = self
                .last_debug_time
                .map(|t| t.elapsed() >= self.config.debug_interval)
                .unwrap_or(true);
            if should_log {
                self.last_debug_time = Some(Instant::now());
            }
            return ServerWatchReadiness {
                is_ready: false,
                elapsed,
                log: if should_log {
                    Some(ServerWatchLog::Debug(elapsed))
                } else {
                    None
                },
            };
        }

        ServerWatchReadiness {
            is_ready: true,
            elapsed,
            log: None,
        }
    }

    /// 记录一次暂停事件（由调用方在暂停发送时调用）
    pub fn record_pause(&mut self) {
        self.stats.total_paused += 1;
    }

    /// 记录一次放弃事件（由调用方在清空队列时调用）
    pub fn record_abort(&mut self) {
        self.stats.total_aborted += 1;
    }

    /// 重置状态（用于断连重连后）
    ///
    /// 重置时间戳、警告节流和统计信息，但保留配置。
    pub fn reset(&mut self) {
        self.last_output = Instant::now();
        self.last_warn_time = None;
        self.last_debug_time = None;
        self.stats = ServerWatchStats::default();
    }

    /// 获取当前配置（只读）
    pub fn config(&self) -> &ServerWatchConfig {
        &self.config
    }

    /// 更新配置
    ///
    /// # 参数
    ///
    /// 传入 `None` 的字段表示保持原值不变。
    pub fn set_config(
        &mut self,
        warn_timeout: Option<Duration>,
        pause_timeout: Option<Duration>,
        check_interval: Option<Duration>,
        max_wait_time: Option<Duration>,
        debug_interval: Option<Duration>,
    ) {
        if let Some(v) = warn_timeout {
            self.config.warn_timeout = v;
        }
        if let Some(v) = pause_timeout {
            self.config.pause_timeout = v;
        }
        if let Some(v) = check_interval {
            self.config.check_interval = v;
        }
        if let Some(v) = max_wait_time {
            self.config.max_wait_time = v;
        }
        if let Some(v) = debug_interval {
            self.config.debug_interval = v;
        }
    }

    /// 获取统计信息（只读）
    pub fn stats(&self) -> &ServerWatchStats {
        &self.stats
    }
}

/// `is_ready` 的返回值
#[derive(Debug, Clone)]
pub struct ServerWatchReadiness {
    /// 是否就绪
    pub is_ready: bool,
    /// 距离上次输出的时间
    #[allow(dead_code)]
    pub elapsed: Duration,
    /// 待输出的日志（如果有）
    pub log: Option<ServerWatchLog>,
}

/// 服务器响应追踪器日志事件
#[derive(Debug, Clone)]
pub enum ServerWatchLog {
    /// 警告：服务器无输出超过 `warn_timeout` 但未超过 `pause_timeout`
    Warn(Duration),
    /// DEBUG：服务器无输出超过 `pause_timeout`，判定为未就绪
    Debug(Duration),
}

impl ServerWatchLog {
    /// 格式化为日志字符串
    pub fn format(&self) -> String {
        match self {
            ServerWatchLog::Warn(elapsed) => {
                format!(
                    "[DEBUG server_watch] 警告：服务器 {:.1}s 无输出",
                    elapsed.as_secs_f64()
                )
            }
            ServerWatchLog::Debug(elapsed) => {
                format!(
                    "[DEBUG server_watch] 服务器 {:.1}s 无输出，判定为未就绪",
                    elapsed.as_secs_f64()
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试默认配置值是否符合 LPC 限速机制的要求
    #[test]
    fn test_default_config_matches_lpc_limits() {
        let config = ServerWatchConfig::default();

        // warn_timeout = 2.0s（提前 0.5s 预警）
        assert_eq!(config.warn_timeout, Duration::from_millis(2000));

        // pause_timeout = 2.5s（20条/秒 × 2.5s = 50条 < 60条雷劈线）
        assert_eq!(config.pause_timeout, Duration::from_millis(2500));

        // check_interval = 0.5s
        assert_eq!(config.check_interval, Duration::from_millis(500));

        // max_wait_time = 30s
        assert_eq!(config.max_wait_time, Duration::from_secs(30));

        // debug_interval = 5s
        assert_eq!(config.debug_interval, Duration::from_secs(5));
    }

    /// 测试新建的追踪器初始状态应为就绪
    #[test]
    fn test_new_server_watch_is_ready() {
        let mut sw = ServerWatch::new();
        let readiness = sw.is_ready();
        assert!(readiness.is_ready);
        assert!(readiness.log.is_none());
    }

    /// 测试 `on_output` 重置时间戳
    #[test]
    fn test_on_output_resets_timestamp() {
        let mut sw = ServerWatch::new();
        // 模拟时间流逝（通过直接设置 last_output 为过去的时间）
        sw.last_output = Instant::now() - Duration::from_secs(5);
        assert!(!sw.is_ready().is_ready);

        // 收到输出后应重置为就绪
        sw.on_output();
        assert!(sw.is_ready().is_ready);
    }

    /// 测试 `on_output` 重置警告节流
    #[test]
    fn test_on_output_resets_warn_throttle() {
        let mut sw = ServerWatch::new();
        sw.last_output = Instant::now() - Duration::from_millis(2100);
        // 第一次调用应输出警告
        let r1 = sw.is_ready();
        assert!(r1.log.is_some());
        // 第二次调用不应输出警告（节流）
        let r2 = sw.is_ready();
        assert!(r2.log.is_none());

        // 收到输出后节流应重置
        sw.on_output();
        sw.last_output = Instant::now() - Duration::from_millis(2100);
        let r3 = sw.is_ready();
        assert!(r3.log.is_some());
    }

    /// 测试警告级别：超过 warn_timeout 但未超过 pause_timeout
    #[test]
    fn test_warn_level_triggered() {
        let mut sw = ServerWatch::new();
        sw.last_output = Instant::now() - Duration::from_millis(2100);

        let readiness = sw.is_ready();
        assert!(readiness.is_ready); // 仍然就绪
        assert!(readiness.log.is_some());
        match readiness.log {
            Some(ServerWatchLog::Warn(d)) => {
                assert!(d.as_secs_f64() >= 2.0);
            }
            other => panic!("期望 Warn 日志，实际：{:?}", other),
        }
    }

    /// 测试暂停级别：超过 pause_timeout
    #[test]
    fn test_pause_level_triggered() {
        let mut sw = ServerWatch::new();
        sw.last_output = Instant::now() - Duration::from_millis(2600);

        let readiness = sw.is_ready();
        assert!(!readiness.is_ready); // 未就绪
    }

    /// 测试 DEBUG 日志节流：首次输出，后续不输出
    #[test]
    fn test_debug_log_throttle() {
        let mut sw = ServerWatch::new();
        sw.last_output = Instant::now() - Duration::from_millis(2600);

        // 首次调用应输出 DEBUG 日志
        let r1 = sw.is_ready();
        assert!(!r1.is_ready);
        assert!(r1.log.is_some());

        // 立即第二次调用不应输出（节流）
        let r2 = sw.is_ready();
        assert!(!r2.is_ready);
        assert!(r2.log.is_none());
    }

    /// 测试 DEBUG 日志节流：超过 debug_interval 后应再次输出
    #[test]
    fn test_debug_log_throttle_expiry() {
        let mut sw = ServerWatch::new();
        sw.last_output = Instant::now() - Duration::from_millis(2600);

        // 首次输出
        let r1 = sw.is_ready();
        assert!(r1.log.is_some());

        // 模拟节流时间已过
        sw.last_debug_time = Some(Instant::now() - Duration::from_secs(6));
        let r2 = sw.is_ready();
        assert!(r2.log.is_some());
    }

    /// 测试统计信息：暂停计数
    #[test]
    fn test_record_pause() {
        let mut sw = ServerWatch::new();
        assert_eq!(sw.stats().total_paused, 0);
        sw.record_pause();
        sw.record_pause();
        assert_eq!(sw.stats().total_paused, 2);
    }

    /// 测试统计信息：放弃计数
    #[test]
    fn test_record_abort() {
        let mut sw = ServerWatch::new();
        assert_eq!(sw.stats().total_aborted, 0);
        sw.record_abort();
        assert_eq!(sw.stats().total_aborted, 1);
    }

    /// 测试重置：清空统计和时间戳
    #[test]
    fn test_reset_clears_state() {
        let mut sw = ServerWatch::new();
        sw.record_pause();
        sw.record_abort();
        sw.last_output = Instant::now() - Duration::from_secs(10);

        sw.reset();

        assert_eq!(sw.stats().total_paused, 0);
        assert_eq!(sw.stats().total_aborted, 0);
        assert!(sw.is_ready().is_ready);
    }

    /// 测试配置更新：部分更新
    #[test]
    fn test_set_config_partial_update() {
        let mut sw = ServerWatch::new();
        let original_pause = sw.config().pause_timeout;
        let original_check = sw.config().check_interval;

        sw.set_config(Some(Duration::from_millis(3000)), None, None, None, None);

        assert_eq!(sw.config().warn_timeout, Duration::from_millis(3000));
        assert_eq!(sw.config().pause_timeout, original_pause);
        assert_eq!(sw.config().check_interval, original_check);
    }

    /// 测试配置更新：全量更新
    #[test]
    fn test_set_config_full_update() {
        let mut sw = ServerWatch::new();
        sw.set_config(
            Some(Duration::from_millis(1500)),
            Some(Duration::from_millis(2000)),
            Some(Duration::from_millis(300)),
            Some(Duration::from_secs(60)),
            Some(Duration::from_secs(10)),
        );

        assert_eq!(sw.config().warn_timeout, Duration::from_millis(1500));
        assert_eq!(sw.config().pause_timeout, Duration::from_millis(2000));
        assert_eq!(sw.config().check_interval, Duration::from_millis(300));
        assert_eq!(sw.config().max_wait_time, Duration::from_secs(60));
        assert_eq!(sw.config().debug_interval, Duration::from_secs(10));
    }

    /// 测试日志格式化
    #[test]
    fn test_log_format() {
        let warn = ServerWatchLog::Warn(Duration::from_millis(2500));
        assert!(warn.format().contains("警告"));
        assert!(warn.format().contains("2.5"));

        let debug = ServerWatchLog::Debug(Duration::from_millis(3000));
        assert!(debug.format().contains("未就绪"));
        assert!(debug.format().contains("3.0"));
    }

    /// 测试边界条件：elapsed 恰好等于 warn_timeout
    ///
    /// 由于 `Instant::elapsed()` 的精度问题，此处使用 `>` 比较，
    /// 恰好等于阈值时不应触发警告。
    #[test]
    fn test_boundary_exact_timeout() {
        let mut sw = ServerWatch::new();
        // 设置为恰好 warn_timeout 前（理论上 elapsed == warn_timeout）
        // 由于 Duration 比较，elapsed 略大于 warn_timeout，会触发警告
        sw.last_output = Instant::now() - Duration::from_millis(2000);
        // 等待一小段时间确保 elapsed > warn_timeout
        std::thread::sleep(Duration::from_millis(10));
        let r = sw.is_ready();
        assert!(r.log.is_some());
    }

    /// 测试 `get_elapsed` 返回的时长随时间增长
    #[test]
    fn test_get_elapsed_increases() {
        let mut sw = ServerWatch::new();
        let e1 = sw.get_elapsed();
        std::thread::sleep(Duration::from_millis(50));
        let e2 = sw.get_elapsed();
        assert!(e2 > e1);
    }

    /// 测试在暂停状态下 `on_output` 能立即恢复就绪
    #[test]
    fn test_recovery_from_pause() {
        let mut sw = ServerWatch::new();
        sw.last_output = Instant::now() - Duration::from_secs(5);
        assert!(!sw.is_ready().is_ready);

        // 收到输出，立即恢复就绪
        sw.on_output();
        assert!(sw.is_ready().is_ready);
    }

    /// 测试配置更新后立即生效
    #[test]
    fn test_config_update_takes_effect_immediately() {
        let mut sw = ServerWatch::new();
        // 默认 pause_timeout = 2.5s，设置 last_output 为 1s 前，应该就绪
        sw.last_output = Instant::now() - Duration::from_millis(1000);
        assert!(sw.is_ready().is_ready);

        // 将 pause_timeout 调整为 0.5s，1s 前的输出应该触发暂停
        sw.set_config(None, Some(Duration::from_millis(500)), None, None, None);
        assert!(!sw.is_ready().is_ready);
    }

    /// 测试极端情况：last_output 在未来（时钟回退）
    ///
    /// 这种情况不应发生，但 `Instant` 保证单调递增，
    /// `elapsed()` 会返回 0，不会 panic。
    #[test]
    fn test_future_timestamp_does_not_panic() {
        let mut sw = ServerWatch::new();
        // 设置 last_output 为未来时间（理论上不可能，但测试健壮性）
        // Instant::now() + Duration 不会 panic，elapsed() 会返回 0
        sw.last_output = Instant::now() + Duration::from_secs(10);
        let r = sw.is_ready();
        // elapsed = 0，小于任何 timeout，应就绪
        assert!(r.is_ready);
    }
}
