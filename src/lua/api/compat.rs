//! Lua 兼容性补丁 注册
//!
//! 对应拆分前 `api.rs` 中的「Lua 兼容性补丁」分节。

use mlua::{Function, Result as LuaResult, Table, Value};

use crate::lua::types::LuaEngine;

impl LuaEngine {
    pub(super) fn register_compat_api(&mut self) -> LuaResult<()> {
        let lua = &self.lua;
        let globals = lua.globals();

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

        Ok(())
    }
}
