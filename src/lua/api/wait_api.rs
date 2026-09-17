//! wait.lua 依赖 注册
//!
//! 对应拆分前 `api.rs` 中的「wait.lua 依赖」分节。

use mlua::{Result as LuaResult, Value};

use crate::lua::helpers::regex_escape;
use crate::lua::types::LuaEngine;

impl LuaEngine {
    pub(super) fn register_wait_api(&mut self) -> LuaResult<()> {
        let lua = &self.lua;
        let globals = lua.globals();

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

        Ok(())
    }
}
