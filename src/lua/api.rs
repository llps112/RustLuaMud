//! MushClient 兼容 Lua API 注册
//!
//! 本模块实现 `LuaEngine::register_api`，注册所有 MushClient 兼容的 Lua API
//! （send/Execute/Note/ColourNote/AddTrigger/AddTimer/AddAlias/GetInfo/...）。

use std::sync::{Arc, Mutex};

use mlua::{Function, Result as LuaResult, Table, Value};
use regex::bytes::Regex as BytesRegex;
use regex::Regex;
use rusqlite::Connection;

use super::database::LuaDb;
use super::helpers::{
    coerce_to_f64, coerce_to_i64, coerce_to_string, colour_to_ansi_bg, colour_to_ansi_fg,
    convert_pcre_to_rust_regex, fix_lua_escape_sequences, i64_to_lua_integer, json_to_lua_value,
    lua_integer_to_i64, lua_value_to_json, regex_escape, utf8_regex_to_gbk_bytes,
};
use super::types::{
    Alias, LuaEngine, PanelUpdate, ScriptEncoding, ScriptState, StyleRun, TimerDef, Trigger,
    TriggerPattern,
};
use crate::ui::terminal::PanelButtonDef;

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
    pub(super) fn register_api(&mut self) -> LuaResult<()> {
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
        let state_rc_dtt = state_rc.clone();
        let delete_temp_timers_fn = lua.create_function_mut(move |_, ()| {
            let mut state = state_rc_dtt.borrow_mut();
            let to_delete: Vec<String> = state
                .timers
                .iter()
                .filter(|t| t.one_shot)
                .map(|t| t.name.clone())
                .collect();
            for name in to_delete {
                state.delete_timer(&name);
            }
            Ok(())
        })?;
        globals.set("DeleteTemporaryTimers", delete_temp_timers_fn)?;

        // ============================================================
        // 输出
        // ============================================================

        // log(message)
        let state_rc4 = state_rc.clone();
        let log_fn = lua.create_function_mut(move |_, msg: String| {
            state_rc4.borrow_mut().pending_logs.push(msg);
            Ok(())
        })?;
        globals.set("log", log_fn)?;

        // ColourNote(fg, bg, text)
        let state_rc5 = state_rc.clone();
        let colour_note_fn =
            lua.create_function_mut(move |_, (fg, bg, text): (String, String, String)| {
                let fg_code = colour_to_ansi_fg(&fg);
                let bg_code = colour_to_ansi_bg(&bg);
                let msg = format!("\x1b[{};{}m{}\x1b[0m", fg_code, bg_code, text);
                state_rc5.borrow_mut().pending_logs.push(msg);
                Ok(())
            })?;
        globals.set("ColourNote", colour_note_fn)?;

        // Note(text)
        let state_rc6 = state_rc.clone();
        let note_fn = lua.create_function_mut(move |_, text: String| {
            let mut state = state_rc6.borrow_mut();
            let buffered = std::mem::take(&mut state.tell_buffer);
            let full_msg = if buffered.is_empty() {
                text
            } else {
                format!("{}{}", buffered, text)
            };
            state.pending_logs.push(full_msg);
            Ok(())
        })?;
        globals.set("Note", note_fn)?;

        // print(...) — 覆盖标准 Lua print，重定向到 pending_logs
        // 标准 Lua print 行为：参数间用 \t 分隔，末尾追加 \n
        let state_rc_print = state_rc.clone();
        let print_fn = lua.create_function_mut(move |_lua, args: mlua::MultiValue| {
            let mut parts = Vec::new();
            for v in args.iter() {
                match v {
                    mlua::Value::Nil => parts.push("nil".to_string()),
                    mlua::Value::String(s) => {
                        // to_str() 借用了 lua 状态的引用，需要转换生命周期
                        let s = s.as_bytes().to_vec();
                        parts.push(String::from_utf8_lossy(&s).to_string());
                    }
                    mlua::Value::Number(n) => parts.push((*n).to_string()),
                    mlua::Value::Integer(i) => parts.push((*i).to_string()),
                    mlua::Value::Boolean(b) => {
                        parts.push(if *b { "true" } else { "false" }.to_string())
                    }
                    mlua::Value::Table(t) => {
                        parts.push(format!("{:?}", t));
                    }
                    mlua::Value::Function(_) => parts.push("function".to_string()),
                    mlua::Value::Thread(_) => parts.push("thread".to_string()),
                    mlua::Value::UserData(_) => parts.push("userdata".to_string()),
                    mlua::Value::Error(e) => parts.push(format!("{:?}", e)),
                    _ => parts.push("?".to_string()),
                }
            }
            let msg = parts.join("\t");
            let mut state = state_rc_print.borrow_mut();
            // 先 flush tell_buffer 中的内联内容，与 print 内容合并为一行
            let buffered = std::mem::take(&mut state.tell_buffer);
            let full_msg = if buffered.is_empty() {
                msg
            } else {
                format!("{}{}", buffered, msg)
            };
            state.pending_logs.push(full_msg);
            drop(state);
            Ok(())
        })?;
        globals.set("print", print_fn)?;

        // SetStatus(text) — MushClient API: 设置状态栏文本
        let state_rc_note = state_rc.clone();
        let set_status_fn = lua.create_function_mut(move |_, text: String| {
            // 存储状态栏文本，UI 层可读取显示
            state_rc_note.borrow_mut().status_text = text;
            Ok(())
        })?;
        globals.set("SetStatus", set_status_fn)?;

        // SetPanel(name, x, y, width, height, text[, buttons]) — 创建/更新浮动面板
        // buttons 是可选的第 7 参数，格式: {{ row=11, start_col=3, end_col=11, action="go" }, ...}
        let state_rc_panel = state_rc.clone();
        let set_panel_fn = lua.create_function_mut(move |_lua, mut args: mlua::MultiValue| {
            if args.len() < 6 {
                return Err(mlua::Error::external(
                    "SetPanel 至少需要 6 个参数: name, x, y, width, height, text",
                ));
            }
            let name: String = {
                let v = args
                    .remove(0)
                    .ok_or_else(|| mlua::Error::external("SetPanel: 缺少 name 参数"))?;
                mlua::FromLua::from_lua(v, _lua)
                    .map_err(|_| mlua::Error::external("SetPanel: name 必须是字符串"))?
            };
            let x: i16 = {
                let v = args
                    .remove(0)
                    .ok_or_else(|| mlua::Error::external("SetPanel: 缺少 x 参数"))?;
                mlua::FromLua::from_lua(v, _lua)
                    .map_err(|_| mlua::Error::external("SetPanel: x 必须是数字"))?
            };
            let y: i16 = {
                let v = args
                    .remove(0)
                    .ok_or_else(|| mlua::Error::external("SetPanel: 缺少 y 参数"))?;
                mlua::FromLua::from_lua(v, _lua)
                    .map_err(|_| mlua::Error::external("SetPanel: y 必须是数字"))?
            };
            let width: u16 = {
                let v = args
                    .remove(0)
                    .ok_or_else(|| mlua::Error::external("SetPanel: 缺少 width 参数"))?;
                mlua::FromLua::from_lua(v, _lua)
                    .map_err(|_| mlua::Error::external("SetPanel: width 必须是数字"))?
            };
            let height: u16 = {
                let v = args
                    .remove(0)
                    .ok_or_else(|| mlua::Error::external("SetPanel: 缺少 height 参数"))?;
                mlua::FromLua::from_lua(v, _lua)
                    .map_err(|_| mlua::Error::external("SetPanel: height 必须是数字"))?
            };
            let text: String = {
                let v = args
                    .remove(0)
                    .ok_or_else(|| mlua::Error::external("SetPanel: 缺少 text 参数"))?;
                mlua::FromLua::from_lua(v, _lua)
                    .map_err(|_| mlua::Error::external("SetPanel: text 必须是字符串"))?
            };
            let lines: Vec<String> = text.split('\n').map(|s| s.to_string()).collect();
            // 解析可选的 buttons 参数（第 7 个）
            let buttons = if !args.is_empty() {
                let v = args
                    .remove(0)
                    .ok_or_else(|| mlua::Error::external("SetPanel: 缺少 buttons 参数"))?;
                let table: mlua::Table = mlua::FromLua::from_lua(v, _lua)
                    .map_err(|_| mlua::Error::external("SetPanel: buttons 必须是 table"))?;
                let mut defs = Vec::new();
                for pair in table.pairs::<mlua::Integer, mlua::Table>() {
                    let (_, btn) = pair.map_err(|e| {
                        mlua::Error::external(format!("SetPanel: buttons 元素无效: {}", e))
                    })?;
                    let row: u16 = btn.get("row").map_err(|e| {
                        mlua::Error::external(format!("SetPanel: buttons 缺 row 字段: {}", e))
                    })?;
                    let start_col: u16 = btn.get("start_col").map_err(|e| {
                        mlua::Error::external(format!("SetPanel: buttons 缺 start_col 字段: {}", e))
                    })?;
                    let end_col: u16 = btn.get("end_col").map_err(|e| {
                        mlua::Error::external(format!("SetPanel: buttons 缺 end_col 字段: {}", e))
                    })?;
                    let action: String = btn.get("action").map_err(|e| {
                        mlua::Error::external(format!("SetPanel: buttons 缺 action 字段: {}", e))
                    })?;
                    // 校验按钮坐标在面板范围内
                    if row >= height {
                        return Err(mlua::Error::external(format!(
                            "SetPanel: buttons row {} 超出面板高度 {}",
                            row, height
                        )));
                    }
                    if end_col > width || start_col >= end_col {
                        return Err(mlua::Error::external(format!(
                            "SetPanel: buttons start_col {} end_col {} 超出面板宽度 {} 或范围无效",
                            start_col, end_col, width
                        )));
                    }
                    defs.push(PanelButtonDef {
                        row,
                        start_col,
                        end_col,
                        action,
                    });
                }
                defs
            } else {
                Vec::new()
            };
            state_rc_panel
                .borrow_mut()
                .pending_panels
                .push(PanelUpdate::Set {
                    name,
                    x,
                    y,
                    width,
                    height,
                    lines,
                    buttons,
                });
            Ok(())
        })?;
        globals.set("SetPanel", set_panel_fn)?;

        // RemovePanel(name) — 扩展 API: 移除浮动面板
        let state_rc_panel_rm = state_rc.clone();
        let remove_panel_fn = lua.create_function_mut(move |_, name: String| {
            state_rc_panel_rm
                .borrow_mut()
                .pending_panels
                .push(PanelUpdate::Remove { name });
            Ok(())
        })?;
        globals.set("RemovePanel", remove_panel_fn)?;

        // RegisterPanelHandler(panel_name, callback) — 注册面板点击回调
        // panel_name: 面板名称（与 SetPanel 的 name 参数一致）
        // callback: function(panel_name, action) — 点击按钮时调用
        //
        // 设计意图: 解耦客户端与脚本。客户端不硬编码脚本侧函数名,
        // 脚本通过此 API 主动注册回调, 与 AddTrigger/AddAlias/AddTimer 模式一致。
        let state_rc_panel_handler = state_rc.clone();
        let register_panel_handler_fn =
            lua.create_function_mut(move |_, (panel_name, callback): (String, mlua::Function)| {
                state_rc_panel_handler
                    .borrow_mut()
                    .panel_handlers
                    .insert(panel_name, callback);
                Ok(())
            })?;
        globals.set("RegisterPanelHandler", register_panel_handler_fn)?;

        // Tell(text...) — 追加到 tell_buffer，实现内联输出（支持多参数拼接）
        let state_rc7 = state_rc.clone();
        let tell_fn = lua.create_function_mut(move |_lua, args: mlua::MultiValue| {
            let mut text = String::new();
            for v in args.iter() {
                match v {
                    mlua::Value::Nil => text.push_str("nil"),
                    mlua::Value::String(s) => {
                        let s = s.as_bytes().to_vec();
                        text.push_str(&String::from_utf8_lossy(&s));
                    }
                    mlua::Value::Number(n) => text.push_str(&n.to_string()),
                    mlua::Value::Integer(i) => text.push_str(&i.to_string()),
                    mlua::Value::Boolean(b) => text.push_str(if *b { "true" } else { "false" }),
                    _ => text.push_str(&format!("{:?}", v)),
                }
            }
            state_rc7.borrow_mut().tell_buffer.push_str(&text);
            Ok(())
        })?;
        globals.set("Tell", tell_fn)?;

        // ============================================================
        // JSON 序列化桥接（供 Web UI 使用）
        // ============================================================

        // json_encode(value) → JSON string
        let json_encode_fn = lua.create_function_mut(move |_lua, value: mlua::Value| {
            let json_val = lua_value_to_json(&value);
            let json_str = serde_json::to_string(&json_val)
                .map_err(|e| mlua::Error::external(format!("json_encode 失败: {}", e)))?;
            Ok(json_str)
        })?;
        globals.set("json_encode", json_encode_fn)?;

        // json_decode(json_string) → Lua value
        let json_decode_fn = lua.create_function_mut(move |lua, json_str: String| {
            let json_val: serde_json::Value = serde_json::from_str(&json_str)
                .map_err(|e| mlua::Error::external(format!("json_decode 失败: {}", e)))?;
            let lua_val = json_to_lua_value(lua, &json_val)?;
            Ok(lua_val)
        })?;
        globals.set("json_decode", json_decode_fn)?;

        // ============================================================
        // 触发器 API
        // ============================================================

        // AddTrigger(name, match_str, response, flags, colour, wildcard, sound, script, send_to, sequence)
        let state_rc8 = state_rc.clone();
        let add_trigger_fn = lua.create_function_mut(
            move |lua,
                  (
                name,
                match_str,
                _response,
                flags,
                _colour,
                _wildcard,
                _sound,
                script,
                _send_to,
                sequence,
            ): (
                String,
                String,
                String,
                i64,
                i64,
                i64,
                String,
                String,
                i64,
                i64,
            )| {
                add_trigger_impl(
                    lua,
                    &state_rc8,
                    &name,
                    &match_str,
                    flags,
                    &script,
                    _send_to,
                    sequence as i32,
                )
            },
        )?;
        globals.set("AddTrigger", add_trigger_fn)?;

        // AddTriggerEx(name, match_str, response_text, flags, [colour], [wildcard], [sound], [script], [send_to], [sequence])
        // MushClient API 兼容：中间参数可选，可能传 nil
        let state_rc9 = state_rc.clone();
        let add_trigger_ex_fn = lua.create_function_mut(move |lua, args: mlua::MultiValue| {
            let args: Vec<mlua::Value> = args.into_vec();

            // 至少需要4个参数: name, match_str, response_text, flags
            if args.len() < 4 {
                return Err(mlua::Error::external(
                    "AddTriggerEx 需要至少4个参数: name, match_str, response_text, flags",
                ));
            }

            let name: String = coerce_to_string(args[0].clone())?;
            let match_str: String = coerce_to_string(args[1].clone())?;
            let _response: String = coerce_to_string(args[2].clone())?;
            let flags: i64 = coerce_to_i64(args[3].clone())?;
            // 第5个参数 colour（可选，忽略）
            // 第6个参数 wildcard（可选，忽略）
            // 第7个参数 sound（可选，忽略）
            // 第8个参数 script（可选）
            let script = if args.len() > 7 && !args[7].is_nil() {
                coerce_to_string(args[7].clone())?
            } else {
                String::new()
            };
            // 第9个参数 send_to（可选，忽略）
            let _send_to: i64 = if args.len() > 8 && !args[8].is_nil() {
                coerce_to_i64(args[8].clone()).unwrap_or(0)
            } else {
                0
            };
            // 第10个参数 sequence（可选）
            let sequence: i64 = if args.len() > 9 && !args[9].is_nil() {
                coerce_to_i64(args[9].clone()).unwrap_or(0)
            } else {
                0
            };

            add_trigger_impl(
                lua,
                &state_rc9,
                &name,
                &match_str,
                flags,
                &script,
                _send_to,
                sequence as i32,
            )
        })?;
        globals.set("AddTriggerEx", add_trigger_ex_fn)?;

        // DeleteTrigger(name)
        let state_rc10 = state_rc.clone();
        let delete_trigger_fn = lua.create_function_mut(move |_, name: String| {
            let mut state = state_rc10.borrow_mut();
            let found = state.delete_trigger(&name);
            if found {
                Ok(0)
            } else {
                Ok(1)
            }
        })?;
        globals.set("DeleteTrigger", delete_trigger_fn)?;

        // GetTriggerList()
        let state_rc11 = state_rc.clone();
        let get_trigger_list_fn = lua.create_function_mut(move |lua, ()| {
            let state = state_rc11.borrow();
            let list = lua.create_table()?;
            for (i, t) in state.triggers.iter().enumerate() {
                list.set(i + 1, t.name.as_str())?;
            }
            Ok(Value::Table(list))
        })?;
        globals.set("GetTriggerList", get_trigger_list_fn)?;

        // GetTriggerInfo(name, code) — MushClient API 兼容
        // code 8 = enabled (Boolean), code 26 = group (String)
        let state_rc12 = state_rc.clone();
        let get_trigger_info_fn =
            lua.create_function_mut(move |lua, (name, code): (String, i64)| {
                let state = state_rc12.borrow();
                if let Some(t) = state
                    .trigger_by_name
                    .get(&name)
                    .map(|&i| &state.triggers[i])
                {
                    match code {
                        1 => Ok(Value::String(lua.create_string(&t.name)?)),
                        2 => Ok(Value::String(lua.create_string(&match &t.pattern {
                            TriggerPattern::Utf8(re) => re.as_str().to_string(),
                            TriggerPattern::Gbk(_) => "<gbk pattern>".to_string(),
                        })?)),
                        4 => {
                            let mut flags = 0i64;
                            if t.enabled {
                                flags |= 1;
                            }
                            Ok(Value::Integer(i64_to_lua_integer(flags)))
                        }
                        5 => Ok(Value::Integer(0)),
                        6 => Ok(Value::Integer(i64_to_lua_integer(t.sequence as i64))),
                        7 => Ok(Value::Boolean(true)), // Keep evaluating (MushClient 默认 true)
                        8 => Ok(Value::Boolean(t.enabled)),
                        9 => Ok(Value::String(lua.create_string(&match &t.pattern {
                            TriggerPattern::Utf8(re) => re.as_str().to_string(),
                            TriggerPattern::Gbk(_) => "<gbk pattern>".to_string(),
                        })?)),
                        26 => {
                            let group = t.group.clone();
                            Ok(Value::String(lua.create_string(&group)?))
                        }
                        _ => Ok(Value::Nil),
                    }
                } else {
                    Ok(Value::Nil)
                }
            })?;
        globals.set("GetTriggerInfo", get_trigger_info_fn)?;

        // SetTriggerOption(name, key, value)
        let state_rc13 = state_rc.clone();
        let set_trigger_option_fn =
            lua.create_function_mut(move |_lua, (name, key, value): (String, String, Value)| {
                let mut state = state_rc13.borrow_mut();
                let encoding = state.current_encoding;
                let idx = state.trigger_by_name.get(&name).copied();
                if let Some(i) = idx {
                    // group 变更需要同步更新索引，单独处理
                    if key == "group" {
                        if let Value::String(s) = value {
                            let new_group = s.to_str().map(|s| s.to_string()).unwrap_or_default();
                            state.update_trigger_group(i, &new_group);
                        }
                        return Ok(Value::Integer(0));
                    }
                    let t = &mut state.triggers[i];
                    match key.as_str() {
                        "regexp" => {
                            if let Value::String(s) = value {
                                let pattern = s.to_str().map_err(|e| {
                                    mlua::Error::external(format!("无效正则字符串: {}", e))
                                })?;
                                let pattern = pattern.to_string();
                                let re_str = convert_pcre_to_rust_regex(&pattern);
                                match encoding {
                                    ScriptEncoding::Gbk => {
                                        let gbk_str = utf8_regex_to_gbk_bytes(&re_str);
                                        let gbk_re = BytesRegex::new(&gbk_str).map_err(|e| {
                                            mlua::Error::external(format!(
                                                "无效GBK正则 '{}': {}",
                                                gbk_str, e
                                            ))
                                        })?;
                                        t.pattern = TriggerPattern::Gbk(gbk_re);
                                    }
                                    ScriptEncoding::Utf8 => {
                                        let re = Regex::new(&re_str).map_err(|e| {
                                            mlua::Error::external(format!(
                                                "无效正则 '{}': {}",
                                                re_str, e
                                            ))
                                        })?;
                                        t.pattern = TriggerPattern::Utf8(re);
                                    }
                                }
                            }
                        }
                        "sequence" => {
                            if let Value::Integer(n) = value {
                                t.sequence = n as i32;
                            }
                        }
                        "multi_line" | "multiline" => {
                            if let Value::Boolean(b) = value {
                                t.multiline = b;
                            } else if let Value::Integer(n) = value {
                                t.multiline = n != 0;
                            }
                        }
                        "lines_to_match" => {
                            if let Value::Integer(n) = value {
                                t.lines_to_match = n as usize;
                            }
                        }
                        "omit_from_output" => {
                            if let Value::Boolean(b) = value {
                                t.omit_from_output = b;
                            } else if let Value::Integer(n) = value {
                                t.omit_from_output = n != 0;
                            }
                        }
                        "enabled" => {
                            if let Value::Boolean(b) = value {
                                t.enabled = b;
                            } else if let Value::Integer(n) = value {
                                t.enabled = n != 0;
                            }
                        }
                        "send" => {
                            if let Value::String(s) = value {
                                t.send_text = s.to_str().map(|s| s.to_string()).unwrap_or_default();
                            }
                        }
                        _ => {}
                    }
                    Ok(Value::Integer(0))
                } else {
                    Ok(Value::Integer(1))
                }
            })?;
        globals.set("SetTriggerOption", set_trigger_option_fn)?;

        // EnableTriggerGroup(group_name, enable)
        let state_rc14 = state_rc.clone();
        let enable_trigger_group_fn =
            lua.create_function_mut(move |_, (group, enable): (String, bool)| {
                let mut state = state_rc14.borrow_mut();
                state.enable_trigger_group(&group, enable);
                Ok(())
            })?;
        globals.set("EnableTriggerGroup", enable_trigger_group_fn)?;

        // EnableTrigger(name, enable)
        let state_rc_et = state_rc.clone();
        let enable_trigger_fn =
            lua.create_function_mut(move |_, (name, enable): (String, bool)| {
                let mut state = state_rc_et.borrow_mut();
                let idx = state.trigger_by_name.get(&name).copied();
                if let Some(i) = idx {
                    state.triggers[i].enabled = enable;
                    Ok(Value::Integer(0))
                } else {
                    Ok(Value::Integer(1))
                }
            })?;
        globals.set("EnableTrigger", enable_trigger_fn)?;

        // ============================================================
        // 别名 API
        // ============================================================

        // AddAlias(name, match_str, response_text, flags, [script_name])
        // MushClient API 兼容：参数5是字符串(script_name)，可选
        let state_rc15 = state_rc.clone();
        let add_alias_fn = lua.create_function_mut(move |lua, args: mlua::MultiValue| {
            let args: Vec<mlua::Value> = args.into_vec();

            // 至少需要4个参数: name, match_str, response_text, flags
            if args.len() < 4 {
                return Err(mlua::Error::external(
                    "AddAlias 需要至少4个参数: name, match_str, response_text, flags",
                ));
            }

            let name: String = coerce_to_string(args[0].clone())?;
            let match_str: String = coerce_to_string(args[1].clone())?;
            let response: String = coerce_to_string(args[2].clone())?;
            let flags: i64 = coerce_to_i64(args[3].clone())?;
            // 第5个参数 script_name（可选）
            let script = if args.len() > 4 {
                coerce_to_string(args[4].clone())?
            } else {
                String::new()
            };
            let is_regex = (flags & 32) != 0;
            let do_replace = (flags & 1024) != 0;

            // Replace flag (1024): 先删除同名 alias
            if do_replace {
                state_rc15.borrow_mut().delete_alias(&name);
            }

            let re_str = if is_regex {
                convert_pcre_to_rust_regex(&match_str)
            } else {
                regex_escape(&match_str)
                    .replace('*', "(.*)")
                    .replace('?', "(.)")
            };
            let re = Regex::new(&re_str)
                .map_err(|e| mlua::Error::external(format!("无效正则 '{}': {}", re_str, e)))?;

            // script 参数在 MUSHclient 中是函数名（不传参），send_to=12 时使用 response 作为 Lua 代码
            let callback: Function = if !response.is_empty() {
                // 有 response 文本时，先创建空函数，执行时再动态替换 %1 并执行
                lua.create_function(|_, _: ()| Ok(()))?
            } else if !script.is_empty() {
                let code = format!("return {}", script);
                match lua.load(&code).eval::<Function>() {
                    Ok(f) => f,
                    Err(_) => lua.load(&script).eval()?,
                }
            } else {
                lua.create_function(|_, _: ()| Ok(()))?
            };

            // MUSHclient AddAlias 默认行为：
            // 当 response 非空且没有提供 script 参数（或 script 为空字符串）时，send_to 默认为 12（执行 Lua 代码）
            let has_script = args.len() > 4 && {
                let s = coerce_to_string(args[4].clone()).unwrap_or_default();
                !s.is_empty()
            };
            let send_to = if !response.is_empty() && !has_script {
                12 // send to script — Lua 代码执行
            } else {
                0 // send to world
            };

            state_rc15.borrow_mut().add_alias(Alias {
                name,
                match_text: match_str,
                pattern: re,
                callback,
                enabled: (flags & 1) != 0,
                group: String::new(),
                send_to,
                response,
                sequence: 0,
            });
            Ok(Value::Integer(0))
        })?;
        globals.set("AddAlias", add_alias_fn)?;

        // DeleteAlias(name)
        let state_rc16 = state_rc.clone();
        let delete_alias_fn = lua.create_function_mut(move |_, name: String| {
            let mut state = state_rc16.borrow_mut();
            let found = state.delete_alias(&name);
            if found {
                Ok(0)
            } else {
                Ok(1)
            }
        })?;
        globals.set("DeleteAlias", delete_alias_fn)?;

        // GetAliasList()
        let state_rc17 = state_rc.clone();
        let get_alias_list_fn = lua.create_function_mut(move |lua, ()| {
            let state = state_rc17.borrow();
            let list = lua.create_table()?;
            for (i, a) in state.aliases.iter().enumerate() {
                list.set(i + 1, a.name.as_str())?;
            }
            Ok(Value::Table(list))
        })?;
        globals.set("GetAliasList", get_alias_list_fn)?;

        // GetAliasInfo(name, code) — MushClient API 兼容
        let state_rc_gi = state_rc.clone();
        let get_alias_info_fn =
            lua.create_function_mut(move |lua, (name, code): (String, i64)| {
                let state = state_rc_gi.borrow();
                if let Some(a) = state.alias_by_name.get(&name).map(|&i| &state.aliases[i]) {
                    match code {
                        1 => Ok(Value::String(lua.create_string(&a.match_text)?)),
                        2 => Ok(Value::String(lua.create_string(&a.response)?)),
                        3 => Ok(Value::String(lua.create_string("")?)),
                        4 => Ok(Value::Boolean(false)),
                        5 => Ok(Value::Boolean(false)),
                        6 => Ok(Value::Boolean(a.enabled)),
                        7 => Ok(Value::Boolean(false)),
                        8 => Ok(Value::Boolean(true)),
                        9 => Ok(Value::Boolean(false)),
                        10 => Ok(Value::Integer(0)),
                        11 => Ok(Value::Integer(0)),
                        12 => Ok(Value::Boolean(false)),
                        13 => Ok(Value::Nil),
                        14 => Ok(Value::Boolean(false)),
                        15 => Ok(Value::Boolean(false)),
                        16 => Ok(Value::String(lua.create_string(&a.group)?)),
                        17 => Ok(Value::String(lua.create_string("")?)),
                        18 => Ok(Value::Integer(i64_to_lua_integer(a.send_to))),
                        19 => Ok(Value::Integer(1)),
                        20 => Ok(Value::Integer(i64_to_lua_integer(a.sequence as i64))),
                        21 => Ok(Value::Boolean(true)),
                        22 => Ok(Value::Boolean(false)),
                        23 => Ok(Value::Integer(0)),
                        24 => Ok(Value::Integer(0)),
                        25 => Ok(Value::Nil),
                        26 => Ok(Value::Boolean(true)),
                        27 => Ok(Value::Boolean(true)),
                        28 => Ok(Value::Integer(0)),
                        29 => Ok(Value::Boolean(false)),
                        30 => Ok(Value::Number(0.0)),
                        31 => Ok(Value::Integer(0)),
                        _ => Ok(Value::Nil),
                    }
                } else {
                    Ok(Value::Nil)
                }
            })?;
        globals.set("GetAliasInfo", get_alias_info_fn)?;

        // SetAliasOption(name, key, value)
        let state_rc18 = state_rc.clone();
        let set_alias_option_fn =
            lua.create_function_mut(move |_, (name, key, value): (String, String, Value)| {
                let mut state = state_rc18.borrow_mut();
                let idx = state.alias_by_name.get(&name).copied();
                if let Some(i) = idx {
                    // group 变更需要同步更新索引，单独处理
                    if key == "group" {
                        if let Value::String(s) = value {
                            let new_group = s.to_str().map(|s| s.to_string()).unwrap_or_default();
                            state.update_alias_group(i, &new_group);
                        }
                        return Ok(Value::Integer(0));
                    }
                    let a = &mut state.aliases[i];
                    match key.as_str() {
                        "regexp" => {
                            if let Value::String(s) = value {
                                let pattern = s.to_str().map_err(|e| {
                                    mlua::Error::external(format!("无效正则字符串: {}", e))
                                })?;
                                let pattern = pattern.to_string();
                                let re_str = convert_pcre_to_rust_regex(&pattern);
                                let re = Regex::new(&re_str).map_err(|e| {
                                    mlua::Error::external(format!("无效正则 '{}': {}", re_str, e))
                                })?;
                                a.pattern = re;
                            }
                        }
                        "sequence" => {
                            if let Value::Integer(n) = value {
                                a.sequence = n as i32;
                            }
                        }
                        "enabled" => {
                            if let Value::Boolean(b) = value {
                                a.enabled = b;
                            } else if let Value::Integer(n) = value {
                                a.enabled = n != 0;
                            }
                        }
                        "send_to" => {
                            if let Value::Integer(n) = value {
                                a.send_to = lua_integer_to_i64(n);
                            } else if let Value::Number(n) = value {
                                a.send_to = n as i64;
                            }
                        }
                        _ => {}
                    }
                    Ok(Value::Integer(0))
                } else {
                    Ok(Value::Integer(1))
                }
            })?;
        globals.set("SetAliasOption", set_alias_option_fn)?;

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
                        14 => Ok(Value::Boolean(false)),    // temporary flag (not tracked)
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

        // ============================================================
        // 配置 API
        // ============================================================

        // GetInfo(code) — MushClient API 兼容
        let script_path_rc = self.script_path.clone();
        let log_dir_rc = self.log_dir.clone();
        let state_rc_gi = state_rc.clone();
        let get_info_fn = lua.create_function_mut(move |lua, code: i64| match code {
            1 => {
                // MushClient: GetInfo(1) = Server name (IP address)
                let host = state_rc_gi.borrow().host.clone();
                Ok(Value::String(lua.create_string(&host)?))
            }
            2 => {
                // MushClient: GetInfo(2) = World name
                let name = state_rc_gi.borrow().world_name.clone();
                Ok(Value::String(lua.create_string(&name)?))
            }
            3 => {
                // MushClient: GetInfo(3) = Character name
                let name = state_rc_gi.borrow().char_name.clone();
                Ok(Value::String(lua.create_string(&name)?))
            }
            35 => {
                // MushClient: GetInfo(35) = Script file name (full path)
                // 保持反斜杠路径格式以兼容 MushClient 移植脚本
                let path = script_path_rc.borrow().clone();
                match path {
                    Some(p) => {
                        let win_path = p.replace('/', "\\");
                        Ok(Value::String(lua.create_string(&win_path)?))
                    }
                    None => Ok(Value::String(lua.create_string("")?)),
                }
            }
            56 => {
                // MushClient: GetInfo(56) = MUSHclient application path name
                // 本引擎不支持，返回空串
                Ok(Value::String(lua.create_string("")?))
            }
            58 => {
                // MushClient: GetInfo(58) = Log files default path (directory)
                // 返回配置的日志目录，供脚本写入日志文件
                let dir = log_dir_rc.borrow().clone();
                let sep = if cfg!(windows) { "\\" } else { "/" };
                let default_dir = format!("logs{}", sep);
                match dir {
                    Some(d) => Ok(Value::String(lua.create_string(&d)?)),
                    None => Ok(Value::String(lua.create_string(&default_dir)?)),
                }
            }
            204 => {
                // MushClient: GetInfo(204) = Packets received
                let count = state_rc_gi.borrow().packet_count;
                Ok(Value::Integer(i64_to_lua_integer(count as i64)))
            }
            _ => Ok(Value::String(lua.create_string("")?)),
        })?;
        globals.set("GetInfo", get_info_fn)?;

        // SetOption(name, value)
        let set_option_fn = lua.create_function(move |lua, (name, value): (String, Value)| {
            let options: Table = lua.globals().get("_mud_options")?;
            options.set(name, value)?;
            Ok(())
        })?;
        let mud_options = lua.create_table()?;
        mud_options.set("enable_timers", 1i64)?;
        mud_options.set("enable_triggers", 1i64)?;
        mud_options.set("enable_aliases", 1i64)?;
        mud_options.set("enable_scripts", 1i64)?;
        mud_options.set("enable_command_queue", 1i64)?;
        globals.set("_mud_options", mud_options)?;
        globals.set("SetOption", set_option_fn)?;

        // GetOption(name)
        let get_option_fn = lua.create_function(move |lua, name: String| {
            let options: Table = lua.globals().get("_mud_options")?;
            let val: Value = options.get(name.as_str())?;
            Ok(val)
        })?;
        globals.set("GetOption", get_option_fn)?;

        // SetAlphaOption(name, value)
        let set_alpha_option_fn =
            lua.create_function(move |lua, (name, value): (String, Value)| {
                let options: Table = lua.globals().get("_mud_alpha_options")?;
                options.set(name, value)?;
                Ok(())
            })?;
        globals.set("_mud_alpha_options", lua.create_table()?)?;
        globals.set("SetAlphaOption", set_alpha_option_fn)?;

        // GetAlphaOption(name)
        let get_alpha_option_fn = lua.create_function(move |lua, name: String| {
            let options: Table = lua.globals().get("_mud_alpha_options")?;
            let val: Value = options.get(name.as_str())?;
            Ok(val)
        })?;
        globals.set("GetAlphaOption", get_alpha_option_fn)?;

        // ============================================================
        // 连接状态 API
        // ============================================================

        // IsConnected()
        let state_rc25 = state_rc.clone();
        let is_connected_fn = lua
            .create_function_mut(move |_, ()| Ok(Value::Boolean(state_rc25.borrow().connected)))?;
        globals.set("IsConnected", is_connected_fn)?;

        // Connect()
        let state_rc26 = state_rc.clone();
        let connect_fn = lua.create_function_mut(move |_, ()| {
            state_rc26.borrow_mut().connect_requested = true;
            Ok(())
        })?;
        globals.set("Connect", connect_fn)?;

        // Disconnect()
        let state_rc27 = state_rc.clone();
        let disconnect_fn = lua.create_function_mut(move |_, ()| {
            state_rc27.borrow_mut().disconnect_requested = true;
            Ok(())
        })?;
        globals.set("Disconnect", disconnect_fn)?;

        // OnConnect() — 连接回调抽象接口，由 Lua 脚本覆盖实现具体逻辑
        // 默认空函数（安全无操作），脚本可覆盖以执行连接后的初始化
        let on_connect_fn = lua.create_function_mut(move |_, ()| Ok(()))?;
        globals.set("OnConnect", on_connect_fn)?;

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

        // ============================================================
        // ANSI 样式 API
        // ============================================================

        /// ANSI 标准色号→名称映射（0-15）
        const ANSI_COLOUR_NAMES: [(&str, u32); 16] = [
            ("black", 0),
            ("red", 1),
            ("green", 2),
            ("yellow", 3),
            ("blue", 4),
            ("magenta", 5),
            ("cyan", 6),
            ("silver", 7),
            ("grey", 8),
            ("bright red", 9),
            ("bright green", 10),
            ("bright yellow", 11),
            ("bright blue", 12),
            ("bright magenta", 13),
            ("bright cyan", 14),
            ("white", 15),
        ];

        /// 将 ANSI 色号转换为颜色名称
        fn ansi_colour_to_name(colour: u32) -> String {
            for (name, code) in &ANSI_COLOUR_NAMES {
                if *code == colour {
                    return name.to_string();
                }
            }
            format!("colour_{}", colour)
        }

        // GetStyle(styles, position) — MushClient API: 查询样式表中指定位置的样式
        // styles: 触发器回调的第 4 参数（一个表，包含所有样式运行片段）
        // position: 1-based 字节位置（Lua string.find 返回值）
        // 返回: {start, length, textcolour, backcolour, bold, italic, underline} 或 nil
        let get_style_fn = lua.create_function(|_, (styles, position): (mlua::Table, i64)| {
            let pos = if position > 0 {
                (position - 1) as usize // 转为 0-based
            } else {
                0usize
            };
            let len = styles.len().unwrap_or(0) as usize;
            // 遍历所有样式运行，找到包含 position 的那个
            for i in 1..=len {
                let entry: mlua::Table = match styles.get(i) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let start: usize = entry.get("start").unwrap_or(0);
                let length: usize = entry.get("length").unwrap_or(0);
                if pos >= start && pos < start + length {
                    return Ok(Value::Table(entry));
                }
            }
            Ok(Value::Nil)
        })?;
        globals.set("GetStyle", get_style_fn)?;

        // RGBColourToName(colour) — MushClient API: 色号转颜色名称
        let rgb_colour_to_name_fn =
            lua.create_function(|_lua, colour: i64| Ok(ansi_colour_to_name(colour as u32)))?;
        globals.set("RGBColourToName", rgb_colour_to_name_fn)?;

        // ============================================================
        // 变量 API
        // ============================================================

        // GetVariable(name)
        let state_rc29 = state_rc.clone();
        let get_variable_fn = lua.create_function_mut(move |lua, name: String| {
            let state = state_rc29.borrow();
            match state.variables.get(&name) {
                Some(val) => Ok(Value::String(lua.create_string(val)?)),
                None => Ok(Value::Nil),
            }
        })?;
        globals.set("GetVariable", get_variable_fn)?;

        // SetVariable(name, value)
        let state_rc30 = state_rc.clone();
        let set_variable_fn =
            lua.create_function_mut(move |_, (name, value): (String, String)| {
                state_rc30.borrow_mut().variables.insert(name, value);
                Ok(())
            })?;
        globals.set("SetVariable", set_variable_fn)?;

        // DeleteVariable(name)
        let state_rc31 = state_rc.clone();
        let delete_variable_fn = lua.create_function_mut(move |_, name: String| {
            state_rc31.borrow_mut().variables.remove(&name);
            Ok(())
        })?;
        globals.set("DeleteVariable", delete_variable_fn)?;

        // GetVariableList() — 返回 key-value 对表
        let state_rc32 = state_rc.clone();
        let get_variable_list_fn = lua.create_function_mut(move |lua, ()| {
            let state = state_rc32.borrow();
            let list = lua.create_table()?;
            for (k, v) in &state.variables {
                list.set(k.as_str(), v.as_str())?;
            }
            Ok(Value::Table(list))
        })?;
        globals.set("GetVariableList", get_variable_list_fn)?;

        // ============================================================
        // 日志 API
        // ============================================================

        // OpenLog(filename, append)
        let open_log_fn =
            lua.create_function(move |_, (_filename, _append): (String, bool)| Ok(()))?;
        globals.set("OpenLog", open_log_fn)?;

        // IsLogOpen()
        let is_log_open_fn = lua.create_function(move |_, ()| Ok(Value::Boolean(true)))?;
        globals.set("IsLogOpen", is_log_open_fn)?;

        // CloseLog() — MushClient API: 关闭日志文件
        let close_log_fn = lua.create_function(move |_, ()| Ok(()))?;
        globals.set("CloseLog", close_log_fn)?;

        // ============================================================
        // 数据库 API
        // ============================================================

        // DatabaseClose(dbname)
        let database_close_fn = lua.create_function(move |_, _dbname: String| Ok(()))?;
        globals.set("DatabaseClose", database_close_fn)?;

        // sqlite3 module
        let sqlite3_mod = lua.create_table()?;
        let open_fn = lua.create_function(|lua, path: String| {
            let conn = Connection::open(&path).map_err(|e| mlua::Error::external(e.to_string()))?;
            let db = LuaDb {
                conn: Arc::new(Mutex::new(conn)),
                text_is_gbk: false,
            };
            lua.create_userdata(db)
        })?;
        sqlite3_mod.set("open", open_fn)?;
        globals.set("sqlite3", sqlite3_mod)?;

        // ============================================================
        // 常量表
        // ============================================================

        // trigger_flag
        let trigger_flag = lua.create_table()?;
        trigger_flag.set("Enabled", 1i64)?;
        trigger_flag.set("OmitFromLog", 2i64)?;
        trigger_flag.set("OmitFromOutput", 4i64)?;
        trigger_flag.set("KeepEvaluating", 8i64)?;
        trigger_flag.set("IgnoreCase", 16i64)?;
        trigger_flag.set("RegularExpression", 32i64)?;
        trigger_flag.set("ExpandVariables", 64i64)?;
        trigger_flag.set("Replace", 1024i64)?;
        trigger_flag.set("LowercaseWildcard", 2048i64)?;
        trigger_flag.set("Temporary", 4096i64)?;
        trigger_flag.set("OneShot", 8192i64)?;
        globals.set("trigger_flag", trigger_flag)?;

        // alias_flag
        let alias_flag = lua.create_table()?;
        alias_flag.set("Enabled", 1i64)?;
        alias_flag.set("IgnoreCase", 16i64)?;
        alias_flag.set("RegularExpression", 32i64)?;
        alias_flag.set("ExpandVariables", 64i64)?;
        alias_flag.set("Replace", 1024i64)?;
        alias_flag.set("Temporary", 4096i64)?;
        globals.set("alias_flag", alias_flag)?;

        // timer_flag — 严格按 MushClient 官方定义
        let timer_flag = lua.create_table()?;
        timer_flag.set("Enabled", 1i64)?;
        timer_flag.set("AtTime", 2i64)?;
        timer_flag.set("OneShot", 4i64)?;
        timer_flag.set("TimerSpeedWalk", 8i64)?;
        timer_flag.set("TimerNote", 16i64)?;
        timer_flag.set("ActiveWhenClosed", 32i64)?;
        timer_flag.set("Replace", 1024i64)?;
        timer_flag.set("Temporary", 16384i64)?;
        globals.set("timer_flag", timer_flag)?;

        // custom_colour
        let custom_colour = lua.create_table()?;
        custom_colour.set("Black", 0i64)?;
        custom_colour.set("Maroon", 1i64)?;
        custom_colour.set("Green", 2i64)?;
        custom_colour.set("Olive", 3i64)?;
        custom_colour.set("Navy", 4i64)?;
        custom_colour.set("Purple", 5i64)?;
        custom_colour.set("Teal", 6i64)?;
        custom_colour.set("Silver", 7i64)?;
        custom_colour.set("Grey", 8i64)?;
        custom_colour.set("Red", 9i64)?;
        custom_colour.set("Lime", 10i64)?;
        custom_colour.set("Yellow", 11i64)?;
        custom_colour.set("Blue", 12i64)?;
        custom_colour.set("Fuchsia", 13i64)?;
        custom_colour.set("Aqua", 14i64)?;
        custom_colour.set("White", 15i64)?;
        globals.set("custom_colour", custom_colour)?;

        // error_code
        let error_code = lua.create_table()?;
        error_code.set("eOK", 0i64)?;
        error_code.set("eUnknownObject", 1i64)?;
        error_code.set("eItemAlreadyExists", 2i64)?;
        error_code.set("eBadRegularExpression", 3i64)?;
        error_code.set("eWildcardNotFound", 4i64)?;
        error_code.set("eCommandCancelled", 5i64)?;
        error_code.set("eNoSuchCommand", 6i64)?;
        error_code.set("eInvalidObjectLabel", 7i64)?;
        error_code.set("eAmbiguousObjectName", 8i64)?;
        globals.set("error_code", error_code)?;

        // error_desc
        let error_desc = lua.create_table()?;
        error_desc.set("eOK", "OK")?;
        error_desc.set("eUnknownObject", "Unknown object")?;
        error_desc.set("eItemAlreadyExists", "Item already exists")?;
        error_desc.set("eBadRegularExpression", "Bad regular expression")?;
        error_desc.set("eWildcardNotFound", "Wildcard not found")?;
        error_desc.set("eCommandCancelled", "Command cancelled")?;
        error_desc.set("eNoSuchCommand", "No such command")?;
        error_desc.set("eInvalidObjectLabel", "Invalid object label")?;
        error_desc.set("eAmbiguousObjectName", "Ambiguous object name")?;
        globals.set("error_desc", error_desc)?;

        // ============================================================
        // wait.lua 依赖
        // ============================================================

        // bit 库
        let bit_mod = lua.create_table()?;
        bit_mod.set(
            "bor",
            lua.create_function(|_, (a, b): (i64, i64)| Ok(a | b))?,
        )?;
        bit_mod.set(
            "band",
            lua.create_function(|_, (a, b): (i64, i64)| Ok(a & b))?,
        )?;
        bit_mod.set(
            "bxor",
            lua.create_function(|_, (a, b): (i64, i64)| Ok(a ^ b))?,
        )?;
        bit_mod.set("bnot", lua.create_function(|_, a: i64| Ok(!a))?)?;
        bit_mod.set(
            "lshift",
            lua.create_function(|_, (a, n): (i64, i64)| Ok(a << n))?,
        )?;
        bit_mod.set(
            "rshift",
            lua.create_function(|_, (a, n): (i64, i64)| Ok(a >> n))?,
        )?;
        globals.set("bit", bit_mod)?;

        // MakeRegularExpression(pattern) — 将通配符转为正则
        let make_re_fn = lua.create_function(move |lua, pattern: String| {
            let re = regex_escape(&pattern).replace('*', ".*").replace('?', ".");
            Ok(Value::String(lua.create_string(&re)?))
        })?;
        globals.set("MakeRegularExpression", make_re_fn)?;

        // GetPluginID()
        let get_plugin_id_fn =
            lua.create_function(move |lua, ()| Ok(Value::String(lua.create_string("")?)))?;
        globals.set("GetPluginID", get_plugin_id_fn)?;

        // GetPluginInfo(id, code) — MushClient API 兼容
        // 官方 code: 1=Name, 14=Date modified, 19=Version, 20=Directory
        let get_plugin_info_fn =
            lua.create_function(move |lua, (_id, code): (String, i64)| match code {
                1 => Ok(Value::String(lua.create_string("RustLuaMud")?)),
                14 => Ok(Value::String(lua.create_string("")?)),
                19 => Ok(Value::Number(1.0)),
                20 => Ok(Value::String(lua.create_string("")?)),
                _ => Ok(Value::Nil),
            })?;
        globals.set("GetPluginInfo", get_plugin_info_fn)?;

        // ============================================================
        // 模块加载机制
        // ============================================================

        // 覆盖 dofile — 支持 GBK 自动转码和路径分隔符兼容
        // 必须使用 create_function（不可变回调），因为 war_members.lua 内部会递归
        // 调用 dofile（加载 war_members_data.lua），create_function_mut 会阻止递归。
        let _script_path_rc = self.script_path.clone();
        let state_rc_dofile = state_rc.clone();
        let dofile_fn = lua.create_function(move |lua, path: String| {
            // 将 \ 替换为 /
            let path = path.replace('\\', "/");

            let bytes = std::fs::read(&path)
                .map_err(|e| mlua::Error::external(format!("读取文件失败 '{}': {}", path, e)))?;

            let (code, is_gbk) = match std::str::from_utf8(&bytes) {
                Ok(s) => (s.to_string(), false),
                Err(_) => {
                    let (cow, _, _) = encoding_rs::GBK.decode(&bytes);
                    (cow.into_owned(), true)
                }
            };

            // 设置当前脚本编码，触发器注册时会根据此标志选择匹配模式
            state_rc_dofile.borrow_mut().current_encoding = if is_gbk {
                ScriptEncoding::Gbk
            } else {
                ScriptEncoding::Utf8
            };

            // 预处理：修复 LuaJIT 不兼容的无效转义序列（如 \- \+ \? 等）
            let code = fix_lua_escape_sequences(&code);

            lua.load(&code)
                .set_name(&path)
                .exec()
                .map_err(|e| mlua::Error::external(format!("err '{}': {}", path, e)))
        })?;
        globals.set("dofile", dofile_fn)?;

        // 设置 require 路径
        let package: Table = globals.get("package")?;
        let current_path: String = package.get("path")?;
        let new_path = format!(
            "./scripts/lua/?.lua;./scripts/lua/?/init.lua;{}",
            current_path
        );
        package.set("path", new_path)?;

        // 注册 MushClient 兼容模块（空表，避免 require 报错）
        let loaded: Table = package.get("loaded")?;
        for module in &["InfoBox", "Gauge", "Miniwin"] {
            loaded.set(*module, lua.create_table()?)?;
        }

        // 注册 MushClient 兼容全局模块（rex PCRE 正则库，基于 Rust regex crate 实现）
        let rex_table = lua.create_table()?;

        // rex.new(pattern) -> 返回正则对象
        rex_table.set(
            "new",
            lua.create_function(|lua, pattern: String| {
                // PCRE 兼容：预处理正则模式
                let pattern = convert_pcre_to_rust_regex(&pattern);
                match regex::Regex::new(&pattern) {
                    Ok(re) => {
                        let regex_obj = lua.create_table()?;
                        let re_match = re.clone();
                        let re_gmatch = re.clone();
                        let re_split = re.clone();
                        let re_find = re.clone();

                        // regex_obj:match(subject) -> 返回匹配和捕获组
                        regex_obj.set(
                            "match",
                            lua.create_function(move |lua, (_self, subject): (Table, String)| {
                                match re_match.captures(&subject) {
                                    Some(caps) => {
                                        let result = lua.create_table()?;
                                        // 第一个捕获组是整体匹配
                                        if let Some(m) = caps.get(0) {
                                            result.set(1, m.as_str())?;
                                        }
                                        // 后续捕获组
                                        for (i, cap) in caps.iter().skip(1).enumerate() {
                                            if let Some(c) = cap {
                                                result.set((i + 2) as i64, c.as_str())?;
                                            }
                                        }
                                        Ok(mlua::Value::Table(result))
                                    }
                                    None => Ok(mlua::Value::Nil),
                                }
                            })?,
                        )?;

                        // regex_obj:gmatch(subject, callback) -> 对每个匹配调用 callback(match, cap1, cap2, ...)
                        regex_obj.set(
                            "gmatch",
                            lua.create_function(move |lua, (_self, subject, callback): (Table, String, Function)| {
                                for caps in re_gmatch.captures_iter(&subject) {
                                    let mut args = Vec::new();
                                    // 第一个参数是整体匹配
                                    if let Some(m) = caps.get(0) {
                                        args.push(mlua::Value::String(lua.create_string(m.as_str())?));
                                    }
                                    // 后续捕获组
                                    for cap in caps.iter().skip(1) {
                                        match cap {
                                            Some(c) => {
                                                args.push(mlua::Value::String(lua.create_string(c.as_str())?));
                                            }
                                            None => {
                                                args.push(mlua::Value::Nil);
                                            }
                                        }
                                    }
                                    // 调用回调，忽略返回值和错误
                                    let _ = callback.call::<mlua::Value>(mlua::MultiValue::from_vec(args));
                                }
                                Ok(mlua::Value::Nil)
                            })?,
                        )?;

                        // regex_obj:split(subject) -> 返回分割后的表
                        regex_obj.set(
                            "split",
                            lua.create_function(move |lua, (_self, subject): (Table, String)| {
                                let result = lua.create_table()?;
                                let parts: Vec<&str> = re_split.split(&subject).collect();
                                for (i, part) in parts.iter().enumerate() {
                                    result.set((i + 1) as i64, *part)?;
                                }
                                Ok(mlua::Value::Table(result))
                            })?,
                        )?;

                        // regex_obj:find(subject) -> 返回匹配起止位置
                        regex_obj.set(
                            "find",
                            lua.create_function(move |lua, (_self, subject): (Table, String)| {
                                match re_find.find(&subject) {
                                    Some(m) => {
                                        let result = lua.create_table()?;
                                        // Lua 索引从 1 开始
                                        result.set(1, (m.start() + 1) as i64)?;
                                        result.set(2, m.end() as i64)?;
                                        result.set(3, m.as_str())?;
                                        Ok(mlua::Value::Table(result))
                                    }
                                    None => Ok(mlua::Value::Nil),
                                }
                            })?,
                        )?;

                        Ok(mlua::Value::Table(regex_obj))
                    }
                    Err(e) => Err(mlua::Error::external(format!(
                        "无效的正则表达式 '{}': {}",
                        pattern, e
                    ))),
                }
            })?,
        )?;

        // rex.split(subject, pattern) -> 便捷函数
        rex_table.set(
            "split",
            lua.create_function(
                |lua, (subject, pattern): (String, String)| match regex::Regex::new(&pattern) {
                    Ok(re) => {
                        let result = lua.create_table()?;
                        let parts: Vec<&str> = re.split(&subject).collect();
                        for (i, part) in parts.iter().enumerate() {
                            result.set((i + 1) as i64, *part)?;
                        }
                        Ok(mlua::Value::Table(result))
                    }
                    Err(e) => Err(mlua::Error::external(format!(
                        "无效的正则表达式 '{}': {}",
                        pattern, e
                    ))),
                },
            )?,
        )?;

        // rex.match(subject, pattern) -> 便捷函数
        rex_table.set(
            "match",
            lua.create_function(|lua, (subject, pattern): (String, String)| {
                let pattern = convert_pcre_to_rust_regex(&pattern);
                match regex::Regex::new(&pattern) {
                    Ok(re) => match re.captures(&subject) {
                        Some(caps) => {
                            let result = lua.create_table()?;
                            if let Some(m) = caps.get(0) {
                                result.set(1, m.as_str())?;
                            }
                            for (i, cap) in caps.iter().skip(1).enumerate() {
                                if let Some(c) = cap {
                                    result.set((i + 2) as i64, c.as_str())?;
                                }
                            }
                            Ok(mlua::Value::Table(result))
                        }
                        None => Ok(mlua::Value::Nil),
                    },
                    Err(e) => Err(mlua::Error::external(format!(
                        "无效的正则表达式 '{}': {}",
                        pattern, e
                    ))),
                }
            })?,
        )?;

        // rex.find(subject, pattern) -> 便捷函数
        rex_table.set(
            "find",
            lua.create_function(|lua, (subject, pattern): (String, String)| {
                let pattern = convert_pcre_to_rust_regex(&pattern);
                match regex::Regex::new(&pattern) {
                    Ok(re) => match re.find(&subject) {
                        Some(m) => {
                            let result = lua.create_table()?;
                            result.set(1, (m.start() + 1) as i64)?;
                            result.set(2, m.end() as i64)?;
                            result.set(3, m.as_str())?;
                            Ok(mlua::Value::Table(result))
                        }
                        None => Ok(mlua::Value::Nil),
                    },
                    Err(e) => Err(mlua::Error::external(format!(
                        "无效的正则表达式 '{}': {}",
                        pattern, e
                    ))),
                }
            })?,
        )?;

        globals.set("rex", rex_table)?;

        // ============================================================
        // Lua 兼容性补丁
        // ============================================================

        // table.getn
        {
            let table_mod: Table = globals.get("table")?;
            table_mod.set(
                "getn",
                lua.create_function(|_, t: Table| Ok(t.len().unwrap_or(0)))?,
            )?;
        }

        // table.foreachi
        {
            let table_mod: Table = globals.get("table")?;
            table_mod.set(
                "foreachi",
                lua.create_function(|_, (t, f): (Table, Function)| {
                    let len = t.len().unwrap_or(0);
                    for i in 1..=len {
                        let val: Value = t.get(i).unwrap_or(Value::Nil);
                        match f.call::<()>((i, val)) {
                            Ok(_) => {}
                            Err(e) => {
                                return Err(e);
                            }
                        }
                    }
                    Ok(())
                })?,
            )?;
        }

        // table.foreach
        {
            let table_mod: Table = globals.get("table")?;
            table_mod.set(
                "foreach",
                lua.create_function(|_, (t, f): (Table, Function)| {
                    for pair in t.pairs::<Value, Value>() {
                        let (k, v) = pair?;
                        match f.call::<()>((k, v)) {
                            Ok(_) => {}
                            Err(e) => {
                                return Err(e);
                            }
                        }
                    }
                    Ok(())
                })?,
            )?;
        }

        // math.mod
        {
            let math_mod: Table = globals.get("math")?;
            math_mod.set(
                "mod",
                lua.create_function(|_, (a, b): (f64, f64)| Ok(a % b))?,
            )?;
        }

        // math.pow
        {
            let math_mod: Table = globals.get("math")?;
            math_mod.set(
                "pow",
                lua.create_function(|_, (a, b): (f64, f64)| Ok(a.powf(b)))?,
            )?;
        }

        // ============================================================
        // 原始 API（保留兼容）
        // ============================================================

        // trigger(pattern, callback)
        let state_rc33 = state_rc.clone();
        let trigger_fn =
            lua.create_function_mut(move |_, (pattern, callback): (String, Function)| {
                let pattern = convert_pcre_to_rust_regex(&pattern);
                let trigger_pattern = {
                    let encoding = state_rc33.borrow().current_encoding;
                    match encoding {
                        ScriptEncoding::Gbk => {
                            let gbk_pattern_str = utf8_regex_to_gbk_bytes(&pattern);
                            let gbk_re = BytesRegex::new(&gbk_pattern_str).map_err(|e| {
                                mlua::Error::external(format!(
                                    "无效GBK正则 '{}': {}",
                                    gbk_pattern_str, e
                                ))
                            })?;
                            TriggerPattern::Gbk(gbk_re)
                        }
                        ScriptEncoding::Utf8 => {
                            let re = Regex::new(&pattern).map_err(|e| {
                                mlua::Error::external(format!("无效正则 '{}': {}", pattern, e))
                            })?;
                            TriggerPattern::Utf8(re)
                        }
                    }
                };
                state_rc33.borrow_mut().add_trigger(Trigger {
                    name: String::new(),
                    pattern: trigger_pattern,
                    callback,
                    enabled: true,
                    group: String::new(),
                    sequence: 0,
                    multiline: false,
                    lines_to_match: 1,
                    omit_from_output: false,
                    temporary: false,
                    send_text: String::new(),
                });
                Ok(())
            })?;
        globals.set("trigger", trigger_fn)?;

        // alias(pattern, callback)
        let state_rc34 = state_rc.clone();
        let alias_fn =
            lua.create_function_mut(move |_, (pattern, callback): (String, Function)| {
                let pattern = convert_pcre_to_rust_regex(&pattern);
                let re = Regex::new(&pattern)
                    .map_err(|e| mlua::Error::external(format!("无效正则 '{}': {}", pattern, e)))?;
                state_rc34.borrow_mut().add_alias(Alias {
                    name: String::new(),
                    match_text: pattern.clone(),
                    pattern: re,
                    callback,
                    enabled: true,
                    group: String::new(),
                    send_to: 0,
                    response: String::new(),
                    sequence: 0,
                });
                Ok(())
            })?;
        globals.set("alias", alias_fn)?;

        // timer(interval, callback)
        let state_rc35 = state_rc.clone();
        let timer_fn =
            lua.create_function_mut(move |_, (interval_secs, callback): (u64, Function)| {
                state_rc35.borrow_mut().add_timer(TimerDef {
                    name: String::new(),
                    interval_millis: interval_secs * 1000,
                    callback: Some(callback),
                    enabled: true,
                    group: String::new(),
                    one_shot: false,
                    at_time: false,
                    send_text: String::new(),
                    next_fire: std::time::Instant::now()
                        + std::time::Duration::from_millis(interval_secs * 1000),
                });
                Ok(())
            })?;
        globals.set("timer", timer_fn)?;

        // get(key)
        let state_rc36 = state_rc.clone();
        let get_fn = lua.create_function_mut(move |_, key: String| {
            let state = state_rc36.borrow();
            Ok(state.variables.get(&key).cloned().unwrap_or_default())
        })?;
        globals.set("get", get_fn)?;

        // set(key, value)
        let state_rc37 = state_rc.clone();
        let set_fn = lua.create_function_mut(move |_, (key, value): (String, String)| {
            state_rc37.borrow_mut().variables.insert(key, value);
            Ok(())
        })?;
        globals.set("set", set_fn)?;

        Ok(())
    }
}

/// 添加触发器的通用实现
#[allow(clippy::too_many_arguments)]
pub(super) fn add_trigger_impl(
    lua: &mlua::Lua,
    state_rc: &std::rc::Rc<std::cell::RefCell<ScriptState>>,
    name: &str,
    match_str: &str,
    flags: i64,
    script: &str,
    _send_to: i64,
    sequence: i32,
) -> LuaResult<Value> {
    let case_insensitive = (flags & 16) != 0;
    let is_regex = (flags & 32) != 0;
    let do_replace = (flags & 1024) != 0;

    let re_str = if is_regex {
        // 正则模式：先做 PCRE 兼容转换
        convert_pcre_to_rust_regex(match_str)
    } else {
        // 通配符模式：不需要 PCRE 转换，直接转义
        regex_escape(match_str)
            .replace('*', "(.*)")
            .replace('?', "(.)")
    };

    let re_str = if case_insensitive {
        format!("(?i){}", re_str)
    } else {
        re_str
    };

    // 根据当前脚本编码选择匹配模式
    let trigger_pattern = {
        let encoding = state_rc.borrow().current_encoding;
        match encoding {
            ScriptEncoding::Gbk => {
                // GBK 模式：将正则转为 GBK 字节正则，.{4} 匹配4字节
                let gbk_pattern_str = utf8_regex_to_gbk_bytes(&re_str);
                let gbk_re = BytesRegex::new(&gbk_pattern_str).map_err(|e| {
                    mlua::Error::external(format!("无效GBK正则 '{}': {}", gbk_pattern_str, e))
                })?;
                TriggerPattern::Gbk(gbk_re)
            }
            ScriptEncoding::Utf8 => {
                // UTF-8 模式：按 Unicode 字符匹配，.{4} 匹配4个字符
                let re = Regex::new(&re_str)
                    .map_err(|e| mlua::Error::external(format!("无效正则 '{}': {}", re_str, e)))?;
                TriggerPattern::Utf8(re)
            }
        }
    };

    let callback: Function = if script.is_empty() {
        lua.create_function(|_, _: ()| Ok(()))?
    } else {
        // script 可能是 "function(...) ... end" 或 "return function(...) ... end" 或函数名
        let code = format!("return {}", script);
        match lua.load(&code).eval::<Function>() {
            Ok(f) => f,
            Err(_) => {
                // 如果 "return ..." 失败，尝试直接执行（可能是函数名引用）
                lua.load(script).eval()?
            }
        }
    };

    let new_trigger = Trigger {
        name: name.to_string(),
        pattern: trigger_pattern,
        callback,
        enabled: (flags & 1) != 0,
        group: String::new(),
        sequence,
        multiline: false,
        lines_to_match: 1,
        omit_from_output: (flags & 4) != 0,
        temporary: (flags & 4096) != 0,
        send_text: String::new(),
    };

    let mut state = state_rc.borrow_mut();
    if do_replace {
        state.delete_trigger(name);
        state.add_trigger(new_trigger);
    } else {
        state.add_trigger(new_trigger);
    }

    Ok(Value::Integer(0))
}
