//! 定时器调度与执行
//!
//! 处理定时器触发，包含：
//! - `fire_keepalive_if_idle`: 空闲心跳（IAC NOP）
//! - `fire_timer_by_name`: 按名称触发定时器（含外层 panic 防御）
//! - `fire_timer_inner`: 实际触发逻辑（回调调用、send_text 执行、one_shot 删除）
//! - `fire_due_timers`: 批量收集并触发所有到期定时器（MushClient 兼容）
//!
//! 时间模型：绝对时间 `next_fire`（MushClient 的 tFireTime 模型），
//! 触发后 `next_fire += interval`，无累积漂移。

use std::panic::AssertUnwindSafe;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::types::LuaEngine;

impl LuaEngine {
    /// 检查服务器是否长时间无响应，是则发送 IAC NOP 心跳包保持连接
    /// 空闲超过 30 秒后，每 30 秒发送一次 IAC NOP（节流）
    ///
    /// 若不节流，空闲期间每次轮询（50ms）都会压入心跳包；
    /// 服务器超时（TCP 半死）导致写任务阻塞不消费队列时，
    /// 原始数据通道（容量 256）约 13 秒即被填满，之后每次
    /// 发送都失败并以轮询频率向终端刷错误信息。
    pub fn fire_keepalive_if_idle(&self) {
        let idle_threshold = Duration::from_secs(30);
        let mut state = self.state.borrow_mut();
        let idle_time = state.last_server_data.elapsed();
        if idle_time >= idle_threshold && state.last_keepalive.elapsed() >= idle_threshold {
            // IAC NOP = \xff\xf1，telnet 标准心跳
            state.pending_raw.push(vec![0xff, 0xf1]);
            state.last_keepalive = Instant::now();
        }
    }

    /// 触发指定定时器（按名称查找）
    pub fn fire_timer_by_name(&self, name: &str) {
        // 使用 catch_unwind 防止 panic 跨越 FFI 边界导致静默崩溃
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            // 使用 timer_by_name HashMap O(1) 查找
            let index = self.state.borrow().timer_by_name.get(name).copied();
            if let Some(i) = index {
                self.fire_timer_inner(i);
            }
        }));
        if result.is_err() {
            self.log_error("fire_timer_by_name 中发生 panic，已捕获以防止崩溃");
        }
    }

    /// 触发指定定时器（按索引，供测试使用）
    pub fn fire_timer(&self, index: usize) {
        // 使用 catch_unwind 防止 panic 跨越 FFI 边界导致静默崩溃
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            self.fire_timer_inner(index);
        }));
        if result.is_err() {
            self.log_error("fire_timer 中发生 panic，已捕获以防止崩溃");
        }
    }

    /// 看门狗标记 helper：包裹执行逻辑，自动标记开始/结束
    /// 使用 Drop guard 确保 panic 时也能清除看门狗状态
    fn exec_with_watchdog(&self, name: &str, f: impl FnOnce()) {
        *self
            .exec_timer_name
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(name.to_string());
        self.exec_start.store(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64,
            Ordering::Relaxed,
        );
        // Drop guard：无论 f() 正常返回还是 panic，都清除看门狗状态
        struct WatchdogGuard<'a>(&'a LuaEngine);
        impl Drop for WatchdogGuard<'_> {
            fn drop(&mut self) {
                self.0.exec_start.store(0, Ordering::Relaxed);
            }
        }
        let _guard = WatchdogGuard(self);
        f();
    }

    /// fire_timer_by_name / fire_timer 的内部实现
    ///
    /// 依赖外层 `fire_timer_by_name` 的 `catch_unwind` 兜底，
    /// 本函数不再使用步骤级 `catch_unwind`，避免 panic 后继续执行后续步骤导致状态不一致。
    fn fire_timer_inner(&self, index: usize) {
        // 读取定时器信息
        let (callback, send_text, one_shot, timer_name) = {
            let state = self.state.borrow();
            if index < state.timers.len() && state.timers[index].enabled {
                (
                    state.timers[index].callback.clone(),
                    state.timers[index].send_text.clone(),
                    state.timers[index].one_shot,
                    state.timers[index].name.clone(),
                )
            } else {
                return;
            }
        };

        // 调用回调（如果存在）
        if let Some(cb) = callback {
            self.exec_with_watchdog(&timer_name, || {
                if let Err(e) = cb.call::<()>(()) {
                    self.log_error(&format!(
                        "[Lua] 定时器 '{}' 回调中发生 Lua 错误: {}",
                        timer_name, e
                    ));
                }
            });
        }

        // 执行 send_text
        if !send_text.is_empty() {
            self.exec_with_watchdog(&timer_name, || {
                // send_text 是 MUSHclient 的 script 参数
                // 判断是函数名还是 Lua 代码：
                // 函数名格式：identifier 或 identifier.identifier（如 "fire_timer_cb" 或 "wait.timer_resume"）
                // Lua 代码：包含空格、赋值、运算符等（如 "counter = counter + 1"）
                let is_function_name = send_text
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
                    && !send_text.is_empty()
                    && send_text
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_alphabetic() || c == '_');

                let result: Result<(), String> = if is_function_name {
                    let code = format!("{}('{}')", send_text, timer_name.replace('\'', "\\'"));
                    self.lua.load(&code).exec().map_err(|e| format!("{}", e))
                } else {
                    self.lua
                        .load(&send_text)
                        .exec()
                        .map_err(|e| format!("{}", e))
                };

                if let Err(lua_err) = result {
                    self.log_error(&format!(
                        "定时器 '{}' send_text 执行 Lua 错误: {}",
                        timer_name, lua_err
                    ));
                }
            });
        }

        // one_shot 删除定时器（通过 name 查找删除，保证索引一致性）
        if one_shot {
            let mut state = self.state.borrow_mut();
            state.delete_timer(&timer_name);
        }
    }

    /// 批量收集并触发所有到期定时器
    ///
    /// MushClient 兼容：一轮扫描收集所有到期 timer 名称，然后逐个触发。
    /// 绝对时间模型：触发前先推进 `next_fire`，防漂移，与 MushClient 的
    /// `tFireTime += interval` 行为一致。
    ///
    /// 返回触发的定时器名称列表（供调用方决定是否刷新 UI）
    pub fn fire_due_timers(&self) -> Vec<String> {
        let now = std::time::Instant::now();
        let mut due_names: Vec<String> = Vec::new();

        // 安全守卫：确认 pending_commands 为空（主循环每轮结束时已 drain，
        // 若入口处非空说明有命令泄漏，需排查调用路径）。
        // 注意：不能放在 fire_timer_inner 内部——同一轮内前一个 timer 回调
        // 通过 Execute 产生的命令会残留到下一个 timer 触发时，属合法累积，
        // 全部触发完后由主循环统一 drain。
        debug_assert!(
            self.state.borrow().pending_commands.is_empty(),
            "pending_commands should be empty before timer fires"
        );

        // 阶段1：扫描所有 timer，收集到期名称并推进 next_fire
        {
            let mut state = self.state.borrow_mut();
            for timer in state.timers.iter_mut() {
                if timer.enabled && now >= timer.next_fire {
                    // 周期性 timer：先推进 next_fire（在回调前，防漂移）
                    // one_shot timer 即将删除，无需推进
                    if !timer.one_shot {
                        if timer.at_time {
                            // at_time timer：每天固定时刻触发，推进 24h
                            timer.next_fire += Duration::from_secs(86400);
                            if timer.next_fire <= now {
                                timer.next_fire = now + Duration::from_secs(86400);
                            }
                        } else {
                            timer.next_fire += Duration::from_millis(timer.interval_millis);
                            // 防漂移保护：如果累加后仍在过去（如时钟跳变），重新对齐
                            if timer.next_fire <= now {
                                timer.next_fire =
                                    now + Duration::from_millis(timer.interval_millis);
                            }
                        }
                    }
                    due_names.push(timer.name.clone());
                }
            }
        }

        // 阶段2：逐个触发到期 timer
        for name in &due_names {
            self.fire_timer_by_name(name);
        }

        due_names
    }
}
