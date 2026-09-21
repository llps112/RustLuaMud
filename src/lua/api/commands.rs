//! 命令执行 / 工具函数 API 注册
//!
//! 对应拆分前 `api.rs` 中的「命令执行」「工具函数」两个分节。

use mlua::{Result as LuaResult, Value};

use crate::lua::helpers::i64_to_lua_integer;
use crate::lua::types::{LuaEngine, StyleRun, TriggerPattern};

impl LuaEngine {
    pub(super) fn register_commands_api(&mut self) -> LuaResult<()> {
        let lua = &self.lua;
        let globals = lua.globals();
        let state_rc = self.state.clone();

        // ============================================================
        // 命令执行
        // ============================================================

        // send(command)
        let state_rc2 = state_rc.clone();
        let send_fn = lua.create_function_mut(move |_, cmd: String| {
            state_rc2.borrow_mut().pending_commands.push(cmd);
            Ok(())
        })?;
        globals.set("send", send_fn)?;

        // Execute(command) — MushClient API
        let state_rc3 = state_rc.clone();
        let execute_fn = lua.create_function_mut(move |_, cmd: String| {
            state_rc3.borrow_mut().pending_commands.push(cmd);
            Ok(0)
        })?;
        globals.set("Execute", execute_fn)?;

        // DiscardQueue() — MushClient API: 丢弃命令队列中所有待发送命令
        let state_rc_dq = state_rc.clone();
        let discard_queue_fn = lua.create_function_mut(move |_, ()| {
            state_rc_dq.borrow_mut().pending_commands.clear();
            Ok(())
        })?;
        globals.set("DiscardQueue", discard_queue_fn)?;

        // SendPkt(data) — MushClient API: 发送原始数据包到 MUD
        let state_rc_pkt = state_rc.clone();
        let send_pkt_fn =
            lua.create_function_mut(move |_, data: mlua::LuaString| -> LuaResult<i64> {
                let bytes = data.as_bytes().to_vec();
                // 限制单包大小，防止恶意或错误脚本导致内存暴涨
                if bytes.len() > 65536 {
                    return Err(mlua::Error::external(format!(
                        "SendPkt: 数据包过大 ({} 字节，上限 65536)",
                        bytes.len()
                    )));
                }
                state_rc_pkt.borrow_mut().pending_raw.push(bytes);
                Ok(0)
            })?;
        globals.set("SendPkt", send_pkt_fn)?;

        // Simulate(text...) — MushClient API: 模拟 MUD 输出，触发匹配的触发器
        // Lua 特性：多个参数会被拼接
        let state_rc_sim = state_rc.clone();
        let simulate_fn = lua.create_function(move |lua, args: mlua::MultiValue| {
            let mut text = String::new();
            for v in args.iter() {
                match v {
                    mlua::Value::String(s) => {
                        text.push_str(&s.to_string_lossy());
                    }
                    mlua::Value::Integer(n) => {
                        text.push_str(&n.to_string());
                    }
                    mlua::Value::Number(n) => {
                        text.push_str(&n.to_string());
                    }
                    _ => {}
                }
            }
            // 按换行符分割，逐行处理
            for line in text.split('\n') {
                let line = line.trim_end_matches('\r');
                if line.is_empty() {
                    continue;
                }

                let clean_line = crate::ui::AnsiParser::strip_ansi(line);
                let clean_line = clean_line.trim_end_matches('\r').to_string();

                // 维护最近行缓冲区
                {
                    let mut state = state_rc_sim.borrow_mut();
                    state.recent_lines.push(clean_line.clone());
                    if state.recent_lines.len() > 20 {
                        state.recent_lines.remove(0);
                    }
                }

                let gbk_line = encoding_rs::GBK.encode(&clean_line).0.into_owned();

                // 收集匹配结果（与 process_output 相同的逻辑，但不清空 pending_commands）
                let matches: Vec<(usize, String, Vec<String>, Vec<StyleRun>)> = {
                    let state = state_rc_sim.borrow();
                    let mut result = Vec::new();
                    for (i, trigger) in state.triggers.iter().enumerate() {
                        if !trigger.enabled {
                            continue;
                        }
                        match &trigger.pattern {
                            TriggerPattern::Gbk(gbk_re) => {
                                if trigger.multiline && trigger.lines_to_match > 1 {
                                    let n = trigger.lines_to_match;
                                    if state.recent_lines.len() >= n {
                                        let combined: String = state
                                            .recent_lines
                                            .iter()
                                            .rev()
                                            .take(n)
                                            .rev()
                                            .cloned()
                                            .collect::<Vec<_>>()
                                            .join("\n");
                                        let gbk_combined =
                                            encoding_rs::GBK.encode(&combined).0.into_owned();
                                        if let Some(caps) = gbk_re.captures(&gbk_combined) {
                                            let full_match = {
                                                let m = caps.get(0).unwrap();
                                                let (cow, _, _) =
                                                    encoding_rs::GBK.decode(m.as_bytes());
                                                cow.into_owned()
                                            };
                                            let caps_list: Vec<String> = caps
                                                .iter()
                                                .skip(1)
                                                .flatten()
                                                .map(|m| {
                                                    let (cow, _, _) =
                                                        encoding_rs::GBK.decode(m.as_bytes());
                                                    cow.into_owned()
                                                })
                                                .collect();
                                            result.push((i, full_match, caps_list, Vec::new()));
                                        }
                                    }
                                } else if let Some(caps) = gbk_re.captures(&gbk_line) {
                                    let full_match = {
                                        let m = caps.get(0).unwrap();
                                        let (cow, _, _) = encoding_rs::GBK.decode(m.as_bytes());
                                        cow.into_owned()
                                    };
                                    let caps_list: Vec<String> = caps
                                        .iter()
                                        .skip(1)
                                        .flatten()
                                        .map(|m| {
                                            let (cow, _, _) = encoding_rs::GBK.decode(m.as_bytes());
                                            cow.into_owned()
                                        })
                                        .collect();
                                    result.push((i, full_match, caps_list, Vec::new()));
                                }
                            }
                            TriggerPattern::Utf8(utf8_re) => {
                                if trigger.multiline && trigger.lines_to_match > 1 {
                                    let n = trigger.lines_to_match;
                                    if state.recent_lines.len() >= n {
                                        let combined: String = state
                                            .recent_lines
                                            .iter()
                                            .rev()
                                            .take(n)
                                            .rev()
                                            .cloned()
                                            .collect::<Vec<_>>()
                                            .join("\n");
                                        if let Some(caps) = utf8_re.captures(&combined) {
                                            let full_match =
                                                caps.get(0).unwrap().as_str().to_string();
                                            let caps_list: Vec<String> = caps
                                                .iter()
                                                .skip(1)
                                                .flatten()
                                                .map(|m| m.as_str().to_string())
                                                .collect();
                                            result.push((i, full_match, caps_list, Vec::new()));
                                        }
                                    }
                                } else if let Some(caps) = utf8_re.captures(&clean_line) {
                                    let full_match = caps.get(0).unwrap().as_str().to_string();
                                    let caps_list: Vec<String> = caps
                                        .iter()
                                        .skip(1)
                                        .flatten()
                                        .map(|m| m.as_str().to_string())
                                        .collect();
                                    result.push((i, full_match, caps_list, Vec::new()));
                                }
                            }
                        }
                    }
                    result
                };

                // 判断是否需要 omit_from_output
                let mut any_omit = false;

                // 逐个触发回调
                for (idx, full_match, caps_list, _sr) in matches {
                    let (callback, send_text, trigger_name, omit) = {
                        let state = state_rc_sim.borrow();
                        (
                            state.triggers[idx].callback.clone(),
                            state.triggers[idx].send_text.clone(),
                            state.triggers[idx].name.clone(),
                            state.triggers[idx].omit_from_output,
                        )
                    };
                    if omit {
                        any_omit = true;
                    }
                    // MUSHclient 触发器回调签名: function(name, line, wildcards, styles)
                    if let Ok(wildcards_table) = lua.create_table() {
                        // w[0] = 完整匹配文本（MUSHclient 兼容）
                        let _ = wildcards_table.set(0, full_match.as_str());
                        for (i, m) in caps_list.iter().enumerate() {
                            let _ = wildcards_table.set(i + 1, m.as_str());
                        }
                        // 使用 catch_unwind 防止 Rust panic 跨越 Lua FFI 边界导致静默崩溃
                        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            if let Err(e) = callback.call::<()>((
                                trigger_name.as_str(),
                                clean_line.as_str(),
                                wildcards_table,
                                mlua::Value::Nil,
                            )) {
                                eprintln!(
                                    "[Lua] Simulate 触发器 '{}' 回调中发生 Lua 错误: {}",
                                    trigger_name, e
                                );
                                if let Ok(mut sim_state) = state_rc_sim.try_borrow_mut() {
                                    sim_state.pending_logs.push(format!(
                                        "[Lua] Simulate 触发器 '{}' 回调中发生 Lua 错误: {}",
                                        trigger_name, e
                                    ));
                                }
                            }
                        }))
                        .is_err()
                        {
                            eprintln!(
                                "[Lua] Simulate 触发器 '{}' 回调中发生 panic，已捕获以防止崩溃",
                                trigger_name
                            );
                            if let Ok(mut sim_state) = state_rc_sim.try_borrow_mut() {
                                sim_state.pending_logs.push(format!(
                                    "[Lua] Simulate 触发器 '{}' 回调中发生 panic",
                                    trigger_name
                                ));
                            }
                        }
                    }
                    if !send_text.is_empty() {
                        state_rc_sim.borrow_mut().pending_commands.push(send_text);
                    }
                }

                // 添加到日志（显示在输出窗口），除非被 omit
                if !any_omit {
                    state_rc_sim
                        .borrow_mut()
                        .pending_logs
                        .push(line.to_string());
                }
            }
            Ok(())
        })?;
        globals.set("Simulate", simulate_fn)?;

        // DeleteTemporaryTimers() — MushClient API: 删除所有临时定时器
        // 按 temporary 标志过滤（DoAfter 系列置位），而非 one_shot——普通 AddTimer
        // 建的 OneShot 定时器不属于「临时定时器」，不应被误删。
        let state_rc_dtt = state_rc.clone();
        let delete_temp_timers_fn = lua.create_function_mut(move |_, ()| {
            let mut state = state_rc_dtt.borrow_mut();
            let to_delete: Vec<String> = state
                .timers
                .iter()
                .filter(|t| t.temporary)
                .map(|t| t.name.clone())
                .collect();
            for name in to_delete {
                state.delete_timer(&name);
            }
            Ok(())
        })?;
        globals.set("DeleteTemporaryTimers", delete_temp_timers_fn)?;

        Ok(())
    }

    pub(super) fn register_utils_api(&mut self) -> LuaResult<()> {
        let lua = &self.lua;
        let globals = lua.globals();
        let state_rc = self.state.clone();

        // ============================================================
        // 工具函数
        // ============================================================

        // GetUniqueNumber()
        let state_rc28 = state_rc.clone();
        let get_unique_number_fn = lua.create_function_mut(move |_, ()| {
            let mut state = state_rc28.borrow_mut();
            state.unique_counter += 1;
            Ok(Value::Integer(i64_to_lua_integer(
                state.unique_counter as i64,
            )))
        })?;
        globals.set("GetUniqueNumber", get_unique_number_fn)?;

        // Trim(string)
        let trim_fn = lua.create_function(move |_, s: String| Ok(s.trim().to_string()))?;
        globals.set("Trim", trim_fn)?;

        Ok(())
    }
}
