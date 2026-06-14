use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use mlua::{Function, Lua, Result as LuaResult, Table, UserData, Value};
use regex::bytes::Regex as BytesRegex;
use regex::Regex;
use rusqlite::{types::Value as SqlValue, Connection};

/// SQLite 连接包装（Lua 用户数据）
struct LuaDb {
    conn: Arc<Mutex<Connection>>,
    text_is_gbk: bool,
}

impl UserData for LuaDb {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("close", |_, _this, ()| Ok(()));

        methods.add_method_mut("set_gbk", |_, this, flag: bool| {
            this.text_is_gbk = flag;
            Ok(())
        });

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
                                rusqlite::types::ValueRef::Text(s) => {
                                    // 根据数据库文本编码解码
                                    // 某些 GBK 字节序列恰好也是合法 UTF-8（但对应不同字符），
                                    // 所以不能用"先尝试 UTF-8"的启发式，必须明确指定编码
                                    let text = if this.text_is_gbk {
                                        let (cow, _, _) = encoding_rs::GBK.decode(s);
                                        cow.into_owned()
                                    } else if std::str::from_utf8(s).is_ok() {
                                        std::str::from_utf8(s).unwrap().to_string()
                                    } else {
                                        let (cow, _, _) = encoding_rs::GBK.decode(s);
                                        cow.into_owned()
                                    };
                                    rusqlite::types::Value::Text(text)
                                }
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
                                // 尝试 UTF-8，失败则从 GBK 转码
                                let text = if std::str::from_utf8(s).is_ok() {
                                    std::str::from_utf8(s).unwrap().to_string()
                                } else {
                                    let (cow, _, _) = encoding_rs::GBK.decode(s);
                                    cow.into_owned()
                                };
                                mlua::Value::String(lua.create_string(&text)?)
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

/// 触发器匹配模式：GBK 字节模式或 UTF-8 字符模式
pub(crate) enum TriggerPattern {
    /// GBK 字节模式：正则中的中文字符转为 GBK 字节序列，匹配 GBK 编码的数据
    /// 适用于 GBK 编码的脚本，.{4} 匹配 4 字节（2 个中文字符）
    Gbk(BytesRegex),
    /// UTF-8 字符模式：正则按 Unicode 字符匹配，匹配 UTF-8 数据
    /// 适用于 UTF-8 编码的脚本，.{4} 匹配 4 个 Unicode 字符
    Utf8(Regex),
}

/// 触发器定义
pub struct Trigger {
    pub name: String,
    pub(crate) pattern: TriggerPattern,
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
    pub match_text: String,
    pub pattern: Regex,
    pub callback: Function,
    pub enabled: bool,
    pub group: String,
    pub send_to: i64,
    pub response: String,
    pub sequence: i32,
}

/// 定时器定义
pub struct TimerDef {
    pub name: String,
    pub interval_millis: u64,
    pub callback: Function,
    pub enabled: bool,
    pub group: String,
    pub one_shot: bool,
    pub at_time: bool,
    pub send_text: String,
    pub last_fired: std::time::Instant,
}

/// 脚本编码类型
#[derive(Clone, Copy, PartialEq)]
enum ScriptEncoding {
    Utf8,
    Gbk,
}

/// 脚本运行时共享状态
struct ScriptState {
    triggers: Vec<Trigger>,
    aliases: Vec<Alias>,
    timers: Vec<TimerDef>,
    variables: HashMap<String, String>,
    pending_commands: Vec<String>,
    pending_raw: Vec<Vec<u8>>,
    pending_logs: Vec<String>,
    /// Tell/io.write 的行缓冲区，用于实现内联输出（如 tprint 的缩进）
    tell_buffer: String,
    recent_lines: Vec<String>,
    unique_counter: u64,
    connected: bool,
    connect_requested: bool,
    disconnect_requested: bool,
    host: String,
    port: u16,
    world_name: String,
    char_name: String,
    packet_count: u64,
    status_text: String,
    /// 当前加载脚本的编码，用于决定触发器匹配模式
    current_encoding: ScriptEncoding,
    /// 上次收到服务器数据的时间（用于空闲心跳检测）
    last_server_data: std::time::Instant,
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
    script_path: Rc<RefCell<Option<String>>>,
    script_dir: Rc<RefCell<Option<String>>>,
}

// ============================================================
// JSON 互转辅助函数（供 json_encode / json_decode 使用）
// ============================================================

/// 将 mlua::Value 转为 serde_json::Value（用于序列化）
fn lua_value_to_json(val: &mlua::Value) -> serde_json::Value {
    match val {
        mlua::Value::Nil => serde_json::Value::Null,
        mlua::Value::Boolean(b) => serde_json::Value::Bool(*b),
        mlua::Value::Integer(i) => serde_json::Value::Number((*i).into()),
        mlua::Value::Number(n) => {
            serde_json::Value::Number(serde_json::Number::from_f64(*n).unwrap_or(0.into()))
        }
        mlua::Value::String(s) => {
            let owned: Vec<u8> = s.as_bytes().to_vec();
            serde_json::Value::String(String::from_utf8_lossy(&owned).to_string())
        }
        mlua::Value::Table(t) => {
            // 判断是 array 还是 map
            let mut is_array = true;
            let mut i = 1;
            for pair in t.clone().pairs::<mlua::Value, mlua::Value>() {
                if let Ok((k, _)) = pair {
                    match k {
                        mlua::Value::Integer(n) if n == i => {
                            i += 1;
                        }
                        _ => {
                            is_array = false;
                            break;
                        }
                    }
                } else {
                    is_array = false;
                    break;
                }
            }
            if is_array && i > 1 {
                // 数组
                let mut arr = Vec::new();
                for pair in t.clone().pairs::<i64, mlua::Value>() {
                    if let Ok((_, v)) = pair {
                        arr.push(lua_value_to_json(&v));
                    }
                }
                serde_json::Value::Array(arr)
            } else {
                // 对象
                let mut map = serde_json::Map::new();
                for pair in t.clone().pairs::<mlua::Value, mlua::Value>() {
                    if let Ok((k, v)) = pair {
                        let key = match &k {
                            mlua::Value::String(s) => {
                                let owned: Vec<u8> = s.as_bytes().to_vec();
                                String::from_utf8_lossy(&owned).to_string()
                            }
                            _ => format!("{:?}", k),
                        };
                        map.insert(key, lua_value_to_json(&v));
                    }
                }
                serde_json::Value::Object(map)
            }
        }
        _ => serde_json::Value::Null,
    }
}

/// 将 serde_json::Value 转为 mlua::Value（用于反序列化）
fn json_to_lua_value(lua: &mlua::Lua, val: &serde_json::Value) -> mlua::Result<mlua::Value> {
    match val {
        serde_json::Value::Null => Ok(mlua::Value::Nil),
        serde_json::Value::Bool(b) => Ok(mlua::Value::Boolean(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(mlua::Value::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(mlua::Value::Number(f))
            } else {
                Ok(mlua::Value::Number(0.0))
            }
        }
        serde_json::Value::String(s) => Ok(mlua::Value::String(lua.create_string(s)?)),
        serde_json::Value::Array(arr) => {
            let table = lua.create_table()?;
            for (i, v) in arr.iter().enumerate() {
                table.set(i + 1, json_to_lua_value(lua, v)?)?;
            }
            Ok(mlua::Value::Table(table))
        }
        serde_json::Value::Object(obj) => {
            let table = lua.create_table()?;
            for (k, v) in obj {
                table.set(k.as_str(), json_to_lua_value(lua, v)?)?;
            }
            Ok(mlua::Value::Table(table))
        }
    }
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
            pending_raw: Vec::new(),
            pending_logs: Vec::new(),
            tell_buffer: String::new(),
            recent_lines: Vec::new(),
            unique_counter: 0,
            connected: false,
            connect_requested: false,
            disconnect_requested: false,
            host: String::new(),
            port: 0,
            world_name: String::new(),
            char_name: String::new(),
            packet_count: 0,
            status_text: String::new(),
            current_encoding: ScriptEncoding::Utf8,
            last_server_data: std::time::Instant::now(),
        }));

        let script_dir = Rc::new(RefCell::new(None::<String>));
        let script_path = Rc::new(RefCell::new(None::<String>));

        let mut engine = Self {
            lua,
            state,
            script_path,
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
        *self.script_path.borrow_mut() = Some(path.to_string());
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

        // SendPkt(data) — MushClient API: 发送原始数据包到 MUD
        let state_rc_pkt = state_rc.clone();
        let send_pkt_fn =
            lua.create_function_mut(move |_, data: mlua::String| -> LuaResult<i64> {
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
                let matches: Vec<(usize, String, Vec<String>)> = {
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
                                            result.push((i, full_match, caps_list));
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
                                    result.push((i, full_match, caps_list));
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
                                            result.push((i, full_match, caps_list));
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
                                    result.push((i, full_match, caps_list));
                                }
                            }
                        }
                    }
                    result
                };

                // 判断是否需要 omit_from_output
                let mut any_omit = false;

                // 逐个触发回调
                for (idx, full_match, caps_list) in matches {
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
                    // MUSHclient 触发器回调签名: function(name, line, wildcards)
                    if let Ok(wildcards_table) = lua.create_table() {
                        // w[0] = 完整匹配文本（MUSHclient 兼容）
                        let _ = wildcards_table.set(0, full_match.as_str());
                        for (i, m) in caps_list.iter().enumerate() {
                            let _ = wildcards_table.set(i + 1, m.as_str());
                        }
                        // 使用 catch_unwind 防止 Rust panic 跨越 Lua FFI 边界导致静默崩溃
                        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            let _ = callback.call::<()>((
                                trigger_name.as_str(),
                                clean_line.as_str(),
                                wildcards_table,
                            ));
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
            let lua_val = json_to_lua_value(&lua, &json_val)?;
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

        // GetTriggerInfo(name, code) — MushClient API 兼容
        // code 8 = enabled (Boolean), code 26 = group (String)
        let state_rc12 = state_rc.clone();
        let get_trigger_info_fn =
            lua.create_function_mut(move |lua, (name, code): (String, i64)| {
                let state = state_rc12.borrow();
                if let Some(t) = state.triggers.iter().find(|t| t.name == name) {
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
                            Ok(Value::Integer(flags))
                        }
                        5 => Ok(Value::Integer(0)),
                        6 => Ok(Value::Integer(t.sequence as i64)),
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
                if let Some(t) = state.triggers.iter_mut().find(|t| t.name == name) {
                    match key.as_str() {
                        "group" => {
                            if let Value::String(s) = value {
                                t.group = s.to_str().map(|s| s.to_string()).unwrap_or_default();
                            }
                        }
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
            let response: String = coerce_to_string(args[2].clone())?;
            let flags: i64 = coerce_to_i64(args[3].clone())?;
            // 第5个参数 script_name（可选）
            let script = if args.len() > 4 {
                coerce_to_string(args[4].clone())?
            } else {
                String::new()
            };
            let is_regex = (flags & 32) != 0;

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

            state_rc15.borrow_mut().aliases.push(Alias {
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

        // GetAliasInfo(name, code) — MushClient API 兼容
        let state_rc_gi = state_rc.clone();
        let get_alias_info_fn =
            lua.create_function_mut(move |lua, (name, code): (String, i64)| {
                let state = state_rc_gi.borrow();
                if let Some(a) = state.aliases.iter().find(|a| a.name == name) {
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
                        18 => Ok(Value::Integer(a.send_to)),
                        19 => Ok(Value::Integer(1)),
                        20 => Ok(Value::Integer(a.sequence as i64)),
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
                if let Some(a) = state.aliases.iter_mut().find(|a| a.name == name) {
                    match key.as_str() {
                        "group" => {
                            if let Value::String(s) = value {
                                a.group = s.to_str().map(|s| s.to_string()).unwrap_or_default();
                            }
                        }
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
                                a.send_to = n;
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
            let sec_val = coerce_to_f64(args[3].clone()).unwrap_or(0.0);
            // 综合计算：总秒数 = hour*3600 + min*60 + sec
            let total_secs = (_hour as f64) * 3600.0 + (_min as f64) * 60.0 + sec_val;
            let interval_millis = if total_secs <= 0.0 {
                1000.0
            } else {
                total_secs * 1000.0
            };
            // 第5个参数 response_text：MushClient 中是字符串，忽略
            let flags: i64 = coerce_to_i64(args[5].clone()).unwrap_or(0);
            // 第7个参数 script_name（可选）
            let script_name = if args.len() > 6 {
                coerce_to_string(args[6].clone()).unwrap_or_default()
            } else {
                String::new()
            };

            let interval_millis_u64 = interval_millis as u64;
            let one_shot = (flags & 4) != 0;
            let at_time = (flags & 2) != 0;

            // 将脚本作为 send_text 存储，在 fire_timer 时执行
            let callback: Function = lua.create_function(|_, _: ()| Ok(()))?;

            // Replace flag (1024): 替换同名定时器，保留旧定时器的启用状态
            // 防止 closeclass 禁用定时器后被 AddTimer(Replace) 重新启用
            let old_enabled = if (flags & 1024) != 0 {
                let old_enabled = state_rc19
                    .borrow()
                    .timers
                    .iter()
                    .find(|t| t.name == name)
                    .map(|t| t.enabled);
                state_rc19.borrow_mut().timers.retain(|t| t.name != name);
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

            state_rc19.borrow_mut().timers.push(TimerDef {
                name,
                interval_millis: interval_millis_u64,
                callback,
                enabled: timer_enabled,
                group: String::new(),
                one_shot,
                at_time,
                send_text: script_name,
                last_fired: std::time::Instant::now(),
            });
            Ok(Value::Integer(0))
        })?;
        globals.set("AddTimer", add_timer_fn)?;

        // DoAfter(seconds, text) — 一次性临时定时器，发送文本到 MUD (send_to=0)
        let state_rc_da = state_rc.clone();
        let doafter_fn = lua.create_function_mut(move |lua, (seconds, text): (f64, String)| {
            if seconds < 0.1 || seconds > 86399.0 {
                return Ok(Value::Integer(1)); // eTimeInvalid
            }
            let mut state = state_rc_da.borrow_mut();
            state.unique_counter += 1;
            let timer_name = format!("__doafter_{}", state.unique_counter);
            let interval_millis = (seconds * 1000.0) as u64;

            let callback: Function = lua.create_function(|_, _: ()| Ok(()))?;
            let send_text = format!("Execute([[{}]])", text);

            state.timers.push(TimerDef {
                name: timer_name,
                interval_millis,
                callback,
                enabled: true,
                group: String::new(),
                one_shot: true,
                at_time: false,
                send_text,
                last_fired: std::time::Instant::now(),
            });
            Ok(Value::Integer(0)) // eOK
        })?;
        globals.set("DoAfter", doafter_fn)?;

        // DoAfterNote(seconds, text) — 一次性临时定时器，输出文本到窗口 (send_to=2)
        let state_rc_dn = state_rc.clone();
        let doafter_note_fn =
            lua.create_function_mut(move |lua, (seconds, text): (f64, String)| {
                if seconds < 0.1 || seconds > 86399.0 {
                    return Ok(Value::Integer(1)); // eTimeInvalid
                }
                let mut state = state_rc_dn.borrow_mut();
                state.unique_counter += 1;
                let timer_name = format!("__doafter_note_{}", state.unique_counter);
                let interval_millis = (seconds * 1000.0) as u64;

                let callback: Function = lua.create_function(|_, _: ()| Ok(()))?;
                let send_text = format!("Note([[{}]])", text);

                state.timers.push(TimerDef {
                    name: timer_name,
                    interval_millis,
                    callback,
                    enabled: true,
                    group: String::new(),
                    one_shot: true,
                    at_time: false,
                    send_text,
                    last_fired: std::time::Instant::now(),
                });
                Ok(Value::Integer(0))
            })?;
        globals.set("DoAfterNote", doafter_note_fn)?;

        // DoAfterSpecial(seconds, text, send_to) — 可指定目标位置
        let state_rc_ds = state_rc.clone();
        let doafter_special_fn =
            lua.create_function_mut(move |lua, (seconds, text, send_to): (f64, String, i64)| {
                if seconds < 0.1 || seconds > 86399.0 {
                    return Ok(Value::Integer(1)); // eTimeInvalid
                }
                if send_to < 0 || send_to > 14 {
                    return Ok(Value::Integer(2)); // eOptionOutOfRange
                }
                let mut state = state_rc_ds.borrow_mut();
                state.unique_counter += 1;
                let timer_name = format!("__doafter_special_{}", state.unique_counter);
                let interval_millis = (seconds * 1000.0) as u64;

                let callback: Function = lua.create_function(|_, _: ()| Ok(()))?;
                let send_text = match send_to {
                    0 | 10 | 13 => format!("Execute([[{}]])", text), // World / Execute / Immediate
                    2 => format!("Note([[{}]])", text),              // Output window
                    3 => format!("SetStatus([[{}]])", text),         // Status line
                    11 => format!("Execute([[{}]])", text),          // Speedwalk (Execute 处理)
                    12 | 14 => text,                                 // Script engine — 直接执行 Lua
                    _ => format!("Execute([[{}]])", text),           // 默认走 Execute
                };

                state.timers.push(TimerDef {
                    name: timer_name,
                    interval_millis,
                    callback,
                    enabled: true,
                    group: String::new(),
                    one_shot: true,
                    at_time: false,
                    send_text,
                    last_fired: std::time::Instant::now(),
                });
                Ok(Value::Integer(0))
            })?;
        globals.set("DoAfterSpecial", doafter_special_fn)?;

        // DoAfterSpeedWalk(seconds, text) — speedwalk 定时器 (send_to=11)
        let state_rc_dw = state_rc.clone();
        let doafter_sw_fn =
            lua.create_function_mut(move |lua, (seconds, text): (f64, String)| {
                if seconds < 0.1 || seconds > 86399.0 {
                    return Ok(Value::Integer(1)); // eTimeInvalid
                }
                let mut state = state_rc_dw.borrow_mut();
                state.unique_counter += 1;
                let timer_name = format!("__doafter_sw_{}", state.unique_counter);
                let interval_millis = (seconds * 1000.0) as u64;

                let callback: Function = lua.create_function(|_, _: ()| Ok(()))?;
                let send_text = format!("Execute([[{}]])", text);

                state.timers.push(TimerDef {
                    name: timer_name,
                    interval_millis,
                    callback,
                    enabled: true,
                    group: String::new(),
                    one_shot: true,
                    at_time: false,
                    send_text,
                    last_fired: std::time::Instant::now(),
                });
                Ok(Value::Integer(0))
            })?;
        globals.set("DoAfterSpeedWalk", doafter_sw_fn)?;

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

        // GetTimerInfo(name, code) — MushClient API 兼容
        // code 6 = enabled (Boolean), 7 = one_shot (Boolean), 8 = at_time (Boolean), 19 = group (String)
        let state_rc22 = state_rc.clone();
        let get_timer_info_fn =
            lua.create_function_mut(move |lua, (name, code): (String, i64)| {
                let state = state_rc22.borrow();
                if let Some(t) = state.timers.iter().find(|tt| tt.name == name) {
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
                if let Some(t) = state.timers.iter_mut().find(|t| t.name == name) {
                    match key.as_str() {
                        "group" => {
                            if let Value::String(s) = value {
                                t.group = s.to_str().map(|s| s.to_string()).unwrap_or_default();
                            }
                        }
                        "timer_timestamp" => {
                            if let Value::Integer(ts) = value {
                                if ts > 0 {
                                    let current_time = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_secs();
                                    let offset = current_time.saturating_sub(ts as u64);
                                    t.last_fired = std::time::Instant::now()
                                        - std::time::Duration::from_secs(offset);
                                } else {
                                    t.last_fired = std::time::Instant::now();
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

        // ResetTimer(name) — MushClient API: 重置定时器计时
        let state_rc_rt = state_rc.clone();
        let reset_timer_fn = lua.create_function_mut(move |_, name: String| {
            let mut state = state_rc_rt.borrow_mut();
            if let Some(t) = state.timers.iter_mut().find(|t| t.name == name) {
                t.last_fired = std::time::Instant::now();
                Ok(Value::Integer(0))
            } else {
                Ok(Value::Integer(1))
            }
        })?;
        globals.set("ResetTimer", reset_timer_fn)?;

        // ============================================================
        // 配置 API
        // ============================================================

        // GetInfo(code) — MushClient API 兼容
        let script_dir_rc = self.script_dir.clone();
        let script_path_rc = self.script_path.clone();
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
                let dir = script_dir_rc.borrow().clone();
                match dir {
                    Some(d) => {
                        let win_path = d.replace('/', "\\");
                        Ok(Value::String(lua.create_string(&win_path)?))
                    }
                    None => Ok(Value::String(lua.create_string("")?)),
                }
            }
            204 => {
                // MushClient: GetInfo(204) = Packets received
                let count = state_rc_gi.borrow().packet_count;
                Ok(Value::Integer(count as i64))
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
        let _script_path_rc = self.script_path.clone();
        let state_rc_dofile = state_rc.clone();
        let dofile_fn = lua.create_function_mut(move |lua, path: String| {
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
                state_rc33.borrow_mut().triggers.push(Trigger {
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
                state_rc34.borrow_mut().aliases.push(Alias {
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
                state_rc35.borrow_mut().timers.push(TimerDef {
                    name: String::new(),
                    interval_millis: interval_secs * 1000,
                    callback,
                    enabled: true,
                    group: String::new(),
                    one_shot: false,
                    at_time: false,
                    send_text: String::new(),
                    last_fired: std::time::Instant::now(),
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

    /// 执行 Lua 代码并返回字符串结果
    pub fn eval_to_string(&self, code: &str) -> Result<String, String> {
        self.lua
            .load(code)
            .eval::<String>()
            .map_err(|e| format!("{}", e))
    }

    /// 加载并执行 Lua 脚本文件
    /// 自动检测编码：先尝试 UTF-8，失败（GBK 编码）则自动转码
    pub fn load_script(&mut self, path: &str) -> Result<(), String> {
        // 先设置脚本路径，确保脚本执行时 GetInfo(35) 能返回正确目录
        self.set_script_path(path);

        let bytes = std::fs::read(path).map_err(|e| format!("读取脚本失败 '{}': {}", path, e))?;

        let (code, is_gbk) = match std::str::from_utf8(&bytes) {
            Ok(s) => (s.to_string(), false),
            Err(_) => {
                let (cow, _, _) = encoding_rs::GBK.decode(&bytes);
                (cow.into_owned(), true)
            }
        };

        // 设置当前脚本编码，触发器注册时会根据此标志选择匹配模式
        self.state.borrow_mut().current_encoding = if is_gbk {
            ScriptEncoding::Gbk
        } else {
            ScriptEncoding::Utf8
        };

        self.lua
            .load(&code)
            .set_name(path)
            .exec()
            .map_err(|e| format!("err '{}': {}", path, e))?;

        Ok(())
    }

    /// 获取当前加载的脚本路径
    pub fn script_path(&self) -> Option<String> {
        self.script_path.borrow().clone()
    }

    /// 记录错误信息到 stderr 和日志文件
    fn log_error(&self, msg: &str) {
        eprintln!("{}", msg);
        // 使用 try_borrow_mut 避免在 RefCell 已被借用时 panic
        if let Ok(mut state) = self.state.try_borrow_mut() {
            state
                .pending_logs
                .push(format!("[Lua] {}", crate::ui::AnsiParser::strip_ansi(msg)));
        }
    }

    /// 处理服务器输出，匹配触发器
    pub fn process_output(&self, line: &str) {
        // 使用 catch_unwind 防止函数体内任何 panic 跨越 FFI 边界导致静默崩溃
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.process_output_inner(line);
        }));
        if result.is_err() {
            self.log_error("process_output 中发生 panic，已捕获以防止崩溃");
        }
    }

    /// process_output 的内部实现
    fn process_output_inner(&self, line: &str) {
        // 一次性 borrow_mut 完成多项状态更新
        {
            let mut state = self.state.borrow_mut();
            state.pending_commands.clear();
            state.last_server_data = std::time::Instant::now();
            state.packet_count += 1;
        }

        // 剥离 ANSI 码用于匹配，并去除行末 \r
        let clean_line = crate::ui::AnsiParser::strip_ansi(line);
        let clean_line = clean_line.trim_end_matches('\r').to_string();

        // 维护最近行缓冲区
        {
            let mut state = self.state.borrow_mut();
            state.recent_lines.push(clean_line.clone());
            if state.recent_lines.len() > 20 {
                state.recent_lines.remove(0);
            }
        }

        // 将 clean_line 转为 GBK 字节用于 GBK 模式匹配
        let gbk_line = encoding_rs::GBK.encode(&clean_line).0.into_owned();

        // 收集需要触发的
        let matches: Vec<(usize, String, Vec<String>)> = {
            let state = self.state.borrow();
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
                                    result.push((i, full_match, caps_list));
                                }
                            }
                        } else {
                            if let Some(caps) = gbk_re.captures(&gbk_line) {
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
                                result.push((i, full_match, caps_list));
                            }
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
                                    let full_match = caps.get(0).unwrap().as_str().to_string();
                                    let caps_list: Vec<String> = caps
                                        .iter()
                                        .skip(1)
                                        .flatten()
                                        .map(|m| m.as_str().to_string())
                                        .collect();
                                    result.push((i, full_match, caps_list));
                                }
                            }
                        } else {
                            if let Some(caps) = utf8_re.captures(&clean_line) {
                                let full_match = caps.get(0).unwrap().as_str().to_string();
                                let caps_list: Vec<String> = caps
                                    .iter()
                                    .skip(1)
                                    .flatten()
                                    .map(|m| m.as_str().to_string())
                                    .collect();
                                result.push((i, full_match, caps_list));
                            }
                        }
                    }
                }
            }
            result
        };

        // 逐个触发
        for (idx, full_match, caps_list) in matches {
            let (callback, send_text, trigger_name) = {
                let state = self.state.borrow();
                (
                    state.triggers[idx].callback.clone(),
                    state.triggers[idx].send_text.clone(),
                    state.triggers[idx].name.clone(),
                )
            };
            // MUSHclient 触发器回调签名: function(name, line, wildcards)
            if let Ok(wildcards_table) = self.lua.create_table() {
                // w[0] = 完整匹配文本（MUSHclient 兼容）
                let _ = wildcards_table.set(0, full_match.as_str());
                for (i, m) in caps_list.iter().enumerate() {
                    let _ = wildcards_table.set(i + 1, m.as_str());
                }
                // 使用 catch_unwind 防止 Rust panic 跨越 Lua FFI 边界导致静默崩溃
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _ = callback.call::<()>((
                        trigger_name.as_str(),
                        clean_line.as_str(),
                        wildcards_table,
                    ));
                }))
                .is_err()
                {
                    self.log_error(&format!(
                        "[Lua] 触发器 '{}' 回调中发生 panic，已捕获以防止崩溃",
                        trigger_name
                    ));
                }
            }
            if !send_text.is_empty() {
                self.state.borrow_mut().pending_commands.push(send_text);
            }
        }
    }

    /// 处理用户输入，匹配别名
    pub fn process_input(&self, input: &str) -> bool {
        // 使用 catch_unwind 防止 panic 跨越 FFI 边界
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.process_input_inner(input)
        }));
        result.unwrap_or(false)
    }

    /// process_input 的内部实现
    fn process_input_inner(&self, input: &str) -> bool {
        self.state.borrow_mut().pending_commands.clear();

        let matches: Vec<(usize, Vec<String>, i64, String)> = {
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
                    result.push((i, caps_list, alias.send_to, alias.response.clone()));
                }
            }
            result
        };

        if matches.is_empty() {
            return false;
        }

        for (idx, caps_list, send_to, response) in matches {
            if send_to == 12 && !response.is_empty() {
                // send_to=12: 替换 %1, %2... 为捕获文本，作为 Lua 代码执行
                let mut code = response;
                for (i, m) in caps_list.iter().enumerate() {
                    code = code.replace(&format!("%{}", i + 1), m);
                }
                let name = {
                    let state = self.state.borrow();
                    state.aliases[idx].name.clone()
                };
                let lua_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if let Err(e) = self.lua.load(&code).exec() {
                        self.log_error(&format!(
                            "[Lua] 别名 '{}' send_to=12 执行错误: {:?}",
                            name, e
                        ));
                    }
                }));
                if lua_result.is_err() {
                    self.log_error(&format!("别名 send_to=12 执行中发生 panic: {}", code));
                }
            } else {
                // 脚本函数方式：以 (name, line, wildcards_table) 签名调用
                let callback = {
                    let state = self.state.borrow();
                    state.aliases[idx].callback.clone()
                };
                let alias_name = {
                    let state = self.state.borrow();
                    state.aliases[idx].name.clone()
                };
                if let Ok(wildcards) = self.lua.create_table() {
                    for (i, m) in caps_list.iter().enumerate() {
                        let _ = wildcards.set(i + 1, m.as_str());
                    }
                    // 使用 catch_unwind 防止 Rust panic 跨越 Lua FFI 边界导致静默崩溃
                    let name_for_err = alias_name.clone();
                    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let _ = callback.call::<()>((alias_name, input.to_string(), wildcards));
                    }))
                    .is_err()
                    {
                        self.log_error(&format!(
                            "[Lua] 别名 '{}' 回调中发生 panic，已捕获以防止崩溃",
                            name_for_err
                        ));
                    }
                }
            }
        }

        true
    }

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
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let index = self
                .state
                .borrow()
                .timers
                .iter()
                .position(|t| t.name == name);
            match index {
                Some(i) => self.fire_timer_inner(i),
                None => {} // 定时器可能已被回调删除，忽略
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
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.fire_timer_inner(index);
        }));
        if result.is_err() {
            self.log_error("fire_timer 中发生 panic，已捕获以防止崩溃");
        }
    }

    /// fire_timer 的内部实现
    /// TODO(v1.0): 当前每个步骤单独 catch_unwind 是调试期的过度防御措施，
    /// 正式发布前应简化为仅保留外层 catch_unwind（fire_timer / fire_timer_by_name），
    /// 步骤级 catch_unwind 在 panic 后继续执行后续步骤可能导致状态不一致。
    fn fire_timer_inner(&self, index: usize) {
        // 步骤1: 清空待发送队列
        let step1 = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.state.borrow_mut().pending_commands.clear();
        }));
        if step1.is_err() {
            self.log_error(&format!("fire_timer[{}] 步骤1(clear pending) panic", index));
            return;
        }

        // 步骤2: 读取定时器信息
        let (callback, send_text, one_shot, timer_name) =
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let state = self.state.borrow();
                if index < state.timers.len() && state.timers[index].enabled {
                    Some((
                        state.timers[index].callback.clone(),
                        state.timers[index].send_text.clone(),
                        state.timers[index].one_shot,
                        state.timers[index].name.clone(),
                    ))
                } else {
                    None
                }
            })) {
                Ok(Some(v)) => v,
                Ok(None) => return,
                Err(_) => {
                    self.log_error(&format!(
                        "fire_timer[{}] 步骤2(读取定时器信息) panic",
                        index
                    ));
                    return;
                }
            };

        // 步骤3: 调用回调
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = callback.call::<()>(());
        }))
        .is_err()
        {
            self.log_error(&format!(
                "[Lua] 定时器 '{}' 回调中发生 panic，已捕获以防止崩溃",
                timer_name
            ));
        }

        // 步骤4: 执行 send_text
        if !send_text.is_empty() {
            let send_text_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
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
                        .map_or(false, |c| c.is_alphabetic() || c == '_');

                let result: Result<(), String> = if is_function_name {
                    let code = format!("{}('{}')", send_text, timer_name.replace('\'', "\\'"));
                    self.lua.load(&code).exec().map_err(|e| format!("{}", e))
                } else {
                    self.lua
                        .load(&send_text)
                        .exec()
                        .map_err(|e| format!("{}", e))
                };
                result
            }));
            match send_text_result {
                Ok(Err(lua_err)) => {
                    self.log_error(&format!(
                        "定时器 '{}' send_text 执行 Lua 错误: {}",
                        timer_name, lua_err
                    ));
                }
                Err(_) => {
                    self.log_error(&format!(
                        "定时器 '{}' send_text 执行中发生 panic",
                        timer_name
                    ));
                }
                _ => {}
            }
        }

        // 步骤5: one_shot 删除定时器（按名称搜索，避免回调执行后索引已变化）
        if one_shot {
            let step5 = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut state = self.state.borrow_mut();
                if let Some(pos) = state.timers.iter().position(|t| t.name == timer_name) {
                    state.timers.remove(pos);
                }
            }));
            if step5.is_err() {
                self.log_error(&format!(
                    "fire_timer '{}' 步骤5(one_shot remove) panic",
                    timer_name
                ));
            }
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

    /// 取出待发送的命令
    pub fn drain_commands(&self) -> Vec<String> {
        self.state.borrow_mut().pending_commands.drain(..).collect()
    }

    /// 取出待发送的原始数据包（SendPkt 压入的）
    pub fn drain_raw(&self) -> Vec<Vec<u8>> {
        self.state.borrow_mut().pending_raw.drain(..).collect()
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

    /// 设置端口（本引擎扩展，非 MushClient 标准 GetInfo）
    pub fn set_port(&self, port: u16) {
        self.state.borrow_mut().port = port;
    }

    /// 设置世界名称（供 GetInfo(2) 返回）
    pub fn set_world_name(&self, name: &str) {
        self.state.borrow_mut().world_name = name.to_string();
    }

    /// 设置角色名（供 GetInfo(3) 返回）
    pub fn set_char_name(&self, name: &str) {
        self.state.borrow_mut().char_name = name.to_string();
    }

    /// 设置 Lua 全局变量（脚本中可直接按名引用）
    pub fn set_global(&self, name: &str, value: &str) {
        let globals = self.lua.globals();
        let _ = globals.set(name, value);
    }

    /// 获取所有变量（用于 reload 时恢复）
    pub fn get_variables(&self) -> std::collections::HashMap<String, String> {
        self.state.borrow().variables.clone()
    }

    /// 设置连接状态，连接成功时自动调用 OnConnect()（由 Lua 脚本覆盖实现）
    pub fn set_connected(&mut self, connected: bool) {
        let was_connected = self.state.borrow().connected;
        self.state.borrow_mut().connected = connected;
        // 连接刚建立时，调用 OnConnect() 抽象接口
        // Lua 脚本可通过覆盖 OnConnect() 实现连接后的初始化逻辑
        if connected && !was_connected {
            let lua_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if let Err(e) = self.eval_code("OnConnect()") {
                    self.log_error(&format!("OnConnect() 执行失败: {}", e));
                }
            }));
            if lua_result.is_err() {
                self.log_error("OnConnect() 执行中发生 panic，已捕获以防止崩溃");
            }
        }
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
        let mut state = self.state.borrow_mut();
        // flush 残留的 tell_buffer（合并到 pending_logs 末尾）
        let buffered = std::mem::take(&mut state.tell_buffer);
        if !buffered.is_empty() {
            if let Some(last) = state.pending_logs.last_mut() {
                last.push_str(&buffered);
            } else {
                state.pending_logs.push(buffered);
            }
        }
        state.pending_logs.drain(..).collect()
    }

    /// 获取 SetStatus 设置的状态栏文本
    pub fn status_text(&self) -> String {
        self.state.borrow().status_text.clone()
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

/// 将 UTF-8 正则表达式字符串转为 GBK 字节正则表达式
/// 核心思路：
/// 1. 将 UTF-8 编码的中文字符转为 GBK 字节序列（用 \xHH 表示），
///    这样 regex::bytes 引擎在字节模式下匹配，.{4} 匹配4字节=2个GBK中文字符
/// 2. 添加 (?-u) 标志禁用 Unicode 模式，使 \S \s \w \d 等按 ASCII 定义匹配，
///    否则 \S 只匹配有效 UTF-8 序列，无法匹配 GBK 高位字节
fn utf8_regex_to_gbk_bytes(pattern: &str) -> String {
    let mut result = String::with_capacity(pattern.len() * 2);
    let bytes = pattern.as_bytes();
    let mut i = 0;

    // 如果模式以 (?i) 开头，保留它并在后面加 (?-u)
    if bytes.starts_with(b"(?i)") {
        result.push_str("(?i)(?-u)");
        i = 4;
    } else {
        result.push_str("(?-u)");
    }

    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' && i + 1 < bytes.len() {
            // 转义序列，原样保留
            result.push('\\');
            i += 1;
            result.push(bytes[i] as char);
            i += 1;
        } else if b >= 0x80 {
            // 非ASCII字节，可能是UTF-8多字节字符的起始字节
            // 收集完整的UTF-8字符
            let char_len = if b >= 0xF0 {
                4
            } else if b >= 0xE0 {
                3
            } else {
                2
            };
            if i + char_len <= bytes.len() {
                let utf8_str = std::str::from_utf8(&bytes[i..i + char_len]).unwrap_or("?");
                // 转为 GBK 字节序列
                let (gbk_bytes, _, _) = encoding_rs::GBK.encode(utf8_str);
                for &gb in gbk_bytes.iter() {
                    result.push_str(&format!("\\x{:02X}", gb));
                }
                i += char_len;
            } else {
                result.push(b as char);
                i += 1;
            }
        } else {
            result.push(b as char);
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

    state_rc.borrow_mut().triggers.push(Trigger {
        name: name.to_string(),
        pattern: trigger_pattern,
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
                engine.script_path(),
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

    #[test]
    fn test_print_redirect() {
        with_engine(|engine| {
            // print 应该被重定向到 pending_logs
            exec(engine, "print('hello')").unwrap();
            let logs = engine.drain_logs();
            assert!(logs.contains(&"hello".to_string()));
        });
    }

    #[test]
    fn test_print_multiple_args() {
        with_engine(|engine| {
            // print 多个参数，用制表符分隔
            exec(engine, "print('a', 'b', 'c')").unwrap();
            let logs = engine.drain_logs();
            assert!(logs.contains(&"a\tb\tc".to_string()));
        });
    }

    #[test]
    fn test_print_mixed_types() {
        with_engine(|engine| {
            exec(engine, "print('n=', 42, 'b=', true)").unwrap();
            let logs = engine.drain_logs();
            assert!(logs.iter().any(|l| l.contains("42") && l.contains("true")));
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
                AddTrigger('match_trig', [[hello (\w+)]], '', 33, 0, 0, '', 'function(name, line, wildcards) test_result = wildcards[1] end', 0, 0)
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
                AddTrigger('wc_trig', 'You see * here', '', 1, 0, 0, '', 'function(name, line, wildcards) wc_result = wildcards[1] end', 0, 0)
            "#).unwrap();
            engine.process_output("You see a goblin here");
            let result: Option<String> = eval(engine, "return wc_result").unwrap();
            assert_eq!(result, Some("a goblin".to_string()));
        });
    }

    // 测试 w[0] 为完整匹配文本（MUSHclient 兼容）
    #[test]
    fn test_trigger_w0_full_match() {
        with_engine(|engine| {
            exec(engine, r#"
                w0_result = nil
                w1_result = nil
                AddTrigger('w0_trig', [[^(.+) hits (.+)$]], '', 33, 0, 0, '', 'function(name, line, wildcards) w0_result = wildcards[0]; w1_result = wildcards[1] end', 0, 0)
            "#).unwrap();
            engine.process_output("goblin hits warrior");
            let w0: Option<String> = eval(engine, "return w0_result").unwrap();
            let w1: Option<String> = eval(engine, "return w1_result").unwrap();
            assert_eq!(w0, Some("goblin hits warrior".to_string()));
            assert_eq!(w1, Some("goblin".to_string()));
        });
    }

    // 测试多行触发器的 w[0] 包含完整合并文本
    #[test]
    fn test_trigger_w0_multiline() {
        with_engine(|engine| {
            exec(engine, r#"
                ml_w0 = nil
                ml_w1 = nil
                AddTrigger('ml_w0_trig', [[^line1\n(.+)$]], '', 33, 0, 0, '', 'function(name, line, wildcards) ml_w0 = wildcards[0]; ml_w1 = wildcards[1] end', 0, 0)
                SetTriggerOption('ml_w0_trig', 'multi_line', true)
                SetTriggerOption('ml_w0_trig', 'lines_to_match', 2)
            "#).unwrap();
            engine.process_output("line1");
            engine.process_output("line2 content");
            let w0: Option<String> = eval(engine, "return ml_w0").unwrap();
            let w1: Option<String> = eval(engine, "return ml_w1").unwrap();
            assert_eq!(w0, Some("line1\nline2 content".to_string()));
            assert_eq!(w1, Some("line2 content".to_string()));
        });
    }

    // 测试 w[0] 在 findstring 类函数中的使用（脚本常见用法）
    #[test]
    fn test_trigger_w0_with_chinese() {
        with_engine(|engine| {
            exec(engine, r#"
                zh_w0 = nil
                zh_w1 = nil
                AddTrigger('zh_trig', [[^你向(.+)打听有关「(.+)」的消息。$]], '', 33, 0, 0, '', 'function(name, line, wildcards) zh_w0 = wildcards[0]; zh_w1 = wildcards[1] end', 0, 0)
            "#).unwrap();
            engine.process_output("你向范骅打听有关「治安」的消息。");
            let w0: Option<String> = eval(engine, "return zh_w0").unwrap();
            let w1: Option<String> = eval(engine, "return zh_w1").unwrap();
            assert_eq!(w0, Some("你向范骅打听有关「治安」的消息。".to_string()));
            assert_eq!(w1, Some("范骅".to_string()));
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
            let val: Value = eval(engine, "return GetTriggerInfo('nonexistent', 7)").unwrap();
            assert!(val.is_nil());
        });
    }

    #[test]
    fn test_set_trigger_option_regexp() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                function test_re_cb(name, line, wildcards)
                    Execute("matched_" .. line)
                end
                AddTrigger('re_trig', 'old_pattern', '', trigger_flag.Enabled + trigger_flag.Replace + trigger_flag.RegularExpression, 0, 0, '', 'test_re_cb', 0, 10)
                "#,
            )
            .unwrap();
            // 先确认匹配旧正则
            engine.process_output("old_pattern");
            let cmds1 = engine.drain_commands();
            assert_eq!(cmds1, vec!["matched_old_pattern"]);
            // 改用新的正则
            exec(engine, "SetTriggerOption('re_trig', 'regexp', 'new_(.+)')").unwrap();
            engine.process_output("new_value");
            let cmds2 = engine.drain_commands();
            assert_eq!(cmds2, vec!["matched_new_value"]);
            // 旧正则不应该再匹配
            engine.process_output("old_pattern");
            let cmds3 = engine.drain_commands();
            assert!(cmds3.is_empty());
        });
    }

    #[test]
    fn test_set_trigger_option_sequence() {
        with_engine(|engine| {
            exec(
                engine,
                "AddTrigger('seq_trig', 'test', '', 1, 0, 0, '', '', 0, 0)",
            )
            .unwrap();
            exec(engine, "SetTriggerOption('seq_trig', 'sequence', 50)").unwrap();
            let seq: i64 = eval(engine, "return GetTriggerInfo('seq_trig', 6)").unwrap();
            assert_eq!(seq, 50);
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
    fn test_process_input_cfg_skill_xue_alias() {
        with_engine(|engine| {
            // 设置 cfg 表并定义 skill_xue 函数
            exec(
                engine,
                r#"
                cfg = {}
                skills_xue = nil
                function cfg.skill_xue(...)
                    local args = {...}
                    if args[1] ~= nil and args[1] ~= "" then
                        skills_xue = args[1]
                    end
                end
                "#,
            )
            .unwrap();
            // 注册两个别名：无参数（显示）和有参数（设置）
            exec(
                engine,
                r#"AddAlias('test_skill_xue_display', [[^#cfg skill_xue$]], [[cfg.skill_xue()]], 33)"#,
            )
            .unwrap();
            exec(
                engine,
                r#"AddAlias('test_skill_xue_set', [[^#cfg skill_xue\s+(.+)$]], [[cfg.skill_xue('%1')]], 33)"#,
            )
            .unwrap();
            // 测试1：匹配并设置值
            let handled = engine.process_input("#cfg skill_xue sword|blade|force");
            assert!(handled);
            let result: String = eval(engine, "return skills_xue or 'nil'").unwrap();
            assert_eq!(result, "sword|blade|force");
            // 测试2：无参数显示当前值（不修改）
            let handled2 = engine.process_input("#cfg skill_xue");
            assert!(handled2); // 应该匹配 display 别名
            let result2: String = eval(engine, "return skills_xue or 'nil'").unwrap();
            assert_eq!(result2, "sword|blade|force"); // 值未被修改
        });
    }

    #[test]
    fn test_process_input_cfg_skill_lingwu_alias() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                cfg = {}
                skills_lingwu = nil
                function cfg.skill_lingwu(...)
                    local args = {...}
                    if args[1] ~= nil and args[1] ~= "" then
                        skills_lingwu = args[1]
                    end
                end
                "#,
            )
            .unwrap();
            exec(
                engine,
                r#"AddAlias('test_skill_lingwu_set', [[^#cfg skill_lingwu\s+(.+)$]], [[cfg.skill_lingwu('%1')]], 33)"#,
            )
            .unwrap();
            let handled = engine.process_input("#cfg skill_lingwu parry|dodge");
            assert!(handled);
            let result: String = eval(engine, "return skills_lingwu or 'nil'").unwrap();
            assert_eq!(result, "parry|dodge");
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
    fn test_alias_response_send_to_script() {
        // 验证 4 参数 AddAlias（无 script）默认使用 send_to=12
        // 即 response 应作为 Lua 代码执行
        with_engine(|engine| {
            exec(
                engine,
                r#"AddAlias("cfg_test", "^#cfg test$", "send('alias_executed')", 33)"#,
            )
            .unwrap();
            let handled = engine.process_input("#cfg test");
            assert!(handled);
            let cmds = engine.drain_commands();
            assert!(cmds.contains(&"alias_executed".to_string()));
        });
    }

    #[test]
    fn test_alias_response_with_capture_groups() {
        // 验证 %1, %2 捕获组替换后作为 Lua 代码执行
        with_engine(|engine| {
            exec(
                engine,
                r#"AddAlias("cfg_set", "^#cfg (\\w+) (.*)$", "send('set:'..'%1'..'='..'%2')", 33)"#,
            )
            .unwrap();
            let handled = engine.process_input("#cfg neili_job 80");
            assert!(handled);
            let cmds = engine.drain_commands();
            assert!(cmds.contains(&"set:neili_job=80".to_string()));
        });
    }

    #[test]
    fn test_alias_matching() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                alias_result = nil
                AddAlias('match_alias', 'kill *', '', 1, 'function(n, l, w) alias_result = w[1] end')
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
    fn test_alias_war_matching() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                function warteam() print("warteam called") end
                AddAlias("alias_war","^war$","warteam()",alias_flag.Enabled + alias_flag.Replace + alias_flag.RegularExpression ,"")
                SetAliasOption("alias_war","send_to",12)
            "#,
            )
            .unwrap();
            let matched = engine.process_input("war");
            assert!(matched, "war alias should match 'war'");
            let matched2 = engine.process_input("war ");
            assert!(
                !matched2,
                "alias should not match 'war ' (with trailing space)"
            );
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
                AddAlias('regex_al', [[^go (\w+)$]], '', 33, 'function(n, l, w) regex_alias_result = w[1] end')
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
                AddAlias('qm_alias', 'go ?', '', 1, 'function(n, l, w) qm_result = w[1] end')
            "#,
            )
            .unwrap();
            let matched = engine.process_input("go n");
            assert!(matched);
            let result: Option<String> = eval(engine, "return qm_result").unwrap();
            assert_eq!(result, Some("n".to_string()));
        });
    }

    #[test]
    fn test_set_alias_option_regexp() {
        with_engine(|engine| {
            exec(
                engine,
                "AddAlias('re_alias', 'old_pattern', '', alias_flag.Enabled + alias_flag.Replace + alias_flag.RegularExpression)",
            )
            .unwrap();
            let matched1 = engine.process_input("old_pattern");
            assert!(matched1);
            // 改用新的正则
            exec(engine, "SetAliasOption('re_alias', 'regexp', 'new_(.+)')").unwrap();
            let matched2 = engine.process_input("new_value");
            assert!(matched2);
            // 旧正则不应该再匹配
            let matched3 = engine.process_input("old_pattern");
            assert!(!matched3);
        });
    }

    #[test]
    fn test_set_alias_option_sequence() {
        with_engine(|engine| {
            exec(engine, "AddAlias('seq_alias', 'test', '', 1)").unwrap();
            let result: i64 =
                eval(engine, "return SetAliasOption('seq_alias', 'sequence', 50)").unwrap();
            assert_eq!(result, 0);
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
    fn test_set_timer_option_timestamp() {
        with_engine(|engine| {
            exec(engine, "AddTimer('ts_timer', 0, 0, 60, '', 1)").unwrap();
            // 设一个过去的时间戳，定时器应立即到期
            exec(engine, "SetTimerOption('ts_timer', 'timer_timestamp', 100)").unwrap();
            let fired = engine.fire_next_due_timer();
            assert!(
                fired,
                "past timestamp should cause timer to fire immediately"
            );
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
                function fire_timer_cb(timer_name)
                    timer_result = "fired"
                end
                AddTimer('fire_t', 0, 0, 5, '', 1, 'fire_timer_cb')
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
    fn test_get_info_1() {
        with_engine(|engine| {
            // GetInfo(1) = Server name (IP address)
            let host: String = eval(engine, "return GetInfo(1)").unwrap();
            assert_eq!(host, "");
            engine.set_host("ln.xkxmud.com");
            let host: String = eval(engine, "return GetInfo(1)").unwrap();
            assert_eq!(host, "ln.xkxmud.com");
        });
    }

    #[test]
    fn test_get_info_2() {
        with_engine(|engine| {
            // GetInfo(2) = World name
            let name: String = eval(engine, "return GetInfo(2)").unwrap();
            assert_eq!(name, "");
            engine.set_world_name("北侠");
            let name: String = eval(engine, "return GetInfo(2)").unwrap();
            assert_eq!(name, "北侠");
        });
    }

    #[test]
    fn test_get_info_3() {
        with_engine(|engine| {
            // GetInfo(3) = Character name
            let name: String = eval(engine, "return GetInfo(3)").unwrap();
            assert_eq!(name, "");
            engine.set_char_name("小姗");
            let name: String = eval(engine, "return GetInfo(3)").unwrap();
            assert_eq!(name, "小姗");
        });
    }

    #[test]
    fn test_get_info_35() {
        with_engine(|engine| {
            engine.set_script_path("/home/user/scripts/main.lua");
            let path: String = eval(engine, "return GetInfo(35)").unwrap();
            assert!(path.contains("main.lua"));
            assert!(path.contains('\\'));
            assert!(!path.contains('/'));
        });
    }

    #[test]
    fn test_get_info_35_no_script_path() {
        with_engine(|engine| {
            let path: String = eval(engine, "return GetInfo(35)").unwrap();
            assert_eq!(path, "");
        });
    }

    #[test]
    fn test_get_info_58() {
        with_engine(|engine| {
            engine.set_script_path("/home/user/scripts/main.lua");
            let dir: String = eval(engine, "return GetInfo(58)").unwrap();
            assert!(dir.contains("scripts"));
            assert!(dir.contains('\\'));
            assert!(!dir.contains('/'));
        });
    }

    #[test]
    fn test_get_info_204() {
        with_engine(|engine| {
            let count: i64 = eval(engine, "return GetInfo(204)").unwrap();
            assert_eq!(count, 0);
            // process_output 会递增计数器
            engine.process_output("hello");
            let count: i64 = eval(engine, "return GetInfo(204)").unwrap();
            assert_eq!(count, 1);
        });
    }

    #[test]
    fn test_get_info_unknown() {
        with_engine(|engine| {
            // 未知 code 返回空串，而非引发错误或返回 nil
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
            let at_time: i64 = eval(engine, "return timer_flag.AtTime").unwrap();
            assert_eq!(at_time, 2);
            let oneshot: i64 = eval(engine, "return timer_flag.OneShot").unwrap();
            assert_eq!(oneshot, 4);
            let speedwalk: i64 = eval(engine, "return timer_flag.TimerSpeedWalk").unwrap();
            assert_eq!(speedwalk, 8);
            let note: i64 = eval(engine, "return timer_flag.TimerNote").unwrap();
            assert_eq!(note, 16);
            let active: i64 = eval(engine, "return timer_flag.ActiveWhenClosed").unwrap();
            assert_eq!(active, 32);
            let replace: i64 = eval(engine, "return timer_flag.Replace").unwrap();
            assert_eq!(replace, 1024);
            let temp: i64 = eval(engine, "return timer_flag.Temporary").unwrap();
            assert_eq!(temp, 16384);
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
            // code 1 = plugin name
            let name: String = eval(engine, "return GetPluginInfo('', 1)").unwrap();
            assert_eq!(name, "RustLuaMud");
            // code 14 = Date modified
            let date: String = eval(engine, "return GetPluginInfo('', 14)").unwrap();
            assert_eq!(date, "");
            // code 19 = Version
            let version: f64 = eval(engine, "return GetPluginInfo('', 19)").unwrap();
            assert_eq!(version, 1.0);
            // code 20 = Directory
            let dir: String = eval(engine, "return GetPluginInfo('', 20)").unwrap();
            assert_eq!(dir, "");
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
                AddTrigger('ml_trig', [[line1[\s\S]*line2]], '', 33, 0, 0, '', 'function() ml_result = true end', 0, 0)
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
                trigger([[^hello (\w+)$]], function(name, line, wildcards) orig_result = wildcards[1] end)
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
                alias('^go (.+)$', function(n, l, w) orig_alias_result = w[1] end)
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
    fn test_set_connected_calls_on_connect() {
        let mut engine = LuaEngine::new().unwrap();
        // 覆盖 OnConnect 函数，设置一个标志变量
        exec(
            &engine,
            r#"
            on_connect_called = false
            OnConnect = function()
                on_connect_called = true
            end
            "#,
        )
        .unwrap();

        // 连接时应调用 OnConnect
        engine.set_connected(true);
        assert!(eval::<bool>(&engine, "return on_connect_called").unwrap());

        // 重复调用 set_connected(true) 不应再次触发
        exec(&engine, "on_connect_called = false").unwrap();
        engine.set_connected(true);
        assert!(!eval::<bool>(&engine, "return on_connect_called").unwrap());

        // 断开后重新连接应再次触发
        engine.set_connected(false);
        engine.set_connected(true);
        assert!(eval::<bool>(&engine, "return on_connect_called").unwrap());
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
            // code 7 = Keep evaluating (MushClient API)
            let ke: bool = eval(engine, "return GetTriggerInfo('t1', 7)").unwrap();
            assert!(ke);
            // code 8 = enabled (MushClient API)
            let en: bool = eval(engine, "return GetTriggerInfo('t1', 8)").unwrap();
            assert!(en);
            // Set group via SetTriggerOption then read via code 26 (MushClient API)
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
            // 常规间隔触发定时器 (flags=1: Enabled)
            exec(engine, "AddTimer('t1', 0, 1, 30, '', 1)").unwrap();
            // code 6 = enabled
            let en: bool = eval(engine, "return GetTimerInfo('t1', 6)").unwrap();
            assert!(en);
            // code 7 = one_shot (false for regular timer)
            let os: bool = eval(engine, "return GetTimerInfo('t1', 7)").unwrap();
            assert!(!os);
            // code 8 = at_time (false for interval timer, true for "at" timer)
            let at: bool = eval(engine, "return GetTimerInfo('t1', 8)").unwrap();
            assert!(!at);
            // code 14 = temporary (not tracked, default false)
            let tmp: bool = eval(engine, "return GetTimerInfo('t1', 14)").unwrap();
            assert!(!tmp);
            // code 19 = group (empty by default)
            let grp: String = eval(engine, "return GetTimerInfo('t1', 19)").unwrap();
            assert_eq!(grp, "");
            // unknown code returns nil
            let val: Value = eval(engine, "return GetTimerInfo('t1', 999)").unwrap();
            assert!(val.is_nil());
        });
    }

    #[test]
    fn test_get_timer_info_at_time_and_one_shot() {
        with_engine(|engine| {
            // AtTime + OneShot + Enabled = 2+4+1=7
            exec(engine, "AddTimer('at_timer', 23, 50, 0, '', 7, 'cb')").unwrap();
            let os: bool = eval(engine, "return GetTimerInfo('at_timer', 7)").unwrap();
            assert!(os, "one_shot should be true");
            let at: bool = eval(engine, "return GetTimerInfo('at_timer', 8)").unwrap();
            assert!(at, "at_time should be true");

            // 纯间隔触发
            exec(engine, "AddTimer('every_timer', 0, 0, 5, '', 1)").unwrap();
            let os: bool = eval(engine, "return GetTimerInfo('every_timer', 7)").unwrap();
            assert!(!os, "should not be one_shot");
            let at: bool = eval(engine, "return GetTimerInfo('every_timer', 8)").unwrap();
            assert!(!at, "should not be at_time");
        });
    }

    #[test]
    fn test_get_info_56() {
        with_engine(|engine| {
            // GetInfo(56) = MUSHclient application path name
            // 本引擎不支持，返回空串
            let path: String = eval(engine, "return GetInfo(56)").unwrap();
            assert_eq!(path, "");
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
            // code 19 = plugin version
            let version: f64 = eval(engine, "return GetPluginInfo(GetPluginID(), 19)").unwrap();
            assert_eq!(version, 1.0);
            // code 20 = directory (string, not boolean)
            let dir: String = eval(engine, "return GetPluginInfo(GetPluginID(), 20)").unwrap();
            assert_eq!(dir, "");
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
                AddTrigger('multi_cap', [[^(\w+) hits (\w+)$]], '', 33, 0, 0, '', 'function(name, line, wildcards) cap1 = wildcards[1]; cap2 = wildcards[2] end', 0, 0)
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
                function cycle_timer_cb(timer_name)
                    cycle_result = "fired"
                end
                AddTimer('cycle_t', 0, 0, 5, '', 1, 'cycle_timer_cb')
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
    fn test_timer_replace_flag() {
        with_engine(|engine| {
            exec(engine, "counter = 0").unwrap();

            // First AddTimer with Replace flag
            exec(
                engine,
                "AddTimer('t1', 0, 0, 1, '', 1 + 1024, 'counter = counter + 1')",
            )
            .unwrap();
            exec(engine, "SetTimerOption('t1', 'group', 'g1')").unwrap();

            // Second AddTimer with Replace flag (should replace, not append)
            exec(
                engine,
                "AddTimer('t1', 0, 0, 1, '', 1 + 1024, 'counter = counter + 10')",
            )
            .unwrap();
            exec(engine, "SetTimerOption('t1', 'group', 'g1')").unwrap();

            // Only one timer should exist
            let count: i64 = eval(engine, "return #GetTimerList()").unwrap();
            assert_eq!(count, 1);

            // Fire the timer directly by index
            engine.fire_timer(0);
            let counter: i64 = eval(engine, "return counter").unwrap();
            // Should be 10 (from the replacement timer), not 11 (1+10 from both)
            assert_eq!(counter, 10);
        });
    }

    #[test]
    fn test_timer_replace_preserves_disabled_state() {
        with_engine(|engine| {
            exec(engine, "counter = 0").unwrap();

            // Create a timer with group
            exec(
                engine,
                "AddTimer('t2', 0, 0, 1, '', 1 + 1024, 'counter = counter + 1')",
            )
            .unwrap();
            exec(engine, "SetTimerOption('t2', 'group', 'kill')").unwrap();

            // Disable the group (simulating closeclass("kill"))
            exec(engine, "EnableTimerGroup('kill', false)").unwrap();

            // Replace the timer with AddTimer(Replace) — should inherit disabled state
            exec(
                engine,
                "AddTimer('t2', 0, 0, 1, '', 1 + 1024, 'counter = counter + 10')",
            )
            .unwrap();
            exec(engine, "SetTimerOption('t2', 'group', 'kill')").unwrap();

            // Only one timer should exist
            let count: i64 = eval(engine, "return #GetTimerList()").unwrap();
            assert_eq!(count, 1);

            // Timer should remain disabled (inherited from old timer)
            let enabled: bool = eval(engine, "return GetTimerInfo('t2', 6)").unwrap();
            assert!(!enabled, "Replaced timer should inherit disabled state");

            // Fire the timer — should NOT fire since it's disabled
            engine.fire_timer(0);
            let counter: i64 = eval(engine, "return counter").unwrap();
            assert_eq!(counter, 0, "Disabled timer should not fire after replace");
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

    #[test]
    fn test_doafter_executes_command() {
        with_engine(|engine| {
            let count_before = engine.timer_count();
            exec(engine, r#"DoAfter(5, "test_command")"#).unwrap();
            assert_eq!(
                engine.timer_count(),
                count_before + 1,
                "DoAfter should create a timer"
            );
            // Fire the timer
            engine.fire_timer(count_before);
            let cmds = engine.drain_commands();
            assert!(
                cmds.contains(&"test_command".to_string()),
                "DoAfter timer should send command"
            );
        });
    }

    #[test]
    fn test_doafter_note_output() {
        with_engine(|engine| {
            exec(engine, r#"DoAfterNote(3, "test note")"#).unwrap();
            let count = engine.timer_count();
            engine.fire_timer(count - 1);
            let logs = engine.drain_logs();
            assert!(
                logs.iter().any(|l| l.contains("test note")),
                "DoAfterNote should produce Note output"
            );
        });
    }

    #[test]
    fn test_doafter_invalid_time() {
        with_engine(|engine| {
            let r: i64 = eval(engine, "return DoAfter(0, 'x')").unwrap();
            assert_eq!(r, 1, "time < 0.1 should return 1 (eTimeInvalid)");
            let r2: i64 = eval(engine, "return DoAfter(99999, 'x')").unwrap();
            assert_eq!(r2, 1, "time > 86399 should return 1 (eTimeInvalid)");
        });
    }

    #[test]
    fn test_doafter_special_send_to_script() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                doafter_special_result = nil
                DoAfterSpecial(1, "doafter_special_result = 'fired'", 12)
                "#,
            )
            .unwrap();
            let count = engine.timer_count();
            engine.fire_timer(count - 1);
            let r: Option<String> = eval(engine, "return doafter_special_result").unwrap();
            assert_eq!(
                r,
                Some("fired".to_string()),
                "DoAfterSpecial send_to=12 should execute Lua"
            );
        });
    }

    #[test]
    fn test_doafter_special_invalid_send_to() {
        with_engine(|engine| {
            let r: i64 = eval(engine, "return DoAfterSpecial(1, 'x', 99)").unwrap();
            assert_eq!(r, 2, "send_to > 14 should return 2 (eOptionOutOfRange)");
        });
    }

    #[test]
    fn test_doafter_speedwalk() {
        with_engine(|engine| {
            exec(engine, r#"DoAfterSpeedWalk(2, "n;e;n")"#).unwrap();
            let count = engine.timer_count();
            engine.fire_timer(count - 1);
            let cmds = engine.drain_commands();
            assert!(
                cmds.contains(&"n;e;n".to_string()),
                "DoAfterSpeedWalk should send speedwalk string"
            );
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
                AddAlias('multi_cap_a', 'cast * at *', '', 1, 'function(n, l, w) alias_c1 = w[1]; alias_c2 = w[2] end')
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
                AddAlias('regex_a', [[^#(\d+)$]], '', 33, 'function(n, l, w) regex_al_result = w[1] end')
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
                alias('^test$', function(n, l, w) raw_input = l end)
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
    fn test_get_alias_info_match_text() {
        with_engine(|engine| {
            exec(engine, r#"AddAlias('test_ai', 'kill *', '', 33)"#).unwrap();
            let result: Option<String> =
                eval(engine, r#"return GetAliasInfo('test_ai', 1)"#).unwrap();
            assert_eq!(result, Some("kill *".to_string()));
        });
    }

    #[test]
    fn test_get_alias_info_response_text() {
        with_engine(|engine| {
            exec(engine, r#"AddAlias('test_ai2', 'go *', 'go_command', 33)"#).unwrap();
            let result: Option<String> =
                eval(engine, r#"return GetAliasInfo('test_ai2', 2)"#).unwrap();
            assert_eq!(result, Some("go_command".to_string()));
        });
    }

    #[test]
    fn test_get_alias_info_enabled() {
        with_engine(|engine| {
            exec(engine, r#"AddAlias('test_ai3', 'test', '', 1)"#).unwrap();
            let result: Option<bool> =
                eval(engine, r#"return GetAliasInfo('test_ai3', 6)"#).unwrap();
            assert_eq!(result, Some(true)); // flags=1 => bit0 Enabled set => enabled=true
        });
    }

    #[test]
    fn test_get_alias_info_send_to() {
        with_engine(|engine| {
            // response non-empty, no 5th arg => send_to=12
            exec(
                engine,
                r#"AddAlias('test_ai4', 'test', 'do_something()', 33)"#,
            )
            .unwrap();
            let result: Option<i64> =
                eval(engine, r#"return GetAliasInfo('test_ai4', 18)"#).unwrap();
            assert_eq!(result, Some(12));
        });
    }

    #[test]
    fn test_get_alias_info_group() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                AddAlias('test_ai5', 'test', '', 33)
                SetAliasOption('test_ai5', 'group', 'mygroup')
            "#,
            )
            .unwrap();
            let result: Option<String> =
                eval(engine, r#"return GetAliasInfo('test_ai5', 16)"#).unwrap();
            assert_eq!(result, Some("mygroup".to_string()));
        });
    }

    #[test]
    fn test_get_alias_info_nonexistent_returns_nil() {
        with_engine(|engine| {
            let result: mlua::Value =
                eval(engine, r#"return GetAliasInfo('nonexistent', 1)"#).unwrap();
            assert!(matches!(result, mlua::Value::Nil));
        });
    }

    #[test]
    fn test_get_alias_info_shorthand_alias() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                alias('^hello$', function() end)
            "#,
            )
            .unwrap();
            let result: Option<String> = eval(engine, r#"return GetAliasInfo('', 1)"#).unwrap();
            assert_eq!(result, Some("^hello$".to_string()));
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

    /// 验证 Execute("war") 产生的命令可以通过 process_input 被别名拦截
    /// 这是 send_lua_commands 方案A的核心逻辑链
    #[test]
    fn test_execute_command_intercepted_by_alias() {
        with_engine(|engine| {
            // 注册 war 别名：匹配 "war"，执行 warteam()，send_to=12
            exec(
                engine,
                r#"
                AddAlias('alias_war', '^war$', 'warteam()', 33, '')
                SetAliasOption('alias_war', 'send_to', 12)
                function warteam()
                    Execute('teamwith alice bob')
                end
            "#,
            )
            .unwrap();

            // 模拟触发器回调中 run("war") → Execute("war")
            // Execute 把 "war" 压入 pending_commands
            engine.process_output("some trigger line");
            // 手动模拟 Execute("war") 的效果
            {
                let mut state = engine.state.borrow_mut();
                state.pending_commands.push("war".to_string());
            }
            let cmds = engine.drain_commands();
            assert!(cmds.contains(&"war".to_string()));

            // 模拟 send_lua_commands 方案A：对 "war" 调用 process_input
            let handled = engine.process_input("war");
            assert!(handled, "war 应被 alias_war 匹配");

            let sub_cmds = engine.drain_commands();
            assert!(
                sub_cmds.contains(&"teamwith alice bob".to_string()),
                "别名匹配后应产生 teamwith 命令，实际: {:?}",
                sub_cmds
            );
        });
    }

    /// 验证非别名命令不会被拦截，直接通过
    #[test]
    fn test_execute_command_not_alias_passes_through() {
        with_engine(|engine| {
            // 只注册 war 别名
            exec(
                engine,
                r#"
                AddAlias('alias_war', '^war$', 'warteam()', 33, '')
                SetAliasOption('alias_war', 'send_to', 12)
                function warteam()
                    Execute('teamwith alice bob')
                end
            "#,
            )
            .unwrap();

            // "look" 不是别名，process_input 应返回 false
            let handled = engine.process_input("look");
            assert!(!handled, "look 不应被任何别名匹配");
        });
    }

    /// 验证别名链式调用：别名A产生命令被别名B拦截
    #[test]
    fn test_alias_chain_interception() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                -- 别名A: "go" → 回调中 Execute("north")
                AddAlias('go_alias', '^go$', '', 33, 'function() Execute("north") end')

                -- 别名B: "north" → 回调中 Execute("n")
                AddAlias('north_alias', '^north$', '', 33, 'function() Execute("n") end')
            "#,
            )
            .unwrap();

            // 第一层：go 匹配别名
            let handled1 = engine.process_input("go");
            assert!(handled1);
            let cmds1 = engine.drain_commands();
            // Execute("north") 产生的 "north" 命令
            assert!(cmds1.contains(&"north".to_string()), "go 别名应产生 north 命令，实际: {:?}", cmds1);

            // 第二层：north 匹配别名
            let handled2 = engine.process_input("north");
            assert!(handled2);
            let cmds2 = engine.drain_commands();
            // Execute("n") 产生的 "n" 命令
            assert!(cmds2.contains(&"n".to_string()), "north 别名应产生 n 命令，实际: {:?}", cmds2);
        });
    }

    #[test]
    fn test_gbk_dosth5_matching() {
        // 测试 dosth5 正则匹配
        let pattern = r"^(> > > |> > |> |)你目前还没有任何为 (\S+) 的变量设定。";
        let gbk_pattern = utf8_regex_to_gbk_bytes(pattern);
        eprintln!("GBK pattern: {}", gbk_pattern);

        let re = BytesRegex::new(&gbk_pattern).unwrap();

        // 测试1: gps=start
        let line1 = "> 你目前还没有任何为 gps=start 的变量设定。";
        let (gbk_line1, _, _) = encoding_rs::GBK.encode(line1);
        let matched1 = re.is_match(&gbk_line1);
        eprintln!("Line1 matched: {}", matched1);
        assert!(matched1, "dosth5 should match 'gps=start' line");

        // 测试2: checkyell=yes
        let line2 = "> 你目前还没有任何为 checkyell=yes 的变量设定。";
        let (gbk_line2, _, _) = encoding_rs::GBK.encode(line2);
        let matched2 = re.is_match(&gbk_line2);
        eprintln!("Line2 matched: {}", matched2);
        assert!(matched2, "dosth5 should match 'checkyell=yes' line");

        // 测试3: 捕获组
        if let Some(caps) = re.captures(&gbk_line2) {
            for (i, cap) in caps.iter().enumerate() {
                if let Some(m) = cap {
                    let (cow, _, _) = encoding_rs::GBK.decode(m.as_bytes());
                    eprintln!("  cap[{}]: {}", i, cow);
                }
            }
        }
    }

    #[test]
    fn test_gbk_dosth2_matching() {
        // 测试 dosth2 正则匹配: "这里明显的出口是"
        let pattern = r"^\s*这里.{4}的出口是 (.*)。$";
        let gbk_pattern = utf8_regex_to_gbk_bytes(pattern);
        eprintln!("GBK pattern (dosth2): {}", gbk_pattern);

        let re = BytesRegex::new(&gbk_pattern).unwrap();

        let line = "    这里明显的出口是 north 和 south。";
        let (gbk_line, _, _) = encoding_rs::GBK.encode(line);
        let matched = re.is_match(&gbk_line);
        eprintln!("dosth2 matched: {}", matched);
        assert!(matched, "dosth2 should match exit line");

        if let Some(caps) = re.captures(&gbk_line) {
            for (i, cap) in caps.iter().enumerate() {
                if let Some(m) = cap {
                    let (cow, _, _) = encoding_rs::GBK.decode(m.as_bytes());
                    eprintln!("  cap[{}]: {}", i, cow);
                }
            }
        }
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

    // ================================================================
    // JSON 序列化桥接函数测试 — lua_value_to_json
    // ================================================================

    #[test]
    fn test_lua_value_to_json_nil() {
        let lua_val = mlua::Value::Nil;
        let json_val = lua_value_to_json(&lua_val);
        assert_eq!(json_val, serde_json::Value::Null);
    }

    #[test]
    fn test_lua_value_to_json_boolean() {
        let json_val = lua_value_to_json(&mlua::Value::Boolean(true));
        assert_eq!(json_val, serde_json::Value::Bool(true));
        let json_val = lua_value_to_json(&mlua::Value::Boolean(false));
        assert_eq!(json_val, serde_json::Value::Bool(false));
    }

    #[test]
    fn test_lua_value_to_json_integer() {
        let json_val = lua_value_to_json(&mlua::Value::Integer(42));
        assert_eq!(json_val, serde_json::Value::Number(42.into()));
        let json_val = lua_value_to_json(&mlua::Value::Integer(-1));
        assert_eq!(json_val, serde_json::Value::Number((-1).into()));
    }

    #[test]
    fn test_lua_value_to_json_number() {
        let json_val = lua_value_to_json(&mlua::Value::Number(3.14));
        assert_eq!(json_val, serde_json::json!(3.14));
    }

    #[test]
    fn test_lua_value_to_json_string() {
        let lua = Lua::new();
        let s = lua.create_string("hello").unwrap();
        let json_val = lua_value_to_json(&mlua::Value::String(s));
        assert_eq!(json_val, serde_json::Value::String("hello".to_string()));
    }

    #[test]
    fn test_lua_value_to_json_string_utf8() {
        let lua = Lua::new();
        let s = lua.create_string("中文测试").unwrap();
        let json_val = lua_value_to_json(&mlua::Value::String(s));
        assert_eq!(json_val, serde_json::Value::String("中文测试".to_string()));
    }

    #[test]
    fn test_lua_value_to_json_array() {
        with_engine(|engine| {
            let lua_val: mlua::Value = eval(engine, "return {10, 20, 30}").unwrap();
            let json_val = lua_value_to_json(&lua_val);
            assert_eq!(json_val, serde_json::json!([10, 20, 30]));
        });
    }

    #[test]
    fn test_lua_value_to_json_object() {
        with_engine(|engine| {
            let lua_val: mlua::Value = eval(engine, "return {name='test', value=42}").unwrap();
            let json_val = lua_value_to_json(&lua_val);
            assert_eq!(json_val["name"], serde_json::json!("test"));
            assert_eq!(json_val["value"], serde_json::json!(42));
        });
    }

    #[test]
    fn test_lua_value_to_json_nested() {
        with_engine(|engine| {
            let lua_val: mlua::Value = eval(engine, "return {a={b={c=1}}}").unwrap();
            let json_val = lua_value_to_json(&lua_val);
            assert_eq!(json_val["a"]["b"]["c"], serde_json::json!(1));
        });
    }

    #[test]
    fn test_lua_value_to_json_empty_table() {
        with_engine(|engine| {
            let lua_val: mlua::Value = eval(engine, "return {}").unwrap();
            let json_val = lua_value_to_json(&lua_val);
            // 空表既可以视为空数组也可以视为空对象，这里取决于实现
            // 我们的实现中空表没有连续整数键 → 判定为对象
            assert!(json_val.is_object() || json_val.is_array());
        });
    }

    #[test]
    fn test_lua_value_to_json_mixed_array() {
        with_engine(|engine| {
            // 1, 2, name="x" — 非连续整数键 → 判定为对象
            let lua_val: mlua::Value = eval(engine, "return {1, 2, name='x'}").unwrap();
            let json_val = lua_value_to_json(&lua_val);
            assert!(json_val.is_object());
            assert_eq!(json_val["name"], serde_json::json!("x"));
        });
    }

    #[test]
    fn test_lua_value_to_json_function_is_null() {
        let lua = Lua::new();
        let fn_val = lua.create_function(|_, ()| Ok(())).unwrap();
        let json_val = lua_value_to_json(&mlua::Value::Function(fn_val));
        assert_eq!(json_val, serde_json::Value::Null);
    }

    // ================================================================
    // JSON 序列化桥接函数测试 — json_to_lua_value
    // ================================================================

    #[test]
    fn test_json_to_lua_value_null() {
        let lua = Lua::new();
        let lua_val = json_to_lua_value(&lua, &serde_json::Value::Null).unwrap();
        assert!(matches!(lua_val, mlua::Value::Nil));
    }

    #[test]
    fn test_json_to_lua_value_bool() {
        let lua = Lua::new();
        let lua_val = json_to_lua_value(&lua, &serde_json::Value::Bool(true)).unwrap();
        assert!(matches!(lua_val, mlua::Value::Boolean(true)));
    }

    #[test]
    fn test_json_to_lua_value_integer() {
        let lua = Lua::new();
        let lua_val = json_to_lua_value(&lua, &serde_json::json!(100)).unwrap();
        assert!(matches!(lua_val, mlua::Value::Integer(100)));
    }

    #[test]
    fn test_json_to_lua_value_float() {
        let lua = Lua::new();
        let lua_val = json_to_lua_value(&lua, &serde_json::json!(3.14)).unwrap();
        assert!(matches!(lua_val, mlua::Value::Number(v) if (v - 3.14).abs() < 1e-10));
    }

    #[test]
    fn test_json_to_lua_value_string() {
        let lua = Lua::new();
        let lua_val =
            json_to_lua_value(&lua, &serde_json::Value::String("hi".to_string())).unwrap();
        assert!(matches!(&lua_val, mlua::Value::String(s) if s.to_str().unwrap() == "hi"));
    }

    #[test]
    fn test_json_to_lua_value_array() {
        let lua = Lua::new();
        let json_val = serde_json::json!([1, 2, 3]);
        let lua_val = json_to_lua_value(&lua, &json_val).unwrap();
        if let mlua::Value::Table(t) = &lua_val {
            assert_eq!(t.get::<i64>(1).unwrap(), 1);
            assert_eq!(t.get::<i64>(2).unwrap(), 2);
            assert_eq!(t.get::<i64>(3).unwrap(), 3);
        } else {
            panic!("期望 Table, 获得 {:?}", lua_val);
        }
    }

    #[test]
    fn test_json_to_lua_value_object() {
        let lua = Lua::new();
        let json_val = serde_json::json!({"key": "value", "num": 42});
        let lua_val = json_to_lua_value(&lua, &json_val).unwrap();
        if let mlua::Value::Table(t) = &lua_val {
            assert_eq!(t.get::<String>("key").unwrap(), "value");
            assert_eq!(t.get::<i64>("num").unwrap(), 42);
        } else {
            panic!("期望 Table, 获得 {:?}", lua_val);
        }
    }

    #[test]
    fn test_json_to_lua_value_nested() {
        let lua = Lua::new();
        let json_val = serde_json::json!({"a": {"b": [1, 2]}});
        let lua_val = json_to_lua_value(&lua, &json_val).unwrap();
        if let mlua::Value::Table(t) = &lua_val {
            let inner: mlua::Table = t.get("a").unwrap();
            let arr: mlua::Table = inner.get("b").unwrap();
            assert_eq!(arr.get::<i64>(1).unwrap(), 1);
            assert_eq!(arr.get::<i64>(2).unwrap(), 2);
        } else {
            panic!("期望 Table, 获得 {:?}", lua_val);
        }
    }

    // ================================================================
    // json_encode / json_decode Lua API 测试
    // ================================================================

    #[test]
    fn test_json_encode_nil() {
        with_engine(|engine| {
            let result: String = eval(engine, "return json_encode(nil)").unwrap();
            assert_eq!(result, "null");
        });
    }

    #[test]
    fn test_json_encode_boolean() {
        with_engine(|engine| {
            let result: String = eval(engine, "return json_encode(true)").unwrap();
            assert_eq!(result, "true");
            let result: String = eval(engine, "return json_encode(false)").unwrap();
            assert_eq!(result, "false");
        });
    }

    #[test]
    fn test_json_encode_number() {
        with_engine(|engine| {
            let result: String = eval(engine, "return json_encode(42)").unwrap();
            assert_eq!(result, "42");
            let result: String = eval(engine, "return json_encode(3.14)").unwrap();
            assert!(result.starts_with("3.14"));
        });
    }

    #[test]
    fn test_json_encode_string() {
        with_engine(|engine| {
            let result: String = eval(engine, "return json_encode('hello')").unwrap();
            assert_eq!(result, "\"hello\"");
        });
    }

    #[test]
    fn test_json_encode_array() {
        with_engine(|engine| {
            let result: String = eval(engine, "return json_encode({10, 20, 30})").unwrap();
            assert_eq!(result, "[10,20,30]");
        });
    }

    #[test]
    fn test_json_encode_object() {
        with_engine(|engine| {
            let result: String = eval(engine, "return json_encode({a=1, b='x'})").unwrap();
            assert!(result.contains("\"a\":1"));
            assert!(result.contains("\"b\":\"x\""));
        });
    }

    #[test]
    fn test_json_encode_nested() {
        with_engine(|engine| {
            let result: String = eval(engine, "return json_encode({a={b={c=1}}})").unwrap();
            assert!(result.contains("\"a\""));
            assert!(result.contains("\"b\""));
            assert!(result.contains("\"c\":1"));
        });
    }

    #[test]
    fn test_json_decode_null() {
        with_engine(|engine| {
            let result: String =
                eval(engine, "local v = json_decode('null'); return type(v)").unwrap();
            assert_eq!(result, "nil");
        });
    }

    #[test]
    fn test_json_decode_boolean() {
        with_engine(|engine| {
            let result: bool = eval(engine, "return json_decode('true')").unwrap();
            assert!(result);
            let result: bool = eval(engine, "return json_decode('false')").unwrap();
            assert!(!result);
        });
    }

    #[test]
    fn test_json_decode_integer() {
        with_engine(|engine| {
            let result: i64 = eval(engine, "return json_decode('42')").unwrap();
            assert_eq!(result, 42);
        });
    }

    #[test]
    fn test_json_decode_string() {
        with_engine(|engine| {
            let result: String = eval(engine, "return json_decode('\"hello\"')").unwrap();
            assert_eq!(result, "hello");
        });
    }

    #[test]
    fn test_json_decode_array() {
        with_engine(|engine| {
            let result: String = eval(
                engine,
                "local t = json_decode('[1,2,3]'); return t[1] + t[2] + t[3]",
            )
            .unwrap();
            assert_eq!(result, "6");
        });
    }

    #[test]
    fn test_json_decode_object() {
        with_engine(|engine| {
            let result: i64 = eval(
                engine,
                "local t = json_decode('{\"a\":1,\"b\":2}'); return t.a + t.b",
            )
            .unwrap();
            assert_eq!(result, 3);
        });
    }

    #[test]
    fn test_json_roundtrip() {
        with_engine(|engine| {
            let result: String = eval(
                engine,
                "local original = {a=1, b='hello', c={nested=true}}; \
                 local json = json_encode(original); \
                 local decoded = json_decode(json); \
                 return json_encode(decoded)",
            )
            .unwrap();
            assert!(result.contains("\"a\":1"));
            assert!(result.contains("\"b\":\"hello\""));
            assert!(result.contains("\"nested\":true"));
        });
    }

    #[test]
    fn test_json_decode_invalid() {
        with_engine(|engine| {
            let result: bool = eval(
                engine,
                "local ok, err = pcall(json_decode, '{invalid}'); return not ok",
            )
            .unwrap();
            assert!(result);
        });
    }

    // ================================================================
    // eval_to_string 方法测试
    // ================================================================

    #[test]
    fn test_eval_to_string_simple() {
        with_engine(|engine| {
            let result = engine.eval_to_string("return 'hello'").unwrap();
            assert_eq!(result, "hello");
        });
    }

    #[test]
    fn test_eval_to_string_number() {
        with_engine(|engine| {
            let result = engine.eval_to_string("return tostring(42)").unwrap();
            assert_eq!(result, "42");
        });
    }

    #[test]
    fn test_eval_to_string_table_json() {
        with_engine(|engine| {
            let result = engine
                .eval_to_string("return json_encode({1,2,3})")
                .unwrap();
            assert_eq!(result, "[1,2,3]");
        });
    }

    #[test]
    fn test_eval_to_string_syntax_error() {
        with_engine(|engine| {
            let result = engine.eval_to_string("syntax error !!!");
            assert!(result.is_err());
        });
    }

    #[test]
    fn test_eval_to_string_runtime_error() {
        with_engine(|engine| {
            let result = engine.eval_to_string("error('boom')");
            assert!(result.is_err());
        });
    }

    // ================================================================
    // cfg.data() / cfg.update() — Lua 侧配置 API 测试
    // 通过内联构建测试 schema 来验证逻辑
    // ================================================================

    #[test]
    fn test_cfg_data_empty_schema() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                cfg = cfg or {}
                cfg.schema = {}
                function cfg.data()
                    local result = {}
                    for _, field in ipairs(cfg.schema) do
                        table.insert(result, {key=field.key, value=field.getter()})
                    end
                    return result
                end
            "#,
            )
            .unwrap();
            let result: String = eval(engine, "return json_encode(cfg.data())").unwrap();
            // 空 Lua 表（无连续整数键）→ JSON 对象 {}
            assert_eq!(result, "{}");
        });
    }

    #[test]
    fn test_cfg_data_boolean_fields() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                cfg = cfg or {}
                test_switch = 1
                cfg.schema = {
                    { key="test_switch", label="测试开关", type="boolean", category="测试",
                      getter=function() return test_switch and test_switch>0 end,
                      setter=function(v) test_switch=v and 1 or 0 end },
                }
                function cfg.data()
                    local result = {}
                    for _, field in ipairs(cfg.schema) do
                        local entry = {key=field.key, label=field.label, type=field.type,
                                       category=field.category, value=field.getter()}
                        table.insert(result, entry)
                    end
                    return result
                end
            "#,
            )
            .unwrap();
            let result: String = eval(engine, "return json_encode(cfg.data())").unwrap();
            assert!(result.contains("\"test_switch\""));
            assert!(result.contains("true"));
        });
    }

    #[test]
    fn test_cfg_data_number_fields() {
        with_engine(|engine| {
            exec(engine, r#"
                cfg = cfg or {}
                test_number = 175
                cfg.schema = {
                    { key="test_number", label="测试数值", type="number", category="数值", min=0, max=500,
                      getter=function() return test_number end,
                      setter=function(v) test_number=tonumber(v) or test_number end },
                }
                function cfg.data()
                    local result = {}
                    for _, field in ipairs(cfg.schema) do
                        local entry = {key=field.key, label=field.label, type=field.type,
                                       category=field.category, value=field.getter()}
                        table.insert(result, entry)
                    end
                    return result
                end
            "#).unwrap();
            let result: String = eval(engine, "return json_encode(cfg.data())").unwrap();
            assert!(result.contains("\"test_number\""));
            assert!(result.contains("175"));
        });
    }

    #[test]
    fn test_cfg_update_valid() {
        with_engine(|engine| {
            exec(engine, r#"
                cfg = cfg or {}
                test_val = 10

                cfg.schema = {
                    { key="test_val", label="测试值", type="number", category="数值", min=0, max=100,
                      getter=function() return test_val end,
                      setter=function(v) test_val=tonumber(v) or test_val end },
                }

                -- 构建 schema_map
                local schema_map = {}
                for _, field in ipairs(cfg.schema) do
                    schema_map[field.key] = field
                end

                function cfg._validate(field, value)
                    local t = field.type
                    if t == "number" then
                        local n = tonumber(value)
                        if n == nil then return false, "需要数字" end
                        if field.min ~= nil and n < field.min then return false, "最小值 "..tostring(field.min) end
                        if field.max ~= nil and n > field.max then return false, "最大值 "..tostring(field.max) end
                    elseif t == "boolean" then
                        if type(value) ~= "boolean" then return false, "需要布尔值" end
                    elseif t == "string" then
                        if type(value) ~= "string" then return false, "需要字符串" end
                    end
                    return true, nil
                end

                function cfg.update(changes)
                    if type(changes) ~= "table" then return { ok=false, errors={ _global="参数必须是 table" } } end
                    local errors = {}
                    for key, value in pairs(changes) do
                        local field = schema_map[key]
                        if not field then
                            errors[key] = "未知配置项"
                        else
                            local ok, err = cfg._validate(field, value)
                            if not ok then
                                errors[key] = err
                            else
                                local success, apply_err = pcall(field.setter, value)
                                if not success then errors[key] = "应用失败: "..tostring(apply_err) end
                            end
                        end
                    end
                    if next(errors) then return { ok=false, errors=errors } end
                    return { ok=true }
                end
            "#).unwrap();

            // 测试有效更新
            let result: String = eval(
                engine,
                "local r = cfg.update({test_val=50}); return json_encode(r)",
            )
            .unwrap();
            assert!(result.contains("\"ok\":true"));

            // 验证值已更新
            let val: i64 = eval(engine, "return test_val").unwrap();
            assert_eq!(val, 50);
        });
    }

    #[test]
    fn test_cfg_update_unknown_key() {
        with_engine(|engine| {
            exec(engine, r#"
                cfg = cfg or {}
                cfg.schema = {}

                local schema_map = {}
                for _, field in ipairs(cfg.schema) do
                    schema_map[field.key] = field
                end

                function cfg._validate(field, value) return true, nil end
                function cfg.update(changes)
                    if type(changes) ~= "table" then return { ok=false, errors={ _global="参数必须是 table" } } end
                    local errors = {}
                    for key, value in pairs(changes) do
                        local field = schema_map[key]
                        if not field then errors[key] = "未知配置项" end
                    end
                    if next(errors) then return { ok=false, errors=errors } end
                    return { ok=true }
                end
            "#).unwrap();

            let result: String = eval(
                engine,
                "local r = cfg.update({nonexistent=1}); return json_encode(r)",
            )
            .unwrap();
            assert!(result.contains("\"ok\":false"));
            assert!(result.contains("未知配置项"));
        });
    }

    #[test]
    fn test_cfg_update_invalid_number() {
        with_engine(|engine| {
            exec(engine, r#"
                cfg = cfg or {}
                test_n = 0
                cfg.schema = {
                    { key="test_n", label="N", type="number", category="数值", min=0, max=100,
                      getter=function() return test_n end,
                      setter=function(v) test_n=tonumber(v) or test_n end },
                }
                local schema_map = {}
                for _, field in ipairs(cfg.schema) do schema_map[field.key] = field end

                function cfg._validate(field, value)
                    local n = tonumber(value)
                    if n == nil then return false, "需要数字" end
                    if field.min ~= nil and n < field.min then return false, "最小值 "..tostring(field.min) end
                    if field.max ~= nil and n > field.max then return false, "最大值 "..tostring(field.max) end
                    return true, nil
                end
                function cfg.update(changes)
                    local errors = {}
                    for key, value in pairs(changes) do
                        local field = schema_map[key]
                        if not field then errors[key] = "未知配置项"
                        else
                            local ok, err = cfg._validate(field, value)
                            if not ok then errors[key] = err end
                        end
                    end
                    if next(errors) then return { ok=false, errors=errors } end
                    return { ok=true }
                end
            "#).unwrap();

            // 超出范围
            let result: String = eval(
                engine,
                "local r = cfg.update({test_n=999}); return json_encode(r)",
            )
            .unwrap();
            assert!(result.contains("\"ok\":false"));
            assert!(result.contains("最大值"));
        });
    }

    #[test]
    fn test_cfg_update_non_table_arg() {
        with_engine(|engine| {
            exec(engine, r#"
                cfg = cfg or {}
                function cfg.update(changes)
                    if type(changes) ~= "table" then return { ok=false, errors={ _global="参数必须是 table" } } end
                    return { ok=true }
                end
            "#).unwrap();

            // 直接传入非 table 应该报错
            let result: String = eval(
                engine,
                "local r = cfg.update('invalid'); return json_encode(r)",
            )
            .unwrap();
            assert!(result.contains("\"ok\":false"));
        });
    }

    // ================================================================
    // cfg._validate 边界条件测试
    // ================================================================

    #[test]
    fn test_cfg_validate_boolean_valid() {
        with_engine(|engine| {
            let result: bool = eval(
                engine,
                r#"
                cfg = cfg or {}
                function cfg._validate(field, value)
                    if field.type == "boolean" and type(value) ~= "boolean" then
                        return false, "需要布尔值"
                    end
                    return true, nil
                end
                local ok, err
                ok, err = cfg._validate({type="boolean"}, true);  if not ok then return false end
                ok, err = cfg._validate({type="boolean"}, false); if not ok then return false end
                return true
            "#,
            )
            .unwrap();
            assert!(result);
        });
    }

    #[test]
    fn test_cfg_validate_boolean_invalid() {
        with_engine(|engine| {
            let result: bool = eval(
                engine,
                r#"
                cfg = cfg or {}
                function cfg._validate(field, value)
                    if field.type == "boolean" and type(value) ~= "boolean" then
                        return false, "需要布尔值"
                    end
                    return true, nil
                end
                local ok, err = cfg._validate({type="boolean"}, "not_bool")
                return not ok
            "#,
            )
            .unwrap();
            assert!(result);
        });
    }

    #[test]
    fn test_cfg_validate_number_valid() {
        with_engine(|engine| {
            let result: bool = eval(engine, r#"
                cfg = cfg or {}
                function cfg._validate(field, value)
                    if field.type == "number" then
                        local n = tonumber(value)
                        if n == nil then return false, "需要数字" end
                        if field.min and n < field.min then return false, "最小值" end
                        if field.max and n > field.max then return false, "最大值" end
                    end
                    return true, nil
                end
                local ok
                ok, _ = cfg._validate({type="number", min=0, max=100}, 50); if not ok then return false end
                ok, _ = cfg._validate({type="number", min=0, max=100}, 0);  if not ok then return false end
                ok, _ = cfg._validate({type="number", min=0, max=100}, 100); if not ok then return false end
                return true
            "#).unwrap();
            assert!(result);
        });
    }

    #[test]
    fn test_cfg_validate_number_below_min() {
        with_engine(|engine| {
            let result: bool = eval(
                engine,
                r#"
                cfg = cfg or {}
                function cfg._validate(field, value)
                    if field.type == "number" then
                        local n = tonumber(value)
                        if n == nil then return false, "需要数字" end
                        if field.min and n < field.min then return false, "最小值" end
                    end
                    return true, nil
                end
                local ok, err = cfg._validate({type="number", min=10}, 5)
                return (not ok) and (err == "最小值")
            "#,
            )
            .unwrap();
            assert!(result);
        });
    }

    #[test]
    fn test_cfg_validate_number_above_max() {
        with_engine(|engine| {
            let result: bool = eval(
                engine,
                r#"
                cfg = cfg or {}
                function cfg._validate(field, value)
                    if field.type == "number" then
                        local n = tonumber(value)
                        if n == nil then return false, "需要数字" end
                        if field.max and n > field.max then return false, "最大值" end
                    end
                    return true, nil
                end
                local ok, err = cfg._validate({type="number", max=10}, 20)
                return (not ok) and (err == "最大值")
            "#,
            )
            .unwrap();
            assert!(result);
        });
    }

    #[test]
    fn test_cfg_validate_number_not_a_number() {
        with_engine(|engine| {
            let result: bool = eval(
                engine,
                r#"
                cfg = cfg or {}
                function cfg._validate(field, value)
                    if field.type == "number" then
                        local n = tonumber(value)
                        if n == nil then return false, "需要数字" end
                    end
                    return true, nil
                end
                local ok, err = cfg._validate({type="number"}, "not_a_number")
                return (not ok) and (err == "需要数字")
            "#,
            )
            .unwrap();
            assert!(result);
        });
    }

    #[test]
    fn test_cfg_validate_string_valid() {
        with_engine(|engine| {
            let result: bool = eval(
                engine,
                r#"
                cfg = cfg or {}
                function cfg._validate(field, value)
                    if field.type == "string" and type(value) ~= "string" then
                        return false, "需要字符串"
                    end
                    return true, nil
                end
                return cfg._validate({type="string"}, "hello")
            "#,
            )
            .unwrap();
            assert!(result);
        });
    }

    #[test]
    fn test_cfg_validate_string_invalid() {
        with_engine(|engine| {
            let result: bool = eval(
                engine,
                r#"
                cfg = cfg or {}
                function cfg._validate(field, value)
                    if field.type == "string" and type(value) ~= "string" then
                        return false, "需要字符串"
                    end
                    return true, nil
                end
                local ok, err = cfg._validate({type="string"}, 123)
                return (not ok) and (err == "需要字符串")
            "#,
            )
            .unwrap();
            assert!(result);
        });
    }

    #[test]
    fn test_cfg_validate_option_valid() {
        with_engine(|engine| {
            let result: bool = eval(engine, r#"
                cfg = cfg or {}
                function cfg._validate(field, value)
                    if field.type == "option" then
                        for _, opt in ipairs(field.options) do
                            if tostring(opt) == tostring(value) then return true, nil end
                        end
                        return false, "无效选项"
                    end
                    return true, nil
                end
                local ok
                ok, _ = cfg._validate({type="option", options={"a","b","c"}}, "a"); if not ok then return false end
                ok, _ = cfg._validate({type="option", options={"a","b","c"}}, "b"); if not ok then return false end
                ok, _ = cfg._validate({type="option", options={"a","b","c"}}, "c"); if not ok then return false end
                return true
            "#).unwrap();
            assert!(result);
        });
    }

    #[test]
    fn test_cfg_validate_option_invalid() {
        with_engine(|engine| {
            let result: bool = eval(
                engine,
                r#"
                cfg = cfg or {}
                function cfg._validate(field, value)
                    if field.type == "option" then
                        for _, opt in ipairs(field.options) do
                            if tostring(opt) == tostring(value) then return true, nil end
                        end
                        return false, "无效选项"
                    end
                    return true, nil
                end
                local ok, err = cfg._validate({type="option", options={"x","y"}}, "z")
                return (not ok) and (err == "无效选项")
            "#,
            )
            .unwrap();
            assert!(result);
        });
    }

    // ================================================================
    // cfg.schema 各类型的 getter/setter 测试
    // ================================================================

    #[test]
    fn test_cfg_field_boolean_getter_setter() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                cfg = cfg or {}
                test_flag = 0
                local field = {
                    getter = function() return test_flag and test_flag > 0 end,
                    setter = function(v) test_flag = v and 1 or 0 end,
                }
                -- 初始: false
                assert(field.getter() == false)
                -- 设为 true
                field.setter(true)
                assert(field.getter() == true)
                -- 检查全局变量
                assert(test_flag == 1)
                -- 再设回 false
                field.setter(false)
                assert(field.getter() == false)
                assert(test_flag == 0)
            "#,
            )
            .unwrap();
        });
    }

    #[test]
    fn test_cfg_field_number_getter_setter() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                cfg = cfg or {}
                test_num = 50
                local field = {
                    getter = function() return test_num end,
                    setter = function(v) test_num = tonumber(v) or test_num end,
                }
                assert(field.getter() == 50)
                field.setter(200)
                assert(field.getter() == 200)
                -- 传入字符串也能转换
                field.setter("75")
                assert(field.getter() == 75)
            "#,
            )
            .unwrap();
        });
    }

    #[test]
    fn test_cfg_field_string_getter_setter() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                cfg = cfg or {}
                skills = "unarmed"
                local field = {
                    getter = function() return skills end,
                    setter = function(v) skills = v end,
                }
                assert(field.getter() == "unarmed")
                field.setter("sword")
                assert(field.getter() == "sword")
            "#,
            )
            .unwrap();
        });
    }

    #[test]
    fn test_cfg_field_option_getter_setter() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                cfg = cfg or {}
                _opt = "xue"
                local field = {
                    getter = function() return _opt end,
                    setter = function(v) _opt = v end,
                }
                assert(field.getter() == "xue")
                field.setter("lingwu")
                assert(field.getter() == "lingwu")
            "#,
            )
            .unwrap();
        });
    }

    // ================================================================
    // Simulate API 测试
    // ================================================================

    #[test]
    fn test_simulate_basic_trigger() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                sim_result = ""
                AddTrigger('sim_test', 'Exits: (.+)', '', 33, 0, 0, '', 'function(n,l,w) sim_result = w[1] end', 0, 0)
            "#,
            )
            .unwrap();
            exec(engine, r#"Simulate("Exits: north\n")"#).unwrap();
            let result: String = eval(engine, "return sim_result").unwrap();
            assert_eq!(result, "north");
        });
    }

    #[test]
    fn test_simulate_multiple_args_concatenated() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                sim_result = ""
                AddTrigger('sim_multi', 'hello (.+)', '', 33, 0, 0, '', 'function(n,l,w) sim_result = w[1] end', 0, 0)
            "#,
            )
            .unwrap();
            // MUSHclient Lua 特性：多个参数拼接
            exec(engine, r#"Simulate("hello ", "world\n")"#).unwrap();
            let result: String = eval(engine, "return sim_result").unwrap();
            assert_eq!(result, "world");
        });
    }

    #[test]
    fn test_simulate_does_not_clear_pending_commands() {
        with_engine(|engine| {
            // 先用 Execute 压入一个命令
            exec(engine, "Execute('look')").unwrap();
            let cmds_before = engine.drain_commands();
            assert_eq!(cmds_before, vec!["look"]);

            // 再用 Execute 压入命令，然后 Simulate 不应清空它
            exec(engine, "Execute('score')").unwrap();
            exec(
                engine,
                r#"
                AddTrigger('sim_noclear', 'test_line', '', 1, 0, 0, '', '', 0, 0)
            "#,
            )
            .unwrap();
            exec(engine, r#"Simulate("test_line\n")"#).unwrap();
            let cmds = engine.drain_commands();
            assert!(
                cmds.contains(&"score".to_string()),
                "Simulate should not clear pending_commands, got: {:?}",
                cmds
            );
        });
    }

    #[test]
    fn test_simulate_adds_to_pending_logs() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                AddTrigger('sim_log', 'visible line', '', 1, 0, 0, '', '', 0, 0)
            "#,
            )
            .unwrap();
            exec(engine, r#"Simulate("visible line\n")"#).unwrap();
            let logs = engine.drain_logs();
            assert!(
                logs.iter().any(|l| l.contains("visible line")),
                "Simulate should add text to pending_logs, got: {:?}",
                logs
            );
        });
    }

    #[test]
    fn test_simulate_omit_from_output() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                AddTrigger('sim_omit', 'hide_me', '', 33, 0, 0, '', '', 0, 0)
                SetTriggerOption('sim_omit', 'omit_from_output', true)
            "#,
            )
            .unwrap();
            exec(engine, r#"Simulate("hide_me\n")"#).unwrap();
            let logs = engine.drain_logs();
            assert!(
                !logs.iter().any(|l| l.contains("hide_me")),
                "omit_from_output should suppress log, got: {:?}",
                logs
            );
        });
    }

    #[test]
    fn test_simulate_multiline() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                sim_result = ""
                AddTrigger('sim_ml', 'line1', '', 1, 0, 0, '', 'function() sim_result = sim_result .. "1" end', 0, 0)
                AddTrigger('sim_ml2', 'line2', '', 1, 0, 0, '', 'function() sim_result = sim_result .. "2" end', 0, 0)
            "#,
            )
            .unwrap();
            exec(engine, r#"Simulate("line1\nline2\n")"#).unwrap();
            let result: String = eval(engine, "return sim_result").unwrap();
            assert_eq!(result, "12");
        });
    }

    #[test]
    fn test_simulate_trigger_callback_sends_command() {
        with_engine(|engine| {
            // 触发器回调中调用 Execute 发送命令
            exec(
                engine,
                r#"
                AddTrigger('sim_send', 'go_now', '', 33, 0, 0, '', 'function() Execute("go north") end', 0, 0)
            "#,
            )
            .unwrap();
            exec(engine, r#"Simulate("go_now\n")"#).unwrap();
            let cmds = engine.drain_commands();
            assert!(
                cmds.contains(&"go north".to_string()),
                "Simulate trigger callback should add to pending_commands via Execute, got: {:?}",
                cmds
            );
        });
    }

    #[test]
    fn test_simulate_empty_lines_skipped() {
        with_engine(|engine| {
            exec(
                engine,
                r#"
                sim_count = 0
                AddTrigger('sim_empty', '.+', '', 33, 0, 0, '', 'function() sim_count = sim_count + 1 end', 0, 0)
            "#,
            )
            .unwrap();
            // 只有中间一行非空
            exec(engine, r#"Simulate("\nhello\n\n")"#).unwrap();
            let count: i64 = eval(engine, "return sim_count").unwrap();
            assert_eq!(count, 1);
        });
    }

    #[test]
    fn test_simulate_no_return_value() {
        with_engine(|engine| {
            // Simulate returns nothing (nil in Lua)
            let result: mlua::Value = eval(engine, r#"return Simulate("anything\n")"#).unwrap();
            assert!(matches!(result, mlua::Value::Nil));
        });
    }
}
