use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use mlua::{Lua, Result as LuaResult, Function};
use regex::Regex;

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

        Ok(())
    }

    /// 加载并执行 Lua 脚本文件
    pub fn load_script(&mut self, path: &str) -> Result<(), String> {
        let code = std::fs::read_to_string(path)
            .map_err(|e| format!("读取脚本失败 '{}': {}", path, e))?;

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

        // 收集匹配的别名
        let matched: Vec<usize> = {
            let state = self.state.borrow();
            state.aliases.iter().enumerate()
                .filter(|(_, a)| a.enabled && a.pattern.is_match(input))
                .map(|(i, _)| i)
                .collect()
        };

        if matched.is_empty() {
            return false;
        }

        for idx in matched {
            let callback = {
                let state = self.state.borrow();
                state.aliases[idx].callback.clone()
            };
            let _ = callback.call::<()>(input);
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
