//! 触发器 API 注册
//!
//! 对应拆分前 `api.rs` 中的「触发器 API」分节，末尾另附文件级自由函数
//! `add_trigger_impl`（触发器注册的通用实现）。

use mlua::{Function, Result as LuaResult, Value};
use regex::bytes::Regex as BytesRegex;
use regex::Regex;

use crate::lua::helpers::{
    coerce_to_i64, coerce_to_string, convert_pcre_to_rust_regex, i64_to_lua_integer, regex_escape,
    utf8_regex_to_gbk_bytes,
};
use crate::lua::types::{LuaEngine, ScriptEncoding, ScriptState, Trigger, TriggerPattern};

impl LuaEngine {
    pub(super) fn register_trigger_api(&mut self) -> LuaResult<()> {
        let lua = &self.lua;
        let globals = lua.globals();
        let state_rc = self.state.clone();

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
                        36 => Ok(Value::Boolean(t.one_shot)), // 'one shot' flag
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
