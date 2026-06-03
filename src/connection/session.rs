use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::config::ConnectionConfig;

/// 连接状态
#[derive(Debug, Clone, PartialEq)]
pub enum SessionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
}

/// 从连接接收到的数据事件
#[derive(Debug, Clone)]
pub enum SessionEvent {
    Data(String),
    StateChange(SessionState),
    Error(String),
}

/// 单个 MUD 连接会话
pub struct Session {
    pub id: usize,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub auto_reconnect: bool,
    pub reconnect_delay_secs: u64,
    pub state: SessionState,

    // 发送命令的通道
    send_tx: Option<mpsc::Sender<String>>,
}

impl Session {
    pub fn new(id: usize, config: &ConnectionConfig) -> Self {
        Self {
            id,
            name: config.name.clone(),
            host: config.host.clone(),
            port: config.port,
            auto_reconnect: config.auto_reconnect,
            reconnect_delay_secs: config.reconnect_delay_secs,
            state: SessionState::Disconnected,
            send_tx: None,
        }
    }

    /// 连接到服务器，返回接收事件通道
    pub async fn connect(&mut self) -> Result<mpsc::Receiver<SessionEvent>, String> {
        let addr = format!("{}:{}", self.host, self.port);
        self.state = SessionState::Connecting;

        let stream = TcpStream::connect(&addr)
            .await
            .map_err(|e| format!("连接 {} 失败: {}", addr, e))?;

        self.state = SessionState::Connected;

        let (event_tx, event_rx) = mpsc::channel(256);
        let (send_tx, mut send_rx) = mpsc::channel::<String>(256);
        self.send_tx = Some(send_tx);

        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);

        let _name = self.name.clone();
        let auto_reconnect = self.auto_reconnect;
        let _reconnect_delay = self.reconnect_delay_secs;
        let _host = self.host.clone();
        let _port = self.port;

        // 读取任务：从服务器接收数据
        let event_tx_read = event_tx.clone();
        tokio::spawn(async move {
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        // 连接关闭
                        let _ = event_tx_read.send(SessionEvent::StateChange(
                            SessionState::Disconnected,
                        )).await;
                        if auto_reconnect {
                            let _ = event_tx_read.send(SessionEvent::StateChange(
                                SessionState::Reconnecting,
                            )).await;
                        }
                        break;
                    }
                    Ok(_) => {
                        let data = line.clone();
                        let _ = event_tx_read.send(SessionEvent::Data(data)).await;
                    }
                    Err(e) => {
                        let _ = event_tx_read.send(SessionEvent::Error(
                            format!("读取错误: {}", e),
                        )).await;
                        let _ = event_tx_read.send(SessionEvent::StateChange(
                            SessionState::Disconnected,
                        )).await;
                        break;
                    }
                }
            }
        });

        // 写入任务：发送用户命令到服务器
        let event_tx_write = event_tx.clone();
        tokio::spawn(async move {
            while let Some(cmd) = send_rx.recv().await {
                // MUD 协议要求命令以 \r\n 结尾
                if let Err(e) = write_half.write_all(format!("{}\r\n", cmd).as_bytes()).await {
                    let _ = event_tx_write.send(SessionEvent::Error(
                        format!("发送失败: {}", e),
                    )).await;
                    break;
                }
            }
        });

        // 通知连接成功
        let _ = event_tx.send(SessionEvent::StateChange(SessionState::Connected)).await;

        Ok(event_rx)
    }

    /// 发送命令到服务器
    pub fn send(&self, cmd: &str) -> Result<(), String> {
        if let Some(tx) = &self.send_tx {
            tx.try_send(cmd.to_string())
                .map_err(|e| format!("发送队列满或已关闭: {}", e))
        } else {
            Err("未连接".to_string())
        }
    }

    /// 断开连接
    pub fn disconnect(&mut self) {
        self.send_tx = None;
        self.state = SessionState::Disconnected;
    }
}
