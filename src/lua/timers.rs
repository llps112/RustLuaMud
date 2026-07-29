//! 定时器调度与执行
//!
//! 处理定时器触发，包含：
//! - `fire_keepalive_if_idle`: 空闲心跳（IAC NOP）
//! - `fire_timer` / `fire_timer_by_name`: 按索引/名称触发定时器（含外层 panic 防御）
//! - `fire_timer_inner`: 实际触发逻辑（回调调用、send_text 执行、one_shot 删除）
//! - `fire_next_due_timer`: 触发第一个到期的定时器
//!
//! 注意：`fire_timer_inner` 不再包含步骤级 `catch_unwind`，依赖外层
//! `fire_timer` / `fire_timer_by_name` 的 `catch_unwind` 兜底。

use std::panic::AssertUnwindSafe;
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

use super::types::LuaEngine;

impl LuaEngine {
    /// 检查服务器是否长时间无响应，是则发送 IAC NOP 心跳包保持连接
    /// 空闲超过 30 秒则发送一次 IAC NOP
    pub fn fire_keepalive_if_idle(&self) {
        let idle_threshold = std::time::Duration::from_secs(30);
        let idle_time = {
            let state = self.state.borrow();
            state.last_server_data.elapsed()
        };
        if idle_time >= idle_threshold {
            // IAC NOP = \xff\xf1，telnet 标准心跳
            self.state.borrow_mut().pending_raw.push(vec![0xff, 0xf1]);
        }
    }

    /// 触发指定定时器（按名称查找，避免索引失效）
    pub fn fire_timer_by_name(&self, name: &str) {
        // 使用 catch_unwind 防止 panic 跨越 FFI 边界导致静默崩溃
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let index = self
                .state
                .borrow()
                .timers
                .iter()
                .position(|t| t.name == name);
            if let Some(i) = index {
                self.fire_timer_inner(i);
            }
        }));
        if result.is_err() {
            self.log_error("fire_timer_by_name 中发生 panic，已捕获以防止崩溃");
        }
    }

    /// 触发指定定时器（按索引，仅供内部使用）
    #[allow(dead_code)]
    pub fn fire_timer(&self, index: usize) {
        // 使用 catch_unwind 防止 panic 跨越 FFI 边界导致静默崩溃
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            self.fire_timer_inner(index);
        }));
        if result.is_err() {
            self.log_error("fire_timer 中发生 panic，已捕获以防止崩溃");
        }
    }

    /// fire_timer 的内部实现
    ///
    /// 依赖外层 `fire_timer` / `fire_timer_by_name` 的 `catch_unwind` 兜底，
    /// 本函数不再使用步骤级 `catch_unwind`，避免 panic 后继续执行后续步骤导致状态不一致。
    fn fire_timer_inner(&self, index: usize) {
        // 步骤1: 清空待发送队列
        self.state.borrow_mut().pending_commands.clear();

        // 步骤2: 读取定时器信息
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

        // 步骤3: 调用回调（如果存在）
        if let Some(cb) = callback {
            // 标记看门狗开始
            *self
                .exec_timer_name
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(timer_name.clone());
            self.exec_start.store(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64,
                Ordering::Relaxed,
            );

            if let Err(e) = cb.call::<()>(()) {
                self.log_error(&format!(
                    "[Lua] 定时器 '{}' 回调中发生 Lua 错误: {}",
                    timer_name, e
                ));
            }

            // 标记看门狗结束
            self.exec_start.store(0, Ordering::Relaxed);
        }

        // 步骤4: 执行 send_text
        if !send_text.is_empty() {
            // 标记看门狗开始
            *self
                .exec_timer_name
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(timer_name.clone());
            self.exec_start.store(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64,
                Ordering::Relaxed,
            );

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

            // 标记看门狗结束
            self.exec_start.store(0, Ordering::Relaxed);

            if let Err(lua_err) = result {
                self.log_error(&format!(
                    "定时器 '{}' send_text 执行 Lua 错误: {}",
                    timer_name, lua_err
                ));
            }
        }

        // 步骤5: one_shot 删除定时器（通过索引辅助函数，保证索引一致性）
        if one_shot {
            let mut state = self.state.borrow_mut();
            state.delete_timer(&timer_name);
        }
    }

    /// 检查并触发第一个到期的定时器
    /// 返回 true 如果触发了某个定时器
    /// 注意：返回定时器名称而非索引，避免回调修改 timers 向量后索引失效
    pub fn fire_next_due_timer(&self) -> bool {
        let now = std::time::Instant::now();
        let timer_name = {
            let mut state = self.state.borrow_mut();
            let mut found = None;
            for timer in state.timers.iter_mut() {
                if timer.enabled {
                    let elapsed = now.duration_since(timer.last_fired);
                    if elapsed.as_millis() as u64 >= timer.interval_millis {
                        timer.last_fired = now;
                        found = Some(timer.name.clone());
                        break;
                    }
                }
            }
            found
        };

        match timer_name {
            Some(name) => {
                self.fire_timer_by_name(&name);
                true
            }
            None => false,
        }
    }
}
