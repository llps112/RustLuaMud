// 应用主逻辑：App 结构体、主事件循环
// 子模块拆分：
// - parse:    内置命令解析（纯逻辑）
// - commands: 内置命令执行
// - events:   终端输入与连接事件处理
// - session:  Session 操作（连接/定时器/命令发送/日志排空）
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;

use crossterm::event::{Event as CrosstermEvent, EventStream};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

use crate::config::AppConfig;
use crate::connection::{ConnectionManager, SessionId};
use crate::log::Logger;
use crate::ui::Terminal;

mod commands;
mod events;
mod parse;
mod session;

use session::{ConnectRequest, ReconnectRequest, RenderTickRequest, TimerRequest};

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
    /// 已展示过原始数据发送错误的 session 集合（用于错误去重，防止刷屏）
    raw_send_err_shown: HashSet<SessionId>,
    /// 已展示过命令发送错误的 session 集合（用于错误去重，防止刷屏）
    cmd_send_err_shown: HashSet<SessionId>,
    /// 守护进程模式：无真实终端交互，跳过终端事件流
    daemon_mode: bool,
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
            raw_send_err_shown: HashSet::new(),
            cmd_send_err_shown: HashSet::new(),
            daemon_mode: false,
        })
    }

    /// 设置守护进程模式（必须在 run 之前调用）
    ///
    /// daemon 模式下不创建终端事件流（无真实终端输入），
    /// 退出由 SIGTERM 信号驱动（--daemon stop）。
    pub fn set_daemon_mode(&mut self, daemon: bool) {
        self.daemon_mode = daemon;
    }

    /// 启动应用主循环
    pub async fn run(&mut self) -> io::Result<()> {
        self.terminal.init_screen()?;

        // 自动连接所有 auto_connect 的连接（包括从 profile 加载的）
        let auto_connect_ids: Vec<SessionId> = self
            .manager
            .ordered_session_ids()
            .iter()
            .filter(|&&id| {
                self.manager
                    .get_by_id(id)
                    .map(|s| s.auto_connect)
                    .unwrap_or(false)
            })
            .copied()
            .collect();
        for session_id in auto_connect_ids {
            let name = self
                .manager
                .get_by_id(session_id)
                .map(|s| s.name.clone())
                .unwrap_or_default();
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

        // 终端事件流（daemon 模式无真实终端输入，跳过初始化避免读 tty 报错）
        let mut term_events: Option<EventStream> = if self.daemon_mode {
            None
        } else {
            Some(EventStream::new())
        };

        // 尺寸兜底轮询：conhost 窗口初始化竞态（如 Start-Process 派生）或
        // Resize 事件缺失时，缓存尺寸与实际视口不符会导致底部行写入
        // 触发视口滚动（状态栏/面板逐行上移），每秒一次同步即可自愈。
        // daemon 模式无真实终端，置 None 永不就绪。
        let mut size_poll: Option<tokio::time::Interval> = if self.daemon_mode {
            None
        } else {
            let mut it = tokio::time::interval(std::time::Duration::from_secs(1));
            it.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            Some(it)
        };

        // 信号处理器：Unix 用 SIGTERM，Windows 用 Ctrl+C
        #[cfg(unix)]
        let mut sigterm =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        // Windows: Ctrl+C 是一次性 Future，通过 channel 转为可重复等待的接收端
        #[cfg(not(unix))]
        let mut signal_waker = {
            let (tx, rx) = tokio::sync::oneshot::channel::<()>();
            tokio::spawn(async move {
                let _ = tokio::signal::ctrl_c().await;
                let _ = tx.send(());
            });
            rx
        };

        // 主事件循环
        while self.running {
            tokio::select! {
                // 处理终端键盘事件（daemon 模式下永不就绪）
                Some(Ok(event)) = async {
                    match term_events.as_mut() {
                        Some(es) => es.next().await,
                        None => std::future::pending().await,
                    }
                } => {
                    match event {
                        CrosstermEvent::Key(key) => {
                            self.handle_key_event(key)?;
                        }
                        CrosstermEvent::Mouse(mouse) => {
                            self.handle_mouse_event(mouse)?;
                        }
                        CrosstermEvent::Resize(w, h) => {
                            // Windows 的 WINDOW_BUFFER_SIZE_EVENT 经 crossterm +1 后
                            // 携带的是「缓冲区尺寸+1」（如 120x30 报成 121x31），
                            // 直接采信会把布局顶出视口：每拍多画一行→视口被迫
                            // 滚动一行→状态栏被推出第 0 行且永不重绘（append_output
                            // 路径不重绘状态栏），表现为逐行上移。
                            // 因此只要主动查询成功就完全忽略事件参数（包括
                            // 查询结果与缓存一致的情况），仅查询失败时回退。
                            match self.terminal.sync_size_if_changed() {
                                Ok(_) => {}
                                Err(_) => self.terminal.resize(w, h),
                            }
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

                // 尺寸兜底轮询（daemon 模式下永不就绪）
                Some(_) = async {
                    match size_poll.as_mut() {
                        Some(it) => {
                            it.tick().await;
                            Some(())
                        }
                        None => std::future::pending().await,
                    }
                } => {
                    // 查询失败（如无 tty）时静默跳过，下个周期再试
                    let _ = self.terminal.sync_size_if_changed();
                }

                // 信号优雅退出（Unix: SIGTERM / Windows: Ctrl+C）
                _ = async {
                    #[cfg(unix)]
                    { sigterm.recv().await }
                    #[cfg(not(unix))]
                    { let _ = (&mut signal_waker).await; }
                } => {
                    self.running = false;
                }
            }
        }

        Ok(())
    }

    /// 退出程序（suppress clippy unused warning）
    #[allow(dead_code)]
    pub fn quit(&mut self) {
        self.running = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
