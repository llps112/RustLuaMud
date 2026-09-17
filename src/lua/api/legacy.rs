//! 原始 API（保留兼容）注册
//!
//! 对应拆分前 `api.rs` 中的「原始 API（保留兼容）」分节（trigger / alias /
//! timer / get / set）。

use mlua::{Function, Result as LuaResult};
use regex::bytes::Regex as BytesRegex;
use regex::Regex;

use crate::lua::helpers::{convert_pcre_to_rust_regex, utf8_regex_to_gbk_bytes};
use crate::lua::types::{Alias, LuaEngine, ScriptEncoding, TimerDef, Trigger, TriggerPattern};

impl LuaEngine {
    pub(super) fn register_legacy_api(&mut self) -> LuaResult<()> {
        let lua = &self.lua;
        let globals = lua.globals();
        let state_rc = self.state.clone();

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
                    one_shot: false,
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
                    one_shot: false,
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
