use tokio::sync::mpsc;

use super::session::{Session, SessionEvent, SessionInfo, SessionState};
use crate::config::ConnectionConfig;

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

/// 最大连接数（Alt+0~9 覆盖 10 个）
const MAX_SESSIONS: usize = 10;

/// 连接管理器
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
    pub fn add_connection(&mut self, config: &ConnectionConfig) -> Result<usize, String> {
        if self.sessions.len() >= MAX_SESSIONS {
            return Err(format!("已达最大连接数限制 ({})", MAX_SESSIONS));
        }
        let id = self.sessions.len();
        let session = Session::new(id, config);
        self.sessions.push(session);
        Ok(id)
    }

    /// 动态添加连接（运行时通过命令行添加）
    pub fn add_connection_dynamic(&mut self, config: &ConnectionConfig) -> Result<usize, String> {
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

    #[allow(dead_code)]
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
        self.sessions
            .iter()
            .map(|s| SessionInfo {
                name: s.name.clone(),
                state: s.state.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConnectionConfig;

    fn make_config(name: &str, host: &str, port: u16) -> ConnectionConfig {
        ConnectionConfig {
            name: name.to_string(),
            host: host.to_string(),
            port,
            encoding: None,
            script: None,
            auto_connect: false,
            auto_reconnect: true,
            reconnect_delay_secs: 5,
            username: None,
            password: None,
        }
    }

    #[test]
    fn test_new_manager() {
        let mgr = ConnectionManager::new();
        assert!(mgr.sessions.is_empty());
        assert_eq!(mgr.foreground_id, 0);
    }

    #[test]
    fn test_add_connection() {
        let mut mgr = ConnectionManager::new();
        let id = mgr
            .add_connection(&make_config("test", "localhost", 4000))
            .unwrap();
        assert_eq!(id, 0);
        assert_eq!(mgr.sessions.len(), 1);
        assert_eq!(mgr.sessions[0].name, "test");
    }

    #[test]
    fn test_add_multiple_connections() {
        let mut mgr = ConnectionManager::new();
        let id1 = mgr
            .add_connection(&make_config("a", "host1", 4000))
            .unwrap();
        let id2 = mgr
            .add_connection(&make_config("b", "host2", 5000))
            .unwrap();
        assert_eq!(id1, 0);
        assert_eq!(id2, 1);
        assert_eq!(mgr.sessions.len(), 2);
    }

    #[test]
    fn test_add_connection_max_limit() {
        let mut mgr = ConnectionManager::new();
        for i in 0..10 {
            let result = mgr.add_connection(&make_config(&format!("s{}", i), "h", 4000));
            assert!(result.is_ok());
        }
        // 第11个应失败
        let result = mgr.add_connection(&make_config("overflow", "h", 4000));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("最大连接数"));
    }

    #[test]
    fn test_add_connection_dynamic() {
        let mut mgr = ConnectionManager::new();
        let id = mgr
            .add_connection_dynamic(&make_config("dyn", "h", 4000))
            .unwrap();
        assert_eq!(id, 0);
    }

    #[test]
    fn test_switch_foreground() {
        let mut mgr = ConnectionManager::new();
        mgr.add_connection(&make_config("a", "h", 4000)).unwrap();
        mgr.add_connection(&make_config("b", "h", 4000)).unwrap();
        assert_eq!(mgr.foreground_id, 0);

        mgr.switch_foreground(1).unwrap();
        assert_eq!(mgr.foreground_id, 1);
    }

    #[test]
    fn test_switch_foreground_invalid() {
        let mut mgr = ConnectionManager::new();
        mgr.add_connection(&make_config("a", "h", 4000)).unwrap();
        let result = mgr.switch_foreground(5);
        assert!(result.is_err());
    }

    #[test]
    fn test_send_to_foreground_not_connected() {
        let mut mgr = ConnectionManager::new();
        mgr.add_connection(&make_config("a", "h", 4000)).unwrap();
        // 未连接，send 应失败
        let result = mgr.send_to_foreground("test");
        assert!(result.is_err());
    }

    #[test]
    fn test_send_to_not_connected() {
        let mut mgr = ConnectionManager::new();
        mgr.add_connection(&make_config("a", "h", 4000)).unwrap();
        let result = mgr.send_to(0, "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_send_to_invalid_id() {
        let mgr = ConnectionManager::new();
        let result = mgr.send_to(99, "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_session() {
        let mut mgr = ConnectionManager::new();
        mgr.add_connection(&make_config("a", "h", 4000)).unwrap();
        mgr.add_connection(&make_config("b", "h", 4000)).unwrap();
        let name = mgr.remove_session(0).unwrap();
        assert_eq!(name, "a");
        assert_eq!(mgr.sessions.len(), 1);
        assert_eq!(mgr.sessions[0].name, "b");
    }

    #[test]
    fn test_remove_session_invalid() {
        let mut mgr = ConnectionManager::new();
        let result = mgr.remove_session(5);
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_session_adjusts_foreground() {
        let mut mgr = ConnectionManager::new();
        mgr.add_connection(&make_config("a", "h", 4000)).unwrap();
        mgr.add_connection(&make_config("b", "h", 4000)).unwrap();
        mgr.switch_foreground(1).unwrap();
        // 移除 b (id=1)，foreground_id 应调整为 0
        mgr.remove_session(1).unwrap();
        assert_eq!(mgr.foreground_id, 0);
    }

    #[test]
    fn test_foreground_name() {
        let mut mgr = ConnectionManager::new();
        mgr.add_connection(&make_config("alpha", "h", 4000))
            .unwrap();
        assert_eq!(mgr.foreground_name(), "alpha");
    }

    #[test]
    fn test_foreground_name_empty() {
        let mgr = ConnectionManager::new();
        assert_eq!(mgr.foreground_name(), "无");
    }

    #[test]
    fn test_foreground_state() {
        let mut mgr = ConnectionManager::new();
        mgr.add_connection(&make_config("a", "h", 4000)).unwrap();
        assert_eq!(*mgr.foreground_state(), SessionState::Disconnected);
    }

    #[test]
    fn test_session_infos() {
        let mut mgr = ConnectionManager::new();
        mgr.add_connection(&make_config("a", "h", 4000)).unwrap();
        mgr.add_connection(&make_config("b", "h", 5000)).unwrap();
        let infos = mgr.session_infos();
        assert_eq!(infos.len(), 2);
        assert_eq!(infos[0].name, "a");
        assert_eq!(infos[1].name, "b");
    }

    #[test]
    fn test_take_event_rx_once() {
        let mut mgr = ConnectionManager::new();
        let rx = mgr.take_event_rx();
        assert!(rx.is_some());
        let rx2 = mgr.take_event_rx();
        assert!(rx2.is_none());
    }
}
