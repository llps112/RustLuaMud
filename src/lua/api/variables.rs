//! JSON / 配置 / 连接状态 / 变量 / 数据库 API 注册
//!
//! 对应拆分前 `api.rs` 中的「JSON 序列化桥接」「配置 API」「连接状态 API」
//! 「变量 API」「数据库 API」五个分节。

use std::sync::{Arc, Mutex};

use mlua::{Result as LuaResult, Table, Value};
use rusqlite::Connection;

use crate::lua::database::LuaDb;
use crate::lua::helpers::{i64_to_lua_integer, json_to_lua_value, lua_value_to_json};
use crate::lua::types::LuaEngine;

impl LuaEngine {
    pub(super) fn register_json_api(&mut self) -> LuaResult<()> {
        let lua = &self.lua;
        let globals = lua.globals();

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
            let lua_val = json_to_lua_value(lua, &json_val)?;
            Ok(lua_val)
        })?;
        globals.set("json_decode", json_decode_fn)?;

        Ok(())
    }

    pub(super) fn register_config_api(&mut self) -> LuaResult<()> {
        let lua = &self.lua;
        let globals = lua.globals();
        let state_rc = self.state.clone();

        // ============================================================
        // 配置 API
        // ============================================================

        // GetInfo(code) — MushClient API 兼容
        let script_path_rc = self.script_path.clone();
        let log_dir_rc = self.log_dir.clone();
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
                // 返回配置的日志目录，供脚本写入日志文件
                let dir = log_dir_rc.borrow().clone();
                let sep = if cfg!(windows) { "\\" } else { "/" };
                let default_dir = format!("logs{}", sep);
                match dir {
                    Some(d) => Ok(Value::String(lua.create_string(&d)?)),
                    None => Ok(Value::String(lua.create_string(&default_dir)?)),
                }
            }
            204 => {
                // MushClient: GetInfo(204) = Packets received
                let count = state_rc_gi.borrow().packet_count;
                Ok(Value::Integer(i64_to_lua_integer(count as i64)))
            }
            88 => {
                // MushClient (GitHub only): GetInfo(88) = Window Title
                // 本引擎无窗口标题，返回 world name
                let name = state_rc_gi.borrow().world_name.clone();
                Ok(Value::String(lua.create_string(&name)?))
            }
            89 => {
                // MushClient (GitHub only): GetInfo(89) = Main Window Title
                Ok(Value::String(lua.create_string("RustLuaMud")?))
            }
            106 => {
                // MushClient: GetInfo(106) = Disconnected flag (1=disconnected)
                let disconnected = !state_rc_gi.borrow().connected;
                Ok(Value::Integer(i64_to_lua_integer(if disconnected {
                    1
                } else {
                    0
                })))
            }
            107 => {
                // MushClient: GetInfo(107) = Currently-connecting flag
                let reconnecting = state_rc_gi.borrow().reconnecting;
                Ok(Value::Integer(i64_to_lua_integer(if reconnecting {
                    1
                } else {
                    0
                })))
            }
            216 => {
                // MushClient: GetInfo(216) = Total bytes received
                let bytes = state_rc_gi.borrow().bytes_recv;
                Ok(Value::Integer(i64_to_lua_integer(bytes as i64)))
            }
            217 => {
                // MushClient: GetInfo(217) = Total bytes sent
                let bytes = state_rc_gi.borrow().bytes_sent;
                Ok(Value::Integer(i64_to_lua_integer(bytes as i64)))
            }
            227 => {
                // MushClient: GetInfo(227) = Connect phase
                // 0=disconnected, 1=connecting, 2=connected
                let state = state_rc_gi.borrow();
                let phase = if state.connected {
                    2
                } else if state.reconnecting {
                    1
                } else {
                    0
                };
                Ok(Value::Integer(i64_to_lua_integer(phase)))
            }
            301 => {
                // MushClient: GetInfo(301) = Time connected (seconds since connect)
                let state = state_rc_gi.borrow();
                let secs = state
                    .connect_time
                    .map(|t| t.elapsed().as_secs() as i64)
                    .unwrap_or(0);
                Ok(Value::Integer(i64_to_lua_integer(secs)))
            }
            _ => Ok(Value::String(lua.create_string("")?)),
        })?;
        globals.set("GetInfo", get_info_fn)?;

        // GetSessionStats() — RustLuaMud 扩展 API，返回连接统计信息
        let state_rc_ss = state_rc.clone();
        let get_session_stats_fn = lua.create_function_mut(move |lua, ()| {
            let state = state_rc_ss.borrow();
            let t = lua.create_table()?;
            // uptime: 连接持续时间（秒）
            let uptime = state
                .connect_time
                .map(|t| t.elapsed().as_secs() as f64)
                .unwrap_or(0.0);
            t.set("uptime", uptime)?;
            // last_recv_secs: 距上次收到数据的时间（秒）
            let last_recv = state.last_server_data.elapsed().as_secs() as f64;
            t.set("last_recv_secs", last_recv)?;
            t.set("reconnect_count", state.reconnect_count as f64)?;
            t.set("reconnect_attempt", state.reconnect_attempt as f64)?;
            t.set("next_retry_secs", state.next_retry_secs as f64)?;
            t.set("bytes_recv", state.bytes_recv as f64)?;
            t.set("bytes_sent", state.bytes_sent as f64)?;
            t.set("connected", state.connected)?;
            t.set("reconnecting", state.reconnecting)?;
            // last_disconnect_reason: string 或 nil
            match &state.last_disconnect_reason {
                Some(reason) => t.set("last_disconnect_reason", reason.as_str())?,
                None => t.set("last_disconnect_reason", Value::Nil)?,
            }
            Ok(Value::Table(t))
        })?;
        globals.set("GetSessionStats", get_session_stats_fn)?;

        // OnDisconnect(reason) — 默认空函数，Lua 脚本可覆盖
        let on_disconnect_fn = lua.create_function(move |_, _reason: String| Ok(()))?;
        globals.set("OnDisconnect", on_disconnect_fn)?;

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

        Ok(())
    }

    pub(super) fn register_connection_api(&mut self) -> LuaResult<()> {
        let lua = &self.lua;
        let globals = lua.globals();
        let state_rc = self.state.clone();

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

        Ok(())
    }

    pub(super) fn register_variables_api(&mut self) -> LuaResult<()> {
        let lua = &self.lua;
        let globals = lua.globals();
        let state_rc = self.state.clone();

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

        Ok(())
    }

    pub(super) fn register_database_api(&mut self) -> LuaResult<()> {
        let lua = &self.lua;
        let globals = lua.globals();

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

        Ok(())
    }
}
