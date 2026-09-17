//! 输出 / ANSI 样式 / 日志 API 注册
//!
//! 对应拆分前 `api.rs` 中的「输出」「ANSI 样式 API」「日志 API」三个分节。

use mlua::{Result as LuaResult, Value};

use crate::lua::helpers::{colour_to_ansi_bg, colour_to_ansi_fg};
use crate::lua::types::{LuaEngine, PanelUpdate};
use crate::ui::terminal::PanelButtonDef;

impl LuaEngine {
    pub(super) fn register_output_api(&mut self) -> LuaResult<()> {
        let lua = &self.lua;
        let globals = lua.globals();
        let state_rc = self.state.clone();

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

        Ok(())
    }

    pub(super) fn register_ansi_api(&mut self) -> LuaResult<()> {
        let lua = &self.lua;
        let globals = lua.globals();

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

        Ok(())
    }

    pub(super) fn register_log_api(&mut self) -> LuaResult<()> {
        let lua = &self.lua;
        let globals = lua.globals();

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

        Ok(())
    }
}
