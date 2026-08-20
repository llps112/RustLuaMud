//! Lua 引擎的核心类型定义
//!
//! 包含触发器、别名、定时器、脚本状态等核心数据结构。

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use mlua::{Function, Lua};
use regex::bytes::Regex as BytesRegex;
use regex::Regex;

use crate::ui::terminal::PanelButtonDef;

/// 触发器匹配模式：GBK 字节模式或 UTF-8 字符模式
pub(crate) enum TriggerPattern {
    /// GBK 字节模式：正则中的中文字符转为 GBK 字节序列，匹配 GBK 编码的数据
    /// 适用于 GBK 编码的脚本，.{4} 匹配 4 字节（2 个中文字符）
    Gbk(BytesRegex),
    /// UTF-8 字符模式：正则按 Unicode 字符匹配，匹配 UTF-8 数据
    /// 适用于 UTF-8 编码的脚本，.{4} 匹配 4 个 Unicode 字符
    Utf8(Regex),
}

/// 触发器定义
pub struct Trigger {
    pub name: String,
    pub(crate) pattern: TriggerPattern,
    pub callback: Function,
    pub enabled: bool,
    pub group: String,
    #[allow(dead_code)]
    pub sequence: i32,
    #[allow(dead_code)]
    pub temporary: bool,
    #[allow(dead_code)]
    pub multiline: bool,
    #[allow(dead_code)]
    pub lines_to_match: usize,
    #[allow(dead_code)]
    pub omit_from_output: bool,
    pub one_shot: bool,
    pub send_text: String,
}

/// 样式运行片段（MUSHclient GetStyle 兼容）
/// 记录 ANSI 颜色/属性在 clean_line 中的字节区间
#[derive(Debug, Clone)]
pub(crate) struct StyleRun {
    /// 在 clean_line 中的起始字节偏移（0-based）
    pub start: usize,
    /// 区间长度（字节数）
    pub length: usize,
    /// 前景色（ANSI 0-15 标准色号）
    pub textcolour: u32,
    /// 背景色（ANSI 色号）
    pub backcolour: u32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

/// 别名定义
pub struct Alias {
    pub name: String,
    pub match_text: String,
    pub pattern: Regex,
    pub callback: Function,
    pub enabled: bool,
    pub group: String,
    pub send_to: i64,
    pub response: String,
    pub sequence: i32,
    pub one_shot: bool,
}

/// 定时器定义
pub struct TimerDef {
    pub name: String,
    pub interval_millis: u64,
    pub callback: Option<Function>,
    pub enabled: bool,
    pub group: String,
    pub one_shot: bool,
    pub at_time: bool,
    pub send_text: String,
    /// 下次触发的绝对时间（MushClient 兼容：tFireTime 模型，无累积漂移）
    pub next_fire: Instant,
}

/// 脚本编码类型
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ScriptEncoding {
    Utf8,
    Gbk,
}

/// 脚本运行时共享状态
pub(crate) struct ScriptState {
    pub triggers: Vec<Trigger>,
    pub aliases: Vec<Alias>,
    pub timers: Vec<TimerDef>,
    /// trigger name → Vec 索引，O(1) 查找
    pub trigger_by_name: HashMap<String, usize>,
    /// trigger group → 索引列表，O(k) 批量操作
    pub trigger_groups: HashMap<String, Vec<usize>>,
    /// alias name → Vec 索引
    pub alias_by_name: HashMap<String, usize>,
    /// alias group → 索引列表
    pub alias_groups: HashMap<String, Vec<usize>>,
    /// timer name → Vec 索引
    pub timer_by_name: HashMap<String, usize>,
    /// timer group → 索引列表
    pub timer_groups: HashMap<String, Vec<usize>>,
    pub variables: HashMap<String, String>,
    pub pending_commands: Vec<String>,
    pub pending_raw: Vec<Vec<u8>>,
    pub pending_logs: Vec<String>,
    /// Tell/io.write 的行缓冲区，用于实现内联输出（如 tprint 的缩进）
    pub tell_buffer: String,
    pub recent_lines: Vec<String>,
    pub unique_counter: u64,
    pub connected: bool,
    pub connect_requested: bool,
    pub disconnect_requested: bool,
    pub host: String,
    pub port: u16,
    pub world_name: String,
    pub char_name: String,
    pub packet_count: u64,
    pub status_text: String,
    /// 当前加载脚本的编码，用于决定触发器匹配模式
    pub current_encoding: ScriptEncoding,
    /// 上次收到服务器数据的时间（用于空闲心跳检测）
    pub last_server_data: Instant,
    /// 上次发送空闲心跳（IAC NOP）的时间（用于心跳节流，每 30s 最多 1 个）
    pub last_keepalive: Instant,
    /// 待处理的面板更新（由 SetPanel/RemovePanel 产生）
    pub pending_panels: Vec<PanelUpdate>,
    /// 面板点击回调注册表: panel_name → 回调函数
    /// 由 RegisterPanelHandler API 注册, handle_panel_click 查找调用
    /// 解耦客户端与脚本: 客户端不再硬编码脚本侧函数名
    pub panel_handlers: HashMap<String, Function>,
    /// 连接建立时间（None = 未连接）
    pub connect_time: Option<Instant>,
    /// 累计接收字节数
    pub bytes_recv: u64,
    /// 累计发送字节数
    pub bytes_sent: u64,
    /// 累计重连成功次数（从 Session 同步）
    pub reconnect_count: u64,
    /// 当前连续重连尝试次数（从 Session 同步）
    pub reconnect_attempt: u32,
    /// 是否正在重连中
    pub reconnecting: bool,
    /// 上次断线原因
    pub last_disconnect_reason: Option<String>,
    /// 下次重连等待秒数（0 = 已连接或不在重连）
    pub next_retry_secs: u64,
}

/// 浮动面板更新指令（由 Lua API 产生，drain 后应用到 TerminalState）
#[derive(Debug, Clone)]
pub enum PanelUpdate {
    Set {
        name: String,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        lines: Vec<String>,
        buttons: Vec<PanelButtonDef>,
    },
    Remove {
        name: String,
    },
}

/// 连接状态（用于 reload 时保存和恢复）
#[derive(Debug, Clone)]
pub struct ConnectionState {
    pub connected: bool,
    pub host: String,
    pub port: u16,
    pub world_name: String,
    pub char_name: String,
    pub status_text: String,
}

/// Lua 引擎与脚本运行时
pub struct LuaEngine {
    pub(super) lua: Lua,
    pub(super) state: Rc<RefCell<ScriptState>>,
    pub(super) script_path: Rc<RefCell<Option<String>>>,
    pub(super) script_dir: Rc<RefCell<Option<String>>>,
    pub(super) log_dir: Rc<RefCell<Option<String>>>,
    /// 延迟触发 OnConnect 的目标时间（None 表示无待触发）
    pub(super) pending_on_connect: Option<Instant>,
    /// 连接建立后延迟执行 OnConnect 的毫秒数
    pub(super) connect_delay_ms: u64,
    /// 延迟排空期内暂存的命令队列
    /// OnConnect() 的 Execute() 和 trigger 的 Execute() 在延迟期内先暂存于此，
    /// 避免被 process_output_inner 的清零操作摧毁。
    pub(super) delayed_commands: std::cell::RefCell<Vec<String>>,

    // ---- 看门狗字段 ----
    /// Lua exec() 开始执行的系统时间戳（纳秒），0 表示未在执行
    pub(super) exec_start: Arc<AtomicU64>,
    /// 当前正在执行的定时器名称（供看门狗输出诊断信息）
    pub(super) exec_timer_name: Arc<Mutex<Option<String>>>,
    /// 看门狗线程停止信号
    pub(super) watchdog_stop: Arc<AtomicBool>,
    /// 看门狗线程句柄
    pub(super) watchdog_handle: Option<thread::JoinHandle<()>>,
}
