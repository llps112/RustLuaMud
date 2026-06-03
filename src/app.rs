use std::io;

use crossterm::event::{Event as CrosstermEvent, EventStream, KeyCode, KeyModifiers};
use futures::StreamExt;
use tokio::sync::mpsc;

use crate::config::AppConfig;
use crate::connection::{ConnectionManager, ManagerEvent, SessionState};
use crate::log::Logger;
use crate::ui::{AnsiParser, Terminal};

/// 重连请求
struct ReconnectRequest {
    session_id: usize,
}

/// 动态连接请求
struct ConnectRequest {
    session_id: usize,
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
}

impl App {
    pub fn new(config: AppConfig) -> io::Result<Self> {
        let mut manager = ConnectionManager::new();
        for conn_config in &config.connections {
            manager.add_connection(conn_config);
        }

        let terminal = Terminal::new()?;
        let logger = Logger::new(
            &config.general.log_dir,
            config.general.log_rotation_size_mb,
            config.general.log_rotation_count,
        );
        let (reconnect_tx, reconnect_rx) = mpsc::channel(32);
        let (connect_tx, connect_rx) = mpsc::channel(16);

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
        })
    }

    /// 启动应用主循环
    pub async fn run(&mut self) -> io::Result<()> {
        self.terminal.init_screen()?;

        // 自动连接所有 auto_connect 的连接
        for (id, conn_config) in self.config.connections.iter().enumerate() {
            if conn_config.auto_connect {
                match self.manager.connect_session(id).await {
                    Ok(()) => {
                        let msg = format!("[系统] 连接 {} ({}) 已建立", id + 1, conn_config.name);
                        self.terminal.append_output(&msg)?;
                    }
                    Err(e) => {
                        let msg = format!("[系统] 连接 {} ({}) 失败: {}", id + 1, conn_config.name, e);
                        self.terminal.append_output(&msg)?;
                    }
                }
            }
        }

        self.update_status_bar()?;

        // 获取管理器事件接收器
        let mut mgr_rx = self.manager.take_event_rx()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "无法获取事件通道"))?;

        // 终端事件流
        let mut term_events = EventStream::new();

        // 主事件循环
        while self.running {
            tokio::select! {
                // 处理终端键盘事件
                Some(Ok(event)) = term_events.next() => {
                    if let CrosstermEvent::Key(key) = event {
                        self.handle_key_event(key)?;
                    } else if let CrosstermEvent::Resize(w, h) = event {
                        self.terminal.resize(w, h);
                        self.update_status_bar()?;
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
                let msg = format!("[系统] 连接 {} ({}) → {}:{} 已建立", session_id + 1, name, host, port);
                self.terminal.append_output(&msg)?;
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

    /// 处理键盘事件
    fn handle_key_event(&mut self, key: crossterm::event::KeyEvent) -> io::Result<()> {
        // Ctrl+C / Ctrl+D: 退出
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && (key.code == KeyCode::Char('c') || key.code == KeyCode::Char('d'))
        {
            self.running = false;
            return Ok(());
        }

        // Alt+1~9: 切换连接
        if key.modifiers.contains(KeyModifiers::ALT) {
            if let KeyCode::Char(c) = key.code {
                if let Some(digit) = c.to_digit(10) {
                    let id = (digit as usize) - 1;
                    if id < self.manager.sessions.len() {
                        self.switch_foreground(id)?;
                    }
                    return Ok(());
                }
            }
        }

        // 其他按键交给终端处理
        if let Some(cmd) = self.terminal.handle_key(key) {
            // 用户按了 Enter，提交命令
            if !cmd.is_empty() {
                self.terminal.append_output(&format!("> {}", cmd))?;
                // 处理内置命令（以 / 开头）
                if cmd.starts_with('/') {
                    self.handle_builtin_command(&cmd)?;
                } else {
                    // 发送到前台连接
                    if let Err(e) = self.manager.send_to_foreground(&cmd) {
                        self.terminal.append_output(&format!("[错误] {}", e))?;
                    }
                }
            }
        }

        Ok(())
    }

    /// 处理内置命令
    fn handle_builtin_command(&mut self, cmd: &str) -> io::Result<()> {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() { return Ok(()); }

        match parts[0] {
            "/connect" => {
                // /connect 名字 主机:端口  或 /connect 名字 主机 端口
                if parts.len() < 3 {
                    self.terminal.append_output(
                        "[用法] /connect <名称> <主机:端口>  或  /connect <名称> <主机> <端口>"
                    )?;
                    return Ok(());
                }
                let name = parts[1];
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

                // 创建临时配置并添加连接
                let conn_config = crate::config::ConnectionConfig {
                    name: name.to_string(),
                    host: host.to_string(),
                    port,
                    encoding: Some("gbk".to_string()),
                    script: None,
                    auto_connect: false,
                    auto_reconnect: true,
                    reconnect_delay_secs: 5,
                };

                let id = self.manager.add_connection_dynamic(&conn_config);
                self.update_status_bar()?;
                // 通过 channel 发送异步连接请求
                let _ = self.connect_tx.try_send(ConnectRequest {
                    session_id: id,
                });
                self.terminal.append_output(
                    &format!("[系统] 正在连接 {} ({}) → {}:{}", id + 1, name, host, port)
                )?;
            }

            "/disconnect" => {
                if parts.len() >= 2 {
                    if let Ok(id) = parts[1].parse::<usize>() {
                        if id > 0 && id <= self.manager.sessions.len() {
                            let target_id = id - 1;
                            self.manager.sessions[target_id].disconnect();
                            let name = self.manager.sessions[target_id].name.clone();
                            self.manager.sessions[target_id].state =
                                crate::connection::SessionState::Disconnected;
                            self.update_status_bar()?;
                            self.terminal.append_output(
                                &format!("[系统] 已断开连接 {} ({})", id, name)
                            )?;
                        } else {
                            self.terminal.append_output(
                                &format!("[错误] 连接 {} 不存在", id)
                            )?;
                        }
                        return Ok(());
                    }
                }
                // 无参数：断开前台连接
                let fg = self.manager.foreground_id;
                if fg < self.manager.sessions.len() {
                    self.manager.sessions[fg].disconnect();
                    self.manager.sessions[fg].state =
                        crate::connection::SessionState::Disconnected;
                    self.update_status_bar()?;
                    self.terminal.append_output(
                        &format!("[系统] 已断开连接 {} ({})",
                            fg + 1, self.manager.sessions[fg].name)
                    )?;
                }
            }

            "/close" => {
                let target = if parts.len() >= 2 {
                    match parts[1].parse::<usize>() {
                        Ok(id) => id - 1,
                        Err(_) => {
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
                        // 如果移除的是前台连接，切换到新的前台
                        if !self.manager.sessions.is_empty() {
                            self.switch_foreground(self.manager.foreground_id)?;
                        } else {
                            self.terminal.replace_output(&Vec::new())?;
                        }
                        self.terminal.append_output(
                            &format!("[系统] 已关闭连接 {} ({})", target + 1, name)
                        )?;
                    }
                    Err(e) => {
                        self.terminal.append_output(&format!("[错误] {}", e))?;
                    }
                }
            }

            "/list" => {
                for (i, s) in self.manager.sessions.iter().enumerate() {
                    let state_str = match s.state {
                        crate::connection::SessionState::Connected => "已连接",
                        crate::connection::SessionState::Disconnected => "已断开",
                        crate::connection::SessionState::Connecting => "连接中...",
                        crate::connection::SessionState::Reconnecting => "重连中...",
                    };
                    let marker = if i == self.manager.foreground_id { "★" } else { " " };
                    self.terminal.append_output(
                        &format!("{} [{}] {} - {}", marker, i + 1, s.name, state_str)
                    )?;
                }
            }

            "/help" | _ => {
                self.terminal.append_output("内置命令:")?;
                self.terminal.append_output("  /connect <名> <主机:端口>   添加并连接新角色")?;
                self.terminal.append_output("  /connect <名> <主机> <端口> 同上")?;
                self.terminal.append_output("  /disconnect [编号]           断开连接（保留 session）")?;
                self.terminal.append_output("  /close [编号]               彻底关闭并移除 session")?;
                self.terminal.append_output("  /list                       列出所有连接")?;
                self.terminal.append_output("  Alt+1~9                     切换前台连接")?;
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
                    for part in data.split_inclusive(|c| c == '\n') {
                        let trimmed = part.trim_end_matches('\r').trim_end_matches('\n');
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
            }
            ManagerEvent::StateChange(id, state) => {
                if id < self.manager.sessions.len() {
                    self.manager.sessions[id].state = state.clone();
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
                let name = if id < self.manager.sessions.len() {
                    &self.manager.sessions[id].name
                } else {
                    "未知"
                };
                self.terminal.append_output(
                    &format!("[系统] 连接 {} ({}) {}", id + 1, name, state_str)
                )?;

                // 自动重连：断开时启动延迟重连任务
                if state == SessionState::Disconnected {
                    let session = if id < self.manager.sessions.len() {
                        &self.manager.sessions[id]
                    } else {
                        return Ok(());
                    };
                    if session.auto_reconnect {
                        let delay = session.reconnect_delay_secs;
                        self.terminal.append_output(
                            &format!("[系统] {} 秒后尝试重连 {}...", delay, name)
                        )?;
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
                self.terminal.append_output(
                    &format!("[错误] 连接 {} ({}): {}", id + 1, name, err)
                )?;
            }
        }
        Ok(())
    }

    /// 更新状态栏
    fn update_status_bar(&mut self) -> io::Result<()> {
        let mut stdout = io::stdout();
        let infos = self.manager.session_infos();
        self.terminal.draw_status_bar(
            &mut stdout,
            &infos,
            self.manager.foreground_id,
        )?;
        Ok(())
    }

    /// 请求退出
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
        self.manager.switch_foreground(id).ok();
        self.update_status_bar()?;
        // 恢复目标连接的输出缓冲区到终端
        let output = if id < self.manager.sessions.len() {
            &self.manager.sessions[id].output_lines
        } else {
            &Vec::new()
        };
        self.terminal.replace_output(output)?;
        self.terminal.append_output(
            &format!("[系统] 切换到连接 {} ({})",
                id + 1, self.manager.foreground_name())
        )?;
        Ok(())
    }
}
