use tokio::sync::mpsc;

use crate::config::ConnectionConfig;
use super::session::{Session, SessionEvent, SessionInfo, SessionState};

/// 连接管理器事件
#[derive(Debug, Clone)]
pub enum ManagerEvent {
    /// 某连接收到数据 (session_id, data)
    Data(usize, String),
    /// 某连接状态变化 (session_id, new_state)
    StateChange(usize, SessionState),
    /// 某连接出错 (session_id, error)
    Error(usize, String),
}

/// 连接管理器（Phase 1: 单连接基础版）
pub struct ConnectionManager {
    pub sessions: Vec<Session>,
    pub foreground_id: usize,
    event_rx: Option<mpsc::Receiver<ManagerEvent>>,
    event_tx: mpsc::Sender<ManagerEvent>,
}

impl ConnectionManager {
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel(512);
        Self {
            sessions: Vec::new(),
            foreground_id: 0,
            event_rx: Some(event_rx),
            event_tx,
        }
    }

    /// 从配置添加连接
    pub fn add_connection(&mut self, config: &ConnectionConfig) -> usize {
        let id = self.sessions.len();
        let session = Session::new(id, config);
        self.sessions.push(session);
        id
    }

    /// 动态添加连接（运行时通过命令行添加）
    pub fn add_connection_dynamic(&mut self, config: &ConnectionConfig) -> usize {
        self.add_connection(config)
    }

    /// 连接指定会话
    pub async fn connect_session(&mut self, id: usize) -> Result<(), String> {
        if id >= self.sessions.len() {
            return Err(format!("连接 {} 不存在", id));
        }

        let session = &mut self.sessions[id];
        let mut event_rx = session.connect().await?;

        let event_tx = self.event_tx.clone();
        // 转发 session 事件为 manager 事件
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                let mgr_event = match event {
                    SessionEvent::Data(data) => ManagerEvent::Data(id, data),
                    SessionEvent::StateChange(state) => ManagerEvent::StateChange(id, state),
                    SessionEvent::Error(err) => ManagerEvent::Error(id, err),
                };
                if event_tx.send(mgr_event).await.is_err() {
                    break;
                }
            }
        });

        Ok(())
    }

    /// 发送命令到前台连接
    pub fn send_to_foreground(&self, cmd: &str) -> Result<(), String> {
        if self.foreground_id < self.sessions.len() {
            self.sessions[self.foreground_id].send(cmd)
        } else {
            Err("无前台连接".to_string())
        }
    }

    /// 发送命令到指定连接
    pub fn send_to(&self, id: usize, cmd: &str) -> Result<(), String> {
        if id < self.sessions.len() {
            self.sessions[id].send(cmd)
        } else {
            Err(format!("连接 {} 不存在", id))
        }
    }

    /// 切换前台连接
    pub fn switch_foreground(&mut self, id: usize) -> Result<(), String> {
        if id < self.sessions.len() {
            self.foreground_id = id;
            Ok(())
        } else {
            Err(format!("连接 {} 不存在", id))
        }
    }

    /// 取出事件接收器（只能取一次）
    pub fn take_event_rx(&mut self) -> Option<mpsc::Receiver<ManagerEvent>> {
        self.event_rx.take()
    }

    /// 彻底移除指定连接
    pub fn remove_session(&mut self, id: usize) -> Result<String, String> {
        if id >= self.sessions.len() {
            return Err(format!("连接 {} 不存在", id + 1));
        }
        let name = self.sessions[id].name.clone();
        self.sessions[id].disconnect();
        self.sessions.remove(id);
        // 如果移除的是前台连接或前台连接在它之后，调整 foreground_id
        if self.foreground_id >= self.sessions.len() && !self.sessions.is_empty() {
            self.foreground_id = self.sessions.len() - 1;
        }
        Ok(name)
    }

    /// 获取前台连接名称
    pub fn foreground_name(&self) -> &str {
        if self.foreground_id < self.sessions.len() {
            &self.sessions[self.foreground_id].name
        } else {
            "无"
        }
    }

    /// 获取前台连接状态
    pub fn foreground_state(&self) -> &SessionState {
        if self.foreground_id < self.sessions.len() {
            &self.sessions[self.foreground_id].state
        } else {
            &SessionState::Disconnected
        }
    }

    /// 获取所有连接的信息摘要
    pub fn session_infos(&self) -> Vec<SessionInfo> {
        self.sessions.iter().map(|s| SessionInfo {
            name: s.name.clone(),
            state: s.state.clone(),
        }).collect()
    }
}
