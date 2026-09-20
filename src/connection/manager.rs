use std::collections::HashMap;
use tokio::sync::mpsc;

use super::session::{Session, SessionEvent, SessionId, SessionInfo, SessionState};
use crate::config::ConnectionConfig;

/// 连接管理器事件
#[derive(Debug, Clone)]
pub enum ManagerEvent {
    /// 某连接收到数据 (session_id, data)
    Data(SessionId, String),
    /// 某连接状态变化 (session_id, new_state, 连接代际号)
    /// 代际号由 session.connect_generation 提供，app 层据此丢弃旧连接残留任务的过期事件
    StateChange(SessionId, SessionState, u64),
    /// 某连接出错 (session_id, error)
    Error(SessionId, String),
}

/// 最大连接数（Alt+0~9 覆盖 10 个）
const MAX_SESSIONS: usize = 10;

/// 连接管理器
pub struct ConnectionManager {
    pub sessions: HashMap<SessionId, Session>,
    pub session_order: Vec<SessionId>,
    pub foreground_id: SessionId,
    next_session_id: u64,
    event_rx: Option<mpsc::Receiver<ManagerEvent>>,
    event_tx: mpsc::Sender<ManagerEvent>,
}

impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionManager {
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel(512);
        Self {
            sessions: HashMap::new(),
            session_order: Vec::new(),
            foreground_id: SessionId(0),
            next_session_id: 0,
            event_rx: Some(event_rx),
            event_tx,
        }
    }

    /// 按 SessionId 获取 session 不可变引用
    pub fn get_by_id(&self, id: SessionId) -> Option<&Session> {
        self.sessions.get(&id)
    }

    /// 按 SessionId 获取 session 可变引用
    pub fn get_mut_by_id(&mut self, id: SessionId) -> Option<&mut Session> {
        self.sessions.get_mut(&id)
    }

    /// 用户 1-based 显示编号 → SessionId
    pub fn session_id_by_display_number(&self, n: usize) -> Option<SessionId> {
        n.checked_sub(1)
            .and_then(|i| self.session_order.get(i).copied())
    }

    /// SessionId → 用户 1-based 显示编号
    pub fn display_number_of(&self, session_id: SessionId) -> usize {
        self.session_order
            .iter()
            .position(|id| *id == session_id)
            .map(|i| i + 1)
            .unwrap_or(0)
    }

    /// 返回按插入顺序排列的 SessionId 切片
    pub fn ordered_session_ids(&self) -> &[SessionId] {
        &self.session_order
    }

    /// 当前 session 数量
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// 循环切换前台：direction=-1 向前，direction=1 向后
    pub fn cycle_foreground(&mut self, direction: i32) -> Option<SessionId> {
        let pos = self
            .session_order
            .iter()
            .position(|id| *id == self.foreground_id)?;
        let total = self.session_order.len();
        let new_pos = if direction < 0 {
            (pos + total - 1) % total
        } else {
            (pos + 1) % total
        };
        let new_id = self.session_order[new_pos];
        self.foreground_id = new_id;
        Some(new_id)
    }

    /// 从配置添加连接，返回稳定 SessionId
    pub fn add_connection(&mut self, config: &ConnectionConfig) -> Result<SessionId, String> {
        if self.sessions.len() >= MAX_SESSIONS {
            return Err(format!("已达最大连接数限制 ({})", MAX_SESSIONS));
        }
        let session_id = SessionId(self.next_session_id);
        self.next_session_id += 1;
        let session = Session::new(session_id, config);
        self.sessions.insert(session_id, session);
        self.session_order.push(session_id);
        // 如果是第一个连接，设为前台
        if self.sessions.len() == 1 {
            self.foreground_id = session_id;
        }
        Ok(session_id)
    }

    /// 动态添加连接（运行时通过 `/profile load`、`/connect` 添加）
    ///
    /// 与 [`add_connection`](Self::add_connection) 的区别是多了重名保护：两个同名
    /// session 会用同一账号登录 → 服务端顶号 → 双方 `auto_reconnect` 互相触发
    /// → 无限重连循环（实测日志以 MB/min 增长、账号反复上下线）。
    ///
    /// 启动期的 `add_connection` 故意不做此检查：那里拒绝等于静默减少连接数，
    /// 且同名 profile 属配置错误，应由用户自行修正而不是在启动时吞掉一个连接。
    pub fn add_connection_dynamic(
        &mut self,
        config: &ConnectionConfig,
    ) -> Result<SessionId, String> {
        // 走 session_order（Vec，插入序确定）而非 sessions（HashMap，迭代序不定）：
        // 启动期 add_connection 故意不做重名检查，所以多个同名 session 是可能的；
        // 那种情况下报错里的显示编号必须稳定指向最早那个，
        // 否则用户按提示 /close 可能关掉非预期的连接
        let duplicate = self.session_order.iter().copied().find(|&sid| {
            self.sessions
                .get(&sid)
                .map(|s| s.name == config.name)
                .unwrap_or(false)
        });
        if let Some(sid) = duplicate {
            let n = self.display_number_of(sid);
            return Err(format!(
                "连接 '{}' 已存在（编号 {}）；重连请用 /reconnect {}，替换请先 /close {}",
                config.name, n, n, n
            ));
        }
        self.add_connection(config)
    }

    /// 连接指定会话
    pub async fn connect_session(&mut self, session_id: SessionId) -> Result<(), String> {
        let session = self
            .get_mut_by_id(session_id)
            .ok_or_else(|| "连接不存在".to_string())?;
        let mut event_rx = session.connect().await?;

        let event_tx = self.event_tx.clone();
        // 转发 session 事件为 manager 事件，捕获稳定的 SessionId
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                let mgr_event = match event {
                    SessionEvent::Data(data) => ManagerEvent::Data(session_id, data),
                    SessionEvent::StateChange(state, generation) => {
                        ManagerEvent::StateChange(session_id, state, generation)
                    }
                    SessionEvent::Error(err) => ManagerEvent::Error(session_id, err),
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
        if let Some(session) = self.sessions.get(&self.foreground_id) {
            session.send(cmd)
        } else {
            Err("无前台连接".to_string())
        }
    }

    /// 发送命令到指定连接
    pub fn send_to(&self, session_id: SessionId, cmd: &str) -> Result<(), String> {
        if let Some(session) = self.sessions.get(&session_id) {
            session.send(cmd)
        } else {
            Err("连接不存在".to_string())
        }
    }

    /// 发送命令到所有连接
    pub fn send_to_all(&self, cmd: &str) -> Vec<(SessionId, String, Result<(), String>)> {
        self.session_order
            .iter()
            .filter_map(|id| {
                let session = self.sessions.get(id)?;
                Some((*id, session.name.clone(), session.send(cmd)))
            })
            .collect()
    }

    /// 发送原始数据包到指定连接
    pub fn send_raw(&self, session_id: SessionId, data: Vec<u8>) -> Result<(), String> {
        if let Some(session) = self.sessions.get(&session_id) {
            session.send_raw(data)
        } else {
            Err("连接不存在".to_string())
        }
    }

    /// 切换前台连接
    pub fn switch_foreground(&mut self, session_id: SessionId) -> Result<(), String> {
        if self.sessions.contains_key(&session_id) {
            self.foreground_id = session_id;
            Ok(())
        } else {
            Err("连接不存在".to_string())
        }
    }

    /// 取出事件接收器（只能取一次）
    pub fn take_event_rx(&mut self) -> Option<mpsc::Receiver<ManagerEvent>> {
        self.event_rx.take()
    }

    /// 彻底移除指定连接
    pub fn remove_session(&mut self, session_id: SessionId) -> Result<String, String> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or_else(|| "连接不存在".to_string())?;
        // shutdown() 会同时取消读任务和定时器轮询任务的 cancel 信号，
        // 防止移除后旧异步任务用过期 ID 发事件
        session.shutdown();
        let name = session.name.clone();
        self.sessions.remove(&session_id);
        self.session_order.retain(|id| *id != session_id);
        // 如果移除的是前台连接，切换到最后一个 session
        if self.foreground_id == session_id && !self.session_order.is_empty() {
            self.foreground_id = *self.session_order.last().unwrap();
        }
        Ok(name)
    }

    /// 获取前台连接名称
    pub fn foreground_name(&self) -> &str {
        if let Some(session) = self.sessions.get(&self.foreground_id) {
            &session.name
        } else {
            "无"
        }
    }

    #[allow(dead_code)]
    /// 获取前台连接状态
    pub fn foreground_state(&self) -> &SessionState {
        if let Some(session) = self.sessions.get(&self.foreground_id) {
            &session.state
        } else {
            &SessionState::Disconnected
        }
    }

    /// 获取所有连接的信息摘要
    pub fn session_infos(&self) -> Vec<SessionInfo> {
        self.session_order
            .iter()
            .filter_map(|id| {
                let session = self.sessions.get(id)?;
                Some(SessionInfo {
                    session_id: *id,
                    name: session.name.clone(),
                    state: session.state.clone(),
                    status_text: session
                        .lua_engine
                        .as_ref()
                        .map(|e| e.status_text())
                        .unwrap_or_default(),
                })
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
            socks5_enable: false,
            socks5_host: None,
            socks5_port: 1080,
            socks5_username: None,
            socks5_password: None,
            log_rotation_count: None,
            render_interval: 1000,
            realtime: false,
            connect_delay_ms: 1000,
            cmd_interval_ms: 50,
            burst_size: 10,
            cmds_per_sec: 20,
            window_limit: 60,
            window_duration_ms: 2000,
            reconnect_max_secs: 1800,
            idle_timeout_secs: 300,
            heartbeat_cmd: String::new(),
            heartbeat_timeout_secs: 60,
            prompt_idle_ms: 0,
        }
    }

    #[test]
    fn test_new_manager() {
        let mgr = ConnectionManager::new();
        assert_eq!(mgr.session_count(), 0);
        assert!(mgr.ordered_session_ids().is_empty());
    }

    #[test]
    fn test_add_connection() {
        let mut mgr = ConnectionManager::new();
        let id = mgr
            .add_connection(&make_config("test", "localhost", 4000))
            .unwrap();
        assert_eq!(id, SessionId(0));
        assert_eq!(mgr.session_count(), 1);
        assert_eq!(mgr.get_by_id(id).unwrap().name, "test");
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
        assert_eq!(id1, SessionId(0));
        assert_eq!(id2, SessionId(1));
        assert_eq!(mgr.session_count(), 2);
    }

    /// 重名保护回归：两个同名 session 会用同一账号登录 → 服务端顶号 →
    /// 双方 auto_reconnect 互相触发 → 无限重连循环。
    /// 2026-09-11 实测：对已在运行的 lpssx 再次 /profile load lpssx，日志 ≈127 KB/min。
    #[test]
    fn test_add_connection_dynamic_rejects_duplicate_name() {
        let mut mgr = ConnectionManager::new();
        let id1 = mgr
            .add_connection_dynamic(&make_config("lpssx", "host1", 5555))
            .unwrap();
        assert_eq!(id1, SessionId(0));

        let err = mgr
            .add_connection_dynamic(&make_config("lpssx", "host1", 5555))
            .expect_err("同名连接必须被拒绝，否则双方会互相顶号");
        assert!(err.contains("已存在"), "应说明连接已存在: {}", err);
        assert!(
            err.contains("编号 1"),
            "应带显示编号便于直接 /close: {}",
            err
        );
        assert!(
            err.contains("/close") && err.contains("/reconnect"),
            "应给出两条可操作出路: {}",
            err
        );
        // 拒绝后不得残留半个 session
        assert_eq!(mgr.session_count(), 1);
        assert_eq!(mgr.ordered_session_ids().len(), 1);
    }

    /// 不同名连接不受重名保护影响
    #[test]
    fn test_add_connection_dynamic_allows_distinct_names() {
        let mut mgr = ConnectionManager::new();
        assert!(mgr
            .add_connection_dynamic(&make_config("a", "h", 4000))
            .is_ok());
        assert!(mgr
            .add_connection_dynamic(&make_config("b", "h", 4000))
            .is_ok());
        assert_eq!(mgr.session_count(), 2);
    }

    /// 启动期路径故意不做重名检查：拒绝等于静默减少连接数，
    /// 同名 profile 属配置错误，应由用户修正而不是启动时吞掉一个连接
    #[test]
    fn test_add_connection_allows_duplicate_name_at_startup() {
        let mut mgr = ConnectionManager::new();
        assert!(mgr.add_connection(&make_config("dup", "h", 4000)).is_ok());
        assert!(mgr.add_connection(&make_config("dup", "h", 4000)).is_ok());
        assert_eq!(mgr.session_count(), 2);
    }

    /// 重名保护不得误伤「先关闭再重新加载」这个正常流程
    #[test]
    fn test_add_connection_dynamic_allows_name_reuse_after_remove() {
        let mut mgr = ConnectionManager::new();
        let id = mgr
            .add_connection_dynamic(&make_config("lpssx", "h", 5555))
            .unwrap();
        assert!(mgr.remove_session(id).is_ok());
        assert_eq!(mgr.session_count(), 0);
        assert!(
            mgr.add_connection_dynamic(&make_config("lpssx", "h", 5555))
                .is_ok(),
            "关闭后应允许同名重建（/close 后 /profile load 是官方推荐的替换流程）"
        );
    }

    /// 存在多个同名 session 时（启动期 add_connection 故意不查重名，所以这是可能的），
    /// 报错里的编号必须稳定指向**最早**那个 —— 实现走 session_order 而非 HashMap
    /// 迭代序，否则用户按提示 /close 可能关掉非预期的连接
    #[test]
    fn test_add_connection_dynamic_reports_earliest_duplicate() {
        let mut mgr = ConnectionManager::new();
        let first = mgr.add_connection(&make_config("dup", "h", 4000)).unwrap();
        mgr.add_connection(&make_config("dup", "h", 4000)).unwrap();
        mgr.add_connection(&make_config("dup", "h", 4000)).unwrap();
        assert_eq!(mgr.session_count(), 3);

        let expected = mgr.display_number_of(first);
        let err = mgr
            .add_connection_dynamic(&make_config("dup", "h", 4000))
            .expect_err("已有同名连接时必须拒绝");
        assert!(
            err.contains(&format!("编号 {}", expected)),
            "应指向最早的同名连接（编号 {}），实际: {}",
            expected,
            err
        );
        assert_eq!(mgr.session_count(), 3, "拒绝后不得新增也不得移除 session");
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
        assert_eq!(id, SessionId(0));
    }

    #[test]
    fn test_session_id_stability_after_remove() {
        let mut mgr = ConnectionManager::new();
        let id0 = mgr.add_connection(&make_config("a", "h", 4000)).unwrap();
        let id1 = mgr.add_connection(&make_config("b", "h", 4000)).unwrap();
        let id2 = mgr.add_connection(&make_config("c", "h", 4000)).unwrap();

        // 移除中间的 session
        mgr.remove_session(id1).unwrap();

        // 剩余 session 的 ID 不变
        assert_eq!(mgr.session_count(), 2);
        assert_eq!(mgr.ordered_session_ids(), &[id0, id2]);

        // 按 ID 查找仍然正确
        assert!(mgr.get_by_id(id0).is_some());
        assert!(mgr.get_by_id(id2).is_some());
        assert!(mgr.get_by_id(id1).is_none());
    }

    #[test]
    fn test_switch_foreground() {
        let mut mgr = ConnectionManager::new();
        let id0 = mgr.add_connection(&make_config("a", "h", 4000)).unwrap();
        let id1 = mgr.add_connection(&make_config("b", "h", 4000)).unwrap();
        assert_eq!(mgr.foreground_id, id0);

        mgr.switch_foreground(id1).unwrap();
        assert_eq!(mgr.foreground_id, id1);
    }

    #[test]
    fn test_switch_foreground_invalid() {
        let mut mgr = ConnectionManager::new();
        mgr.add_connection(&make_config("a", "h", 4000)).unwrap();
        let result = mgr.switch_foreground(SessionId(99));
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
        let id = mgr.add_connection(&make_config("a", "h", 4000)).unwrap();
        let result = mgr.send_to(id, "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_send_to_invalid_id() {
        let mgr = ConnectionManager::new();
        let result = mgr.send_to(SessionId(99), "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_session() {
        let mut mgr = ConnectionManager::new();
        let id0 = mgr.add_connection(&make_config("a", "h", 4000)).unwrap();
        let id1 = mgr.add_connection(&make_config("b", "h", 4000)).unwrap();
        let name = mgr.remove_session(id0).unwrap();
        assert_eq!(name, "a");
        assert_eq!(mgr.session_count(), 1);
        assert_eq!(mgr.get_by_id(id1).unwrap().name, "b");
    }

    #[test]
    fn test_remove_session_invalid() {
        let mut mgr = ConnectionManager::new();
        let result = mgr.remove_session(SessionId(5));
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_session_adjusts_foreground() {
        let mut mgr = ConnectionManager::new();
        let id0 = mgr.add_connection(&make_config("a", "h", 4000)).unwrap();
        let id1 = mgr.add_connection(&make_config("b", "h", 4000)).unwrap();
        mgr.switch_foreground(id1).unwrap();
        // 移除 b，foreground_id 应调整为 a
        mgr.remove_session(id1).unwrap();
        assert_eq!(mgr.foreground_id, id0);
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
        let id0 = mgr.add_connection(&make_config("a", "h", 4000)).unwrap();
        let id1 = mgr.add_connection(&make_config("b", "h", 5000)).unwrap();
        let infos = mgr.session_infos();
        assert_eq!(infos.len(), 2);
        assert_eq!(infos[0].session_id, id0);
        assert_eq!(infos[0].name, "a");
        assert_eq!(infos[1].session_id, id1);
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

    #[test]
    fn test_new_session_id_after_remove() {
        let mut mgr = ConnectionManager::new();
        let id0 = mgr.add_connection(&make_config("a", "h", 4000)).unwrap();
        let id1 = mgr.add_connection(&make_config("b", "h", 4000)).unwrap();
        mgr.remove_session(id0).unwrap();
        // 新 session 应获得新 ID（不与旧 ID 冲突）
        let id2 = mgr.add_connection(&make_config("c", "h", 4000)).unwrap();
        assert_eq!(id2, SessionId(2));
        assert_ne!(id2, id0);
        assert_ne!(id2, id1);
        // 按 ID 查找不会混淆
        assert!(mgr.get_by_id(id0).is_none());
        assert!(mgr.get_by_id(id2).is_some());
    }

    #[test]
    fn test_display_number() {
        let mut mgr = ConnectionManager::new();
        let id0 = mgr.add_connection(&make_config("a", "h", 4000)).unwrap();
        let id1 = mgr.add_connection(&make_config("b", "h", 4000)).unwrap();
        let id2 = mgr.add_connection(&make_config("c", "h", 4000)).unwrap();
        assert_eq!(mgr.display_number_of(id0), 1);
        assert_eq!(mgr.display_number_of(id1), 2);
        assert_eq!(mgr.display_number_of(id2), 3);
        assert_eq!(mgr.session_id_by_display_number(2), Some(id1));
        // 移除中间的 session，显示编号重新排列
        mgr.remove_session(id1).unwrap();
        assert_eq!(mgr.display_number_of(id0), 1);
        assert_eq!(mgr.display_number_of(id2), 2);
        assert_eq!(mgr.session_id_by_display_number(1), Some(id0));
        assert_eq!(mgr.session_id_by_display_number(2), Some(id2));
    }

    #[test]
    fn test_cycle_foreground() {
        let mut mgr = ConnectionManager::new();
        let id0 = mgr.add_connection(&make_config("a", "h", 4000)).unwrap();
        let id1 = mgr.add_connection(&make_config("b", "h", 4000)).unwrap();
        let id2 = mgr.add_connection(&make_config("c", "h", 4000)).unwrap();
        assert_eq!(mgr.foreground_id, id0);
        mgr.cycle_foreground(1);
        assert_eq!(mgr.foreground_id, id1);
        mgr.cycle_foreground(1);
        assert_eq!(mgr.foreground_id, id2);
        mgr.cycle_foreground(1);
        assert_eq!(mgr.foreground_id, id0); // 循环
        mgr.cycle_foreground(-1);
        assert_eq!(mgr.foreground_id, id2); // 反向循环
    }
}
