use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use mlua::{Lua, Result as LuaResult, Function, UserData, Table};
use regex::Regex;
use rusqlite::{Connection, types::Value};

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
            // Validate SQL by preparing it
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

            // Collect params
            let params_vec = if let Some(ref t) = args {
                let len = t.len().unwrap_or(0) as usize;
                let mut vals: Vec<Value> = Vec::with_capacity(len);
                for i in 1..=len {
                    let v: String = t.get(i).unwrap_or_default();
                    vals.push(Value::Text(v));
                }
                vals
            } else {
                Vec::new()
            };

            // Convert to rusqlite params
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
                let mut vals: Vec<Value> = Vec::with_capacity(len);
                for i in 1..=len {
                    let v: String = t.get(i).unwrap_or_default();
                    vals.push(Value::Text(v));
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
    pub pattern: Regex,
    pub callback: Function,
    pub enabled: bool,
}

/// 别名定义
pub struct Alias {
    pub pattern: Regex,
    pub callback: Function,
    pub enabled: bool,
}

/// 定时器定义
pub struct TimerDef {
    pub interval_secs: u64,
    pub callback: Function,
    pub enabled: bool,
}

/// 脚本运行时共享状态
struct ScriptState {
    triggers: Vec<Trigger>,
    aliases: Vec<Alias>,
    timers: Vec<TimerDef>,
    variables: HashMap<String, String>,
    pending_commands: Vec<String>,
    pending_logs: Vec<String>,
}

/// Lua 引擎与脚本运行时
pub struct LuaEngine {
    lua: Lua,
    state: Rc<RefCell<ScriptState>>,
    script_path: Option<String>,
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
        }));

        let mut engine = Self { lua, state, script_path: None };
        engine.register_api()?;
        Ok(engine)
    }

    /// 注册 Lua API
    fn register_api(&mut self) -> LuaResult<()> {
        let lua = &self.lua;
        let globals = lua.globals();

        // send(command)
        let state_rc = self.state.clone();
        let send_fn = lua.create_function_mut(move |_, cmd: String| {
            state_rc.borrow_mut().pending_commands.push(cmd);
            Ok(())
        })?;
        globals.set("send", send_fn)?;

        // log(message)
        let state_rc = self.state.clone();
        let log_fn = lua.create_function_mut(move |_, msg: String| {
            state_rc.borrow_mut().pending_logs.push(msg);
            Ok(())
        })?;
        globals.set("log", log_fn)?;

        // trigger(pattern, callback)
        let state_rc = self.state.clone();
        let trigger_fn = lua.create_function_mut(move |_, (pattern, callback): (String, Function)| {
            let re = Regex::new(&pattern)
                .map_err(|e| mlua::Error::external(format!("无效正则 '{}': {}", pattern, e)))?;
            state_rc.borrow_mut().triggers.push(Trigger {
                pattern: re,
                callback,
                enabled: true,
            });
            Ok(())
        })?;
        globals.set("trigger", trigger_fn)?;

        // alias(pattern, callback)
        let state_rc = self.state.clone();
        let alias_fn = lua.create_function_mut(move |_, (pattern, callback): (String, Function)| {
            let re = Regex::new(&pattern)
                .map_err(|e| mlua::Error::external(format!("无效正则 '{}': {}", pattern, e)))?;
            state_rc.borrow_mut().aliases.push(Alias {
                pattern: re,
                callback,
                enabled: true,
            });
            Ok(())
        })?;
        globals.set("alias", alias_fn)?;

        // timer(interval, callback)
        let state_rc = self.state.clone();
        let timer_fn = lua.create_function_mut(move |_, (interval_secs, callback): (u64, Function)| {
            state_rc.borrow_mut().timers.push(TimerDef {
                interval_secs,
                callback,
                enabled: true,
            });
            Ok(())
        })?;
        globals.set("timer", timer_fn)?;

        // get(key)
        let state_rc = self.state.clone();
        let get_fn = lua.create_function_mut(move |_, key: String| {
            let state = state_rc.borrow();
            Ok(state.variables.get(&key).cloned().unwrap_or_default())
        })?;
        globals.set("get", get_fn)?;

        // set(key, value)
        let state_rc = self.state.clone();
        let set_fn = lua.create_function_mut(move |_, (key, value): (String, String)| {
            state_rc.borrow_mut().variables.insert(key, value);
            Ok(())
        })?;
        globals.set("set", set_fn)?;

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
                // UTF-8 失败，尝试 GBK 解码（兼容 MushClient 的 GBK 脚本）
                let (cow, _, _) = encoding_rs::GBK.decode(&bytes);
                cow.into_owned()
            }
        };

        self.lua.load(&code)
            .set_name(path)
            .exec()
            .map_err(|e| format!("脚本执行错误 '{}': {}", path, e))?;

        self.script_path = Some(path.to_string());
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

        // 收集需要触发的（避免持有 RefCell 借用时调用 Lua）
        let matches: Vec<(usize, Vec<String>)> = {
            let state = self.state.borrow();
            let mut result = Vec::new();
            for (i, trigger) in state.triggers.iter().enumerate() {
                if !trigger.enabled { continue; }
                if let Some(caps) = trigger.pattern.captures(&clean_line) {
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

        // 逐个触发
        for (idx, caps_list) in matches {
            let callback = {
                let state = self.state.borrow();
                state.triggers[idx].callback.clone()
            };
            if let Ok(args_table) = self.lua.create_table() {
                for (i, m) in caps_list.iter().enumerate() {
                    let _ = args_table.set(i + 1, m.as_str());
                }
                let _ = callback.call::<()>(args_table);
            }
        }
    }

    /// 处理用户输入，匹配别名
    /// 返回 true 表示别名已处理（不再发送原始命令）
    pub fn process_input(&self, input: &str) -> bool {
        self.state.borrow_mut().pending_commands.clear();

        // 收集匹配的别名及其捕获组
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

        // 逐个触发（与触发器一致，传 matches table）
        for (idx, caps_list) in matches {
            let callback = {
                let state = self.state.borrow();
                state.aliases[idx].callback.clone()
            };
            if let Ok(args_table) = self.lua.create_table() {
                // matches[0] = 原始输入, matches[1] = 第一个捕获组, ...
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

        let callback = {
            let state = self.state.borrow();
            if index < state.timers.len() && state.timers[index].enabled {
                state.timers[index].callback.clone()
            } else {
                return;
            }
        };

        let _ = callback.call::<()>(());
    }

    /// 取出待发送的命令（由 send() 产生）
    pub fn drain_commands(&self) -> Vec<String> {
        self.state.borrow_mut().pending_commands.drain(..).collect()
    }

    /// 设置 Lua 变量（用于注入 profile 凭证等）
    pub fn set_variable(&mut self, key: &str, value: &str) {
        self.state.borrow_mut().variables.insert(key.to_string(), value.to_string());
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
