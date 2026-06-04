use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use mlua::{Lua, Result as LuaResult, Function, UserData, Table, Value};
use regex::Regex;
use rusqlite::{Connection, types::Value as SqlValue};

/// SQLite 连接包装（Lua 用户数据）
struct LuaDb {
    conn: Arc<Mutex<Connection>>,
}

impl UserData for LuaDb {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("close", |_, _this, ()| {
            Ok(())
        });

        methods.add_method("exec", |_, this, sql: String| {
            let conn = this.conn.lock().unwrap();
            conn.execute_batch(&sql)
                .map_err(|e| mlua::Error::external(e.to_string()))
        });

        methods.add_method("prepare", |lua, this, sql: String| {
            let conn = this.conn.lock().unwrap();
            conn.prepare(&sql)
                .map_err(|e| mlua::Error::external(e.to_string()))?;
            let lua_stmt = LuaStmt {
                conn: this.conn.clone(),
                sql: sql.clone(),
            };
            let ud = lua.create_userdata(lua_stmt)?;
            Ok(ud)
        });

        methods.add_method("changes", |_, this, ()| {
            let conn = this.conn.lock().unwrap();
            Ok(conn.changes() as i64)
        });

        methods.add_method("last_insert_rowid", |_, this, ()| {
            let conn = this.conn.lock().unwrap();
            Ok(conn.last_insert_rowid())
        });
    }
}

/// SQLite 预处理语句包装
struct LuaStmt {
    conn: Arc<Mutex<Connection>>,
    sql: String,
}

impl UserData for LuaStmt {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("step", |lua, this, args: Option<Table>| {
            let conn = this.conn.lock().unwrap();
            let mut stmt = conn.prepare(&this.sql)
                .map_err(|e| mlua::Error::external(e.to_string()))?;

            let params_vec = if let Some(ref t) = args {
                let len = t.len().unwrap_or(0) as usize;
                let mut vals: Vec<SqlValue> = Vec::with_capacity(len);
                for i in 1..=len {
                    let v: String = t.get(i).unwrap_or_default();
                    vals.push(SqlValue::Text(v));
                }
                vals
            } else {
                Vec::new()
            };

            let params_refs: Vec<&dyn rusqlite::types::ToSql> =
                params_vec.iter().map(|v| v as &dyn rusqlite::types::ToSql).collect();

            let mut rows = stmt.query(params_refs.as_slice())
                .map_err(|e| mlua::Error::external(e.to_string()))?;

            if let Some(row) = rows.next().map_err(|e| mlua::Error::external(e.to_string()))? {
                let lua_table = lua.create_table()?;
                let col_count = row.as_ref().column_count();
                for i in 0..col_count {
                    let val = match row.get_ref(i) {
                        Ok(r) => match r {
                            rusqlite::types::ValueRef::Null => mlua::Value::Nil,
                            rusqlite::types::ValueRef::Integer(n) => mlua::Value::Integer(n),
                            rusqlite::types::ValueRef::Real(f) => mlua::Value::Number(f),
                            rusqlite::types::ValueRef::Text(s) => {
                                mlua::Value::String(lua.create_string(s)?)
                            }
                            rusqlite::types::ValueRef::Blob(b) => {
                                mlua::Value::String(lua.create_string(b)?)
                            }
                        },
                        Err(_) => mlua::Value::Nil,
                    };
                    lua_table.set(i + 1, val)?;
                }
                return Ok(Some(lua_table));
            }

            Ok(None)
        });

        methods.add_method("run", |_, this, args: Option<Table>| {
            let conn = this.conn.lock().unwrap();
            let mut stmt = conn.prepare(&this.sql)
                .map_err(|e| mlua::Error::external(e.to_string()))?;

            let params_vec = if let Some(ref t) = args {
                let len = t.len().unwrap_or(0) as usize;
                let mut vals: Vec<SqlValue> = Vec::with_capacity(len);
                for i in 1..=len {
                    let v: String = t.get(i).unwrap_or_default();
                    vals.push(SqlValue::Text(v));
                }
                vals
            } else {
                Vec::new()
            };

            let params_refs: Vec<&dyn rusqlite::types::ToSql> =
                params_vec.iter().map(|v| v as &dyn rusqlite::types::ToSql).collect();

            stmt.execute(params_refs.as_slice())
                .map_err(|e| mlua::Error::external(e.to_string()))?;

            Ok(())
        });
    }
}

/// 触发器定义
pub struct Trigger {
    pub name: String,
    pub pattern: Regex,
    pub callback: Function,
    pub enabled: bool,
    pub group: String,
    pub sequence: i32,
    pub multiline: bool,
    pub lines_to_match: usize,
    pub omit_from_output: bool,
    pub temporary: bool,
    pub send_text: String,
}

/// 别名定义
pub struct Alias {
    pub name: String,
    pub pattern: Regex,
    pub callback: Function,
    pub enabled: bool,
    pub group: String,
}

/// 定时器定义
pub struct TimerDef {
    pub name: String,
    pub interval_secs: u64,
    pub callback: Function,
    pub enabled: bool,
    pub group: String,
    pub one_shot: bool,
    pub send_text: String,
}

/// 脚本运行时共享状态
struct ScriptState {
    triggers: Vec<Trigger>,
    aliases: Vec<Alias>,
    timers: Vec<TimerDef>,
    variables: HashMap<String, String>,
    pending_commands: Vec<String>,
    pending_logs: Vec<String>,
    recent_lines: Vec<String>,
    unique_counter: u64,
    connected: bool,
    connect_requested: bool,
    disconnect_requested: bool,
}

/// Lua 引擎与脚本运行时
pub struct LuaEngine {
    lua: Lua,
    state: Rc<RefCell<ScriptState>>,
    script_path: Option<String>,
    script_dir: Option<String>,
}

impl LuaEngine {
    /// 创建新的 Lua 引擎实例
    pub fn new() -> LuaResult<Self> {
        let lua = Lua::new();

        let state = Rc::new(RefCell::new(ScriptState {
            triggers: Vec::new(),
            aliases: Vec::new(),
            timers: Vec::new(),
            variables: HashMap::new(),
            pending_commands: Vec::new(),
            pending_logs: Vec::new(),
            recent_lines: Vec::new(),
            unique_counter: 0,
            connected: false,
            connect_requested: false,
            disconnect_requested: false,
        }));

        let mut engine = Self { lua, state, script_path: None, script_dir: None };
        engine.register_api()?;
        Ok(engine)
    }

    /// 设置脚本路径（同时提取目录）
    pub fn set_script_path(&mut self, path: &str) {
        if let Some(pos) = path.rfind('/') {
            self.script_dir = Some(path[..pos + 1].to_string());
        } else {
            self.script_dir = Some("./".to_string());
        }
        self.script_path = Some(path.to_string());
    }

    /// 注册 Lua API
    fn register_api(&mut self) -> LuaResult<()> {
        let lua = &self.lua;
        let globals = lua.globals();

        let state_rc = self.state.clone();

        // ============================================================
        // 命令执行
        // ============================================================

        // send(command)
        let state_rc2 = state_rc.clone();
        let send_fn = lua.create_function_mut(move |_, cmd: String| {
            state_rc2.borrow_mut().pending_commands.push(cmd);
            Ok(())
        })?;
        globals.set("send", send_fn)?;

        // Execute(command) — MushClient API
        let state_rc3 = state_rc.clone();
        let execute_fn = lua.create_function_mut(move |_, cmd: String| {
            state_rc3.borrow_mut().pending_commands.push(cmd);
            Ok(0)
        })?;
        globals.set("Execute", execute_fn)?;

        // ============================================================
        // 输出
        // ============================================================

        // log(message)
        let state_rc4 = state_rc.clone();
        let log_fn = lua.create_function_mut(move |_, msg: String| {
            state_rc4.borrow_mut().pending_logs.push(msg);
            Ok(())
        })?;
        globals.set("log", log_fn)?;

        // ColourNote(fg, bg, text)
        let state_rc5 = state_rc.clone();
        let colour_note_fn = lua.create_function_mut(move |_, (fg, bg, text): (String, String, String)| {
            let msg = format!("[{}:{}]: {}", fg, bg, text);
            state_rc5.borrow_mut().pending_logs.push(msg);
            Ok(())
        })?;
        globals.set("ColourNote", colour_note_fn)?;

        // Note(text)
        let state_rc6 = state_rc.clone();
        let note_fn = lua.create_function_mut(move |_, text: String| {
            state_rc6.borrow_mut().pending_logs.push(text);
            Ok(())
        })?;
        globals.set("Note", note_fn)?;

        // Tell(text)
        let state_rc7 = state_rc.clone();
        let tell_fn = lua.create_function_mut(move |_, text: String| {
            state_rc7.borrow_mut().pending_logs.push(text);
            Ok(())
        })?;
        globals.set("Tell", tell_fn)?;

        // ============================================================
        // 触发器 API
        // ============================================================

        // AddTrigger(name, match_str, response, flags, colour, wildcard, sound, script, send_to, sequence)
        let state_rc8 = state_rc.clone();
        let add_trigger_fn = lua.create_function_mut(move |lua, (name, match_str, _response, flags, _colour, _wildcard, _sound, script, _send_to, sequence): (String, String, String, i64, i64, i64, String, String, i64, i64)| {
            add_trigger_impl(lua, &state_rc8, &name, &match_str, flags, &script, _send_to, sequence as i32)
        })?;
        globals.set("AddTrigger", add_trigger_fn)?;

        // AddTriggerEx(name, match_str, response, flags, colour, wildcard, sound, script, send_to, sequence)
        let state_rc9 = state_rc.clone();
        let add_trigger_ex_fn = lua.create_function_mut(move |lua, (name, match_str, _response, flags, _colour, _wildcard, _sound, script, _send_to, sequence): (String, String, String, i64, i64, i64, String, String, i64, i64)| {
            add_trigger_impl(lua, &state_rc9, &name, &match_str, flags, &script, _send_to, sequence as i32)
        })?;
        globals.set("AddTriggerEx", add_trigger_ex_fn)?;

        // DeleteTrigger(name)
        let state_rc10 = state_rc.clone();
        let delete_trigger_fn = lua.create_function_mut(move |_, name: String| {
            let mut state = state_rc10.borrow_mut();
            let before = state.triggers.len();
            state.triggers.retain(|t| t.name != name);
            if state.triggers.len() < before { Ok(0) } else { Ok(1) }
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

        // GetTriggerInfo(name, code)
        let state_rc12 = state_rc.clone();
        let get_trigger_info_fn = lua.create_function_mut(move |lua, (name, code): (String, i64)| {
            let state = state_rc12.borrow();
            if let Some(t) = state.triggers.iter().find(|t| t.name == name) {
                match code {
                    8 => Ok(Value::Boolean(t.enabled)), // enabled
                    26 => {
                        let group = t.group.clone();
                        Ok(Value::String(lua.create_string(&group)?))
                    }
                    _ => Ok(Value::Nil),
                }
            } else {
                Ok(Value::Nil)
            }
        })?;
        globals.set("GetTriggerInfo", get_trigger_info_fn)?;

        // SetTriggerOption(name, key, value)
        let state_rc13 = state_rc.clone();
        let set_trigger_option_fn = lua.create_function_mut(move |lua, (name, key, value): (String, String, Value)| {
            let mut state = state_rc13.borrow_mut();
            if let Some(t) = state.triggers.iter_mut().find(|t| t.name == name) {
                match key.as_str() {
                    "group" => {
                        if let Value::String(s) = value {
                            t.group = s.to_str().map(|s| s.to_string()).unwrap_or_default();
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
        let enable_trigger_group_fn = lua.create_function_mut(move |_, (group, enable): (String, bool)| {
            let mut state = state_rc14.borrow_mut();
            for t in state.triggers.iter_mut() {
                if !t.group.is_empty() && t.group == group {
                    t.enabled = enable;
                }
            }
            Ok(())
        })?;
        globals.set("EnableTriggerGroup", enable_trigger_group_fn)?;

        // EnableTrigger(name, enable)
        let state_rc_et = state_rc.clone();
        let enable_trigger_fn = lua.create_function_mut(move |_, (name, enable): (String, bool)| {
            let mut state = state_rc_et.borrow_mut();
            if let Some(t) = state.triggers.iter_mut().find(|t| t.name == name) {
                t.enabled = enable;
                Ok(Value::Integer(0))
            } else {
                Ok(Value::Integer(1))
            }
        })?;
        globals.set("EnableTrigger", enable_trigger_fn)?;

        // ============================================================
        // 别名 API
        // ============================================================

        // AddAlias(name, match_str, response, flags, colour, script, send_to)
        let state_rc15 = state_rc.clone();
        let add_alias_fn = lua.create_function_mut(move |lua, (name, match_str, _response, flags, _colour, script, _send_to): (String, String, String, i64, i64, String, i64)| {
            let re_str = if (flags & 32) != 0 {
                match_str.clone()
            } else {
                regex_escape(&match_str).replace('*', "(.*)").replace('?', "(.)")
            };
            let re = Regex::new(&re_str)
                .map_err(|e| mlua::Error::external(format!("无效正则 '{}': {}", re_str, e)))?;

            let callback: Function = if script.is_empty() {
                lua.create_function(|_, _: ()| Ok(()))?
            } else {
                let code = format!("return {}", script);
                match lua.load(&code).eval::<Function>() {
                    Ok(f) => f,
                    Err(_) => lua.load(script).eval()?,
                }
            };

            state_rc15.borrow_mut().aliases.push(Alias {
                name,
                pattern: re,
                callback,
                enabled: (flags & 1) != 0,
                group: String::new(),
            });
            Ok(Value::Integer(0))
        })?;
        globals.set("AddAlias", add_alias_fn)?;

        // DeleteAlias(name)
        let state_rc16 = state_rc.clone();
        let delete_alias_fn = lua.create_function_mut(move |_, name: String| {
            let mut state = state_rc16.borrow_mut();
            let before = state.aliases.len();
            state.aliases.retain(|a| a.name != name);
            if state.aliases.len() < before { Ok(0) } else { Ok(1) }
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

        // SetAliasOption(name, key, value)
        let state_rc18 = state_rc.clone();
        let set_alias_option_fn = lua.create_function_mut(move |_, (name, key, value): (String, String, Value)| {
            let mut state = state_rc18.borrow_mut();
            if let Some(a) = state.aliases.iter_mut().find(|a| a.name == name) {
                match key.as_str() {
                    "group" => {
                        if let Value::String(s) = value {
                            a.group = s.to_str().map(|s| s.to_string()).unwrap_or_default();
                        }
                    }
                    "enabled" => {
                        if let Value::Boolean(b) = value {
                            a.enabled = b;
                        } else if let Value::Integer(n) = value {
                            a.enabled = n != 0;
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

        // ============================================================
        // 定时器 API
        // ============================================================

        // AddTimer(name, hour, min, sec, callback, flags, colour, script, send_to)
        let state_rc19 = state_rc.clone();
        let add_timer_fn = lua.create_function_mut(move |lua, (name, _hour, _min, sec, _callback, flags, _colour, script, _send_to): (String, i64, i64, i64, i64, i64, i64, String, i64)| {
            let interval = if sec <= 0 { 1 } else { sec as u64 };
            let one_shot = (flags & 4) != 0;

            // 将脚本作为 send_text 存储，在 fire_timer 时执行
            let callback: Function = lua.create_function(|_, _: ()| Ok(()))?;

            state_rc19.borrow_mut().timers.push(TimerDef {
                name,
                interval_secs: interval,
                callback,
                enabled: (flags & 1) != 0,
                group: String::new(),
                one_shot,
                send_text: script,
            });
            Ok(Value::Integer(0))
        })?;
        globals.set("AddTimer", add_timer_fn)?;

        // DeleteTimer(name)
        let state_rc20 = state_rc.clone();
        let delete_timer_fn = lua.create_function_mut(move |_, name: String| {
            let mut state = state_rc20.borrow_mut();
            let before = state.timers.len();
            state.timers.retain(|t| t.name != name);
            if state.timers.len() < before { Ok(0) } else { Ok(1) }
        })?;
        globals.set("DeleteTimer", delete_timer_fn)?;

        // GetTimerList()
        let state_rc21 = state_rc.clone();
        let get_timer_list_fn = lua.create_function_mut(move |lua, ()| {
            let state = state_rc21.borrow();
            let list = lua.create_table()?;
            for (i, t) in state.timers.iter().enumerate() {
                list.set(i + 1, t.name.as_str())?;
            }
            Ok(Value::Table(list))
        })?;
        globals.set("GetTimerList", get_timer_list_fn)?;

        // GetTimerInfo(name, code)
        let state_rc22 = state_rc.clone();
        let get_timer_info_fn = lua.create_function_mut(move |lua, (name, code): (String, i64)| {
            let state = state_rc22.borrow();
            if let Some(t) = state.timers.iter().find(|t| t.name == name) {
                match code {
                    6 => Ok(Value::Boolean(t.enabled)), // enabled
                    19 => {
                        let group = t.group.clone();
                        Ok(Value::String(lua.create_string(&group)?))
                    }
                    _ => Ok(Value::Nil),
                }
            } else {
                Ok(Value::Nil)
            }
        })?;
        globals.set("GetTimerInfo", get_timer_info_fn)?;

        // SetTimerOption(name, key, value)
        let state_rc23 = state_rc.clone();
        let set_timer_option_fn = lua.create_function_mut(move |_, (name, key, value): (String, String, Value)| {
            let mut state = state_rc23.borrow_mut();
            if let Some(t) = state.timers.iter_mut().find(|t| t.name == name) {
                match key.as_str() {
                    "group" => {
                        if let Value::String(s) = value {
                            t.group = s.to_str().map(|s| s.to_string()).unwrap_or_default();
                        }
                    }
                    "enabled" => {
                        if let Value::Boolean(b) = value {
                            t.enabled = b;
                        } else if let Value::Integer(n) = value {
                            t.enabled = n != 0;
                        }
                    }
                    "send_to" => {}
                    _ => {}
                }
                Ok(Value::Integer(0))
            } else {
                Ok(Value::Integer(1))
            }
        })?;
        globals.set("SetTimerOption", set_timer_option_fn)?;

        // EnableTimerGroup(group_name, enable)
        let state_rc24 = state_rc.clone();
        let enable_timer_group_fn = lua.create_function_mut(move |_, (group, enable): (String, bool)| {
            let mut state = state_rc24.borrow_mut();
            for t in state.timers.iter_mut() {
                if !t.group.is_empty() && t.group == group {
                    t.enabled = enable;
                }
            }
            Ok(())
        })?;
        globals.set("EnableTimerGroup", enable_timer_group_fn)?;

        // EnableTimer(name, enable)
        let state_rc_emt = state_rc.clone();
        let enable_timer_fn = lua.create_function_mut(move |_, (name, enable): (String, bool)| {
            let mut state = state_rc_emt.borrow_mut();
            if let Some(t) = state.timers.iter_mut().find(|t| t.name == name) {
                t.enabled = enable;
                Ok(Value::Integer(0))
            } else {
                Ok(Value::Integer(1))
            }
        })?;
        globals.set("EnableTimer", enable_timer_fn)?;

        // ============================================================
        // 配置 API
        // ============================================================

        // GetInfo(code)
        let script_dir_rc = Rc::new(RefCell::new(self.script_dir.clone()));
        let get_info_fn = lua.create_function_mut(move |lua, code: i64| {
            match code {
                35 => {
                    let dir = script_dir_rc.borrow().clone();
                    match dir {
                        Some(d) => {
                            let win_path = d.replace('/', "\\");
                            Ok(Value::String(lua.create_string(&win_path)?))
                        }
                        None => Ok(Value::String(lua.create_string(".\\")?)),
                    }
                }
                1 => Ok(Value::String(lua.create_string("RustLuaMud 1.0")?)),
                _ => Ok(Value::String(lua.create_string("")?)),
            }
        })?;
        globals.set("GetInfo", get_info_fn)?;

        // SetOption(name, value)
        let set_option_fn = lua.create_function(move |lua, (name, value): (String, Value)| {
            let options: Table = lua.globals().get("_mud_options")?;
            options.set(name, value)?;
            Ok(())
        })?;
        globals.set("_mud_options", lua.create_table()?)?;
        globals.set("SetOption", set_option_fn)?;

        // GetOption(name)
        let get_option_fn = lua.create_function(move |lua, name: String| {
            let options: Table = lua.globals().get("_mud_options")?;
            let val: Value = options.get(name.as_str())?;
            Ok(val)
        })?;
        globals.set("GetOption", get_option_fn)?;

        // SetAlphaOption(name, value)
        let set_alpha_option_fn = lua.create_function(move |lua, (name, value): (String, Value)| {
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

        // ============================================================
        // 连接状态 API
        // ============================================================

        // IsConnected()
        let state_rc25 = state_rc.clone();
        let is_connected_fn = lua.create_function_mut(move |_, ()| {
            Ok(Value::Boolean(state_rc25.borrow().connected))
        })?;
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

        // ============================================================
        // 工具函数
        // ============================================================

        // GetUniqueNumber()
        let state_rc28 = state_rc.clone();
        let get_unique_number_fn = lua.create_function_mut(move |_, ()| {
            let mut state = state_rc28.borrow_mut();
            state.unique_counter += 1;
            Ok(Value::Integer(state.unique_counter as i64))
        })?;
        globals.set("GetUniqueNumber", get_unique_number_fn)?;

        // Trim(string)
        let trim_fn = lua.create_function(move |_, s: String| {
            Ok(s.trim().to_string())
        })?;
        globals.set("Trim", trim_fn)?;

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
        let set_variable_fn = lua.create_function_mut(move |_, (name, value): (String, String)| {
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

        // ============================================================
        // 日志 API
        // ============================================================

        // OpenLog(filename, append)
        let open_log_fn = lua.create_function(move |_, (_filename, _append): (String, bool)| {
            Ok(())
        })?;
        globals.set("OpenLog", open_log_fn)?;

        // IsLogOpen()
        let is_log_open_fn = lua.create_function(move |_, ()| {
            Ok(Value::Boolean(true))
        })?;
        globals.set("IsLogOpen", is_log_open_fn)?;

        // ============================================================
        // 数据库 API
        // ============================================================

        // DatabaseClose(dbname)
        let database_close_fn = lua.create_function(move |_, _dbname: String| {
            Ok(())
        })?;
        globals.set("DatabaseClose", database_close_fn)?;

        // sqlite3 module
        let sqlite3_mod = lua.create_table()?;
        let open_fn = lua.create_function(|lua, path: String| {
            let conn = Connection::open(&path)
                .map_err(|e| mlua::Error::external(e.to_string()))?;
            let db = LuaDb {
                conn: Arc::new(Mutex::new(conn)),
            };
            lua.create_userdata(db)
        })?;
        sqlite3_mod.set("open", open_fn)?;
        globals.set("sqlite3", sqlite3_mod)?;

        // ============================================================
        // 常量表
        // ============================================================

        // trigger_flag
        let trigger_flag = lua.create_table()?;
        trigger_flag.set("Enabled", 1i64)?;
        trigger_flag.set("OmitFromLog", 2i64)?;
        trigger_flag.set("OmitFromOutput", 4i64)?;
        trigger_flag.set("KeepEvaluating", 8i64)?;
        trigger_flag.set("IgnoreCase", 16i64)?;
        trigger_flag.set("RegularExpression", 32i64)?;
        trigger_flag.set("ExpandVariables", 64i64)?;
        trigger_flag.set("Replace", 1024i64)?;
        trigger_flag.set("LowercaseWildcard", 2048i64)?;
        trigger_flag.set("Temporary", 4096i64)?;
        trigger_flag.set("OneShot", 8192i64)?;
        globals.set("trigger_flag", trigger_flag)?;

        // alias_flag
        let alias_flag = lua.create_table()?;
        alias_flag.set("Enabled", 1i64)?;
        alias_flag.set("IgnoreCase", 16i64)?;
        alias_flag.set("RegularExpression", 32i64)?;
        alias_flag.set("ExpandVariables", 64i64)?;
        alias_flag.set("Replace", 1024i64)?;
        alias_flag.set("Temporary", 4096i64)?;
        globals.set("alias_flag", alias_flag)?;

        // timer_flag
        let timer_flag = lua.create_table()?;
        timer_flag.set("Enabled", 1i64)?;
        timer_flag.set("Temporary", 4096i64)?;
        timer_flag.set("OneShot", 8192i64)?;
        timer_flag.set("ActiveWhenClosed", 16384i64)?;
        timer_flag.set("Replace", 1024i64)?;
        globals.set("timer_flag", timer_flag)?;

        // custom_colour
        let custom_colour = lua.create_table()?;
        custom_colour.set("Black", 0i64)?;
        custom_colour.set("Maroon", 1i64)?;
        custom_colour.set("Green", 2i64)?;
        custom_colour.set("Olive", 3i64)?;
        custom_colour.set("Navy", 4i64)?;
        custom_colour.set("Purple", 5i64)?;
        custom_colour.set("Teal", 6i64)?;
        custom_colour.set("Silver", 7i64)?;
        custom_colour.set("Grey", 8i64)?;
        custom_colour.set("Red", 9i64)?;
        custom_colour.set("Lime", 10i64)?;
        custom_colour.set("Yellow", 11i64)?;
        custom_colour.set("Blue", 12i64)?;
        custom_colour.set("Fuchsia", 13i64)?;
        custom_colour.set("Aqua", 14i64)?;
        custom_colour.set("White", 15i64)?;
        globals.set("custom_colour", custom_colour)?;

        // error_code
        let error_code = lua.create_table()?;
        error_code.set("eOK", 0i64)?;
        error_code.set("eUnknownObject", 1i64)?;
        error_code.set("eItemAlreadyExists", 2i64)?;
        error_code.set("eBadRegularExpression", 3i64)?;
        error_code.set("eWildcardNotFound", 4i64)?;
        error_code.set("eCommandCancelled", 5i64)?;
        error_code.set("eNoSuchCommand", 6i64)?;
        error_code.set("eInvalidObjectLabel", 7i64)?;
        error_code.set("eAmbiguousObjectName", 8i64)?;
        globals.set("error_code", error_code)?;

        // error_desc
        let error_desc = lua.create_table()?;
        error_desc.set("eOK", "OK")?;
        error_desc.set("eUnknownObject", "Unknown object")?;
        error_desc.set("eItemAlreadyExists", "Item already exists")?;
        error_desc.set("eBadRegularExpression", "Bad regular expression")?;
        error_desc.set("eWildcardNotFound", "Wildcard not found")?;
        error_desc.set("eCommandCancelled", "Command cancelled")?;
        error_desc.set("eNoSuchCommand", "No such command")?;
        error_desc.set("eInvalidObjectLabel", "Invalid object label")?;
        error_desc.set("eAmbiguousObjectName", "Ambiguous object name")?;
        globals.set("error_desc", error_desc)?;

        // ============================================================
        // wait.lua 依赖
        // ============================================================

        // bit 库
        let bit_mod = lua.create_table()?;
        bit_mod.set("bor", lua.create_function(|_, (a, b): (i64, i64)| Ok(a | b))?)?;
        bit_mod.set("band", lua.create_function(|_, (a, b): (i64, i64)| Ok(a & b))?)?;
        bit_mod.set("bxor", lua.create_function(|_, (a, b): (i64, i64)| Ok(a ^ b))?)?;
        bit_mod.set("bnot", lua.create_function(|_, a: i64| Ok(!a))?)?;
        bit_mod.set("lshift", lua.create_function(|_, (a, n): (i64, i64)| Ok(a << n))?)?;
        bit_mod.set("rshift", lua.create_function(|_, (a, n): (i64, i64)| Ok(a >> n))?)?;
        globals.set("bit", bit_mod)?;

        // MakeRegularExpression(pattern) — 将通配符转为正则
        let make_re_fn = lua.create_function(move |lua, pattern: String| {
            let re = regex_escape(&pattern)
                .replace('*', ".*")
                .replace('?', ".");
            Ok(Value::String(lua.create_string(&re)?))
        })?;
        globals.set("MakeRegularExpression", make_re_fn)?;

        // GetPluginID()
        let get_plugin_id_fn = lua.create_function(move |lua, ()| {
            Ok(Value::String(lua.create_string("")?))
        })?;
        globals.set("GetPluginID", get_plugin_id_fn)?;

        // GetPluginInfo(id, code)
        let get_plugin_info_fn = lua.create_function(move |lua, (_id, code): (String, i64)| {
            match code {
                1 => Ok(Value::String(lua.create_string("RustLuaMud")?)),
                _ => Ok(Value::Nil),
            }
        })?;
        globals.set("GetPluginInfo", get_plugin_info_fn)?;

        // ============================================================
        // 模块加载机制
        // ============================================================

        // 覆盖 dofile — 支持 GBK 自动转码和路径分隔符兼容
        let script_path_rc = Rc::new(RefCell::new(self.script_path.clone()));
        let dofile_fn = lua.create_function_mut(move |lua, path: String| {
            // 将 \ 替换为 /
            let path = path.replace('\\', "/");

            let bytes = std::fs::read(&path)
                .map_err(|e| mlua::Error::external(format!("读取文件失败 '{}': {}", path, e)))?;

            let code = match std::str::from_utf8(&bytes) {
                Ok(s) => s.to_string(),
                Err(_) => {
                    let (cow, _, _) = encoding_rs::GBK.decode(&bytes);
                    cow.into_owned()
                }
            };

            lua.load(&code)
                .set_name(&path)
                .exec()
                .map_err(|e| mlua::Error::external(format!("脚本执行错误 '{}': {}", path, e)))
        })?;
        globals.set("dofile", dofile_fn)?;

        // 设置 require 路径
        let package: Table = globals.get("package")?;
        let current_path: String = package.get("path")?;
        let new_path = format!("./scripts/lua/?.lua;./scripts/lua/?/init.lua;{}", current_path);
        package.set("path", new_path)?;

        // ============================================================
        // Lua 兼容性补丁
        // ============================================================

        // table.getn
        {
            let table_mod: Table = globals.get("table")?;
            table_mod.set("getn", lua.create_function(|_, t: Table| {
                Ok(t.len().unwrap_or(0))
            })?)?;
        }

        // table.foreachi
        {
            let table_mod: Table = globals.get("table")?;
            table_mod.set("foreachi", lua.create_function(|_, (t, f): (Table, Function)| {
                let len = t.len().unwrap_or(0);
                for i in 1..=len {
                    let val: Value = t.get(i).unwrap_or(Value::Nil);
                    match f.call::<()>((i, val)) {
                        Ok(_) => {}
                        Err(e) => { return Err(e); }
                    }
                }
                Ok(())
            })?)?;
        }

        // table.foreach
        {
            let table_mod: Table = globals.get("table")?;
            table_mod.set("foreach", lua.create_function(|_, (t, f): (Table, Function)| {
                for pair in t.pairs::<Value, Value>() {
                    let (k, v) = pair?;
                    match f.call::<()>((k, v)) {
                        Ok(_) => {}
                        Err(e) => { return Err(e); }
                    }
                }
                Ok(())
            })?)?;
        }

        // math.mod
        {
            let math_mod: Table = globals.get("math")?;
            math_mod.set("mod", lua.create_function(|_, (a, b): (f64, f64)| {
                Ok(a % b)
            })?)?;
        }

        // math.pow
        {
            let math_mod: Table = globals.get("math")?;
            math_mod.set("pow", lua.create_function(|_, (a, b): (f64, f64)| {
                Ok(a.powf(b))
            })?)?;
        }

        // ============================================================
        // 原始 API（保留兼容）
        // ============================================================

        // trigger(pattern, callback)
        let state_rc33 = state_rc.clone();
        let trigger_fn = lua.create_function_mut(move |_, (pattern, callback): (String, Function)| {
            let re = Regex::new(&pattern)
                .map_err(|e| mlua::Error::external(format!("无效正则 '{}': {}", pattern, e)))?;
            state_rc33.borrow_mut().triggers.push(Trigger {
                name: String::new(),
                pattern: re,
                callback,
                enabled: true,
                group: String::new(),
                sequence: 0,
                multiline: false,
                lines_to_match: 1,
                omit_from_output: false,
                temporary: false,
                send_text: String::new(),
            });
            Ok(())
        })?;
        globals.set("trigger", trigger_fn)?;

        // alias(pattern, callback)
        let state_rc34 = state_rc.clone();
        let alias_fn = lua.create_function_mut(move |_, (pattern, callback): (String, Function)| {
            let re = Regex::new(&pattern)
                .map_err(|e| mlua::Error::external(format!("无效正则 '{}': {}", pattern, e)))?;
            state_rc34.borrow_mut().aliases.push(Alias {
                name: String::new(),
                pattern: re,
                callback,
                enabled: true,
                group: String::new(),
            });
            Ok(())
        })?;
        globals.set("alias", alias_fn)?;

        // timer(interval, callback)
        let state_rc35 = state_rc.clone();
        let timer_fn = lua.create_function_mut(move |_, (interval_secs, callback): (u64, Function)| {
            state_rc35.borrow_mut().timers.push(TimerDef {
                name: String::new(),
                interval_secs,
                callback,
                enabled: true,
                group: String::new(),
                one_shot: false,
                send_text: String::new(),
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

    /// 直接执行 Lua 代码（用于 /eval 命令）
    pub fn eval_code(&self, code: &str) -> Result<(), String> {
        self.lua.load(code)
            .exec()
            .map_err(|e| format!("{}", e))
    }

    /// 加载并执行 Lua 脚本文件
    /// 自动检测编码：先尝试 UTF-8，失败（GBK 编码）则自动转码
    pub fn load_script(&mut self, path: &str) -> Result<(), String> {
        let bytes = std::fs::read(path)
            .map_err(|e| format!("读取脚本失败 '{}': {}", path, e))?;

        let code = match std::str::from_utf8(&bytes) {
            Ok(s) => s.to_string(),
            Err(_) => {
                let (cow, _, _) = encoding_rs::GBK.decode(&bytes);
                cow.into_owned()
            }
        };

        self.lua.load(&code)
            .set_name(path)
            .exec()
            .map_err(|e| format!("脚本执行错误 '{}': {}", path, e))?;

        self.set_script_path(path);
        Ok(())
    }

    /// 获取当前加载的脚本路径
    pub fn script_path(&self) -> Option<&String> {
        self.script_path.as_ref()
    }

    /// 处理服务器输出，匹配触发器
    pub fn process_output(&self, line: &str) {
        // 清空待发送队列
        self.state.borrow_mut().pending_commands.clear();

        // 剥离 ANSI 码用于匹配
        let clean_line = crate::ui::AnsiParser::strip_ansi(line);

        // 维护最近行缓冲区
        {
            let mut state = self.state.borrow_mut();
            state.recent_lines.push(clean_line.clone());
            if state.recent_lines.len() > 20 {
                state.recent_lines.remove(0);
            }
        }

        // 收集需要触发的
        let matches: Vec<(usize, Vec<String>)> = {
            let state = self.state.borrow();
            let mut result = Vec::new();
            for (i, trigger) in state.triggers.iter().enumerate() {
                if !trigger.enabled { continue; }

                if trigger.multiline && trigger.lines_to_match > 1 {
                    let n = trigger.lines_to_match;
                    if state.recent_lines.len() >= n {
                        let combined: String = state.recent_lines
                            .iter()
                            .rev()
                            .take(n)
                            .rev()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join("\n");
                        // 多行模式下让 . 匹配换行符
                        let multiline_pattern = format!("(?s){}", trigger.pattern.as_str());
                        let multiline_re = Regex::new(&multiline_pattern).unwrap_or_else(|_| trigger.pattern.clone());
                        if let Some(caps) = multiline_re.captures(&combined) {
                            let caps_list: Vec<String> = caps.iter()
                                .skip(1)
                                .flatten()
                                .map(|m| m.as_str().to_string())
                                .collect();
                            result.push((i, caps_list));
                        }
                    }
                } else {
                    if let Some(caps) = trigger.pattern.captures(&clean_line) {
                        let caps_list: Vec<String> = caps.iter()
                            .skip(1)
                            .flatten()
                            .map(|m| m.as_str().to_string())
                            .collect();
                        result.push((i, caps_list));
                    }
                }
            }
            result
        };

        // 逐个触发
        for (idx, caps_list) in matches {
            let (callback, send_text) = {
                let state = self.state.borrow();
                (state.triggers[idx].callback.clone(), state.triggers[idx].send_text.clone())
            };
            if let Ok(args_table) = self.lua.create_table() {
                for (i, m) in caps_list.iter().enumerate() {
                    let _ = args_table.set(i + 1, m.as_str());
                }
                let _ = callback.call::<()>(args_table);
            }
            if !send_text.is_empty() {
                self.state.borrow_mut().pending_commands.push(send_text);
            }
        }
    }

    /// 处理用户输入，匹配别名
    pub fn process_input(&self, input: &str) -> bool {
        self.state.borrow_mut().pending_commands.clear();

        let matches: Vec<(usize, Vec<String>)> = {
            let state = self.state.borrow();
            let mut result = Vec::new();
            for (i, alias) in state.aliases.iter().enumerate() {
                if !alias.enabled { continue; }
                if let Some(caps) = alias.pattern.captures(input) {
                    let caps_list: Vec<String> = caps.iter()
                        .skip(1)
                        .flatten()
                        .map(|m| m.as_str().to_string())
                        .collect();
                    result.push((i, caps_list));
                }
            }
            result
        };

        if matches.is_empty() {
            return false;
        }

        for (idx, caps_list) in matches {
            let callback = {
                let state = self.state.borrow();
                state.aliases[idx].callback.clone()
            };
            if let Ok(args_table) = self.lua.create_table() {
                let _ = args_table.set(0, input);
                for (i, m) in caps_list.iter().enumerate() {
                    let _ = args_table.set(i + 1, m.as_str());
                }
                let _ = callback.call::<()>(args_table);
            }
        }

        true
    }

    /// 触发指定定时器
    pub fn fire_timer(&self, index: usize) {
        self.state.borrow_mut().pending_commands.clear();

        let (callback, send_text, one_shot) = {
            let state = self.state.borrow();
            if index < state.timers.len() && state.timers[index].enabled {
                (state.timers[index].callback.clone(), state.timers[index].send_text.clone(), state.timers[index].one_shot)
            } else {
                return;
            }
        };

        let _ = callback.call::<()>(());

        if !send_text.is_empty() {
            // send_text 可能是 Lua 代码（MUSHclient 的 script 参数）
            let _ = self.lua.load(&send_text).exec();
        }

        if one_shot {
            self.state.borrow_mut().timers.remove(index);
        }
    }

    /// 取出待发送的命令
    pub fn drain_commands(&self) -> Vec<String> {
        self.state.borrow_mut().pending_commands.drain(..).collect()
    }

    /// 设置 Lua 变量
    pub fn set_variable(&mut self, key: &str, value: &str) {
        self.state.borrow_mut().variables.insert(key.to_string(), value.to_string());
    }

    /// 设置连接状态
    pub fn set_connected(&mut self, connected: bool) {
        self.state.borrow_mut().connected = connected;
    }

    /// 取出连接请求标志（一次性消费）
    pub fn take_connect_requested(&self) -> bool {
        let val = self.state.borrow_mut().connect_requested;
        if val {
            self.state.borrow_mut().connect_requested = false;
        }
        val
    }

    /// 取出断开请求标志（一次性消费）
    pub fn take_disconnect_requested(&self) -> bool {
        let val = self.state.borrow_mut().disconnect_requested;
        if val {
            self.state.borrow_mut().disconnect_requested = false;
        }
        val
    }

    /// 取出待发送的日志消息
    pub fn drain_logs(&self) -> Vec<String> {
        self.state.borrow_mut().pending_logs.drain(..).collect()
    }

    /// 获取定时器列表（interval_secs）
    pub fn timer_intervals(&self) -> Vec<u64> {
        self.state.borrow().timers.iter()
            .filter(|t| t.enabled)
            .map(|t| t.interval_secs)
            .collect()
    }

    /// 获取触发器数量
    pub fn trigger_count(&self) -> usize {
        self.state.borrow().triggers.len()
    }

    /// 获取别名数量
    pub fn alias_count(&self) -> usize {
        self.state.borrow().aliases.len()
    }

    /// 获取定时器数量
    pub fn timer_count(&self) -> usize {
        self.state.borrow().timers.len()
    }
}

/// 添加触发器的通用实现
fn add_trigger_impl(
    lua: &Lua,
    state_rc: &Rc<RefCell<ScriptState>>,
    name: &str,
    match_str: &str,
    flags: i64,
    script: &str,
    _send_to: i64,
    sequence: i32,
) -> LuaResult<Value> {
    let case_insensitive = (flags & 16) != 0;
    let is_regex = (flags & 32) != 0;

    let re_str = if is_regex {
        match_str.to_string()
    } else {
        regex_escape(match_str).replace('*', "(.*)").replace('?', "(.)")
    };

    let re_str = if case_insensitive {
        format!("(?i){}", re_str)
    } else {
        re_str
    };

    let re = Regex::new(&re_str)
        .map_err(|e| mlua::Error::external(format!("无效正则 '{}': {}", re_str, e)))?;

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

    state_rc.borrow_mut().triggers.push(Trigger {
        name: name.to_string(),
        pattern: re,
        callback,
        enabled: (flags & 1) != 0,
        group: String::new(),
        sequence,
        multiline: false,
        lines_to_match: 1,
        omit_from_output: false,
        temporary: (flags & 4096) != 0,
        send_text: String::new(),
    });

    Ok(Value::Integer(0))
}

/// 转义正则特殊字符（保留 * 和 ? 用于通配符转换）
fn regex_escape(s: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 辅助：创建引擎并执行一段 Lua 代码
    fn with_engine<F>(f: F)
    where
        F: FnOnce(&mut LuaEngine),
    {
        let mut engine = LuaEngine::new().expect("引擎创建失败");
        f(&mut engine);
    }

    /// 辅助：执行 Lua 代码并返回结果
    fn eval<T: mlua::FromLua>(engine: &LuaEngine, code: &str) -> mlua::Result<T> {
        engine.lua.load(code).eval()
    }

    /// 辅助：执行 Lua 代码（无返回值）
    fn exec(engine: &LuaEngine, code: &str) -> mlua::Result<()> {
        engine.lua.load(code).exec()
    }

    // ================================================================
    // 引擎基础
    // ================================================================

    #[test]
    fn test_engine_new() {
        let engine = LuaEngine::new();
        assert!(engine.is_ok());
    }

    #[test]
    fn test_set_script_path() {
        with_engine(|engine| {
            engine.set_script_path("/home/user/scripts/main.lua");
            assert_eq!(engine.script_dir, Some("/home/user/scripts/".to_string()));
            assert_eq!(engine.script_path, Some("/home/user/scripts/main.lua".to_string()));
        });
    }

    #[test]
    fn test_set_script_path_no_slash() {
        with_engine(|engine| {
            engine.set_script_path("main.lua");
            assert_eq!(engine.script_dir, Some("./".to_string()));
        });
    }

    // ================================================================
    // 命令执行 API
    // ================================================================

    #[test]
    fn test_send() {
        with_engine(|engine| {
            exec(engine, "send('look')").unwrap();
            let cmds = engine.drain_commands();
            assert_eq!(cmds, vec!["look"]);
        });
    }

    #[test]
    fn test_execute() {
        with_engine(|engine| {
            let result: i64 = eval(engine, "return Execute('look')").unwrap();
            assert_eq!(result, 0);
            let cmds = engine.drain_commands();
            assert_eq!(cmds, vec!["look"]);
        });
    }

    // ================================================================
    // 输出 API
    // ================================================================

    #[test]
    fn test_note() {
        with_engine(|engine| {
            exec(engine, "Note('hello')").unwrap();
            let logs = engine.drain_logs();
            assert!(logs.contains(&"hello".to_string()));
        });
    }

    #[test]
    fn test_colour_note() {
        with_engine(|engine| {
            exec(engine, "ColourNote('red', 'black', 'test')").unwrap();
            let logs = engine.drain_logs();
            assert!(logs.iter().any(|l| l.contains("test")));
        });
    }

    #[test]
    fn test_tell() {
        with_engine(|engine| {
            exec(engine, "Tell('inline')").unwrap();
            let logs = engine.drain_logs();
            assert!(logs.contains(&"inline".to_string()));
        });
    }

    // ================================================================
    // 触发器 API
    // ================================================================

    #[test]
    fn test_add_trigger() {
        with_engine(|engine| {
            let result: i64 = eval(engine,
                "return AddTrigger('test_trig', 'hello', '', 1, 0, 0, '', '', 0, 0)"
            ).unwrap();
            assert_eq!(result, 0);
            assert_eq!(engine.trigger_count(), 1);
        });
    }

    #[test]
    fn test_add_trigger_regex() {
        with_engine(|engine| {
            let result: i64 = eval(engine,
                r#"return AddTrigger('regex_trig', [[^\d+hp]], '', 33, 0, 0, '', '', 0, 0)"#
            ).unwrap();
            assert_eq!(result, 0);
        });
    }

    #[test]
    fn test_add_trigger_case_insensitive() {
        with_engine(|engine| {
            let result: i64 = eval(engine,
                "return AddTrigger('ci_trig', 'HELLO', '', 17, 0, 0, '', '', 0, 0)"
            ).unwrap();
            assert_eq!(result, 0);
        });
    }

    #[test]
    fn test_delete_trigger() {
        with_engine(|engine| {
            exec(engine,
                "AddTrigger('del_trig', 'test', '', 1, 0, 0, '', '', 0, 0)"
            ).unwrap();
            assert_eq!(engine.trigger_count(), 1);
            let result: i64 = eval(engine, "return DeleteTrigger('del_trig')").unwrap();
            assert_eq!(result, 0);
            assert_eq!(engine.trigger_count(), 0);
        });
    }

    #[test]
    fn test_delete_trigger_not_found() {
        with_engine(|engine| {
            let result: i64 = eval(engine, "return DeleteTrigger('nonexistent')").unwrap();
            assert_eq!(result, 1);
        });
    }

    #[test]
    fn test_get_trigger_list() {
        with_engine(|engine| {
            exec(engine, "AddTrigger('trig1', 'a', '', 1, 0, 0, '', '', 0, 0)").unwrap();
            exec(engine, "AddTrigger('trig2', 'b', '', 1, 0, 0, '', '', 0, 0)").unwrap();
            let list: Vec<String> = eval(engine,
                "local t = GetTriggerList(); local r = {}; for i=1,#t do r[i]=t[i] end; return r"
            ).unwrap();
            assert!(list.contains(&"trig1".to_string()));
            assert!(list.contains(&"trig2".to_string()));
        });
    }

    #[test]
    fn test_get_trigger_info_enabled() {
        with_engine(|engine| {
            exec(engine, "AddTrigger('info_trig', 'test', '', 1, 0, 0, '', '', 0, 0)").unwrap();
            let enabled: bool = eval(engine, "return GetTriggerInfo('info_trig', 8)").unwrap();
            assert!(enabled);
        });
    }

    #[test]
    fn test_get_trigger_info_group() {
        with_engine(|engine| {
            exec(engine, "AddTrigger('grp_trig', 'test', '', 1, 0, 0, '', '', 0, 0)").unwrap();
            exec(engine, "SetTriggerOption('grp_trig', 'group', 'mygroup')").unwrap();
            let group: String = eval(engine, "return GetTriggerInfo('grp_trig', 26)").unwrap();
            assert_eq!(group, "mygroup");
        });
    }

    #[test]
    fn test_set_trigger_option() {
        with_engine(|engine| {
            exec(engine, "AddTrigger('opt_trig', 'test', '', 1, 0, 0, '', '', 0, 0)").unwrap();
            exec(engine, "SetTriggerOption('opt_trig', 'enabled', false)").unwrap();
            let enabled: bool = eval(engine, "return GetTriggerInfo('opt_trig', 8)").unwrap();
            assert!(!enabled);
        });
    }

    #[test]
    fn test_set_trigger_option_multiline() {
        with_engine(|engine| {
            exec(engine, "AddTrigger('ml_trig', 'test', '', 1, 0, 0, '', '', 0, 0)").unwrap();
            let result: i64 = eval(engine,
                "SetTriggerOption('ml_trig', 'multi_line', true); SetTriggerOption('ml_trig', 'lines_to_match', 3); return 0"
            ).unwrap();
            assert_eq!(result, 0);
        });
    }

    #[test]
    fn test_enable_trigger_group() {
        with_engine(|engine| {
            exec(engine, "AddTrigger('g1', 'a', '', 1, 0, 0, '', '', 0, 0)").unwrap();
            exec(engine, "AddTrigger('g2', 'b', '', 1, 0, 0, '', '', 0, 0)").unwrap();
            exec(engine, "SetTriggerOption('g1', 'group', 'grp_a')").unwrap();
            exec(engine, "SetTriggerOption('g2', 'group', 'grp_a')").unwrap();
            exec(engine, "EnableTriggerGroup('grp_a', false)").unwrap();
            let e1: bool = eval(engine, "return GetTriggerInfo('g1', 8)").unwrap();
            let e2: bool = eval(engine, "return GetTriggerInfo('g2', 8)").unwrap();
            assert!(!e1);
            assert!(!e2);
        });
    }

    #[test]
    fn test_enable_trigger_group_skips_empty_group() {
        with_engine(|engine| {
            exec(engine, "AddTrigger('nogrp', 'x', '', 1, 0, 0, '', '', 0, 0)").unwrap();
            exec(engine, "EnableTriggerGroup('somegroup', false)").unwrap();
            let enabled: bool = eval(engine, "return GetTriggerInfo('nogrp', 8)").unwrap();
            assert!(enabled); // 空group的触发器不应被影响
        });
    }

    #[test]
    fn test_enable_trigger() {
        with_engine(|engine| {
            exec(engine, "AddTrigger('et', 'test', '', 1, 0, 0, '', '', 0, 0)").unwrap();
            exec(engine, "EnableTrigger('et', false)").unwrap();
            let enabled: bool = eval(engine, "return GetTriggerInfo('et', 8)").unwrap();
            assert!(!enabled);
            exec(engine, "EnableTrigger('et', true)").unwrap();
            let enabled2: bool = eval(engine, "return GetTriggerInfo('et', 8)").unwrap();
            assert!(enabled2);
        });
    }

    #[test]
    fn test_enable_trigger_not_found() {
        with_engine(|engine| {
            let result: i64 = eval(engine, "return EnableTrigger('nonexistent', true)").unwrap();
            assert_eq!(result, 1); // 1 = not found
        });
    }

    #[test]
    fn test_trigger_matching() {
        with_engine(|engine| {
            exec(engine, r#"
                test_result = nil
                AddTrigger('match_trig', [[hello (\w+)]], '', 33, 0, 0, '', 'function(t) test_result = t[1] end', 0, 0)
            "#).unwrap();
            engine.process_output("hello world");
            let result: Option<String> = eval(engine, "return test_result").unwrap();
            assert_eq!(result, Some("world".to_string()));
        });
    }

    #[test]
    fn test_trigger_disabled_not_matching() {
        with_engine(|engine| {
            exec(engine, r#"
                test_result = nil
                AddTrigger('dis_trig', 'test', '', 0, 0, 0, '', 'function() test_result = true end', 0, 0)
            "#).unwrap();
            engine.process_output("test");
            let result: Option<bool> = eval(engine, "return test_result").unwrap();
            assert_eq!(result, None);
        });
    }

    #[test]
    fn test_trigger_wildcard_matching() {
        with_engine(|engine| {
            exec(engine, r#"
                wc_result = nil
                AddTrigger('wc_trig', 'You see * here', '', 1, 0, 0, '', 'function(t) wc_result = t[1] end', 0, 0)
            "#).unwrap();
            engine.process_output("You see a goblin here");
            let result: Option<String> = eval(engine, "return wc_result").unwrap();
            assert_eq!(result, Some("a goblin".to_string()));
        });
    }

    #[test]
    fn test_trigger_case_insensitive_matching() {
        with_engine(|engine| {
            exec(engine, r#"
                ci_result = nil
                AddTrigger('ci_trig2', 'HELLO', '', 17, 0, 0, '', 'function() ci_result = true end', 0, 0)
            "#).unwrap();
            engine.process_output("hello");
            let result: Option<bool> = eval(engine, "return ci_result").unwrap();
            assert_eq!(result, Some(true));
        });
    }

    #[test]
    fn test_add_trigger_ex_same_as_add_trigger() {
        with_engine(|engine| {
            let result: i64 = eval(engine,
                "return AddTriggerEx('ex_trig', 'test', '', 1, 0, 0, '', '', 0, 0)"
            ).unwrap();
            assert_eq!(result, 0);
            assert_eq!(engine.trigger_count(), 1);
        });
    }

    #[test]
    fn test_trigger_omit_from_output() {
        with_engine(|engine| {
            exec(engine, "AddTrigger('omit_trig', 'secret', '', 1, 0, 0, '', '', 0, 0)").unwrap();
            exec(engine, "SetTriggerOption('omit_trig', 'omit_from_output', true)").unwrap();
            // omit_from_output 标记已设置，验证通过 GetTriggerInfo 间接确认
            // 实际的 omit 行为由 app 层处理
            assert_eq!(engine.trigger_count(), 1);
        });
    }

    #[test]
    fn test_trigger_temporary_flag() {
        with_engine(|engine| {
            // flag 4096 = Temporary
            let result: i64 = eval(engine,
                "return AddTrigger('temp_trig', 'test', '', 4097, 0, 0, '', '', 0, 0)"
            ).unwrap();
            assert_eq!(result, 0);
        });
    }

    #[test]
    fn test_trigger_sequence() {
        with_engine(|engine| {
            let result: i64 = eval(engine,
                "return AddTrigger('seq_trig', 'test', '', 1, 0, 0, '', '', 0, 100)"
            ).unwrap();
            assert_eq!(result, 0);
        });
    }

    #[test]
    fn test_get_trigger_info_unknown_code() {
        with_engine(|engine| {
            exec(engine, "AddTrigger('unk_trig', 'test', '', 1, 0, 0, '', '', 0, 0)").unwrap();
            let val: Value = eval(engine, "return GetTriggerInfo('unk_trig', 999)").unwrap();
            assert!(val.is_nil());
        });
    }

    #[test]
    fn test_get_trigger_info_not_found() {
        with_engine(|engine| {
            let val: Value = eval(engine, "return GetTriggerInfo('nonexistent', 8)").unwrap();
            assert!(val.is_nil());
        });
    }

    #[test]
    fn test_set_trigger_option_not_found() {
        with_engine(|engine| {
            let result: i64 = eval(engine, "return SetTriggerOption('nonexistent', 'enabled', true)").unwrap();
            assert_eq!(result, 1); // 1 = not found
        });
    }

    // ================================================================
    // 别名 API
    // ================================================================

    #[test]
    fn test_add_alias() {
        with_engine(|engine| {
            let result: i64 = eval(engine,
                "return AddAlias('test_alias', 'kill *', '', 1, 0, '', 0)"
            ).unwrap();
            assert_eq!(result, 0);
            assert_eq!(engine.alias_count(), 1);
        });
    }

    #[test]
    fn test_add_alias_regex() {
        with_engine(|engine| {
            let result: i64 = eval(engine,
                r#"return AddAlias('regex_alias', [[^go (\w+)$]], '', 33, 0, '', 0)"#
            ).unwrap();
            assert_eq!(result, 0);
        });
    }

    #[test]
    fn test_delete_alias() {
        with_engine(|engine| {
            exec(engine, "AddAlias('del_alias', 'test', '', 1, 0, '', 0)").unwrap();
            let result: i64 = eval(engine, "return DeleteAlias('del_alias')").unwrap();
            assert_eq!(result, 0);
            assert_eq!(engine.alias_count(), 0);
        });
    }

    #[test]
    fn test_delete_alias_not_found() {
        with_engine(|engine| {
            let result: i64 = eval(engine, "return DeleteAlias('nonexistent')").unwrap();
            assert_eq!(result, 1);
        });
    }

    #[test]
    fn test_get_alias_list() {
        with_engine(|engine| {
            exec(engine, "AddAlias('a1', 'x', '', 1, 0, '', 0)").unwrap();
            exec(engine, "AddAlias('a2', 'y', '', 1, 0, '', 0)").unwrap();
            let list: Vec<String> = eval(engine,
                "local t = GetAliasList(); local r = {}; for i=1,#t do r[i]=t[i] end; return r"
            ).unwrap();
            assert!(list.contains(&"a1".to_string()));
            assert!(list.contains(&"a2".to_string()));
        });
    }

    #[test]
    fn test_set_alias_option() {
        with_engine(|engine| {
            exec(engine, "AddAlias('opt_alias', 'test', '', 1, 0, '', 0)").unwrap();
            exec(engine, "SetAliasOption('opt_alias', 'group', 'mygroup')").unwrap();
            let result: i64 = eval(engine, "return SetAliasOption('opt_alias', 'enabled', false)").unwrap();
            assert_eq!(result, 0);
        });
    }

    #[test]
    fn test_set_alias_option_not_found() {
        with_engine(|engine| {
            let result: i64 = eval(engine, "return SetAliasOption('nonexistent', 'enabled', true)").unwrap();
            assert_eq!(result, 1);
        });
    }

    #[test]
    fn test_alias_matching() {
        with_engine(|engine| {
            exec(engine, r#"
                alias_result = nil
                AddAlias('match_alias', 'kill *', '', 1, 0, 'function(t) alias_result = t[1] end', 0)
            "#).unwrap();
            let matched = engine.process_input("kill goblin");
            assert!(matched);
            let result: Option<String> = eval(engine, "return alias_result").unwrap();
            assert_eq!(result, Some("goblin".to_string()));
        });
    }

    #[test]
    fn test_alias_no_match() {
        with_engine(|engine| {
            exec(engine, "AddAlias('no_match', 'kill *', '', 1, 0, '', 0)").unwrap();
            let matched = engine.process_input("look");
            assert!(!matched);
        });
    }

    #[test]
    fn test_alias_regex_matching() {
        with_engine(|engine| {
            exec(engine, r#"
                regex_alias_result = nil
                AddAlias('regex_al', [[^go (\w+)$]], '', 33, 0, 'function(t) regex_alias_result = t[1] end', 0)
            "#).unwrap();
            let matched = engine.process_input("go north");
            assert!(matched);
            let result: Option<String> = eval(engine, "return regex_alias_result").unwrap();
            assert_eq!(result, Some("north".to_string()));
        });
    }

    #[test]
    fn test_alias_disabled_not_matching() {
        with_engine(|engine| {
            exec(engine, "AddAlias('dis_al', 'test', '', 0, 0, '', 0)").unwrap();
            let matched = engine.process_input("test");
            assert!(!matched);
        });
    }

    #[test]
    fn test_alias_wildcard_question_mark() {
        with_engine(|engine| {
            exec(engine, r#"
                qm_result = nil
                AddAlias('qm_alias', 'go ?', '', 1, 0, 'function(t) qm_result = t[1] end', 0)
            "#).unwrap();
            let matched = engine.process_input("go n");
            assert!(matched);
            let result: Option<String> = eval(engine, "return qm_result").unwrap();
            assert_eq!(result, Some("n".to_string()));
        });
    }

    // ================================================================
    // 定时器 API
    // ================================================================

    #[test]
    fn test_add_timer() {
        with_engine(|engine| {
            let result: i64 = eval(engine,
                "return AddTimer('test_timer', 0, 0, 5, 0, 1, 0, '', 0)"
            ).unwrap();
            assert_eq!(result, 0);
            assert_eq!(engine.timer_count(), 1);
        });
    }

    #[test]
    fn test_delete_timer() {
        with_engine(|engine| {
            exec(engine, "AddTimer('del_timer', 0, 0, 5, 0, 1, 0, '', 0)").unwrap();
            let result: i64 = eval(engine, "return DeleteTimer('del_timer')").unwrap();
            assert_eq!(result, 0);
            assert_eq!(engine.timer_count(), 0);
        });
    }

    #[test]
    fn test_delete_timer_not_found() {
        with_engine(|engine| {
            let result: i64 = eval(engine, "return DeleteTimer('nonexistent')").unwrap();
            assert_eq!(result, 1);
        });
    }

    #[test]
    fn test_get_timer_list() {
        with_engine(|engine| {
            exec(engine, "AddTimer('t1', 0, 0, 5, 0, 1, 0, '', 0)").unwrap();
            exec(engine, "AddTimer('t2', 0, 0, 10, 0, 1, 0, '', 0)").unwrap();
            let list: Vec<String> = eval(engine,
                "local t = GetTimerList(); local r = {}; for i=1,#t do r[i]=t[i] end; return r"
            ).unwrap();
            assert!(list.contains(&"t1".to_string()));
            assert!(list.contains(&"t2".to_string()));
        });
    }

    #[test]
    fn test_get_timer_info() {
        with_engine(|engine| {
            exec(engine, "AddTimer('info_timer', 0, 0, 5, 0, 1, 0, '', 0)").unwrap();
            let enabled: bool = eval(engine, "return GetTimerInfo('info_timer', 6)").unwrap();
            assert!(enabled);
        });
    }

    #[test]
    fn test_get_timer_info_group() {
        with_engine(|engine| {
            exec(engine, "AddTimer('grp_timer', 0, 0, 5, 0, 1, 0, '', 0)").unwrap();
            exec(engine, "SetTimerOption('grp_timer', 'group', 'mygroup')").unwrap();
            let group: String = eval(engine, "return GetTimerInfo('grp_timer', 19)").unwrap();
            assert_eq!(group, "mygroup");
        });
    }

    #[test]
    fn test_get_timer_info_not_found() {
        with_engine(|engine| {
            let val: Value = eval(engine, "return GetTimerInfo('nonexistent', 6)").unwrap();
            assert!(val.is_nil());
        });
    }

    #[test]
    fn test_set_timer_option() {
        with_engine(|engine| {
            exec(engine, "AddTimer('opt_timer', 0, 0, 5, 0, 1, 0, '', 0)").unwrap();
            exec(engine, "SetTimerOption('opt_timer', 'enabled', false)").unwrap();
            let enabled: bool = eval(engine, "return GetTimerInfo('opt_timer', 6)").unwrap();
            assert!(!enabled);
        });
    }

    #[test]
    fn test_enable_timer_group() {
        with_engine(|engine| {
            exec(engine, "AddTimer('tg1', 0, 0, 5, 0, 1, 0, '', 0)").unwrap();
            exec(engine, "AddTimer('tg2', 0, 0, 10, 0, 1, 0, '', 0)").unwrap();
            exec(engine, "SetTimerOption('tg1', 'group', 'grp_t')").unwrap();
            exec(engine, "SetTimerOption('tg2', 'group', 'grp_t')").unwrap();
            exec(engine, "EnableTimerGroup('grp_t', false)").unwrap();
            let e1: bool = eval(engine, "return GetTimerInfo('tg1', 6)").unwrap();
            assert!(!e1);
        });
    }

    #[test]
    fn test_enable_timer_group_skips_empty_group() {
        with_engine(|engine| {
            exec(engine, "AddTimer('nogrp_t', 0, 0, 5, 0, 1, 0, '', 0)").unwrap();
            exec(engine, "EnableTimerGroup('somegroup', false)").unwrap();
            let enabled: bool = eval(engine, "return GetTimerInfo('nogrp_t', 6)").unwrap();
            assert!(enabled); // 空group的定时器不应被影响
        });
    }

    #[test]
    fn test_enable_timer() {
        with_engine(|engine| {
            exec(engine, "AddTimer('et_t', 0, 0, 5, 0, 1, 0, '', 0)").unwrap();
            exec(engine, "EnableTimer('et_t', false)").unwrap();
            let enabled: bool = eval(engine, "return GetTimerInfo('et_t', 6)").unwrap();
            assert!(!enabled);
            exec(engine, "EnableTimer('et_t', true)").unwrap();
            let enabled2: bool = eval(engine, "return GetTimerInfo('et_t', 6)").unwrap();
            assert!(enabled2);
        });
    }

    #[test]
    fn test_enable_timer_not_found() {
        with_engine(|engine| {
            let result: i64 = eval(engine, "return EnableTimer('nonexistent', true)").unwrap();
            assert_eq!(result, 1);
        });
    }

    #[test]
    fn test_timer_intervals() {
        with_engine(|engine| {
            exec(engine, "AddTimer('i1', 0, 0, 5, 0, 1, 0, '', 0)").unwrap();
            exec(engine, "AddTimer('i2', 0, 0, 10, 0, 1, 0, '', 0)").unwrap();
            let intervals = engine.timer_intervals();
            assert!(intervals.contains(&5));
            assert!(intervals.contains(&10));
        });
    }

    #[test]
    fn test_fire_timer() {
        with_engine(|engine| {
            exec(engine, r#"
                timer_result = nil
                AddTimer('fire_t', 0, 0, 5, 0, 1, 0, 'timer_result = "fired"', 0)
            "#).unwrap();
            engine.fire_timer(0);
            let result: Option<String> = eval(engine, "return timer_result").unwrap();
            assert_eq!(result, Some("fired".to_string()));
        });
    }

    #[test]
    fn test_fire_timer_one_shot() {
        with_engine(|engine| {
            // flag 4 = OneShot, flag 1 = Enabled
            exec(engine, "AddTimer('oneshot', 0, 0, 5, 0, 5, 0, '', 0)").unwrap();
            assert_eq!(engine.timer_count(), 1);
            engine.fire_timer(0);
            assert_eq!(engine.timer_count(), 0);
        });
    }

    #[test]
    fn test_fire_timer_disabled() {
        with_engine(|engine| {
            exec(engine, r#"
                disabled_timer_result = nil
                AddTimer('dis_t', 0, 0, 5, 0, 0, 0, 'disabled_timer_result = true', 0)
            "#).unwrap();
            engine.fire_timer(0);
            let result: Option<bool> = eval(engine, "return disabled_timer_result").unwrap();
            assert_eq!(result, None);
        });
    }

    #[test]
    fn test_timer_zero_interval() {
        with_engine(|engine| {
            // 0秒间隔应被设为1秒
            exec(engine, "AddTimer('zero_t', 0, 0, 0, 0, 1, 0, '', 0)").unwrap();
            let intervals = engine.timer_intervals();
            assert!(intervals.contains(&1));
        });
    }

    // ================================================================
    // 变量 API
    // ================================================================

    #[test]
    fn test_set_get_variable() {
        with_engine(|engine| {
            exec(engine, "SetVariable('key1', 'value1')").unwrap();
            let val: String = eval(engine, "return GetVariable('key1')").unwrap();
            assert_eq!(val, "value1");
        });
    }

    #[test]
    fn test_get_variable_not_found() {
        with_engine(|engine| {
            let val: Value = eval(engine, "return GetVariable('nonexistent')").unwrap();
            assert!(val.is_nil());
        });
    }

    #[test]
    fn test_delete_variable() {
        with_engine(|engine| {
            exec(engine, "SetVariable('del_key', 'val')").unwrap();
            exec(engine, "DeleteVariable('del_key')").unwrap();
            let val: Value = eval(engine, "return GetVariable('del_key')").unwrap();
            assert!(val.is_nil());
        });
    }

    #[test]
    fn test_get_variable_list() {
        with_engine(|engine| {
            exec(engine, "SetVariable('a', '1')").unwrap();
            exec(engine, "SetVariable('b', '2')").unwrap();
            // GetVariableList 返回 key-value 对表
            let val_a: String = eval(engine, "local t = GetVariableList(); return t.a").unwrap();
            let val_b: String = eval(engine, "local t = GetVariableList(); return t.b").unwrap();
            assert_eq!(val_a, "1");
            assert_eq!(val_b, "2");
        });
    }

    #[test]
    fn test_set_variable_rust() {
        with_engine(|engine| {
            engine.set_variable("rust_key", "rust_val");
            let val: String = eval(engine, "return GetVariable('rust_key')").unwrap();
            assert_eq!(val, "rust_val");
        });
    }

    #[test]
    fn test_variable_overwrite() {
        with_engine(|engine| {
            exec(engine, "SetVariable('ow_key', 'old')").unwrap();
            exec(engine, "SetVariable('ow_key', 'new')").unwrap();
            let val: String = eval(engine, "return GetVariable('ow_key')").unwrap();
            assert_eq!(val, "new");
        });
    }

    // ================================================================
    // 配置 API
    // ================================================================

    #[test]
    fn test_get_info_35() {
        with_engine(|engine| {
            engine.set_script_path("/home/user/scripts/main.lua");
            let path: String = eval(engine, "return GetInfo(35)").unwrap();
            assert!(path.contains('\\'));
            assert!(!path.contains('/'));
        });
    }

    #[test]
    fn test_get_info_35_no_script_path() {
        with_engine(|engine| {
            let path: String = eval(engine, "return GetInfo(35)").unwrap();
            assert_eq!(path, ".\\");
        });
    }

    #[test]
    fn test_get_info_1() {
        with_engine(|engine| {
            let ver: String = eval(engine, "return GetInfo(1)").unwrap();
            assert!(ver.starts_with("RustLuaMud"));
        });
    }

    #[test]
    fn test_get_info_unknown() {
        with_engine(|engine| {
            let val: String = eval(engine, "return GetInfo(999)").unwrap();
            assert_eq!(val, "");
        });
    }

    #[test]
    fn test_set_get_option() {
        with_engine(|engine| {
            exec(engine, "SetOption('myopt', 42)").unwrap();
            let val: i64 = eval(engine, "return GetOption('myopt')").unwrap();
            assert_eq!(val, 42);
        });
    }

    #[test]
    fn test_get_option_not_found() {
        with_engine(|engine| {
            let val: Value = eval(engine, "return GetOption('nonexistent')").unwrap();
            assert!(val.is_nil());
        });
    }

    #[test]
    fn test_set_get_alpha_option() {
        with_engine(|engine| {
            exec(engine, "SetAlphaOption('myalpha', 'hello')").unwrap();
            let val: String = eval(engine, "return GetAlphaOption('myalpha')").unwrap();
            assert_eq!(val, "hello");
        });
    }

    #[test]
    fn test_get_alpha_option_not_found() {
        with_engine(|engine| {
            let val: Value = eval(engine, "return GetAlphaOption('nonexistent')").unwrap();
            assert!(val.is_nil());
        });
    }

    // ================================================================
    // 连接状态 API
    // ================================================================

    #[test]
    fn test_is_connected_default() {
        with_engine(|engine| {
            let connected: bool = eval(engine, "return IsConnected()").unwrap();
            assert!(!connected);
        });
    }

    #[test]
    fn test_connect_disconnect() {
        with_engine(|engine| {
            exec(engine, "Connect()").unwrap();
            assert!(engine.take_connect_requested());
            assert!(!engine.take_connect_requested());

            exec(engine, "Disconnect()").unwrap();
            assert!(engine.take_disconnect_requested());
        });
    }

    // ================================================================
    // 工具函数
    // ================================================================

    #[test]
    fn test_get_unique_number() {
        with_engine(|engine| {
            let n1: i64 = eval(engine, "return GetUniqueNumber()").unwrap();
            let n2: i64 = eval(engine, "return GetUniqueNumber()").unwrap();
            assert!(n2 > n1);
        });
    }

    #[test]
    fn test_trim() {
        with_engine(|engine| {
            let result: String = eval(engine, "return Trim('  hello  ')").unwrap();
            assert_eq!(result, "hello");
        });
    }

    #[test]
    fn test_trim_no_whitespace() {
        with_engine(|engine| {
            let result: String = eval(engine, "return Trim('hello')").unwrap();
            assert_eq!(result, "hello");
        });
    }

    // ================================================================
    // 日志 API
    // ================================================================

    #[test]
    fn test_is_log_open() {
        with_engine(|engine| {
            let open: bool = eval(engine, "return IsLogOpen()").unwrap();
            assert!(open);
        });
    }

    #[test]
    fn test_open_log() {
        with_engine(|engine| {
            // OpenLog 不应报错
            exec(engine, "OpenLog('test.log', true)").unwrap();
        });
    }

    // ================================================================
    // 常量表
    // ================================================================

    #[test]
    fn test_trigger_flag_constants() {
        with_engine(|engine| {
            let enabled: i64 = eval(engine, "return trigger_flag.Enabled").unwrap();
            assert_eq!(enabled, 1);
            let regex: i64 = eval(engine, "return trigger_flag.RegularExpression").unwrap();
            assert_eq!(regex, 32);
            let temp: i64 = eval(engine, "return trigger_flag.Temporary").unwrap();
            assert_eq!(temp, 4096);
        });
    }

    #[test]
    fn test_alias_flag_constants() {
        with_engine(|engine| {
            let enabled: i64 = eval(engine, "return alias_flag.Enabled").unwrap();
            assert_eq!(enabled, 1);
            let regex: i64 = eval(engine, "return alias_flag.RegularExpression").unwrap();
            assert_eq!(regex, 32);
        });
    }

    #[test]
    fn test_timer_flag_constants() {
        with_engine(|engine| {
            let enabled: i64 = eval(engine, "return timer_flag.Enabled").unwrap();
            assert_eq!(enabled, 1);
            let oneshot: i64 = eval(engine, "return timer_flag.OneShot").unwrap();
            assert_eq!(oneshot, 8192);
        });
    }

    #[test]
    fn test_error_code_constants() {
        with_engine(|engine| {
            let eok: i64 = eval(engine, "return error_code.eOK").unwrap();
            assert_eq!(eok, 0);
            let ebad: i64 = eval(engine, "return error_code.eBadRegularExpression").unwrap();
            assert_eq!(ebad, 3);
        });
    }

    #[test]
    fn test_error_desc_constants() {
        with_engine(|engine| {
            let eok: String = eval(engine, "return error_desc.eOK").unwrap();
            assert_eq!(eok, "OK");
        });
    }

    #[test]
    fn test_custom_colour_constants() {
        with_engine(|engine| {
            let black: i64 = eval(engine, "return custom_colour.Black").unwrap();
            assert_eq!(black, 0);
            let white: i64 = eval(engine, "return custom_colour.White").unwrap();
            assert_eq!(white, 15);
        });
    }

    // ================================================================
    // bit 库
    // ================================================================

    #[test]
    fn test_bit_bor() {
        with_engine(|engine| {
            let result: i64 = eval(engine, "return bit.bor(1, 2)").unwrap();
            assert_eq!(result, 3);
        });
    }

    #[test]
    fn test_bit_band() {
        with_engine(|engine| {
            let result: i64 = eval(engine, "return bit.band(3, 1)").unwrap();
            assert_eq!(result, 1);
        });
    }

    #[test]
    fn test_bit_bxor() {
        with_engine(|engine| {
            let result: i64 = eval(engine, "return bit.bxor(5, 3)").unwrap();
            assert_eq!(result, 6);
        });
    }

    #[test]
    fn test_bit_bnot() {
        with_engine(|engine| {
            let result: i64 = eval(engine, "return bit.bnot(0)").unwrap();
            assert_eq!(result, -1);
        });
    }

    #[test]
    fn test_bit_lshift() {
        with_engine(|engine| {
            let result: i64 = eval(engine, "return bit.lshift(1, 4)").unwrap();
            assert_eq!(result, 16);
        });
    }

    #[test]
    fn test_bit_rshift() {
        with_engine(|engine| {
            let result: i64 = eval(engine, "return bit.rshift(16, 4)").unwrap();
            assert_eq!(result, 1);
        });
    }

    // ================================================================
    // wait.lua 依赖
    // ================================================================

    #[test]
    fn test_make_regular_expression() {
        with_engine(|engine| {
            let result: String = eval(engine, "return MakeRegularExpression('hello * world?')").unwrap();
            assert_eq!(result, "hello .* world.");
        });
    }

    #[test]
    fn test_get_plugin_id() {
        with_engine(|engine| {
            let id: String = eval(engine, "return GetPluginID()").unwrap();
            assert_eq!(id, "");
        });
    }

    #[test]
    fn test_get_plugin_info() {
        with_engine(|engine| {
            let name: String = eval(engine, "return GetPluginInfo('', 1)").unwrap();
            assert_eq!(name, "RustLuaMud");
        });
    }

    // ================================================================
    // Lua 兼容性补丁
    // ================================================================

    #[test]
    fn test_table_getn() {
        with_engine(|engine| {
            let n: i64 = eval(engine, "return table.getn({1, 2, 3})").unwrap();
            assert_eq!(n, 3);
        });
    }

    #[test]
    fn test_table_foreachi() {
        with_engine(|engine| {
            let result: i64 = eval(engine, r#"
                local sum = 0
                table.foreachi({10, 20, 30}, function(i, v) sum = sum + v end)
                return sum
            "#).unwrap();
            assert_eq!(result, 60);
        });
    }

    #[test]
    fn test_table_foreach() {
        with_engine(|engine| {
            let result: i64 = eval(engine, r#"
                local sum = 0
                local t = {a=1, b=2, c=3}
                table.foreach(t, function(k, v) sum = sum + v end)
                return sum
            "#).unwrap();
            assert_eq!(result, 6);
        });
    }

    #[test]
    fn test_math_mod() {
        with_engine(|engine| {
            let result: f64 = eval(engine, "return math.mod(10, 3)").unwrap();
            assert!((result - 1.0).abs() < f64::EPSILON);
        });
    }

    #[test]
    fn test_math_pow() {
        with_engine(|engine| {
            let result: f64 = eval(engine, "return math.pow(2, 10)").unwrap();
            assert!((result - 1024.0).abs() < f64::EPSILON);
        });
    }

    // ================================================================
    // 多行触发器
    // ================================================================

    #[test]
    fn test_multiline_trigger() {
        with_engine(|engine| {
            exec(engine, r#"
                ml_result = nil
                AddTrigger('ml_trig', 'line1.*line2', '', 33, 0, 0, '', 'function() ml_result = true end', 0, 0)
                SetTriggerOption('ml_trig', 'multi_line', true)
                SetTriggerOption('ml_trig', 'lines_to_match', 2)
            "#).unwrap();
            engine.process_output("line1");
            engine.process_output("line2");
            let result: Option<bool> = eval(engine, "return ml_result").unwrap();
            assert_eq!(result, Some(true));
        });
    }

    #[test]
    fn test_single_line_trigger_no_multiline() {
        with_engine(|engine| {
            exec(engine, r#"
                sl_result = nil
                AddTrigger('sl_trig', 'exact_match', '', 33, 0, 0, '', 'function() sl_result = true end', 0, 0)
            "#).unwrap();
            engine.process_output("exact_match");
            let result: Option<bool> = eval(engine, "return sl_result").unwrap();
            assert_eq!(result, Some(true));
        });
    }

    // ================================================================
    // SQLite3
    // ================================================================

    #[test]
    fn test_sqlite3_open_close() {
        with_engine(|engine| {
            let result: i64 = eval(engine, r#"
                local db = sqlite3.open("/tmp/test_rustluamud.db")
                db:exec("CREATE TABLE IF NOT EXISTS test (id INTEGER PRIMARY KEY, name TEXT)")
                db:close()
                return 0
            "#).unwrap();
            assert_eq!(result, 0);
        });
    }

    #[test]
    fn test_sqlite3_insert_query() {
        with_engine(|engine| {
            let result: i64 = eval(engine, r#"
                local db = sqlite3.open("/tmp/test_rustluamud2.db")
                db:exec("DROP TABLE IF EXISTS test")
                db:exec("CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT)")
                db:exec("INSERT INTO test (name) VALUES ('hello')")
                local stmt = db:prepare("SELECT name FROM test WHERE id = ?")
                local row = stmt:step({1})
                local name = row and row[1] or nil
                stmt = nil
                db:close()
                return name == 'hello' and 1 or 0
            "#).unwrap();
            assert_eq!(result, 1);
        });
    }

    #[test]
    fn test_database_close() {
        with_engine(|engine| {
            // DatabaseClose 是全局函数，不应报错
            exec(engine, "DatabaseClose('test_db')").unwrap();
        });
    }

    // ================================================================
    // 触发器 send_text
    // ================================================================

    #[test]
    fn test_trigger_send_text() {
        with_engine(|engine| {
            exec(engine, r#"
                AddTrigger('send_trig', 'go', '', 1, 0, 0, '', '', 0, 0)
                SetTriggerOption('send_trig', 'send', 'north')
            "#).unwrap();
            engine.process_output("go");
            let cmds = engine.drain_commands();
            assert!(cmds.contains(&"north".to_string()));
        });
    }

    // ================================================================
    // 原始 API 兼容
    // ================================================================

    #[test]
    fn test_original_trigger_api() {
        with_engine(|engine| {
            exec(engine, r#"
                orig_result = nil
                trigger([[^hello (\w+)$]], function(t) orig_result = t[1] end)
            "#).unwrap();
            engine.process_output("hello Rust");
            let result: Option<String> = eval(engine, "return orig_result").unwrap();
            assert_eq!(result, Some("Rust".to_string()));
        });
    }

    #[test]
    fn test_original_alias_api() {
        with_engine(|engine| {
            exec(engine, r#"
                orig_alias_result = nil
                alias('^go (.+)$', function(t) orig_alias_result = t[1] end)
            "#).unwrap();
            let matched = engine.process_input("go north");
            assert!(matched);
            let result: Option<String> = eval(engine, "return orig_alias_result").unwrap();
            assert_eq!(result, Some("north".to_string()));
        });
    }

    #[test]
    fn test_original_get_set_api() {
        with_engine(|engine| {
            exec(engine, "set('mykey', 'myval')").unwrap();
            let val: String = eval(engine, "return get('mykey')").unwrap();
            assert_eq!(val, "myval");
        });
    }

    // ================================================================
    // eval_code
    // ================================================================

    #[test]
    fn test_eval_code() {
        with_engine(|engine| {
            engine.eval_code("eval_result = 42").unwrap();
            let val: i64 = eval(engine, "return eval_result").unwrap();
            assert_eq!(val, 42);
        });
    }

    #[test]
    fn test_eval_code_error() {
        with_engine(|engine| {
            let result = engine.eval_code("invalid!!!lua");
            assert!(result.is_err());
        });
    }

    // ================================================================
    // regex_escape 辅助函数
    // ================================================================

    #[test]
    fn test_regex_escape() {
        assert_eq!(regex_escape("hello.world"), r"hello\.world");
        assert_eq!(regex_escape("a+b"), r"a\+b");
        assert_eq!(regex_escape("test*"), "test*"); // * 保留
        assert_eq!(regex_escape("test?"), "test?"); // ? 保留
        assert_eq!(regex_escape("(group)"), r"\(group\)");
        assert_eq!(regex_escape("a|b"), r"a\|b");
        assert_eq!(regex_escape("^start"), r"\^start");
        assert_eq!(regex_escape("end$"), r"end\$");
        assert_eq!(regex_escape("path\\file"), r"path\\file");
    }

    // ================================================================
    // drain_commands / drain_logs
    // ================================================================

    #[test]
    fn test_drain_commands_clears() {
        with_engine(|engine| {
            exec(engine, "send('cmd1')").unwrap();
            exec(engine, "send('cmd2')").unwrap();
            let cmds = engine.drain_commands();
            assert_eq!(cmds.len(), 2);
            let cmds2 = engine.drain_commands();
            assert!(cmds2.is_empty());
        });
    }

    #[test]
    fn test_drain_logs_clears() {
        with_engine(|engine| {
            exec(engine, "Note('log1')").unwrap();
            exec(engine, "Note('log2')").unwrap();
            let logs = engine.drain_logs();
            assert_eq!(logs.len(), 2);
            let logs2 = engine.drain_logs();
            assert!(logs2.is_empty());
        });
    }
}
