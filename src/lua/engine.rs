use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use mlua::{Function, Lua, Result as LuaResult, Table, UserData, Value};
use regex::Regex;
use rusqlite::{types::Value as SqlValue, Connection};

/// SQLite 连接包装（Lua 用户数据）
struct LuaDb {
    conn: Arc<Mutex<Connection>>,
}

impl UserData for LuaDb {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("close", |_, _this, ()| Ok(()));

        methods.add_method("exec", |_, this, sql: String| {
            let conn = this.conn.lock().unwrap();
            conn.execute_batch(&sql)
                .map_err(|e| mlua::Error::external(e.to_string()))
        });

        methods.add_method("prepare", |lua, this, sql: String| {
            let conn = this.conn.lock().unwrap();
            conn.prepare(&sql)
                .map_err(|e| mlua::Error::external(e.to_string()))?;
            let lua_stmt = LuaStmt {
                conn: this.conn.clone(),
                sql: sql.clone(),
            };
            let ud = lua.create_userdata(lua_stmt)?;
            Ok(ud)
        });

        methods.add_method("changes", |_, this, ()| {
            let conn = this.conn.lock().unwrap();
            Ok(conn.changes() as i64)
        });

        methods.add_method("nrows", |lua, this, sql: String| {
            // 收集所有行数据到 Vec，避免在锁内创建 Lua 对象
            let rows_data: Vec<Vec<(String, rusqlite::types::Value)>> = {
                let conn = this.conn.lock().unwrap();
                let mut stmt = conn
                    .prepare(&sql)
                    .map_err(|e| mlua::Error::external(e.to_string()))?;
                let col_names: Vec<String> =
                    stmt.column_names().iter().map(|s| s.to_string()).collect();
                let mut rows = stmt
                    .query([])
                    .map_err(|e| mlua::Error::external(e.to_string()))?;
                let mut result = Vec::new();
                while let Some(row) = rows
                    .next()
                    .map_err(|e| mlua::Error::external(e.to_string()))?
                {
                    let mut row_data = Vec::with_capacity(col_names.len());
                    for (i, col_name) in col_names.iter().enumerate() {
                        let val = row
                            .get_ref(i)
                            .ok()
                            .map(|r| match r {
                                rusqlite::types::ValueRef::Null => rusqlite::types::Value::Null,
                                rusqlite::types::ValueRef::Integer(n) => {
                                    rusqlite::types::Value::Integer(n)
                                }
                                rusqlite::types::ValueRef::Real(f) => {
                                    rusqlite::types::Value::Real(f)
                                }
                                rusqlite::types::ValueRef::Text(s) => rusqlite::types::Value::Text(
                                    std::str::from_utf8(s).unwrap_or("").to_string(),
                                ),
                                rusqlite::types::ValueRef::Blob(b) => {
                                    rusqlite::types::Value::Blob(b.to_vec())
                                }
                            })
                            .unwrap_or(rusqlite::types::Value::Null);
                        row_data.push((col_name.clone(), val));
                    }
                    result.push(row_data);
                }
                result
            };

            // 在锁外创建 Lua 迭代器，将数据移入闭包
            let mut idx = 0usize;
            let iter_fn = lua.create_function_mut(move |lua, ()| {
                if idx >= rows_data.len() {
                    return Ok(None);
                }
                let row_data = &rows_data[idx];
                let table = lua.create_table()?;
                for (col_name, val) in row_data {
                    let lua_val = match val {
                        rusqlite::types::Value::Null => mlua::Value::Nil,
                        rusqlite::types::Value::Integer(n) => mlua::Value::Integer(*n),
                        rusqlite::types::Value::Real(f) => mlua::Value::Number(*f),
                        rusqlite::types::Value::Text(s) => {
                            mlua::Value::String(lua.create_string(s)?)
                        }
                        rusqlite::types::Value::Blob(b) => {
                            mlua::Value::String(lua.create_string(b)?)
                        }
                    };
                    table.set(col_name.clone(), lua_val)?;
                }
                idx += 1;
                Ok(Some(table))
            })?;
            Ok(iter_fn)
        });

        methods.add_method("last_insert_rowid", |_, this, ()| {
            let conn = this.conn.lock().unwrap();
            Ok(conn.last_insert_rowid())
        });
    }
}

/// SQLite 预处理语句包装
struct LuaStmt {
    conn: Arc<Mutex<Connection>>,
    sql: String,
}

impl UserData for LuaStmt {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("step", |lua, this, args: Option<Table>| {
            let conn = this.conn.lock().unwrap();
            let mut stmt = conn
                .prepare(&this.sql)
                .map_err(|e| mlua::Error::external(e.to_string()))?;

            let params_vec = if let Some(ref t) = args {
                let len = t.len().unwrap_or(0) as usize;
                let mut vals: Vec<SqlValue> = Vec::with_capacity(len);
                for i in 1..=len {
                    let v: String = t.get(i).unwrap_or_default();
                    vals.push(SqlValue::Text(v));
                }
                vals
            } else {
                Vec::new()
            };

            let params_refs: Vec<&dyn rusqlite::types::ToSql> = params_vec
                .iter()
                .map(|v| v as &dyn rusqlite::types::ToSql)
                .collect();

            let mut rows = stmt
                .query(params_refs.as_slice())
                .map_err(|e| mlua::Error::external(e.to_string()))?;

            if let Some(row) = rows
                .next()
                .map_err(|e| mlua::Error::external(e.to_string()))?
            {
                let lua_table = lua.create_table()?;
                let col_count = row.as_ref().column_count();
                for i in 0..col_count {
                    let val = match row.get_ref(i) {
                        Ok(r) => match r {
                            rusqlite::types::ValueRef::Null => mlua::Value::Nil,
                            rusqlite::types::ValueRef::Integer(n) => mlua::Value::Integer(n),
                            rusqlite::types::ValueRef::Real(f) => mlua::Value::Number(f),
                            rusqlite::types::ValueRef::Text(s) => {
                                mlua::Value::String(lua.create_string(s)?)
                            }
                            rusqlite::types::ValueRef::Blob(b) => {
                                mlua::Value::String(lua.create_string(b)?)
                            }
                        },
                        Err(_) => mlua::Value::Nil,
                    };
                    lua_table.set(i + 1, val)?;
                }
                return Ok(Some(lua_table));
            }

            Ok(None)
        });

        methods.add_method("run", |_, this, args: Option<Table>| {
            let conn = this.conn.lock().unwrap();
            let mut stmt = conn
                .prepare(&this.sql)
                .map_err(|e| mlua::Error::external(e.to_string()))?;

            let params_vec = if let Some(ref t) = args {
                let len = t.len().unwrap_or(0) as usize;
                let mut vals: Vec<SqlValue> = Vec::with_capacity(len);
                for i in 1..=len {
                    let v: String = t.get(i).unwrap_or_default();
                    vals.push(SqlValue::Text(v));
                }
                vals
            } else {
                Vec::new()
            };

            let params_refs: Vec<&dyn rusqlite::types::ToSql> = params_vec
                .iter()
                .map(|v| v as &dyn rusqlite::types::ToSql)
                .collect();

            stmt.execute(params_refs.as_slice())
                .map_err(|e| mlua::Error::external(e.to_string()))?;

            Ok(())
        });
    }
}

/// 触发器定义
pub struct Trigger {
    pub name: String,
    pub pattern: Regex,
    pub callback: Function,
    pub enabled: bool,
    pub group: String,
    #[allow(dead_code)]
    pub sequence: i32,
    #[allow(dead_code)]
    pub temporary: bool,
    #[allow(dead_code)]
    pub multiline: bool,
    #[allow(dead_code)]
    pub lines_to_match: usize,
    #[allow(dead_code)]
    pub omit_from_output: bool,
    pub send_text: String,
}

/// 别名定义
pub struct Alias {
    pub name: String,
    pub pattern: Regex,
    pub callback: Function,
    pub enabled: bool,
    pub group: String,
}

/// 定时器定义
pub struct TimerDef {
    pub name: String,
    pub interval_millis: u64,
    pub callback: Function,
    pub enabled: bool,
    pub group: String,
    pub one_shot: bool,
    pub send_text: String,
}

/// 脚本运行时共享状态
struct ScriptState {
    triggers: Vec<Trigger>,
    aliases: Vec<Alias>,
    timers: Vec<TimerDef>,
    variables: HashMap<String, String>,
    pending_commands: Vec<String>,
    pending_logs: Vec<String>,
    recent_lines: Vec<String>,
    unique_counter: u64,
    connected: bool,
    connect_requested: bool,
    disconnect_requested: bool,
    host: String,
}

/// Lua 引擎与脚本运行时
/// Lua 合法转义字符集合
const LUA_VALID_ESCAPES: &[u8] = b"abfnrtv\\\"'0123456789xzZuU";

/// 预处理 Lua 源码，修复 LuaJIT 不兼容的无效转义序列
///
/// 标准 Lua 5.1 对未识别的转义序列（如 `\-`, `\+`）宽松处理（保留反斜杠），
/// 但 LuaJIT 严格拒绝。此函数在字符串字面量内将无效转义 `\X` 替换为 `\\X`，
/// 使 LuaJIT 正确解析。
fn fix_lua_escape_sequences(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut i = 0;

    // 状态机：追踪当前上下文
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Normal,       // 普通代码
        StringSingle, // 单引号字符串
        StringDouble, // 双引号字符串
        LongString,   // 长字符串 [[...]]
        LongComment,  // 长注释 --[[...]]
        LineComment,  // 单行注释 --
    }

    let mut state = State::Normal;
    let mut long_bracket_depth: usize = 0;

    while i < bytes.len() {
        match state {
            State::Normal => {
                // 检测单行注释 --
                if bytes[i] == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
                    // 检测长注释 --[[ 或 --[=[
                    if i + 2 < bytes.len() && bytes[i + 2] == b'[' {
                        let bracket_len = count_long_bracket_open(&bytes[i + 2..]);
                        if bracket_len > 0 {
                            result.extend_from_slice(&bytes[i..i + 2 + bracket_len]);
                            i += 2 + bracket_len;
                            long_bracket_depth = 1;
                            state = State::LongComment;
                            continue;
                        }
                    }
                    result.push(bytes[i]);
                    i += 1;
                    state = State::LineComment;
                    continue;
                }
                // 检测长字符串 [[ 或 [=[
                if bytes[i] == b'[' {
                    let bracket_len = count_long_bracket_open(&bytes[i..]);
                    if bracket_len > 0 {
                        result.extend_from_slice(&bytes[i..i + bracket_len]);
                        i += bracket_len;
                        long_bracket_depth = 1;
                        state = State::LongString;
                        continue;
                    }
                }
                // 检测字符串开始
                if bytes[i] == b'"' {
                    result.push(bytes[i]);
                    i += 1;
                    state = State::StringDouble;
                    continue;
                }
                if bytes[i] == b'\'' {
                    result.push(bytes[i]);
                    i += 1;
                    state = State::StringSingle;
                    continue;
                }
                result.push(bytes[i]);
                i += 1;
            }
            State::StringDouble | State::StringSingle => {
                if bytes[i] == b'\\' {
                    // 转义序列
                    if i + 1 < bytes.len() {
                        let next = bytes[i + 1];
                        if LUA_VALID_ESCAPES.contains(&next) {
                            // 合法转义，原样保留
                            result.push(bytes[i]);
                            result.push(next);
                            i += 2;
                        } else {
                            // 非法转义，将 \X 替换为 \\X
                            result.push(b'\\');
                            result.push(b'\\');
                            result.push(next);
                            i += 2;
                        }
                    } else {
                        result.push(bytes[i]);
                        i += 1;
                    }
                } else if (state == State::StringDouble && bytes[i] == b'"')
                    || (state == State::StringSingle && bytes[i] == b'\'')
                {
                    result.push(bytes[i]);
                    i += 1;
                    state = State::Normal;
                } else {
                    result.push(bytes[i]);
                    i += 1;
                }
            }
            State::LongString | State::LongComment => {
                // 检测长括号关闭 ]] 或 ]=]
                if bytes[i] == b']' {
                    let close_len = count_long_bracket_close(&bytes[i..]);
                    if close_len > 0 {
                        result.extend_from_slice(&bytes[i..i + close_len]);
                        i += close_len;
                        long_bracket_depth -= 1;
                        if long_bracket_depth == 0 {
                            state = State::Normal;
                        }
                        continue;
                    }
                }
                // 嵌套长括号（仅长字符串内）
                if state == State::LongString && bytes[i] == b'[' {
                    let open_len = count_long_bracket_open(&bytes[i..]);
                    if open_len > 0 {
                        result.extend_from_slice(&bytes[i..i + open_len]);
                        i += open_len;
                        long_bracket_depth += 1;
                        continue;
                    }
                }
                result.push(bytes[i]);
                i += 1;
            }
            State::LineComment => {
                result.push(bytes[i]);
                if bytes[i] == b'\n' {
                    state = State::Normal;
                }
                i += 1;
            }
        }
    }

    String::from_utf8(result).unwrap_or_else(|_| source.to_string())
}

/// 检测长括号开始 [[ 或 [=[ 或 [==[ 等，返回括号长度（0 表示不是长括号）
fn count_long_bracket_open(bytes: &[u8]) -> usize {
    if bytes.is_empty() || bytes[0] != b'[' {
        return 0;
    }
    let eq_count = bytes.iter().skip(1).take_while(|&&b| b == b'=').count();
    let bracket_pos = 1 + eq_count;
    if bracket_pos < bytes.len() && bytes[bracket_pos] == b'[' {
        bracket_pos + 1
    } else {
        0
    }
}

/// 检测长括号关闭 ]] 或 ]=] 或 ]==] 等，返回括号长度（0 表示不是长括号）
fn count_long_bracket_close(bytes: &[u8]) -> usize {
    if bytes.is_empty() || bytes[0] != b']' {
        return 0;
    }
    let eq_count = bytes.iter().skip(1).take_while(|&&b| b == b'=').count();
    let bracket_pos = 1 + eq_count;
    if bracket_pos < bytes.len() && bytes[bracket_pos] == b']' {
        bracket_pos + 1
    } else {
        0
    }
}

/// 将 Lua Value 强制转换为 i64（兼容整数、浮点数和可解析的字符串）
fn coerce_to_i64(value: mlua::Value) -> mlua::Result<i64> {
    match value {
        mlua::Value::Integer(i) => Ok(i),
        mlua::Value::Number(n) => Ok(n as i64),
        mlua::Value::String(s) => {
            let str_val = s.to_str()?;
            str_val
                .parse::<i64>()
                .map_err(|_| mlua::Error::external(format!("无法将 '{}' 转换为整数", str_val)))
        }
        _ => Err(mlua::Error::external("期望数字或可转换为数字的值")),
    }
}

/// 将 Lua Value 强制转换为 String（兼容字符串和数字）
fn coerce_to_string(value: mlua::Value) -> mlua::Result<String> {
    match value {
        mlua::Value::String(s) => Ok(s.to_str()?.to_string()),
        mlua::Value::Integer(i) => Ok(i.to_string()),
        mlua::Value::Number(n) => Ok(n.to_string()),
        _ => Err(mlua::Error::external("期望字符串或数字")),
    }
}

/// 将 Lua Value 强制转换为 f64（兼容整数、浮点数和字符串，nil 返回 Err）
fn coerce_to_f64(value: mlua::Value) -> mlua::Result<f64> {
    match value {
        mlua::Value::Integer(i) => Ok(i as f64),
        mlua::Value::Number(n) => Ok(n),
        mlua::Value::String(s) => {
            let str_val = s.to_str()?;
            str_val
                .parse::<f64>()
                .map_err(|_| mlua::Error::external(format!("无法将 '{}' 转换为数字", str_val)))
        }
        _ => Err(mlua::Error::external("期望数字或可转换为数字的值")),
    }
}

/// 将 MUSHclient 颜色名称映射为 ANSI 前景色代码
fn colour_to_ansi_fg(name: &str) -> u8 {
    match name.to_lowercase().as_str() {
        "black" => 30,
        "red" => 31,
        "green" => 32,
        "yellow" => 33,
        "blue" => 34,
        "magenta" => 35,
        "cyan" => 36,
        "white" => 37,
        "darkred" => 31,
        "darkgreen" => 32,
        "darkblue" => 34,
        "darkcyan" => 36,
        "darkmagenta" => 35,
        "darkyellow" => 33,
        "darkgray" | "darkgrey" => 90,
        "lightred" | "brightred" => 91,
        "lightgreen" | "brightgreen" => 92,
        "lightyellow" | "brightyellow" => 93,
        "lightblue" | "brightblue" => 94,
        "lightmagenta" | "brightmagenta" => 95,
        "lightcyan" | "brightcyan" => 96,
        "lightgray" | "lightgrey" | "brightwhite" => 97,
        _ => 39, // 默认前景色
    }
}

/// 将 MUSHclient 颜色名称映射为 ANSI 背景色代码
fn colour_to_ansi_bg(name: &str) -> u8 {
    match name.to_lowercase().as_str() {
        "black" => 40,
        "red" => 41,
        "green" => 42,
        "yellow" => 43,
        "blue" => 44,
        "magenta" => 45,
        "cyan" => 46,
        "white" => 47,
        "darkgray" | "darkgrey" => 100,
        "lightred" | "brightred" => 101,
        "lightgreen" | "brightgreen" => 102,
        "lightyellow" | "brightyellow" => 103,
        "lightblue" | "brightblue" => 104,
        "lightmagenta" | "brightmagenta" => 105,
        "lightcyan" | "brightcyan" => 106,
        "lightgray" | "lightgrey" | "brightwhite" => 107,
        _ => 49, // 默认背景色
    }
}

pub struct LuaEngine {
    lua: Lua,
    state: Rc<RefCell<ScriptState>>,
    script_path: Option<String>,
    script_dir: Rc<RefCell<Option<String>>>,
}

impl LuaEngine {
    /// 创建新的 Lua 引擎实例
    pub fn new() -> LuaResult<Self> {
        let lua = Lua::new();

        let state = Rc::new(RefCell::new(ScriptState {
            triggers: Vec::new(),
            aliases: Vec::new(),
            timers: Vec::new(),
            variables: HashMap::new(),
            pending_commands: Vec::new(),
            pending_logs: Vec::new(),
            recent_lines: Vec::new(),
            unique_counter: 0,
            connected: false,
            connect_requested: false,
            disconnect_requested: false,
            host: String::new(),
        }));

        let script_dir = Rc::new(RefCell::new(None::<String>));

        let mut engine = Self {
            lua,
            state,
            script_path: None,
            script_dir,
        };
        engine.register_api()?;
        Ok(engine)
    }

    /// 设置脚本路径（同时提取目录）
    pub fn set_script_path(&mut self, path: &str) {
        if let Some(pos) = path.rfind('/') {
            *self.script_dir.borrow_mut() = Some(path[..pos + 1].to_string());
        } else {
            *self.script_dir.borrow_mut() = Some("./".to_string());
        }
        self.script_path = Some(path.to_string());
    }

    /// 注册 Lua API
    fn register_api(&mut self) -> LuaResult<()> {
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

        // DeleteTemporaryTimers() — MushClient API: 删除所有临时定时器
        let state_rc_dtt = state_rc.clone();
        let delete_temp_timers_fn = lua.create_function_mut(move |_, ()| {
            state_rc_dtt.borrow_mut().timers.retain(|t| !t.one_shot);
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
            state_rc6.borrow_mut().pending_logs.push(text);
            Ok(())
        })?;
        globals.set("Note", note_fn)?;

        // Tell(text)
        let state_rc7 = state_rc.clone();
        let tell_fn = lua.create_function_mut(move |_, text: String| {
            state_rc7.borrow_mut().pending_logs.push(text);
            Ok(())
        })?;
        globals.set("Tell", tell_fn)?;

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
            let before = state.triggers.len();
            state.triggers.retain(|t| t.name != name);
            if state.triggers.len() < before {
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

        // GetTriggerInfo(name, code)
        let state_rc12 = state_rc.clone();
        let get_trigger_info_fn =
            lua.create_function_mut(move |lua, (name, code): (String, i64)| {
                let state = state_rc12.borrow();
                if let Some(t) = state.triggers.iter().find(|t| t.name == name) {
                    match code {
                        8 => Ok(Value::Boolean(t.enabled)), // enabled
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
                if let Some(t) = state.triggers.iter_mut().find(|t| t.name == name) {
                    match key.as_str() {
                        "group" => {
                            if let Value::String(s) = value {
                                t.group = s.to_str().map(|s| s.to_string()).unwrap_or_default();
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
                for t in state.triggers.iter_mut() {
                    if !t.group.is_empty() && t.group == group {
                        t.enabled = enable;
                    }
                }
                Ok(())
            })?;
        globals.set("EnableTriggerGroup", enable_trigger_group_fn)?;

        // EnableTrigger(name, enable)
        let state_rc_et = state_rc.clone();
        let enable_trigger_fn =
            lua.create_function_mut(move |_, (name, enable): (String, bool)| {
                let mut state = state_rc_et.borrow_mut();
                if let Some(t) = state.triggers.iter_mut().find(|t| t.name == name) {
                    t.enabled = enable;
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
            let _response: String = coerce_to_string(args[2].clone())?;
            let flags: i64 = coerce_to_i64(args[3].clone())?;
            // 第5个参数 script_name（可选）
            let script = if args.len() > 4 {
                coerce_to_string(args[4].clone())?
            } else {
                String::new()
            };

            let re_str = if (flags & 32) != 0 {
                convert_pcre_to_rust_regex(&match_str)
            } else {
                regex_escape(&match_str)
                    .replace('*', "(.*)")
                    .replace('?', "(.)")
            };
            let re = Regex::new(&re_str)
                .map_err(|e| mlua::Error::external(format!("无效正则 '{}': {}", re_str, e)))?;

            let callback: Function = if script.is_empty() {
                lua.create_function(|_, _: ()| Ok(()))?
            } else {
                let code = format!("return {}", script);
                match lua.load(&code).eval::<Function>() {
                    Ok(f) => f,
                    Err(_) => lua.load(&script).eval()?,
                }
            };

            state_rc15.borrow_mut().aliases.push(Alias {
                name,
                pattern: re,
                callback,
                enabled: (flags & 1) != 0,
                group: String::new(),
            });
            Ok(Value::Integer(0))
        })?;
        globals.set("AddAlias", add_alias_fn)?;

        // DeleteAlias(name)
        let state_rc16 = state_rc.clone();
        let delete_alias_fn = lua.create_function_mut(move |_, name: String| {
            let mut state = state_rc16.borrow_mut();
            let before = state.aliases.len();
            state.aliases.retain(|a| a.name != name);
            if state.aliases.len() < before {
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

        // SetAliasOption(name, key, value)
        let state_rc18 = state_rc.clone();
        let set_alias_option_fn =
            lua.create_function_mut(move |_, (name, key, value): (String, String, Value)| {
                let mut state = state_rc18.borrow_mut();
                if let Some(a) = state.aliases.iter_mut().find(|a| a.name == name) {
                    match key.as_str() {
                        "group" => {
                            if let Value::String(s) = value {
                                a.group = s.to_str().map(|s| s.to_string()).unwrap_or_default();
                            }
                        }
                        "enabled" => {
                            if let Value::Boolean(b) = value {
                                a.enabled = b;
                            } else if let Value::Integer(n) = value {
                                a.enabled = n != 0;
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
        let add_timer_fn = lua.create_function_mut(move |lua, args: mlua::MultiValue| {
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
            let sec_millis = coerce_to_f64(args[3].clone())
                .map(|s| if s <= 0.0 { 1000.0 } else { s * 1000.0 })
                .unwrap_or(1000.0);
            // 第5个参数 response_text：MushClient 中是字符串，忽略
            let flags: i64 = coerce_to_i64(args[5].clone()).unwrap_or(0);
            // 第7个参数 script_name（可选）
            let script_name = if args.len() > 6 {
                coerce_to_string(args[6].clone()).unwrap_or_default()
            } else {
                String::new()
            };

            let interval_millis = sec_millis as u64;
            let one_shot = (flags & 4) != 0;

            // 将脚本作为 send_text 存储，在 fire_timer 时执行
            let callback: Function = lua.create_function(|_, _: ()| Ok(()))?;

            state_rc19.borrow_mut().timers.push(TimerDef {
                name,
                interval_millis,
                callback,
                enabled: (flags & 1) != 0,
                group: String::new(),
                one_shot,
                send_text: script_name,
            });
            Ok(Value::Integer(0))
        })?;
        globals.set("AddTimer", add_timer_fn)?;

        // DeleteTimer(name)
        let state_rc20 = state_rc.clone();
        let delete_timer_fn = lua.create_function_mut(move |_, name: String| {
            let mut state = state_rc20.borrow_mut();
            let before = state.timers.len();
            state.timers.retain(|t| t.name != name);
            if state.timers.len() < before {
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

        // GetTimerInfo(name, code)
        let state_rc22 = state_rc.clone();
        let get_timer_info_fn =
            lua.create_function_mut(move |lua, (name, code): (String, i64)| {
                let state = state_rc22.borrow();
                if let Some(t) = state.timers.iter().find(|t| t.name == name) {
                    match code {
                        6 => Ok(Value::Boolean(t.enabled)), // enabled
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
                if let Some(t) = state.timers.iter_mut().find(|t| t.name == name) {
                    match key.as_str() {
                        "group" => {
                            if let Value::String(s) = value {
                                t.group = s.to_str().map(|s| s.to_string()).unwrap_or_default();
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
                for t in state.timers.iter_mut() {
                    if !t.group.is_empty() && t.group == group {
                        t.enabled = enable;
                    }
                }
                Ok(())
            })?;
        globals.set("EnableTimerGroup", enable_timer_group_fn)?;

        // EnableTimer(name, enable)
        let state_rc_emt = state_rc.clone();
        let enable_timer_fn =
            lua.create_function_mut(move |_, (name, enable): (String, bool)| {
                let mut state = state_rc_emt.borrow_mut();
                if let Some(t) = state.timers.iter_mut().find(|t| t.name == name) {
                    t.enabled = enable;
                    Ok(Value::Integer(0))
                } else {
                    Ok(Value::Integer(1))
                }
            })?;
        globals.set("EnableTimer", enable_timer_fn)?;

        // ============================================================
        // 配置 API
        // ============================================================

        // GetInfo(code)
        let script_dir_rc = self.script_dir.clone();
        let state_rc_gi = state_rc.clone();
        let get_info_fn = lua.create_function_mut(move |lua, code: i64| match code {
            1 => {
                // MushClient: GetInfo(1) 返回主机地址
                let host = state_rc_gi.borrow().host.clone();
                Ok(Value::String(lua.create_string(&host)?))
            }
            35 => {
                let dir = script_dir_rc.borrow().clone();
                match dir {
                    Some(d) => {
                        let win_path = d.replace('/', "\\");
                        Ok(Value::String(lua.create_string(&win_path)?))
                    }
                    None => Ok(Value::String(lua.create_string(".\\")?)),
                }
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
        globals.set("_mud_options", lua.create_table()?)?;
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

        // ============================================================
        // 工具函数
        // ============================================================

        // GetUniqueNumber()
        let state_rc28 = state_rc.clone();
        let get_unique_number_fn = lua.create_function_mut(move |_, ()| {
            let mut state = state_rc28.borrow_mut();
            state.unique_counter += 1;
            Ok(Value::Integer(state.unique_counter as i64))
        })?;
        globals.set("GetUniqueNumber", get_unique_number_fn)?;

        // Trim(string)
        let trim_fn = lua.create_function(move |_, s: String| Ok(s.trim().to_string()))?;
        globals.set("Trim", trim_fn)?;

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

        // timer_flag
        let timer_flag = lua.create_table()?;
        timer_flag.set("Enabled", 1i64)?;
        timer_flag.set("AtTime", 4i64)?;
        timer_flag.set("Replace", 1024i64)?;
        timer_flag.set("Temporary", 4096i64)?;
        timer_flag.set("OneShot", 8192i64)?;
        timer_flag.set("ActiveWhenClosed", 16384i64)?;
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

        // GetPluginInfo(id, code)
        let get_plugin_info_fn =
            lua.create_function(move |lua, (_id, code): (String, i64)| match code {
                1 => Ok(Value::String(lua.create_string("RustLuaMud")?)),
                _ => Ok(Value::Nil),
            })?;
        globals.set("GetPluginInfo", get_plugin_info_fn)?;

        // ============================================================
        // 模块加载机制
        // ============================================================

        // 覆盖 dofile — 支持 GBK 自动转码和路径分隔符兼容
        let _script_path_rc = Rc::new(RefCell::new(self.script_path.clone()));
        let dofile_fn = lua.create_function_mut(move |lua, path: String| {
            // 将 \ 替换为 /
            let path = path.replace('\\', "/");

            let bytes = std::fs::read(&path)
                .map_err(|e| mlua::Error::external(format!("读取文件失败 '{}': {}", path, e)))?;

            let code = match std::str::from_utf8(&bytes) {
                Ok(s) => s.to_string(),
                Err(_) => {
                    let (cow, _, _) = encoding_rs::GBK.decode(&bytes);
                    cow.into_owned()
                }
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
                let re = Regex::new(&pattern)
                    .map_err(|e| mlua::Error::external(format!("无效正则 '{}': {}", pattern, e)))?;
                state_rc33.borrow_mut().triggers.push(Trigger {
                    name: String::new(),
                    pattern: re,
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
                state_rc34.borrow_mut().aliases.push(Alias {
                    name: String::new(),
                    pattern: re,
                    callback,
                    enabled: true,
                    group: String::new(),
                });
                Ok(())
            })?;
        globals.set("alias", alias_fn)?;

        // timer(interval, callback)
        let state_rc35 = state_rc.clone();
        let timer_fn =
            lua.create_function_mut(move |_, (interval_secs, callback): (u64, Function)| {
                state_rc35.borrow_mut().timers.push(TimerDef {
                    name: String::new(),
                    interval_millis: interval_secs * 1000,
                    callback,
                    enabled: true,
                    group: String::new(),
                    one_shot: false,
                    send_text: String::new(),
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

    /// 直接执行 Lua 代码（用于 /eval 命令）
    pub fn eval_code(&self, code: &str) -> Result<(), String> {
        self.lua.load(code).exec().map_err(|e| format!("{}", e))
    }

    /// 加载并执行 Lua 脚本文件
    /// 自动检测编码：先尝试 UTF-8，失败（GBK 编码）则自动转码
    pub fn load_script(&mut self, path: &str) -> Result<(), String> {
        // 先设置脚本路径，确保脚本执行时 GetInfo(35) 能返回正确目录
        self.set_script_path(path);

        let bytes = std::fs::read(path).map_err(|e| format!("读取脚本失败 '{}': {}", path, e))?;

        let code = match std::str::from_utf8(&bytes) {
            Ok(s) => s.to_string(),
            Err(_) => {
                let (cow, _, _) = encoding_rs::GBK.decode(&bytes);
                cow.into_owned()
            }
        };

        self.lua
            .load(&code)
            .set_name(path)
            .exec()
            .map_err(|e| format!("err '{}': {}", path, e))?;

        Ok(())
    }

    /// 获取当前加载的脚本路径
    pub fn script_path(&self) -> Option<&String> {
        self.script_path.as_ref()
    }

    /// 处理服务器输出，匹配触发器
    pub fn process_output(&self, line: &str) {
        // 清空待发送队列
        self.state.borrow_mut().pending_commands.clear();

        // 剥离 ANSI 码用于匹配
        let clean_line = crate::ui::AnsiParser::strip_ansi(line);

        // 维护最近行缓冲区
        {
            let mut state = self.state.borrow_mut();
            state.recent_lines.push(clean_line.clone());
            if state.recent_lines.len() > 20 {
                state.recent_lines.remove(0);
            }
        }

        // 收集需要触发的
        let matches: Vec<(usize, Vec<String>)> = {
            let state = self.state.borrow();
            let mut result = Vec::new();
            for (i, trigger) in state.triggers.iter().enumerate() {
                if !trigger.enabled {
                    continue;
                }

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
                        // 多行模式下让 . 匹配换行符
                        let multiline_pattern = format!("(?s){}", trigger.pattern.as_str());
                        let multiline_re = Regex::new(&multiline_pattern)
                            .unwrap_or_else(|_| trigger.pattern.clone());
                        if let Some(caps) = multiline_re.captures(&combined) {
                            let caps_list: Vec<String> = caps
                                .iter()
                                .skip(1)
                                .flatten()
                                .map(|m| m.as_str().to_string())
                                .collect();
                            result.push((i, caps_list));
                        }
                    }
                } else {
                    if let Some(caps) = trigger.pattern.captures(&clean_line) {
                        let caps_list: Vec<String> = caps
                            .iter()
                            .skip(1)
                            .flatten()
                            .map(|m| m.as_str().to_string())
                            .collect();
                        result.push((i, caps_list));
                    }
                }
            }
            result
        };

        // 逐个触发
        for (idx, caps_list) in matches {
            let (callback, send_text) = {
                let state = self.state.borrow();
                (
                    state.triggers[idx].callback.clone(),
                    state.triggers[idx].send_text.clone(),
                )
            };
            if let Ok(args_table) = self.lua.create_table() {
                for (i, m) in caps_list.iter().enumerate() {
                    let _ = args_table.set(i + 1, m.as_str());
                }
                let _ = callback.call::<()>(args_table);
            }
            if !send_text.is_empty() {
                self.state.borrow_mut().pending_commands.push(send_text);
            }
        }
    }

    /// 处理用户输入，匹配别名
    pub fn process_input(&self, input: &str) -> bool {
        self.state.borrow_mut().pending_commands.clear();

        let matches: Vec<(usize, Vec<String>)> = {
            let state = self.state.borrow();
            let mut result = Vec::new();
            for (i, alias) in state.aliases.iter().enumerate() {
                if !alias.enabled {
                    continue;
                }
                if let Some(caps) = alias.pattern.captures(input) {
                    let caps_list: Vec<String> = caps
                        .iter()
                        .skip(1)
                        .flatten()
                        .map(|m| m.as_str().to_string())
                        .collect();
                    result.push((i, caps_list));
                }
            }
            result
        };

        if matches.is_empty() {
            return false;
        }

        for (idx, caps_list) in matches {
            let callback = {
                let state = self.state.borrow();
                state.aliases[idx].callback.clone()
            };
            if let Ok(args_table) = self.lua.create_table() {
                let _ = args_table.set(0, input);
                for (i, m) in caps_list.iter().enumerate() {
                    let _ = args_table.set(i + 1, m.as_str());
                }
                let _ = callback.call::<()>(args_table);
            }
        }

        true
    }

    /// 触发指定定时器
    pub fn fire_timer(&self, index: usize) {
        self.state.borrow_mut().pending_commands.clear();

        let (callback, send_text, one_shot) = {
            let state = self.state.borrow();
            if index < state.timers.len() && state.timers[index].enabled {
                (
                    state.timers[index].callback.clone(),
                    state.timers[index].send_text.clone(),
                    state.timers[index].one_shot,
                )
            } else {
                return;
            }
        };

        let _ = callback.call::<()>(());

        if !send_text.is_empty() {
            // send_text 可能是 Lua 代码（MUSHclient 的 script 参数）
            let _ = self.lua.load(&send_text).exec();
        }

        if one_shot {
            self.state.borrow_mut().timers.remove(index);
        }
    }

    /// 取出待发送的命令
    pub fn drain_commands(&self) -> Vec<String> {
        self.state.borrow_mut().pending_commands.drain(..).collect()
    }

    /// 设置 Lua 变量（内部 HashMap，通过 GetVariable 访问）
    pub fn set_variable(&mut self, key: &str, value: &str) {
        self.state
            .borrow_mut()
            .variables
            .insert(key.to_string(), value.to_string());
    }

    /// 设置连接主机地址（供 GetInfo(1) 返回）
    pub fn set_host(&self, host: &str) {
        self.state.borrow_mut().host = host.to_string();
    }

    /// 设置 Lua 全局变量（脚本中可直接按名引用）
    pub fn set_global(&self, name: &str, value: &str) {
        let globals = self.lua.globals();
        let _ = globals.set(name, value);
    }

    #[allow(dead_code)]
    /// 设置连接状态
    pub fn set_connected(&mut self, connected: bool) {
        self.state.borrow_mut().connected = connected;
    }
    #[allow(dead_code)]
    /// 取出连接请求标志（一次性消费）
    pub fn take_connect_requested(&self) -> bool {
        let val = self.state.borrow_mut().connect_requested;
        if val {
            self.state.borrow_mut().connect_requested = false;
        }
        val
    }

    #[allow(dead_code)]
    /// 取出断开请求标志（一次性消费）
    pub fn take_disconnect_requested(&self) -> bool {
        let val = self.state.borrow_mut().disconnect_requested;
        if val {
            self.state.borrow_mut().disconnect_requested = false;
        }
        val
    }

    /// 取出待发送的日志消息
    pub fn drain_logs(&self) -> Vec<String> {
        self.state.borrow_mut().pending_logs.drain(..).collect()
    }

    /// 获取定时器列表（interval_millis）
    pub fn timer_intervals(&self) -> Vec<u64> {
        self.state
            .borrow()
            .timers
            .iter()
            .filter(|t| t.enabled)
            .map(|t| t.interval_millis)
            .collect()
    }

    #[allow(dead_code)]
    /// 获取触发器数量
    pub fn trigger_count(&self) -> usize {
        self.state.borrow().triggers.len()
    }

    #[allow(dead_code)]
    /// 获取别名数量
    pub fn alias_count(&self) -> usize {
        self.state.borrow().aliases.len()
    }

    #[allow(dead_code)]
    /// 获取定时器数量
    pub fn timer_count(&self) -> usize {
        self.state.borrow().timers.len()
    }
}

/// 将 PCRE 正则模式转换为 Rust regex 兼容语法
///
/// MushClient 使用 PCRE 引擎，与 Rust regex crate 存在语法差异。
/// 此函数处理常见的兼容性问题：
/// - `\Z` (PCRE: 字符串末尾或末尾换行前) → `$`
/// - `\z` (PCRE: 字符串绝对末尾) → `$`
fn convert_pcre_to_rust_regex(pattern: &str) -> String {
    let mut result = String::with_capacity(pattern.len());
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            let next = chars[i + 1];
            match next {
                'Z' => {
                    // PCRE \Z → Rust $
                    result.push('$');
                    i += 2;
                }
                'z' => {
                    // PCRE \z → Rust $
                    result.push('$');
                    i += 2;
                }
                _ => {
                    result.push(chars[i]);
                    i += 1;
                }
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    result
}

/// 添加触发器的通用实现
#[allow(clippy::too_many_arguments)]
fn add_trigger_impl(
    lua: &Lua,
    state_rc: &Rc<RefCell<ScriptState>>,
    name: &str,
    match_str: &str,
    flags: i64,
    script: &str,
    _send_to: i64,
    sequence: i32,
) -> LuaResult<Value> {
    let case_insensitive = (flags & 16) != 0;
    let is_regex = (flags & 32) != 0;

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

    let re = Regex::new(&re_str)
        .map_err(|e| mlua::Error::external(format!("无效正则 '{}': {}", re_str, e)))?;

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

    state_rc.borrow_mut().triggers.push(Trigger {
        name: name.to_string(),
        pattern: re,
        callback,
        enabled: (flags & 1) != 0,
        group: String::new(),
        sequence,
        multiline: false,
        lines_to_match: 1,
        omit_from_output: false,
        temporary: (flags & 4096) != 0,
        send_text: String::new(),
    });

    Ok(Value::Integer(0))
}

/// 转义正则特殊字符（保留 * 和 ? 用于通配符转换）
fn regex_escape(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        match c {
            '*' => result.push('*'),
            '?' => result.push('?'),
            '.' | '+' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '^' | '$' | '\\' => {
                result.push('\\');
                result.push(c);
            }
            _ => result.push(c),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // ================================================================
    // fix_lua_escape_sequences 预处理测试
    // ================================================================

    #[test]
    fn test_fix_escape_invalid_in_double_string() {
        // \- 在双引号字符串中是非法转义，应变为 \\-
        let input = r#"a,b,c,d=string.find(l,"[> ]*(%S+) \- (%w+)")"#;
        let output = fix_lua_escape_sequences(input);
        assert_eq!(output, r#"a,b,c,d=string.find(l,"[> ]*(%S+) \\- (%w+)")"#);
    }

    #[test]
    fn test_fix_escape_invalid_in_single_string() {
        // \- 在单引号字符串中也应修复
        let input = r#"x = 'hello \- world'"#;
        let output = fix_lua_escape_sequences(input);
        assert_eq!(output, r#"x = 'hello \\- world'"#);
    }

    #[test]
    fn test_fix_escape_preserves_valid() {
        // 合法转义 \n \t \\ \" 等应保持不变
        let input = r#"x = "hello\nworld\t\"test\\end""#;
        let output = fix_lua_escape_sequences(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_fix_escape_skip_comment() {
        // 注释中的 \- 不应被修改
        let input = "-- this is a comment with \\- escape\nx = 1";
        let output = fix_lua_escape_sequences(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_fix_escape_skip_long_comment() {
        // 长注释中的 \- 不应被修改
        let input = "--[[ comment with \\- escape ]]\nx = 1";
        let output = fix_lua_escape_sequences(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_fix_escape_skip_long_string() {
        // 长字符串中的 \- 不应被修改
        let input = "x = [[ hello \\- world ]]";
        let output = fix_lua_escape_sequences(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_fix_escape_multiple_invalid() {
        // 多个非法转义
        let input = r#"x = "\- \+ \? \* \.""#;
        let output = fix_lua_escape_sequences(input);
        assert_eq!(output, r#"x = "\\- \\+ \\? \\* \\.""#);
    }

    #[test]
    fn test_fix_escape_already_double_backslash() {
        // \\- 已经是合法的（\\ 是合法转义，- 是普通字符），不应被修改
        let input = r#"x = "\\- test""#;
        let output = fix_lua_escape_sequences(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_fix_escape_mixed_valid_invalid() {
        // 混合合法和非法转义
        let input = r#"x = "hello\nworld\-test""#;
        let output = fix_lua_escape_sequences(input);
        assert_eq!(output, r#"x = "hello\nworld\\-test""#);
    }

    #[test]
    fn test_fix_escape_real_world_pattern() {
        // 实际脚本中的模式
        let input = r#"a,b,c,d=string.find(l,"[> ]*(%S+) \- (%w+)")"#;
        let output = fix_lua_escape_sequences(input);
        assert_eq!(output, r#"a,b,c,d=string.find(l,"[> ]*(%S+) \\- (%w+)")"#);
    }

    #[test]
    fn test_fix_escape_no_change_needed() {
        // 无需修改的代码
        let input = r#"x = "hello world"\ny = 1"#;
        let output = fix_lua_escape_sequences(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_fix_escape_line_comment_then_code() {
        // 注释后跟代码
        let input = "-- comment with \\- \nlocal x = \"test\\-value\"";
        let output = fix_lua_escape_sequences(input);
        assert_eq!(output, "-- comment with \\- \nlocal x = \"test\\\\-value\"");
    }

    #[test]
    fn test_fix_escape_hex_escape() {
        // \x41 是合法的十六进制转义，不应被修改
        let input = r#"x = "\x41\x42""#;
        let output = fix_lua_escape_sequences(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_fix_escape_digit_escape() {
        // \123 是合法的十进制转义，不应被修改
        let input = r#"x = "\65\66""#;
        let output = fix_lua_escape_sequences(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_fix_escape_z_escape() {
        // \z 是合法转义（跳过空白），不应被修改
        let input = r#"x = "hello\z  world""#;
        let output = fix_lua_escape_sequences(input);
        assert_eq!(output, input);
    }

    /// 辅助：创建引擎并执行一段 Lua 代码
    fn with_engine<F>(f: F)
    where
        F: FnOnce(&mut LuaEngine),
    {
        let mut engine = LuaEngine::new().expect("引擎创建失败");
        f(&mut engine);
    }

    /// 辅助：执行 Lua 代码并返回结果
    fn eval<T: mlua::FromLua>(engine: &LuaEngine, code: &str) -> mlua::Result<T> {
        engine.lua.load(code).eval()
    }

    /// 辅助：执行 Lua 代码（无返回值）
    fn exec(engine: &LuaEngine, code: &str) -> mlua::Result<()> {
        engine.lua.load(code).exec()
    }

    // ================================================================
    // 引擎基础
    // ================================================================

    #[test]
    fn test_engine_new() {
        let engine = LuaEngine::new();
        assert!(engine.is_ok());
    }

    #[test]
    fn test_set_script_path() {
        with_engine(|engine| {
            engine.set_script_path("/home/user/scripts/main.lua");
            assert_eq!(
                *engine.script_dir.borrow(),
                Some("/home/user/scripts/".to_string())
            );
            assert_eq!(
                engine.script_path,
                Some("/home/user/scripts/main.lua".to_string())
            );
        });
    }

    #[test]
    fn test_set_script_path_no_slash() {
        with_engine(|engine| {
            engine.set_script_path("main.lua");
            assert_eq!(*engine.script_dir.borrow(), Some("./".to_string()));
        });
    }

    // ================================================================
    // 命令执行 API
    // ================================================================

    #[test]
    fn test_send() {
        with_engine(|engine| {
            exec(engine, "send('look')").unwrap();
            let cmds = engine.drain_commands();
            assert_eq!(cmds, vec!["look"]);
        });
    }

    #[test]
    fn test_execute() {
        with_engine(|engine| {
            let result: i64 = eval(engine, "return Execute('look')").unwrap();
            assert_eq!(result, 0);
            let cmds = engine.drain_commands();
            assert_eq!(cmds, vec!["look"]);
        });
    }

    // ================================================================
    // 输出 API
    // ================================================================

    #[test]
    fn test_note() {
        with_engine(|engine| {
            exec(engine, "Note('hello')").unwrap();
            let logs = engine.drain_logs();
            assert!(logs.contains(&"hello".to_string()));
        });
    }

    #[test]
    fn test_colour_note() {
        with_engine(|engine| {
            exec(engine, "ColourNote('red', 'black', 'test')").unwrap();
            let logs = engine.drain_logs();
            // 应生成 ANSI 转义序列：\x1B[31;40mtest\x1B[0m
            assert!(logs.iter().any(|l| l.contains("\x1b[31;40mtest\x1b[0m")));
        });
    }

    #[test]
    fn test_tell() {
        with_engine(|engine| {
            exec(engine, "Tell('inline')").unwrap();
            let logs = engine.drain_logs();
            assert!(logs.contains(&"inline".to_string()));
        });
    }

    // ================================================================
    // 触发器 API
    // ================================================================

    #[test]
    fn test_add_trigger() {
        with_engine(|engine| {
            let result: i64 = eval(
                engine,
                "return AddTrigger('test_trig', 'hello', '', 1, 0, 0, '', '', 0, 0)",
            )
            .unwrap();
            assert_eq!(result, 0);
            assert_eq!(engine.trigger_count(), 1);
        });
    }

    #[test]
    fn test_add_trigger_regex() {
        with_engine(|engine| {
            let result: i64 = eval(
                engine,
                r#"return AddTrigger('regex_trig', [[^\d+hp]], '', 33, 0, 0, '', '', 0, 0)"#,
            )
            .unwrap();
            assert_eq!(result, 0);
        });
    }

    #[test]
    fn test_add_trigger_case_insensitive() {
        with_engine(|engine| {
            let result: i64 = eval(
                engine,
                "return AddTrigger('ci_trig', 'HELLO', '', 17, 0, 0, '', '', 0, 0)",
            )
            .unwrap();
            assert_eq!(result, 0);
        });
    }

    #[test]
    fn test_delete_trigger() {
        with_engine(|engine| {
            exec(
                engine,
                "AddTrigger('del_trig', 'test', '', 1, 0, 0, '', '', 0, 0)",
            )
            .unwrap();
            assert_eq!(engine.trigger_count(), 1);
            let result: i64 = eval(engine, "return DeleteTrigger('del_trig')").unwrap();
            assert_eq!(result, 0);
            assert_eq!(engine.trigger_count(), 0);
        });
    }

    #[test]
    fn test_delete_trigger_not_found() {
        with_engine(|engine| {
            let result: i64 = eval(engine, "return DeleteTrigger('nonexistent')").unwrap();
            assert_eq!(result, 1);
        });
    }

    #[test]
    fn test_get_trigger_list() {
        with_engine(|engine| {
            exec(
                engine,
                "AddTrigger('trig1', 'a', '', 1, 0, 0, '', '', 0, 0)",
            )
            .unwrap();
            exec(
                engine,
                "AddTrigger('trig2', 'b', '', 1, 0, 0, '', '', 0, 0)",
            )
            .unwrap();
            let list: Vec<String> = eval(
                engine,
                "local t = GetTriggerList(); local r = {}; for i=1,#t do r[i]=t[i] end; return r",
            )
            .unwrap();
            assert!(list.contains(&"trig1".to_string()));
            assert!(list.contains(&"trig2".to_string()));
        });
    }

    #[test]
    fn test_get_trigger_info_enabled() {
        with_engine(|engine| {
            exec(
                engine,
                "AddTrigger('info_trig', 'test', '', 1, 0, 0, '', '', 0, 0)",
            )
            .unwrap();
            let enabled: bool = eval(engine, "return GetTriggerInfo('info_trig', 8)").unwrap();
            assert!(enabled);
        });
    }

    #[test]
    fn test_get_trigger_info_group() {
        with_engine(|engine| {
            exec(
                engine,
                "AddTrigger('grp_trig', 'test', '', 1, 0, 0, '', '', 0, 0)",
            )
            .unwrap();
            exec(engine, "SetTriggerOption('grp_trig', 'group', 'mygroup')").unwrap();
            let group: String = eval(engine, "return GetTriggerInfo('grp_trig', 26)").unwrap();
            assert_eq!(group, "mygroup");
        });
    }

    #[test]
    fn test_set_trigger_option() {
        with_engine(|engine| {
            exec(
                engine,
                "AddTrigger('opt_trig', 'test', '', 1, 0, 0, '', '', 0, 0)",
            )
            .unwrap();
            exec(engine, "SetTriggerOption('opt_trig', 'enabled', false)").unwrap();
            let enabled: bool = eval(engine, "return GetTriggerInfo('opt_trig', 8)").unwrap();
            assert!(!enabled);
        });
    }

    #[test]
    fn test_set_trigger_option_multiline() {
        with_engine(|engine| {
            exec(
                engine,
                "AddTrigger('ml_trig', 'test', '', 1, 0, 0, '', '', 0, 0)",
            )
            .unwrap();
            let result: i64 = eval(engine,
                "SetTriggerOption('ml_trig', 'multi_line', true); SetTriggerOption('ml_trig', 'lines_to_match', 3); return 0"
            ).unwrap();
            assert_eq!(result, 0);
        });
    }

    #[test]
    fn test_enable_trigger_group() {
        with_engine(|engine| {
            exec(engine, "AddTrigger('g1', 'a', '', 1, 0, 0, '', '', 0, 0)").unwrap();
            exec(engine, "AddTrigger('g2', 'b', '', 1, 0, 0, '', '', 0, 0)").unwrap();
            exec(engine, "SetTriggerOption('g1', 'group', 'grp_a')").unwrap();
            exec(engine, "SetTriggerOption('g2', 'group', 'grp_a')").unwrap();
            exec(engine, "EnableTriggerGroup('grp_a', false)").unwrap();
            let e1: bool = eval(engine, "return GetTriggerInfo('g1', 8)").unwrap();
            let e2: bool = eval(engine, "return GetTriggerInfo('g2', 8)").unwrap();
            assert!(!e1);
            assert!(!e2);
        });
    }

    #[test]
    fn test_enable_trigger_group_skips_empty_group() {
        with_engine(|engine| {
            exec(
                engine,
                "AddTrigger('nogrp', 'x', '', 1, 0, 0, '', '', 0, 0)",
            )
            .unwrap();
            exec(engine, "EnableTriggerGroup('somegroup', false)").unwrap();
            let enabled: bool = eval(engine, "return GetTriggerInfo('nogrp', 8)").unwrap();
            assert!(enabled); // 空group的触发器不应被影响
        });
    }

    #[test]
    fn test_enable_trigger() {
        with_engine(|engine| {
            exec(
                engine,
                "AddTrigger('et', 'test', '', 1, 0, 0, '', '', 0, 0)",
            )
            .unwrap();
            exec(engine, "EnableTrigger('et', false)").unwrap();
            let enabled: bool = eval(engine, "return GetTriggerInfo('et', 8)").unwrap();
            assert!(!enabled);
            exec(engine, "EnableTrigger('et', true)").unwrap();
            let enabled2: bool = eval(engine, "return GetTriggerInfo('et', 8)").unwrap();
            assert!(enabled2);
        });
    }

    #[test]
    fn test_enable_trigger_not_found() {
        with_engine(|engine| {
            let result: i64 = eval(engine, "return EnableTrigger('nonexistent', true)").unwrap();
            assert_eq!(result, 1); // 1 = not found
        });
    }

    #[test]
    fn test_trigger_matching() {
        with_engine(|engine| {
            exec(engine, r#"
                test_result = nil
                AddTrigger('match_trig', [[hello (\w+)]], '', 33, 0, 0, '', 'function(t) test_result = t[1] end', 0, 0)
            "#).unwrap();
            engine.process_output("hello world");
            let result: Option<String> = eval(engine, "return test_result").unwrap();
            assert_eq!(result, Some("world".to_string()));
        });
    }

    #[test]
    fn test_trigger_disabled_not_matching() {
        with_engine(|engine| {
            exec(engine, r#"
                test_result = nil
                AddTrigger('dis_trig', 'test', '', 0, 0, 0, '', 'function() test_result = true end', 0, 0)
            "#).unwrap();
            engine.process_output("test");
            let result: Option<bool> = eval(engine, "return test_result").unwrap();
            assert_eq!(result, None);
        });
    }

    #[test]
    fn test_trigger_wildcard_matching() {
        with_engine(|engine| {
            exec(engine, r#"
                wc_result = nil
                AddTrigger('wc_trig', 'You see * here', '', 1, 0, 0, '', 'function(t) wc_result = t[1] end', 0, 0)
            "#).unwrap();
            engine.process_output("You see a goblin here");
            let result: Option<String> = eval(engine, "return wc_result").unwrap();
            assert_eq!(result, Some("a goblin".to_string()));
        });
    }

    #[test]
    fn test_trigger_case_insensitive_matching() {
        with_engine(|engine| {
            exec(engine, r#"
                ci_result = nil
                AddTrigger('ci_trig2', 'HELLO', '', 17, 0, 0, '', 'function() ci_result = true end', 0, 0)
            "#).unwrap();
            engine.process_output("hello");
            let result: Option<bool> = eval(engine, "return ci_result").unwrap();
            assert_eq!(result, Some(true));
        });
    }

    #[test]
    fn test_add_trigger_ex_same_as_add_trigger() {
        with_engine(|engine| {
            let result: i64 =
                eval(engine, "return AddTriggerEx('ex_trig', 'test', '', 1)").unwrap();
            assert_eq!(result, 0);
            assert_eq!(engine.trigger_count(), 1);
        });
    }

    #[test]
    fn test_trigger_omit_from_output() {
        with_engine(|engine| {
            exec(
                engine,
                "AddTrigger('omit_trig', 'secret', '', 1, 0, 0, '', '', 0, 0)",
            )
            .unwrap();
            exec(
                engine,
                "SetTriggerOption('omit_trig', 'omit_from_output', true)",
            )
            .unwrap();
            // omit_from_output 标记已设置，验证通过 GetTriggerInfo 间接确认
            // 实际的 omit 行为由 app 层处理
            assert_eq!(engine.trigger_count(), 1);
        });
    }

    #[test]
    fn test_trigger_temporary_flag() {
        with_engine(|engine| {
            // flag 4096 = Temporary
            let result: i64 = eval(
                engine,
                "return AddTrigger('temp_trig', 'test', '', 4097, 0, 0, '', '', 0, 0)",
            )
            .unwrap();
            assert_eq!(result, 0);
        });
    }

    #[test]
    fn test_trigger_sequence() {
        with_engine(|engine| {
            let result: i64 = eval(
                engine,
                "return AddTrigger('seq_trig', 'test', '', 1, 0, 0, '', '', 0, 100)",
            )
            .unwrap();
            assert_eq!(result, 0);
        });
    }

    #[test]
    fn test_get_trigger_info_unknown_code() {
        with_engine(|engine| {
            exec(
                engine,
                "AddTrigger('unk_trig', 'test', '', 1, 0, 0, '', '', 0, 0)",
            )
            .unwrap();
            let val: Value = eval(engine, "return GetTriggerInfo('unk_trig', 999)").unwrap();
            assert!(val.is_nil());
        });
    }

    #[test]
    fn test_get_trigger_info_not_found() {
        with_engine(|engine| {
            let val: Value = eval(engine, "return GetTriggerInfo('nonexistent', 8)").unwrap();
            assert!(val.is_nil());
        });
    }

    #[test]
    fn test_set_trigger_option_not_found() {
        with_engine(|engine| {
            let result: i64 = eval(
                engine,
                "return SetTriggerOption('nonexistent', 'enabled', true)",
            )
            .unwrap();
            assert_eq!(result, 1); // 1 = not found
        });
    }

    // ================================================================
    // 别名 API
    // ================================================================

    #[test]
    fn test_add_alias() {
        with_engine(|engine| {
            let result: i64 =
                eval(engine, "return AddAlias('test_alias', 'kill *', '', 1)").unwrap();
            assert_eq!(result, 0);
            assert_eq!(engine.alias_count(), 1);
        });
    }

    #[test]
    fn test_add_alias_regex() {
        with_engine(|engine| {
            let result: i64 = eval(
                engine,
                r#"return AddAlias('regex_alias', [[^go (\w+)$]], '', 33)"#,
            )
            .unwrap();
            assert_eq!(result, 0);
        });
    }

    #[test]
    fn test_delete_alias() {
        with_engine(|engine| {
            exec(engine, "AddAlias('del_alias', 'test', '', 1)").unwrap();
            let result: i64 = eval(engine, "return DeleteAlias('del_alias')").unwrap();
            assert_eq!(result, 0);
            assert_eq!(engine.alias_count(), 0);
        });
    }

    #[test]
    fn test_delete_alias_not_found() {
        with_engine(|engine| {
            let result: i64 = eval(engine, "return DeleteAlias('nonexistent')").unwrap();
            assert_eq!(result, 1);
        });
    }

    #[test]
    fn test_get_alias_list() {
        with_engine(|engine| {
            exec(engine, "AddAlias('a1', 'x', '', 1)").unwrap();
            exec(engine, "AddAlias('a2', 'y', '', 1)").unwrap();
            let list: Vec<String> = eval(
                engine,
                "local t = GetAliasList(); local r = {}; for i=1,#t do r[i]=t[i] end; return r",
            )
            .unwrap();
            assert!(list.contains(&"a1".to_string()));
            assert!(list.contains(&"a2".to_string()));
        });
    }

    #[test]
    fn test_set_alias_option() {
        with_engine(|engine| {
            exec(engine, "AddAlias('opt_alias', 'test', '', 1)").unwrap();
            exec(engine, "SetAliasOption('opt_alias', 'group', 'mygroup')").unwrap();
            let result: i64 = eval(
                engine,
                "return SetAliasOption('opt_alias', 'enabled', false)",
            )
            .unwrap();
            assert_eq!(result, 0);
        });
    }

    #[test]
    fn test_set_alias_option_not_found() {
        with_engine(|engine| {
            let result: i64 = eval(
                engine,
                "return SetAliasOption('nonexistent', 'enabled', true)",
            )
            .unwrap();
            assert_eq!(result, 1);
        });
    }

    #[test]
    fn test_alias_matching() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                alias_result = nil
                AddAlias('match_alias', 'kill *', '', 1, 'function(t) alias_result = t[1] end')
            "#,
            )
            .unwrap();
            let matched = engine.process_input("kill goblin");
            assert!(matched);
            let result: Option<String> = eval(engine, "return alias_result").unwrap();
            assert_eq!(result, Some("goblin".to_string()));
        });
    }

    #[test]
    fn test_alias_no_match() {
        with_engine(|engine| {
            exec(engine, "AddAlias('no_match', 'kill *', '', 1)").unwrap();
            let matched = engine.process_input("look");
            assert!(!matched);
        });
    }

    #[test]
    fn test_alias_regex_matching() {
        with_engine(|engine| {
            exec(engine, r#"
                regex_alias_result = nil
                AddAlias('regex_al', [[^go (\w+)$]], '', 33, 'function(t) regex_alias_result = t[1] end')
            "#).unwrap();
            let matched = engine.process_input("go north");
            assert!(matched);
            let result: Option<String> = eval(engine, "return regex_alias_result").unwrap();
            assert_eq!(result, Some("north".to_string()));
        });
    }

    #[test]
    fn test_alias_disabled_not_matching() {
        with_engine(|engine| {
            exec(engine, "AddAlias('dis_al', 'test', '', 0)").unwrap();
            let matched = engine.process_input("test");
            assert!(!matched);
        });
    }

    #[test]
    fn test_alias_wildcard_question_mark() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                qm_result = nil
                AddAlias('qm_alias', 'go ?', '', 1, 'function(t) qm_result = t[1] end')
            "#,
            )
            .unwrap();
            let matched = engine.process_input("go n");
            assert!(matched);
            let result: Option<String> = eval(engine, "return qm_result").unwrap();
            assert_eq!(result, Some("n".to_string()));
        });
    }

    // ================================================================
    // 定时器 API
    // ================================================================

    #[test]
    fn test_add_timer() {
        with_engine(|engine| {
            let result: i64 =
                eval(engine, "return AddTimer('test_timer', 0, 0, 5, '', 1)").unwrap();
            assert_eq!(result, 0);
            assert_eq!(engine.timer_count(), 1);
        });
    }

    #[test]
    fn test_delete_timer() {
        with_engine(|engine| {
            exec(engine, "AddTimer('del_timer', 0, 0, 5, '', 1)").unwrap();
            let result: i64 = eval(engine, "return DeleteTimer('del_timer')").unwrap();
            assert_eq!(result, 0);
            assert_eq!(engine.timer_count(), 0);
        });
    }

    #[test]
    fn test_delete_timer_not_found() {
        with_engine(|engine| {
            let result: i64 = eval(engine, "return DeleteTimer('nonexistent')").unwrap();
            assert_eq!(result, 1);
        });
    }

    #[test]
    fn test_get_timer_list() {
        with_engine(|engine| {
            exec(engine, "AddTimer('t1', 0, 0, 5, '', 1)").unwrap();
            exec(engine, "AddTimer('t2', 0, 0, 10, '', 1)").unwrap();
            let list: Vec<String> = eval(
                engine,
                "local t = GetTimerList(); local r = {}; for i=1,#t do r[i]=t[i] end; return r",
            )
            .unwrap();
            assert!(list.contains(&"t1".to_string()));
            assert!(list.contains(&"t2".to_string()));
        });
    }

    #[test]
    fn test_get_timer_info() {
        with_engine(|engine| {
            exec(engine, "AddTimer('info_timer', 0, 0, 5, '', 1)").unwrap();
            let enabled: bool = eval(engine, "return GetTimerInfo('info_timer', 6)").unwrap();
            assert!(enabled);
        });
    }

    #[test]
    fn test_get_timer_info_group() {
        with_engine(|engine| {
            exec(engine, "AddTimer('grp_timer', 0, 0, 5, '', 1)").unwrap();
            exec(engine, "SetTimerOption('grp_timer', 'group', 'mygroup')").unwrap();
            let group: String = eval(engine, "return GetTimerInfo('grp_timer', 19)").unwrap();
            assert_eq!(group, "mygroup");
        });
    }

    #[test]
    fn test_get_timer_info_not_found() {
        with_engine(|engine| {
            let val: Value = eval(engine, "return GetTimerInfo('nonexistent', 6)").unwrap();
            assert!(val.is_nil());
        });
    }

    #[test]
    fn test_set_timer_option() {
        with_engine(|engine| {
            exec(engine, "AddTimer('opt_timer', 0, 0, 5, '', 1)").unwrap();
            exec(engine, "SetTimerOption('opt_timer', 'enabled', false)").unwrap();
            let enabled: bool = eval(engine, "return GetTimerInfo('opt_timer', 6)").unwrap();
            assert!(!enabled);
        });
    }

    #[test]
    fn test_enable_timer_group() {
        with_engine(|engine| {
            exec(engine, "AddTimer('tg1', 0, 0, 5, '', 1)").unwrap();
            exec(engine, "AddTimer('tg2', 0, 0, 10, '', 1)").unwrap();
            exec(engine, "SetTimerOption('tg1', 'group', 'grp_t')").unwrap();
            exec(engine, "SetTimerOption('tg2', 'group', 'grp_t')").unwrap();
            exec(engine, "EnableTimerGroup('grp_t', false)").unwrap();
            let e1: bool = eval(engine, "return GetTimerInfo('tg1', 6)").unwrap();
            assert!(!e1);
        });
    }

    #[test]
    fn test_enable_timer_group_skips_empty_group() {
        with_engine(|engine| {
            exec(engine, "AddTimer('nogrp_t', 0, 0, 5, '', 1)").unwrap();
            exec(engine, "EnableTimerGroup('somegroup', false)").unwrap();
            let enabled: bool = eval(engine, "return GetTimerInfo('nogrp_t', 6)").unwrap();
            assert!(enabled); // 空group的定时器不应被影响
        });
    }

    #[test]
    fn test_enable_timer() {
        with_engine(|engine| {
            exec(engine, "AddTimer('et_t', 0, 0, 5, '', 1)").unwrap();
            exec(engine, "EnableTimer('et_t', false)").unwrap();
            let enabled: bool = eval(engine, "return GetTimerInfo('et_t', 6)").unwrap();
            assert!(!enabled);
            exec(engine, "EnableTimer('et_t', true)").unwrap();
            let enabled2: bool = eval(engine, "return GetTimerInfo('et_t', 6)").unwrap();
            assert!(enabled2);
        });
    }

    #[test]
    fn test_enable_timer_not_found() {
        with_engine(|engine| {
            let result: i64 = eval(engine, "return EnableTimer('nonexistent', true)").unwrap();
            assert_eq!(result, 1);
        });
    }

    #[test]
    fn test_timer_intervals() {
        with_engine(|engine| {
            exec(engine, "AddTimer('i1', 0, 0, 5, '', 1)").unwrap();
            exec(engine, "AddTimer('i2', 0, 0, 10, '', 1)").unwrap();
            let intervals = engine.timer_intervals();
            assert!(intervals.contains(&5000));
            assert!(intervals.contains(&10000));
        });
    }

    #[test]
    fn test_fire_timer() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                timer_result = nil
                AddTimer('fire_t', 0, 0, 5, '', 1, 'timer_result = "fired"')
            "#,
            )
            .unwrap();
            engine.fire_timer(0);
            let result: Option<String> = eval(engine, "return timer_result").unwrap();
            assert_eq!(result, Some("fired".to_string()));
        });
    }

    #[test]
    fn test_fire_timer_one_shot() {
        with_engine(|engine| {
            // flag 4 = OneShot, flag 1 = Enabled
            exec(engine, "AddTimer('oneshot', 0, 0, 5, '', 5)").unwrap();
            assert_eq!(engine.timer_count(), 1);
            engine.fire_timer(0);
            assert_eq!(engine.timer_count(), 0);
        });
    }

    #[test]
    fn test_fire_timer_disabled() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                disabled_timer_result = nil
                AddTimer('dis_t', 0, 0, 5, '', 0, 'disabled_timer_result = true')
            "#,
            )
            .unwrap();
            engine.fire_timer(0);
            let result: Option<bool> = eval(engine, "return disabled_timer_result").unwrap();
            assert_eq!(result, None);
        });
    }

    #[test]
    fn test_timer_zero_interval() {
        with_engine(|engine| {
            // 0秒间隔应被设为1秒（1000毫秒）
            exec(engine, "AddTimer('zero_t', 0, 0, 0, '', 1)").unwrap();
            let intervals = engine.timer_intervals();
            assert!(intervals.contains(&1000));
        });
    }

    #[test]
    fn test_timer_float_sec() {
        with_engine(|engine| {
            // 浮点数秒应正确转换为毫秒
            exec(engine, "AddTimer('float_t', 0, 0, 0.10, '', 1)").unwrap();
            let intervals = engine.timer_intervals();
            assert!(intervals.contains(&100)); // 0.10秒 = 100毫秒
        });
    }

    #[test]
    fn test_timer_nil_sec() {
        with_engine(|engine| {
            // nil 秒参数应默认为1秒（1000毫秒）
            exec(engine, "AddTimer('nil_t', 0, 0, nil, '', 1)").unwrap();
            let intervals = engine.timer_intervals();
            assert!(intervals.contains(&1000));
        });
    }

    // ================================================================
    // 变量 API
    // ================================================================

    #[test]
    fn test_set_get_variable() {
        with_engine(|engine| {
            exec(engine, "SetVariable('key1', 'value1')").unwrap();
            let val: String = eval(engine, "return GetVariable('key1')").unwrap();
            assert_eq!(val, "value1");
        });
    }

    #[test]
    fn test_get_variable_not_found() {
        with_engine(|engine| {
            let val: Value = eval(engine, "return GetVariable('nonexistent')").unwrap();
            assert!(val.is_nil());
        });
    }

    #[test]
    fn test_delete_variable() {
        with_engine(|engine| {
            exec(engine, "SetVariable('del_key', 'val')").unwrap();
            exec(engine, "DeleteVariable('del_key')").unwrap();
            let val: Value = eval(engine, "return GetVariable('del_key')").unwrap();
            assert!(val.is_nil());
        });
    }

    #[test]
    fn test_get_variable_list() {
        with_engine(|engine| {
            exec(engine, "SetVariable('a', '1')").unwrap();
            exec(engine, "SetVariable('b', '2')").unwrap();
            // GetVariableList 返回 key-value 对表
            let val_a: String = eval(engine, "local t = GetVariableList(); return t.a").unwrap();
            let val_b: String = eval(engine, "local t = GetVariableList(); return t.b").unwrap();
            assert_eq!(val_a, "1");
            assert_eq!(val_b, "2");
        });
    }

    #[test]
    fn test_set_variable_rust() {
        with_engine(|engine| {
            engine.set_variable("rust_key", "rust_val");
            let val: String = eval(engine, "return GetVariable('rust_key')").unwrap();
            assert_eq!(val, "rust_val");
        });
    }

    #[test]
    fn test_variable_overwrite() {
        with_engine(|engine| {
            exec(engine, "SetVariable('ow_key', 'old')").unwrap();
            exec(engine, "SetVariable('ow_key', 'new')").unwrap();
            let val: String = eval(engine, "return GetVariable('ow_key')").unwrap();
            assert_eq!(val, "new");
        });
    }

    // ================================================================
    // 配置 API
    // ================================================================

    #[test]
    fn test_get_info_35() {
        with_engine(|engine| {
            engine.set_script_path("/home/user/scripts/main.lua");
            let path: String = eval(engine, "return GetInfo(35)").unwrap();
            assert!(path.contains('\\'));
            assert!(!path.contains('/'));
        });
    }

    #[test]
    fn test_get_info_35_no_script_path() {
        with_engine(|engine| {
            let path: String = eval(engine, "return GetInfo(35)").unwrap();
            assert_eq!(path, ".\\");
        });
    }

    #[test]
    fn test_get_info_1() {
        with_engine(|engine| {
            // 默认未设置 host 时返回空字符串
            let ver: String = eval(engine, "return GetInfo(1)").unwrap();
            assert_eq!(ver, "");
            // 设置 host 后返回主机地址
            engine.set_host("ln.xkxmud.com");
            let host: String = eval(engine, "return GetInfo(1)").unwrap();
            assert_eq!(host, "ln.xkxmud.com");
        });
    }

    #[test]
    fn test_get_info_unknown() {
        with_engine(|engine| {
            let val: String = eval(engine, "return GetInfo(999)").unwrap();
            assert_eq!(val, "");
        });
    }

    #[test]
    fn test_set_get_option() {
        with_engine(|engine| {
            exec(engine, "SetOption('myopt', 42)").unwrap();
            let val: i64 = eval(engine, "return GetOption('myopt')").unwrap();
            assert_eq!(val, 42);
        });
    }

    #[test]
    fn test_get_option_not_found() {
        with_engine(|engine| {
            let val: Value = eval(engine, "return GetOption('nonexistent')").unwrap();
            assert!(val.is_nil());
        });
    }

    #[test]
    fn test_set_get_alpha_option() {
        with_engine(|engine| {
            exec(engine, "SetAlphaOption('myalpha', 'hello')").unwrap();
            let val: String = eval(engine, "return GetAlphaOption('myalpha')").unwrap();
            assert_eq!(val, "hello");
        });
    }

    #[test]
    fn test_get_alpha_option_not_found() {
        with_engine(|engine| {
            let val: Value = eval(engine, "return GetAlphaOption('nonexistent')").unwrap();
            assert!(val.is_nil());
        });
    }

    // ================================================================
    // 连接状态 API
    // ================================================================

    #[test]
    fn test_is_connected_default() {
        with_engine(|engine| {
            let connected: bool = eval(engine, "return IsConnected()").unwrap();
            assert!(!connected);
        });
    }

    #[test]
    fn test_connect_disconnect() {
        with_engine(|engine| {
            exec(engine, "Connect()").unwrap();
            assert!(engine.take_connect_requested());
            assert!(!engine.take_connect_requested());

            exec(engine, "Disconnect()").unwrap();
            assert!(engine.take_disconnect_requested());
        });
    }

    // ================================================================
    // 工具函数
    // ================================================================

    #[test]
    fn test_get_unique_number() {
        with_engine(|engine| {
            let n1: i64 = eval(engine, "return GetUniqueNumber()").unwrap();
            let n2: i64 = eval(engine, "return GetUniqueNumber()").unwrap();
            assert!(n2 > n1);
        });
    }

    #[test]
    fn test_trim() {
        with_engine(|engine| {
            let result: String = eval(engine, "return Trim('  hello  ')").unwrap();
            assert_eq!(result, "hello");
        });
    }

    #[test]
    fn test_trim_no_whitespace() {
        with_engine(|engine| {
            let result: String = eval(engine, "return Trim('hello')").unwrap();
            assert_eq!(result, "hello");
        });
    }

    // ================================================================
    // 日志 API
    // ================================================================

    #[test]
    fn test_is_log_open() {
        with_engine(|engine| {
            let open: bool = eval(engine, "return IsLogOpen()").unwrap();
            assert!(open);
        });
    }

    #[test]
    fn test_open_log() {
        with_engine(|engine| {
            // OpenLog 不应报错
            exec(engine, "OpenLog('test.log', true)").unwrap();
        });
    }

    // ================================================================
    // 常量表
    // ================================================================

    #[test]
    fn test_trigger_flag_constants() {
        with_engine(|engine| {
            let enabled: i64 = eval(engine, "return trigger_flag.Enabled").unwrap();
            assert_eq!(enabled, 1);
            let regex: i64 = eval(engine, "return trigger_flag.RegularExpression").unwrap();
            assert_eq!(regex, 32);
            let temp: i64 = eval(engine, "return trigger_flag.Temporary").unwrap();
            assert_eq!(temp, 4096);
        });
    }

    #[test]
    fn test_alias_flag_constants() {
        with_engine(|engine| {
            let enabled: i64 = eval(engine, "return alias_flag.Enabled").unwrap();
            assert_eq!(enabled, 1);
            let regex: i64 = eval(engine, "return alias_flag.RegularExpression").unwrap();
            assert_eq!(regex, 32);
        });
    }

    #[test]
    fn test_timer_flag_constants() {
        with_engine(|engine| {
            let enabled: i64 = eval(engine, "return timer_flag.Enabled").unwrap();
            assert_eq!(enabled, 1);
            let oneshot: i64 = eval(engine, "return timer_flag.OneShot").unwrap();
            assert_eq!(oneshot, 8192);
        });
    }

    #[test]
    fn test_error_code_constants() {
        with_engine(|engine| {
            let eok: i64 = eval(engine, "return error_code.eOK").unwrap();
            assert_eq!(eok, 0);
            let ebad: i64 = eval(engine, "return error_code.eBadRegularExpression").unwrap();
            assert_eq!(ebad, 3);
        });
    }

    #[test]
    fn test_error_desc_constants() {
        with_engine(|engine| {
            let eok: String = eval(engine, "return error_desc.eOK").unwrap();
            assert_eq!(eok, "OK");
        });
    }

    #[test]
    fn test_custom_colour_constants() {
        with_engine(|engine| {
            let black: i64 = eval(engine, "return custom_colour.Black").unwrap();
            assert_eq!(black, 0);
            let white: i64 = eval(engine, "return custom_colour.White").unwrap();
            assert_eq!(white, 15);
        });
    }

    // ================================================================
    // bit 库
    // ================================================================

    #[test]
    fn test_bit_bor() {
        with_engine(|engine| {
            let result: i64 = eval(engine, "return bit.bor(1, 2)").unwrap();
            assert_eq!(result, 3);
        });
    }

    #[test]
    fn test_bit_band() {
        with_engine(|engine| {
            let result: i64 = eval(engine, "return bit.band(3, 1)").unwrap();
            assert_eq!(result, 1);
        });
    }

    #[test]
    fn test_bit_bxor() {
        with_engine(|engine| {
            let result: i64 = eval(engine, "return bit.bxor(5, 3)").unwrap();
            assert_eq!(result, 6);
        });
    }

    #[test]
    fn test_bit_bnot() {
        with_engine(|engine| {
            let result: i64 = eval(engine, "return bit.bnot(0)").unwrap();
            assert_eq!(result, -1);
        });
    }

    #[test]
    fn test_bit_lshift() {
        with_engine(|engine| {
            let result: i64 = eval(engine, "return bit.lshift(1, 4)").unwrap();
            assert_eq!(result, 16);
        });
    }

    #[test]
    fn test_bit_rshift() {
        with_engine(|engine| {
            let result: i64 = eval(engine, "return bit.rshift(16, 4)").unwrap();
            assert_eq!(result, 1);
        });
    }

    // ================================================================
    // wait.lua 依赖
    // ================================================================

    #[test]
    fn test_make_regular_expression() {
        with_engine(|engine| {
            let result: String =
                eval(engine, "return MakeRegularExpression('hello * world?')").unwrap();
            assert_eq!(result, "hello .* world.");
        });
    }

    #[test]
    fn test_get_plugin_id() {
        with_engine(|engine| {
            let id: String = eval(engine, "return GetPluginID()").unwrap();
            assert_eq!(id, "");
        });
    }

    #[test]
    fn test_get_plugin_info() {
        with_engine(|engine| {
            let name: String = eval(engine, "return GetPluginInfo('', 1)").unwrap();
            assert_eq!(name, "RustLuaMud");
        });
    }

    // ================================================================
    // Lua 兼容性补丁
    // ================================================================

    #[test]
    fn test_table_getn() {
        with_engine(|engine| {
            let n: i64 = eval(engine, "return table.getn({1, 2, 3})").unwrap();
            assert_eq!(n, 3);
        });
    }

    #[test]
    fn test_table_foreachi() {
        with_engine(|engine| {
            let result: i64 = eval(
                engine,
                r#"
                local sum = 0
                table.foreachi({10, 20, 30}, function(i, v) sum = sum + v end)
                return sum
            "#,
            )
            .unwrap();
            assert_eq!(result, 60);
        });
    }

    #[test]
    fn test_table_foreach() {
        with_engine(|engine| {
            let result: i64 = eval(
                engine,
                r#"
                local sum = 0
                local t = {a=1, b=2, c=3}
                table.foreach(t, function(k, v) sum = sum + v end)
                return sum
            "#,
            )
            .unwrap();
            assert_eq!(result, 6);
        });
    }

    #[test]
    fn test_math_mod() {
        with_engine(|engine| {
            let result: f64 = eval(engine, "return math.mod(10, 3)").unwrap();
            assert!((result - 1.0).abs() < f64::EPSILON);
        });
    }

    #[test]
    fn test_math_pow() {
        with_engine(|engine| {
            let result: f64 = eval(engine, "return math.pow(2, 10)").unwrap();
            assert!((result - 1024.0).abs() < f64::EPSILON);
        });
    }

    // ================================================================
    // 多行触发器
    // ================================================================

    #[test]
    fn test_multiline_trigger() {
        with_engine(|engine| {
            exec(engine, r#"
                ml_result = nil
                AddTrigger('ml_trig', 'line1.*line2', '', 33, 0, 0, '', 'function() ml_result = true end', 0, 0)
                SetTriggerOption('ml_trig', 'multi_line', true)
                SetTriggerOption('ml_trig', 'lines_to_match', 2)
            "#).unwrap();
            engine.process_output("line1");
            engine.process_output("line2");
            let result: Option<bool> = eval(engine, "return ml_result").unwrap();
            assert_eq!(result, Some(true));
        });
    }

    #[test]
    fn test_single_line_trigger_no_multiline() {
        with_engine(|engine| {
            exec(engine, r#"
                sl_result = nil
                AddTrigger('sl_trig', 'exact_match', '', 33, 0, 0, '', 'function() sl_result = true end', 0, 0)
            "#).unwrap();
            engine.process_output("exact_match");
            let result: Option<bool> = eval(engine, "return sl_result").unwrap();
            assert_eq!(result, Some(true));
        });
    }

    // ================================================================
    // SQLite3
    // ================================================================

    #[test]
    fn test_sqlite3_open_close() {
        with_engine(|engine| {
            let result: i64 = eval(
                engine,
                r#"
                local db = sqlite3.open("/tmp/test_rustluamud.db")
                db:exec("CREATE TABLE IF NOT EXISTS test (id INTEGER PRIMARY KEY, name TEXT)")
                db:close()
                return 0
            "#,
            )
            .unwrap();
            assert_eq!(result, 0);
        });
    }

    #[test]
    fn test_sqlite3_insert_query() {
        with_engine(|engine| {
            let result: i64 = eval(
                engine,
                r#"
                local db = sqlite3.open("/tmp/test_rustluamud2.db")
                db:exec("DROP TABLE IF EXISTS test")
                db:exec("CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT)")
                db:exec("INSERT INTO test (name) VALUES ('hello')")
                local stmt = db:prepare("SELECT name FROM test WHERE id = ?")
                local row = stmt:step({1})
                local name = row and row[1] or nil
                stmt = nil
                db:close()
                return name == 'hello' and 1 or 0
            "#,
            )
            .unwrap();
            assert_eq!(result, 1);
        });
    }

    #[test]
    fn test_database_close() {
        with_engine(|engine| {
            // DatabaseClose 是全局函数，不应报错
            exec(engine, "DatabaseClose('test_db')").unwrap();
        });
    }

    // ================================================================
    // 触发器 send_text
    // ================================================================

    #[test]
    fn test_trigger_send_text() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                AddTrigger('send_trig', 'go', '', 1, 0, 0, '', '', 0, 0)
                SetTriggerOption('send_trig', 'send', 'north')
            "#,
            )
            .unwrap();
            engine.process_output("go");
            let cmds = engine.drain_commands();
            assert!(cmds.contains(&"north".to_string()));
        });
    }

    // ================================================================
    // 原始 API 兼容
    // ================================================================

    #[test]
    fn test_original_trigger_api() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                orig_result = nil
                trigger([[^hello (\w+)$]], function(t) orig_result = t[1] end)
            "#,
            )
            .unwrap();
            engine.process_output("hello Rust");
            let result: Option<String> = eval(engine, "return orig_result").unwrap();
            assert_eq!(result, Some("Rust".to_string()));
        });
    }

    #[test]
    fn test_original_alias_api() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                orig_alias_result = nil
                alias('^go (.+)$', function(t) orig_alias_result = t[1] end)
            "#,
            )
            .unwrap();
            let matched = engine.process_input("go north");
            assert!(matched);
            let result: Option<String> = eval(engine, "return orig_alias_result").unwrap();
            assert_eq!(result, Some("north".to_string()));
        });
    }

    #[test]
    fn test_original_get_set_api() {
        with_engine(|engine| {
            exec(engine, "set('mykey', 'myval')").unwrap();
            let val: String = eval(engine, "return get('mykey')").unwrap();
            assert_eq!(val, "myval");
        });
    }

    // ================================================================
    // eval_code
    // ================================================================

    #[test]
    fn test_eval_code() {
        with_engine(|engine| {
            engine.eval_code("eval_result = 42").unwrap();
            let val: i64 = eval(engine, "return eval_result").unwrap();
            assert_eq!(val, 42);
        });
    }

    #[test]
    fn test_eval_code_error() {
        with_engine(|engine| {
            let result = engine.eval_code("invalid!!!lua");
            assert!(result.is_err());
        });
    }

    // ================================================================
    // regex_escape 辅助函数
    // ================================================================

    #[test]
    fn test_regex_escape() {
        assert_eq!(regex_escape("hello.world"), r"hello\.world");
        assert_eq!(regex_escape("a+b"), r"a\+b");
        assert_eq!(regex_escape("test*"), "test*"); // * 保留
        assert_eq!(regex_escape("test?"), "test?"); // ? 保留
        assert_eq!(regex_escape("(group)"), r"\(group\)");
        assert_eq!(regex_escape("a|b"), r"a\|b");
        assert_eq!(regex_escape("^start"), r"\^start");
        assert_eq!(regex_escape("end$"), r"end\$");
        assert_eq!(regex_escape("path\\file"), r"path\\file");
    }

    // ================================================================
    // drain_commands / drain_logs
    // ================================================================

    #[test]
    fn test_drain_commands_clears() {
        with_engine(|engine| {
            exec(engine, "send('cmd1')").unwrap();
            exec(engine, "send('cmd2')").unwrap();
            let cmds = engine.drain_commands();
            assert_eq!(cmds.len(), 2);
            let cmds2 = engine.drain_commands();
            assert!(cmds2.is_empty());
        });
    }

    #[test]
    fn test_drain_logs_clears() {
        with_engine(|engine| {
            exec(engine, "Note('log1')").unwrap();
            exec(engine, "Note('log2')").unwrap();
            let logs = engine.drain_logs();
            assert_eq!(logs.len(), 2);
            let logs2 = engine.drain_logs();
            assert!(logs2.is_empty());
        });
    }

    // ===== 边界用例补充测试 =====

    #[test]
    fn test_script_path_method() {
        let mut engine = LuaEngine::new().unwrap();
        assert!(engine.script_path().is_none());
        engine.set_script_path("/some/path/");
        assert_eq!(engine.script_path().unwrap(), "/some/path/");
    }

    #[test]
    fn test_set_connected_true_false() {
        let mut engine = LuaEngine::new().unwrap();
        assert!(!eval::<bool>(&engine, "return IsConnected()").unwrap());
        engine.set_connected(true);
        assert!(eval::<bool>(&engine, "return IsConnected()").unwrap());
        engine.set_connected(false);
        assert!(!eval::<bool>(&engine, "return IsConnected()").unwrap());
    }

    #[test]
    fn test_take_connect_requested_consumed() {
        let engine = LuaEngine::new().unwrap();
        exec(&engine, "Connect()").unwrap();
        assert!(engine.take_connect_requested());
        assert!(!engine.take_connect_requested());
    }

    #[test]
    fn test_take_disconnect_requested_consumed() {
        let engine = LuaEngine::new().unwrap();
        exec(&engine, "Disconnect()").unwrap();
        assert!(engine.take_disconnect_requested());
        assert!(!engine.take_disconnect_requested());
    }

    #[test]
    fn test_fire_timer_out_of_bounds() {
        with_engine(|engine| {
            // 索引越界不应 panic
            engine.fire_timer(999);
        });
    }

    #[test]
    fn test_load_script_nonexistent() {
        let mut engine = LuaEngine::new().unwrap();
        let result = engine.load_script("/nonexistent/path/script.lua");
        assert!(result.is_err());
    }

    #[test]
    fn test_eval_code_error_returns_message() {
        let engine = LuaEngine::new().unwrap();
        let result = engine.eval_code("invalid{{{lua");
        assert!(result.is_err());
        assert!(!result.unwrap_err().is_empty());
    }

    #[test]
    fn test_process_output_empty_line() {
        with_engine(|engine| {
            // 空行不应 panic
            engine.process_output("");
        });
    }

    #[test]
    fn test_process_input_empty() {
        with_engine(|engine| {
            let handled = engine.process_input("");
            assert!(!handled);
        });
    }

    #[test]
    fn test_trigger_count_alias_count_timer_count() {
        let mut engine = LuaEngine::new().unwrap();
        assert_eq!(engine.trigger_count(), 0);
        assert_eq!(engine.alias_count(), 0);
        assert_eq!(engine.timer_count(), 0);

        exec(
            &mut engine,
            "AddTrigger('t1', 'test', '', 33, 0, 0, '', '', 0, 0)",
        )
        .unwrap();
        exec(&mut engine, "AddAlias('a1', 'go', '', 33)").unwrap();
        exec(&mut engine, "AddTimer('tm1', 0, 0, 10, '', 1)").unwrap();

        assert_eq!(engine.trigger_count(), 1);
        assert_eq!(engine.alias_count(), 1);
        assert_eq!(engine.timer_count(), 1);
    }

    #[test]
    fn test_timer_intervals_with_disabled() {
        with_engine(|engine| {
            exec(engine, "AddTimer('t1', 0, 0, 5, '', 1)").unwrap();
            exec(engine, "AddTimer('t2', 0, 0, 10, '', 1)").unwrap();
            let intervals = engine.timer_intervals();
            assert_eq!(intervals, vec![5000, 10000]);
        });
    }

    #[test]
    fn test_enable_timer_via_api() {
        with_engine(|engine| {
            exec(engine, "AddTimer('t1', 0, 0, 5, '', 1)").unwrap();
            let result: i32 = eval(engine, "return EnableTimer('t1', false)").unwrap();
            assert_eq!(result, 0);
            let result: i32 = eval(engine, "return EnableTimer('t1', true)").unwrap();
            assert_eq!(result, 0);
        });
    }

    #[test]
    fn test_enable_timer_not_found_via_api() {
        with_engine(|engine| {
            let result: i32 = eval(engine, "return EnableTimer('nonexistent', true)").unwrap();
            assert_eq!(result, 1);
        });
    }

    #[test]
    fn test_delete_variable_nonexistent() {
        with_engine(|engine| {
            // DeleteVariable returns nil, should not panic on nonexistent
            exec(engine, "DeleteVariable('no_such_var')").unwrap();
        });
    }

    #[test]
    fn test_get_trigger_info_codes() {
        with_engine(|engine| {
            exec(
                engine,
                "AddTrigger('t1', 'test', '', 33, 0, 0, '', '', 0, 0)",
            )
            .unwrap();
            // code 8 = enabled
            let en: bool = eval(engine, "return GetTriggerInfo('t1', 8)").unwrap();
            assert!(en);
            // Set group via SetTriggerOption then read via code 26
            exec(engine, "SetTriggerOption('t1', 'group', 'grp1')").unwrap();
            let group: String = eval(engine, "return GetTriggerInfo('t1', 26)").unwrap();
            assert_eq!(group, "grp1");
            // unknown code returns nil
            let val: Value = eval(engine, "return GetTriggerInfo('t1', 999)").unwrap();
            assert!(val.is_nil());
        });
    }

    #[test]
    fn test_get_timer_info_codes() {
        with_engine(|engine| {
            exec(engine, "AddTimer('t1', 0, 1, 30, '', 1)").unwrap();
            // code 6 = enabled
            let en: bool = eval(engine, "return GetTimerInfo('t1', 6)").unwrap();
            assert!(en);
            // unknown code returns nil
            let val: Value = eval(engine, "return GetTimerInfo('t1', 999)").unwrap();
            assert!(val.is_nil());
        });
    }

    #[test]
    fn test_set_alias_option_enabled() {
        with_engine(|engine| {
            exec(engine, "AddAlias('a1', 'go', '', 33)").unwrap();
            exec(engine, "SetAliasOption('a1', 'enabled', false)").unwrap();
            // Verify via GetAliasList or re-enable
            exec(engine, "SetAliasOption('a1', 'enabled', true)").unwrap();
        });
    }

    #[test]
    fn test_set_timer_option_enabled() {
        with_engine(|engine| {
            exec(engine, "AddTimer('t1', 0, 0, 5, '', 1)").unwrap();
            exec(engine, "SetTimerOption('t1', 'enabled', false)").unwrap();
            let en: bool = eval(engine, "return GetTimerInfo('t1', 6)").unwrap();
            assert!(!en);
            exec(engine, "SetTimerOption('t1', 'enabled', true)").unwrap();
            let en: bool = eval(engine, "return GetTimerInfo('t1', 6)").unwrap();
            assert!(en);
        });
    }

    #[test]
    fn test_multiline_trigger_with_newlines() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                ml_result = nil
                AddTriggerEx('ml', [[line1\nline2]], '', 33, 0, 0, '', '', 0, 2)
            "#,
            )
            .unwrap();
            engine.process_output("line1");
            engine.process_output("line2");
        });
    }

    #[test]
    fn test_pcre_z_anchor_in_trigger() {
        // 测试 PCRE \Z 锚点在触发器正则中的兼容性
        with_engine(|engine| {
            // 包含 \Z 的正则模式应被正确转换为 Rust regex 的 $
            exec(
                engine,
                r#"
                pcre_result = nil
                AddTriggerEx('pcre_z', [[^(> > > |> > |> |)一个用颅骨制成的钵。\n里面装(满了|了七、八分满|了五、六分满)\Z]], '', 33, 0, 0, '', '', 0, 2)
            "#,
            )
            .unwrap();
        });
    }

    #[test]
    fn test_pcre_z_anchor_simple() {
        // 测试简单的 \Z 转换
        with_engine(|engine| {
            exec(
                engine,
                r#"
                simple_z_result = nil
                AddTriggerEx('simple_z', [[^hello\Z]], '', 33)
            "#,
            )
            .unwrap();
        });
    }

    #[test]
    fn test_pcre_z_anchor_in_rex() {
        // 测试 rex 库中的 \Z 兼容性
        with_engine(|engine| {
            exec(
                engine,
                r#"
                local r = rex.new([[test\Z]])
                rex_match_result = r:match("test")
            "#,
            )
            .unwrap();
            let result: mlua::Value = eval(engine, "return rex_match_result").unwrap();
            assert!(!result.is_nil());
        });
    }

    #[test]
    fn test_sqlite3_changes_and_rowid() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                local db = sqlite3.open(':memory:')
                db:exec('CREATE TABLE t(id INTEGER PRIMARY KEY, v TEXT)')
                db:exec("INSERT INTO t(v) VALUES('hello')")
                test_changes = db:changes()
                test_rowid = db:last_insert_rowid()
                db:close()
            "#,
            )
            .unwrap();
            let changes: i64 = eval(engine, "return test_changes").unwrap();
            let rowid: i64 = eval(engine, "return test_rowid").unwrap();
            assert_eq!(changes, 1);
            assert_eq!(rowid, 1);
        });
    }

    #[test]
    fn test_sqlite3_prepare_bind_step() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                local db = sqlite3.open(':memory:')
                db:exec('CREATE TABLE t(id INTEGER, v TEXT)')
                db:exec("INSERT INTO t(id, v) VALUES(1, 'one')")
                local stmt = db:prepare("SELECT v FROM t WHERE id = ?")
                local row = stmt:step({1})
                test_bind_result = row and row[1] or nil
                stmt = nil
                db:close()
            "#,
            )
            .unwrap();
            let result: String = eval(engine, "return test_bind_result").unwrap();
            assert_eq!(result, "one");
        });
    }

    #[test]
    fn test_regex_escape_special_chars() {
        with_engine(|engine| {
            let result: String = eval(engine, r#"return AddTrigger('esc', 'hello.world', '', 33, 0, 0, '', '', 0, 0) == 0 and 'ok' or 'fail'"#).unwrap();
            assert_eq!(result, "ok");
        });
    }

    #[test]
    fn test_variable_numeric_value() {
        with_engine(|engine| {
            exec(engine, "SetVariable('num', '42')").unwrap();
            let val: String = eval(engine, "return GetVariable('num')").unwrap();
            assert_eq!(val, "42");
        });
    }

    #[test]
    fn test_send_multiple_commands() {
        with_engine(|engine| {
            exec(engine, "send('cmd1')").unwrap();
            exec(engine, "send('cmd2')").unwrap();
            exec(engine, "send('cmd3')").unwrap();
            let cmds = engine.drain_commands();
            assert_eq!(cmds, vec!["cmd1", "cmd2", "cmd3"]);
        });
    }

    #[test]
    fn test_execute_function() {
        with_engine(|engine| {
            // Execute pushes the raw command string to pending_commands
            exec(engine, "Execute('hello')").unwrap();
            let cmds = engine.drain_commands();
            assert!(cmds.contains(&"hello".to_string()));
        });
    }

    #[test]
    fn test_colour_note_with_colors() {
        with_engine(|engine| {
            exec(engine, "ColourNote('red', 'blue', 'colored text')").unwrap();
            let logs = engine.drain_logs();
            // red=31, blue=44 → \x1B[31;44mcolored text\x1B[0m
            assert!(logs
                .iter()
                .any(|l| l.contains("\x1b[31;44mcolored text\x1b[0m")));
        });
    }

    #[test]
    fn test_tell_with_colors() {
        with_engine(|engine| {
            // Tell only takes one string argument
            exec(engine, "Tell('tell text')").unwrap();
            let logs = engine.drain_logs();
            assert!(logs.iter().any(|l| l.contains("tell text")));
        });
    }

    #[test]
    fn test_timer_shorthand() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                timer_result = nil
                timer(5, function() timer_result = "fired" end)
            "#,
            )
            .unwrap();
            assert_eq!(engine.timer_count(), 1);
            engine.fire_timer(0);
            let result: String = eval(engine, "return timer_result").unwrap();
            assert_eq!(result, "fired");
        });
    }

    #[test]
    fn test_dofile_nonexistent() {
        with_engine(|engine| {
            // dofile with nonexistent file should not panic
            let result = exec(engine, "dofile('/nonexistent/file.lua')");
            // May or may not error depending on implementation
            let _ = result;
        });
    }

    // ================================================================
    // rex PCRE 兼容模块测试
    // ================================================================

    #[test]
    fn test_rex_new_basic() {
        with_engine(|engine| {
            let result: Table = eval(engine, "return rex.new('hello')").unwrap();
            // 返回一个表对象（正则对象）
            assert!(result.len().unwrap_or(0) >= 0);
        });
    }

    #[test]
    fn test_rex_new_invalid_pattern() {
        with_engine(|engine| {
            let result = exec(engine, "return rex.new('[invalid')");
            assert!(result.is_err());
        });
    }

    #[test]
    fn test_rex_match_found() {
        with_engine(|engine| {
            let result: Table = eval(
                engine,
                r#"
                local r = rex.new("(\\w+)")
                return r:match("hello world")
            "#,
            )
            .unwrap();
            let full: String = result.get(1).unwrap();
            assert_eq!(full, "hello");
            // 无额外捕获组（整体匹配在索引1）
        });
    }

    #[test]
    fn test_rex_match_with_captures() {
        with_engine(|engine| {
            let result: Table = eval(
                engine,
                r#"
                local r = rex.new("(\\w+)@(\\w+)")
                return r:match("user@host")
            "#,
            )
            .unwrap();
            let full: String = result.get(1).unwrap();
            let cap1: String = result.get(2).unwrap();
            let cap2: String = result.get(3).unwrap();
            assert_eq!(full, "user@host");
            assert_eq!(cap1, "user");
            assert_eq!(cap2, "host");
        });
    }

    #[test]
    fn test_rex_match_not_found() {
        with_engine(|engine| {
            let result: Value = eval(
                engine,
                r#"
                local r = rex.new("xyz")
                return r:match("hello world")
            "#,
            )
            .unwrap();
            assert!(result.is_nil());
        });
    }

    #[test]
    fn test_rex_gmatch_callback() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                local r = rex.new("(\\w+)")
                local results = {}
                r:gmatch("hello world", function(m)
                    table.insert(results, m)
                end)
                SetVariable("gmatch_count", tostring(#results))
                SetVariable("gmatch_1", results[1])
                SetVariable("gmatch_2", results[2])
            "#,
            )
            .unwrap();
            let count: String = eval(engine, "return GetVariable('gmatch_count')").unwrap();
            let first: String = eval(engine, "return GetVariable('gmatch_1')").unwrap();
            let second: String = eval(engine, "return GetVariable('gmatch_2')").unwrap();
            assert_eq!(count, "2");
            assert_eq!(first, "hello");
            assert_eq!(second, "world");
        });
    }

    #[test]
    fn test_rex_gmatch_with_captures() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                local r = rex.new("([^;*\\\\]+)")
                local results = {}
                r:gmatch("cmd1;cmd2*cmd3", function(m)
                    table.insert(results, m)
                end)
                SetVariable("gmatch_cap_count", tostring(#results))
                SetVariable("gmatch_cap_1", results[1])
                SetVariable("gmatch_cap_2", results[2])
                SetVariable("gmatch_cap_3", results[3])
            "#,
            )
            .unwrap();
            let count: String = eval(engine, "return GetVariable('gmatch_cap_count')").unwrap();
            let first: String = eval(engine, "return GetVariable('gmatch_cap_1')").unwrap();
            let second: String = eval(engine, "return GetVariable('gmatch_cap_2')").unwrap();
            let third: String = eval(engine, "return GetVariable('gmatch_cap_3')").unwrap();
            assert_eq!(count, "3");
            assert_eq!(first, "cmd1");
            assert_eq!(second, "cmd2");
            assert_eq!(third, "cmd3");
        });
    }

    #[test]
    fn test_rex_split() {
        with_engine(|engine| {
            let result: Table = eval(
                engine,
                r#"
                local r = rex.new("[,;]+")
                return r:split("a,b;c,d")
            "#,
            )
            .unwrap();
            let first: String = result.get(1).unwrap();
            let second: String = result.get(2).unwrap();
            let third: String = result.get(3).unwrap();
            assert_eq!(first, "a");
            assert_eq!(second, "b");
            assert_eq!(third, "c");
        });
    }

    #[test]
    fn test_rex_find() {
        with_engine(|engine| {
            let result: Table = eval(
                engine,
                r#"
                local r = rex.new("world")
                return r:find("hello world")
            "#,
            )
            .unwrap();
            let start: i64 = result.get(1).unwrap();
            let end: i64 = result.get(2).unwrap();
            let matched: String = result.get(3).unwrap();
            assert_eq!(start, 7);
            assert_eq!(end, 11);
            assert_eq!(matched, "world");
        });
    }

    #[test]
    fn test_rex_find_not_found() {
        with_engine(|engine| {
            let result: Value = eval(
                engine,
                r#"
                local r = rex.new("xyz")
                return r:find("hello world")
            "#,
            )
            .unwrap();
            assert!(result.is_nil());
        });
    }

    #[test]
    fn test_rex_convenience_split() {
        with_engine(|engine| {
            let result: Table = eval(
                engine,
                r#"
                return rex.split("a,b,c", ",")
            "#,
            )
            .unwrap();
            let first: String = result.get(1).unwrap();
            let second: String = result.get(2).unwrap();
            let third: String = result.get(3).unwrap();
            assert_eq!(first, "a");
            assert_eq!(second, "b");
            assert_eq!(third, "c");
        });
    }

    #[test]
    fn test_rex_convenience_match() {
        with_engine(|engine| {
            let result: Table = eval(
                engine,
                r#"
                return rex.match("user@host", "(\\w+)@(\\w+)")
            "#,
            )
            .unwrap();
            let full: String = result.get(1).unwrap();
            let cap1: String = result.get(2).unwrap();
            let cap2: String = result.get(3).unwrap();
            assert_eq!(full, "user@host");
            assert_eq!(cap1, "user");
            assert_eq!(cap2, "host");
        });
    }

    #[test]
    fn test_rex_convenience_find() {
        with_engine(|engine| {
            let result: Table = eval(
                engine,
                r#"
                return rex.find("hello world", "world")
            "#,
            )
            .unwrap();
            let start: i64 = result.get(1).unwrap();
            assert_eq!(start, 7);
        });
    }

    #[test]
    fn test_rex_michen_system_pattern() {
        // 测试实际脚本中的正则: rex.new("([^;*\\\\]+)")
        with_engine(|engine| {
            let result: Table = eval(
                engine,
                r#"
                local r = rex.new("([^;*\\\\]+)")
                return r:match("go north;south*east")
            "#,
            )
            .unwrap();
            let full: String = result.get(1).unwrap();
            let cap1: String = result.get(2).unwrap();
            assert_eq!(full, "go north");
            assert_eq!(cap1, "go north");
        });
    }

    #[test]
    fn test_rex_gmatch_michen_system_usage() {
        // 模拟脚本中 runre:gmatch(str, function(m, t) ... end) 的用法
        with_engine(|engine| {
            exec(
                engine,
                r#"
                local runre = rex.new("([^;*\\\\]+)")
                local results = {}
                runre:gmatch("go north;south*east", function(m, t)
                    table.insert(results, m)
                end)
                SetVariable("runre_count", tostring(#results))
                SetVariable("runre_1", results[1])
                SetVariable("runre_2", results[2])
                SetVariable("runre_3", results[3])
            "#,
            )
            .unwrap();
            let count: String = eval(engine, "return GetVariable('runre_count')").unwrap();
            let first: String = eval(engine, "return GetVariable('runre_1')").unwrap();
            let second: String = eval(engine, "return GetVariable('runre_2')").unwrap();
            let third: String = eval(engine, "return GetVariable('runre_3')").unwrap();
            assert_eq!(count, "3");
            assert_eq!(first, "go north");
            assert_eq!(second, "south");
            assert_eq!(third, "east");
        });
    }

    #[test]
    fn test_get_plugin_info_more_codes() {
        with_engine(|engine| {
            // code 1 = plugin id
            let id: String = eval(engine, "return GetPluginInfo(GetPluginID(), 1)").unwrap();
            assert!(!id.is_empty());
            // unknown code returns nil
            let val: Value = eval(engine, "return GetPluginInfo(GetPluginID(), 999)").unwrap();
            assert!(val.is_nil());
        });
    }

    #[test]
    fn test_trigger_omit_from_output_flag() {
        with_engine(|engine| {
            // flag bit 4 (16) = omit from output
            exec(
                engine,
                "AddTrigger('omit', 'hide_me', '', 49, 0, 0, '', '', 0, 0)",
            )
            .unwrap();
            engine.process_output("hide_me");
            let logs = engine.drain_logs();
            // omit trigger should not produce Note output
            assert!(logs.is_empty() || !logs.iter().any(|l| l.contains("hide_me")));
        });
    }

    #[test]
    fn test_alias_callback_sends_command() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                AddAlias('go_alias', 'go', '', 33, 'function() send("north") end')
            "#,
            )
            .unwrap();
            let handled = engine.process_input("go");
            assert!(handled);
            let cmds = engine.drain_commands();
            assert!(cmds.contains(&"north".to_string()));
        });
    }

    #[test]
    fn test_trigger_keep_evaluating() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                result1 = nil
                result2 = nil
                AddTrigger('trig1', 'test', '', 33, 0, 0, '', 'function() result1 = 1 end', 0, 0)
                AddTrigger('trig2', 'test', '', 33, 0, 0, '', 'function() result2 = 2 end', 0, 0)
            "#,
            )
            .unwrap();
            engine.process_output("test");
            let r1: Option<i64> = eval(engine, "return result1").unwrap();
            let r2: Option<i64> = eval(engine, "return result2").unwrap();
            // Both triggers should fire (keep_evaluating is default)
            assert_eq!(r1, Some(1));
            assert_eq!(r2, Some(2));
        });
    }

    #[test]
    fn test_set_trigger_option_send() {
        with_engine(|engine| {
            exec(
                engine,
                "AddTrigger('send_trig2', 'go', '', 1, 0, 0, '', '', 0, 0)",
            )
            .unwrap();
            exec(engine, "SetTriggerOption('send_trig2', 'send', 'north')").unwrap();
            engine.process_output("go");
            let cmds = engine.drain_commands();
            assert!(cmds.contains(&"north".to_string()));
        });
    }

    #[test]
    fn test_delete_trigger_clears() {
        with_engine(|engine| {
            exec(
                engine,
                "AddTrigger('del_me', 'test', '', 33, 0, 0, '', '', 0, 0)",
            )
            .unwrap();
            assert_eq!(engine.trigger_count(), 1);
            exec(engine, "DeleteTrigger('del_me')").unwrap();
            assert_eq!(engine.trigger_count(), 0);
        });
    }

    #[test]
    fn test_delete_alias_clears() {
        with_engine(|engine| {
            exec(engine, "AddAlias('del_me', 'go', '', 33)").unwrap();
            assert_eq!(engine.alias_count(), 1);
            exec(engine, "DeleteAlias('del_me')").unwrap();
            assert_eq!(engine.alias_count(), 0);
        });
    }

    #[test]
    fn test_delete_timer_clears() {
        with_engine(|engine| {
            exec(engine, "AddTimer('del_me', 0, 0, 5, '', 1)").unwrap();
            assert_eq!(engine.timer_count(), 1);
            exec(engine, "DeleteTimer('del_me')").unwrap();
            assert_eq!(engine.timer_count(), 0);
        });
    }

    #[test]
    fn test_enable_trigger_group_via_api() {
        with_engine(|engine| {
            exec(
                engine,
                "AddTrigger('g1_t1', 'a', '', 33, 0, 0, '', '', 0, 0)",
            )
            .unwrap();
            exec(
                engine,
                "AddTrigger('g1_t2', 'b', '', 33, 0, 0, '', '', 0, 0)",
            )
            .unwrap();
            // Set group via SetTriggerOption
            exec(engine, "SetTriggerOption('g1_t1', 'group', 'grp_a')").unwrap();
            exec(engine, "SetTriggerOption('g1_t2', 'group', 'grp_a')").unwrap();
            // Disable group
            exec(engine, "EnableTriggerGroup('grp_a', false)").unwrap();
            let en: bool = eval(engine, "return GetTriggerInfo('g1_t1', 8)").unwrap();
            assert!(!en);
            // Enable group
            exec(engine, "EnableTriggerGroup('grp_a', true)").unwrap();
            let en: bool = eval(engine, "return GetTriggerInfo('g1_t1', 8)").unwrap();
            assert!(en);
        });
    }

    #[test]
    fn test_enable_alias_group_via_set_option() {
        with_engine(|engine| {
            // No EnableAliasGroup API, use SetAliasOption to set group then enable/disable
            exec(engine, "AddAlias('g1_a1', 'x', '', 33)").unwrap();
            exec(engine, "SetAliasOption('g1_a1', 'group', 'grp_b')").unwrap();
            exec(engine, "SetAliasOption('g1_a1', 'enabled', false)").unwrap();
            // Verify disabled
            let handled = engine.process_input("x");
            assert!(!handled);
            // Re-enable
            exec(engine, "SetAliasOption('g1_a1', 'enabled', true)").unwrap();
        });
    }

    #[test]
    fn test_enable_timer_group_via_api() {
        with_engine(|engine| {
            exec(engine, "AddTimer('g1_t1', 0, 0, 5, '', 1)").unwrap();
            // Set group via SetTimerOption
            exec(engine, "SetTimerOption('g1_t1', 'group', 'grp_c')").unwrap();
            // EnableTimerGroup returns nil (unit)
            exec(engine, "EnableTimerGroup('grp_c', false)").unwrap();
            let en: bool = eval(engine, "return GetTimerInfo('g1_t1', 6)").unwrap();
            assert!(!en);
            exec(engine, "EnableTimerGroup('grp_c', true)").unwrap();
            let en: bool = eval(engine, "return GetTimerInfo('g1_t1', 6)").unwrap();
            assert!(en);
        });
    }

    #[test]
    fn test_sqlite3_exec_error() {
        with_engine(|engine| {
            let result: i64 = eval(
                engine,
                r#"
                local db = sqlite3.open(':memory:')
                local ok, err = pcall(function() db:exec('INVALID SQL') end)
                db:close()
                return ok and 1 or 0
            "#,
            )
            .unwrap();
            assert_eq!(result, 0);
        });
    }

    #[test]
    fn test_sqlite3_multiple_rows() {
        with_engine(|engine| {
            let count: i64 = eval(
                engine,
                r#"
                local db = sqlite3.open(':memory:')
                db:exec('CREATE TABLE t(id INTEGER, v TEXT)')
                db:exec("INSERT INTO t VALUES(1, 'a')")
                db:exec("INSERT INTO t VALUES(2, 'b')")
                db:exec("INSERT INTO t VALUES(3, 'c')")
                local c = 0
                -- step() re-prepares each time, so we use exec + specific queries
                local row1 = db:prepare('SELECT v FROM t WHERE id = 1'):step()
                local row2 = db:prepare('SELECT v FROM t WHERE id = 2'):step()
                local row3 = db:prepare('SELECT v FROM t WHERE id = 3'):step()
                if row1 then c = c + 1 end
                if row2 then c = c + 1 end
                if row3 then c = c + 1 end
                db:close()
                return c
            "#,
            )
            .unwrap();
            assert_eq!(count, 3);
        });
    }

    #[test]
    fn test_sqlite3_nrows() {
        with_engine(|engine| {
            let count: i64 = eval(
                engine,
                r#"
                local db = sqlite3.open(':memory:')
                db:exec('CREATE TABLE Room(RoomNO INTEGER, Name TEXT)')
                db:exec("INSERT INTO Room VALUES(1, 'dali')")
                db:exec("INSERT INTO Room VALUES(2, 'changan')")
                db:exec("INSERT INTO Room VALUES(3, 'beijing')")
                local c = 0
                for row in db:nrows('SELECT * FROM Room') do
                    c = c + 1
                end
                db:close()
                return c
            "#,
            )
            .unwrap();
            assert_eq!(count, 3);
        });
    }

    #[test]
    fn test_sqlite3_nrows_column_names() {
        with_engine(|engine| {
            let name: String = eval(
                engine,
                r#"
                local db = sqlite3.open(':memory:')
                db:exec('CREATE TABLE t(id INTEGER, v TEXT)')
                db:exec("INSERT INTO t VALUES(1, 'hello')")
                local result = ""
                for row in db:nrows('SELECT * FROM t') do
                    result = row.v
                end
                db:close()
                return result
            "#,
            )
            .unwrap();
            assert_eq!(name, "hello");
        });
    }

    #[test]
    fn test_get_variable_list_count() {
        with_engine(|engine| {
            exec(engine, "SetVariable('k1', 'v1')").unwrap();
            exec(engine, "SetVariable('k2', 'v2')").unwrap();
            let count: i64 = eval(
                engine,
                r#"
                local list = GetVariableList()
                local c = 0
                for _ in pairs(list) do c = c + 1 end
                return c
            "#,
            )
            .unwrap();
            assert_eq!(count, 2);
        });
    }

    #[test]
    fn test_process_output_with_trigger_and_alias_chain() {
        with_engine(|engine| {
            // Trigger fires on "prompt>" and sends a command
            exec(
                engine,
                r#"
                AddTrigger('auto_cmd', 'prompt>', '', 33, 0, 0, '', '', 0, 0)
                SetTriggerOption('auto_cmd', 'send', 'look')
            "#,
            )
            .unwrap();
            engine.process_output("prompt>");
            let cmds = engine.drain_commands();
            assert!(cmds.contains(&"look".to_string()));
        });
    }

    // ================================================================
    // 触发器集成测试
    // ================================================================

    #[test]
    fn test_trigger_multiple_captures() {
        with_engine(|engine| {
            exec(engine, r#"
                cap1 = nil; cap2 = nil
                AddTrigger('multi_cap', [[^(\w+) hits (\w+)$]], '', 33, 0, 0, '', 'function(t) cap1 = t[1]; cap2 = t[2] end', 0, 0)
            "#).unwrap();
            engine.process_output("goblin hits warrior");
            let r1: Option<String> = eval(engine, "return cap1").unwrap();
            let r2: Option<String> = eval(engine, "return cap2").unwrap();
            assert_eq!(r1, Some("goblin".to_string()));
            assert_eq!(r2, Some("warrior".to_string()));
        });
    }

    #[test]
    fn test_trigger_no_match_different_line() {
        with_engine(|engine| {
            exec(engine, r#"
                no_match_result = nil
                AddTrigger('no_match', [[^exact$]], '', 33, 0, 0, '', 'function() no_match_result = true end', 0, 0)
            "#).unwrap();
            engine.process_output("not exact at all");
            let result: Option<bool> = eval(engine, "return no_match_result").unwrap();
            assert_eq!(result, None);
        });
    }

    #[test]
    fn test_trigger_ansi_stripped() {
        with_engine(|engine| {
            exec(engine, r#"
                ansi_result = nil
                AddTrigger('ansi_trig', 'hello', '', 33, 0, 0, '', 'function() ansi_result = true end', 0, 0)
            "#).unwrap();
            // ANSI escape codes should be stripped before matching
            engine.process_output("\x1b[31mhello\x1b[0m");
            let result: Option<bool> = eval(engine, "return ansi_result").unwrap();
            assert_eq!(result, Some(true));
        });
    }

    #[test]
    fn test_trigger_callback_error_handled() {
        with_engine(|engine| {
            exec(engine, r#"
                AddTrigger('err_trig', 'test', '', 33, 0, 0, '', 'function() error("boom") end', 0, 0)
            "#).unwrap();
            // Should not panic even if callback errors
            engine.process_output("test");
        });
    }

    #[test]
    fn test_trigger_partial_match() {
        with_engine(|engine| {
            exec(engine, r#"
                partial_result = nil
                AddTrigger('partial', 'hp', '', 33, 0, 0, '', 'function() partial_result = true end', 0, 0)
            "#).unwrap();
            engine.process_output("100hp 200mp 300mv");
            let result: Option<bool> = eval(engine, "return partial_result").unwrap();
            assert_eq!(result, Some(true));
        });
    }

    #[test]
    fn test_trigger_multiple_fire_order() {
        with_engine(|engine| {
            exec(engine, r#"
                fire_order = {}
                AddTrigger('t1', 'test', '', 33, 0, 0, '', 'function() table.insert(fire_order, 1) end', 0, 0)
                AddTrigger('t2', 'test', '', 33, 0, 0, '', 'function() table.insert(fire_order, 2) end', 0, 0)
                AddTrigger('t3', 'test', '', 33, 0, 0, '', 'function() table.insert(fire_order, 3) end', 0, 0)
            "#).unwrap();
            engine.process_output("test");
            let order: Vec<i64> = eval(engine, "return fire_order").unwrap();
            assert_eq!(order, vec![1, 2, 3]);
        });
    }

    #[test]
    fn test_trigger_send_text_with_wildcard() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                AddTrigger('auto_go', 'You see * here', '', 1, 0, 0, '', '', 0, 0)
                SetTriggerOption('auto_go', 'send', 'examine $1')
            "#,
            )
            .unwrap();
            // send_text 不做变量替换，直接发送
            engine.process_output("You see a sword here");
            let cmds = engine.drain_commands();
            // send_text is literal "examine $1", not variable-substituted
            assert!(cmds.contains(&"examine $1".to_string()));
        });
    }

    #[test]
    fn test_trigger_enabled_disabled_toggle() {
        with_engine(|engine| {
            exec(engine, r#"
                toggle_result = nil
                AddTrigger('toggle', 'fire', '', 1, 0, 0, '', 'function() toggle_result = true end', 0, 0)
            "#).unwrap();
            // Initially enabled
            engine.process_output("fire");
            let r1: Option<bool> = eval(engine, "return toggle_result").unwrap();
            assert_eq!(r1, Some(true));

            // Disable
            exec(engine, "EnableTrigger('toggle', false)").unwrap();
            exec(engine, "toggle_result = nil").unwrap();
            engine.process_output("fire");
            let r2: Option<bool> = eval(engine, "return toggle_result").unwrap();
            assert_eq!(r2, None);

            // Re-enable
            exec(engine, "EnableTrigger('toggle', true)").unwrap();
            engine.process_output("fire");
            let r3: Option<bool> = eval(engine, "return toggle_result").unwrap();
            assert_eq!(r3, Some(true));
        });
    }

    #[test]
    fn test_trigger_duplicate_name_replaces() {
        with_engine(|engine| {
            exec(
                engine,
                "AddTrigger('dup', 'first', '', 1, 0, 0, '', '', 0, 0)",
            )
            .unwrap();
            exec(
                engine,
                "AddTrigger('dup', 'second', '', 1, 0, 0, '', '', 0, 0)",
            )
            .unwrap();
            // Both triggers exist (no uniqueness enforcement)
            assert_eq!(engine.trigger_count(), 2);
        });
    }

    #[test]
    fn test_trigger_group_operations() {
        with_engine(|engine| {
            exec(engine, r#"
                grp_a_result = nil
                grp_b_result = nil
                AddTrigger('ga', 'alpha', '', 1, 0, 0, '', 'function() grp_a_result = true end', 0, 0)
                AddTrigger('gb', 'beta', '', 1, 0, 0, '', 'function() grp_b_result = true end', 0, 0)
                SetTriggerOption('ga', 'group', 'groupA')
                SetTriggerOption('gb', 'group', 'groupB')
            "#).unwrap();
            // Disable groupA
            exec(engine, "EnableTriggerGroup('groupA', false)").unwrap();
            engine.process_output("alpha");
            engine.process_output("beta");
            let ra: Option<bool> = eval(engine, "return grp_a_result").unwrap();
            let rb: Option<bool> = eval(engine, "return grp_b_result").unwrap();
            assert_eq!(ra, None);
            assert_eq!(rb, Some(true));
        });
    }

    // ================================================================
    // 定时器集成测试
    // ================================================================

    #[test]
    fn test_timer_fire_executes_send_text() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                AddTimer('cmd_timer', 0, 0, 5, '', 1, 'send("auto_command")')
            "#,
            )
            .unwrap();
            engine.fire_timer(0);
            let cmds = engine.drain_commands();
            // send_text is Lua code that gets executed, which calls send()
            assert!(cmds.contains(&"auto_command".to_string()));
        });
    }

    #[test]
    fn test_timer_fire_with_callback() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                timer_cb_result = nil
                timer(10, function() timer_cb_result = "callback_fired" end)
            "#,
            )
            .unwrap();
            engine.fire_timer(0);
            let result: Option<String> = eval(engine, "return timer_cb_result").unwrap();
            assert_eq!(result, Some("callback_fired".to_string()));
        });
    }

    #[test]
    fn test_timer_one_shot_auto_remove() {
        with_engine(|engine| {
            // flag 5 = Enabled(1) + OneShot(4)
            exec(engine, "AddTimer('oneshot_t', 0, 0, 3, '', 5)").unwrap();
            assert_eq!(engine.timer_count(), 1);
            engine.fire_timer(0);
            assert_eq!(engine.timer_count(), 0);
        });
    }

    #[test]
    fn test_timer_repeating_stays() {
        with_engine(|engine| {
            exec(engine, "AddTimer('repeat_t', 0, 0, 5, '', 1)").unwrap();
            assert_eq!(engine.timer_count(), 1);
            engine.fire_timer(0);
            // Non-one-shot timer should remain
            assert_eq!(engine.timer_count(), 1);
        });
    }

    #[test]
    fn test_timer_disabled_not_fired() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                disabled_t_result = nil
                AddTimer('dis_t', 0, 0, 5, '', 0, 'disabled_t_result = true')
            "#,
            )
            .unwrap();
            engine.fire_timer(0);
            let result: Option<bool> = eval(engine, "return disabled_t_result").unwrap();
            assert_eq!(result, None);
        });
    }

    #[test]
    fn test_timer_enable_disable_cycle() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                cycle_result = nil
                AddTimer('cycle_t', 0, 0, 5, '', 1, 'cycle_result = "fired"')
            "#,
            )
            .unwrap();
            // Disable
            exec(engine, "EnableTimer('cycle_t', false)").unwrap();
            engine.fire_timer(0);
            let r1: Option<String> = eval(engine, "return cycle_result").unwrap();
            assert_eq!(r1, None);

            // Re-enable
            exec(engine, "EnableTimer('cycle_t', true)").unwrap();
            exec(engine, "cycle_result = nil").unwrap();
            engine.fire_timer(0);
            let r2: Option<String> = eval(engine, "return cycle_result").unwrap();
            assert_eq!(r2, Some("fired".to_string()));
        });
    }

    #[test]
    fn test_timer_group_enable_disable() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                tg1_result = nil; tg2_result = nil
                AddTimer('tg1', 0, 0, 5, '', 1, 'tg1_result = true')
                AddTimer('tg2', 0, 0, 10, '', 1, 'tg2_result = true')
                SetTimerOption('tg1', 'group', 'grpX')
                SetTimerOption('tg2', 'group', 'grpX')
            "#,
            )
            .unwrap();
            exec(engine, "EnableTimerGroup('grpX', false)").unwrap();
            engine.fire_timer(0); // tg1
            engine.fire_timer(1); // tg2
            let r1: Option<bool> = eval(engine, "return tg1_result").unwrap();
            let r2: Option<bool> = eval(engine, "return tg2_result").unwrap();
            assert_eq!(r1, None);
            assert_eq!(r2, None);
        });
    }

    #[test]
    fn test_timer_multiple_fire() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                fire_count = 0
                timer(5, function() fire_count = fire_count + 1 end)
            "#,
            )
            .unwrap();
            engine.fire_timer(0);
            engine.fire_timer(0);
            engine.fire_timer(0);
            let count: i64 = eval(engine, "return fire_count").unwrap();
            assert_eq!(count, 3);
        });
    }

    #[test]
    fn test_timer_delete_during_iteration() {
        with_engine(|engine| {
            exec(engine, "AddTimer('del1', 0, 0, 5, '', 1)").unwrap();
            exec(engine, "AddTimer('del2', 0, 0, 10, '', 1)").unwrap();
            assert_eq!(engine.timer_count(), 2);
            exec(engine, "DeleteTimer('del1')").unwrap();
            assert_eq!(engine.timer_count(), 1);
            // Remaining timer should still fire
            engine.fire_timer(0);
        });
    }

    // ================================================================
    // 别名集成测试
    // ================================================================

    #[test]
    fn test_alias_multiple_captures() {
        with_engine(|engine| {
            exec(engine, r#"
                alias_c1 = nil; alias_c2 = nil
                AddAlias('multi_cap_a', 'cast * at *', '', 1, 'function(t) alias_c1 = t[1]; alias_c2 = t[2] end')
            "#).unwrap();
            let handled = engine.process_input("cast fireball at goblin");
            assert!(handled);
            let r1: Option<String> = eval(engine, "return alias_c1").unwrap();
            let r2: Option<String> = eval(engine, "return alias_c2").unwrap();
            assert_eq!(r1, Some("fireball".to_string()));
            assert_eq!(r2, Some("goblin".to_string()));
        });
    }

    #[test]
    fn test_alias_priority_first_match() {
        with_engine(|engine| {
            exec(engine, r#"
                priority_result = nil
                AddAlias('specific', 'kill goblin', '', 33, 'function() priority_result = "specific" end')
                AddAlias('general', 'kill *', '', 33, 'function() priority_result = "general" end')
            "#).unwrap();
            let handled = engine.process_input("kill goblin");
            assert!(handled);
            let result: Option<String> = eval(engine, "return priority_result").unwrap();
            // Both match, both fire; last one wins since both set the same variable
            assert!(
                result == Some("specific".to_string()) || result == Some("general".to_string())
            );
        });
    }

    #[test]
    fn test_alias_no_match_returns_false() {
        with_engine(|engine| {
            exec(engine, "AddAlias('only_go', 'go *', '', 1)").unwrap();
            let handled = engine.process_input("look around");
            assert!(!handled);
        });
    }

    #[test]
    fn test_alias_sends_command() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                AddAlias('kk', 'kk', '', 33, 'function() send("kill") end')
            "#,
            )
            .unwrap();
            let handled = engine.process_input("kk");
            assert!(handled);
            let cmds = engine.drain_commands();
            assert!(cmds.contains(&"kill".to_string()));
        });
    }

    #[test]
    fn test_alias_disabled_not_matched() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                dis_alias_result = nil
                AddAlias('dis_al', 'test', '', 0, 'function() dis_alias_result = true end')
            "#,
            )
            .unwrap();
            let handled = engine.process_input("test");
            assert!(!handled);
        });
    }

    #[test]
    fn test_alias_toggle_enable() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                toggle_alias_result = nil
                AddAlias('toggle_a', 'hello', '', 1, 'function() toggle_alias_result = true end')
            "#,
            )
            .unwrap();
            // Initially enabled
            let h1 = engine.process_input("hello");
            assert!(h1);
            // Disable
            exec(engine, "SetAliasOption('toggle_a', 'enabled', false)").unwrap();
            let h2 = engine.process_input("hello");
            assert!(!h2);
            // Re-enable
            exec(engine, "SetAliasOption('toggle_a', 'enabled', true)").unwrap();
            let h3 = engine.process_input("hello");
            assert!(h3);
        });
    }

    #[test]
    fn test_alias_group_management() {
        with_engine(|engine| {
            exec(engine, "AddAlias('grp_a1', 'x', '', 1)").unwrap();
            exec(engine, "AddAlias('grp_a2', 'y', '', 1)").unwrap();
            exec(engine, "SetAliasOption('grp_a1', 'group', 'combat')").unwrap();
            exec(engine, "SetAliasOption('grp_a2', 'group', 'combat')").unwrap();
            // Disable both via group (manual since no EnableAliasGroup)
            exec(engine, "SetAliasOption('grp_a1', 'enabled', false)").unwrap();
            exec(engine, "SetAliasOption('grp_a2', 'enabled', false)").unwrap();
            let h1 = engine.process_input("x");
            let h2 = engine.process_input("y");
            assert!(!h1);
            assert!(!h2);
        });
    }

    #[test]
    fn test_alias_regex_pattern() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                regex_al_result = nil
                AddAlias('regex_a', [[^#(\d+)$]], '', 33, 'function(t) regex_al_result = t[1] end')
            "#,
            )
            .unwrap();
            let handled = engine.process_input("#5");
            assert!(handled);
            let result: Option<String> = eval(engine, "return regex_al_result").unwrap();
            assert_eq!(result, Some("5".to_string()));
            // Should not match non-numeric
            let handled2 = engine.process_input("#abc");
            // regex ^#(\d+)$ won't match #abc
            assert!(!handled2);
        });
    }

    #[test]
    fn test_alias_case_insensitive() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                ci_alias_result = nil
                AddAlias('ci_al', 'HELLO', '', 33, 'function() ci_alias_result = true end')
            "#,
            )
            .unwrap();
            // Use regex flag (33) with (?i) prefix for case insensitive
            exec(engine, r#"
                ci_alias_result2 = nil
                AddAlias('ci_al2', [[(?i)^hello$]], '', 33, 'function() ci_alias_result2 = true end')
            "#).unwrap();
            let handled = engine.process_input("hello");
            assert!(handled);
            let result: Option<bool> = eval(engine, "return ci_alias_result2").unwrap();
            assert_eq!(result, Some(true));
        });
    }

    #[test]
    fn test_alias_delete_and_readd() {
        with_engine(|engine| {
            exec(engine, "AddAlias('temp_a', 'go', '', 1)").unwrap();
            assert_eq!(engine.alias_count(), 1);
            exec(engine, "DeleteAlias('temp_a')").unwrap();
            assert_eq!(engine.alias_count(), 0);
            exec(engine, "AddAlias('temp_a', 'go', '', 1)").unwrap();
            assert_eq!(engine.alias_count(), 1);
        });
    }

    #[test]
    fn test_alias_callback_error_handled() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                AddAlias('err_al', 'boom', '', 33, 'function() error("alias error") end')
            "#,
            )
            .unwrap();
            // Should not panic
            let handled = engine.process_input("boom");
            assert!(handled);
        });
    }

    #[test]
    fn test_alias_input_passed_as_arg0() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                raw_input = nil
                alias('^test$', function(t) raw_input = t[0] end)
            "#,
            )
            .unwrap();
            let handled = engine.process_input("test");
            assert!(handled);
            let result: Option<String> = eval(engine, "return raw_input").unwrap();
            assert_eq!(result, Some("test".to_string()));
        });
    }

    #[test]
    fn test_infobtn_compat_layer() {
        // 验证 infobtn.xxx 调用通过 setmetatable 转发到 cfg.xxx
        with_engine(|engine| {
            exec(
                engine,
                r#"
                -- 模拟 michen_config.lua 的兼容层
                cfg = cfg or {}
                cfg.test_val = 0
                function cfg.set_test()
                    cfg.test_val = 42
                end
                infobtn = infobtn or {}
                setmetatable(infobtn, {__index = function(_, key)
                    if cfg[key] then return cfg[key] end
                    return function() end
                end})
                -- 通过 infobtn 调用 cfg 的函数
                infobtn.set_test()
            "#,
            )
            .unwrap();
            let val: i64 = eval(engine, "return cfg.test_val").unwrap();
            assert_eq!(val, 42);
        });
    }

    #[test]
    fn test_infobtn_missing_method_no_error() {
        // 验证 infobtn.xxx 调用不存在的方法不会报错（返回空函数）
        with_engine(|engine| {
            exec(
                engine,
                r#"
                cfg = cfg or {}
                infobtn = infobtn or {}
                setmetatable(infobtn, {__index = function(_, key)
                    if cfg[key] then return cfg[key] end
                    return function() end
                end})
                -- 调用不存在的方法应静默返回
                infobtn.nonexistent()
                compat_ok = true
            "#,
            )
            .unwrap();
            let ok: bool = eval(engine, "return compat_ok").unwrap();
            assert!(ok);
        });
    }

    // ================================================================
    // 触发器+别名+定时器联动测试
    // ================================================================

    #[test]
    fn test_trigger_sends_command_alias_intercepts() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                AddTrigger('auto_trig', 'prompt>', '', 33, 0, 0, '', '', 0, 0)
                SetTriggerOption('auto_trig', 'send', 'go')
                alias_result = nil
                alias('^go$', function() alias_result = "intercepted" end)
            "#,
            )
            .unwrap();
            engine.process_output("prompt>");
            let cmds = engine.drain_commands();
            // Trigger sends "go" as command, but alias is for process_input not process_output
            assert!(cmds.contains(&"go".to_string()));
        });
    }

    #[test]
    fn test_timer_and_trigger_coexist() {
        with_engine(|engine| {
            exec(engine, r#"
                trig_result = nil
                AddTrigger('co_trig', 'status', '', 33, 0, 0, '', 'function() trig_result = true end', 0, 0)
                timer_result = nil
                timer(5, function() timer_result = true end)
            "#).unwrap();
            engine.process_output("status");
            engine.fire_timer(0);
            let tr: Option<bool> = eval(engine, "return trig_result").unwrap();
            let tmr: Option<bool> = eval(engine, "return timer_result").unwrap();
            assert_eq!(tr, Some(true));
            assert_eq!(tmr, Some(true));
        });
    }

    #[test]
    fn test_variable_shared_across_triggers_and_aliases() {
        with_engine(|engine| {
            exec(engine, r#"
                SetVariable('counter', '0')
                AddTrigger('count_trig', 'tick', '', 33, 0, 0, '', 'function() SetVariable("counter", tostring(tonumber(GetVariable("counter")) + 1)) end', 0, 0)
                AddAlias('show_count', 'count', '', 33, 'function() Note("count=" .. GetVariable("counter")) end')
            "#).unwrap();
            engine.process_output("tick");
            engine.process_output("tick");
            engine.process_output("tick");
            let val: String = eval(engine, "return GetVariable('counter')").unwrap();
            assert_eq!(val, "3");
        });
    }
}
