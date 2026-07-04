use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use crossterm::event::{
    Event as CrosstermEvent, EventStream, KeyCode, KeyModifiers, MouseButton, MouseEventKind,
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

use crate::config::AppConfig;
use crate::connection::{ConnectionManager, ManagerEvent, SessionId, SessionState};
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
    /// /reconnect [编号] — 断开并重新连接
    Reconnect { id: Option<usize> },
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
        "/reconnect" => {
            let id = parts.get(1).and_then(|s| s.parse::<usize>().ok());
            BuiltinCommand::Reconnect { id }
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
    session_id: SessionId,
}

/// 动态连接请求
struct ConnectRequest {
    session_id: SessionId,
}

/// 定时器触发请求
struct TimerRequest {
    session_id: SessionId,
}

/// 渲染刷新请求
struct RenderTickRequest {
    session_id: SessionId,
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
    render_tick_tx: mpsc::Sender<RenderTickRequest>,
    render_tick_rx: mpsc::Receiver<RenderTickRequest>,
    render_tick_cancels: HashMap<SessionId, oneshot::Sender<()>>,
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
        let (render_tick_tx, render_tick_rx) = mpsc::channel(32);

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
            render_tick_tx,
            render_tick_rx,
            render_tick_cancels: HashMap::new(),
        })
    }

    /// 启动应用主循环
    pub async fn run(&mut self) -> io::Result<()> {
        self.terminal.init_screen()?;

        // 自动连接所有 auto_connect 的连接（包括从 profile 加载的）
        let auto_connect_ids: Vec<SessionId> = self
            .manager
            .ordered_session_ids()
            .iter()
            .filter(|&&id| self.manager.get_by_id(id).map(|s| s.auto_connect).unwrap_or(false))
            .copied()
            .collect();
        for session_id in auto_connect_ids {
            let name = self.manager.get_by_id(session_id).map(|s| s.name.clone()).unwrap_or_default();
            let display_pos = self.manager.display_number_of(session_id);
            match self.manager.connect_session(session_id).await {
                Ok(()) => {
                    let msg = format!("[系统] 连接 {} ({}) 已建立", display_pos, name);
                    self.terminal.append_output(&msg)?;
                    self.init_lua_for_session(session_id)?;
                }
                Err(e) => {
                    let msg = format!("[系统] 连接 {} ({}) 失败: {}", display_pos, name, e);
                    self.terminal.append_output(&msg)?;
                }
            }
        }

        self.update_status_bar()?;

        // 为每个 session 启动渲染刷新定时器（非实时模式且 render_interval > 0）
        let render_tick_sessions: Vec<(SessionId, u64)> = self
            .manager
            .ordered_session_ids()
            .iter()
            .filter_map(|&id| {
                let s = self.manager.get_by_id(id)?;
                if !s.realtime && s.render_interval > 0 {
                    Some((id, s.render_interval))
                } else {
                    None
                }
            })
            .collect();
        for (session_id, interval) in render_tick_sessions {
            self.start_render_tick_timer(session_id, interval);
        }

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

                // 处理渲染刷新请求
                Some(req) = self.render_tick_rx.recv() => {
                    self.handle_render_tick(req.session_id)?;
                }
            }
        }

        Ok(())
    }

    /// 执行重连
    async fn perform_reconnect(&mut self, session_id: SessionId) -> io::Result<()> {
        let name = self.manager.get_by_id(session_id).map(|s| s.name.clone()).unwrap_or_else(|| "未知".to_string());
        let display_pos = self.manager.display_number_of(session_id);

        match self.manager.connect_session(session_id).await {
            Ok(()) => {
                let msg = format!("[系统] 连接 {} ({}) 重连成功", display_pos, name);
                self.terminal.append_output(&msg)?;
                // 如果 Lua 引擎已存在（重连前已加载脚本），不重建引擎，
                // 保留 Lua 变量状态（stat.* 等统计数据）。仅通知引擎已连接以触发 OnConnect。
                // 若引擎不存在（首次连接或未加载脚本），则执行标准初始化流程。
                let has_engine = self.manager.get_by_id(session_id).map(|s| s.lua_engine.is_some()).unwrap_or(false);
                if has_engine {
                    // 先排空 OnConnect() 产生的命令和日志，再发送
                    let queued_cmds = {
                        let engine = self.manager.get_mut_by_id(session_id)
                            .and_then(|s| s.lua_engine.as_mut())
                            .unwrap();
                        engine.set_connected(true);
                        engine.drain_commands()
                    };
                    for cmd in &queued_cmds {
                        self.logger.log_command(&name, cmd);
                        if let Err(e) = self.manager.send_to(session_id, cmd) {
                            self.terminal.append_output(&format!("[发送错误] {}", e))?;
                        }
                    }
                    self.drain_lua_logs(session_id)?;
                } else {
                    self.init_lua_for_session(session_id)?;
                }
                // 重连后刷新状态栏（Lua 脚本可能调用了 SetStatus）
                if session_id == self.manager.foreground_id {
                    self.update_status_bar()?;
                }
            }
            Err(e) => {
                let msg = format!("[系统] 重连 {} ({}) 失败: {}", display_pos, name, e);
                self.terminal.append_output(&msg)?;
            }
        }
        Ok(())
    }

    /// 执行动态连接
    async fn perform_connect(&mut self, session_id: SessionId) -> io::Result<()> {
        let (name, host, port) = match self.manager.get_by_id(session_id) {
            Some(s) => (s.name.clone(), s.host.clone(), s.port),
            None => {
                self.terminal.append_output("[错误] 无效的连接 ID")?;
                return Ok(());
            }
        };
        let display_pos = self.manager.display_number_of(session_id);

        match self.manager.connect_session(session_id).await {
            Ok(()) => {
                let msg = format!(
                    "[系统] 连接 {} ({}) → {}:{} 已建立",
                    display_pos,
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
    fn init_lua_for_session(&mut self, session_id: SessionId) -> io::Result<()> {
        // 从 Session 自身获取配置
        let (name, script_path, username, password, host) = match self.manager.get_by_id(session_id) {
            Some(s) => (s.name.clone(), s.script_path.clone(), s.username.clone(), s.password.clone(), s.host.clone()),
            None => return Ok(()),
        };
        let display_pos = self.manager.display_number_of(session_id);

        match crate::lua::LuaEngine::new() {
            Ok(mut engine) => {
                // 注入主机地址（供 GetInfo(1) 返回）
                engine.set_host(&host);
                // 注入世界名称（供 GetInfo(2) 返回）
                engine.set_world_name(&name);
                // 注入日志目录（供 GetInfo(58) 返回）
                engine.set_log_dir(&self.config.general.log_dir);

                // 注入登录凭证到 Lua 变量和全局变量
                if let Some(ref uname) = username {
                    if !uname.is_empty() {
                        engine.set_variable("char_name", uname);
                        engine.set_global("char_name", uname);
                        engine.set_char_name(uname); // 供 GetInfo(3) 返回
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
                                    if let Err(e) = self.manager.send_to(session_id, cmd) {
                                        self.terminal
                                            .append_output(&format!("[发送错误] {}", e))?;
                                    }
                                }
                            }

                            // 排空脚本加载期间的 Lua 日志
                            self.drain_lua_logs(session_id)?;

                            let msg = format!("[Lua] 连接 {} 脚本已加载: {}", display_pos, path);
                            self.terminal.append_output(&msg)?;
                        }
                        Err(e) => {
                            let err_msg = e.to_string();
                            for line in format_lua_error(&err_msg) {
                                self.terminal.append_output(&line)?;
                            }
                            // 脚本加载错误也写入日志
                            for line in format_lua_error(&err_msg) {
                                self.logger.log_debug(&name, &line);
                            }
                        }
                    }
                }

                self.manager.get_mut_by_id(session_id).map(|s| s.lua_engine = Some(engine));
                // 同步连接状态：session.connect() 在创建事件通道前已设置 state，
                // 初始 Connected 状态不会通过 StateChange 事件到达 engine，
                // 此处手动同步，确保 engine 知道当前已连接并触发 alias.atconnect()
                {
                    let is_connected = self.manager.get_by_id(session_id)
                        .map(|s| matches!(s.state, crate::connection::SessionState::Connected))
                        .unwrap_or(false);
                    if is_connected {
                        let queued_cmds = {
                            match self.manager.get_mut_by_id(session_id).and_then(|s| s.lua_engine.as_mut()) {
                                Some(eng) => {
                                    eng.set_connected(true);
                                    eng.drain_commands()
                                }
                                None => Vec::new(),
                            }
                        };
                        for cmd in &queued_cmds {
                            self.logger.log_command(&name, cmd);
                            if let Err(e) = self.manager.send_to(session_id, cmd) {
                                self.terminal.append_output(&format!("[发送错误] {}", e))?;
                            }
                        }
                        self.drain_lua_logs(session_id)?;
                    }
                }
                // 启动定时器
                self.start_timers_for_session(session_id);
            }
            Err(e) => {
                let msg = format!("[Lua] 连接 {} 引擎初始化失败: {}", display_pos, e);
                self.terminal.append_output(&msg)?;
            }
        }
        Ok(())
    }

    /// 为指定连接启动定时器任务
    fn start_timers_for_session(&mut self, session_id: SessionId) {
        let session = match self.manager.get_mut_by_id(session_id) {
            Some(s) => s,
            None => return,
        };
        let (timer_cancel_tx, mut timer_cancel_rx) = oneshot::channel();
        session.timer_cancel_tx = Some(timer_cancel_tx);

        // 使用轮询方式：单个 tokio 任务定期检查所有定时器
        // 这解决了动态创建的定时器（如 wait.time 创建的）无法触发的问题
        let timer_tx = self.timer_tx.clone();
        tokio::spawn(async move {
            // 轮询间隔 50ms，确保定时器精度
            let poll_interval = tokio::time::Duration::from_millis(50);
            loop {
                tokio::select! {
                    _ = &mut timer_cancel_rx => { break; }
                    _ = tokio::time::sleep(poll_interval) => {
                        if timer_tx
                            .send(TimerRequest { session_id })
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        });
    }

    /// 发送 Lua 引擎产生的命令，拦截 / 开头的命令作为 Lua 代码执行
    fn send_lua_commands(&mut self, session_id: SessionId, commands: Vec<String>) -> io::Result<()> {
        let name = match self.manager.get_by_id(session_id) {
            Some(s) => s.name.clone(),
            None => return Ok(()),
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
                if let Some(ref engine) = self.manager.get_by_id(session_id).and_then(|s| s.lua_engine.as_ref()) {
                    match engine.eval_code(lua_code) {
                        Ok(_) => {
                            let sub_commands = engine.drain_commands();
                            for sub_cmd in sub_commands {
                                self.logger.log_command(&name, &sub_cmd);
                                if let Err(e) = self.manager.send_to(session_id, &sub_cmd) {
                                    self.terminal.append_output(&format!("[Lua 错误] {}", e))?;
                                }
                            }
                        }
                        Err(e) => {
                            self.terminal.append_output(&format!("[Lua 错误] {}", e))?;
                        }
                    }
                }
            } else if depth < max_depth {
                // 非 / 开头的命令：先尝试别名匹配（与 MUSHclient Execute 行为一致）
                let alias_handled =
                    if let Some(ref engine) = self.manager.get_by_id(session_id).and_then(|s| s.lua_engine.as_ref()) {
                        let handled = engine.process_input(&cmd);
                        if handled {
                            let sub_commands = engine.drain_commands();
                            if !sub_commands.is_empty() {
                                // 别名匹配成功，产生的命令加入队列继续处理
                                for sub_cmd in sub_commands {
                                    queue.push_front(sub_cmd);
                                }
                            }
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
    fn send_lua_raw(&mut self, session_id: SessionId) -> io::Result<()> {
        let raw_packets = self.manager.get_by_id(session_id)
            .and_then(|s| s.lua_engine.as_ref())
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
    fn handle_timer(&mut self, session_id: SessionId) -> io::Result<()> {
        if self.manager.get_by_id(session_id).is_none() {
            return Ok(());
        }
        let mut any_fired = false;
        loop {
            // 先检查是否有到期的定时器，确保 engine 引用在调用 self 方法前被释放
            let should_fire = self.manager.get_by_id(session_id)
                .and_then(|s| s.lua_engine.as_ref())
                .map(|engine| engine.fire_next_due_timer())
                .unwrap_or(false);
            if !should_fire {
                break;
            }
            any_fired = true;
            let commands = self.manager.get_by_id(session_id)
                .and_then(|s| s.lua_engine.as_ref())
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
        if let Some(ref engine) = self.manager.get_by_id(session_id).and_then(|s| s.lua_engine.as_ref()) {
            engine.fire_keepalive_if_idle();
        }
        self.send_lua_raw(session_id)?;
        // 仅在定时器真正触发时才刷新状态栏（避免每 50ms 写终端，破坏鼠标选中）
        if any_fired && session_id == self.manager.foreground_id {
            self.update_status_bar()?;
        }
        Ok(())
    }

    /// 停止指定 session 的渲染刷新定时器
    fn stop_render_tick_timer(&mut self, session_id: SessionId) {
        if let Some(cancel_tx) = self.render_tick_cancels.remove(&session_id) {
            let _ = cancel_tx.send(());
        }
    }

    /// 启动渲染刷新定时器：按指定间隔定期发送刷新请求
    fn start_render_tick_timer(&mut self, session_id: SessionId, interval_ms: u64) {
        // 先停止旧的定时器
        self.stop_render_tick_timer(session_id);

        let tx = self.render_tick_tx.clone();
        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        self.render_tick_cancels.insert(session_id, cancel_tx);

        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(tokio::time::Duration::from_millis(interval_ms));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if tx.send(RenderTickRequest { session_id }).await.is_err() {
                            break;
                        }
                    }
                    _ = &mut cancel_rx => {
                        break;
                    }
                }
            }
        });
    }

    /// 处理渲染刷新请求：将缓冲的待渲染数据一次性输出到终端
    fn handle_render_tick(&mut self, session_id: SessionId) -> io::Result<()> {
        if self.manager.get_by_id(session_id).is_none() {
            return Ok(());
        }
        // 仅当前台 session 且有待渲染数据时才刷新
        if session_id != self.manager.foreground_id {
            if let Some(session) = self.manager.get_mut_by_id(session_id) {
                session.render_dirty = false;
            }
            return Ok(());
        }
        if !self.manager.get_by_id(session_id).map(|s| s.render_dirty).unwrap_or(false) {
            return Ok(());
        }
        let pending = self.manager.get_mut_by_id(session_id).map(|s| std::mem::take(&mut s.pending_data)).unwrap_or_default();
        if let Some(session) = self.manager.get_mut_by_id(session_id) {
            session.render_dirty = false;
        }
        if pending.is_empty() {
            return Ok(());
        }
        // 合并所有待渲染行，一次性输出
        let mut combined = String::new();
        for line in &pending {
            combined.push_str(line);
            combined.push('\n');
        }
        if !combined.is_empty() {
            self.terminal.append_output(&combined)?;
        }
        Ok(())
    }

    /// 处理 Lua 引擎产生的日志
    fn drain_lua_logs(&mut self, session_id: SessionId) -> io::Result<()> {
        let logs = self.manager.get_by_id(session_id)
            .and_then(|s| s.lua_engine.as_ref())
            .map(|engine| engine.drain_logs())
            .unwrap_or_default();
        let name = self.manager.get_by_id(session_id).map(|s| s.name.clone()).unwrap_or_default();
        let is_foreground = session_id == self.manager.foreground_id;
        // 节流模式下将 Lua 日志缓冲到 pending_data，与 MUD 数据一起刷新
        let buffer = is_foreground && !self.manager.get_by_id(session_id).map(|s| s.realtime).unwrap_or(false);
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
            if is_foreground {
                if buffer {
                    self.manager.get_mut_by_id(session_id)
                        .map(|s| {
                            s.pending_data.push(format!("\x1b[36m[Lua] {}\x1b[0m", msg));
                            s.render_dirty = true;
                        });
                } else {
                    self.terminal
                        .append_output(&format!("\x1b[36m[Lua] {}\x1b[0m", msg))?;
                }
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
                    if self.manager.get_by_id(region.session_id).is_some() {
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
                    let display_num = if digit == 0 { 10 } else { digit as usize };
                    if let Some(session_id) = self.manager.session_id_by_display_number(display_num) {
                        self.switch_foreground(session_id)?;
                    }
                    return Ok(());
                }
            }
        }

        // xterm 8-bit 模式：Alt+数字 发送高位字符 (0x30 | 0x80 = 0xB0)
        // U+00B0 (°) = Alt+0, U+00B1 (±) = Alt+1, ..., U+00B9 (¹) = Alt+9
        if let KeyCode::Char(c) = key.code {
            if let Some(digit) = Self::parse_xterm_alt_digit(c) {
                let display_num = if digit == 0 { 10 } else { digit as usize };
                if let Some(session_id) = self.manager.session_id_by_display_number(display_num) {
                    self.switch_foreground(session_id)?;
                }
                return Ok(());
            }
        }

        // Alt+Left: 切换到前一个连接 (循环), Alt+Right: 切换到后一个连接 (循环)
        if self.manager.session_count() > 0 {
            if key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Left {
                if let Some(new_id) = self.manager.cycle_foreground(-1) {
                    self.switch_foreground(new_id)?;
                }
                return Ok(());
            }
            if key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Right {
                if let Some(new_id) = self.manager.cycle_foreground(1) {
                    self.switch_foreground(new_id)?;
                }
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
                        let fg_id = self.manager.foreground_id;
                        let alias_handled = if let Some(ref engine) = self.manager.get_by_id(fg_id).and_then(|s| s.lua_engine.as_ref()) {
                                let handled = engine.process_input(&single_cmd);
                                if handled {
                                    let commands = engine.drain_commands();
                                    self.send_lua_commands(fg_id, commands)?;
                                    self.drain_lua_logs(fg_id)?;
                                } else {
                                    self.drain_lua_logs(fg_id)?;
                                }
                                handled
                            } else {
                                false
                            };

                        if !alias_handled {
                            // 无别名匹配，发送到前台连接
                            if let Some(fg) = self.manager.get_by_id(self.manager.foreground_id)
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
                    render_interval: 1000,
                    realtime: false,
                };

                let session_id = match self.manager.add_connection_dynamic(&conn_config) {
                    Ok(id) => id,
                    Err(e) => {
                        self.terminal.append_output(&format!("[错误] {}", e))?;
                        return Ok(());
                    }
                };
                // 启动渲染定时器（非实时模式且配置了 render_interval > 0）
                if !conn_config.realtime && conn_config.render_interval > 0 {
                    self.start_render_tick_timer(session_id, conn_config.render_interval);
                }
                self.update_status_bar()?;
                let _ = self.connect_tx.try_send(ConnectRequest { session_id });
                let display_pos = self.manager.display_number_of(session_id);
                self.terminal.append_output(&format!(
                    "[系统] 正在连接 {} ({}) → {}:{}",
                    display_pos,
                    name,
                    host,
                    port
                ))?;
            }

            BuiltinCommand::Disconnect { id } => {
                if let Some(id) = id {
                    if let Some(session_id) = self.manager.session_id_by_display_number(id) {
                        if let Some(session) = self.manager.get_mut_by_id(session_id) {
                            session.disconnect();
                            session.state = crate::connection::SessionState::Disconnected;
                        }
                        let name = self.manager.get_by_id(session_id).map(|s| s.name.clone()).unwrap_or_default();
                        self.update_status_bar()?;
                        self.terminal
                            .append_output(&format!("[系统] 已断开连接 {} ({})", id, name))?;
                    } else {
                        self.terminal
                            .append_output(&format!("[错误] 连接 {} 不存在", id))?;
                    }
                } else {
                    let fg_id = self.manager.foreground_id;
                    if self.manager.get_by_id(fg_id).is_some() {
                        if let Some(session) = self.manager.get_mut_by_id(fg_id) {
                            session.disconnect();
                            session.state =
                                crate::connection::SessionState::Disconnected;
                        }
                        self.update_status_bar()?;
                        let name = self.manager.get_by_id(fg_id).map(|s| s.name.clone()).unwrap_or_default();
                        let display_pos = self.manager.display_number_of(fg_id);
                        self.terminal.append_output(&format!(
                            "[系统] 已断开连接 {} ({})",
                            display_pos, name
                        ))?;
                    }
                }
            }

            BuiltinCommand::Reconnect { id } => {
                let session_id = if let Some(id) = id {
                    if let Some(sid) = self.manager.session_id_by_display_number(id) {
                        sid
                    } else {
                        self.terminal
                            .append_output(&format!("[错误] 连接 {} 不存在", id))?;
                        return Ok(());
                    }
                } else {
                    self.manager.foreground_id
                };
                if self.manager.get_by_id(session_id).is_none() {
                    self.terminal.append_output("[错误] 连接不存在")?;
                    return Ok(());
                }
                let name = self.manager.get_by_id(session_id).map(|s| s.name.clone()).unwrap_or_default();
                if let Some(session) = self.manager.get_mut_by_id(session_id) {
                    session.disconnect();
                    session.state = crate::connection::SessionState::Disconnected;
                }
                let display_pos = self.manager.display_number_of(session_id);
                self.terminal.append_output(&format!(
                    "[系统] 正在重连 {} ({})...",
                    display_pos,
                    name
                ))?;
                self.update_status_bar()?;
                let _ = self
                    .reconnect_tx
                    .try_send(ReconnectRequest { session_id });
            }

            BuiltinCommand::Close { id } => {
                let session_id = if let Some(id) = id {
                    if let Some(sid) = self.manager.session_id_by_display_number(id) {
                        sid
                    } else {
                        self.terminal
                            .append_output(&format!("[错误] 连接 {} 不存在", id))?;
                        return Ok(());
                    }
                } else {
                    self.manager.foreground_id
                };
                let display_pos = self.manager.display_number_of(session_id);
                // 清理定时器：停止目标 session 的渲染刷新定时器
                self.stop_render_tick_timer(session_id);

                match self.manager.remove_session(session_id) {
                    Ok(name) => {
                        self.update_status_bar()?;
                        if self.manager.session_count() > 0 {
                            self.switch_foreground(self.manager.foreground_id)?;
                        } else {
                            self.terminal.replace_output(&Vec::new())?;
                        }
                        self.terminal.append_output(&format!(
                            "[系统] 已关闭连接 {} ({})",
                            display_pos,
                            name
                        ))?;
                    }
                    Err(e) => {
                        self.terminal.append_output(&format!("[错误] {}", e))?;
                    }
                }
            }

            BuiltinCommand::List => {
                for &sid in self.manager.ordered_session_ids() {
                    if let Some(s) = self.manager.get_by_id(sid) {
                        let state_str = match s.state {
                            crate::connection::SessionState::Connected => "已连接",
                            crate::connection::SessionState::Disconnected => "已断开",
                            crate::connection::SessionState::Connecting => "连接中...",
                            crate::connection::SessionState::Reconnecting => "重连中...",
                        };
                        let marker = if sid == self.manager.foreground_id {
                            "★"
                        } else {
                            " "
                        };
                        let display_num = self.manager.display_number_of(sid);
                        self.terminal.append_output(&format!(
                            "{} [{}] {} - {}",
                            marker,
                            display_num,
                            s.name,
                            state_str
                        ))?;
                    }
                }
            }

            BuiltinCommand::Load { path } => {
                let fg_id = self.manager.foreground_id;
                if self.manager.get_by_id(fg_id).is_none() {
                    self.terminal.append_output("[错误] 无前台连接")?;
                    return Ok(());
                }
                match crate::lua::LuaEngine::new() {
                    Ok(mut engine) => match engine.load_script(&path) {
                        Ok(()) => {
                            if let Some(session) = self.manager.get_mut_by_id(fg_id) {
                                session.lua_engine = Some(engine);
                            }
                            self.terminal.append_output(&format!(
                                "\x1b[36m[Lua] 脚本已加载: {}\x1b[0m",
                                path
                            ))?;
                            self.start_timers_for_session(fg_id);
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
                let fg_id = self.manager.foreground_id;
                if self.manager.get_by_id(fg_id).is_none() {
                    self.terminal.append_output("[错误] 无前台连接")?;
                    return Ok(());
                }
                let script_path = self.manager.get_by_id(fg_id)
                    .and_then(|s| s.lua_engine.as_ref())
                    .and_then(|e| e.script_path());
                // 保存原 engine 的变量（如 char_name 等）
                let saved_vars = self.manager.get_by_id(fg_id)
                    .and_then(|s| s.lua_engine.as_ref())
                    .map(|e| e.get_variables());
                // 保存原 engine 的连接状态
                let saved_conn_state = self.manager.get_by_id(fg_id)
                    .and_then(|s| s.lua_engine.as_ref())
                    .map(|e| e.get_connection_state());
                let fg_name = self.manager.get_by_id(fg_id).map(|s| s.name.clone()).unwrap_or_default();
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
                                    // 排空 Lua 日志（drain_lua_logs 会处理日志写入和终端输出）
                                    if let Some(session) = self.manager.get_mut_by_id(fg_id) {
                                        session.lua_engine = Some(engine);
                                    }
                                    self.drain_lua_logs(fg_id)?;
                                    self.terminal.append_output(&format!(
                                        "\x1b[36m[Lua] 脚本已重新加载: {}\x1b[0m",
                                        path
                                    ))?;
                                    self.start_timers_for_session(fg_id);
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
                                    for line in format_lua_error(&err_msg) {
                                        self.logger.log_debug(&fg_name, &line);
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
                let fg_id = self.manager.foreground_id;
                if self.manager.get_by_id(fg_id).is_none() {
                    self.terminal.append_output("[错误] 无前台连接")?;
                    return Ok(());
                }
                let name = self.manager.get_by_id(fg_id).map(|s| s.name.clone()).unwrap_or_default();
                self.logger.log_lua(&name, &code);
                if let Some(ref engine) = self.manager.get_by_id(fg_id).and_then(|s| s.lua_engine.as_ref()) {
                    match engine.eval_code(&code) {
                        Ok(_) => {
                            let commands = engine.drain_commands();
                            self.send_lua_commands(fg_id, commands)?;
                            self.send_lua_raw(fg_id)?;
                            self.drain_lua_logs(fg_id)?;
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
                "render_interval" => {
                    let fg_id = self.manager.foreground_id;
                    if self.manager.get_by_id(fg_id).is_none() {
                        self.terminal.append_output("[错误] 无前台连接")?;
                        return Ok(());
                    }
                    match value.parse::<u64>() {
                        Ok(ms) => {
                            // 限制范围：[50, 10000]ms
                            let clamped = ms.max(50).min(10000);
                            let is_realtime = self.manager.get_by_id(fg_id).map(|s| s.realtime).unwrap_or(false);
                            if let Some(session) = self.manager.get_mut_by_id(fg_id) {
                                session.render_interval = clamped;
                            }
                            // 仅在非实时模式下重启定时器
                            if !is_realtime {
                                self.start_render_tick_timer(fg_id, clamped);
                            }
                            self.terminal.append_output(&format!(
                                "[系统] 渲染间隔已设置为: {}ms (当前连接)",
                                clamped
                            ))?;
                        }
                        Err(_) => {
                            self.terminal.append_output(&format!(
                                "[错误] render_interval 必须是正整数（毫秒），当前值: {}",
                                value
                            ))?;
                        }
                    }
                }
                "realtime" => {
                    let fg_id = self.manager.foreground_id;
                    if self.manager.get_by_id(fg_id).is_none() {
                        self.terminal.append_output("[错误] 无前台连接")?;
                        return Ok(());
                    }
                    let enabled = matches!(value.as_str(), "on" | "1" | "true" | "yes");
                    if let Some(session) = self.manager.get_mut_by_id(fg_id) {
                        session.realtime = enabled;
                    }
                    // 根据新模式调整定时器
                    if enabled {
                        // 实时模式：停止定时器
                        self.stop_render_tick_timer(fg_id);
                    } else {
                        // 节流模式：启动定时器
                        let interval = self.manager.get_by_id(fg_id).map(|s| s.render_interval).unwrap_or(0);
                        if interval > 0 {
                            self.start_render_tick_timer(fg_id, interval);
                        }
                    }
                    let status = if enabled { "实时" } else { "节流" };
                    self.terminal.append_output(&format!(
                        "[系统] 渲染模式已切换为: {} (当前连接)",
                        status
                    ))?;
                }
                _ => {
                    self.terminal.append_output(&format!(
                        "[错误] 未知设置选项: {}。可用选项: keep_command, render_interval, realtime",
                        option
                    ))?;
                }
            },

            BuiltinCommand::Switch { target } => {
                // 尝试解析为数字
                if let Ok(id) = target.parse::<usize>() {
                    if let Some(session_id) = self.manager.session_id_by_display_number(id) {
                        self.switch_foreground(session_id)?;
                        let name = self.manager.get_by_id(session_id).map(|s| s.name.clone()).unwrap_or_default();
                        self.terminal.append_output(&format!(
                            "[系统] 已切换到连接 {} ({})",
                            id, name
                        ))?;
                    } else {
                        self.terminal
                            .append_output(&format!("[错误] 连接 {} 不存在", id))?;
                    }
                } else {
                    // 按名称查找
                    let target_name = target.to_lowercase();
                    if let Some(&session_id) = self.manager.ordered_session_ids().iter().find(|&&sid| {
                        self.manager.get_by_id(sid).map(|s| s.name.to_lowercase() == target_name).unwrap_or(false)
                    }) {
                        self.switch_foreground(session_id)?;
                        let display_num = self.manager.display_number_of(session_id);
                        let name = self.manager.get_by_id(session_id).map(|s| s.name.clone()).unwrap_or_default();
                        self.terminal.append_output(&format!(
                            "[系统] 已切换到连接 {} ({})",
                            display_num,
                            name
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
                                let loaded = self.manager.ordered_session_ids().iter().any(|&sid| self.manager.get_by_id(sid).map(|s| s.name == p.name).unwrap_or(false));
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

                    let session_id = match self.manager.add_connection_dynamic(&conn_config) {
                        Ok(id) => id,
                        Err(e) => {
                            self.terminal.append_output(&format!("[错误] {}", e))?;
                            return Ok(());
                        }
                    };

                    // 启动渲染定时器（非实时模式且配置了 render_interval > 0）
                    if !conn_config.realtime && conn_config.render_interval > 0 {
                        self.start_render_tick_timer(session_id, conn_config.render_interval);
                    }

                    // 设置日志保留数量
                    if let Some(count) = conn_config.log_rotation_count {
                        self.logger.set_session_max_files(&conn_config.name, count);
                    }

                    self.update_status_bar()?;
                    let _ = self.connect_tx.try_send(ConnectRequest { session_id });
                    self.terminal.append_output(&format!(
                        "[系统] 正在从配置文件加载角色 '{}' 并连接 ({}:{})",
                        conn_config.name, conn_config.host, conn_config.port
                    ))?;
                }
            },

            BuiltinCommand::All { cmd } => {
                // 判断是否为客户端命令（以 / 开头）
                if cmd.starts_with('/') {
                    self.handle_all_client_command(&cmd)?;
                } else {
                    // 普通命令，直接发送到所有连接的服务器
                    let results = self.manager.send_to_all(&cmd);
                    let count = results.len();
                    let mut ok_count = 0;
                    for (_session_id, name, result) in &results {
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
                    .append_output("  /reconnect [编号]           断开并重新连接")?;
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
                    .append_output("  /set realtime on|off          实时/节流渲染模式切换")?;
                self.terminal.append_output(
                    "  /set render_interval <毫秒>  设置渲染间隔（0=实时，默认1000）",
                )?;
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
                if self.manager.get_by_id(id).is_none() {
                    return Ok(());
                }
                // 将数据追加到对应连接的输出缓冲区
                {
                    let max_lines = self.config.general.scroll_buffer;
                    if let Some(session) = self.manager.get_mut_by_id(id) {
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
                }
                // 仅渲染前台连接的数据
                let is_realtime = self.manager.get_by_id(id).map(|s| s.realtime).unwrap_or(false);
                if id == self.manager.foreground_id {
                    if is_realtime {
                        // 实时渲染模式
                        self.terminal.append_output(&data)?;
                    } else {
                        // 节流渲染模式：缓冲数据，等待定时器刷新
                        if let Some(session) = self.manager.get_mut_by_id(id) {
                            for part in data.split_inclusive('\n') {
                                let trimmed = part.trim_end_matches(['\r', '\n']);
                                if !trimmed.is_empty() {
                                    session.pending_data.push(trimmed.to_string());
                                }
                            }
                            session.render_dirty = true;
                        }
                    }
                }
                // 所有连接数据写入日志
                self.log_session_data(id, &data);

                // 触发器处理（所有连接都触发，不仅仅是前台）
                {
                    let mut pending_lua_logs = Vec::new();
                    let trigger_commands =
                        if let Some(ref engine) = self.manager.get_by_id(id).and_then(|s| s.lua_engine.as_ref()) {
                            // 对每行数据分别匹配触发器
                            let mut all_cmds = Vec::new();
                            for part in data.split_inclusive('\n') {
                                let trimmed = part.trim_end_matches(['\r', '\n']);
                                if !trimmed.is_empty() {
                                    engine.process_output(trimmed);
                                    all_cmds.extend(engine.drain_commands());
                                }
                            }
                            // 收集 Lua 日志（写入文件 + 暂存待终端输出）
                            let logs = engine.drain_logs();
                            let name = self.manager.get_by_id(id).map(|s| s.name.clone()).unwrap_or_default();
                            for msg in &logs {
                                let clean = crate::ui::AnsiParser::strip_ansi(msg);
                                self.logger.log(&name, &clean);
                            }
                            pending_lua_logs = logs;
                            all_cmds
                        } else {
                            Vec::new()
                        };
                    // 发送触发器产生的命令
                    self.send_lua_commands(id, trigger_commands)?;
                    // 处理 Lua 日志（写入日志文件已在上面的分支中完成）
                    // 节流模式下缓冲到 pending_data，实时模式直接输出
                    if !pending_lua_logs.is_empty() && id == self.manager.foreground_id {
                        if !is_realtime {
                            if let Some(session) = self.manager.get_mut_by_id(id) {
                                for msg in pending_lua_logs {
                                    session.pending_data.push(format!("\x1b[36m[Lua] {}\x1b[0m", msg));
                                }
                                session.render_dirty = true;
                            }
                        } else {
                            for msg in pending_lua_logs {
                                self.terminal
                                    .append_output(&format!("\x1b[36m[Lua] {}\x1b[0m", msg))?;
                            }
                        }
                    }
                    // 排空引擎中剩余的 Lua 日志（触发器处理后又产生的）
                    self.drain_lua_logs(id)?;
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
                if self.manager.get_by_id(id).is_none() {
                    return Ok(());
                }
                if let Some(session) = self.manager.get_mut_by_id(id) {
                    session.state = state.clone();
                    // 同步 Lua 引擎的连接状态（同步到对应 session，不限于前台）
                    if let Some(ref mut engine) = session.lua_engine {
                        engine.set_connected(matches!(state, SessionState::Connected));
                    }
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
                let name = self.manager.get_by_id(id).map(|s| s.name.clone()).unwrap_or_default();
                let display_pos = self.manager.display_number_of(id);
                self.terminal.append_output(&format!(
                    "[系统] 连接 {} ({}) {}",
                    display_pos,
                    name,
                    state_str
                ))?;

                // 自动重连：断开时启动延迟重连任务
                if state == SessionState::Disconnected {
                    let (auto_reconnect, delay) = self.manager.get_by_id(id)
                        .map(|s| (s.auto_reconnect, s.reconnect_delay_secs))
                        .unwrap_or((false, 5));
                    if auto_reconnect {
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
                let name = self.manager.get_by_id(id).map(|s| s.name.clone()).unwrap_or_else(|| "未知".to_string());
                let display_pos = self.manager.display_number_of(id);
                self.terminal.append_output(&format!(
                    "[错误] 连接 {} ({}): {}",
                    display_pos,
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
    fn log_session_data(&self, session_id: SessionId, data: &str) {
        let name = match self.manager.get_by_id(session_id) {
            Some(s) => &s.name,
            None => return,
        };
        let clean = AnsiParser::strip_ansi(data);
        self.logger.log(name, &clean);
    }

    /// 切换前台连接，恢复目标连接的输出缓冲区
    fn switch_foreground(&mut self, session_id: SessionId) -> io::Result<()> {
        // 保存当前前台 session 的输入状态
        let old_id = self.manager.foreground_id;
        if self.manager.get_by_id(old_id).is_some() {
            let saved = self.terminal.save_input_state();
            if let Some(session) = self.manager.get_mut_by_id(old_id) {
                session.input_state = saved;
            }
        }

        self.manager.switch_foreground(session_id).ok();
        self.update_status_bar()?;

        // 恢复目标 session 的输入状态
        if let Some(saved) = self.manager.get_by_id(session_id).map(|s| s.input_state.clone()) {
            self.terminal.restore_input_state(&saved);
        }

        // 恢复目标连接的输出缓冲区到终端
        // 预提取 display_pos 和 foreground_name，避免后续与 terminal 借用冲突
        let display_pos = self.manager.display_number_of(session_id);
        let fg_name = self.manager.foreground_name().to_string();
        // 拆分借用：manager（不可变）和 terminal（可变）是 App 的不同字段
        let empty = Vec::new();
        let output: &[String] = self.manager.get_by_id(session_id)
            .map(|s| s.output_lines.as_slice())
            .unwrap_or(&empty);
        self.terminal.replace_output(output)?;
        self.terminal.append_output(&format!(
            "[系统] 切换到连接 {} ({})",
            display_pos,
            fg_name
        ))?;
        Ok(())
    }

    /// 处理 /all 后的客户端命令（以 / 开头），逐 session 执行
    fn handle_all_client_command(&mut self, cmd: &str) -> io::Result<()> {
        let inner = cmd.strip_prefix('/').unwrap_or("");
        let parts: Vec<&str> = inner.split_whitespace().collect();
        if parts.is_empty() {
            return Ok(());
        }

        let safe = match parts[0] {
            "lua" | "reload" | "disconnect" | "reconnect" => true,
            "load" if parts.len() >= 2 => true,
            "list" => true,
            _ => false,
        };

        if !safe {
            self.terminal.append_output(&format!(
                "[错误] /all 不允许广播客户端命令 /{}。允许: /lua, /reload, /load, /list, /disconnect, /reconnect",
                parts[0]
            ))?;
            return Ok(());
        }

        let session_count = self.manager.session_count();
        let session_ids: Vec<SessionId> = self.manager.ordered_session_ids().to_vec();

        match parts[0] {
            "lua" => {
                let code = inner.strip_prefix("lua").map(|s| s.trim()).unwrap_or("");
                if code.is_empty() {
                    self.terminal
                        .append_output("[错误] /all /lua 需要 Lua 代码参数")?;
                    return Ok(());
                }
                let mut executed = 0usize;
                let mut skipped = 0usize;
                for &sid in &session_ids {
                    let name = self.manager.get_by_id(sid).map(|s| s.name.clone()).unwrap_or_default();
                    if let Some(ref engine) = self.manager.get_by_id(sid).and_then(|s| s.lua_engine.as_ref()) {
                        self.logger.log_lua(&name, code);
                        match engine.eval_code(code) {
                            Ok(_) => {
                                let _ = self.send_lua_commands(sid, engine.drain_commands());
                                let _ = self.send_lua_raw(sid);
                                let _ = self.drain_lua_logs(sid);
                                executed += 1;
                            }
                            Err(e) => {
                                self.terminal.append_output(&format!(
                                    "[错误] /all /lua [{}]: {}",
                                    name, e
                                ))?;
                            }
                        }
                    } else {
                        self.terminal
                            .append_output(&format!("[错误] /all /lua [{}]: 未加载脚本", name))?;
                        skipped += 1;
                    }
                }
                self.update_status_bar()?;
                let extra = if skipped > 0 {
                    format!("，{} 个未加载脚本被跳过", skipped)
                } else {
                    String::new()
                };
                self.terminal.append_output(&format!(
                    "[系统] /all /lua: 在 {}/{} 个连接上执行{}",
                    executed, session_count, extra
                ))?;
            }
            "reload" | "load" => {
                let is_reload =
                    parts[0] == "reload" || parts.get(1).is_some_and(|&p| p == "reload");
                let mut executed = 0usize;
                for &sid in &session_ids {
                    let name = self.manager.get_by_id(sid).map(|s| s.name.clone()).unwrap_or_default();
                    if is_reload {
                        let path = self.manager.get_by_id(sid)
                            .and_then(|s| s.lua_engine.as_ref())
                            .and_then(|e| e.script_path());
                        if let Some(path) = path {
                            let saved_vars = self.manager.get_by_id(sid)
                                .and_then(|s| s.lua_engine.as_ref())
                                .map(|e| e.get_variables());
                            let saved_conn = self.manager.get_by_id(sid)
                                .and_then(|s| s.lua_engine.as_ref())
                                .map(|e| e.get_connection_state());
                            match crate::lua::LuaEngine::new() {
                                Ok(mut engine) => {
                                    if let Some(ref vars) = saved_vars {
                                        for (k, v) in vars {
                                            engine.set_variable(k, v);
                                            engine.set_global(k, v);
                                        }
                                    }
                                    if let Some(ref conn) = saved_conn {
                                        engine.restore_connection_state(conn);
                                    }
                                    match engine.load_script(&path) {
                                        Ok(()) => {
                                            // 排空脚本加载期间的 Lua 日志
                                            if let Some(session) = self.manager.get_mut_by_id(sid) {
                                                session.lua_engine = Some(engine);
                                            }
                                            self.drain_lua_logs(sid)?;
                                            executed += 1;
                                        }
                                        Err(e) => {
                                            self.terminal.append_output(&format!(
                                                "[错误] /all /reload [{}]: {}",
                                                name, e
                                            ))?;
                                        }
                                    }
                                }
                                Err(e) => {
                                    self.terminal.append_output(&format!(
                                        "[错误] /all /reload [{}]: {}",
                                        name, e
                                    ))?;
                                }
                            }
                        } else {
                            self.terminal.append_output(&format!(
                                "[错误] /all /reload [{}]: 无已加载脚本",
                                name
                            ))?;
                        }
                    } else {
                        let path = parts[1].to_string();
                        match crate::lua::LuaEngine::new() {
                            Ok(mut engine) => match engine.load_script(&path) {
                                Ok(()) => {
                                    if let Some(session) = self.manager.get_mut_by_id(sid) {
                                        session.lua_engine = Some(engine);
                                    }
                                    self.start_timers_for_session(sid);
                                    executed += 1;
                                }
                                Err(e) => {
                                    self.terminal.append_output(&format!(
                                        "[错误] /all /load [{}]: {}",
                                        name, e
                                    ))?;
                                }
                            },
                            Err(e) => {
                                self.terminal.append_output(&format!(
                                    "[错误] /all /load [{}]: {}",
                                    name, e
                                ))?;
                            }
                        }
                    }
                }
                if is_reload && executed > 0 {
                    for &sid in &session_ids {
                        self.start_timers_for_session(sid);
                    }
                }
                self.terminal.append_output(&format!(
                    "[系统] /all /{}: 在 {}/{} 个连接上执行",
                    parts[0], executed, session_count
                ))?;
                self.update_status_bar()?;
            }
            "list" => {
                return self.handle_builtin_command("/list");
            }
            "disconnect" => {
                for &sid in &session_ids {
                    if let Some(session) = self.manager.get_mut_by_id(sid) {
                        session.disconnect();
                        session.state = crate::connection::SessionState::Disconnected;
                    }
                }
                self.update_status_bar()?;
                self.terminal.append_output(&format!(
                    "[系统] /all /disconnect: 已断开 {} 个连接",
                    session_count
                ))?;
            }
            "reconnect" => {
                for &sid in &session_ids {
                    let name = self.manager.get_by_id(sid).map(|s| s.name.clone()).unwrap_or_default();
                    if let Some(session) = self.manager.get_mut_by_id(sid) {
                        session.disconnect();
                        session.state = crate::connection::SessionState::Disconnected;
                    }
                    let display_pos = self.manager.display_number_of(sid);
                    self.terminal.append_output(&format!(
                        "[系统] 正在重连 {} ({})...",
                        display_pos,
                        name
                    ))?;
                    let _ = self
                        .reconnect_tx
                        .try_send(ReconnectRequest { session_id: sid });
                }
                self.update_status_bar()?;
            }
            _ => unreachable!(),
        }
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
    fn test_parse_builtin_reconnect_with_id() {
        let cmd = parse_builtin_command("/reconnect 2");
        assert_eq!(cmd, BuiltinCommand::Reconnect { id: Some(2) });
    }

    #[test]
    fn test_parse_builtin_reconnect_no_id() {
        let cmd = parse_builtin_command("/reconnect");
        assert_eq!(cmd, BuiltinCommand::Reconnect { id: None });
    }

    #[test]
    fn test_parse_builtin_reconnect_invalid_id() {
        let cmd = parse_builtin_command("/reconnect abc");
        assert_eq!(cmd, BuiltinCommand::Reconnect { id: None });
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
    fn test_parse_builtin_set_realtime_on() {
        let cmd = parse_builtin_command("/set realtime on");
        assert_eq!(
            cmd,
            BuiltinCommand::Set {
                option: "realtime".to_string(),
                value: "on".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_builtin_set_realtime_off() {
        let cmd = parse_builtin_command("/set realtime off");
        assert_eq!(
            cmd,
            BuiltinCommand::Set {
                option: "realtime".to_string(),
                value: "off".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_builtin_set_render_interval() {
        let cmd = parse_builtin_command("/set render_interval 500");
        assert_eq!(
            cmd,
            BuiltinCommand::Set {
                option: "render_interval".to_string(),
                value: "500".to_string(),
            }
        );
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
