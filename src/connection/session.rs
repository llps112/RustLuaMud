use std::io::{self, Read, Write};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
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

/// 连接信息摘要（供 UI 层使用）
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub name: String,
    pub state: SessionState,
}

/// 从连接接收到的数据事件
#[derive(Debug, Clone)]
pub enum SessionEvent {
    Data(String),
    StateChange(SessionState),
    Error(String),
}

/// 编码类型
#[derive(Debug, Clone, PartialEq)]
pub enum Encoding {
    Gbk,
    Utf8,
}

/// 单个 MUD 连接会话
pub struct Session {
    pub id: usize,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub encoding: Encoding,
    pub auto_reconnect: bool,
    pub reconnect_delay_secs: u64,
    pub state: SessionState,

    /// 该连接的输出缓冲区（前台切换时恢复用）
    pub output_lines: Vec<String>,

    /// Lua 脚本引擎
    pub lua_engine: Option<crate::lua::LuaEngine>,

    /// Lua 脚本路径
    pub script_path: Option<String>,
    /// 登录凭证（注入 Lua 变量 char_name / char_password）
    pub username: Option<String>,
    pub password: Option<String>,
    /// 是否自动连接
    pub auto_connect: bool,

    // 发送命令的通道
    send_tx: Option<mpsc::Sender<String>>,
}

/// 将 GBK 字节解码为 UTF-8 字符串
fn decode_gbk(bytes: &[u8]) -> String {
    let (cow, _encoding_used, _had_errors) = encoding_rs::GBK.decode(bytes);
    cow.into_owned()
}

/// 将 UTF-8 字符串编码为 GBK 字节
fn encode_gbk(text: &str) -> Vec<u8> {
    let (cow, _encoding_used, _had_errors) = encoding_rs::GBK.encode(text);
    cow.into_owned()
}

/// 过滤 telnet IAC 协议字节，避免二进制协商数据被当作文本解码
fn strip_telnet_iac(bytes: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0xFF {
            if i + 1 < bytes.len() {
                match bytes[i + 1] {
                    // IAC IAC = 转义的 0xFF 字面量
                    0xFF => {
                        result.push(0xFF);
                        i += 2;
                    }
                    // IAC WILL/WONT/DO/DONT: 3字节命令
                    0xFB..=0xFE => {
                        i += 3;
                    }
                    // IAC SB: 子协商，跳到 IAC SE
                    0xFA => {
                        i += 2;
                        while i + 1 < bytes.len() {
                            if bytes[i] == 0xFF && bytes[i + 1] == 0xF0 {
                                i += 2;
                                break;
                            }
                            i += 1;
                        }
                    }
                    // 其他 IAC 命令: 跳过2字节
                    _ => {
                        i += 2;
                    }
                }
            } else {
                // 行尾单独的 IAC，跳过
                i += 1;
            }
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    result
}

impl Session {
    pub fn new(id: usize, config: &ConnectionConfig) -> Self {
        let encoding = if config.encoding.as_deref() == Some("gbk")
            || config.encoding.as_deref() == Some("GBK")
        {
            Encoding::Gbk
        } else {
            Encoding::Utf8
        };
        Self {
            id,
            name: config.name.clone(),
            host: config.host.clone(),
            port: config.port,
            encoding,
            auto_reconnect: config.auto_reconnect,
            reconnect_delay_secs: config.reconnect_delay_secs,
            state: SessionState::Disconnected,
            output_lines: Vec::new(),
            lua_engine: None,
            script_path: config.script.clone(),
            username: config.username.clone(),
            password: config.password.clone(),
            auto_connect: config.auto_connect,
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

        let auto_reconnect = self.auto_reconnect;
        let encoding = self.encoding.clone();
        let session_id = self.id;

        // 读取任务：从服务器接收数据，按行读取并转码
        let event_tx_read = event_tx.clone();
        tokio::spawn(async move {
            // 按字节读取行，避免 UTF-8 解码问题
            let mut byte_buf: Vec<u8> = Vec::with_capacity(4096);

            loop {
                byte_buf.clear();
                // 逐字节读取直到遇到 \n
                loop {
                    let mut one_byte = [0u8; 1];
                    match reader.read(&mut one_byte).await {
                        Ok(0) => {
                            // 连接关闭
                            let _ = event_tx_read
                                .send(SessionEvent::StateChange(SessionState::Disconnected))
                                .await;
                            return;
                        }
                        Ok(_) => {
                            byte_buf.push(one_byte[0]);
                            if one_byte[0] == b'\n' {
                                break;
                            }
                        }
                        Err(e) => {
                            let _ = event_tx_read
                                .send(SessionEvent::Error(format!("读取错误: {}", e)))
                                .await;
                            let _ = event_tx_read
                                .send(SessionEvent::StateChange(SessionState::Disconnected))
                                .await;
                            return;
                        }
                    }
                }

                // 过滤 telnet IAC 协议字节
                let cleaned = strip_telnet_iac(&byte_buf);

                // 跳过过滤后为空的行
                if cleaned.is_empty() {
                    continue;
                }

                // 解码行数据
                let line_str = match encoding {
                    Encoding::Gbk => decode_gbk(&cleaned),
                    Encoding::Utf8 => String::from_utf8_lossy(&cleaned).into_owned(),
                };

                let _ = event_tx_read.send(SessionEvent::Data(line_str)).await;
            }
        });

        // 写入任务：发送用户命令到服务器
        let event_tx_write = event_tx.clone();
        let write_encoding = self.encoding.clone();
        tokio::spawn(async move {
            while let Some(cmd) = send_rx.recv().await {
                // 根据编码将命令转为字节
                let bytes = match write_encoding {
                    Encoding::Gbk => encode_gbk(&cmd),
                    Encoding::Utf8 => cmd.into_bytes(),
                };
                // MUD 协议要求命令以 \r\n 结尾
                let mut packet = bytes;
                packet.extend_from_slice(b"\r\n");
                if let Err(e) = write_half.write_all(&packet).await {
                    let _ = event_tx_write
                        .send(SessionEvent::Error(format!("发送失败: {}", e)))
                        .await;
                    break;
                }
            }
        });

        // 通知连接成功
        let _ = event_tx
            .send(SessionEvent::StateChange(SessionState::Connected))
            .await;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_gbk_ascii() {
        let bytes = b"hello";
        assert_eq!(decode_gbk(bytes), "hello");
    }

    #[test]
    fn test_decode_gbk_chinese() {
        // GBK 编码的 "你好" = 0xC4 0xE3 0xBA 0xC3
        let bytes: &[u8] = &[0xC4, 0xE3, 0xBA, 0xC3];
        let result = decode_gbk(bytes);
        assert_eq!(result, "你好");
    }

    #[test]
    fn test_decode_gbk_mixed() {
        // "hi你好" = "hi" + GBK "你好"
        let mut bytes: Vec<u8> = b"hi".to_vec();
        bytes.extend_from_slice(&[0xC4, 0xE3, 0xBA, 0xC3]);
        let result = decode_gbk(&bytes);
        assert_eq!(result, "hi你好");
    }

    #[test]
    fn test_encode_gbk_ascii() {
        let result = encode_gbk("hello");
        assert_eq!(result, b"hello");
    }

    #[test]
    fn test_encode_gbk_chinese() {
        let result = encode_gbk("你好");
        assert_eq!(result, vec![0xC4, 0xE3, 0xBA, 0xC3]);
    }

    #[test]
    fn test_encode_gbk_mixed() {
        let result = encode_gbk("hi你好");
        let mut expected: Vec<u8> = b"hi".to_vec();
        expected.extend_from_slice(&[0xC4, 0xE3, 0xBA, 0xC3]);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_strip_telnet_iac_no_iac() {
        let input = b"hello world";
        assert_eq!(strip_telnet_iac(input), b"hello world");
    }

    #[test]
    fn test_strip_telnet_iac_escaped_ff() {
        // IAC IAC (0xFF 0xFF) = 转义的 0xFF 字面量
        let input: &[u8] = &[0x41, 0xFF, 0xFF, 0x42];
        assert_eq!(strip_telnet_iac(input), &[0x41, 0xFF, 0x42]);
    }

    #[test]
    fn test_strip_telnet_iac_will() {
        // IAC WILL (0xFF 0xFB 0x01) = 3字节命令
        let input: &[u8] = &[0x41, 0xFF, 0xFB, 0x01, 0x42];
        assert_eq!(strip_telnet_iac(input), b"AB");
    }

    #[test]
    fn test_strip_telnet_iac_do() {
        // IAC DO (0xFF 0xFD 0x01)
        let input: &[u8] = &[0x48, 0xFF, 0xFD, 0x03, 0x69];
        assert_eq!(strip_telnet_iac(input), b"Hi");
    }

    #[test]
    fn test_strip_telnet_iac_dont() {
        // IAC DONT (0xFF 0xFE 0x01)
        let input: &[u8] = &[0xFF, 0xFE, 0x01];
        assert_eq!(strip_telnet_iac(input), b"");
    }

    #[test]
    fn test_strip_telnet_iac_subnegotiation() {
        // IAC SB ... IAC SE
        let input: &[u8] = &[0x41, 0xFF, 0xFA, 0x01, 0x02, 0x03, 0xFF, 0xF0, 0x42];
        assert_eq!(strip_telnet_iac(input), b"AB");
    }

    #[test]
    fn test_strip_telnet_iac_truncated_iac() {
        // 行尾单独的 IAC
        let input: &[u8] = &[0x41, 0xFF];
        assert_eq!(strip_telnet_iac(input), b"A");
    }

    #[test]
    fn test_strip_telnet_iac_truncated_will() {
        // IAC WILL 但缺少选项字节
        let input: &[u8] = &[0x41, 0xFF, 0xFB];
        assert_eq!(strip_telnet_iac(input), b"A");
    }

    #[test]
    fn test_strip_telnet_iac_multiple_commands() {
        // IAC WILL + IAC DO + 文本
        let input: &[u8] = &[0xFF, 0xFB, 0x01, 0xFF, 0xFD, 0x03, 0x41, 0x42];
        assert_eq!(strip_telnet_iac(input), b"AB");
    }

    #[test]
    fn test_session_default_state() {
        let config = ConnectionConfig {
            name: "test".to_string(),
            host: "localhost".to_string(),
            port: 4000,
            encoding: None,
            script: None,
            auto_connect: true,
            auto_reconnect: true,
            reconnect_delay_secs: 5,
            username: None,
            password: None,
        };
        let session = Session::new(1, &config);
        assert_eq!(session.name, "test");
        assert_eq!(session.host, "localhost");
        assert_eq!(session.port, 4000);
        assert!(matches!(session.state, SessionState::Disconnected));
        assert!(session.send_tx.is_none());
    }

    #[test]
    fn test_session_send_not_connected() {
        let config = ConnectionConfig {
            name: "test".to_string(),
            host: "localhost".to_string(),
            port: 4000,
            encoding: None,
            script: None,
            auto_connect: true,
            auto_reconnect: true,
            reconnect_delay_secs: 5,
            username: None,
            password: None,
        };
        let session = Session::new(1, &config);
        assert!(session.send("hello").is_err());
    }

    #[test]
    fn test_session_disconnect() {
        let config = ConnectionConfig {
            name: "test".to_string(),
            host: "localhost".to_string(),
            port: 4000,
            encoding: None,
            script: None,
            auto_connect: true,
            auto_reconnect: true,
            reconnect_delay_secs: 5,
            username: None,
            password: None,
        };
        let mut session = Session::new(1, &config);
        session.disconnect();
        assert!(matches!(session.state, SessionState::Disconnected));
    }

    #[test]
    fn test_session_gbk_encoding() {
        let config = ConnectionConfig {
            name: "gbk_test".to_string(),
            host: "mud.example.com".to_string(),
            port: 3000,
            encoding: Some("gbk".to_string()),
            script: None,
            auto_connect: false,
            auto_reconnect: true,
            reconnect_delay_secs: 5,
            username: None,
            password: None,
        };
        let session = Session::new(2, &config);
        assert!(matches!(session.encoding, Encoding::Gbk));
    }
}
