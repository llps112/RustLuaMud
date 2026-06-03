use std::io;

use crossterm::event::{Event as CrosstermEvent, EventStream, KeyCode, KeyModifiers};
use futures::StreamExt;

use crate::config::AppConfig;
use crate::connection::{ConnectionManager, ManagerEvent, SessionState};
use crate::ui::{AnsiParser, Terminal};

/// 应用主结构
pub struct App {
    config: AppConfig,
    terminal: Terminal,
    manager: ConnectionManager,
    running: bool,
}

impl App {
    pub fn new(config: AppConfig) -> io::Result<Self> {
        let mut manager = ConnectionManager::new();
        for conn_config in &config.connections {
            manager.add_connection(conn_config);
        }

        let terminal = Terminal::new()?;

        Ok(Self {
            config,
            terminal,
            manager,
            running: true,
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
                        self.manager.switch_foreground(id).ok();
                        self.update_status_bar()?;
                        // 重绘前台连接的输出缓冲区
                        self.terminal.append_output(
                            &format!("[系统] 切换到连接 {} ({})",
                                id + 1, self.manager.foreground_name())
                        )?;
                    }
                    return Ok(());
                }
            }
        }

        // 其他按键交给终端处理
        if let Some(cmd) = self.terminal.handle_key(key) {
            // 用户按了 Enter，发送命令
            if !cmd.is_empty() {
                // 在输出区回显用户输入
                self.terminal.append_output(&format!("> {}", cmd))?;
                // 发送到前台连接
                if let Err(e) = self.manager.send_to_foreground(&cmd) {
                    self.terminal.append_output(&format!("[错误] {}", e))?;
                }
            }
        }

        Ok(())
    }

    /// 处理连接管理器事件
    fn handle_manager_event(&mut self, event: ManagerEvent) -> io::Result<()> {
        match event {
            ManagerEvent::Data(id, data) => {
                // 仅渲染前台连接的数据
                if id == self.manager.foreground_id {
                    let clean = AnsiParser::strip_ansi(&data);
                    self.terminal.append_output(&clean)?;
                }
                // 后台连接数据暂不渲染，Phase 4 加入日志
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

                // 自动重连逻辑
                if state == SessionState::Disconnected {
                    let should_reconnect = if id < self.config.connections.len() {
                        self.config.connections[id].auto_reconnect
                    } else {
                        false
                    };
                    if should_reconnect {
                        let delay = if id < self.config.connections.len() {
                            self.config.connections[id].reconnect_delay_secs
                        } else {
                            5
                        };
                        let name_clone = name.clone();
                        let host = if id < self.config.connections.len() {
                            self.config.connections[id].host.clone()
                        } else {
                            String::new()
                        };
                        let port = if id < self.config.connections.len() {
                            self.config.connections[id].port
                        } else {
                            0
                        };
                        self.terminal.append_output(
                            &format!("[系统] {} 秒后尝试重连 {}...", delay, name_clone)
                        )?;
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
    fn update_status_bar(&self) -> io::Result<()> {
        let mut stdout = io::stdout();
        self.terminal.draw_status_bar(
            &mut stdout,
            self.manager.foreground_name(),
            self.manager.foreground_state(),
            self.manager.sessions.len(),
            self.manager.foreground_id,
        )?;
        Ok(())
    }

    /// 请求退出
    pub fn quit(&mut self) {
        self.running = false;
    }
}
