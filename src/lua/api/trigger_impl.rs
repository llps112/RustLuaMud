//! 触发器注册的通用实现
//!
//! 从拆分前 `api.rs` 的「触发器 API」分节拆出的实现细节，由 `register_trigger_api`
//! 注册的 `AddTrigger` / `AddTriggerEx` 两个入口共用。

use mlua::{Function, Result as LuaResult, Value};
use regex::bytes::Regex as BytesRegex;
use regex::Regex;

use crate::lua::helpers::{convert_pcre_to_rust_regex, regex_escape, utf8_regex_to_gbk_bytes};
use crate::lua::types::{ScriptEncoding, ScriptState, Trigger, TriggerPattern};

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
        temporary: (flags & 16384) != 0,
        one_shot: (flags & 32768) != 0,
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
