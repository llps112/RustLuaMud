//! Lua 引擎的辅助函数
//!
//! 包含整数转换、Lua 源码预处理、类型强转、颜色映射、JSON 互转等辅助函数。

/// mlua::Integer 在 64 位平台是 i64，在 32 位平台（如 i686）是 i32。
/// 内部逻辑统一使用 i64，在与 mlua 交互的边界点做转换。
///
/// # 32 位平台截断行为
///
/// 在 32 位平台上，`i64_to_lua_integer` 会截断高位（`i64 as i32`）。
/// 这是预期行为：MUD 脚本中的数值（经验值、HP、标志位等）不会超过 2^31，
/// 因此截断不会影响实际功能。
#[inline]
pub(super) fn i64_to_lua_integer(v: i64) -> mlua::Integer {
    v as mlua::Integer
}

/// 将 mlua::Integer 转换为 i64。
///
/// 在 32 位平台是安全的扩展转换（i32→i64），在 64 位平台是同类型转换（i64→i64），
/// 两种情况都不会丢失数据。
#[inline]
#[allow(clippy::unnecessary_cast)]
pub(super) fn lua_integer_to_i64(v: mlua::Integer) -> i64 {
    v as i64
}

/// Lua 合法转义字符集合
const LUA_VALID_ESCAPES: &[u8] = b"abfnrtv\\\"'0123456789xzZuU";

/// 预处理 Lua 源码，修复 LuaJIT 不兼容的无效转义序列
///
/// 标准 Lua 5.1 对未识别的转义序列（如 `\-`, `\+`）宽松处理（保留反斜杠），
/// 但 LuaJIT 严格拒绝。此函数在字符串字面量内将无效转义 `\X` 替换为 `\\X`，
/// 使 LuaJIT 正确解析。
pub(super) fn fix_lua_escape_sequences(source: &str) -> String {
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
pub(super) fn count_long_bracket_open(bytes: &[u8]) -> usize {
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
pub(super) fn count_long_bracket_close(bytes: &[u8]) -> usize {
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
pub(super) fn coerce_to_i64(value: mlua::Value) -> mlua::Result<i64> {
    match value {
        mlua::Value::Integer(i) => Ok(lua_integer_to_i64(i)),
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
pub(super) fn coerce_to_string(value: mlua::Value) -> mlua::Result<String> {
    match value {
        mlua::Value::String(s) => Ok(s.to_str()?.to_string()),
        mlua::Value::Integer(i) => Ok(i.to_string()),
        mlua::Value::Number(n) => Ok(n.to_string()),
        _ => Err(mlua::Error::external("期望字符串或数字")),
    }
}

/// 将 Lua Value 强制转换为 f64（兼容整数、浮点数和字符串，nil 返回 Err）
pub(super) fn coerce_to_f64(value: mlua::Value) -> mlua::Result<f64> {
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
pub(super) fn colour_to_ansi_fg(name: &str) -> u8 {
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
pub(super) fn colour_to_ansi_bg(name: &str) -> u8 {
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

// ============================================================
// JSON 互转辅助函数（供 json_encode / json_decode 使用）
// ============================================================

/// 将 mlua::Value 转为 serde_json::Value（用于序列化）
pub(super) fn lua_value_to_json(val: &mlua::Value) -> serde_json::Value {
    match val {
        mlua::Value::Nil => serde_json::Value::Null,
        mlua::Value::Boolean(b) => serde_json::Value::Bool(*b),
        mlua::Value::Integer(i) => serde_json::Value::Number(lua_integer_to_i64(*i).into()),
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
            let mut i: mlua::Integer = 1;
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
                for (_, v) in t.clone().pairs::<mlua::Integer, mlua::Value>().flatten() {
                    arr.push(lua_value_to_json(&v));
                }
                serde_json::Value::Array(arr)
            } else {
                // 对象
                let mut map = serde_json::Map::new();
                for (k, v) in t.clone().pairs::<mlua::Value, mlua::Value>().flatten() {
                    let key = match &k {
                        mlua::Value::String(s) => {
                            let owned: Vec<u8> = s.as_bytes().to_vec();
                            String::from_utf8_lossy(&owned).to_string()
                        }
                        _ => format!("{:?}", k),
                    };
                    map.insert(key, lua_value_to_json(&v));
                }
                serde_json::Value::Object(map)
            }
        }
        _ => serde_json::Value::Null,
    }
}

/// 将 serde_json::Value 转为 mlua::Value（用于反序列化）
pub(super) fn json_to_lua_value(
    lua: &mlua::Lua,
    val: &serde_json::Value,
) -> mlua::Result<mlua::Value> {
    match val {
        serde_json::Value::Null => Ok(mlua::Value::Nil),
        serde_json::Value::Bool(b) => Ok(mlua::Value::Boolean(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(mlua::Value::Integer(i64_to_lua_integer(i)))
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

// ============================================================
// 正则与触发器辅助函数
// ============================================================

/// 将 PCRE 正则模式转换为 Rust regex 兼容语法
///
/// MushClient 使用 PCRE 引擎，与 Rust regex crate 存在语法差异。
/// 此函数处理常见的兼容性问题：
/// - `\Z` (PCRE: 字符串末尾或末尾换行前) → `$`
/// - `\z` (PCRE: 字符串绝对末尾) → `$`
pub(super) fn convert_pcre_to_rust_regex(pattern: &str) -> String {
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
pub(super) fn utf8_regex_to_gbk_bytes(pattern: &str) -> String {
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

/// 转义正则特殊字符（保留 * 和 ? 用于通配符转换）
pub(super) fn regex_escape(s: &str) -> String {
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
