//! 别名匹配与执行
//!
//! 处理用户输入，匹配别名并调用回调。包含：
//! - `process_input`: 处理用户输入（含 panic 防御）
//! - `process_input_inner`: 实际匹配逻辑（send_to=12 脚本执行 / 函数回调）

use std::panic::AssertUnwindSafe;

use super::types::LuaEngine;

impl LuaEngine {
    /// 处理用户输入，匹配别名
    pub fn process_input(&self, input: &str) -> bool {
        // 使用 catch_unwind 防止 panic 跨越 FFI 边界
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| self.process_input_inner(input)));
        result.unwrap_or(false)
    }

    /// process_input 的内部实现
    fn process_input_inner(&self, input: &str) -> bool {
        self.state.borrow_mut().pending_commands.clear();

        let matches: Vec<(usize, Vec<String>, i64, String, bool)> = {
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
                    result.push((
                        i,
                        caps_list,
                        alias.send_to,
                        alias.response.clone(),
                        alias.one_shot,
                    ));
                }
            }
            result
        };

        if matches.is_empty() {
            return false;
        }

        // OneShot alias 匹配后自动删除（MushClient 兼容：alias_flag.OneShot = 32768）
        // 必须在 for 循环消费 matches 之前收集
        let oneshot_names: Vec<String> = {
            let state = self.state.borrow();
            matches
                .iter()
                .filter(|(_, _, _, _, one_shot)| *one_shot)
                .filter_map(|(idx, _, _, _, _)| state.aliases.get(*idx).map(|a| a.name.clone()))
                .collect()
        };

        for (idx, caps_list, send_to, response, _one_shot) in matches {
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
                let lua_result = std::panic::catch_unwind(AssertUnwindSafe(|| {
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
                    if std::panic::catch_unwind(AssertUnwindSafe(|| {
                        if let Err(e) =
                            callback.call::<()>((alias_name, input.to_string(), wildcards))
                        {
                            self.log_error(&format!(
                                "[Lua] 别名 '{}' 回调中发生 Lua 错误: {}",
                                name_for_err, e
                            ));
                        }
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

        // OneShot alias 匹配后自动删除
        if !oneshot_names.is_empty() {
            let mut state = self.state.borrow_mut();
            for name in &oneshot_names {
                state.delete_alias(name);
            }
        }

        true
    }
}
