use std::fs;
use std::io::{self, Write};
use std::path::Path;

use crossterm::event::{
    Event as CrosstermEvent, EventStream, KeyCode, KeyModifiers, MouseButton, MouseEventKind,
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::config::AppConfig;
use crate::connection::{ConnectionManager, ManagerEvent, SessionState};
use crate::log::Logger;
use crate::ui::{AnsiParser, Terminal};

/// 终端设置持久化
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TermSettings {
    /// 是否在 Enter 后保留命令栏输入内容
    keep_command: bool,
}

impl TermSettings {
    fn path() -> &'static str {
        "profiles/terminal.json"
    }

    fn load() -> Self {
        let path = Self::path();
        if Path::new(path).exists() {
            fs::read_to_string(path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            Self::default()
        }
    }

    fn save(&self) {
        let path = Self::path();
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path, json);
        }
    }
}

impl Default for TermSettings {
    fn default() -> Self {
        Self { keep_command: true }
    }
}

/// 内置命令解析结果
#[derive(Debug, PartialEq)]
enum BuiltinCommand {
    /// /connect <名称> <主机> <端口>
    Connect {
        name: String,
        host: String,
        port: u16,
    },
    /// /disconnect [编号]
    Disconnect { id: Option<usize> },
    /// /close [编号]
    Close { id: Option<usize> },
    /// /list
    List,
    /// /load <脚本路径>
    Load { path: String },
    /// /load reload
    LoadReload,
    /// /lua <代码>
    Lua { code: String },
    /// /set <选项> <值>
    Set { option: String, value: String },
    /// /switch <角色名或编号>
    Switch { target: String },
    /// /profile load <角色名> | /profile list
    Profile { sub: ProfileSubcommand },
    /// /all <命令> — 发送命令到所有连接
    All { cmd: String },
    /// 未知命令
    Unknown,
}

/// /profile 子命令
#[derive(Debug, PartialEq)]
enum ProfileSubcommand {
    /// /profile load <角色名> — 从 profiles/ 加载角色配置并连接
    Load { name: String },
    /// /profile list — 列出 profiles/ 下可用角色
    List,
}

/// 解析内置命令（纯逻辑，无 IO 依赖）
fn parse_builtin_command(cmd: &str) -> BuiltinCommand {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        return BuiltinCommand::Unknown;
    }

    match parts[0] {
        "/connect" => {
            if let Some((host, port)) = parse_connect_args(&parts) {
                let name = parts[1].to_string();
                BuiltinCommand::Connect { name, host, port }
            } else {
                BuiltinCommand::Unknown
            }
        }
        "/disconnect" => {
            let id = parts.get(1).and_then(|s| s.parse::<usize>().ok());
            BuiltinCommand::Disconnect { id }
        }
        "/close" => {
            let id = parts.get(1).and_then(|s| s.parse::<usize>().ok());
            BuiltinCommand::Close { id }
        }
        "/list" => BuiltinCommand::List,
        "/reload" => BuiltinCommand::LoadReload,
        "/load" => {
            if parts.len() < 2 {
                return BuiltinCommand::Unknown;
            }
            if parts[1] == "reload" {
                BuiltinCommand::LoadReload
            } else {
                BuiltinCommand::Load {
                    path: parts[1].to_string(),
                }
            }
        }
        "/lua" => {
            let code = cmd.strip_prefix("/lua ").unwrap_or("").to_string();
            if code.is_empty() {
                BuiltinCommand::Unknown
            } else {
                BuiltinCommand::Lua { code }
            }
        }
        "/set" => {
            if parts.len() < 3 {
                BuiltinCommand::Unknown
            } else {
                BuiltinCommand::Set {
                    option: parts[1].to_string(),
                    value: parts[2].to_string(),
                }
            }
        }
        "/switch" | "/sw" => {
            if parts.len() < 2 {
                BuiltinCommand::Unknown
            } else {
                BuiltinCommand::Switch {
                    target: parts[1].to_string(),
                }
            }
        }
        "/all" => {
            let rest = cmd.strip_prefix("/all ").unwrap_or("").to_string();
            if rest.is_empty() {
                BuiltinCommand::Unknown
            } else {
                BuiltinCommand::All { cmd: rest }
            }
        }
        "/profile" => {
            if parts.len() < 2 {
                BuiltinCommand::Unknown
            } else {
                match parts[1] {
                    "load" => {
                        if parts.len() < 3 {
                            BuiltinCommand::Unknown
                        } else {
                            BuiltinCommand::Profile {
                                sub: ProfileSubcommand::Load {
                                    name: parts[2].to_string(),
                                },
                            }
                        }
                    }
                    "list" => BuiltinCommand::Profile {
                        sub: ProfileSubcommand::List,
                    },
                    _ => BuiltinCommand::Unknown,
                }
            }
        }
        _ => BuiltinCommand::Unknown,
    }
}

/// 解析分号分隔的命令，支持转义（\; 表示字面量分号）
fn split_commands(cmd: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut chars = cmd.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            // 转义字符：检查下一个字符
            if let Some(&next) = chars.peek() {
                if next == ';' {
                    // \; 表示字面量分号
                    current.push(';');
                    chars.next();
                } else {
                    // 其他情况保留反斜杠
                    current.push('\\');
                }
            } else {
                // 字符串末尾的反斜杠
                current.push('\\');
            }
        } else if c == ';' {
            // 分号：结束当前命令
            let trimmed = current.trim();
            if !trimmed.is_empty() {
                result.push(trimmed.to_string());
            }
            current.clear();
        } else {
            current.push(c);
        }
    }

    // 处理最后一个命令
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        result.push(trimmed.to_string());
    }

    result
}

/// 重连请求
struct ReconnectRequest {
    session_id: usize,
}

/// 动态连接请求
struct ConnectRequest {
    session_id: usize,
}

/// 定时器触发请求
struct TimerRequest {
    session_id: usize,
}

/// 解析 /connect 命令参数，返回 (host, port)
fn parse_connect_args(parts: &[&str]) -> Option<(String, u16)> {
    if parts.len() < 3 {
        return None;
    }
    let (host, port) = if parts[2].contains(':') && parts.len() == 3 {
        let hp: Vec<&str> = parts[2].splitn(2, ':').collect();
        (hp[0], hp[1].parse::<u16>().unwrap_or(5555))
    } else {
        let p = if parts.len() > 3 {
            parts[3].parse::<u16>().ok()
        } else {
            None
        };
        (parts[2], p.unwrap_or(5555))
    };
    Some((host.to_string(), port))
}

/// 格式化 Lua 错误信息，将含路径的文本分行
fn format_lua_error(err: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for line in err.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("stack traceback:") {
            lines.push("stack traceback:".to_string());
        } else if !trimmed.is_empty() {
            lines.push(trimmed.to_string());
        }
    }
    if lines.is_empty() {
        lines.push(err.to_string());
    }
    lines
}

/// 应用主结构
pub struct App {
    config: AppConfig,
    terminal: Terminal,
    manager: ConnectionManager,
    logger: Logger,
    running: bool,
    reconnect_tx: mpsc::Sender<ReconnectRequest>,
    reconnect_rx: mpsc::Receiver<ReconnectRequest>,
    connect_tx: mpsc::Sender<ConnectRequest>,
    connect_rx: mpsc::Receiver<ConnectRequest>,
    timer_tx: mpsc::Sender<TimerRequest>,
    timer_rx: mpsc::Receiver<TimerRequest>,
}

impl App {
    pub fn new(config: AppConfig) -> io::Result<Self> {
        let mut manager = ConnectionManager::new();

        let logger = Logger::new(
            &config.general.log_dir,
            config.general.log_rotation_size_mb,
            config.general.log_rotation_count,
        );

        // 加载配置文件中的连接，并设置各角色的日志保留数量
        for conn_config in &config.connections {
            if let Err(e) = manager.add_connection(conn_config) {
                eprintln!("警告: {}", e);
            }
            if let Some(count) = conn_config.log_rotation_count {
                logger.set_session_max_files(&conn_config.name, count);
            }
        }

        let mut terminal = Terminal::new()?;

        // 加载并应用终端设置
        let ts = TermSettings::load();
        terminal.state_mut().keep_command = ts.keep_command;

        let (reconnect_tx, reconnect_rx) = mpsc::channel(32);
        let (connect_tx, connect_rx) = mpsc::channel(16);
        let (timer_tx, timer_rx) = mpsc::channel(64);

        Ok(Self {
            config,
            terminal,
            manager,
            logger,
            running: true,
            reconnect_tx,
            reconnect_rx,
            connect_tx,
            connect_rx,
            timer_tx,
            timer_rx,
        })
    }

    /// 启动应用主循环
    pub async fn run(&mut self) -> io::Result<()> {
        self.terminal.init_screen()?;

        // 自动连接所有 auto_connect 的连接（包括从 profile 加载的）
        let auto_connect_ids: Vec<usize> = (0..self.manager.sessions.len())
            .filter(|&id| self.manager.sessions[id].auto_connect)
            .collect();
        for id in auto_connect_ids {
            let name = self.manager.sessions[id].name.clone();
            match self.manager.connect_session(id).await {
                Ok(()) => {
                    let msg = format!("[系统] 连接 {} ({}) 已建立", id + 1, name);
                    self.terminal.append_output(&msg)?;
                    self.init_lua_for_session(id)?;
                }
                Err(e) => {
                    let msg = format!("[系统] 连接 {} ({}) 失败: {}", id + 1, name, e);
                    self.terminal.append_output(&msg)?;
                }
            }
        }

        self.update_status_bar()?;

        // 获取管理器事件接收器
        let mut mgr_rx = self
            .manager
            .take_event_rx()
            .ok_or_else(|| io::Error::other("无法获取事件通道"))?;

        // 终端事件流
        let mut term_events = EventStream::new();

        // 主事件循环
        while self.running {
            tokio::select! {
                // 处理终端键盘事件
                Some(Ok(event)) = term_events.next() => {
                    match event {
                        CrosstermEvent::Key(key) => {
                            self.handle_key_event(key)?;
                        }
                        CrosstermEvent::Mouse(mouse) => {
                            self.handle_mouse_event(mouse)?;
                        }
                        CrosstermEvent::Resize(w, h) => {
                            self.terminal.resize(w, h);
                            self.update_status_bar()?;
                        }
                        _ => {}
                    }
                }

                // 处理连接事件
                Some(event) = mgr_rx.recv() => {
                    self.handle_manager_event(event)?;
                }

                // 处理重连请求
                Some(req) = self.reconnect_rx.recv() => {
                    self.perform_reconnect(req.session_id).await?;
                }

                // 处理动态连接请求
                Some(req) = self.connect_rx.recv() => {
                    self.perform_connect(req.session_id).await?;
                }

                // 处理定时器触发（轮询到达）
                Some(req) = self.timer_rx.recv() => {
                    self.handle_timer(req.session_id)?;
                }
            }
        }

        Ok(())
    }

    /// 执行重连
    async fn perform_reconnect(&mut self, session_id: usize) -> io::Result<()> {
        let name = if session_id < self.manager.sessions.len() {
            self.manager.sessions[session_id].name.clone()
        } else {
            "未知".to_string()
        };

        match self.manager.connect_session(session_id).await {
            Ok(()) => {
                let msg = format!("[系统] 连接 {} ({}) 重连成功", session_id + 1, name);
                self.terminal.append_output(&msg)?;
                // 如果 Lua 引擎已存在（重连前已加载脚本），不重建引擎，
                // 保留 Lua 变量状态（stat.* 等统计数据）。仅通知引擎已连接以触发 OnConnect。
                // 若引擎不存在（首次连接或未加载脚本），则执行标准初始化流程。
                if session_id < self.manager.sessions.len()
                    && self.manager.sessions[session_id].lua_engine.is_some()
                {
                    // 先排空 OnConnect() 产生的命令和日志，再发送
                    let (queued_cmds, queued_logs) = {
                        let engine = self.manager.sessions[session_id]
                            .lua_engine
                            .as_mut()
                            .unwrap();
                        engine.set_connected(true);
                        (engine.drain_commands(), engine.drain_logs())
                    };
                    for cmd in &queued_cmds {
                        self.logger.log_command(&name, cmd);
                        if let Err(e) = self.manager.send_to(session_id, cmd) {
                            self.terminal.append_output(&format!("[发送错误] {}", e))?;
                        }
                    }
                    for msg in &queued_logs {
                        self.terminal.append_output(msg)?;
                    }
                } else {
                    self.init_lua_for_session(session_id)?;
                }
                // 重连后刷新状态栏（Lua 脚本可能调用了 SetStatus）
                if session_id == self.manager.foreground_id {
                    self.update_status_bar()?;
                }
            }
            Err(e) => {
                let msg = format!("[系统] 重连 {} ({}) 失败: {}", session_id + 1, name, e);
                self.terminal.append_output(&msg)?;
            }
        }
        Ok(())
    }

    /// 执行动态连接
    async fn perform_connect(&mut self, session_id: usize) -> io::Result<()> {
        if session_id >= self.manager.sessions.len() {
            self.terminal.append_output("[错误] 无效的连接 ID")?;
            return Ok(());
        }
        let name = self.manager.sessions[session_id].name.clone();
        let host = self.manager.sessions[session_id].host.clone();
        let port = self.manager.sessions[session_id].port;

        match self.manager.connect_session(session_id).await {
            Ok(()) => {
                let msg = format!(
                    "[系统] 连接 {} ({}) → {}:{} 已建立",
                    session_id + 1,
                    name,
                    host,
                    port
                );
                self.terminal.append_output(&msg)?;
                self.init_lua_for_session(session_id)?;
                // 自动切换到新连接
                self.switch_foreground(session_id)?;
            }
            Err(e) => {
                let msg = format!("[系统] 连接失败 ({}:{}): {}", host, port, e);
                self.terminal.append_output(&msg)?;
            }
        }
        Ok(())
    }

    /// 为指定连接初始化 Lua 引擎并加载脚本
    fn init_lua_for_session(&mut self, id: usize) -> io::Result<()> {
        if id >= self.manager.sessions.len() {
            return Ok(());
        }

        // 从 Session 自身获取配置
        let name = self.manager.sessions[id].name.clone();
        let script_path = self.manager.sessions[id].script_path.clone();
        let username = self.manager.sessions[id].username.clone();
        let password = self.manager.sessions[id].password.clone();
        let host = self.manager.sessions[id].host.clone();

        match crate::lua::LuaEngine::new() {
            Ok(mut engine) => {
                // 注入主机地址（供 GetInfo(1) 返回）
                engine.set_host(&host);
                // 注入世界名称（供 GetInfo(2) 返回）
                engine.set_world_name(&self.manager.sessions[id].name);
                // 注入日志目录（供 GetInfo(58) 返回）
                engine.set_log_dir(&self.config.general.log_dir);

                // 注入登录凭证到 Lua 变量和全局变量
                if let Some(ref name) = username {
                    if !name.is_empty() {
                        engine.set_variable("char_name", name);
                        engine.set_global("char_name", name);
                        engine.set_char_name(name); // 供 GetInfo(3) 返回
                    }
                }
                if let Some(ref pwd) = password {
                    if !pwd.is_empty() {
                        engine.set_variable("char_password", pwd);
                        engine.set_global("char_password", pwd);
                    }
                }

                // 加载脚本
                if let Some(ref path) = script_path {
                    match engine.load_script(path) {
                        Ok(()) => {
                            // 排空脚本加载期间 Execute 等压入的命令（如 "/set_dl()"、"score" 等）
                            let queued_cmds = engine.drain_commands();
                            for cmd in &queued_cmds {
                                if let Some(lua_code) = cmd.strip_prefix('/') {
                                    if let Err(e) = engine.eval_code(lua_code) {
                                        self.terminal.append_output(&format!(
                                            "[Lua] 执行排队命令失败: {}",
                                            e
                                        ))?;
                                    }
                                } else {
                                    self.logger.log_command(&name, cmd);
                                    if let Err(e) = self.manager.send_to(id, cmd) {
                                        self.terminal
                                            .append_output(&format!("[发送错误] {}", e))?;
                                    }
                                }
                            }

                            // 排空并显示脚本加载期间的 Lua 日志
                            let name = self.manager.sessions[id].name.clone();
                            let logs = engine.drain_logs();
                            for msg in logs {
                                let clean = crate::ui::AnsiParser::strip_ansi(&msg);
                                self.logger.log(&name, &clean);
                                self.terminal.append_output(&msg)?;
                            }

                            let msg = format!("[Lua] 连接 {} 脚本已加载: {}", id + 1, path);
                            self.terminal.append_output(&msg)?;
                        }
                        Err(e) => {
                            let err_msg = e.to_string();
                            for line in format_lua_error(&err_msg) {
                                self.terminal.append_output(&line)?;
                            }
                            // 脚本加载错误也写入日志
                            let name = self.manager.sessions[id].name.clone();
                            for line in format_lua_error(&err_msg) {
                                self.logger.log_debug(&name, &line);
                            }
                        }
                    }
                }

                self.manager.sessions[id].lua_engine = Some(engine);
                // 同步连接状态：session.connect() 在创建事件通道前已设置 state，
                // 初始 Connected 状态不会通过 StateChange 事件到达 engine，
                // 此处手动同步，确保 engine 知道当前已连接并触发 alias.atconnect()
                if id < self.manager.sessions.len() {
                    let is_connected = matches!(
                        self.manager.sessions[id].state,
                        crate::connection::SessionState::Connected
                    );
                    if is_connected {
                        let (queued_cmds, queued_logs) = {
                            match self.manager.sessions[id].lua_engine.as_mut() {
                                Some(eng) => {
                                    eng.set_connected(true);
                                    (eng.drain_commands(), eng.drain_logs())
                                }
                                None => (Vec::new(), Vec::new()),
                            }
                        };
                        for cmd in &queued_cmds {
                            self.logger.log_command(&name, cmd);
                            if let Err(e) = self.manager.send_to(id, cmd) {
                                self.terminal.append_output(&format!("[发送错误] {}", e))?;
                            }
                        }
                        for msg in &queued_logs {
                            self.terminal.append_output(msg)?;
                        }
                    } else {
                        // session 尚未连接，无需同步
                    }
                }
                // 启动定时器
                self.start_timers_for_session(id);
            }
            Err(e) => {
                let msg = format!("[Lua] 连接 {} 引擎初始化失败: {}", id + 1, e);
                self.terminal.append_output(&msg)?;
            }
        }
        Ok(())
    }

    /// 为指定连接启动定时器任务
    fn start_timers_for_session(&mut self, id: usize) {
        if id >= self.manager.sessions.len() {
            return;
        }

        // 使用轮询方式：单个 tokio 任务定期检查所有定时器
        // 这解决了动态创建的定时器（如 wait.time 创建的）无法触发的问题
        let timer_tx = self.timer_tx.clone();
        tokio::spawn(async move {
            // 轮询间隔 50ms，确保定时器精度
            let poll_interval = tokio::time::Duration::from_millis(50);
            loop {
                tokio::time::sleep(poll_interval).await;
                if timer_tx
                    .send(TimerRequest { session_id: id })
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
    }

    /// 发送 Lua 引擎产生的命令，拦截 / 开头的命令作为 Lua 代码执行
    fn send_lua_commands(&mut self, session_id: usize, commands: Vec<String>) -> io::Result<()> {
        let name = if session_id < self.manager.sessions.len() {
            self.manager.sessions[session_id].name.clone()
        } else {
            return Ok(());
        };
        // 使用队列处理命令，别名匹配可能产生新命令需要继续处理
        // 先按 ; 拆分，保证带多个分号分隔的命令被逐条处理
        let mut queue: std::collections::VecDeque<String> = commands
            .into_iter()
            .flat_map(|cmd| {
                if cmd.contains(';') {
                    split_commands(&cmd)
                } else {
                    vec![cmd]
                }
            })
            .collect();
        // 防止无限递归：限制别名匹配的嵌套深度
        let mut depth = 0;
        let max_depth = 10;

        while let Some(cmd) = queue.pop_front() {
            if let Some(lua_code) = cmd.strip_prefix('/') {
                // / 开头的命令作为 Lua 代码执行
                // 去掉前导 /
                self.logger.log_lua(&name, lua_code);
                if let Some(ref engine) = self.manager.sessions[session_id].lua_engine {
                    match engine.eval_code(lua_code) {
                        Ok(_) => {
                            let sub_commands = engine.drain_commands();
                            for sub_cmd in sub_commands {
                                self.logger.log_command(&name, &sub_cmd);
                                if let Err(e) = self.manager.send_to(session_id, &sub_cmd) {
                                    self.terminal.append_output(&format!("[Lua 错误] {}", e))?;
                                }
                            }
                            self.drain_lua_logs(session_id)?;
                        }
                        Err(e) => {
                            self.terminal.append_output(&format!("[Lua 错误] {}", e))?;
                        }
                    }
                }
            } else if depth < max_depth {
                // 非 / 开头的命令：先尝试别名匹配（与 MUSHclient Execute 行为一致）
                let alias_handled =
                    if let Some(ref engine) = self.manager.sessions[session_id].lua_engine {
                        let handled = engine.process_input(&cmd);
                        if handled {
                            let sub_commands = engine.drain_commands();
                            if !sub_commands.is_empty() {
                                // 别名匹配成功，产生的命令加入队列继续处理
                                for sub_cmd in sub_commands {
                                    queue.push_front(sub_cmd);
                                }
                            }
                            self.drain_lua_logs(session_id)?;
                        }
                        handled
                    } else {
                        false
                    };

                if !alias_handled {
                    // 无别名匹配，直接发送到 MUD
                    self.logger.log_command(&name, &cmd);
                    if let Err(e) = self.manager.send_to(session_id, &cmd) {
                        self.terminal.append_output(&format!("[发送错误] {}", e))?;
                    }
                }
                depth += 1;
            } else {
                // 超过嵌套深度，直接发送防止无限递归
                self.logger.log_command(&name, &cmd);
                if let Err(e) = self.manager.send_to(session_id, &cmd) {
                    self.terminal.append_output(&format!("[发送错误] {}", e))?;
                }
            }
        }
        Ok(())
    }

    /// 发送 Lua 引擎产生的原始数据包（SendPkt 压入的）
    fn send_lua_raw(&mut self, session_id: usize) -> io::Result<()> {
        if session_id >= self.manager.sessions.len() {
            return Ok(());
        }
        let raw_packets = self.manager.sessions[session_id]
            .lua_engine
            .as_ref()
            .map(|engine| engine.drain_raw())
            .unwrap_or_default();
        for data in raw_packets {
            if let Err(e) = self.manager.send_raw(session_id, data) {
                self.terminal
                    .append_output(&format!("[发送原始数据错误] {}", e))?;
            }
        }
        Ok(())
    }

    /// 处理定时器触发（轮询模式：检查所有到期定时器）
    fn handle_timer(&mut self, session_id: usize) -> io::Result<()> {
        if session_id >= self.manager.sessions.len() {
            return Ok(());
        }
        let mut any_fired = false;
        loop {
            // 先检查是否有到期的定时器，确保 engine 引用在调用 self 方法前被释放
            let should_fire = self.manager.sessions[session_id]
                .lua_engine
                .as_ref()
                .map(|engine| engine.fire_next_due_timer())
                .unwrap_or(false);
            if !should_fire {
                break;
            }
            any_fired = true;
            let commands = self.manager.sessions[session_id]
                .lua_engine
                .as_ref()
                .map(|engine| engine.drain_commands())
                .unwrap_or_default();
            if !commands.is_empty() {
                self.send_lua_commands(session_id, commands)?;
            }
            // 处理 SendPkt 压入的原始数据包
            self.send_lua_raw(session_id)?;
            self.drain_lua_logs(session_id)?;
        }
        // 空闲心跳检测：服务器静默超过 30 秒时发送 IAC NOP
        if let Some(ref engine) = self.manager.sessions[session_id].lua_engine {
            engine.fire_keepalive_if_idle();
        }
        self.send_lua_raw(session_id)?;
        // 仅在定时器真正触发时才刷新状态栏（避免每 50ms 写终端，破坏鼠标选中）
        if any_fired && session_id == self.manager.foreground_id {
            self.update_status_bar()?;
        }
        Ok(())
    }

    /// 处理 Lua 引擎产生的日志
    fn drain_lua_logs(&mut self, session_id: usize) -> io::Result<()> {
        if session_id >= self.manager.sessions.len() {
            return Ok(());
        }
        let logs = if let Some(ref engine) = self.manager.sessions[session_id].lua_engine {
            engine.drain_logs()
        } else {
            Vec::new()
        };
        let name = self.manager.sessions[session_id].name.clone();
        for msg in logs {
            // 日志写入文件（剥离 ANSI 码），根据前缀分类
            let clean = crate::ui::AnsiParser::strip_ansi(&msg);
            if clean.starts_with("[GPS-MATCH]")
                || clean.starts_with("[GPS]")
                || clean.starts_with("[DEBUG")
            {
                self.logger.log_debug(&name, &clean);
            } else {
                self.logger.log(&name, &clean);
            }
            // 如果是前台连接，也在终端显示（保留 ANSI 码以显示颜色）
            if session_id == self.manager.foreground_id {
                self.terminal
                    .append_output(&format!("\x1b[36m[Lua] {}\x1b[0m", msg))?;
            }
        }
        Ok(())
    }

    /// 处理鼠标事件
    fn handle_mouse_event(&mut self, mouse: crossterm::event::MouseEvent) -> io::Result<()> {
        // 只在状态栏行（y=0）响应鼠标点击
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) && mouse.row == 0 {
            let x = mouse.column;
            for region in self.terminal.click_regions() {
                if x >= region.start_x && x < region.end_x {
                    if region.session_id < self.manager.sessions.len() {
                        self.switch_foreground(region.session_id)?;
                    }
                    break;
                }
            }
        }
        Ok(())
    }

    /// 处理键盘事件
    fn handle_key_event(&mut self, key: crossterm::event::KeyEvent) -> io::Result<()> {
        // Ctrl+C / Ctrl+D: 退出
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && (key.code == KeyCode::Char('c') || key.code == KeyCode::Char('d'))
        {
            self.running = false;
            return Ok(());
        }

        // Alt+1~9: 切换到第 1~9 个连接, Alt+0: 切换到第 10 个连接
        // 支持两种模式：
        // 1. 标准模式：带 ALT 修饰符的数字键
        // 2. xterm 8-bit 模式：Alt+数字 发送高位字符 (U+00B0~U+00B9)
        if key.modifiers.contains(KeyModifiers::ALT) {
            if let KeyCode::Char(c) = key.code {
                if let Some(digit) = c.to_digit(10) {
                    let id = if digit == 0 { 9 } else { (digit as usize) - 1 };
                    if id < self.manager.sessions.len() {
                        self.switch_foreground(id)?;
                    }
                    return Ok(());
                }
            }
        }

        // xterm 8-bit 模式：Alt+数字 发送高位字符 (0x30 | 0x80 = 0xB0)
        // U+00B0 (°) = Alt+0, U+00B1 (±) = Alt+1, ..., U+00B9 (¹) = Alt+9
        if let KeyCode::Char(c) = key.code {
            if let Some(digit) = Self::parse_xterm_alt_digit(c) {
                let id = if digit == 0 { 9 } else { (digit as usize) - 1 };
                if id < self.manager.sessions.len() {
                    self.switch_foreground(id)?;
                }
                return Ok(());
            }
        }

        // Alt+Left: 切换到前一个连接 (循环), Alt+Right: 切换到后一个连接 (循环)
        let total = self.manager.sessions.len();
        if total > 0 {
            if key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Left {
                let new_id = (self.manager.foreground_id + total - 1) % total;
                self.switch_foreground(new_id)?;
                return Ok(());
            }
            if key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Right {
                let new_id = (self.manager.foreground_id + 1) % total;
                self.switch_foreground(new_id)?;
                return Ok(());
            }
        }

        // 其他按键交给终端处理
        if let Some(cmd) = self.terminal.handle_key(key) {
            // 用户按了 Enter，提交命令
            if !cmd.is_empty() {
                self.terminal
                    .append_output(&format!("> \x1b[38;2;255;235;59m{}\x1b[0m", cmd))?;
                // 处理内置命令（以 / 开头）
                if cmd.starts_with('/') {
                    self.handle_builtin_command(&cmd)?;
                } else {
                    // 检查是否包含分号，如果有则拆分处理
                    let commands = if cmd.contains(';') {
                        split_commands(&cmd)
                    } else {
                        vec![cmd]
                    };

                    // 逐条处理命令
                    for single_cmd in commands {
                        // 先尝试别名匹配
                        let fg = self.manager.foreground_id;
                        let alias_handled = if fg < self.manager.sessions.len() {
                            if let Some(ref engine) = self.manager.sessions[fg].lua_engine {
                                let handled = engine.process_input(&single_cmd);
                                if handled {
                                    let commands = engine.drain_commands();
                                    self.send_lua_commands(fg, commands)?;
                                    self.drain_lua_logs(fg)?;
                                } else {
                                    self.drain_lua_logs(fg)?;
                                }
                                handled
                            } else {
                                false
                            }
                        } else {
                            false
                        };

                        if !alias_handled {
                            // 无别名匹配，发送到前台连接
                            if let Some(fg) = self.manager.sessions.get(self.manager.foreground_id)
                            {
                                self.logger.log_command(&fg.name, &single_cmd);
                            }
                            if let Err(e) = self.manager.send_to_foreground(&single_cmd) {
                                self.terminal.append_output(&format!("[错误] {}", e))?;
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// 解析 xterm 8-bit 模式的 Alt+数字
    /// xterm 在 8-bit 模式下，Alt+数字 会发送 U+00B0~U+00B9 范围的字符
    /// 例如：Alt+1 → U+00B1 (±), Alt+2 → U+00B2 (²)
    fn parse_xterm_alt_digit(c: char) -> Option<u8> {
        let code = c as u32;
        if (0x00B0..=0x00B9).contains(&code) {
            Some((code - 0x00B0) as u8)
        } else {
            None
        }
    }

    /// 处理内置命令（基于 parse_builtin_command 分发）
    fn handle_builtin_command(&mut self, cmd: &str) -> io::Result<()> {
        match parse_builtin_command(cmd) {
            BuiltinCommand::Connect { name, host, port } => {
                let conn_config = crate::config::ConnectionConfig {
                    name: name.clone(),
                    host: host.clone(),
                    port,
                    encoding: Some("gbk".to_string()),
                    script: None,
                    auto_connect: false,
                    auto_reconnect: true,
                    reconnect_delay_secs: 5,
                    username: None,
                    password: None,
                    socks5_enable: false,
                    socks5_host: None,
                    socks5_port: 1080,
                    socks5_username: None,
                    socks5_password: None,
                    log_rotation_count: None,
                };

                let id = match self.manager.add_connection_dynamic(&conn_config) {
                    Ok(id) => id,
                    Err(e) => {
                        self.terminal.append_output(&format!("[错误] {}", e))?;
                        return Ok(());
                    }
                };
                self.update_status_bar()?;
                let _ = self.connect_tx.try_send(ConnectRequest { session_id: id });
                self.terminal.append_output(&format!(
                    "[系统] 正在连接 {} ({}) → {}:{}",
                    id + 1,
                    name,
                    host,
                    port
                ))?;
            }

            BuiltinCommand::Disconnect { id } => {
                if let Some(id) = id {
                    if id > 0 && id <= self.manager.sessions.len() {
                        let target_id = id - 1;
                        self.manager.sessions[target_id].disconnect();
                        let name = self.manager.sessions[target_id].name.clone();
                        self.manager.sessions[target_id].state =
                            crate::connection::SessionState::Disconnected;
                        self.update_status_bar()?;
                        self.terminal
                            .append_output(&format!("[系统] 已断开连接 {} ({})", id, name))?;
                    } else {
                        self.terminal
                            .append_output(&format!("[错误] 连接 {} 不存在", id))?;
                    }
                } else {
                    let fg = self.manager.foreground_id;
                    if fg < self.manager.sessions.len() {
                        self.manager.sessions[fg].disconnect();
                        self.manager.sessions[fg].state =
                            crate::connection::SessionState::Disconnected;
                        self.update_status_bar()?;
                        self.terminal.append_output(&format!(
                            "[系统] 已断开连接 {} ({})",
                            fg + 1,
                            self.manager.sessions[fg].name
                        ))?;
                    }
                }
            }

            BuiltinCommand::Close { id } => {
                let target = if let Some(id) = id {
                    match id.checked_sub(1) {
                        Some(t) => t,
                        None => {
                            self.terminal.append_output("[错误] 无效的编号")?;
                            return Ok(());
                        }
                    }
                } else {
                    self.manager.foreground_id
                };
                match self.manager.remove_session(target) {
                    Ok(name) => {
                        self.update_status_bar()?;
                        if !self.manager.sessions.is_empty() {
                            self.switch_foreground(self.manager.foreground_id)?;
                        } else {
                            self.terminal.replace_output(&Vec::new())?;
                        }
                        self.terminal.append_output(&format!(
                            "[系统] 已关闭连接 {} ({})",
                            target + 1,
                            name
                        ))?;
                    }
                    Err(e) => {
                        self.terminal.append_output(&format!("[错误] {}", e))?;
                    }
                }
            }

            BuiltinCommand::List => {
                for (i, s) in self.manager.sessions.iter().enumerate() {
                    let state_str = match s.state {
                        crate::connection::SessionState::Connected => "已连接",
                        crate::connection::SessionState::Disconnected => "已断开",
                        crate::connection::SessionState::Connecting => "连接中...",
                        crate::connection::SessionState::Reconnecting => "重连中...",
                    };
                    let marker = if i == self.manager.foreground_id {
                        "★"
                    } else {
                        " "
                    };
                    self.terminal.append_output(&format!(
                        "{} [{}] {} - {}",
                        marker,
                        i + 1,
                        s.name,
                        state_str
                    ))?;
                }
            }

            BuiltinCommand::Load { path } => {
                let fg = self.manager.foreground_id;
                if fg >= self.manager.sessions.len() {
                    self.terminal.append_output("[错误] 无前台连接")?;
                    return Ok(());
                }
                match crate::lua::LuaEngine::new() {
                    Ok(mut engine) => match engine.load_script(&path) {
                        Ok(()) => {
                            self.manager.sessions[fg].lua_engine = Some(engine);
                            self.terminal.append_output(&format!(
                                "\x1b[36m[Lua] 脚本已加载: {}\x1b[0m",
                                path
                            ))?;
                            self.start_timers_for_session(fg);
                        }
                        Err(e) => {
                            let err_msg = e.to_string();
                            for line in format_lua_error(&err_msg) {
                                self.terminal
                                    .append_output(&format!("\x1b[36m[Lua] {}\x1b[0m", line))?;
                            }
                        }
                    },
                    Err(e) => {
                        self.terminal.append_output(&format!(
                            "\x1b[36m[Lua] 引擎初始化失败: {}\x1b[0m",
                            e
                        ))?;
                    }
                }
            }

            BuiltinCommand::LoadReload => {
                let fg = self.manager.foreground_id;
                if fg >= self.manager.sessions.len() {
                    self.terminal.append_output("[错误] 无前台连接")?;
                    return Ok(());
                }
                let script_path = self.manager.sessions[fg]
                    .lua_engine
                    .as_ref()
                    .and_then(|e| e.script_path());
                // 保存原 engine 的变量（如 char_name 等）
                let saved_vars = self.manager.sessions[fg]
                    .lua_engine
                    .as_ref()
                    .map(|e| e.get_variables());
                // 保存原 engine 的连接状态
                let saved_conn_state = self.manager.sessions[fg]
                    .lua_engine
                    .as_ref()
                    .map(|e| e.get_connection_state());
                if let Some(path) = script_path {
                    match crate::lua::LuaEngine::new() {
                        Ok(mut engine) => {
                            // 恢复之前保存的变量
                            if let Some(ref vars) = saved_vars {
                                for (k, v) in vars {
                                    engine.set_variable(k, v);
                                    engine.set_global(k, v);
                                }
                            }
                            // 恢复之前保存的连接状态
                            if let Some(ref conn_state) = saved_conn_state {
                                engine.restore_connection_state(conn_state);
                            }
                            match engine.load_script(&path) {
                                Ok(()) => {
                                    // 排空并记录加载期间的 Lua 日志
                                    let name = self.manager.sessions[fg].name.clone();
                                    let logs = engine.drain_logs();
                                    for msg in logs {
                                        let clean = crate::ui::AnsiParser::strip_ansi(&msg);
                                        self.logger.log(&name, &clean);
                                        self.terminal
                                            .append_output(&format!("\x1b[36m{}\x1b[0m", msg))?;
                                    }
                                    self.manager.sessions[fg].lua_engine = Some(engine);
                                    self.terminal.append_output(&format!(
                                        "\x1b[36m[Lua] 脚本已重新加载: {}\x1b[0m",
                                        path
                                    ))?;
                                    self.start_timers_for_session(fg);
                                }
                                Err(e) => {
                                    let err_msg = e.to_string();
                                    for line in format_lua_error(&err_msg) {
                                        self.terminal.append_output(&format!(
                                            "\x1b[36m[Lua] {}\x1b[0m",
                                            line
                                        ))?;
                                    }
                                    // 脚本加载错误也写入日志
                                    let name = self.manager.sessions[fg].name.clone();
                                    for line in format_lua_error(&err_msg) {
                                        self.logger.log_debug(&name, &line);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            self.terminal.append_output(&format!(
                                "\x1b[36m[Lua] 引擎初始化失败: {}\x1b[0m",
                                e
                            ))?;
                        }
                    }
                } else {
                    self.terminal
                        .append_output("\x1b[36m[Lua] 未找到之前加载的脚本路径\x1b[0m")?;
                }
            }

            BuiltinCommand::Lua { code } => {
                let fg = self.manager.foreground_id;
                if fg >= self.manager.sessions.len() {
                    self.terminal.append_output("[错误] 无前台连接")?;
                    return Ok(());
                }
                let name = self.manager.sessions[fg].name.clone();
                self.logger.log_lua(&name, &code);
                if let Some(ref engine) = self.manager.sessions[fg].lua_engine {
                    match engine.eval_code(&code) {
                        Ok(_) => {
                            let commands = engine.drain_commands();
                            self.send_lua_commands(fg, commands)?;
                            self.send_lua_raw(fg)?;
                            self.drain_lua_logs(fg)?;
                            // /lua 命令可能调用了 SetStatus，刷新状态栏
                            self.update_status_bar()?;
                        }
                        Err(e) => {
                            self.terminal.append_output(&format!("[Lua 错误] {}", e))?;
                        }
                    }
                } else {
                    self.terminal
                        .append_output("[错误] 未加载 Lua 引擎，请先加载脚本")?;
                }
            }

            BuiltinCommand::Set { option, value } => match option.as_str() {
                "keep_command" => {
                    let enabled = matches!(value.as_str(), "on" | "1" | "true" | "yes");
                    self.terminal.state_mut().keep_command = enabled;
                    let status = if enabled { "已启用" } else { "已关闭" };
                    TermSettings {
                        keep_command: enabled,
                    }
                    .save();
                    self.terminal
                        .append_output(&format!("[系统] 保留命令栏输入: {} (已保存)", status))?;
                }
                _ => {
                    self.terminal.append_output(&format!(
                        "[错误] 未知设置选项: {}。可用选项: keep_command",
                        option
                    ))?;
                }
            },

            BuiltinCommand::Switch { target } => {
                // 尝试解析为数字
                if let Ok(id) = target.parse::<usize>() {
                    if id > 0 && id <= self.manager.sessions.len() {
                        let target_id = id - 1;
                        self.switch_foreground(target_id)?;
                        self.terminal.append_output(&format!(
                            "[系统] 已切换到连接 {} ({})",
                            id, self.manager.sessions[target_id].name
                        ))?;
                    } else {
                        self.terminal
                            .append_output(&format!("[错误] 连接 {} 不存在", id))?;
                    }
                } else {
                    // 按名称查找
                    let target_name = target.to_lowercase();
                    if let Some((id, _)) = self
                        .manager
                        .sessions
                        .iter()
                        .enumerate()
                        .find(|(_, s)| s.name.to_lowercase() == target_name)
                    {
                        self.switch_foreground(id)?;
                        self.terminal.append_output(&format!(
                            "[系统] 已切换到连接 {} ({})",
                            id + 1,
                            self.manager.sessions[id].name
                        ))?;
                    } else {
                        self.terminal
                            .append_output(&format!("[错误] 未找到角色 '{}'", target))?;
                    }
                }
            }

            BuiltinCommand::Profile { sub } => match sub {
                ProfileSubcommand::List => {
                    let profile_dir = &self.config.general.profile_dir;
                    match AppConfig::load_profiles(profile_dir) {
                        (profiles, _) if profiles.is_empty() => {
                            self.terminal
                                .append_output("[系统] profiles/ 目录下没有可用角色配置")?;
                        }
                        (profiles, _) => {
                            self.terminal.append_output("[系统] 可用角色配置:")?;
                            for p in &profiles {
                                let loaded = self.manager.sessions.iter().any(|s| s.name == p.name);
                                let marker = if loaded { " (已加载)" } else { "" };
                                self.terminal.append_output(&format!(
                                    "  {} — {}:{}{}",
                                    p.name, p.host, p.port, marker
                                ))?;
                            }
                        }
                    }
                }
                ProfileSubcommand::Load { name } => {
                    // /profile load 与 load_profiles 一致，拒绝加载示例配置
                    if name.eq_ignore_ascii_case("example") {
                        self.terminal
                            .append_output("[错误] 不能加载示例配置文件 (example.toml)")?;
                        return Ok(());
                    }
                    let profile_dir = &self.config.general.profile_dir;
                    let profile_path = Path::new(profile_dir).join(format!("{}.toml", name));
                    if !profile_path.exists() {
                        self.terminal.append_output(&format!(
                            "[错误] 角色配置不存在: {}",
                            profile_path.display()
                        ))?;
                        return Ok(());
                    }
                    let content = match fs::read_to_string(&profile_path) {
                        Ok(c) => c,
                        Err(e) => {
                            self.terminal.append_output(&format!(
                                "[错误] 无法读取配置文件 {}: {}",
                                profile_path.display(),
                                e
                            ))?;
                            return Ok(());
                        }
                    };
                    let conn_config =
                        match toml::from_str::<crate::config::ConnectionConfig>(&content) {
                            Ok(c) => c,
                            Err(e) => {
                                self.terminal
                                    .append_output(&format!("[错误] 配置文件格式错误: {}", e))?;
                                return Ok(());
                            }
                        };

                    let id = match self.manager.add_connection_dynamic(&conn_config) {
                        Ok(id) => id,
                        Err(e) => {
                            self.terminal.append_output(&format!("[错误] {}", e))?;
                            return Ok(());
                        }
                    };

                    // 设置日志保留数量
                    if let Some(count) = conn_config.log_rotation_count {
                        self.logger.set_session_max_files(&conn_config.name, count);
                    }

                    self.update_status_bar()?;
                    let _ = self.connect_tx.try_send(ConnectRequest { session_id: id });
                    self.terminal.append_output(&format!(
                        "[系统] 正在从配置文件加载角色 '{}' 并连接 ({}:{})",
                        conn_config.name, conn_config.host, conn_config.port
                    ))?;
                }
            },

            BuiltinCommand::All { cmd } => {
                let results = self.manager.send_to_all(&cmd);
                let count = results.len();
                let mut ok_count = 0;
                for (name, result) in &results {
                    match result {
                        Ok(()) => ok_count += 1,
                        Err(e) => {
                            self.terminal.append_output(&format!(
                                "[错误] 向 {} 发送命令失败: {}",
                                name, e
                            ))?;
                        }
                    }
                }
                self.terminal.append_output(&format!(
                    "[系统] /all: 已向 {}/{} 个连接发送指令",
                    ok_count, count
                ))?;
                self.logger.log_command("all", &cmd);
            }

            BuiltinCommand::Unknown => {
                self.terminal.append_output("内置命令:")?;
                self.terminal
                    .append_output("  /connect <名> <主机:端口>   添加并连接新角色")?;
                self.terminal
                    .append_output("  /connect <名> <主机> <端口> 同上")?;
                self.terminal
                    .append_output("  /disconnect [编号]           断开连接（保留 session）")?;
                self.terminal
                    .append_output("  /close [编号]               彻底关闭并移除 session")?;
                self.terminal
                    .append_output("  /list                       列出所有连接")?;
                self.terminal
                    .append_output("  /load <脚本路径>            为前台连接加载 Lua 脚本")?;
                self.terminal
                    .append_output("  /load reload                重新加载前台连接的 Lua 脚本")?;
                self.terminal
                    .append_output("  /lua <Lua 代码>             直接执行 Lua 代码")?;
                self.terminal
                    .append_output("  /set keep_command on|off     执行后保留命令栏输入")?;
                self.terminal
                    .append_output("  /switch <编号或名称>        切换到指定连接")?;
                self.terminal
                    .append_output("  /sw <编号或名称>            切换到指定连接 (简写)")?;
                self.terminal
                    .append_output("  /profile list               列出 profiles/ 下可用角色")?;
                self.terminal.append_output(
                    "  /profile load <角色名>      从 profiles/ 加载角色配置并连接",
                )?;
                self.terminal
                    .append_output("  /all <命令>                  向所有连接发送指令")?;
                self.terminal
                    .append_output("  Alt+0~9                     切换前台连接 (最多10个)")?;
                self.terminal
                    .append_output("  Alt+←/→                     循环切换前台连接")?;
            }
        }
        Ok(())
    }

    /// 处理连接管理器事件
    fn handle_manager_event(&mut self, event: ManagerEvent) -> io::Result<()> {
        match event {
            ManagerEvent::Data(id, data) => {
                // 将数据追加到对应连接的输出缓冲区
                if id < self.manager.sessions.len() {
                    let max_lines = self.config.general.scroll_buffer;
                    let session = &mut self.manager.sessions[id];
                    for part in data.split_inclusive('\n') {
                        let trimmed = part.trim_end_matches(['\r', '\n']);
                        if !trimmed.is_empty() {
                            session.output_lines.push(trimmed.to_string());
                        }
                    }
                    // 限制缓冲区大小
                    if session.output_lines.len() > max_lines {
                        let drain_count = session.output_lines.len() - max_lines;
                        session.output_lines.drain(..drain_count);
                    }
                }
                // 仅渲染前台连接的数据
                if id == self.manager.foreground_id {
                    self.terminal.append_output(&data)?;
                }
                // 所有连接数据写入日志
                self.log_session_data(id, &data);

                // 触发器处理（所有连接都触发，不仅仅是前台）
                if id < self.manager.sessions.len() {
                    let trigger_commands =
                        if let Some(ref engine) = self.manager.sessions[id].lua_engine {
                            // 对每行数据分别匹配触发器
                            let mut all_cmds = Vec::new();
                            for part in data.split_inclusive('\n') {
                                let trimmed = part.trim_end_matches(['\r', '\n']);
                                if !trimmed.is_empty() {
                                    engine.process_output(trimmed);
                                    all_cmds.extend(engine.drain_commands());
                                }
                            }
                            // 处理 Lua 日志
                            let logs = engine.drain_logs();
                            let name = self.manager.sessions[id].name.clone();
                            for msg in logs {
                                let clean = crate::ui::AnsiParser::strip_ansi(&msg);
                                self.logger.log(&name, &clean);
                                if id == self.manager.foreground_id {
                                    self.terminal
                                        .append_output(&format!("\x1b[36m[Lua] {}\x1b[0m", msg))?;
                                }
                            }
                            all_cmds
                        } else {
                            Vec::new()
                        };
                    // 发送触发器产生的命令
                    self.send_lua_commands(id, trigger_commands)?;
                    // 发送 SendPkt 压入的原始数据包
                    self.send_lua_raw(id)?;
                    // 触发器中可能调用了 SetStatus，刷新状态栏
                    if id == self.manager.foreground_id {
                        self.update_status_bar()?;
                    }
                }
            }
            ManagerEvent::StateChange(id, state) => {
                // 检查 session 是否仍然存在（可能已被 /close 移除）
                if id >= self.manager.sessions.len() {
                    return Ok(());
                }
                self.manager.sessions[id].state = state.clone();
                // 同步 Lua 引擎的连接状态（同步到对应 session，不限于前台）
                if let Some(ref mut engine) = self.manager.sessions[id].lua_engine {
                    engine.set_connected(matches!(state, SessionState::Connected));
                }
                if id == self.manager.foreground_id {
                    self.update_status_bar()?;
                }
                let state_str = match &state {
                    SessionState::Connected => "已连接",
                    SessionState::Disconnected => "已断开",
                    SessionState::Connecting => "连接中...",
                    SessionState::Reconnecting => "重连中...",
                };
                let name = &self.manager.sessions[id].name;
                self.terminal.append_output(&format!(
                    "[系统] 连接 {} ({}) {}",
                    id + 1,
                    name,
                    state_str
                ))?;

                // 自动重连：断开时启动延迟重连任务
                if state == SessionState::Disconnected {
                    let session = &self.manager.sessions[id];
                    if session.auto_reconnect {
                        let delay = session.reconnect_delay_secs;
                        self.terminal
                            .append_output(&format!("[系统] {} 秒后尝试重连 {}...", delay, name))?;
                        // 启动延迟重连任务
                        let tx = self.reconnect_tx.clone();
                        tokio::spawn(async move {
                            tokio::time::sleep(tokio::time::Duration::from_secs(delay)).await;
                            let _ = tx.send(ReconnectRequest { session_id: id }).await;
                        });
                    }
                }
            }
            ManagerEvent::Error(id, err) => {
                let name = if id < self.manager.sessions.len() {
                    &self.manager.sessions[id].name
                } else {
                    "未知"
                };
                self.terminal.append_output(&format!(
                    "[错误] 连接 {} ({}): {}",
                    id + 1,
                    name,
                    err
                ))?;
            }
        }
        Ok(())
    }

    /// 更新状态栏（包括 session 状态栏和 Lua 状态栏）
    fn update_status_bar(&mut self) -> io::Result<()> {
        let mut stdout = io::stdout();
        let infos = self.manager.session_infos();
        let fg = self.manager.foreground_id;
        self.terminal.draw_status_bar(&mut stdout, &infos, fg)?;
        self.terminal.draw_lua_status_bar(&mut stdout, &infos, fg)?;
        // 将光标定位到输入行（draw_lua_status_bar 不再内部 flush）
        self.terminal.draw_input_line(&mut stdout)?;
        stdout.flush()?;
        Ok(())
    }

    /// 退出程序（suppress clippy unused warning）
    #[allow(dead_code)]
    pub fn quit(&mut self) {
        self.running = false;
    }

    /// 记录连接数据到日志文件（剥离 ANSI 码）
    fn log_session_data(&self, id: usize, data: &str) {
        let name = if id < self.manager.sessions.len() {
            &self.manager.sessions[id].name
        } else {
            return;
        };
        let clean = AnsiParser::strip_ansi(data);
        self.logger.log(name, &clean);
    }

    /// 切换前台连接，恢复目标连接的输出缓冲区
    fn switch_foreground(&mut self, id: usize) -> io::Result<()> {
        // 保存当前前台 session 的输入状态
        let old_id = self.manager.foreground_id;
        if old_id < self.manager.sessions.len() {
            let saved = self.terminal.save_input_state();
            self.manager.sessions[old_id].input_state = saved;
        }

        self.manager.switch_foreground(id).ok();
        self.update_status_bar()?;

        // 恢复目标 session 的输入状态
        if id < self.manager.sessions.len() {
            let saved = self.manager.sessions[id].input_state.clone();
            self.terminal.restore_input_state(&saved);
        }

        // 恢复目标连接的输出缓冲区到终端
        let output = if id < self.manager.sessions.len() {
            &self.manager.sessions[id].output_lines
        } else {
            &Vec::new()
        };
        self.terminal.replace_output(output)?;
        self.terminal.append_output(&format!(
            "[系统] 切换到连接 {} ({})",
            id + 1,
            self.manager.foreground_name()
        ))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_connect_args_host_port() {
        let parts: Vec<&str> = "/connect test mud.example.com 4000"
            .split_whitespace()
            .collect();
        let result = parse_connect_args(&parts);
        assert_eq!(result, Some(("mud.example.com".to_string(), 4000)));
    }

    #[test]
    fn test_parse_connect_args_host_colon_port() {
        let parts: Vec<&str> = "/connect test mud.example.com:4000"
            .split_whitespace()
            .collect();
        let result = parse_connect_args(&parts);
        assert_eq!(result, Some(("mud.example.com".to_string(), 4000)));
    }

    #[test]
    fn test_parse_connect_args_default_port() {
        let parts: Vec<&str> = "/connect test mud.example.com".split_whitespace().collect();
        let result = parse_connect_args(&parts);
        assert_eq!(result, Some(("mud.example.com".to_string(), 5555)));
    }

    #[test]
    fn test_parse_connect_args_invalid_port() {
        let parts: Vec<&str> = "/connect test mud.example.com abc"
            .split_whitespace()
            .collect();
        let result = parse_connect_args(&parts);
        assert_eq!(result, Some(("mud.example.com".to_string(), 5555)));
    }

    #[test]
    fn test_parse_connect_args_too_few() {
        let parts: Vec<&str> = "/connect test".split_whitespace().collect();
        let result = parse_connect_args(&parts);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_connect_args_empty() {
        let parts: Vec<&str> = "/connect".split_whitespace().collect();
        let result = parse_connect_args(&parts);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_connect_args_host_colon_invalid_port() {
        let parts: Vec<&str> = "/connect test host:abc".split_whitespace().collect();
        let result = parse_connect_args(&parts);
        assert_eq!(result, Some(("host".to_string(), 5555)));
    }

    #[test]
    fn test_parse_connect_args_host_colon_port_with_extra() {
        // host:port format with extra arg should use separate format
        let parts: Vec<&str> = "/connect test host:4000 extra".split_whitespace().collect();
        let result = parse_connect_args(&parts);
        // When len != 3 and contains ':', falls to else branch
        assert_eq!(result, Some(("host:4000".to_string(), 5555)));
    }

    // === parse_builtin_command 测试 ===

    #[test]
    fn test_parse_builtin_connect() {
        let cmd = parse_builtin_command("/connect test mud.example.com 4000");
        assert_eq!(
            cmd,
            BuiltinCommand::Connect {
                name: "test".to_string(),
                host: "mud.example.com".to_string(),
                port: 4000
            }
        );
    }

    #[test]
    fn test_parse_builtin_connect_colon_port() {
        let cmd = parse_builtin_command("/connect mymud host:5555");
        assert_eq!(
            cmd,
            BuiltinCommand::Connect {
                name: "mymud".to_string(),
                host: "host".to_string(),
                port: 5555
            }
        );
    }

    #[test]
    fn test_parse_builtin_connect_invalid() {
        let cmd = parse_builtin_command("/connect");
        assert_eq!(cmd, BuiltinCommand::Unknown);

        let cmd = parse_builtin_command("/connect test");
        assert_eq!(cmd, BuiltinCommand::Unknown);
    }

    #[test]
    fn test_parse_builtin_disconnect_with_id() {
        let cmd = parse_builtin_command("/disconnect 2");
        assert_eq!(cmd, BuiltinCommand::Disconnect { id: Some(2) });
    }

    #[test]
    fn test_parse_builtin_disconnect_no_id() {
        let cmd = parse_builtin_command("/disconnect");
        assert_eq!(cmd, BuiltinCommand::Disconnect { id: None });
    }

    #[test]
    fn test_parse_builtin_disconnect_invalid_id() {
        let cmd = parse_builtin_command("/disconnect abc");
        assert_eq!(cmd, BuiltinCommand::Disconnect { id: None });
    }

    #[test]
    fn test_parse_builtin_close_with_id() {
        let cmd = parse_builtin_command("/close 3");
        assert_eq!(cmd, BuiltinCommand::Close { id: Some(3) });
    }

    #[test]
    fn test_parse_builtin_close_no_id() {
        let cmd = parse_builtin_command("/close");
        assert_eq!(cmd, BuiltinCommand::Close { id: None });
    }

    #[test]
    fn test_parse_builtin_list() {
        let cmd = parse_builtin_command("/list");
        assert_eq!(cmd, BuiltinCommand::List);
    }

    #[test]
    fn test_parse_builtin_load() {
        let cmd = parse_builtin_command("/load /path/to/script.lua");
        assert_eq!(
            cmd,
            BuiltinCommand::Load {
                path: "/path/to/script.lua".to_string()
            }
        );
    }

    #[test]
    fn test_parse_builtin_load_reload() {
        let cmd = parse_builtin_command("/load reload");
        assert_eq!(cmd, BuiltinCommand::LoadReload);
    }

    #[test]
    fn test_parse_builtin_load_no_path() {
        let cmd = parse_builtin_command("/load");
        assert_eq!(cmd, BuiltinCommand::Unknown);
    }

    #[test]
    fn test_parse_builtin_lua() {
        let cmd = parse_builtin_command("/lua print('hello')");
        assert_eq!(
            cmd,
            BuiltinCommand::Lua {
                code: "print('hello')".to_string()
            }
        );
    }

    #[test]
    fn test_parse_builtin_lua_no_code() {
        let cmd = parse_builtin_command("/lua");
        assert_eq!(cmd, BuiltinCommand::Unknown);
    }

    #[test]
    fn test_parse_builtin_set_keep_command_on() {
        let cmd = parse_builtin_command("/set keep_command on");
        assert_eq!(
            cmd,
            BuiltinCommand::Set {
                option: "keep_command".to_string(),
                value: "on".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_builtin_set_keep_command_off() {
        let cmd = parse_builtin_command("/set keep_command off");
        assert_eq!(
            cmd,
            BuiltinCommand::Set {
                option: "keep_command".to_string(),
                value: "off".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_builtin_set_too_few_args() {
        let cmd = parse_builtin_command("/set");
        assert_eq!(cmd, BuiltinCommand::Unknown);
    }

    #[test]
    fn test_parse_builtin_unknown() {
        assert_eq!(parse_builtin_command("/unknown"), BuiltinCommand::Unknown);
        assert_eq!(parse_builtin_command(""), BuiltinCommand::Unknown);
        assert_eq!(parse_builtin_command("hello"), BuiltinCommand::Unknown);
    }

    #[test]
    fn test_parse_builtin_profile_load() {
        let result = parse_builtin_command("/profile load mychar");
        assert_eq!(
            result,
            BuiltinCommand::Profile {
                sub: ProfileSubcommand::Load {
                    name: "mychar".to_string()
                }
            }
        );
    }

    #[test]
    fn test_parse_builtin_profile_list() {
        let result = parse_builtin_command("/profile list");
        assert_eq!(
            result,
            BuiltinCommand::Profile {
                sub: ProfileSubcommand::List
            }
        );
    }

    #[test]
    fn test_parse_builtin_profile_no_subcommand() {
        assert_eq!(parse_builtin_command("/profile"), BuiltinCommand::Unknown);
    }

    #[test]
    fn test_parse_builtin_profile_unknown_subcommand() {
        assert_eq!(
            parse_builtin_command("/profile foo bar"),
            BuiltinCommand::Unknown
        );
    }

    #[test]
    fn test_parse_builtin_profile_load_no_name() {
        assert_eq!(
            parse_builtin_command("/profile load"),
            BuiltinCommand::Unknown
        );
    }

    #[test]
    fn test_parse_builtin_set_partial_arg() {
        // 只有 option 没有 value
        assert_eq!(
            parse_builtin_command("/set keep_command"),
            BuiltinCommand::Unknown
        );
    }

    #[test]
    fn test_term_settings_default() {
        let settings = TermSettings::default();
        assert!(settings.keep_command);
    }

    #[test]
    fn test_term_settings_serde_round_trip() {
        let settings = TermSettings {
            keep_command: false,
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert_eq!(json, r#"{"keep_command":false}"#);
        let deserialized: TermSettings = serde_json::from_str(&json).unwrap();
        assert!(!deserialized.keep_command);

        let settings2 = TermSettings { keep_command: true };
        let json2 = serde_json::to_string(&settings2).unwrap();
        assert_eq!(json2, r#"{"keep_command":true}"#);
        let deserialized2: TermSettings = serde_json::from_str(&json2).unwrap();
        assert!(deserialized2.keep_command);
    }

    #[test]
    fn test_term_settings_json_field_case() {
        // 验证反序列化项名称大小写敏感
        let json = r#"{"keep_command":true}"#;
        let settings: TermSettings = serde_json::from_str(json).unwrap();
        assert!(settings.keep_command);

        let json_false = r#"{"keep_command":false}"#;
        let settings_false: TermSettings = serde_json::from_str(json_false).unwrap();
        assert!(!settings_false.keep_command);
    }

    #[test]
    fn test_term_settings_path() {
        assert_eq!(TermSettings::path(), "profiles/terminal.json");
    }

    #[test]
    fn test_split_commands_basic() {
        let result = split_commands("east;east;look");
        assert_eq!(result, vec!["east", "east", "look"]);
    }

    #[test]
    fn test_split_commands_single() {
        let result = split_commands("look");
        assert_eq!(result, vec!["look"]);
    }

    #[test]
    fn test_split_commands_empty() {
        let result = split_commands("");
        assert_eq!(result, Vec::<String>::new());
    }

    #[test]
    fn test_split_commands_escape_semicolon() {
        let result = split_commands("say hello\\;world");
        assert_eq!(result, vec!["say hello;world"]);
    }

    #[test]
    fn test_split_commands_mixed_escape() {
        let result = split_commands("east;say hi\\;there;west");
        assert_eq!(result, vec!["east", "say hi;there", "west"]);
    }

    #[test]
    fn test_split_commands_empty_parts() {
        let result = split_commands("east;;west");
        assert_eq!(result, vec!["east", "west"]);
    }

    #[test]
    fn test_split_commands_whitespace() {
        let result = split_commands("  east  ;  west  ");
        assert_eq!(result, vec!["east", "west"]);
    }

    #[test]
    fn test_split_commands_trailing_semicolon() {
        let result = split_commands("east;");
        assert_eq!(result, vec!["east"]);
    }

    #[test]
    fn test_split_commands_leading_semicolon() {
        let result = split_commands(";east");
        assert_eq!(result, vec!["east"]);
    }

    #[test]
    fn test_split_commands_backslash_not_before_semicolon() {
        let result = split_commands("east\\;west");
        assert_eq!(result, vec!["east;west"]);
    }

    #[test]
    fn test_split_commands_trailing_backslash() {
        let result = split_commands("east\\");
        assert_eq!(result, vec!["east\\"]);
    }

    #[test]
    fn test_split_commands_backslash_at_end() {
        let result = split_commands("say test\\");
        assert_eq!(result, vec!["say test\\"]);
    }

    #[test]
    fn test_format_lua_error_basic() {
        let result = format_lua_error("error: syntax error");
        assert_eq!(result, vec!["error: syntax error"]);
    }

    #[test]
    fn test_format_lua_error_stack_traceback() {
        let err = "stack traceback:\n\t[string \"line\"]:1: in main chunk";
        let result = format_lua_error(err);
        assert_eq!(
            result,
            vec!["stack traceback:", "[string \"line\"]:1: in main chunk"]
        );
    }

    #[test]
    fn test_format_lua_error_empty_lines() {
        let err = "line1\n\n\nline2";
        let result = format_lua_error(err);
        assert_eq!(result, vec!["line1", "line2"]);
    }

    #[test]
    fn test_format_lua_error_all_whitespace() {
        let result = format_lua_error("   \n  \n  ");
        assert_eq!(result, vec!["   \n  \n  "]);
    }

    #[test]
    fn test_format_lua_error_empty_string() {
        let result = format_lua_error("");
        assert_eq!(result, vec![""]);
    }

    #[test]
    fn test_format_lua_error_single_line() {
        let result = format_lua_error("just one line");
        assert_eq!(result, vec!["just one line"]);
    }

    #[test]
    fn test_parse_builtin_switch_by_name() {
        let result = parse_builtin_command("/switch char2");
        assert_eq!(
            result,
            BuiltinCommand::Switch {
                target: "char2".to_string()
            }
        );
    }

    #[test]
    fn test_parse_builtin_switch_alias() {
        let result = parse_builtin_command("/sw char2");
        assert_eq!(
            result,
            BuiltinCommand::Switch {
                target: "char2".to_string()
            }
        );
    }

    #[test]
    fn test_parse_builtin_switch_no_target() {
        let result = parse_builtin_command("/switch");
        assert_eq!(result, BuiltinCommand::Unknown);
    }

    #[test]
    fn test_parse_builtin_switch_alias_no_target() {
        let result = parse_builtin_command("/sw");
        assert_eq!(result, BuiltinCommand::Unknown);
    }

    #[test]
    fn test_parse_builtin_switch_by_number() {
        let result = parse_builtin_command("/switch 3");
        assert_eq!(
            result,
            BuiltinCommand::Switch {
                target: "3".to_string()
            }
        );
    }

    #[test]
    fn test_parse_builtin_connect_host_port_separate() {
        let result = parse_builtin_command("/connect char2 mud.example.com 6666");
        assert_eq!(
            result,
            BuiltinCommand::Connect {
                name: "char2".to_string(),
                host: "mud.example.com".to_string(),
                port: 6666,
            }
        );
    }
}
