//! Lua 引擎主模块
//!
//! `LuaEngine` 的核心实现：构造、析构、脚本加载、错误日志、看门狗。
//!
//! 其他功能分散在子模块中：
//! - [`super::api`][]: MushClient 兼容 API 注册
//! - [`super::triggers`][]: 触发器匹配
//! - [`super::aliases`][]: 别名匹配
//! - [`super::timers`][]: 定时器调度
//! - [`super::commands`][]: 命令队列与状态访问
//! - [`super::types`][]: 核心类型定义
//! - [`super::helpers`][]: 辅助函数
//! - [`super::database`][]: SQLite 封装

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mlua::{Lua, Result as LuaResult};

use super::helpers::fix_lua_escape_sequences;
use super::types::{LuaEngine, ScriptEncoding, ScriptState};

impl LuaEngine {
    /// 创建新的 Lua 引擎实例
    pub fn new() -> LuaResult<Self> {
        let lua = Lua::new();

        let state = Rc::new(RefCell::new(ScriptState {
            triggers: Vec::new(),
            aliases: Vec::new(),
            timers: Vec::new(),
            trigger_by_name: HashMap::new(),
            trigger_groups: HashMap::new(),
            alias_by_name: HashMap::new(),
            alias_groups: HashMap::new(),
            timer_by_name: HashMap::new(),
            timer_groups: HashMap::new(),
            variables: HashMap::new(),
            pending_commands: Vec::new(),
            pending_raw: Vec::new(),
            pending_logs: Vec::new(),
            tell_buffer: String::new(),
            recent_lines: Vec::new(),
            unique_counter: 0,
            connected: false,
            connect_requested: false,
            disconnect_requested: false,
            host: String::new(),
            port: 0,
            world_name: String::new(),
            char_name: String::new(),
            packet_count: 0,
            status_text: String::new(),
            current_encoding: ScriptEncoding::Utf8,
            last_server_data: std::time::Instant::now(),
            last_keepalive: std::time::Instant::now(),
            pending_panels: Vec::new(),
            panel_handlers: HashMap::new(),
            connect_time: None,
            bytes_recv: 0,
            bytes_sent: 0,
            reconnect_count: 0,
            reconnect_attempt: 0,
            reconnecting: false,
            last_disconnect_reason: None,
            next_retry_secs: 0,
        }));

        let script_dir = Rc::new(RefCell::new(None::<String>));
        let script_path = Rc::new(RefCell::new(None::<String>));
        let log_dir = Rc::new(RefCell::new(None::<String>));

        // 创建看门狗共享状态
        let exec_start = Arc::new(AtomicU64::new(0));
        let exec_timer_name = Arc::new(Mutex::new(None::<String>));
        let watchdog_stop = Arc::new(AtomicBool::new(false));

        let mut engine = Self {
            lua,
            state,
            script_path,
            script_dir,
            log_dir,
            pending_on_connect: None,
            connect_delay_ms: 0,
            delayed_commands: std::cell::RefCell::new(Vec::new()),
            exec_start: exec_start.clone(),
            exec_timer_name: exec_timer_name.clone(),
            watchdog_stop: watchdog_stop.clone(),
            watchdog_handle: None,
        };

        // 启动看门狗线程
        engine.watchdog_handle = Some(Self::spawn_watchdog(
            exec_start,
            exec_timer_name,
            watchdog_stop,
            30,
        ));

        engine.register_api()?;
        Ok(engine)
    }

    /// 设置脚本路径（同时提取目录）
    pub fn set_script_path(&mut self, path: &str) {
        // 同时支持 / 和 \ 分隔符，兼容 Linux 和 Windows
        let pos = path.rfind(['/', '\\']);
        if let Some(p) = pos {
            *self.script_dir.borrow_mut() = Some(path[..p + 1].to_string());
        } else {
            *self.script_dir.borrow_mut() = Some("./".to_string());
        }
        *self.script_path.borrow_mut() = Some(path.to_string());
    }

    /// 设置日志目录（供 GetInfo(58) 返回）
    pub fn set_log_dir(&mut self, path: &str) {
        // 规范化：确保末尾带平台原生路径分隔符
        let sep = if cfg!(windows) { "\\" } else { "/" };
        let normalized = if path.ends_with('/') || path.ends_with('\\') {
            path.to_string()
        } else {
            format!("{}{}", path, sep)
        };
        *self.log_dir.borrow_mut() = Some(normalized);
    }

    /// 直接执行 Lua 代码（用于 /eval 命令）
    pub fn eval_code(&self, code: &str) -> Result<(), String> {
        self.lua.load(code).exec().map_err(|e| format!("{}", e))
    }

    /// 执行 Lua 代码并返回字符串结果
    #[allow(dead_code)]
    pub fn eval_to_string(&self, code: &str) -> Result<String, String> {
        self.lua
            .load(code)
            .eval::<String>()
            .map_err(|e| format!("{}", e))
    }

    /// 加载并执行 Lua 脚本文件
    /// 自动检测编码：先尝试 UTF-8，失败（GBK 编码）则自动转码
    pub fn load_script(&mut self, path: &str) -> Result<(), String> {
        // 先设置脚本路径，确保脚本执行时 GetInfo(35) 能返回正确目录
        self.set_script_path(path);

        let bytes = std::fs::read(path).map_err(|e| format!("读取脚本失败 '{}': {}", path, e))?;

        // 先尝试 UTF-8 解码
        let (code, encoding) = match std::str::from_utf8(&bytes) {
            Ok(s) => (s.to_string(), ScriptEncoding::Utf8),
            Err(_) => {
                // UTF-8 解码失败，尝试 GBK
                let (cow, _, had_errors) = encoding_rs::GBK.decode(&bytes);
                if had_errors {
                    return Err(format!("脚本 '{}' 既不是有效 UTF-8 也不是有效 GBK", path));
                }
                (cow.into_owned(), ScriptEncoding::Gbk)
            }
        };

        // 记录当前脚本编码，供 AddTrigger 决定正则匹配模式
        self.state.borrow_mut().current_encoding = encoding;

        // 预处理 Lua 源码，修复 LuaJIT 不兼容的无效转义序列
        let code = fix_lua_escape_sequences(&code);

        // 执行脚本
        self.lua
            .load(&code)
            .set_name(path)
            .exec()
            .map_err(|e| format!("脚本 '{}' 执行错误: {}", path, e))?;

        Ok(())
    }

    /// 获取当前脚本路径
    pub fn script_path(&self) -> Option<String> {
        self.script_path.borrow().clone()
    }

    /// 记录错误信息到 stderr 和日志文件
    pub(super) fn log_error(&self, msg: &str) {
        eprintln!("{}", msg);
        // 使用 try_borrow_mut 避免在 RefCell 已被借用时 panic
        if let Ok(mut state) = self.state.try_borrow_mut() {
            state
                .pending_logs
                .push(format!("[Lua] {}", crate::ui::AnsiParser::strip_ansi(msg)));
        }
    }

    /// 启动看门狗线程，监控 Lua exec() 执行是否超时
    fn spawn_watchdog(
        exec_start: Arc<AtomicU64>,
        exec_timer_name: Arc<Mutex<Option<String>>>,
        watchdog_stop: Arc<AtomicBool>,
        timeout_secs: u64,
    ) -> thread::JoinHandle<()> {
        thread::Builder::new()
            .name("lua-watchdog".to_string())
            .spawn(move || {
                loop {
                    // 分段睡眠：每 100ms 检查停止信号，确保 Drop 时最多等 100ms 而非 5s
                    for _ in 0..50 {
                        if watchdog_stop.load(Ordering::Relaxed) {
                            return;
                        }
                        thread::sleep(Duration::from_millis(100));
                    }

                    let start_ns = exec_start.load(Ordering::Relaxed);
                    if start_ns == 0 {
                        continue; // 当前无 Lua 执行
                    }

                    let now_ns = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos() as u64;

                    let elapsed_secs = (now_ns.saturating_sub(start_ns)) / 1_000_000_000;
                    if elapsed_secs < timeout_secs {
                        continue;
                    }

                    // 超时！获取诊断信息
                    let timer = exec_timer_name
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .clone()
                        .unwrap_or_else(|| "<unknown>".to_string());

                    let panic_msg = format!(
                        "Lua execution watchdog timeout - timer '{}' exceeded {}s, forcing abort",
                        timer, timeout_secs
                    );
                    eprintln!("{}", panic_msg);

                    // 尝试通过全局 PANIC_CONTEXT 写入日志
                    if let Some(ctx) = crate::log::panic_hook::get_context() {
                        let session = ctx
                            .session_name
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .clone();
                        let session = if session.is_empty() {
                            "watchdog"
                        } else {
                            &session
                        };
                        let backtrace = std::backtrace::Backtrace::capture();
                        ctx.logger
                            .log_panic(session, &panic_msg, &format!("{}", backtrace));
                    }

                    // 强制终止进程（确保日志刷新）
                    std::process::abort();
                }
            })
            .expect("failed to spawn watchdog thread")
    }
}

/// 确保 LuaEngine 析构时优雅停止看门狗线程
impl Drop for LuaEngine {
    fn drop(&mut self) {
        self.watchdog_stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.watchdog_handle.take() {
            let _ = handle.join();
        }
    }
}
