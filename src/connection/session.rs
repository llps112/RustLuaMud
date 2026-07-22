use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio_socks::tcp::Socks5Stream;

use crate::config::ConnectionConfig;

/// 稳定会话 ID，在 session 生命周期内不变，不受 Vec 索引移位影响
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId(pub u64);

impl SessionId {
    // 用于显示给用户的 1-based 编号（从 Vec 位置转换）
    // 当前直接在各处使用 index + 1，保留此方法供未来统一使用
    #[allow(dead_code)]
    pub fn display_number(index: usize) -> usize {
        index + 1
    }
}

/// 连接状态
#[derive(Debug, Clone, PartialEq)]
pub enum SessionState {
    Disconnected,
    Connecting,
    Connected,
    #[allow(dead_code)]
    Reconnecting,
}

/// 连接信息摘要（供 UI 层使用）
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub session_id: SessionId,
    pub name: String,
    pub state: SessionState,
    pub status_text: String,
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
    pub session_id: SessionId,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub encoding: Encoding,
    pub auto_reconnect: bool,
    pub reconnect_delay_secs: u64,
    /// 当前重连退避秒数（失败后翻倍，成功后重置为 reconnect_delay_secs）
    pub reconnect_backoff_secs: u64,
    pub state: SessionState,

    /// 该连接的输出缓冲区（前台切换时恢复用）
    pub output_lines: Vec<String>,

    /// 该连接的输入状态（前台切换时保存/恢复）
    pub input_state: crate::ui::terminal::InputState,

    /// Lua 脚本引擎
    pub lua_engine: Option<crate::lua::LuaEngine>,

    /// Lua 脚本路径
    pub script_path: Option<String>,
    /// 登录凭证（注入 Lua 变量 char_name / char_password）
    pub username: Option<String>,
    pub password: Option<String>,
    /// 是否自动连接
    pub auto_connect: bool,

    /// SOCKS5 代理开关
    pub socks5_enable: bool,
    /// SOCKS5 代理地址
    pub socks5_host: Option<String>,
    /// SOCKS5 代理端口
    pub socks5_port: u16,
    /// SOCKS5 代理用户名（可选）
    pub socks5_username: Option<String>,
    /// SOCKS5 代理密码（可选）
    pub socks5_password: Option<String>,

    /// 渲染间隔（毫秒），最小值 50ms
    pub render_interval: u64,
    /// 实时渲染开关，true 时忽略 render_interval 直接实时渲染
    pub realtime: bool,
    /// 待渲染的缓冲数据（realtime = false 时使用）
    pub pending_data: Vec<String>,
    /// 是否有待渲染的数据
    pub render_dirty: bool,
    /// 连接建立后延迟执行 OnConnect 的毫秒数
    pub connect_delay_ms: u64,

    // 发送命令的通道
    send_tx: Option<mpsc::Sender<String>>,
    /// 发送原始数据包的通道
    send_raw_tx: Option<mpsc::Sender<Vec<u8>>>,
    /// 取消信号发送端：shutdown() 时发送信号让读任务静默退出
    cancel_tx: Option<oneshot::Sender<()>>,
    /// 定时器轮询任务的取消信号发送端
    pub timer_cancel_tx: Option<oneshot::Sender<()>>,
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
    pub fn new(session_id: SessionId, config: &ConnectionConfig) -> Self {
        let encoding = if config.encoding.as_deref() == Some("gbk")
            || config.encoding.as_deref() == Some("GBK")
        {
            Encoding::Gbk
        } else {
            Encoding::Utf8
        };
        Self {
            session_id,
            name: config.name.clone(),
            host: config.host.clone(),
            port: config.port,
            encoding,
            auto_reconnect: config.auto_reconnect,
            reconnect_delay_secs: config.reconnect_delay_secs,
            reconnect_backoff_secs: config.reconnect_delay_secs,
            state: SessionState::Disconnected,
            output_lines: Vec::new(),
            input_state: crate::ui::terminal::InputState::default(),
            lua_engine: None,
            script_path: config.script.clone(),
            username: config.username.clone(),
            password: config.password.clone(),
            auto_connect: config.auto_connect,
            socks5_enable: config.socks5_enable,
            socks5_host: config.socks5_host.clone(),
            socks5_port: config.socks5_port,
            socks5_username: config.socks5_username.clone(),
            socks5_password: config.socks5_password.clone(),
            render_interval: config.render_interval,
            realtime: config.realtime,
            pending_data: Vec::new(),
            render_dirty: false,
            connect_delay_ms: config.connect_delay_ms,
            send_tx: None,
            send_raw_tx: None,
            cancel_tx: None,
            timer_cancel_tx: None,
        }
    }

    /// 连接到服务器，返回接收事件通道
    pub async fn connect(&mut self) -> Result<mpsc::Receiver<SessionEvent>, String> {
        let addr = format!("{}:{}", self.host, self.port);
        self.state = SessionState::Connecting;

        // 判断是否使用 SOCKS5 代理
        let use_socks5 =
            self.socks5_enable && self.socks5_host.as_ref().is_some_and(|h| !h.is_empty());

        let connect_timeout = std::time::Duration::from_secs(10);
        let tokio_stream: TcpStream = if use_socks5 {
            // 通过 SOCKS5 代理连接
            let proxy_addr = format!(
                "{}:{}",
                self.socks5_host.as_ref().unwrap(),
                self.socks5_port
            );
            let target_addr = format!("{}:{}", self.host, self.port);

            let proxy_stream = if let Some(ref username) = self.socks5_username {
                if !username.is_empty() {
                    // 带认证的连接
                    let password = self.socks5_password.as_deref().unwrap_or("");
                    tokio::time::timeout(
                        connect_timeout,
                        Socks5Stream::connect_with_password(
                            proxy_addr.as_str(),
                            target_addr.as_str(),
                            username,
                            password,
                        ),
                    )
                    .await
                    .map_err(|_| format!("SOCKS5 代理连接 {} 超时（10秒）", proxy_addr))?
                    .map_err(|e| format!("SOCKS5 代理连接 {} 失败: {}", proxy_addr, e))?
                } else {
                    // 无认证的连接
                    tokio::time::timeout(
                        connect_timeout,
                        Socks5Stream::connect(proxy_addr.as_str(), target_addr.as_str()),
                    )
                    .await
                    .map_err(|_| format!("SOCKS5 代理连接 {} 超时（10秒）", proxy_addr))?
                    .map_err(|e| format!("SOCKS5 代理连接 {} 失败: {}", proxy_addr, e))?
                }
            } else {
                // 无认证的连接
                tokio::time::timeout(
                    connect_timeout,
                    Socks5Stream::connect(proxy_addr.as_str(), target_addr.as_str()),
                )
                .await
                .map_err(|_| format!("SOCKS5 代理连接 {} 超时（10秒）", proxy_addr))?
                .map_err(|e| format!("SOCKS5 代理连接 {} 失败: {}", proxy_addr, e))?
            };

            // 从 Socks5Stream 提取底层 TcpStream
            proxy_stream.into_inner()
        } else {
            // 直连
            tokio::time::timeout(connect_timeout, TcpStream::connect(&addr))
                .await
                .map_err(|_| format!("连接 {} 超时（10秒）", addr))?
                .map_err(|e| format!("连接 {} 失败: {}", addr, e))?
        };

        // 转换到 std 流来配置 keepalive
        let std_stream = tokio_stream
            .into_std()
            .map_err(|e| format!("转换 TCP 流失败: {}", e))?;

        // 启用 TCP keepalive，防止断包导致连接静默断开
        // 用 libc 统一配置（包括 SO_KEEPALIVE 和 Linux 特有参数）
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::io::AsRawFd;
            let fd = std_stream.as_raw_fd();
            let enable: libc::c_int = 1;
            let idle: libc::c_int = 15; // 空闲 15 秒后开始探测
            let intvl: libc::c_int = 5; // 探测间隔 5 秒
            let cnt: libc::c_int = 3; // 3 次失败后断开（最多 15+3*5=30 秒）
            unsafe {
                let set_keepalive = libc::setsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_KEEPALIVE,
                    &enable as *const _ as *const libc::c_void,
                    std::mem::size_of::<libc::c_int>() as libc::socklen_t,
                );
                if set_keepalive != 0 {
                    eprintln!("[警告] 设置 SO_KEEPALIVE 失败，TCP keepalive 未启用");
                }
                let set_idle = libc::setsockopt(
                    fd,
                    libc::SOL_TCP,
                    libc::TCP_KEEPIDLE,
                    &idle as *const _ as *const libc::c_void,
                    std::mem::size_of::<libc::c_int>() as libc::socklen_t,
                );
                if set_idle != 0 {
                    eprintln!("[警告] 设置 TCP_KEEPIDLE 失败");
                }
                let set_intvl = libc::setsockopt(
                    fd,
                    libc::SOL_TCP,
                    libc::TCP_KEEPINTVL,
                    &intvl as *const _ as *const libc::c_void,
                    std::mem::size_of::<libc::c_int>() as libc::socklen_t,
                );
                if set_intvl != 0 {
                    eprintln!("[警告] 设置 TCP_KEEPINTVL 失败");
                }
                let set_cnt = libc::setsockopt(
                    fd,
                    libc::SOL_TCP,
                    libc::TCP_KEEPCNT,
                    &cnt as *const _ as *const libc::c_void,
                    std::mem::size_of::<libc::c_int>() as libc::socklen_t,
                );
                if set_cnt != 0 {
                    eprintln!("[警告] 设置 TCP_KEEPCNT 失败");
                }
            }
        }

        // tokio 要求非阻塞模式
        std_stream
            .set_nonblocking(true)
            .map_err(|e| format!("设置非阻塞失败: {}", e))?;

        let stream = tokio::net::TcpStream::from_std(std_stream)
            .map_err(|e| format!("转为 tokio 流失败: {}", e))?;

        self.state = SessionState::Connected;

        let (event_tx, event_rx) = mpsc::channel(256);
        let (send_tx, mut send_rx) = mpsc::channel::<String>(256);
        self.send_tx = Some(send_tx);
        let (send_raw_tx, mut send_raw_rx) = mpsc::channel::<Vec<u8>>(256);
        self.send_raw_tx = Some(send_raw_tx);

        // 创建取消通道：shutdown() 时发送信号让读任务静默退出
        let (cancel_tx, cancel_rx) = oneshot::channel();
        self.cancel_tx = Some(cancel_tx);

        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);

        let _auto_reconnect = self.auto_reconnect;
        let encoding = self.encoding.clone();
        let _session_id = self.session_id;

        // 读取任务：从服务器接收数据，按行读取并转码
        let event_tx_read = event_tx.clone();
        tokio::spawn(async move {
            // pin cancel_rx 以便在 select! 中使用 &mut 引用
            tokio::pin!(cancel_rx);

            // 按字节读取行，避免 UTF-8 解码问题
            let mut byte_buf: Vec<u8> = Vec::with_capacity(4096);

            loop {
                byte_buf.clear();
                // 逐字节读取直到遇到 \n（或 \r 作为部分行交付）
                loop {
                    let mut one_byte = [0u8; 1];
                    tokio::select! {
                        // cancel 信号优先：shutdown() 时静默退出，不发送 StateChange
                        _ = &mut cancel_rx => {
                            return;
                        }
                        result = reader.read(&mut one_byte) => {
                            match result {
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
                                    if one_byte[0] == b'\r' && !byte_buf.is_empty() {
                                        // \r 作为部分行交付（MUD 常用 \r 覆盖当前行，无 \n）
                                        // 去掉 \r 本身，交付当前缓冲区内容
                                        byte_buf.pop(); // 移除 \r
                                        if !byte_buf.is_empty() {
                                            // 先过滤 telnet IAC 协议字节
                                            let cleaned = strip_telnet_iac(&byte_buf);
                                            if !cleaned.is_empty() {
                                                let line_str = match encoding {
                                                    Encoding::Gbk => decode_gbk(&cleaned),
                                                    Encoding::Utf8 => {
                                                        String::from_utf8_lossy(&cleaned).into_owned()
                                                    }
                                                };
                                                let _ =
                                                    event_tx_read.send(SessionEvent::Data(line_str)).await;
                                            }
                                        }
                                        byte_buf.clear();
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
                    }
                }

                // 跳过仅含 \n 的空行（紧随 \r 交付后产生，如 CRLF 序列的第二个字节）
                if byte_buf.len() == 1 && byte_buf[0] == b'\n' {
                    continue;
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
            use tokio::select;
            loop {
                select! {
                    maybe_cmd = send_rx.recv() => {
                        match maybe_cmd {
                            Some(cmd) => {
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
                            None => break,
                        }
                    }
                    maybe_raw = send_raw_rx.recv() => {
                        match maybe_raw {
                            Some(bytes) => {
                                // 原始数据包直接写入，不编码、不加 \r\n
                                if let Err(e) = write_half.write_all(&bytes).await {
                                    let _ = event_tx_write
                                        .send(SessionEvent::Error(format!("发送原始数据失败: {}", e)))
                                        .await;
                                    break;
                                }
                            }
                            None => break,
                        }
                    }
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

    /// 发送原始数据包到服务器
    pub fn send_raw(&self, data: Vec<u8>) -> Result<(), String> {
        if let Some(tx) = &self.send_raw_tx {
            tx.try_send(data)
                .map_err(|e| format!("原始数据发送队列满或已关闭: {}", e))
        } else {
            Err("未连接".to_string())
        }
    }

    /// 断开连接（用于 /disconnect 命令）
    /// 丢弃发送通道，读任务将在检测到 EOF 后自然退出并发送 StateChange(Disconnected)，
    /// 从而触发自动重连逻辑。
    pub fn disconnect(&mut self) {
        self.send_tx = None;
        self.send_raw_tx = None;
        self.state = SessionState::Disconnected;
    }

    /// 彻底关闭连接（用于 /close 命令）
    /// 发送取消信号让读任务和定时器轮询任务静默退出，
    /// 防止 remove_session 后旧异步任务用过期 id 发事件。
    pub fn shutdown(&mut self) {
        if let Some(tx) = self.cancel_tx.take() {
            let _ = tx.send(());
        }
        if let Some(tx) = self.timer_cancel_tx.take() {
            let _ = tx.send(());
        }
        self.send_tx = None;
        self.send_raw_tx = None;
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
            socks5_enable: false,
            socks5_host: None,
            socks5_port: 1080,
            socks5_username: None,
            socks5_password: None,
            log_rotation_count: None,
            render_interval: 1000,
            realtime: false,
            connect_delay_ms: 1000,
        };
        let session = Session::new(SessionId(1), &config);
        assert_eq!(session.name, "test");
        assert_eq!(session.host, "localhost");
        assert_eq!(session.port, 4000);
        assert!(matches!(session.state, SessionState::Disconnected));
        assert!(session.send_tx.is_none());
    }

    #[test]
    fn test_session_render_fields_default() {
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
            socks5_enable: false,
            socks5_host: None,
            socks5_port: 1080,
            socks5_username: None,
            socks5_password: None,
            log_rotation_count: None,
            render_interval: 2000,
            realtime: false,
            connect_delay_ms: 1000,
        };
        let session = Session::new(SessionId(1), &config);
        assert_eq!(session.render_interval, 2000);
        assert!(!session.render_dirty);
        assert!(session.pending_data.is_empty());
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
            socks5_enable: false,
            socks5_host: None,
            socks5_port: 1080,
            socks5_username: None,
            socks5_password: None,
            log_rotation_count: None,
            render_interval: 1000,
            realtime: false,
            connect_delay_ms: 1000,
        };
        let session = Session::new(SessionId(1), &config);
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
            socks5_enable: false,
            socks5_host: None,
            socks5_port: 1080,
            socks5_username: None,
            socks5_password: None,
            log_rotation_count: None,
            render_interval: 1000,
            realtime: false,
            connect_delay_ms: 1000,
        };
        let mut session = Session::new(SessionId(1), &config);
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
            socks5_enable: false,
            socks5_host: None,
            socks5_port: 1080,
            socks5_username: None,
            socks5_password: None,
            log_rotation_count: None,
            render_interval: 1000,
            realtime: false,
            connect_delay_ms: 1000,
        };
        let session = Session::new(SessionId(2), &config);
        assert!(matches!(session.encoding, Encoding::Gbk));
    }

    #[test]
    fn test_session_default_encoding() {
        let config = ConnectionConfig {
            name: "default_enc".to_string(),
            host: "mud.example.com".to_string(),
            port: 3000,
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
        };
        let session = Session::new(SessionId(3), &config);
        assert!(matches!(session.encoding, Encoding::Utf8));
    }

    #[test]
    fn test_session_with_script_path() {
        let config = ConnectionConfig {
            name: "scripted".to_string(),
            host: "mud.example.com".to_string(),
            port: 3000,
            encoding: None,
            script: Some("/path/to/script.lua".to_string()),
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
        };
        let session = Session::new(SessionId(5), &config);
        assert_eq!(session.script_path, Some("/path/to/script.lua".to_string()));
    }

    #[test]
    fn test_session_with_credentials() {
        let config = ConnectionConfig {
            name: "auth".to_string(),
            host: "mud.example.com".to_string(),
            port: 3000,
            encoding: None,
            script: None,
            auto_connect: false,
            auto_reconnect: true,
            reconnect_delay_secs: 5,
            username: Some("player".to_string()),
            password: Some("secret".to_string()),
            socks5_enable: false,
            socks5_host: None,
            socks5_port: 1080,
            socks5_username: None,
            socks5_password: None,
            log_rotation_count: None,
            render_interval: 1000,
            realtime: false,
            connect_delay_ms: 1000,
        };
        let session = Session::new(SessionId(6), &config);
        assert_eq!(session.username, Some("player".to_string()));
        assert_eq!(session.password, Some("secret".to_string()));
    }

    #[test]
    fn test_session_auto_connect_flag() {
        let config = ConnectionConfig {
            name: "auto".to_string(),
            host: "mud.example.com".to_string(),
            port: 3000,
            encoding: None,
            script: None,
            auto_connect: true,
            auto_reconnect: false,
            reconnect_delay_secs: 3,
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
        };
        let session = Session::new(SessionId(7), &config);
        assert!(session.auto_connect);
        assert!(!session.auto_reconnect);
        assert_eq!(session.reconnect_delay_secs, 3);
    }

    #[test]
    fn test_session_gbk_encoding_uppercase() {
        let config = ConnectionConfig {
            name: "gbk_upper".to_string(),
            host: "mud.example.com".to_string(),
            port: 3000,
            encoding: Some("GBK".to_string()),
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
        };
        let session = Session::new(SessionId(8), &config);
        assert!(matches!(session.encoding, Encoding::Gbk));
    }

    #[test]
    fn test_session_unknown_encoding_defaults_utf8() {
        let config = ConnectionConfig {
            name: "unknown_enc".to_string(),
            host: "mud.example.com".to_string(),
            port: 3000,
            encoding: Some("shift_jis".to_string()),
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
        };
        let session = Session::new(SessionId(9), &config);
        assert!(matches!(session.encoding, Encoding::Utf8));
    }

    #[test]
    fn test_decode_gbk_empty() {
        assert_eq!(decode_gbk(&[]), "");
    }

    #[test]
    fn test_encode_gbk_empty() {
        assert_eq!(encode_gbk(""), b"");
    }

    #[test]
    fn test_decode_gbk_invalid_bytes() {
        // Invalid GBK bytes should not panic
        let bytes: &[u8] = &[0x80, 0x81];
        let result = decode_gbk(bytes);
        assert!(!result.is_empty()); // encoding_rs replaces invalid with ?
    }

    #[test]
    fn test_strip_telnet_iac_empty() {
        assert_eq!(strip_telnet_iac(&[]), b"");
    }

    #[test]
    fn test_strip_telnet_iac_wont() {
        // IAC WONT (0xFF 0xFC 0x01)
        let input: &[u8] = &[0xFF, 0xFC, 0x01];
        assert_eq!(strip_telnet_iac(input), b"");
    }

    #[test]
    fn test_strip_telnet_iac_subnegotiation_unterminated() {
        // IAC SB without IAC SE - scans for IAC SE but doesn't find it
        // Bytes consumed by the while loop are skipped, remaining bytes after loop are kept
        let input: &[u8] = &[0x41, 0xFF, 0xFA, 0x01, 0x02];
        let result = strip_telnet_iac(input);
        // After IAC SB, while loop scans bytes[3]=0x01 (not IAC SE), i=4
        // Loop exits, then bytes[4]=0x02 is pushed as normal data
        assert_eq!(result, &[0x41, 0x02]);
    }

    #[test]
    fn test_strip_telnet_iac_other_command() {
        // IAC + other command byte (0xF1 = NOP, 2-byte command)
        let input: &[u8] = &[0x41, 0xFF, 0xF1, 0x42];
        assert_eq!(strip_telnet_iac(input), b"AB");
    }

    #[test]
    fn test_session_output_lines_initially_empty() {
        let config = ConnectionConfig {
            name: "test".to_string(),
            host: "localhost".to_string(),
            port: 4000,
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
        };
        let session = Session::new(SessionId(1), &config);
        assert!(session.output_lines.is_empty());
    }

    #[test]
    fn test_session_lua_engine_initially_none() {
        let config = ConnectionConfig {
            name: "test".to_string(),
            host: "localhost".to_string(),
            port: 4000,
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
        };
        let session = Session::new(SessionId(1), &config);
        assert!(session.lua_engine.is_none());
    }

    #[test]
    fn test_session_disconnect_clears_send_tx() {
        let config = ConnectionConfig {
            name: "test".to_string(),
            host: "localhost".to_string(),
            port: 4000,
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
        };
        let mut session = Session::new(SessionId(1), &config);
        session.disconnect();
        assert!(session.send_tx.is_none());
        // Send should fail after disconnect
        assert!(session.send("test").is_err());
    }

    #[test]
    fn test_session_state_equality() {
        assert_eq!(SessionState::Connected, SessionState::Connected);
        assert_ne!(SessionState::Connected, SessionState::Disconnected);
    }

    #[test]
    fn test_session_socks5_disabled_by_default() {
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
            socks5_enable: false,
            socks5_host: None,
            socks5_port: 1080,
            socks5_username: None,
            socks5_password: None,
            log_rotation_count: None,
            render_interval: 1000,
            realtime: false,
            connect_delay_ms: 1000,
        };
        let session = Session::new(SessionId(1), &config);
        assert!(!session.socks5_enable);
        assert!(session.socks5_host.is_none());
        assert_eq!(session.socks5_port, 1080);
    }

    #[test]
    fn test_session_socks5_enabled_with_host() {
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
            socks5_enable: true,
            socks5_host: Some("127.0.0.1".to_string()),
            socks5_port: 1080,
            socks5_username: None,
            socks5_password: None,
            log_rotation_count: None,
            render_interval: 1000,
            realtime: false,
            connect_delay_ms: 1000,
        };
        let session = Session::new(SessionId(1), &config);
        assert!(session.socks5_enable);
        assert_eq!(session.socks5_host, Some("127.0.0.1".to_string()));
        assert_eq!(session.socks5_port, 1080);
    }

    #[test]
    fn test_session_socks5_with_authentication() {
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
            socks5_enable: true,
            socks5_host: Some("proxy.example.com".to_string()),
            socks5_port: 1081,
            socks5_username: Some("user".to_string()),
            socks5_password: Some("pass".to_string()),
            log_rotation_count: None,
            render_interval: 1000,
            realtime: false,
            connect_delay_ms: 1000,
        };
        let session = Session::new(SessionId(1), &config);
        assert!(session.socks5_enable);
        assert_eq!(session.socks5_host, Some("proxy.example.com".to_string()));
        assert_eq!(session.socks5_port, 1081);
        assert_eq!(session.socks5_username, Some("user".to_string()));
        assert_eq!(session.socks5_password, Some("pass".to_string()));
    }

    #[test]
    fn test_session_socks5_enabled_but_no_host() {
        // 如果启用了 SOCKS5 但没有配置 host，应该回退到直连
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
            socks5_enable: true,
            socks5_host: None,
            socks5_port: 1080,
            socks5_username: None,
            socks5_password: None,
            log_rotation_count: None,
            render_interval: 1000,
            realtime: false,
            connect_delay_ms: 1000,
        };
        let session = Session::new(SessionId(1), &config);
        assert!(session.socks5_enable);
        assert!(session.socks5_host.is_none());
    }

    #[test]
    fn test_session_socks5_enabled_but_empty_host() {
        // 如果启用了 SOCKS5 但 host 为空字符串，应该回退到直连
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
            socks5_enable: true,
            socks5_host: Some("".to_string()),
            socks5_port: 1080,
            socks5_username: None,
            socks5_password: None,
            log_rotation_count: None,
            render_interval: 1000,
            realtime: false,
            connect_delay_ms: 1000,
        };
        let session = Session::new(SessionId(1), &config);
        assert!(session.socks5_enable);
        assert_eq!(session.socks5_host, Some("".to_string()));
    }

    // === 异步集成测试（本地 TCP 回环对） ===

    fn make_test_config(name: &str, port: u16) -> ConnectionConfig {
        ConnectionConfig {
            name: name.to_string(),
            host: "127.0.0.1".to_string(),
            port,
            encoding: None,
            script: None,
            auto_connect: false,
            auto_reconnect: false,
            reconnect_delay_secs: 1,
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
        }
    }

    fn make_gbk_test_config(name: &str, port: u16) -> ConnectionConfig {
        ConnectionConfig {
            name: name.to_string(),
            host: "127.0.0.1".to_string(),
            port,
            encoding: Some("gbk".to_string()),
            script: None,
            auto_connect: false,
            auto_reconnect: false,
            reconnect_delay_secs: 1,
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
        }
    }

    /// 启动一个本地 TCP 回显服务器，返回监听端口
    async fn start_echo_server() -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                if let Ok((mut stream, _)) = listener.accept().await {
                    tokio::spawn(async move {
                        let (mut read_half, mut write_half) = stream.split();
                        let _ = tokio::io::copy(&mut read_half, &mut write_half).await;
                    });
                }
            }
        });
        port
    }

    /// 启动一个本地 TCP 服务器，发送预设数据后关闭
    async fn start_send_and_close_server(data: &[u8]) -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let data = data.to_vec();
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let _ = stream.write_all(&data).await;
                // 短暂等待后关闭
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        });
        port
    }

    #[tokio::test]
    async fn test_session_connect_and_receive_data() {
        let data = b"hello world\n";
        let port = start_send_and_close_server(data).await;
        let mut session = Session::new(SessionId(0), &make_test_config("test", port));

        let mut event_rx = session.connect().await.unwrap();
        assert!(matches!(session.state, SessionState::Connected));

        // 接收数据事件
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while let Some(event) = event_rx.recv().await {
                if let SessionEvent::Data(text) = event {
                    assert!(text.contains("hello world"));
                    return;
                }
            }
        })
        .await
        .expect("timed out waiting for data");
    }

    #[tokio::test]
    async fn test_session_connect_state_changes() {
        let port = start_echo_server().await;
        let mut session = Session::new(SessionId(0), &make_test_config("test", port));

        let mut event_rx = session.connect().await.unwrap();

        // 应该收到 Connected 状态变更
        let mut got_connected = false;
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while let Some(event) = event_rx.recv().await {
                if let SessionEvent::StateChange(state) = event {
                    if matches!(state, SessionState::Connected) {
                        got_connected = true;
                        return;
                    }
                }
            }
        })
        .await
        .expect("timed out waiting for state change");
        assert!(got_connected);
    }

    #[tokio::test]
    async fn test_session_send_command() {
        let port = start_echo_server().await;
        let mut session = Session::new(SessionId(0), &make_test_config("test", port));

        let mut event_rx = session.connect().await.unwrap();

        // 发送命令
        session.send("look").unwrap();

        // 等待回显数据
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while let Some(event) = event_rx.recv().await {
                if let SessionEvent::Data(text) = event {
                    if text.contains("look") {
                        return;
                    }
                }
            }
        })
        .await
        .expect("timed out waiting for echo");
    }

    #[tokio::test]
    async fn test_session_connect_failure() {
        // 连接到一个不存在的端口
        let mut session = Session::new(SessionId(0), &make_test_config("test", 1));

        let result = session.connect().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("连接"));
    }

    #[tokio::test]
    async fn test_session_disconnect_after_connect() {
        let port = start_echo_server().await;
        let mut session = Session::new(SessionId(0), &make_test_config("test", port));

        let _event_rx = session.connect().await.unwrap();
        assert!(matches!(session.state, SessionState::Connected));

        session.disconnect();
        assert!(matches!(session.state, SessionState::Disconnected));
        assert!(session.send("test").is_err());
    }

    #[tokio::test]
    async fn test_session_gbk_receive_data() {
        // GBK 编码的 "你好\n"
        let gbk_bytes = encode_gbk("你好\n");
        let port = start_send_and_close_server(&gbk_bytes).await;
        let mut session = Session::new(SessionId(0), &make_gbk_test_config("gbk_test", port));

        let mut event_rx = session.connect().await.unwrap();

        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while let Some(event) = event_rx.recv().await {
                if let SessionEvent::Data(text) = event {
                    assert!(text.contains("你好"));
                    return;
                }
            }
        })
        .await
        .expect("timed out waiting for GBK data");
    }

    #[tokio::test]
    async fn test_session_receive_multiple_lines() {
        let data = b"line1\nline2\nline3\n";
        let port = start_send_and_close_server(data).await;
        let mut session = Session::new(SessionId(0), &make_test_config("test", port));

        let mut event_rx = session.connect().await.unwrap();

        let mut received_lines = Vec::new();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while let Some(event) = event_rx.recv().await {
                match event {
                    SessionEvent::Data(text) => {
                        received_lines.push(text);
                        if received_lines.len() >= 3 {
                            return;
                        }
                    }
                    SessionEvent::StateChange(SessionState::Disconnected) => {
                        return;
                    }
                    _ => {}
                }
            }
        })
        .await
        .expect("timed out waiting for multiple lines");

        assert!(received_lines.len() >= 3);
    }

    #[tokio::test]
    async fn test_session_server_disconnect_detected() {
        // 服务器立即关闭连接
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                // 发送一行数据后立即关闭
                let _ = stream.write_all(b"bye\n").await;
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                let _ = stream.shutdown().await;
            }
        });

        let mut session = Session::new(SessionId(0), &make_test_config("test", port));
        let mut event_rx = session.connect().await.unwrap();

        let mut got_disconnect = false;
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while let Some(event) = event_rx.recv().await {
                if let SessionEvent::StateChange(SessionState::Disconnected) = event {
                    got_disconnect = true;
                    return;
                }
            }
        })
        .await
        .expect("timed out waiting for disconnect");
        assert!(got_disconnect);
    }
}
