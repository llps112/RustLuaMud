//! 模块加载机制 注册
//!
//! 对应拆分前 `api.rs` 中的「模块加载机制」分节。

use mlua::{Function, Result as LuaResult, Table};

use crate::lua::helpers::{convert_pcre_to_rust_regex, fix_lua_escape_sequences};
use crate::lua::types::{LuaEngine, ScriptEncoding};

impl LuaEngine {
    pub(super) fn register_module_loader_api(&mut self) -> LuaResult<()> {
        let lua = &self.lua;
        let globals = lua.globals();
        let state_rc = self.state.clone();

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

        // 注册 MushClient 兼容全局模块（rex 正则库，基于 Rust regex crate 实现，
        // PCRE 语法子集：\Z/\z 自动转换，不支持反向引用与前后查找）
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

        Ok(())
    }
}
