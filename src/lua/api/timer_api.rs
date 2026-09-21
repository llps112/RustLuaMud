//! 定时器 API 注册
//!
//! 对应拆分前 `api.rs` 中的「定时器 API」分节，文件开头另附文件级自由函数
//! `compute_next_at_time`（at_time 定时器计算下次触发时刻）。

use mlua::{Result as LuaResult, Value};

use crate::lua::helpers::{coerce_to_f64, coerce_to_i64, coerce_to_string};
use crate::lua::types::{LuaEngine, TimerDef};

/// 计算 at_time timer 的下次触发时间（本地时区的下一个 HH:MM:SS）
fn compute_next_at_time(hour: i64, min: i64, sec: f64) -> std::time::Instant {
    use chrono::{Duration as ChronoDuration, Local, NaiveTime};
    let now = Local::now();
    let target_time = NaiveTime::from_hms_opt(
        hour.clamp(0, 23) as u32,
        min.clamp(0, 59) as u32,
        sec.floor().clamp(0.0, 59.0) as u32,
    )
    .unwrap_or_else(|| NaiveTime::from_hms_opt(0, 0, 0).unwrap());
    let today_target = now.date_naive().and_time(target_time);
    let next = if today_target >= now.naive_local() {
        today_target
    } else {
        today_target + ChronoDuration::days(1)
    };
    let duration = (next - now.naive_local())
        .to_std()
        .unwrap_or(std::time::Duration::from_secs(86400));
    std::time::Instant::now() + duration
}

impl LuaEngine {
    pub(super) fn register_timer_api(&mut self) -> LuaResult<()> {
        let lua = &self.lua;
        let globals = lua.globals();
        let state_rc = self.state.clone();

        // ============================================================
        // 定时器 API
        // ============================================================

        // AddTimer(name, hour, min, sec, response_text, flags, [script_name], [send_to])
        // MushClient API 兼容：参数5是字符串(response_text)，参数7是字符串(script_name)
        // sec 参数支持浮点数（如 0.10 秒）和 nil（默认 0）
        let state_rc19 = state_rc.clone();
        let add_timer_fn = lua.create_function_mut(move |_lua, args: mlua::MultiValue| {
            let args: Vec<mlua::Value> = args.into_vec();

            // 至少需要6个参数: name, hour, min, sec, response_text, flags
            if args.len() < 6 {
                return Err(mlua::Error::external(
                    "AddTimer 需要至少6个参数: name, hour, min, sec, response_text, flags",
                ));
            }

            let name: String = coerce_to_string(args[0].clone())?;
            let _hour: i64 = coerce_to_i64(args[1].clone()).unwrap_or(0);
            let _min: i64 = coerce_to_i64(args[2].clone()).unwrap_or(0);
            // sec 支持浮点数和 nil（MushClient 兼容）
            let sec_val = coerce_to_f64(args[3].clone()).unwrap_or(0.0);
            // 综合计算：总秒数 = hour*3600 + min*60 + sec
            let total_secs = (_hour as f64) * 3600.0 + (_min as f64) * 60.0 + sec_val;
            let interval_millis = if total_secs <= 0.0 {
                1000.0
            } else {
                total_secs * 1000.0
            };
            // 第5个参数 response_text：send_to=0 时作为 MUD 命令发送
            let response_text = coerce_to_string(args[4].clone()).unwrap_or_default();
            let flags: i64 = coerce_to_i64(args[5].clone()).unwrap_or(0);
            // 第7个参数 script_name（可选）
            let script_name = if args.len() > 6 {
                coerce_to_string(args[6].clone()).unwrap_or_default()
            } else {
                String::new()
            };
            // 第8个参数 send_to（可选，默认 0=发送到 MUD）
            let send_to: i64 = if args.len() > 7 {
                coerce_to_i64(args[7].clone()).unwrap_or(0)
            } else {
                0
            };

            let interval_millis_u64 = interval_millis as u64;
            let one_shot = (flags & 4) != 0;
            let at_time = (flags & 2) != 0;

            // 决定 send_text 内容（按优先级）：
            // 1. script_name 非空 → 作为 Lua 代码执行
            // 2. response_text 非空且 send_to=0 → 作为 MUD 命令发送
            // 3. 否则 → 空（什么都不做）
            let send_text = if !script_name.is_empty() {
                script_name
            } else if !response_text.is_empty() && send_to == 0 {
                format!("Execute([[\n{}\n]])", response_text)
            } else {
                script_name // 空串
            };

            // Replace flag (1024): 替换同名定时器，保留旧定时器的启用状态
            // 防止 closeclass 禁用定时器后被 AddTimer(Replace) 重新启用
            let old_enabled = if (flags & 1024) != 0 {
                let old_enabled = state_rc19
                    .borrow()
                    .timer_by_name
                    .get(&name)
                    .map(|&i| state_rc19.borrow().timers[i].enabled);
                state_rc19.borrow_mut().delete_timer(&name);
                old_enabled
            } else {
                None
            };

            let timer_enabled = match old_enabled {
                // 替换旧定时器时：旧定时器若被禁用，新定时器继承禁用状态
                Some(false) => false,
                // 旧定时器启用或无旧定时器，按 flags 决定
                _ => (flags & 1) != 0,
            };

            // at_time timer：计算到下一个本地 HH:MM:SS 时刻
            // 否则：now + interval
            let next_fire = if at_time {
                compute_next_at_time(_hour, _min, sec_val)
            } else {
                std::time::Instant::now() + std::time::Duration::from_millis(interval_millis_u64)
            };

            state_rc19.borrow_mut().add_timer(TimerDef {
                name,
                interval_millis: interval_millis_u64,
                callback: None,
                enabled: timer_enabled,
                group: String::new(),
                one_shot,
                at_time,
                temporary: false,
                send_text,
                next_fire,
            });
            Ok(Value::Integer(0))
        })?;
        globals.set("AddTimer", add_timer_fn)?;

        // DoAfter(seconds, text) — 一次性临时定时器，发送文本到 MUD (send_to=0)
        let state_rc_da = state_rc.clone();
        let doafter_fn = lua.create_function_mut(move |_lua, (seconds, text): (f64, String)| {
            if !(0.1..=86399.0).contains(&seconds) {
                return Ok(Value::Integer(1)); // eTimeInvalid
            }
            let interval_millis = (seconds * 1000.0) as u64;
            let send_text = format!("Execute([[{}]])", text);
            state_rc_da
                .borrow_mut()
                .add_doafter_timer("__doafter", interval_millis, send_text);
            Ok(Value::Integer(0)) // eOK
        })?;
        globals.set("DoAfter", doafter_fn)?;

        // DoAfterNote(seconds, text) — 一次性临时定时器，输出文本到窗口 (send_to=2)
        let state_rc_dn = state_rc.clone();
        let doafter_note_fn =
            lua.create_function_mut(move |_lua, (seconds, text): (f64, String)| {
                if !(0.1..=86399.0).contains(&seconds) {
                    return Ok(Value::Integer(1)); // eTimeInvalid
                }
                let interval_millis = (seconds * 1000.0) as u64;
                let send_text = format!("Note([[{}]])", text);
                state_rc_dn.borrow_mut().add_doafter_timer(
                    "__doafter_note",
                    interval_millis,
                    send_text,
                );
                Ok(Value::Integer(0))
            })?;
        globals.set("DoAfterNote", doafter_note_fn)?;

        // DoAfterSpecial(seconds, text, send_to) — 可指定目标位置
        let state_rc_ds = state_rc.clone();
        let doafter_special_fn =
            lua.create_function_mut(move |_lua, (seconds, text, send_to): (f64, String, i64)| {
                if !(0.1..=86399.0).contains(&seconds) {
                    return Ok(Value::Integer(1)); // eTimeInvalid
                }
                if !(0..=14).contains(&send_to) {
                    return Ok(Value::Integer(2)); // eOptionOutOfRange
                }
                let interval_millis = (seconds * 1000.0) as u64;
                let send_text = match send_to {
                    0 | 10 | 13 => format!("Execute([[{}]])", text), // World / Execute / Immediate
                    2 => format!("Note([[{}]])", text),              // Output window
                    3 => format!("SetStatus([[{}]])", text),         // Status line
                    11 => format!("Execute([[{}]])", text),          // Speedwalk (Execute 处理)
                    12 | 14 => text,                                 // Script engine — 直接执行 Lua
                    _ => format!("Execute([[{}]])", text),           // 默认走 Execute
                };
                state_rc_ds.borrow_mut().add_doafter_timer(
                    "__doafter_special",
                    interval_millis,
                    send_text,
                );
                Ok(Value::Integer(0))
            })?;
        globals.set("DoAfterSpecial", doafter_special_fn)?;

        // DoAfterSpeedWalk(seconds, text) — speedwalk 定时器 (send_to=11)
        let state_rc_dw = state_rc.clone();
        let doafter_sw_fn =
            lua.create_function_mut(move |_lua, (seconds, text): (f64, String)| {
                if !(0.1..=86399.0).contains(&seconds) {
                    return Ok(Value::Integer(1)); // eTimeInvalid
                }
                let interval_millis = (seconds * 1000.0) as u64;
                let send_text = format!("Execute([[{}]])", text);
                state_rc_dw.borrow_mut().add_doafter_timer(
                    "__doafter_sw",
                    interval_millis,
                    send_text,
                );
                Ok(Value::Integer(0))
            })?;
        globals.set("DoAfterSpeedWalk", doafter_sw_fn)?;

        // DeleteTimer(name)
        let state_rc20 = state_rc.clone();
        let delete_timer_fn = lua.create_function_mut(move |_, name: String| {
            let mut state = state_rc20.borrow_mut();
            let found = state.delete_timer(&name);
            if found {
                Ok(0)
            } else {
                Ok(1)
            }
        })?;
        globals.set("DeleteTimer", delete_timer_fn)?;

        // GetTimerList()
        let state_rc21 = state_rc.clone();
        let get_timer_list_fn = lua.create_function_mut(move |lua, ()| {
            let state = state_rc21.borrow();
            let list = lua.create_table()?;
            for (i, t) in state.timers.iter().enumerate() {
                list.set(i + 1, t.name.as_str())?;
            }
            Ok(Value::Table(list))
        })?;
        globals.set("GetTimerList", get_timer_list_fn)?;

        // GetTimerInfo(name, code) — MushClient API 兼容
        // code 6 = enabled (Boolean), 7 = one_shot (Boolean), 8 = at_time (Boolean), 19 = group (String)
        let state_rc22 = state_rc.clone();
        let get_timer_info_fn =
            lua.create_function_mut(move |lua, (name, code): (String, i64)| {
                let state = state_rc22.borrow();
                if let Some(t) = state.timer_by_name.get(&name).map(|&i| &state.timers[i]) {
                    match code {
                        1 => Ok(Value::String(lua.create_string(&t.name)?)),
                        6 => Ok(Value::Boolean(t.enabled)), // enabled
                        7 => Ok(Value::Boolean(t.one_shot)), // one shot
                        8 => Ok(Value::Boolean(t.at_time)), // "At" timer flag
                        14 => Ok(Value::Boolean(t.temporary)), // temporary flag
                        19 => {
                            let group = t.group.clone();
                            Ok(Value::String(lua.create_string(&group)?))
                        }
                        _ => Ok(Value::Nil),
                    }
                } else {
                    Ok(Value::Nil)
                }
            })?;
        globals.set("GetTimerInfo", get_timer_info_fn)?;

        // SetTimerOption(name, key, value)
        let state_rc23 = state_rc.clone();
        let set_timer_option_fn =
            lua.create_function_mut(move |_, (name, key, value): (String, String, Value)| {
                let mut state = state_rc23.borrow_mut();
                let idx = state.timer_by_name.get(&name).copied();
                if let Some(i) = idx {
                    // group 变更需要同步更新索引，单独处理
                    if key == "group" {
                        if let Value::String(s) = value {
                            let new_group = s.to_str().map(|s| s.to_string()).unwrap_or_default();
                            state.update_timer_group(i, &new_group);
                        }
                        return Ok(Value::Integer(0));
                    }
                    let t = &mut state.timers[i];
                    match key.as_str() {
                        "timer_timestamp" => {
                            if let Value::Integer(ts) = value {
                                if ts > 0 {
                                    let current_time = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_secs();
                                    let offset = current_time.saturating_sub(ts as u64);
                                    // 绝对时间模型：通过偏移量计算下次触发时间
                                    t.next_fire = std::time::Instant::now()
                                        - std::time::Duration::from_secs(offset)
                                        + std::time::Duration::from_millis(t.interval_millis);
                                } else {
                                    t.next_fire = std::time::Instant::now()
                                        + std::time::Duration::from_millis(t.interval_millis);
                                }
                            }
                        }
                        "enabled" => {
                            if let Value::Boolean(b) = value {
                                t.enabled = b;
                            } else if let Value::Integer(n) = value {
                                t.enabled = n != 0;
                            }
                        }
                        "send_to" => {}
                        _ => {}
                    }
                    Ok(Value::Integer(0))
                } else {
                    Ok(Value::Integer(1))
                }
            })?;
        globals.set("SetTimerOption", set_timer_option_fn)?;

        // EnableTimerGroup(group_name, enable)
        let state_rc24 = state_rc.clone();
        let enable_timer_group_fn =
            lua.create_function_mut(move |_, (group, enable): (String, bool)| {
                let mut state = state_rc24.borrow_mut();
                state.enable_timer_group(&group, enable);
                Ok(())
            })?;
        globals.set("EnableTimerGroup", enable_timer_group_fn)?;

        // EnableTimer(name, enable)
        let state_rc_emt = state_rc.clone();
        let enable_timer_fn =
            lua.create_function_mut(move |_, (name, enable): (String, bool)| {
                let mut state = state_rc_emt.borrow_mut();
                let idx = state.timer_by_name.get(&name).copied();
                if let Some(i) = idx {
                    state.timers[i].enabled = enable;
                    Ok(Value::Integer(0))
                } else {
                    Ok(Value::Integer(1))
                }
            })?;
        globals.set("EnableTimer", enable_timer_fn)?;

        // ResetTimer(name) — MushClient API: 重置定时器计时
        let state_rc_rt = state_rc.clone();
        let reset_timer_fn = lua.create_function_mut(move |_, name: String| {
            let mut state = state_rc_rt.borrow_mut();
            let idx = state.timer_by_name.get(&name).copied();
            if let Some(i) = idx {
                let timer = &mut state.timers[i];
                timer.next_fire = if timer.at_time {
                    // at_time timer：重置到下一个本地 HH:MM:SS 时刻
                    // 从 interval_millis 反推 hour/min/sec
                    let total_secs = timer.interval_millis / 1000;
                    let hour = (total_secs / 3600) as i64;
                    let min = ((total_secs % 3600) / 60) as i64;
                    let sec = (total_secs % 60) as f64;
                    compute_next_at_time(hour, min, sec)
                } else {
                    std::time::Instant::now()
                        + std::time::Duration::from_millis(timer.interval_millis)
                };
                Ok(Value::Integer(0))
            } else {
                Ok(Value::Integer(1))
            }
        })?;
        globals.set("ResetTimer", reset_timer_fn)?;

        // IsTimer(name) — 测试 timer 是否存在
        // 返回 0=存在（eOK），1=不存在（与 DeleteTimer 返回值模式一致）
        let state_rc_it = state_rc.clone();
        let is_timer_fn = lua.create_function_mut(move |_, name: String| {
            Ok(Value::Integer(
                if state_rc_it.borrow().timer_by_name.contains_key(&name) {
                    0
                } else {
                    1
                },
            ))
        })?;
        globals.set("IsTimer", is_timer_fn)?;

        Ok(())
    }
}
