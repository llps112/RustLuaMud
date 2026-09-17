//! 别名 API 注册
//!
//! 对应拆分前 `api.rs` 中的「别名 API」分节。

use mlua::{Function, Result as LuaResult, Value};
use regex::Regex;

use crate::lua::helpers::{
    coerce_to_i64, coerce_to_string, convert_pcre_to_rust_regex, i64_to_lua_integer,
    lua_integer_to_i64, regex_escape,
};
use crate::lua::types::{Alias, LuaEngine};

impl LuaEngine {
    pub(super) fn register_alias_api(&mut self) -> LuaResult<()> {
        let lua = &self.lua;
        let globals = lua.globals();
        let state_rc = self.state.clone();

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
            let is_regex = (flags & 128) != 0;
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
                one_shot: (flags & 32768) != 0,
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

        Ok(())
    }
}
