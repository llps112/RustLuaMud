use crossterm::{
    cursor,
    event::{DisableMouseCapture, EnableMouseCapture, KeyCode, KeyEvent, KeyModifiers},
    execute, queue,
    style::{self, Color, Print, SetForegroundColor},
    terminal::{self, ClearType},
};
use std::io::{self, Write};

use crate::connection::{SessionId, SessionInfo, SessionState};
use crate::ui::{ensure_ansi_reset, AnsiParser};

/// 可点击区域（状态栏上的 session 标签）
#[derive(Debug, Clone)]
pub struct ClickRegion {
    pub start_x: u16,
    pub end_x: u16,
    pub session_id: SessionId,
}

/// 面板按钮定义（相对面板左上角的坐标）
#[derive(Debug, Clone)]
pub struct PanelButtonDef {
    /// 按钮所在行（0 = 面板第一行）
    pub row: u16,
    /// 按钮起始列（相对面板左边缘）
    pub start_col: u16,
    /// 按钮结束列（独占，相对面板左边缘）
    pub end_col: u16,
    /// 按钮点击时传递给 Lua 的动作名
    pub action: String,
}

/// 浮动面板（overlay，绘制在输出区之上）
#[derive(Debug, Clone)]
pub struct Panel {
    pub name: String,
    /// 列位置（负数 = 从右边缘往左偏移）
    pub x: i16,
    /// 行位置（负数 = 从底边缘往上偏移）
    pub y: i16,
    pub width: u16,
    pub height: u16,
    /// 预分割的行内容（每行可含 ANSI 转义序列）
    pub lines: Vec<String>,
    /// 面板上的可点击按钮
    pub buttons: Vec<PanelButtonDef>,
}

/// 提取字符串中最后一组 CSI SGR 序列（形如 \x1b[...m），返回完整序列
fn extract_last_sgr(s: &str) -> Option<String> {
    let mut last = None;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            let mut seq = String::from("\x1b[");
            chars.next(); // consume '['
            while let Some(&next) = chars.peek() {
                seq.push(next);
                if next == 'm' {
                    chars.next(); // consume 'm'
                    break;
                }
                chars.next();
            }
            if seq.ends_with('m') {
                last = Some(seq);
            }
        }
    }
    last
}

/// 判断 SGR 序列是否为重置语义（参数全为空或 0，如 \x1b[0m、\x1b[0;0m、\x1b[m）
fn sgr_is_reset(seq: &str) -> bool {
    let inner = seq
        .strip_prefix("\x1b[")
        .and_then(|s| s.strip_suffix('m'))
        .unwrap_or("");
    inner.split(';').all(|p| p.is_empty() || p == "0")
}

/// 扫描行内所有 SGR 序列，任一为重置语义即视为含重置（兼容 \x1b[0;0m 等变体）
fn line_contains_reset(s: &str) -> bool {
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            let mut seq = String::from("\x1b[");
            chars.next(); // consume '['
            while let Some(&next) = chars.peek() {
                seq.push(next);
                chars.next();
                if next == 'm' {
                    break;
                }
            }
            if seq.ends_with('m') && sgr_is_reset(&seq) {
                return true;
            }
        }
    }
    false
}
/// 将 TAB 展开为空格（制表位每 8 列，按可见宽度计列，ANSI 序列不计列）。
///
/// 不能把 \t 原样交给 conhost 跳格：本应用的换行/截断/面板定位均按
/// `visible_width`（\t 计 0）计算，而 conhost 实际会把光标跳到下一个 8 列
/// 制表位，导致含 TAB 的行（服务端大量用 \t 对齐彩色面板）实际渲染宽度
/// 大于计算宽度，向右侵占按坐标定位的元素，产生字符重叠。
/// 展开为空格后计数与渲染严格一致；行从第 0 列开始绘制，行内列号即绝对列。
fn expand_tabs(s: &str) -> String {
    if !s.contains('\t') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut col = 0usize;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // 转义序列原样搬运不计列：CSI 直到终结字节，其余形式取 ESC+1 字符
            out.push(c);
            if let Some(&next) = chars.peek() {
                if next == '[' {
                    out.push(next);
                    chars.next();
                    while let Some(&p) = chars.peek() {
                        out.push(p);
                        chars.next();
                        if ('\u{40}'..='\u{7e}').contains(&p) {
                            break;
                        }
                    }
                } else {
                    out.push(next);
                    chars.next();
                }
            }
        } else if c == '\t' {
            let pad = 8 - (col % 8);
            out.push_str(&" ".repeat(pad));
            col += pad;
        } else {
            out.push(c);
            col += char_width(c);
        }
    }
    out
}

/// 将一行文本按可见宽度拆分为多个段
/// - ANSI 转义序列不计入宽度
/// - CJK 字符按 2 cell 计宽
/// - 跨段保持颜色状态（每段以当前 SGR 前缀开头，以 reset 结尾）
fn wrap_line_to_width(line: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![line.to_string()];
    }

    let mut segments = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;
    // 累积 SGR 状态（自上次 reset 以来的所有 SGR 序列）
    let mut sgr_state = String::new();

    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            // 收集完整 CSI 序列（按 ECMA-48 规范）
            let mut seq = String::from("\x1b[");
            chars.next(); // consume '['
                          // 参数字节 0x30-0x3F + 中间字节 0x20-0x2F
            while let Some(&next) = chars.peek() {
                if ('\x30'..='\x3f').contains(&next) || ('\x20'..='\x2f').contains(&next) {
                    seq.push(next);
                    chars.next();
                } else {
                    break;
                }
            }
            // final byte 0x40-0x7E
            let is_sgr = if let Some(&fb) = chars.peek() {
                if ('\x40'..='\x7e').contains(&fb) {
                    seq.push(fb);
                    chars.next();
                    fb == 'm'
                } else {
                    false
                }
            } else {
                false
            };
            if is_sgr {
                // SGR 序列：更新状态（兼容 \x1b[0;0m 等重置变体，与 push_output 的判定保持一致）
                if sgr_is_reset(&seq) {
                    sgr_state.clear();
                } else {
                    sgr_state.push_str(&seq);
                }
            }
            current.push_str(&seq);
        } else {
            let cw = char_width(c);
            if cw > 0 && current_width + cw > max_width {
                // 当前行已满：保存当前段，开启新段
                if !sgr_state.is_empty() {
                    current.push_str("\x1b[0m");
                }
                segments.push(std::mem::take(&mut current));
                if !sgr_state.is_empty() {
                    current.push_str(&sgr_state);
                }
                current_width = 0;
            }
            current.push(c);
            current_width += cw;
        }
    }

    if !current.is_empty() {
        segments.push(current);
    }

    if segments.is_empty() {
        segments.push(String::new());
    }

    segments
}

/// 将字符索引转换为字节偏移量
/// 字符索引 = 第 N 个 Unicode 字符，字节偏移 = 该字符在 UTF-8 编码中的起始字节位置
fn char_pos_to_byte_pos(s: &str, char_pos: usize) -> usize {
    s.char_indices()
        .nth(char_pos)
        .map(|(pos, _)| pos)
        .unwrap_or(s.len())
}

/// 按显示宽度截取字符串，确保不超过 max_width 列
#[allow(dead_code)]
fn truncate_to_width(s: &str, max_width: usize) -> String {
    let mut result = String::new();
    let mut width = 0;
    for ch in s.chars() {
        let cw = char_width(ch);
        if width + cw > max_width {
            break;
        }
        result.push(ch);
        width += cw;
    }
    result
}

/// 按显示宽度截取含 ANSI CSI 转义序列的字符串
/// - ANSI CSI 序列（\x1b[...letter）不计入宽度，且不会被截断在中间
/// - CJK 字符按 Unicode 显示宽度计宽
///
/// 用于在面板覆盖行截断输出文本，避免文本延伸到面板区域
fn truncate_ansi_to_width(s: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    let mut result = String::new();
    let mut visible = 0usize;
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' && chars.peek() == Some(&'[') {
            // CSI 序列：完整消费，不计入可见宽度
            result.push(ch);
            result.push('[');
            chars.next(); // consume '['
            while let Some(&next) = chars.peek() {
                result.push(next);
                chars.next();
                // final byte 0x40-0x7E 标志序列结束
                if ('\x40'..='\x7e').contains(&next) {
                    break;
                }
            }
        } else if ch == '\x1b' {
            // 非 CSI 转义（罕见）：消费该字符和下一个
            result.push(ch);
            if let Some(&next) = chars.peek() {
                result.push(next);
                chars.next();
            }
        } else {
            let cw = char_width(ch);
            if visible + cw > max_width {
                break;
            }
            result.push(ch);
            visible += cw;
        }
    }
    result
}

/// 计算字符串的可见宽度（忽略 ANSI 转义序列）
fn visible_width(s: &str) -> usize {
    let stripped = AnsiParser::strip_ansi(s);
    stripped.chars().map(char_width).sum()
}

/// 计算单个字符的终端显示宽度（平台感知）
///
/// Windows 传统控制台（conhost + 中文点阵/宋体字体）将 Box Drawing、
/// Block Elements 等制表符号按全角渲染（占 2 格），而 unicode-width
/// 按 Unicode 标准判定为 1 格。为保证换行/截断/光标定位与实际渲染一致，
/// conhost 下对这些字符按 2 格计。Windows Terminal（`WT_SESSION` 检测）
/// 与 Linux 终端字体遵循标准宽度，无需修正。
fn char_width(ch: char) -> usize {
    #[cfg(windows)]
    {
        let base = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if base > 0 && wide_on_conhost(is_conhost(), ch) {
            return 2;
        }
        base
    }
    #[cfg(not(windows))]
    {
        unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0)
    }
}

/// Windows conhost 中文字体下按全角（2 格）渲染的字符范围：
/// - Box Drawing (U+2500-U+257F): 制表符 ─│┌┐└┘├┤┬┴┼ 等
/// - Block Elements (U+2580-U+259F): █▓▒░▄▀▌▐ 等
///
/// 纯逻辑：conhost 判定显式传入，便于跨平台单测（不依赖进程环境变量）。
/// 非 Windows 构建下仅测试引用，故与 test 一并保留。
#[cfg(any(windows, test))]
fn wide_on_conhost(is_conhost: bool, ch: char) -> bool {
    is_conhost && gbk_full_width(ch)
}

/// 字符在 GBK 编码中是否为双字节（即 conhost 下是否按全角渲染）。
///
/// conhost 的宽度规则不是 Unicode Standard，而是“GBK 双字节即全角”：
/// ●○◎★·— 等 Unicode 判为 1 格（ambiguous）的字符，在 GBK 中均为双字节，
/// conhost 实际占 2 格（实测：状态栏 ● 在缓冲网格占两格，导致计数每遇
/// 一个符号就少 1 列，误差沿行向右累积，靠右元素被叠压）。逐个枚举
/// 符号范围总会遗漏，故以 GBK 编码字节数为准（与 conhost 行为同源），
/// 建 BMP 布尔查找表一次缓存。ASCII 永远单字节直接短路。
#[cfg(any(windows, test))]
fn gbk_full_width(ch: char) -> bool {
    static TABLE: std::sync::OnceLock<Box<[bool]>> = std::sync::OnceLock::new();
    let code = ch as u32;
    if !(0x80..0x10000).contains(&code) {
        return false; // ASCII 单字节；非 BMP 字符 conhost 不支持，保持原宽
    }
    let table = TABLE.get_or_init(|| {
        let mut t: Box<[bool]> = vec![false; 0x10000].into_boxed_slice();
        for cp in 0x80..0x10000u32 {
            if let Some(c) = char::from_u32(cp) {
                let mut utf8_buf = [0u8; 4];
                let (bytes, _, had_errors) = encoding_rs::GBK.encode(c.encode_utf8(&mut utf8_buf));
                t[cp as usize] = !had_errors && bytes.len() == 2;
            }
        }
        t
    });
    table[code as usize]
}

/// 是否运行于 Windows 传统控制台（conhost）。
///
/// Windows Terminal 会为所有子进程设置 `WT_SESSION` 环境变量，
/// 其缺失即视为 conhost（或其他同样按全角渲染制表符的旧式控制台）。
/// 环境变量在进程生命周期内不会变化，而 char_width 位于逐字符热路径，
/// 用 OnceLock 缓存，避免每字符读一次环境变量。
fn is_conhost() -> bool {
    static CACHED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CACHED.get_or_init(|| cfg!(windows) && std::env::var_os("WT_SESSION").is_none())
}

/// Windows 传统控制台 (conhost) 右侧竖向滚动条占用并截断的列数。
/// conhost 会常驻一根竖向滚动条占用最右列，而 crossterm 的 `terminal::size()`
/// 仍将其计入宽度，导致右对齐元素（状态栏 logo、右锚定面板）被截断。
/// 保留 2 列（1 列滚动条 + 1 列余量）避免贴边被切。
const CONHOST_RESERVED_COLS: u16 = 2;

/// 纯逻辑：当处于 conhost 时从原始宽度扣减保留列（跨平台可单测）。
fn reserve_conhost_cols(raw: u16, is_conhost: bool) -> u16 {
    if is_conhost {
        raw.saturating_sub(CONHOST_RESERVED_COLS)
    } else {
        raw
    }
}

/// 计算实际可用宽度。仅 Windows 传统控制台（非 Windows Terminal）需要
/// 扣减右侧竖向滚动条占用的列；Windows Terminal（环境变量 `WT_SESSION`）
/// 与非 Windows 平台返回原始宽度。
fn usable_width(raw: u16) -> u16 {
    reserve_conhost_cols(raw, is_conhost())
}

/// 计算实际可用高度。
///
/// 历史上 conhost 需要在此 `-1` 避让缓冲区末行（末行写入会升级为整屏上滚，
/// 把顶部状态栏顶出）。现在末行由 Lua 状态栏占用（见 `TerminalState::lua_row`），
/// 末行可安全承载内容，省下的那一行回归输出区，屏幕底部不再留空行。成立前提（三者缺一即会复发上滚）：
/// ① `init_screen` 已关闭 `ENABLE_WRAP_AT_EOL_OUTPUT`，行末写入不自动换行；
/// ② 全部渲染走绝对坐标，向末行的写入永不伴随 LF；
/// ③ 服务端危险转义（`ESC[2J`、`ESC D` 等）已在入口与渲染前双重剔除。
/// 保留本函数作为高度加工的唯一切点：若日后某类终端又需要避让末行，只改这里。
fn usable_height(raw: u16) -> u16 {
    raw
}

/// 剔除数据中的危险终端转义序列，仅保留 SGR（颜色/样式）。
///
/// 部分游戏服务端会下发 `ESC[2J`（清屏）、`ESC[H`（光标归位）等控制序列；
/// 裸转义如 `ESC D`（IND，物理下滚一行）同样危险。这些序列若随输出写入
/// conhost 会被直接执行：清屏/滚动会把备用屏内容（含顶部状态栏）物理顶出，
/// 造成布局永久错位。SGR 序列无副作用且用于着色，予以保留；
/// 其余 CSI、OSC、裸转义全部剔除。
pub fn strip_unsafe_escapes(s: &str) -> String {
    // 快速路径：绝大多数行不含任何转义序列，省去状态机扫描（仍有一次拷贝）
    if !s.contains('\x1b') {
        return s.to_string();
    }
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            if i + 1 < bytes.len() && bytes[i + 1] == b'[' {
                // CSI 序列：参数区后紧跟终结字节才算完整；仅保留 SGR（'m'）
                let mut j = i + 2;
                let mut fin = None;
                while j < bytes.len() && j - i < 32 {
                    if (0x40..=0x7e).contains(&bytes[j]) {
                        fin = Some(bytes[j]);
                        break;
                    }
                    j += 1;
                }
                match fin {
                    Some(b'm') => {
                        out.push_str(&s[i..=j]); // SGR 保留
                        i = j + 1;
                    }
                    Some(_) => i = j + 1, // 其余 CSI（2J/H/2K…）剔除
                    None => {
                        // 残缺序列（无终结字节）：原样保留，不吞后续内容；
                        // 回退到字符边界，避免在中文等多字节字符中间切片 panic
                        let mut e = j;
                        while e > i && !s.is_char_boundary(e) {
                            e -= 1;
                        }
                        out.push_str(&s[i..e]);
                        i = e;
                    }
                }
            } else if i + 1 < bytes.len() && bytes[i + 1] == b']' {
                // OSC 序列：剔除到 BEL/ST 或行尾（最多扫 64 字节）
                let mut j = i + 2;
                while j < bytes.len() && j - i < 64 && bytes[j] != 0x07 {
                    if bytes[j] == 0x1b && j + 1 < bytes.len() && bytes[j + 1] == b'\\' {
                        j += 1;
                        break;
                    }
                    j += 1;
                }
                i = (j + 1).min(bytes.len());
            } else if i + 1 < bytes.len() && (0x40..=0x5f).contains(&bytes[i + 1]) {
                // 裸转义 Fe 序列（ESC D=IND 下滚、ESC M=上滚、ESC 7/8、ESC c 等）：
                // 全部剔除（两字节）
                i += 2;
            } else {
                out.push('\x1b');
                i += 1;
            }
        } else {
            let c = s[i..].chars().next().unwrap();
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

/// 纯逻辑：判断缓存尺寸是否需要按新的原始视口尺寸更新。
/// 加工规则必须与 resize()/Terminal::new() 保持一致：usable_width + 下限保底，
/// 否则极小/异常尺寸下会每分钟反复触发重绘。
fn needs_resize(cached_w: u16, cached_h: u16, raw_w: u16, raw_h: u16) -> bool {
    usable_width(raw_w).max(20) != cached_w || usable_height(raw_h).max(5) != cached_h
}

/// [Windows conhost] 将控制台视口钉在缓冲区顶部（窗口 Top 移到 0）。
///
/// 根因：conhost 视口可能在写入/清屏时自行下移一行，使屏幕顶部行移出可见区域。
/// 应用用绝对坐标绘制的一切仍然正确，只是顶部若干行用户看不到；
/// 缩放窗口后短暂恢复是此机制的典型表现。底行 Lua 状态栏靠每帧守卫覆写自愈，
/// 但顶行 session 状态栏与输出区首行仍需钉顶才能保持绝对坐标与屏幕行号对齐。
///
/// 关键约束：必须**保持窗口现有宽高不变**，只把 Top 移到 0。
/// 若用绝对坐标设置固定高度（如缓存高度=窗口高度-1），会形成反馈循环：
/// 每次钉顶窗口矮一行，下一轮读到的尺寸又减 1，窗口持续缩小直到保底值。
#[cfg(windows)]
fn pin_viewport_top() {
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Coord {
        x: i16,
        y: i16,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Sr {
        l: i16,
        t: i16,
        r: i16,
        b: i16,
    }
    #[repr(C)]
    struct Csbi {
        size: Coord,
        cursor: Coord,
        attrs: u16,
        window: Sr,
        max_window: Coord,
    }
    extern "system" {
        fn GetStdHandle(n: i32) -> isize;
        fn GetConsoleScreenBufferInfo(h: isize, info: *mut Csbi) -> i32;
        fn SetConsoleWindowInfo(h: isize, absolute: i32, rect: *const Sr) -> i32;
    }
    unsafe {
        let h = GetStdHandle(-11);
        let mut info: Csbi = std::mem::zeroed();
        if GetConsoleScreenBufferInfo(h, &mut info) == 0 {
            return;
        }
        // 已在顶部：无需调用，避免无谓的系统调用与潜在闪烁
        if info.window.t == 0 {
            return;
        }
        // 保持宽高不变，仅整体上移到缓冲区顶部
        let height = info.window.b - info.window.t;
        let r = Sr {
            l: info.window.l,
            t: 0,
            r: info.window.r,
            b: height,
        };
        SetConsoleWindowInfo(h, 1, &r);
    }
}

/// 构建 session 状态栏字符串（纯逻辑，无 IO 依赖）
/// 返回 (状态栏字符串, 可点击区域列表)
fn build_status_bar(
    sessions: &[SessionInfo],
    foreground_id: SessionId,
    total_width: usize,
) -> (String, Vec<ClickRegion>) {
    let mut bar = String::new();
    let mut regions = Vec::new();
    for (i, info) in sessions.iter().enumerate() {
        let state_icon = match info.state {
            SessionState::Connected => "\x1b[32m●",
            SessionState::Disconnected => "\x1b[90m○",
            SessionState::Connecting => "\x1b[33m◎",
            SessionState::Reconnecting => "\x1b[35m⟳",
        };
        // 记录当前 x 位置（不包括 ANSI 码的可见宽度）
        let start_x = visible_width(&bar) as u16;
        if info.session_id == foreground_id {
            bar.push_str(&format!(
                "\x1b[1;37;44m[{}]{} {}\x1b[0m ",
                i + 1,
                info.name,
                state_icon
            ));
        } else {
            bar.push_str(&format!("[{}]{} {}\x1b[0m ", i + 1, info.name, state_icon));
        }
        let end_x = visible_width(&bar) as u16;
        regions.push(ClickRegion {
            start_x,
            end_x,
            session_id: info.session_id,
        });
    }
    let right_text = "RustLuaMud";
    if visible_width(&bar) + right_text.len() + 2 < total_width {
        // 当前 bar 的可见宽度
        let padding = total_width - visible_width(&bar) - right_text.len() - 2;
        for _ in 0..padding {
            bar.push(' ');
        }
        bar.push_str(&format!("\x1b[36m{}\x1b[0m", right_text));
    }
    (bar, regions)
}

/// 构建 Lua SetStatus 状态栏字符串（前台连接的自定义状态文本）
fn build_lua_status_text(
    sessions: &[SessionInfo],
    foreground_id: SessionId,
    total_width: usize,
) -> String {
    if let Some(fg) = sessions.iter().find(|s| s.session_id == foreground_id) {
        if !fg.status_text.is_empty() {
            let truncated: String = fg.status_text.chars().take(total_width).collect();
            return truncated;
        }
    }
    String::new()
}

/// 把底行状态栏文本重涂为蓝底：行首强制“亮白前景 + 蓝背景”，并保留文本原有的
/// SGR 意图（脚本常用颜色标记低血/告警），但每个 SGR 后紧跟一道 `ESC[44m`。
/// 必要性：`SetStatus` 文本普遍自带 `ESC[0m`，而 reset 会把**前景与背景一起**擦回
/// 默认属性，只在行首设一次背景会从第一个 reset 起失效（表现为蓝底不显示）。
/// 尾部不复位（由调用方补齐空格后统一 `ESC[0m`），使整行背景连空白部分都是蓝的。
/// 纯逻辑、无 IO，可单测；追加的转义序列可见宽度为 0，不影响列对齐。
fn force_blue_bg(text: &str) -> String {
    const ROW_START: &str = "\x1b[1;37;44m";
    const REASSERT_BG: &str = "\x1b[44m";
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len() + ROW_START.len() + 16);
    out.push_str(ROW_START);
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            let mut j = i + 2;
            while j < bytes.len() && !(0x40..=0x7e).contains(&bytes[j]) {
                j += 1;
            }
            // 完整 SGR（终结字节 'm'）：原样输出后补回蓝底
            if j < bytes.len() && bytes[j] == b'm' {
                out.push_str(&text[i..=j]);
                out.push_str(REASSERT_BG);
                i = j + 1;
                continue;
            }
            // 残缺转义（如被列宽截断）：丢掉 `ESC` 本身，避免把后续字串当文本打印
            i = j.min(bytes.len());
            continue;
        }
        let c = text[i..].chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// 每个 session 独立的输入状态（切换 session 时保存/恢复）
#[derive(Debug, Clone)]
pub struct InputState {
    /// 当前输入行内容
    pub input_buffer: String,
    /// 输入光标位置（字符偏移）
    pub input_cursor: usize,
    /// 命令历史
    pub history: Vec<String>,
    /// 历史浏览位置
    pub history_pos: usize,
    /// 前缀搜索的当前前缀
    pub history_prefix: String,
    /// 是否处于普通历史浏览模式
    pub history_browsing: bool,
    /// Enter 后下次按键先清空输入（模拟"全选替换"行为）
    pub clear_on_next_key: bool,
    /// Enter 后文本处于全选高亮状态
    pub text_selected: bool,
}

#[allow(clippy::derivable_impls)]
impl Default for InputState {
    fn default() -> Self {
        Self {
            input_buffer: String::new(),
            input_cursor: 0,
            history: Vec::new(),
            history_pos: 0,
            history_prefix: String::new(),
            history_browsing: false,
            clear_on_next_key: false,
            text_selected: false,
        }
    }
}

/// 终端状态（纯数据，可脱离 IO 测试）
pub struct TerminalState {
    /// 输出缓冲区（滚动回看用）
    pub output_lines: Vec<String>,
    /// 当前输入行内容
    pub input_buffer: String,
    /// 输入光标位置（字符偏移）
    pub input_cursor: usize,
    /// 命令历史
    pub history: Vec<String>,
    /// 历史浏览位置
    pub history_pos: usize,
    /// 历史最大容量
    pub history_max: usize,
    /// 前缀搜索的当前前缀（非空时 Up/Down 按前缀匹配过滤历史）
    pub history_prefix: String,
    /// 是否处于普通历史浏览模式（按Up从历史载入，非前缀搜索）
    pub history_browsing: bool,
    /// 终端宽度（列数）
    pub width: u16,
    /// 终端高度（行数）
    pub height: u16,
    /// 状态栏高度
    pub status_height: u16,
    /// Lua 状态栏高度
    pub lua_status_height: u16,
    /// 输入行高度
    pub input_height: u16,
    /// 状态栏缓存（session 连接信息）
    pub status_bar_cache: Option<String>,
    /// Lua 状态栏缓存（SetStatus 文本）
    pub lua_status_cache: Option<String>,
    /// 是否在 Enter 后保留命令栏输入内容
    pub keep_command: bool,
    /// Enter 后下次按键先清空输入（模拟"全选替换"行为）
    pub clear_on_next_key: bool,
    /// Enter 后文本处于全选高亮状态，光标在文本末尾
    pub text_selected: bool,
    /// 最近一次看到的 ANSI SGR 颜色序列，用于跨行颜色继承
    pub last_ansi_sgr: String,
    /// 输出区滚动偏移（0 = 底部，即最新输出）
    pub scroll_offset: usize,
    /// 状态栏可点击区域
    pub status_bar_regions: Vec<ClickRegion>,
    /// 浮动面板列表
    pub panels: Vec<Panel>,
}

impl TerminalState {
    /// 创建默认状态（用于测试）
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            output_lines: Vec::new(),
            input_buffer: String::new(),
            input_cursor: 0,
            history: Vec::new(),
            history_pos: 0,
            history_max: 1000,
            history_prefix: String::new(),
            history_browsing: false,
            width,
            height,
            status_height: 1,
            lua_status_height: 1,
            input_height: 1,
            status_bar_cache: None,
            lua_status_cache: None,
            keep_command: true,
            clear_on_next_key: false,
            text_selected: false,
            last_ansi_sgr: String::new(),
            scroll_offset: 0,
            status_bar_regions: Vec::new(),
            panels: Vec::new(),
        }
    }

    /// 获取输出区可用行数
    pub fn output_height(&self) -> u16 {
        self.height
            .saturating_sub(self.status_height + self.lua_status_height + self.input_height)
    }

    // ---- 行号模型（全平台唯一的坐标真值源）----
    //
    //   0 .. output_top-1        session 状态栏（可点击切 tab，贴屏幕顶行）
    //   output_top .. bottom-1   输出区
    //   input_row                输入行（光标常驻于此）
    //   lua_row                  Lua 状态栏（SetStatus 文本，贴屏幕底行）
    // 底部两行自底向上堆叠，因此高度不足时靠 saturating_sub 收敛到 0，不会下溢 panic；
    // 顶部状态栏用 saturating_sub 后剩余行自动让给输出区。

    /// session 状态栏首行（贴屏幕顶行，向下占 `status_height` 行）
    pub fn status_row(&self) -> u16 {
        0
    }

    /// 输出区首行（session 状态栏之下）
    pub fn output_top(&self) -> u16 {
        self.status_height
    }

    /// Lua 状态栏首行（贴屏幕底行，即缓冲区末行）
    pub fn lua_row(&self) -> u16 {
        self.height.saturating_sub(self.lua_status_height)
    }

    /// 输入行首行（Lua 状态栏之上，光标常驻于此）
    pub fn input_row(&self) -> u16 {
        self.lua_row().saturating_sub(self.input_height)
    }

    /// 输出区末行（exclusive），即输入行所在行
    pub fn output_bottom(&self) -> u16 {
        self.input_row()
    }

    /// 将当前输入相关状态保存到 InputState（切换 session 前调用）
    pub fn save_input_state(&self) -> InputState {
        InputState {
            input_buffer: self.input_buffer.clone(),
            input_cursor: self.input_cursor,
            history: self.history.clone(),
            history_pos: self.history_pos,
            history_prefix: self.history_prefix.clone(),
            history_browsing: self.history_browsing,
            clear_on_next_key: self.clear_on_next_key,
            text_selected: self.text_selected,
        }
    }

    /// 从 InputState 恢复输入相关状态（切换 session 后调用）
    pub fn restore_input_state(&mut self, state: &InputState) {
        self.input_buffer = state.input_buffer.clone();
        self.input_cursor = state.input_cursor;
        self.history = state.history.clone();
        self.history_pos = state.history_pos;
        self.history_prefix = state.history_prefix.clone();
        self.history_browsing = state.history_browsing;
        self.clear_on_next_key = state.clear_on_next_key;
        self.text_selected = state.text_selected;
    }

    /// 追加输出行到缓冲区（纯逻辑，不涉及 IO）
    /// 追踪最近一次看到的 ANSI SGR 颜色序列，对有文本但无自身 ANSI 的行
    /// 自动补上颜色前缀，实现行间颜色继承（如服务器在 ">" 行设置红色，
    /// 下一行"面色凝重"无 ANSI，自动继承红色）
    pub fn push_output(&mut self, line: &str) {
        // 总入口防护：无论来源（服务端数据、Lua Note、系统消息），
        // 凡进入渲染缓冲的内容都必须剔除危险转义序列（保留 SGR 颜色），
        // 否则清屏/滚动类序列会被 conhost 直接执行，物理顶出布局。
        let line = strip_unsafe_escapes(line);
        let old_len = self.output_lines.len();

        for part in line.split_inclusive('\n') {
            let trimmed = part.trim_end_matches('\n').trim_end_matches('\r');
            if !trimmed.is_empty() {
                let stripped = AnsiParser::strip_ansi(trimmed);
                // 提取本行的 SGR 序列和 reset 标记（兼容 \x1b[0;0m 等重置变体）
                let last_sgr = extract_last_sgr(trimmed);
                let has_reset = line_contains_reset(trimmed);

                if stripped.is_empty() {
                    // 纯 ANSI 行（不可见）：只更新状态，不加入输出
                    // 优先用行尾最后一个 SGR（如 \x1b[0;0m\x1b[1;37m 的末尾是 \x1b[1;37m）
                    // 只有当 last_sgr 为 None 时才根据 has_reset 清空
                    if let Some(sgr) = &last_sgr {
                        if sgr_is_reset(sgr) {
                            self.last_ansi_sgr.clear();
                        } else {
                            self.last_ansi_sgr = sgr.clone();
                        }
                    } else if has_reset {
                        self.last_ansi_sgr.clear();
                    }
                } else if last_sgr.is_some() {
                    // 有可见文本且自身带 ANSI：行首若是纯文本则补继承色（修复颜色截断：
                    // 如上一行末尾为绿色，本行开头"据说当年..."应继承绿色而非默认灰白）
                    let line = if !trimmed.starts_with('\x1b') && !self.last_ansi_sgr.is_empty() {
                        format!("{}{}", self.last_ansi_sgr, trimmed)
                    } else {
                        trimmed.to_string()
                    };
                    // 保存颜色：行末最后一个 SGR 决定状态（与纯 ANSI 分支一致）；
                    // 行中部的 reset 不影响跨行继承（如 ...<ESC>[0;0m<ESC>[1;37m后文，行末亮白应保留）
                    if last_sgr.as_deref().is_some_and(sgr_is_reset) {
                        self.last_ansi_sgr.clear();
                    } else if let Some(sgr) = &last_sgr {
                        self.last_ansi_sgr = sgr.clone();
                    }
                    self.output_lines.push(ensure_ansi_reset(&line));
                } else if !self.last_ansi_sgr.is_empty() {
                    // 可见文本，无自身 ANSI，但有继承的颜色：补上颜色；
                    // 行末追加 reset 保证自包含，但不清空继承状态——颜色持续直到服务端发送 reset
                    let mut final_line = String::new();
                    final_line.push_str(&self.last_ansi_sgr);
                    final_line.push_str(trimmed);
                    final_line.push_str("\x1b[0m");
                    self.output_lines.push(final_line);
                } else {
                    // 纯文本，无颜色继承：直接加入
                    self.output_lines.push(trimmed.to_string());
                }
            }
        }

        let new_lines = self.output_lines.len() - old_len;

        // 限制缓冲区大小
        const MAX_OUTPUT_LINES: usize = 5000;
        let drained = if self.output_lines.len() > MAX_OUTPUT_LINES {
            let drain_count = self.output_lines.len() - MAX_OUTPUT_LINES;
            self.output_lines.drain(..drain_count);
            drain_count
        } else {
            0
        };

        // 历史浏览模式（scroll_offset > 0）：调整偏移量保持视口内容稳定
        // - 新行追加到底部 → 内容向下增长，scroll_offset 需等量增加以保持视口指向同一批内容
        // - drain 从顶部移除旧行 → 每行索引前移 drained，但视口中索引对应的内容已自然前移，
        //   因此 scroll_offset 不应减去 drained（减了会额外上推 drained 行）
        // 综合公式：scroll_offset += new_lines（不减去 drained）
        if self.scroll_offset > 0 && (new_lines > 0 || drained > 0) {
            self.scroll_offset = self.scroll_offset.saturating_add(new_lines);
            let max_offset = self
                .output_lines
                .len()
                .saturating_sub(self.output_height() as usize);
            self.scroll_offset = self.scroll_offset.min(max_offset);
        }
    }

    /// 处理键盘事件，返回是否需要发送命令（纯逻辑）
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c'))
            | (KeyModifiers::CONTROL, KeyCode::Char('d')) => None,

            (KeyModifiers::NONE, KeyCode::Enter) => {
                self.scroll_offset = 0;
                let cmd = self.input_buffer.clone();
                if !cmd.is_empty() {
                    self.history.push(cmd.clone());
                    if self.history.len() > self.history_max {
                        self.history.remove(0);
                    }
                    self.history_pos = self.history.len();
                    self.history_prefix.clear();
                    self.history_browsing = false;
                }
                if self.keep_command {
                    // 保留文本，全选高亮，光标移到末尾，下次按键替换旧内容
                    self.input_cursor = self.input_buffer.chars().count();
                    self.clear_on_next_key = true;
                    self.text_selected = !self.input_buffer.is_empty();
                } else {
                    self.input_buffer.clear();
                    self.input_cursor = 0;
                    self.history_prefix.clear();
                    self.history_browsing = false;
                }
                Some(cmd)
            }

            // Ctrl+U: 清除从行首到光标的全部内容
            (KeyModifiers::CONTROL, KeyCode::Char('u')) => {
                self.clear_on_next_key = false;
                self.text_selected = false;
                let cursor_byte = char_pos_to_byte_pos(&self.input_buffer, self.input_cursor);
                self.input_buffer.drain(..cursor_byte);
                self.input_cursor = 0;
                None
            }

            // Ctrl+K: 清除从光标到行尾的全部内容
            (KeyModifiers::CONTROL, KeyCode::Char('k')) => {
                self.clear_on_next_key = false;
                self.text_selected = false;
                let cursor_byte = char_pos_to_byte_pos(&self.input_buffer, self.input_cursor);
                self.input_buffer.truncate(cursor_byte);
                None
            }

            // Ctrl+W: 删除光标前的一个单词（先跳过非空格字符，再跳过空格，与 readline unix-word-rubout 行为一致）
            (KeyModifiers::CONTROL, KeyCode::Char('w')) => {
                self.clear_on_next_key = false;
                self.text_selected = false;
                if self.input_cursor > 0 {
                    let mut pos = self.input_cursor;
                    while pos > 0
                        && self
                            .input_buffer
                            .chars()
                            .nth(pos - 1)
                            .map(|c| c != ' ')
                            .unwrap_or(false)
                    {
                        pos -= 1;
                    }
                    while pos > 0 && self.input_buffer.chars().nth(pos - 1) == Some(' ') {
                        pos -= 1;
                    }
                    let start_byte = char_pos_to_byte_pos(&self.input_buffer, pos);
                    let end_byte = char_pos_to_byte_pos(&self.input_buffer, self.input_cursor);
                    self.input_buffer.drain(start_byte..end_byte);
                    self.input_cursor = pos;
                }
                None
            }

            // Ctrl+A: 跳到行首（同 Home）
            (KeyModifiers::CONTROL, KeyCode::Char('a')) => {
                self.clear_on_next_key = false;
                self.text_selected = false;
                self.input_cursor = 0;
                None
            }

            // Ctrl+E: 跳到行尾（同 End）
            (KeyModifiers::CONTROL, KeyCode::Char('e')) => {
                self.clear_on_next_key = false;
                self.text_selected = false;
                self.input_cursor = self.input_buffer.chars().count();
                None
            }

            (KeyModifiers::NONE, KeyCode::Backspace) => {
                self.clear_on_next_key = false;
                if self.text_selected {
                    self.input_buffer.clear();
                    self.input_cursor = 0;
                    self.text_selected = false;
                    return None;
                }
                self.history_prefix.clear();
                self.history_browsing = false;
                if self.input_cursor > 0 {
                    self.input_cursor -= 1;
                    let byte_pos = char_pos_to_byte_pos(&self.input_buffer, self.input_cursor);
                    self.input_buffer.remove(byte_pos);
                }
                None
            }

            (KeyModifiers::NONE, KeyCode::Delete) => {
                self.clear_on_next_key = false;
                if self.text_selected {
                    self.input_buffer.clear();
                    self.input_cursor = 0;
                    self.text_selected = false;
                    return None;
                }
                self.history_prefix.clear();
                self.history_browsing = false;
                if self.input_cursor < self.input_buffer.chars().count() {
                    let byte_pos = char_pos_to_byte_pos(&self.input_buffer, self.input_cursor);
                    self.input_buffer.remove(byte_pos);
                }
                None
            }

            (KeyModifiers::NONE, KeyCode::Left) => {
                self.clear_on_next_key = false;
                self.text_selected = false;
                if self.input_cursor > 0 {
                    self.input_cursor -= 1;
                }
                None
            }

            (KeyModifiers::NONE, KeyCode::Right) => {
                self.clear_on_next_key = false;
                self.text_selected = false;
                if self.input_cursor < self.input_buffer.chars().count() {
                    self.input_cursor += 1;
                }
                None
            }

            (KeyModifiers::NONE, KeyCode::Up) => {
                self.clear_on_next_key = false;
                self.text_selected = false;
                if !self.input_buffer.is_empty() && !self.history_browsing {
                    // 用户手动输入文本 → 进入前缀搜索模式
                    self.history_prefix.clone_from(&self.input_buffer);
                    self.history_pos = self.history.len();
                    for pos in (0..self.history.len()).rev() {
                        if self.history[pos].starts_with(&self.history_prefix) {
                            self.history_pos = pos;
                            self.input_buffer = self.history[pos].clone();
                            self.input_cursor = self.input_buffer.chars().count();
                            self.history_browsing = true;
                            break;
                        }
                    }
                } else if !self.history_prefix.is_empty() {
                    // 前缀搜索模式：继续向上找
                    if self.history_pos > 0 {
                        for pos in (0..self.history_pos).rev() {
                            if self.history[pos].starts_with(&self.history_prefix) {
                                self.history_pos = pos;
                                self.input_buffer = self.history[pos].clone();
                                self.input_cursor = self.input_buffer.chars().count();
                                break;
                            }
                        }
                    }
                } else if self.history_pos > 0 {
                    // 输入为空：普通历史浏览
                    self.history_pos -= 1;
                    self.input_buffer = self.history[self.history_pos].clone();
                    self.input_cursor = self.input_buffer.chars().count();
                    self.history_browsing = true;
                }
                None
            }

            (KeyModifiers::NONE, KeyCode::Down) => {
                self.clear_on_next_key = false;
                self.text_selected = false;
                if !self.history_prefix.is_empty() {
                    // 前缀搜索模式：向下找
                    let mut found = false;
                    for pos in self.history_pos + 1..self.history.len() {
                        if self.history[pos].starts_with(&self.history_prefix) {
                            self.history_pos = pos;
                            self.input_buffer = self.history[pos].clone();
                            self.input_cursor = self.input_buffer.chars().count();
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        // 没有更多匹配，退出前缀搜索，恢复前缀
                        self.history_pos = self.history.len();
                        self.input_buffer = self.history_prefix.clone();
                        self.input_cursor = self.input_buffer.chars().count();
                        self.history_prefix.clear();
                        self.history_browsing = false;
                    }
                } else if self.history_pos < self.history.len() {
                    self.history_pos += 1;
                    if self.history_pos < self.history.len() {
                        self.input_buffer = self.history[self.history_pos].clone();
                    } else {
                        self.input_buffer.clear();
                        self.history_browsing = false;
                    }
                    self.input_cursor = self.input_buffer.chars().count();
                }
                None
            }

            (KeyModifiers::NONE, KeyCode::Home) => {
                self.clear_on_next_key = false;
                self.text_selected = false;
                self.input_cursor = 0;
                None
            }

            (KeyModifiers::NONE, KeyCode::End) => {
                self.clear_on_next_key = false;
                self.text_selected = false;
                if self.input_buffer.is_empty() {
                    // 输入框为空时，End 键回到底部
                    self.scroll_offset = 0;
                } else {
                    // 输入框有内容时，光标移到行尾
                    self.input_cursor = self.input_buffer.chars().count();
                }
                None
            }

            (KeyModifiers::NONE, KeyCode::PageUp) => {
                self.clear_on_next_key = false;
                self.text_selected = false;
                // 向上滚动半屏
                let scroll_amount = (self.output_height() / 2) as usize;
                let max_offset = if self.output_lines.len() > self.output_height() as usize {
                    self.output_lines.len() - self.output_height() as usize
                } else {
                    0
                };
                self.scroll_offset = (self.scroll_offset + scroll_amount).min(max_offset);
                None
            }

            (KeyModifiers::NONE, KeyCode::PageDown) => {
                self.clear_on_next_key = false;
                self.text_selected = false;
                // 向下滚动半屏
                let scroll_amount = (self.output_height() / 2) as usize;
                self.scroll_offset = self.scroll_offset.saturating_sub(scroll_amount);
                None
            }

            // 兜底：某些终端/tmux 下 backspace 可能以原始字节形式传入
            (KeyModifiers::NONE, KeyCode::Char('\x08'))
            | (KeyModifiers::NONE, KeyCode::Char('\x7f')) => {
                self.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE))
            }

            (KeyModifiers::SHIFT, KeyCode::Char(c)) | (KeyModifiers::NONE, KeyCode::Char(c)) => {
                // 全选替换：若 clear_on_next_key 为真，先清空输入
                if self.clear_on_next_key {
                    self.input_buffer.clear();
                    self.input_cursor = 0;
                    self.clear_on_next_key = false;
                    self.text_selected = false;
                    self.history_prefix.clear();
                    self.history_browsing = false;
                }
                let byte_pos = char_pos_to_byte_pos(&self.input_buffer, self.input_cursor);
                self.input_buffer.insert(byte_pos, c);
                self.input_cursor += 1;
                // 编辑输入后退出前缀搜索和历史浏览模式
                self.history_prefix.clear();
                self.history_browsing = false;
                None
            }

            _ => None,
        }
    }

    /// 更新状态栏缓存（纯逻辑）
    pub fn update_status_bar(&mut self, sessions: &[SessionInfo], foreground_id: SessionId) {
        let (bar, regions) = build_status_bar(sessions, foreground_id, self.width as usize);
        self.status_bar_cache = Some(bar);
        self.status_bar_regions = regions;
    }

    /// 更新 Lua 状态栏缓存（纯逻辑）
    pub fn update_lua_status_bar(&mut self, sessions: &[SessionInfo], foreground_id: SessionId) {
        let text = build_lua_status_text(sessions, foreground_id, self.width as usize);
        self.lua_status_cache = if text.is_empty() { None } else { Some(text) };
    }

    /// 获取当前可见的输出行
    pub fn visible_output_lines(&self) -> &[String] {
        let output_height = self.output_height() as usize;
        let total_lines = self.output_lines.len();

        if total_lines == 0 {
            return &[];
        }

        // 计算可见范围的起始位置
        // scroll_offset = 0 表示显示最新的 output_height 行
        // scroll_offset = N 表示向上滚动 N 行
        let end = if total_lines > output_height {
            total_lines - self.scroll_offset.min(total_lines - output_height)
        } else {
            total_lines
        };

        let start = end.saturating_sub(output_height);

        &self.output_lines[start..end]
    }

    /// 插入或替换同名浮动面板
    #[allow(clippy::too_many_arguments)]
    pub fn set_panel(
        &mut self,
        name: &str,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        lines: Vec<String>,
        buttons: Vec<PanelButtonDef>,
    ) {
        if let Some(panel) = self.panels.iter_mut().find(|p| p.name == name) {
            panel.x = x;
            panel.y = y;
            panel.width = width;
            panel.height = height;
            panel.lines = lines;
            panel.buttons = buttons;
        } else {
            self.panels.push(Panel {
                name: name.to_string(),
                x,
                y,
                width,
                height,
                lines,
                buttons,
            });
        }
    }

    /// 移除浮动面板
    pub fn remove_panel(&mut self, name: &str) {
        self.panels.retain(|p| p.name != name);
    }

    /// 将面板的相对坐标解析为绝对坐标
    /// x < 0: 从右边缘往左偏移；y < 0: 从底边缘往上偏移
    fn resolve_panel_position(&self, panel: &Panel) -> (u16, u16) {
        let abs_x = if panel.x < 0 {
            (self.width as i16 + panel.x).max(0) as u16
        } else {
            panel.x as u16
        };
        // panel.y 相对输出区顶部计（输出区始于 session 状态栏之下），负值相对输出区底部
        let output_top = self.output_top();
        let output_bottom = self.output_bottom();
        let abs_y = if panel.y < 0 {
            (output_bottom as i16 + panel.y).max(output_top as i16) as u16
        } else {
            output_top + panel.y as u16
        };
        (abs_x, abs_y)
    }

    /// 计算输出区每行的面板覆盖范围
    /// 返回 `Vec<Option<(u16, u16)>>`，索引为输出区内的行号（0 = 输出区顶部）
    /// 每个元素为 `Some((min_abs_x, max_abs_end))`：该行被面板覆盖的最左列和最右列（exclusive）
    /// `None` 表示该行无面板覆盖
    /// 注意：同一行上多个不重叠面板之间的间隙不会被标记为覆盖（这是已知限制，极为罕见）
    fn panel_coverage_mask(&self) -> Vec<Option<(u16, u16)>> {
        let output_top = self.output_top();
        let output_bottom = self.output_bottom();
        let output_h = (output_bottom.saturating_sub(output_top)) as usize;
        let mut mask = vec![None; output_h];

        for panel in &self.panels {
            let (abs_x, abs_y) = self.resolve_panel_position(panel);
            if abs_y >= output_bottom || abs_x >= self.width {
                continue;
            }
            let max_rows = (output_bottom - abs_y) as usize;
            let rows_to_draw = (panel.height as usize).min(max_rows);
            let abs_end = (abs_x.saturating_add(panel.width)).min(self.width);

            for i in 0..rows_to_draw {
                let row = abs_y + i as u16;
                let idx = (row - output_top) as usize;
                if idx < output_h {
                    let entry = &mut mask[idx];
                    match entry {
                        None => *entry = Some((abs_x, abs_end)),
                        Some((min_x, max_end)) => {
                            *min_x = (*min_x).min(abs_x);
                            *max_end = (*max_end).max(abs_end);
                        }
                    }
                }
            }
        }
        mask
    }

    /// 获取输入行显示内容（考虑滚动）
    pub fn input_display(&self) -> (String, usize) {
        let prompt_len: usize = 2; // "> "
        let avail_width = (self.width as usize).saturating_sub(prompt_len);
        let chars: Vec<char> = self.input_buffer.chars().collect();
        let total_chars = chars.len();

        // 计算每个字符的显示宽度
        let char_widths: Vec<usize> = chars.iter().map(|c| char_width(*c)).collect();

        // 确定显示起始字符索引：根据光标的列位置滚动
        let cursor_col_before = char_widths[..self.input_cursor].iter().sum::<usize>();
        let display_start = if cursor_col_before >= avail_width {
            // 从光标位置向前找足够宽度作为显示起点
            let mut col = 0;
            let mut start = self.input_cursor;
            for i in (0..self.input_cursor).rev() {
                if col + char_widths[i] > avail_width - 1 {
                    break;
                }
                col += char_widths[i];
                start = i;
            }
            start
        } else {
            0
        };

        // 计算显示结束字符索引
        let mut display_col = 0;
        let mut display_end = total_chars;
        for (i, &w) in char_widths
            .iter()
            .enumerate()
            .skip(display_start)
            .take(total_chars - display_start)
        {
            if display_col + w > avail_width {
                display_end = i;
                break;
            }
            display_col += w;
        }

        let display_str: String = chars[display_start..display_end].iter().collect();

        // 光标在显示区域内的列位置
        let cursor_col_in_display: usize = if self.input_cursor <= display_start {
            0
        } else {
            char_widths[display_start..self.input_cursor].iter().sum()
        };
        let cursor_x = prompt_len + cursor_col_in_display;
        (display_str, cursor_x)
    }
}

/// 终端 UI 渲染器（持有 TerminalState + IO 渲染）
pub struct Terminal {
    state: TerminalState,
    /// 缩放后待执行的延迟二次全屏重绘。
    /// conhost 对窗口缩放的 reflow/滚动可能晚于首次重绘发生，
    /// 由主循环周期性 `sync_size_if_changed()` 消费该标志补画一次。
    pending_post_resize_refresh: bool,
}

impl Terminal {
    pub fn new() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let (raw_w, raw_h) = terminal::size()?;
        // Windows conhost：右侧竖向滚动条会占用最右列而 terminal::size() 仍计入，
        // 先扣减保留列，避免右对齐的 logo/面板被截断（Windows Terminal 与非 Windows 不扣减）。
        // 尺寸保底：异常环境（如 headless pty 未设置 winsize）可能返回 0，
        // 会导致渲染代码的减法运算溢出 panic；
        // 高度加工统一走 usable_height（当前为恒等：缓冲区末行由底行 Lua 状态栏承载）
        let width = usable_width(raw_w).max(20);
        let height = usable_height(raw_h).max(5);
        // 启用鼠标捕获（跨平台标准方式）
        // Windows: EnableMouseCapture 通过 SetConsoleMode 设置 ENABLE_MOUSE_INPUT，
        //          手写 ?1000h 转义序列在 Console API 输入模式下不会被解释，导致鼠标事件不产生；
        // Unix: 发送 ?1000h 等鼠标追踪序列。
        // 终端处于鼠标应用模式时，按住 Shift 拖拽可绕过应用模式进行原生文本选中
        execute!(io::stdout(), EnableMouseCapture)?;
        // [Windows conhost] 关闭 ENABLE_WRAP_AT_EOL_OUTPUT：输出模式保留该标志时，
        // 光标在行末继续写入会自动换行，在缓冲区末行会升级为物理滚动，把备用屏
        // 内容（含顶部状态栏）顶出。本应用全部用绝对坐标定位渲染，不需要自动换行。
        #[cfg(windows)]
        {
            extern "system" {
                fn GetStdHandle(n: i32) -> isize;
                fn GetConsoleMode(h: isize, mode: *mut u32) -> i32;
                fn SetConsoleMode(h: isize, mode: u32) -> i32;
            }
            unsafe {
                let h = GetStdHandle(-11);
                let mut mode: u32 = 0;
                if GetConsoleMode(h, &mut mode) != 0 {
                    // ENABLE_WRAP_AT_EOL_OUTPUT = 0x0002
                    SetConsoleMode(h, mode & !0x0002);
                }
            }
        }
        Ok(Self {
            state: TerminalState::new(width, height),
            pending_post_resize_refresh: false,
        })
    }

    /// 获取状态引用
    #[allow(dead_code)]
    pub fn state(&self) -> &TerminalState {
        &self.state
    }

    /// 获取状态可变引用
    #[allow(dead_code)]
    pub fn state_mut(&mut self) -> &mut TerminalState {
        &mut self.state
    }

    /// 初始化屏幕
    pub fn init_screen(&mut self) -> io::Result<()> {
        let mut stdout = io::stdout();
        execute!(
            stdout,
            terminal::EnterAlternateScreen,
            terminal::Clear(ClearType::All)
        )?;
        // [Windows conhost] 备用屏缓冲可能拥有独立的输出模式，切屏后再次关闭
        // ENABLE_WRAP_AT_EOL_OUTPUT：行末继续写入自动换行，在末行会升级为物理滚动，
        // 把整屏内容（含底行状态栏）顶出。本应用全用绝对坐标渲染，无需自动换行。
        #[cfg(windows)]
        {
            extern "system" {
                fn GetStdHandle(n: i32) -> isize;
                fn GetConsoleMode(h: isize, mode: *mut u32) -> i32;
                fn SetConsoleMode(h: isize, mode: u32) -> i32;
            }
            unsafe {
                let h = GetStdHandle(-11);
                let mut mode: u32 = 0;
                if GetConsoleMode(h, &mut mode) != 0 {
                    SetConsoleMode(h, mode & !0x0002);
                }
            }
        }
        // 视口钉顶：确保视口锁在缓冲区顶部，绝对坐标与屏幕行号一致（根因修复）
        #[cfg(windows)]
        pin_viewport_top();
        self.refresh_all(&mut stdout)?;
        // 启动首帧的写入自身可能触发 conhost 滚动（如末行写入升级换行），
        // 标记延迟二次全屏重绘，由主循环周期性同步时补画（约 1 秒内）
        self.pending_post_resize_refresh = true;
        Ok(())
    }

    /// 把缓存的 session 状态栏画到其所在行（屏幕顶行）。
    ///
    /// 不清行、直接覆写并用空格补齐行宽，因此可每帧无条件调用：意外的整屏上滚或
    /// reflow 若损坏了顶行，下一帧即自愈（视口偏移另由 `pin_viewport_top` 处理）。
    fn queue_status_bar(&self, stdout: &mut io::Stdout) -> io::Result<()> {
        if let Some(ref bar) = self.state.status_bar_cache {
            let pad = (self.state.width as usize).saturating_sub(visible_width(bar));
            queue!(stdout, cursor::MoveTo(0, self.state.status_row()))?;
            queue!(stdout, Print(bar))?;
            if pad > 0 {
                queue!(stdout, Print(" ".repeat(pad)))?;
            }
        }
        Ok(())
    }

    /// 把缓存的 Lua 状态栏（SetStatus 文本）画到屏幕底行，整行蓝底白字高亮。
    ///
    /// 底行是全平台唯一被 ANSI 写入的缓冲区末行，因此：
    /// ① 无条件覆写（即使无文本也要写空白）——意外的滚动/reflow 损坏底行时下一帧自愈；
    /// ② 用空格补齐而不发 `ESC[2K`，写入本身永不伴随 LF，配合已关闭的
    ///    `ENABLE_WRAP_AT_EOL_OUTPUT` 不会升级为整屏上滚；
    /// ③ 写完必须紧跟一次光标重定位（由后续 `draw_input_line` 收尾）以消除行末
    ///    wrap-pending 挂起，否则后续任何 LF 会触发上滚。
    fn queue_lua_bar(&self, stdout: &mut io::Stdout) -> io::Result<()> {
        let width = self.state.width as usize;
        let text = self.state.lua_status_cache.as_deref().unwrap_or("");
        let pad = width.saturating_sub(visible_width(text));
        queue!(stdout, cursor::MoveTo(0, self.state.lua_row()))?;
        if text.is_empty() {
            // 无 SetStatus 文本：不把底行刷成蓝带，用默认底色空白清行
            if pad > 0 {
                queue!(stdout, Print(" ".repeat(pad)))?;
            }
        } else {
            queue!(stdout, Print(force_blue_bg(text)))?;
            if pad > 0 {
                queue!(stdout, Print(" ".repeat(pad)))?;
            }
            queue!(stdout, Print("\x1b[0m"))?;
        }
        Ok(())
    }

    /// 完整刷新屏幕（包括状态栏 + 输出区 + 面板 + 输入行）
    fn refresh_all(&self, stdout: &mut io::Stdout) -> io::Result<()> {
        // session 状态栏（屏幕顶行）
        self.queue_status_bar(stdout)?;

        self.draw_output_area(stdout)?;
        self.draw_panels(stdout)?;

        // Lua 状态栏（屏幕底行）守卫：本次重绘自身的写入也可能触发滚动而顶偏底行。
        // 放在 draw_input_line 之前，让输入行收尾把光标移回输入行，不滞留末行。
        self.queue_lua_bar(stdout)?;
        self.draw_input_line(stdout)?;
        stdout.flush()?;
        Ok(())
    }

    /// 仅刷新输出区、面板、底行 Lua 状态栏和输入行（状态栏不清行、无闪烁）
    fn refresh_output_area(&self, stdout: &mut io::Stdout) -> io::Result<()> {
        self.draw_output_area(stdout)?;
        self.draw_panels(stdout)?;
        // 底行守卫（无条件覆写）：服务端 ESC[2J] 在 conhost 中执行为整屏上滚、
        // 窗口缩放触发 reflow，都会把底行物理顶偏；而本路径为防闪烁原本不重绘
        // 状态栏，导致底行永久损坏。进程内读屏校验不可靠，改为每帧用缓存文本
        // 直接覆写底行（补齐空格、无闪烁），任何滚动下一帧即自愈。
        self.queue_lua_bar(stdout)?;
        // 放在最后：把光标移出底行，消除底行行末写入留下的 wrap-pending 挂起
        self.draw_input_line(stdout)?;
        stdout.flush()?;
        Ok(())
    }

    /// 绘制输出区所有可见行
    fn draw_output_area(&self, stdout: &mut io::Stdout) -> io::Result<()> {
        let output_height = self.state.output_height() as usize;
        let visible = self.state.visible_output_lines();
        let max_width = self.state.width as usize;

        if output_height == 0 {
            return Ok(());
        }

        // 从底部向上计算包装段，确保最新内容优先可见
        let mut all_segments: Vec<Vec<String>> = Vec::new();
        let mut total_rows = 0usize;

        for line in visible.iter().rev() {
            if total_rows >= output_height {
                break;
            }
            let expanded = expand_tabs(line);
            let segs = wrap_line_to_width(&expanded, max_width);
            total_rows += segs.len();
            all_segments.push(segs);
        }
        all_segments.reverse(); // 恢复为从上到下顺序

        // 扁平化为段列表，从顶部裁剪超出部分
        let mut flat: Vec<&String> = Vec::new();
        for segs in &all_segments {
            for seg in segs.iter() {
                flat.push(seg);
            }
        }
        let start = flat.len().saturating_sub(output_height);
        let to_render = &flat[start..];

        // 计算面板覆盖范围，避免擦除面板区域导致闪烁
        let panel_mask = self.state.panel_coverage_mask();
        let term_width = self.state.width;

        // 渲染
        for (i, seg) in to_render.iter().enumerate() {
            let row = self.state.output_top() + i as u16;
            match panel_mask.get(i).copied().flatten() {
                None => {
                    // 无面板覆盖：整行清除并打印（与原逻辑一致）
                    queue!(stdout, cursor::MoveTo(0, row))?;
                    queue!(stdout, terminal::Clear(ClearType::CurrentLine))?;
                    queue!(stdout, style::ResetColor)?;
                    queue!(stdout, Print(seg))?;
                }
                Some((min_x, max_end)) => {
                    // 面板覆盖该行 [min_x, max_end)：
                    // 只绘制 [0, min_x) 部分，不 Clear、不触碰面板区域，避免闪烁
                    let truncated = truncate_ansi_to_width(seg, min_x as usize);
                    let trunc_w = visible_width(&truncated);
                    let padding = (min_x as usize).saturating_sub(trunc_w);
                    queue!(stdout, cursor::MoveTo(0, row))?;
                    queue!(stdout, style::ResetColor)?;
                    queue!(stdout, Print(&truncated))?;
                    // 用空格清除左侧残留（旧内容可能比新内容长）
                    queue!(stdout, style::ResetColor)?;
                    queue!(stdout, Print(&" ".repeat(padding)))?;
                    // 清除面板右侧间隙（面板未到右边缘时）
                    if max_end < term_width {
                        let gap = (term_width - max_end) as usize;
                        queue!(stdout, cursor::MoveTo(max_end, row))?;
                        queue!(stdout, style::ResetColor)?;
                        queue!(stdout, Print(&" ".repeat(gap)))?;
                    }
                }
            }
        }

        // 清除剩余行
        for i in to_render.len()..output_height {
            let row = self.state.output_top() + i as u16;
            match panel_mask.get(i).copied().flatten() {
                None => {
                    queue!(stdout, cursor::MoveTo(0, row))?;
                    queue!(stdout, terminal::Clear(ClearType::CurrentLine))?;
                }
                Some((min_x, max_end)) => {
                    // 只清除左侧，不触碰面板区域
                    queue!(stdout, cursor::MoveTo(0, row))?;
                    queue!(stdout, style::ResetColor)?;
                    queue!(stdout, Print(&" ".repeat(min_x as usize)))?;
                    if max_end < term_width {
                        let gap = (term_width - max_end) as usize;
                        queue!(stdout, cursor::MoveTo(max_end, row))?;
                        queue!(stdout, style::ResetColor)?;
                        queue!(stdout, Print(&" ".repeat(gap)))?;
                    }
                }
            }
        }

        Ok(())
    }

    /// 绘制浮动面板（overlay，覆盖在输出区之上）
    fn draw_panels(&self, stdout: &mut io::Stdout) -> io::Result<()> {
        if self.state.panels.is_empty() {
            return Ok(());
        }

        let output_bottom = self.state.output_bottom();
        let term_width = self.state.width;

        for panel in &self.state.panels {
            let (abs_x, abs_y) = self.state.resolve_panel_position(panel);

            // 裁剪：面板超出输出区可见范围则跳过
            if abs_y >= output_bottom || abs_x >= term_width {
                continue;
            }

            let max_rows = (output_bottom - abs_y) as usize;
            let rows_to_draw = (panel.height as usize).min(max_rows);
            // 水平裁剪：面板宽度不超过终端剩余宽度
            let effective_width = (panel.width as usize).min((term_width - abs_x) as usize);

            for i in 0..rows_to_draw {
                let row = abs_y + i as u16;
                let col = abs_x;

                // 取内容行（不足则用空串）
                let content = panel.lines.get(i).map(|s| s.as_str()).unwrap_or("");

                // 计算内容可见宽度
                let content_w = visible_width(content);
                let panel_w = effective_width;

                // 内容截断到面板宽度
                let display_content: String = if content_w > panel_w {
                    // 按可见宽度截取
                    let mut result = String::new();
                    let mut width = 0;
                    for ch in content.chars() {
                        let cw = char_width(ch);
                        if width + cw > panel_w {
                            break;
                        }
                        result.push(ch);
                        width += cw;
                    }
                    result
                } else {
                    content.to_string()
                };

                let padding = panel_w.saturating_sub(visible_width(&display_content));

                queue!(stdout, cursor::MoveTo(col, row))?;
                // 内容由 Lua 脚本控制全部样式（背景色、前景色），Rust 只负责定位和打印
                // 不使用 Clear 以避免擦除面板左侧的输出文本
                queue!(
                    stdout,
                    Print(&display_content),
                    Print(&" ".repeat(padding)),
                    Print("\x1b[0m")
                )?;
            }
        }
        Ok(())
    }

    /// 绘制 session 状态栏（屏幕顶行）
    pub fn draw_status_bar(
        &mut self,
        stdout: &mut io::Stdout,
        sessions: &[SessionInfo],
        foreground_id: SessionId,
    ) -> io::Result<()> {
        self.state.update_status_bar(sessions, foreground_id);
        self.queue_status_bar(stdout)
    }

    /// 绘制 Lua 状态栏（屏幕底行）
    pub fn draw_lua_status_bar(
        &mut self,
        stdout: &mut io::Stdout,
        sessions: &[SessionInfo],
        foreground_id: SessionId,
    ) -> io::Result<()> {
        self.state.update_lua_status_bar(sessions, foreground_id);
        self.queue_lua_bar(stdout)
    }

    /// 追加一行输出（仅刷新输出区 + 输入行，不重绘状态栏避免闪烁）
    pub fn append_output(&mut self, line: &str) -> io::Result<()> {
        self.state.push_output(line);
        let mut stdout = io::stdout();
        self.refresh_output_area(&mut stdout)?;
        Ok(())
    }

    /// 绘制输入行
    pub fn draw_input_line(&self, stdout: &mut io::Stdout) -> io::Result<()> {
        let input_y = self.state.input_row();
        queue!(stdout, cursor::MoveTo(0, input_y))?;
        queue!(stdout, terminal::Clear(ClearType::CurrentLine))?;
        queue!(stdout, SetForegroundColor(Color::Green), Print("> "))?;
        queue!(stdout, style::ResetColor)?;

        let (display_str, cursor_x) = self.state.input_display();
        if self.state.text_selected && !display_str.is_empty() {
            // 反选效果（\x1b[7m）：高亮显示被选中的文本
            queue!(
                stdout,
                Print("\x1b[7m"),
                Print(&display_str),
                Print("\x1b[27m")
            )?;
        } else {
            queue!(stdout, Print(&display_str))?;
        }
        queue!(stdout, cursor::MoveTo(cursor_x as u16, input_y))?;
        Ok(())
    }

    /// 处理键盘事件，返回是否需要发送命令
    /// PgUp/PgDn 需要重绘输出区（滚动），其他键仅重绘输入行
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        let needs_output_redraw = matches!(
            key.code,
            KeyCode::PageUp | KeyCode::PageDown | KeyCode::End | KeyCode::Home
        );
        let result = self.state.handle_key(key);
        let mut stdout = io::stdout();
        if needs_output_redraw {
            let _ = self.refresh_output_area(&mut stdout);
        } else {
            let _ = self.draw_input_line(&mut stdout);
            let _ = stdout.flush();
        }
        result
    }

    /// 获取当前输入缓冲区内容
    #[allow(dead_code)]
    pub fn input_buffer(&self) -> &str {
        &self.state.input_buffer
    }

    /// 处理终端大小变化
    pub fn resize(&mut self, width: u16, height: u16) {
        // 尺寸保底：与 Terminal::new 保持一致，防止拖拽/最大化过程中
        // conhost 报告极小的中间尺寸导致渲染代码减法下溢而崩溃；
        // 高度同样走 usable_height（当前为恒等加工，末行由底行 Lua 状态栏占用）
        self.state.width = usable_width(width).max(20);
        self.state.height = usable_height(height).max(5);
        // 视口钉顶：缩放会重置视口位置，同时防止 conhost 再次下移视口使第 0 行不可见
        #[cfg(windows)]
        pin_viewport_top();
        let _ = self.refresh_all(&mut io::stdout());
        // conhost 的 reflow/滚动可能晚于本次重绘发生，标记延迟二次全屏重绘，
        // 由主循环周期性调用 sync_size_if_changed() 时消费（约 1 秒内补画）
        self.pending_post_resize_refresh = true;
    }

    /// 兜底尺寸同步：主动查询真实视口尺寸，与缓存不一致时重排布局。
    ///
    /// 背景：Start-Process 等方式派生时 conhost 窗口初始化存在竞态，
    /// `Terminal::new()` 查到的尺寸可能是过时的默认值；且 Windows 侧
    /// crossterm 仅在收到 WINDOW_BUFFER_SIZE_EVENT 时才转发 Resize 事件，
    /// 事件携带的还是缓冲区尺寸而非视口尺寸。缓存高度大于真实视口时，
    /// 底部行写入会不断触发视口滚动，表现为状态栏/面板随行刷新上移。
    /// 主循环周期性调用本方法即可自愈。返回 Ok(true) 表示尺寸变更并已重排；
    /// Ok(false) 表示查询成功且尺寸一致（无需任何动作）；
    /// Err 表示查询失败（如无 tty），调用方可自行决定是否回退到其他尺寸来源。
    pub fn sync_size_if_changed(&mut self) -> io::Result<bool> {
        // 视口防漂移：运行中写入/清屏也可能使 conhost 视口下移一行，
        // 每秒同步时钉一次顶，漂移最多存在 1 秒即自愈（已在顶部时零开销）
        #[cfg(windows)]
        pin_viewport_top();
        // 消费缩放后的延迟二次重绘标志（与尺寸是否变化无关）
        if self.pending_post_resize_refresh {
            self.pending_post_resize_refresh = false;
            self.refresh_all(&mut io::stdout())?;
        }
        let (raw_w, raw_h) = terminal::size()?;
        let changed = needs_resize(self.state.width, self.state.height, raw_w, raw_h);
        if !changed {
            return Ok(false);
        }
        self.resize(raw_w, raw_h);
        Ok(true)
    }

    /// 替换整个输出缓冲区（切换前台连接时使用）
    pub fn replace_output(&mut self, lines: &[String]) -> io::Result<()> {
        self.state.output_lines = lines.to_vec();
        self.state.last_ansi_sgr.clear(); // 切换连接时清除累积的颜色前缀
        self.state.scroll_offset = 0; // 切换时回到最新输出
        let mut stdout = io::stdout();
        self.refresh_all(&mut stdout)?;
        Ok(())
    }

    /// 设置/更新浮动面板（由 Lua API SetPanel 调用）
    #[allow(clippy::too_many_arguments)]
    pub fn set_panel(
        &mut self,
        name: &str,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        lines: Vec<String>,
        buttons: Vec<PanelButtonDef>,
    ) {
        self.state
            .set_panel(name, x, y, width, height, lines, buttons);
    }

    /// 移除浮动面板（由 Lua API RemovePanel 调用）
    pub fn remove_panel(&mut self, name: &str) {
        self.state.remove_panel(name);
    }

    /// 保存当前面板状态（切换 session 前调用）
    pub fn save_panels(&self) -> Vec<Panel> {
        self.state.panels.clone()
    }

    /// 恢复面板状态（切换 session 后调用）
    pub fn restore_panels(&mut self, panels: &[Panel]) {
        self.state.panels = panels.to_vec();
    }

    /// 保存当前输入状态（切换 session 前调用）
    pub fn save_input_state(&self) -> InputState {
        self.state.save_input_state()
    }

    /// 恢复输入状态（切换 session 后调用）
    pub fn restore_input_state(&mut self, state: &InputState) {
        self.state.restore_input_state(state);
        let mut stdout = io::stdout();
        let _ = self.draw_input_line(&mut stdout);
        let _ = stdout.flush();
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), DisableMouseCapture);
        let _ = execute!(io::stdout(), terminal::LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

/// 检测鼠标是否命中某个面板的按钮
/// 返回 (面板名, 动作名) 或 None
impl TerminalState {
    pub fn panel_hit_test(&self, mouse_col: u16, mouse_row: u16) -> Option<(String, String)> {
        // 与 draw_panels 保持一致的裁剪：不检测输出区以外的点击
        let output_bottom = self.output_bottom();
        for panel in &self.panels {
            let (abs_x, abs_y) = self.resolve_panel_position(panel);
            // 面板完全在输出区外则跳过
            if abs_y >= output_bottom {
                continue;
            }
            // 检查是否在面板可见范围内（裁剪到 output_bottom）
            if mouse_row < abs_y || mouse_row >= abs_y.saturating_add(panel.height) {
                continue;
            }
            if mouse_row >= output_bottom {
                continue;
            }
            if mouse_col < abs_x || mouse_col >= abs_x.saturating_add(panel.width) {
                continue;
            }
            // 在面板内，检查按钮
            let rel_row = mouse_row - abs_y;
            let rel_col = mouse_col - abs_x;
            for btn in &panel.buttons {
                if btn.row == rel_row && rel_col >= btn.start_col && rel_col < btn.end_col {
                    return Some((panel.name.clone(), btn.action.clone()));
                }
            }
        }
        None
    }
}

/// 获取状态栏可点击区域
impl Terminal {
    /// session 状态栏所在行（屏幕顶行），供鼠标命中测试使用
    pub fn status_row(&self) -> u16 {
        self.state.status_row()
    }

    pub fn click_regions(&self) -> &[ClickRegion] {
        &self.state.status_bar_regions
    }

    pub fn panel_hit_test(&self, mouse_col: u16, mouse_row: u16) -> Option<(String, String)> {
        self.state.panel_hit_test(mouse_col, mouse_row)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::field_reassign_with_default)]
    use super::*;

    #[test]
    fn test_strip_unsafe_escapes_keeps_plain_text() {
        assert_eq!(strip_unsafe_escapes("hello 世界"), "hello 世界");
        assert_eq!(strip_unsafe_escapes(""), "");
    }

    #[test]
    fn test_strip_unsafe_escapes_keeps_sgr() {
        assert_eq!(
            strip_unsafe_escapes("\x1b[32m●\x1b[0m ok"),
            "\x1b[32m●\x1b[0m ok"
        );
        assert_eq!(
            strip_unsafe_escapes("\x1b[1;37;44m[1]name\x1b[0m"),
            "\x1b[1;37;44m[1]name\x1b[0m"
        );
    }

    #[test]
    fn test_strip_unsafe_escapes_removes_control_csi() {
        // 清屏/光标归位/行清除等副作用序列必须剔除
        assert_eq!(strip_unsafe_escapes("\x1b[2J"), "");
        assert_eq!(strip_unsafe_escapes("\x1b[H"), "");
        assert_eq!(strip_unsafe_escapes("a\x1b[2Kb"), "ab");
        assert_eq!(strip_unsafe_escapes("\x1b[5;10Htext"), "text");
        // SGR 与危险序列混排：只保留 SGR 部分
        assert_eq!(
            strip_unsafe_escapes("\x1b[31mred\x1b[2J\x1b[0m"),
            "\x1b[31mred\x1b[0m"
        );
    }

    #[test]
    fn test_strip_unsafe_escapes_removes_bare_fe() {
        // 裸转义：ESC D(IND 下滚)/ESC M(RI 上滚)/ESC E 等必须剔除，
        // 它们是 conhost 物理滚动备用屏的另一类触发源
        assert_eq!(strip_unsafe_escapes("\x1bD"), "");
        assert_eq!(strip_unsafe_escapes("a\x1bDb"), "ab");
        assert_eq!(strip_unsafe_escapes("\x1bM"), "");
        // 光标移动类 CSI 同样剔除，颜色保留
        assert_eq!(
            strip_unsafe_escapes("\x1b[1;1H\x1b[32mok\x1b[D"),
            "\x1b[32mok"
        );
    }

    #[test]
    fn test_strip_unsafe_escapes_incomplete_sequence_preserved() {
        // 末尾残缺序列（无终结字节）不吞后续内容，按普通字节保留，
        // 避免因丢字节而破坏中文等多字节字符（下一字符仍按 char 边界推进）
        assert_eq!(strip_unsafe_escapes("a\x1b["), "a\x1b[");
        assert_eq!(strip_unsafe_escapes("\x1b[12"), "\x1b[12");
    }

    #[test]
    fn test_needs_resize_matches_processed_size() {
        // 加工规则与 resize()/Terminal::new() 一致：usable_width + usable_height + 下限保底
        let processed_w = usable_width(100).max(20);
        let processed_h = usable_height(30).max(5);
        // 尺寸一致 → 无需更新（不触发重绘）
        assert!(!needs_resize(processed_w, processed_h, 100, 30));
        // 任一维度不一致 → 需要更新
        assert!(needs_resize(processed_w, 24, 100, 30));
        assert!(needs_resize(processed_w, processed_h, 120, 30));
        // 异常极小尺寸被保底钳制：缓存已在保底值时不反复触发更新
        assert!(!needs_resize(20, 5, 1, 1));
        assert!(needs_resize(80, 30, 1, 1));
    }

    #[test]
    fn test_expand_tabs_basic() {
        // 制表位每 8 列：\t 跳到下一个 8 的倍数列
        assert_eq!(expand_tabs("a\tb"), "a       b"); // col1 → 补 7 空格到 col8
        assert_eq!(expand_tabs("hello\tworld"), "hello   world"); // col5 → 补 3 空格到 col8
        assert_eq!(expand_tabs("\tX"), "        X"); // col0 → 补 8 空格到 col8
        assert_eq!(expand_tabs("12345678\tX"), "12345678        X"); // col8 → 补 8 到 col16
    }

    #[test]
    fn test_expand_tabs_no_tab_passthrough() {
        assert_eq!(expand_tabs("no_tabs"), "no_tabs");
        assert_eq!(expand_tabs(""), "");
    }

    #[test]
    fn test_expand_tabs_ansi_not_counted() {
        // ANSI 序列不计列：\x1b[31m 后 ab 占 col0-1，\t 补 6 空格到 col8
        assert_eq!(expand_tabs("\x1b[31mab\tc"), "\x1b[31mab      c");
    }

    #[test]
    fn test_expand_tabs_cjk_counts_double() {
        // 汉字占 2 列："你" 后在 col2，\t 补 6 空格到 col8
        assert_eq!(expand_tabs("你\t好"), "你      好");
    }

    #[test]
    fn test_truncate_to_width_ascii() {
        assert_eq!(truncate_to_width("hello", 3), "hel");
        assert_eq!(truncate_to_width("hi", 5), "hi");
        assert_eq!(truncate_to_width("", 5), "");
    }

    #[test]
    fn test_truncate_to_width_exact() {
        assert_eq!(truncate_to_width("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_to_width_zero() {
        assert_eq!(truncate_to_width("hello", 0), "");
    }

    #[test]
    fn test_truncate_to_width_cjk() {
        assert_eq!(truncate_to_width("你好", 3), "你");
        assert_eq!(truncate_to_width("你好", 2), "你");
        assert_eq!(truncate_to_width("你好", 1), "");
    }

    #[test]
    fn test_truncate_to_width_mixed() {
        assert_eq!(truncate_to_width("a你好", 4), "a你");
        assert_eq!(truncate_to_width("a你好", 3), "a你");
    }

    #[test]
    fn test_truncate_to_width_ansi_codes_counted() {
        let result = truncate_to_width("\x1b[32mhello\x1b[0m", 5);
        assert!(!result.is_empty());
    }

    // === TerminalState 纯逻辑测试 ===

    #[test]
    fn test_state_new() {
        let state = TerminalState::new(80, 24);
        assert_eq!(state.width, 80);
        assert_eq!(state.height, 24);
        assert!(state.output_lines.is_empty());
        assert!(state.input_buffer.is_empty());
        assert_eq!(state.input_cursor, 0);
        assert!(state.history.is_empty());
    }

    #[test]
    fn test_state_push_output() {
        let mut state = TerminalState::new(80, 24);
        state.push_output("hello");
        assert_eq!(state.output_lines, vec!["hello"]);

        state.push_output("line1\nline2\n");
        assert_eq!(state.output_lines, vec!["hello", "line1", "line2"]);
    }

    #[test]
    fn test_state_push_output_trims_cr() {
        let mut state = TerminalState::new(80, 24);
        state.push_output("line\r\n");
        assert_eq!(state.output_lines, vec!["line"]);
    }

    #[test]
    fn test_state_push_output_skips_empty() {
        let mut state = TerminalState::new(80, 24);
        state.push_output("\n\n");
        assert!(state.output_lines.is_empty());
    }

    #[test]
    fn test_state_push_output_buffer_limit() {
        let mut state = TerminalState::new(80, 24);
        for i in 0..5005 {
            state.push_output(&format!("line {}", i));
        }
        assert_eq!(state.output_lines.len(), 5000);
        assert_eq!(state.output_lines[0], "line 5");
    }

    #[test]
    fn test_state_push_output_line_start_inherit_color() {
        // 行首继承色补全（复现纯阳神通功场景）：上一行末尾绿色，本行行首纯文本 + 行内 ANSI
        let mut state = TerminalState::new(80, 24);
        state.push_output("\x1b[1;32m绿色行");
        state.push_output("纯文本前缀\x1b[1;33m黄");
        assert_eq!(
            state.output_lines[1],
            "\x1b[1;32m纯文本前缀\x1b[1;33m黄\x1b[0m"
        );
    }

    #[test]
    fn test_state_push_output_line_start_with_ansi_no_prepend() {
        // 行首自带 ANSI 不重复补色
        let mut state = TerminalState::new(80, 24);
        state.push_output("\x1b[1;32m绿色行");
        state.push_output("\x1b[1;33m黄色开头");
        assert_eq!(state.output_lines[1], "\x1b[1;33m黄色开头\x1b[0m");
    }

    #[test]
    fn test_state_push_output_no_inherit_state_no_prepend() {
        // 无继承状态（首行）：行首纯文本 + 行内 ANSI，原样输出仅补行末 reset
        let mut state = TerminalState::new(80, 24);
        state.push_output("纯文本前缀\x1b[1;33m黄");
        assert_eq!(state.output_lines[0], "纯文本前缀\x1b[1;33m黄\x1b[0m");
    }

    #[test]
    fn test_state_push_output_reset_variant_clears_state() {
        // \x1b[0;0m 重置变体识别：行末已重置，后续纯文本行不应继承颜色
        let mut state = TerminalState::new(80, 24);
        state.push_output("\x1b[1;32m绿色行\x1b[0;0m");
        state.push_output("普通文本");
        assert_eq!(state.output_lines[1], "普通文本");
        assert!(state.last_ansi_sgr.is_empty());
    }

    #[test]
    fn test_state_push_output_inherit_persists_until_reset() {
        // 颜色持续：无 ANSI 行继承颜色后，状态不清空，后续行继续继承，直到服务端发送 reset
        let mut state = TerminalState::new(80, 24);
        state.push_output("\x1b[1;32m"); // 服务端单独发绿色码
        state.push_output("第一行"); // 继承绿色
        state.push_output("第二行"); // 继续继承绿色
        state.push_output("\x1b[0m重置"); // 遇到 reset
        state.push_output("第四行"); // 无颜色
        assert_eq!(state.output_lines[0], "\x1b[1;32m第一行\x1b[0m");
        assert_eq!(state.output_lines[1], "\x1b[1;32m第二行\x1b[0m");
        assert_eq!(state.output_lines[3], "第四行");
    }

    #[test]
    fn test_state_push_output_pure_ansi_with_reset_and_color() {
        // 纯 ANSI 行同时包含 reset 和后续颜色码：优先用行尾最后一个 SGR
        // 场景：服务端发送 \x1b[0;0m\x1b[1;37m（先重置再设亮白），状态应为亮白
        let mut state = TerminalState::new(80, 24);
        state.push_output("\x1b[0;0m\x1b[1;37m"); // 纯 ANSI 行：reset + 亮白
        state.push_output("你运起纯阳神通功..."); // 应继承亮白
        assert_eq!(
            state.output_lines[0],
            "\x1b[1;37m你运起纯阳神通功...\x1b[0m"
        );
        assert_eq!(state.last_ansi_sgr, "\x1b[1;37m");
    }

    #[test]
    fn test_state_push_output_visible_line_mid_reset_trailing_color() {
        // 可见行中部含 reset、行末为颜色码：状态应保留行末颜色（最后 SGR 决定）
        // 场景：合并战斗消息 ...<ESC>[0;0m<ESC>[1;37m只听见...被划开一道口子。
        let mut state = TerminalState::new(80, 24);
        state.push_output("前文\x1b[0;0m\x1b[1;37m后文");
        state.push_output("下一行"); // 应继承行末亮白，而非被中部 reset 清空
        assert_eq!(state.output_lines[1], "\x1b[1;37m下一行\x1b[0m");
        assert_eq!(state.last_ansi_sgr, "\x1b[1;37m");
    }

    #[test]
    fn test_state_output_height() {
        let state = TerminalState::new(80, 24);
        assert_eq!(state.output_height(), 21); // 24 - 1 (status) - 1 (lua_status) - 1 (input)
    }

    #[test]
    fn test_row_layout_accessors() {
        // 行号模型：顶行 session 状态栏，其下输出区，底部自下往上为 Lua 状态栏、输入行
        let state = TerminalState::new(80, 24);
        assert_eq!(state.status_row(), 0); // session 状态栏贴屏幕顶行
        assert_eq!(state.output_top(), 1);
        assert_eq!(state.input_row(), 22);
        assert_eq!(state.lua_row(), 23); // 贴屏幕底行（不再有末行避让）
        assert_eq!(state.output_bottom(), 22);
        // 输出区行数必须与 output_height() 相等，否则有行被漏画或重叠
        assert_eq!(
            (state.output_bottom() - state.output_top()) as usize,
            state.output_height() as usize
        );
        // 各区互不重叠且按序上升：status < output <= input < lua
        assert!(state.status_row() <= state.output_top());
        assert!(state.output_top() < state.output_bottom());
        assert!(state.output_bottom() <= state.input_row());
        assert!(state.input_row() < state.lua_row());
    }

    #[test]
    fn test_row_layout_min_size_no_underflow() {
        // resize 保底下限 20x5：各级仍可完整容纳
        let tiny = TerminalState::new(20, 5);
        assert_eq!(tiny.status_row(), 0);
        assert_eq!(tiny.output_top(), 1);
        assert_eq!(tiny.input_row(), 3);
        assert_eq!(tiny.lua_row(), 4); // 底行正是缓冲区末行（height-1）
        assert_eq!(tiny.output_bottom(), 3);
        assert_eq!(tiny.output_height(), 2);
        // 高度不足 3 行时 saturating_sub 收敛到 0，不得 panic 或回绕
        let zero = TerminalState::new(80, 0);
        assert_eq!(zero.status_row(), 0);
        assert_eq!(zero.input_row(), 0);
        assert_eq!(zero.lua_row(), 0);
        assert_eq!(zero.output_height(), 0);
        let one = TerminalState::new(80, 1);
        assert_eq!(one.status_row(), 0);
        assert_eq!(one.input_row(), 0);
        assert_eq!(one.lua_row(), 0);
        // 退化尺寸下输出区可能为空，行数计算必须走 saturating_sub 不下溢
        assert_eq!(
            one.output_bottom().saturating_sub(one.output_top()),
            one.output_height()
        );
    }

    // === force_blue_bg（底行蓝底重涂）测试 ===

    #[test]
    fn test_force_blue_bg_plain_text() {
        // 无转义文本：行首加亮白+蓝底，正文原样，不在尾部复位（留给调用方补空白后统一 reset）
        let out = force_blue_bg("hp 100/100");
        assert!(out.starts_with("\x1b[1;37;44m"));
        assert!(out.ends_with("hp 100/100"));
        assert!(!out.ends_with("\x1b[0m"));
    }

    #[test]
    fn test_force_blue_bg_survives_reset() {
        // SetStatus 文本自带 ESC[0m：每个 SGR 后必须补回蓝底，否则背景被 reset 擦掉
        let out = force_blue_bg("\x1b[0m血\x1b[1;31m低血\x1b[0m");
        assert_eq!(AnsiParser::strip_ansi(&out), "血低血");
        for (pos, _) in out.match_indices("\x1b[0m") {
            let after = &out[pos + 4..];
            assert!(
                after.starts_with("\x1b[44m"),
                "reset 后应紧跟蓝底补写: {out:?}"
            );
        }
        // 前景色意图保留（低血仍为亮红）
        assert!(out.contains("\x1b[1;31m"));
    }

    #[test]
    fn test_force_blue_bg_keeps_visible_width() {
        // 追加的转义序列可见宽度为 0，不影响行宽补齐
        let text = "姓名 渡伏 ★统:4分 死亡:0";
        assert_eq!(
            visible_width(&force_blue_bg(text)),
            visible_width(text),
            "重涂不应改变可见列数"
        );
    }

    #[test]
    fn test_force_blue_bg_drops_truncated_escape() {
        // 被列宽截断的残缺转义（无 'm' 终结）不得退化成字面文本
        let out = force_blue_bg("abc\x1b[1;3");
        assert_eq!(out, "\x1b[1;37;44mabc");
        assert_eq!(AnsiParser::strip_ansi(&out), "abc");
    }

    #[test]
    fn test_state_visible_output_lines() {
        let mut state = TerminalState::new(80, 24);
        state.push_output("line1");
        state.push_output("line2");
        let visible = state.visible_output_lines();
        assert_eq!(visible.len(), 2);
    }

    #[test]
    fn test_state_visible_output_lines_scroll() {
        let mut state = TerminalState::new(80, 24);
        let output_height = state.output_height() as usize;
        for i in 0..output_height + 5 {
            state.push_output(&format!("line {}", i));
        }
        let visible = state.visible_output_lines();
        assert_eq!(visible.len(), output_height);
        // Should show the last output_height lines
        assert_eq!(visible[0], format!("line 5"));
    }

    #[test]
    fn test_page_up_scrolls_half_screen() {
        let mut state = TerminalState::new(80, 24);
        let output_height = state.output_height() as usize;
        // 添加足够多的输出行
        for i in 0..output_height * 3 {
            state.push_output(&format!("line {}", i));
        }

        // 初始状态：scroll_offset = 0
        assert_eq!(state.scroll_offset, 0);

        // 按 PageUp，应该向上滚动半屏
        state.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        assert_eq!(state.scroll_offset, output_height / 2);

        // 再按一次 PageUp
        state.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        // output_height / 2 * 2 (整数除法可能少1)
        assert_eq!(state.scroll_offset, (output_height / 2) * 2);
    }

    #[test]
    fn test_page_down_scrolls_half_screen() {
        let mut state = TerminalState::new(80, 24);
        let output_height = state.output_height() as usize;
        // 添加足够多的输出行
        for i in 0..output_height * 3 {
            state.push_output(&format!("line {}", i));
        }

        // 先向上滚动
        state.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        let offset_before = state.scroll_offset;

        // 按 PageDown，应该向下滚动半屏
        state.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        assert_eq!(state.scroll_offset, offset_before - output_height / 2);
    }

    #[test]
    fn test_page_up_boundary_at_top() {
        let mut state = TerminalState::new(80, 24);
        let output_height = state.output_height() as usize;
        // 只添加少量输出行
        for i in 0..output_height + 5 {
            state.push_output(&format!("line {}", i));
        }

        // 连续按 PageUp 直到顶部
        let max_offset = 5; // 总共 5 行可以向上滚动
        for _ in 0..10 {
            state.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        }

        // 应该停在最大偏移量
        assert_eq!(state.scroll_offset, max_offset);
    }

    #[test]
    fn test_page_down_boundary_at_bottom() {
        let mut state = TerminalState::new(80, 24);
        let output_height = state.output_height() as usize;
        // 添加足够多的输出行
        for i in 0..output_height * 3 {
            state.push_output(&format!("line {}", i));
        }

        // 先向上滚动很多
        for _ in 0..10 {
            state.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        }
        assert!(state.scroll_offset > 0);

        // 连续按 PageDown 直到回到底部
        for _ in 0..10 {
            state.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        }

        // 应该回到 0
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn test_end_key_returns_to_bottom_when_input_empty() {
        let mut state = TerminalState::new(80, 24);
        let output_height = state.output_height() as usize;
        // 添加足够多的输出行
        for i in 0..output_height * 3 {
            state.push_output(&format!("line {}", i));
        }

        // 先向上滚动
        state.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        assert!(state.scroll_offset > 0);

        // 输入框为空时按 End，应该回到底部
        state.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn test_end_key_moves_cursor_when_input_has_content() {
        let mut state = TerminalState::new(80, 24);
        state.input_buffer = "hello".to_string();
        state.input_cursor = 0;

        // 输入框有内容时按 End，光标应该移到行尾
        state.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        assert_eq!(state.input_cursor, 5);
        // scroll_offset 不应该改变
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn test_visible_output_lines_with_scroll_offset() {
        let mut state = TerminalState::new(80, 24);
        let output_height = state.output_height() as usize;
        // 添加足够多的输出行（至少 output_height + 10）
        let total = output_height + 10;
        for i in 0..total {
            state.push_output(&format!("line {}", i));
        }

        // 初始状态：显示最后 output_height 行
        let visible = state.visible_output_lines();
        assert_eq!(visible[0], format!("line {}", total - output_height));

        // 向上滚动 3 行
        state.scroll_offset = 3;
        let visible = state.visible_output_lines();
        assert_eq!(visible[0], format!("line {}", total - output_height - 3));

        // 向上滚动 5 行
        state.scroll_offset = 5;
        let visible = state.visible_output_lines();
        assert_eq!(visible[0], format!("line {}", total - output_height - 5));
    }

    #[test]
    fn test_new_output_preserves_scroll_viewport() {
        let mut state = TerminalState::new(80, 24);
        let output_height = state.output_height() as usize;
        // 添加足够多的输出行
        for i in 0..output_height * 3 {
            state.push_output(&format!("line {}", i));
        }

        // 向上滚动
        state.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        let offset_before = state.scroll_offset;
        assert!(offset_before > 0);

        // 记录当前可见行
        let visible_before: Vec<String> = state.visible_output_lines().to_vec();

        // 添加新输出
        state.push_output("new line");

        // scroll_offset 应增加，保持视口内容不变
        assert_eq!(state.scroll_offset, offset_before + 1);

        // 视口内容应保持相同
        let visible_after: Vec<String> = state.visible_output_lines().to_vec();
        assert_eq!(visible_before, visible_after);
    }

    #[test]
    fn test_state_handle_key_enter() {
        let mut state = TerminalState::new(80, 24);
        state.keep_command = false; // 覆盖默认值，测试清空行为
        state.input_buffer = "hello".to_string();
        state.input_cursor = 5;
        let result = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(result, Some("hello".to_string()));
        assert!(state.input_buffer.is_empty());
        assert_eq!(state.input_cursor, 0);
        assert_eq!(state.history, vec!["hello"]);
    }

    #[test]
    fn test_keep_command_enter_preserves_buffer() {
        let mut state = TerminalState::new(80, 24);
        state.keep_command = true;
        state.input_buffer = "hello".to_string();
        state.input_cursor = 5;
        let result = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(result, Some("hello".to_string()));
        // 缓冲区应被保留
        assert_eq!(state.input_buffer, "hello");
        // 光标移到末尾，全选高亮
        assert_eq!(state.input_cursor, 5);
        assert!(state.clear_on_next_key);
        assert!(state.text_selected);
        assert_eq!(state.history, vec!["hello"]);
    }

    #[test]
    fn test_keep_command_clear_on_next_key_replaces() {
        let mut state = TerminalState::new(80, 24);
        state.keep_command = true;
        state.input_buffer = "hello".to_string();
        state.input_cursor = 5;
        // Enter 提交，保留文本
        let _ = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(state.input_buffer, "hello");
        assert!(state.clear_on_next_key);
        // 输入字符 'w'，应替换旧文本
        let _ = state.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE));
        assert_eq!(state.input_buffer, "w");
        assert_eq!(state.input_cursor, 1);
        assert!(!state.clear_on_next_key);
        // 继续输入 'o'，正常追加
        let _ = state.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE));
        assert_eq!(state.input_buffer, "wo");
    }

    #[test]
    fn test_keep_command_clear_on_next_key_cancel_by_nav() {
        let mut state = TerminalState::new(80, 24);
        state.keep_command = true;
        state.input_buffer = "hello".to_string();
        state.input_cursor = 5;
        let _ = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(state.clear_on_next_key);
        assert!(state.text_selected);
        // 按方向键取消全选状态（光标在末尾，Left 移到 "o" 之前）
        let _ = state.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert!(!state.clear_on_next_key);
        assert!(!state.text_selected);
        // clear_on_next_key 已取消，光标在末尾前，正常插入
        let _ = state.handle_key(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE));
        assert_eq!(state.input_buffer, "hellXo");
        // End 再到末尾
        let _ = state.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        // 清除 clear_on_next_key
        let _ = state.handle_key(KeyEvent::new(KeyCode::Char('!'), KeyModifiers::NONE));
        assert_eq!(state.input_buffer, "hellXo!");
    }

    #[test]
    fn test_keep_command_toggle_on_by_default() {
        let mut state = TerminalState::new(80, 24);
        // 默认 keep_command = true
        assert!(state.keep_command);
        state.input_buffer = "test".to_string();
        state.input_cursor = 4;
        let _ = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        // 应保留（默认行为）
        assert_eq!(state.input_buffer, "test");
    }

    #[test]
    fn test_state_handle_key_enter_empty() {
        let mut state = TerminalState::new(80, 24);
        let result = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(result, Some(String::new()));
        assert!(state.history.is_empty());
    }

    #[test]
    fn test_state_handle_key_char() {
        let mut state = TerminalState::new(80, 24);
        state.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert_eq!(state.input_buffer, "a");
        assert_eq!(state.input_cursor, 1);

        state.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        assert_eq!(state.input_buffer, "ab");
        assert_eq!(state.input_cursor, 2);
    }

    #[test]
    fn test_state_handle_key_backspace() {
        let mut state = TerminalState::new(80, 24);
        state.input_buffer = "ab".to_string();
        state.input_cursor = 2;
        state.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(state.input_buffer, "a");
        assert_eq!(state.input_cursor, 1);
    }

    #[test]
    fn test_state_handle_key_backspace_at_start() {
        let mut state = TerminalState::new(80, 24);
        state.input_buffer = "a".to_string();
        state.input_cursor = 0;
        state.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(state.input_buffer, "a");
        assert_eq!(state.input_cursor, 0);
    }

    #[test]
    fn test_state_handle_key_delete() {
        let mut state = TerminalState::new(80, 24);
        state.input_buffer = "ab".to_string();
        state.input_cursor = 0;
        state.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        assert_eq!(state.input_buffer, "b");
        assert_eq!(state.input_cursor, 0);
    }

    #[test]
    fn test_state_handle_key_delete_at_end() {
        let mut state = TerminalState::new(80, 24);
        state.input_buffer = "a".to_string();
        state.input_cursor = 1;
        state.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        assert_eq!(state.input_buffer, "a");
        assert_eq!(state.input_cursor, 1);
    }

    #[test]
    fn test_state_handle_key_left_right() {
        let mut state = TerminalState::new(80, 24);
        state.input_buffer = "abc".to_string();
        state.input_cursor = 3;

        state.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(state.input_cursor, 2);

        state.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(state.input_cursor, 1);

        state.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(state.input_cursor, 2);
    }

    #[test]
    fn test_state_handle_key_left_at_start() {
        let mut state = TerminalState::new(80, 24);
        state.input_cursor = 0;
        state.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(state.input_cursor, 0);
    }

    #[test]
    fn test_state_handle_key_right_at_end() {
        let mut state = TerminalState::new(80, 24);
        state.input_buffer = "a".to_string();
        state.input_cursor = 1;
        state.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(state.input_cursor, 1);
    }

    #[test]
    fn test_clear_on_next_key_home_end_cancel() {
        let mut state = TerminalState::new(80, 24);
        state.input_buffer = "abc".to_string();
        state.input_cursor = 2;

        state.handle_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
        assert_eq!(state.input_cursor, 0);

        state.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        assert_eq!(state.input_cursor, 3);
    }

    #[test]
    fn test_state_handle_key_history_up_down() {
        let mut state = TerminalState::new(80, 24);
        state.history = vec!["cmd1".to_string(), "cmd2".to_string()];
        state.history_pos = 2;

        state.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(state.input_buffer, "cmd2");
        assert_eq!(state.history_pos, 1);

        state.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(state.input_buffer, "cmd1");
        assert_eq!(state.history_pos, 0);

        state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(state.input_buffer, "cmd2");
        assert_eq!(state.history_pos, 1);

        state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert!(state.input_buffer.is_empty());
        assert_eq!(state.history_pos, 2);
    }

    #[test]
    fn test_state_handle_key_ctrl_c_returns_none() {
        let mut state = TerminalState::new(80, 24);
        let result = state.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(result.is_none());
    }

    #[test]
    fn test_state_handle_key_ctrl_d_returns_none() {
        let mut state = TerminalState::new(80, 24);
        let result = state.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
        assert!(result.is_none());
    }

    #[test]
    fn test_state_input_display() {
        let mut state = TerminalState::new(80, 24);
        state.input_buffer = "hello".to_string();
        state.input_cursor = 5;
        let (display, cursor_x) = state.input_display();
        assert_eq!(display, "hello");
        assert_eq!(cursor_x, 7); // 2 (prompt) + 5
    }

    #[test]
    fn test_state_min_clamped_size_no_overflow() {
        // Terminal::new 的尺寸保底为 20x5（headless pty 可能返回 0x0），
        // 验证保底尺寸下 input_display 不 panic（回归：0 宽时减法溢出）
        let mut state = TerminalState::new(20, 5);
        state.input_buffer = "a".repeat(50);
        state.input_cursor = 50;
        let (display, _cursor_x) = state.input_display();
        assert!(!display.is_empty());
    }

    #[test]
    fn test_state_input_display_scroll() {
        let mut state = TerminalState::new(10, 24);
        state.input_buffer = "abcdefghij".to_string(); // 10 chars
        state.input_cursor = 10;
        let (display, _cursor_x) = state.input_display();
        // avail_width = 10 - 2 = 8, cursor > avail_width so scroll
        assert!(display.len() <= 8);
    }

    #[test]
    fn test_state_insert_char_in_middle() {
        let mut state = TerminalState::new(80, 24);
        state.input_buffer = "ac".to_string();
        state.input_cursor = 1;
        state.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        assert_eq!(state.input_buffer, "abc");
        assert_eq!(state.input_cursor, 2);
    }

    #[test]
    fn test_build_status_bar_empty() {
        let (bar, regions) = build_status_bar(&[], SessionId(0), 80);
        assert!(bar.contains("RustLuaMud"));
        assert!(regions.is_empty());
    }

    #[test]
    fn test_build_status_bar_with_sessions() {
        let sessions = vec![
            SessionInfo {
                session_id: SessionId(0),
                name: "mud1".to_string(),
                state: SessionState::Connected,
                status_text: String::new(),
            },
            SessionInfo {
                session_id: SessionId(1),
                name: "mud2".to_string(),
                state: SessionState::Disconnected,
                status_text: String::new(),
            },
        ];
        let (bar, regions) = build_status_bar(&sessions, SessionId(0), 80);
        assert!(bar.contains("mud1"));
        assert!(bar.contains("mud2"));
        assert!(bar.contains("RustLuaMud"));
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0].session_id, SessionId(0));
        assert_eq!(regions[1].session_id, SessionId(1));
        assert!(regions[1].start_x >= regions[0].end_x);
    }

    #[test]
    fn test_build_status_bar_foreground_highlight() {
        let sessions = vec![SessionInfo {
            session_id: SessionId(0),
            name: "mud1".to_string(),
            state: SessionState::Connected,
            status_text: String::new(),
        }];
        let (bar, _regions) = build_status_bar(&sessions, SessionId(0), 80);
        // Foreground should have bold+white-on-blue highlight
        assert!(bar.contains("\x1b[1;37;44m[1]"));
    }

    #[test]
    fn test_state_update_status_bar() {
        let mut state = TerminalState::new(80, 24);
        let sessions = vec![SessionInfo {
            session_id: SessionId(0),
            name: "test".to_string(),
            state: SessionState::Connected,
            status_text: String::new(),
        }];
        state.update_status_bar(&sessions, SessionId(0));
        assert!(state.status_bar_cache.is_some());
        let bar = state.status_bar_cache.as_ref().unwrap();
        assert!(bar.contains("test"));
        // 验证可点击区域
        assert_eq!(state.status_bar_regions.len(), 1);
        assert_eq!(state.status_bar_regions[0].session_id, SessionId(0));
        assert!(state.status_bar_regions[0].end_x > state.status_bar_regions[0].start_x);
    }

    #[test]
    fn test_reserve_conhost_cols() {
        // 跨平台纯逻辑：conhost 下扣减保留列，其他情况原样返回
        assert_eq!(reserve_conhost_cols(80, false), 80);
        assert_eq!(reserve_conhost_cols(80, true), 80 - CONHOST_RESERVED_COLS);
        // 极小宽度下 saturating_sub 不下溢
        assert_eq!(reserve_conhost_cols(1, true), 0);
        assert_eq!(reserve_conhost_cols(0, true), 0);
    }

    #[test]
    fn test_wide_on_conhost() {
        // conhost 下 GBK 双字节字符按 2 格计（char_width 返回 2 的前提）：
        // 旧实现只枚举 U+2500-259F，●○◎★·— 等符号被漏判导致行内计数错位叠压
        assert!(wide_on_conhost(true, '─')); // U+2500 Box Drawing
        assert!(wide_on_conhost(true, '█')); // U+2588 Block Elements
        assert!(wide_on_conhost(true, '●')); // U+25CF 状态栏分隔符（旧范围未覆盖→叠字根因）
        assert!(wide_on_conhost(true, '○')); // U+25CB
        assert!(wide_on_conhost(true, '◎')); // U+25CE
        assert!(wide_on_conhost(true, '★')); // U+2605 Lua 状态行
        assert!(wide_on_conhost(true, '■')); // U+25A0 新规则下也是双字节
        assert!(wide_on_conhost(true, '—')); // U+2014 破折号
        assert!(wide_on_conhost(true, '·')); // U+00B7 间隔号
                                             // Windows Terminal / Linux：不做修正
        assert!(!wide_on_conhost(false, '─'));
        assert!(!wide_on_conhost(false, '●'));
        // ASCII（GBK 单字节）任何终端都不做全角修正
        assert!(!wide_on_conhost(true, 'A'));
        assert!(!wide_on_conhost(true, ' '));
        // GBK 无法编码的字符（替换为 '?' 单字节）保持原宽
        assert!(!wide_on_conhost(true, '\u{1F600}')); // emoji，非 BMP，短路返回 false
        assert!(!wide_on_conhost(true, '\u{0374}')); // Greek 形似号，GBK 不可编码
                                                     // 私用区 U+E000-U+E864 是 GBK 扩展映射的双字节字符，conhost 按全角渲染
        assert!(wide_on_conhost(true, '\u{E000}'));
    }

    #[test]
    fn test_usable_width_bounds_and_wiring() {
        // usable_width 结果应始终在 [raw - reserve, raw] 区间
        let raw = 80u16;
        let w = usable_width(raw);
        assert!(w <= raw);
        assert!(w >= raw - CONHOST_RESERVED_COLS);
        // 与非 Windows / Windows Terminal 行为一致：不扣减
        if !cfg!(windows) || std::env::var_os("WT_SESSION").is_some() {
            assert_eq!(w, raw);
        }
    }

    #[test]
    fn test_build_status_bar_fits_within_conhost_width() {
        // conhost 扣减后的宽度下，右对齐 logo 仍完整且不超出 total_width
        let w = reserve_conhost_cols(80, true) as usize;
        let (bar, _) = build_status_bar(&[], SessionId(0), w);
        assert!(bar.contains("RustLuaMud"));
        assert!(visible_width(&bar) <= w);
    }

    #[test]
    fn test_build_status_bar_logo_not_overflow_width() {
        // 含 CJK 会话名（双宽）时，状态栏可见宽度不应超出 total_width
        for &tw in &[24usize, 40, 80, 120] {
            let sessions = vec![SessionInfo {
                session_id: SessionId(0),
                name: "侠客行".to_string(),
                state: SessionState::Connected,
                status_text: String::new(),
            }];
            let (bar, _) = build_status_bar(&sessions, SessionId(0), tw);
            assert!(visible_width(&bar) <= tw, "bar overflow at width {}", tw);
        }
    }

    // ---- 新增覆盖测试 ----

    #[test]
    fn test_push_output_with_ansi_auto_reset() {
        let mut state = TerminalState::new(80, 24);
        // 行尾没有 \x1b[0m，应自动追加
        state.push_output("\x1b[31mred text");
        assert_eq!(state.output_lines.len(), 1);
        assert!(state.output_lines[0].ends_with("\x1b[0m"));
        assert!(state.output_lines[0].starts_with("\x1b[31m"));
    }

    #[test]
    fn test_push_output_with_ansi_already_reset() {
        let mut state = TerminalState::new(80, 24);
        // 行尾已有 \x1b[0m，不应重复追加
        state.push_output("\x1b[32mgreen\x1b[0m");
        assert_eq!(state.output_lines[0], "\x1b[32mgreen\x1b[0m");
    }

    #[test]
    fn test_push_output_plain_text() {
        let mut state = TerminalState::new(80, 24);
        state.push_output("plain text");
        assert_eq!(state.output_lines[0], "plain text");
    }

    #[test]
    fn test_keep_command_empty_enter_no_history() {
        let mut state = TerminalState::new(80, 24);
        state.keep_command = true;
        // 空 Enter，不应加入历史
        let result = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(result, Some(String::new()));
        assert!(state.history.is_empty());
        // input_buffer 仍为空，clear_on_next_key 已置位（不影响）
        assert!(state.clear_on_next_key);
        assert!(!state.text_selected);
        assert!(state.input_buffer.is_empty());
    }

    #[test]
    fn test_keep_command_ctrl_c_clears_flag() {
        let mut state = TerminalState::new(80, 24);
        state.keep_command = true;
        state.input_buffer = "hello".to_string();
        let _ = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(state.clear_on_next_key);
        // Ctrl+C 不清除标志（直接返回 None），但输入内容应保持不变
        let result = state.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(result, None);
        assert_eq!(state.input_buffer, "hello");
    }

    #[test]
    fn test_input_display_scroll() {
        let mut state = TerminalState::new(10, 24); // 窄终端触发滚动
        state.input_buffer = "hello world".to_string();
        // 光标在末尾（超出可用宽度），应从偏移显示
        state.input_cursor = state.input_buffer.chars().count();
        state.input_height = 1;
        let (display, cursor_x) = state.input_display();
        // 可用宽度 = 10 - 2("> ") = 8
        // cursor = 11, display_start = 11 - 8 + 1 = 4
        // display = "o world" (偏移 4, 取 8 字符)
        assert!(!display.is_empty());
        assert!(cursor_x >= 2); // 至少是 prompt 宽度
    }

    #[test]
    fn test_input_display_no_scroll_needed() {
        let mut state = TerminalState::new(80, 24);
        state.input_buffer = "hi".to_string();
        state.input_cursor = 2;
        let (display, cursor_x) = state.input_display();
        assert_eq!(display, "hi");
        assert_eq!(cursor_x, 4); // "> " + 2
    }

    #[test]
    fn test_state_handle_key_arrow_left_right() {
        let mut state = TerminalState::new(80, 24);
        state.keep_command = true;
        state.input_buffer = "ab".to_string();
        state.input_cursor = 2;
        state.clear_on_next_key = true;
        // 按左键应取消 clear_on_next_key
        let _ = state.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert!(!state.clear_on_next_key);
        assert_eq!(state.input_cursor, 1);
    }

    #[test]
    fn test_clear_on_next_key_home_end_flag() {
        let mut state = TerminalState::new(80, 24);
        state.input_buffer = "hello".to_string();
        state.input_cursor = 3;
        state.clear_on_next_key = true;
        state.text_selected = true;
        // Home 取消标志并回到开头
        let _ = state.handle_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
        assert!(!state.clear_on_next_key);
        assert!(!state.text_selected);
        assert_eq!(state.input_cursor, 0);
        // End 到末尾
        let _ = state.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        assert_eq!(state.input_cursor, 5);
    }

    #[test]
    fn test_text_selected_backspace_clears_buffer() {
        let mut state = TerminalState::new(80, 24);
        state.input_buffer = "hello".to_string();
        state.input_cursor = 5;
        state.text_selected = true;
        // Backspace 清空缓冲区
        let _ = state.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert!(state.input_buffer.is_empty());
        assert_eq!(state.input_cursor, 0);
        assert!(!state.text_selected);
    }

    #[test]
    fn test_text_selected_delete_clears_buffer() {
        let mut state = TerminalState::new(80, 24);
        state.input_buffer = "hello".to_string();
        state.input_cursor = 5;
        state.text_selected = true;
        // Delete 清空缓冲区
        let _ = state.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        assert!(state.input_buffer.is_empty());
        assert_eq!(state.input_cursor, 0);
        assert!(!state.text_selected);
    }

    #[test]
    fn test_text_selected_cancelled_by_nav_keys() {
        let mut state = TerminalState::new(80, 24);
        state.input_buffer = "hello".to_string();
        state.text_selected = true;
        state.input_cursor = 5;

        // Right 取消
        let _ = state.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert!(!state.text_selected);
        state.text_selected = true;

        // Down 取消
        let _ = state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert!(!state.text_selected);
        state.text_selected = true;

        // Up 取消
        let _ = state.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert!(!state.text_selected);
        state.text_selected = true;

        // End 取消
        let _ = state.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        assert!(!state.text_selected);
        state.text_selected = true;

        // PgUp 取消
        let _ = state.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        assert!(!state.text_selected);
        state.text_selected = true;

        // PgDn 取消
        let _ = state.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        assert!(!state.text_selected);
    }

    #[test]
    fn test_text_selected_not_set_when_keep_command_false() {
        let mut state = TerminalState::new(80, 24);
        state.keep_command = false;
        state.input_buffer = "hello".to_string();
        state.input_cursor = 5;
        let _ = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        // 缓冲区应清空，text_selected 应为 false
        assert!(state.input_buffer.is_empty());
        assert_eq!(state.input_cursor, 0);
        assert!(!state.text_selected);
    }

    #[test]
    fn test_state_handle_key_delete_in_middle() {
        let mut state = TerminalState::new(80, 24);
        state.clear_on_next_key = true;
        state.input_buffer = "abcd".to_string();
        state.input_cursor = 2;
        // Delete 取消 clear_on_next_key 并删除当前字符
        let _ = state.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        assert!(!state.clear_on_next_key);
        assert_eq!(state.input_buffer, "abd");
    }

    #[test]
    fn test_state_handle_key_up_down_history() {
        let mut state = TerminalState::new(80, 24);
        state.keep_command = false; // 默认清空，方便测试历史
        state.input_buffer = "cmd1".to_string();
        let _ = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        state.input_buffer = "cmd2".to_string();
        let _ = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        // Up 进入历史
        let _ = state.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(state.input_buffer, "cmd2");
        // Up 再次
        let _ = state.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(state.input_buffer, "cmd1");
        // Down
        let _ = state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(state.input_buffer, "cmd2");
        // Down 到底回到空白
        let _ = state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert!(state.input_buffer.is_empty());
    }

    #[test]
    fn test_state_handle_key_up_down_history_cancels_flag() {
        let mut state = TerminalState::new(80, 24);
        state.keep_command = true;
        state.input_buffer = "cmd".to_string();
        let _ = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(state.clear_on_next_key);
        // Up 取消标志
        let _ = state.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert!(!state.clear_on_next_key);
    }

    // === extract_last_sgr 测试 ===

    #[test]
    fn test_extract_last_sgr_none() {
        assert_eq!(extract_last_sgr("plain text"), None);
        assert_eq!(extract_last_sgr(""), None);
    }

    #[test]
    fn test_extract_last_sgr_single() {
        assert_eq!(
            extract_last_sgr("\x1b[31mred text"),
            Some("\x1b[31m".to_string())
        );
    }

    #[test]
    fn test_extract_last_sgr_at_end() {
        assert_eq!(
            extract_last_sgr("> \x1b[1;31m"),
            Some("\x1b[1;31m".to_string())
        );
    }

    #[test]
    fn test_extract_last_sgr_multiple() {
        assert_eq!(
            extract_last_sgr("\x1b[33mhello\x1b[32mworld\x1b[31m"),
            Some("\x1b[31m".to_string())
        );
    }

    #[test]
    fn test_extract_last_sgr_with_reset() {
        assert_eq!(
            extract_last_sgr("\x1b[31mred\x1b[0m"),
            Some("\x1b[0m".to_string())
        );
    }

    #[test]
    fn test_extract_last_sgr_only_ansi() {
        assert_eq!(
            extract_last_sgr("\x1b[1;31m"),
            Some("\x1b[1;31m".to_string())
        );
    }

    #[test]
    fn test_extract_last_sgr_bright_color() {
        assert_eq!(extract_last_sgr("\x1b[91m"), Some("\x1b[91m".to_string()));
    }

    // === 颜色继承测试 ===

    #[test]
    fn test_push_output_plain_text_no_inherit() {
        let mut state = TerminalState::new(80, 24);
        // 无颜色前缀时，纯文本不变
        state.push_output("plain text");
        assert_eq!(state.output_lines[0], "plain text");
        assert!(state.last_ansi_sgr.is_empty());
    }

    #[test]
    fn test_push_output_colored_line_saves_sgr() {
        let mut state = TerminalState::new(80, 24);
        state.push_output("\x1b[1;31m> ");
        // 行尾应自动追加 reset
        assert_eq!(state.output_lines[0], "\x1b[1;31m> \x1b[0m");
        // 颜色应保存到 last_ansi_sgr
        assert_eq!(state.last_ansi_sgr, "\x1b[1;31m");
    }

    #[test]
    fn test_push_output_colored_line_with_reset_clears_sgr() {
        let mut state = TerminalState::new(80, 24);
        state.push_output("\x1b[1;31m> \x1b[0m");
        // 自带 reset 的行应清除 last_ansi_sgr
        assert!(state.last_ansi_sgr.is_empty());
    }

    #[test]
    fn test_push_output_inherit_color_to_next_line() {
        let mut state = TerminalState::new(80, 24);
        // 模拟：同一批次收到 "> "（带红）和"面色凝重"（无 ANSI）
        state.push_output("\x1b[1;31m> \n面色凝重");
        assert_eq!(state.output_lines.len(), 2);
        // 第1行：红色 >
        assert_eq!(state.output_lines[0], "\x1b[1;31m> \x1b[0m");
        // 第2行：继承红色 → 自动补上 \x1b[1;31m
        assert_eq!(state.output_lines[1], "\x1b[1;31m面色凝重\x1b[0m");
    }

    #[test]
    fn test_push_output_inherit_does_not_override_own_ansi() {
        let mut state = TerminalState::new(80, 24);
        state.push_output("\x1b[1;31m> ");
        assert_eq!(state.last_ansi_sgr, "\x1b[1;31m");
        // 下一行有自身 ANSI，不应被覆盖
        state.push_output("\x1b[32mgreen text");
        assert_eq!(state.output_lines[1], "\x1b[32mgreen text\x1b[0m");
        assert_eq!(state.last_ansi_sgr, "\x1b[32m"); // 更新为绿色
    }

    #[test]
    fn test_push_output_pure_ansi_line_saves_sgr() {
        let mut state = TerminalState::new(80, 24);
        // 纯 ANSI 行（不可见字符）
        state.push_output("\x1b[1;31m");
        // 不可见行不加入输出
        assert!(state.output_lines.is_empty());
        // 但状态已保存
        assert_eq!(state.last_ansi_sgr, "\x1b[1;31m");
    }

    #[test]
    fn test_push_output_pure_ansi_reset_clears_sgr() {
        let mut state = TerminalState::new(80, 24);
        // 先设颜色，再发 reset
        state.push_output("\x1b[1;31m");
        assert_eq!(state.last_ansi_sgr, "\x1b[1;31m");
        state.push_output("\x1b[0m");
        assert!(state.last_ansi_sgr.is_empty());
        // 后面的纯文本不应被着色
        state.push_output("normal text");
        assert_eq!(state.output_lines[0], "normal text");
    }

    #[test]
    fn test_push_output_ansi_line_between_text() {
        let mut state = TerminalState::new(80, 24);
        // 模拟服务器发送：ANSI色 + 文本 + ANSI重置
        state.push_output("\x1b[1;31m看起来红衣武士想杀死你！\x1b[0m");
        assert_eq!(state.output_lines.len(), 1);
        assert!(state.last_ansi_sgr.is_empty()); // reset 已清除
    }

    #[test]
    fn test_push_output_separate_calls_inherit() {
        let mut state = TerminalState::new(80, 24);
        // 分两次调用（不同 TCP 包）
        state.push_output("\x1b[1;31m> ");
        state.push_output("面色凝重");
        assert_eq!(state.output_lines[0], "\x1b[1;31m> \x1b[0m");
        assert_eq!(state.output_lines[1], "\x1b[1;31m面色凝重\x1b[0m");
    }

    // === InputState save/restore 测试 ===

    #[test]
    fn test_input_state_default() {
        let state = InputState::default();
        assert!(state.input_buffer.is_empty());
        assert_eq!(state.input_cursor, 0);
        assert!(state.history.is_empty());
        assert_eq!(state.history_pos, 0);
        assert!(state.history_prefix.is_empty());
        assert!(!state.history_browsing);
        assert!(!state.clear_on_next_key);
        assert!(!state.text_selected);
    }

    #[test]
    fn test_save_input_state_captures_all_fields() {
        let mut ts = TerminalState::new(80, 24);
        ts.input_buffer = "kill npc".to_string();
        ts.input_cursor = 5;
        ts.history = vec!["look".to_string(), "kill npc".to_string()];
        ts.history_pos = 1;
        ts.history_prefix = "ki".to_string();
        ts.history_browsing = true;
        ts.clear_on_next_key = true;
        ts.text_selected = true;

        let saved = ts.save_input_state();
        assert_eq!(saved.input_buffer, "kill npc");
        assert_eq!(saved.input_cursor, 5);
        assert_eq!(saved.history, vec!["look", "kill npc"]);
        assert_eq!(saved.history_pos, 1);
        assert_eq!(saved.history_prefix, "ki");
        assert!(saved.history_browsing);
        assert!(saved.clear_on_next_key);
        assert!(saved.text_selected);
    }

    #[test]
    fn test_restore_input_state_restores_all_fields() {
        let mut ts = TerminalState::new(80, 24);
        // 设置一些初始状态
        ts.input_buffer = "old".to_string();
        ts.input_cursor = 3;

        let saved = InputState {
            input_buffer: "new command".to_string(),
            input_cursor: 7,
            history: vec!["cmd1".to_string(), "cmd2".to_string()],
            history_pos: 2,
            history_prefix: "cmd".to_string(),
            history_browsing: true,
            clear_on_next_key: true,
            text_selected: true,
        };
        ts.restore_input_state(&saved);

        assert_eq!(ts.input_buffer, "new command");
        assert_eq!(ts.input_cursor, 7);
        assert_eq!(ts.history, vec!["cmd1", "cmd2"]);
        assert_eq!(ts.history_pos, 2);
        assert_eq!(ts.history_prefix, "cmd");
        assert!(ts.history_browsing);
        assert!(ts.clear_on_next_key);
        assert!(ts.text_selected);
    }

    #[test]
    fn test_save_restore_roundtrip() {
        let mut ts1 = TerminalState::new(80, 24);
        ts1.input_buffer = "test cmd".to_string();
        ts1.input_cursor = 4;
        ts1.history = vec!["hist1".to_string()];
        ts1.history_pos = 1;
        ts1.history_prefix = "te".to_string();
        ts1.history_browsing = true;
        ts1.clear_on_next_key = true;
        ts1.text_selected = true;

        // 保存
        let saved = ts1.save_input_state();

        // 创建新的 TerminalState，恢复
        let mut ts2 = TerminalState::new(80, 24);
        ts2.restore_input_state(&saved);

        // 验证所有字段一致
        assert_eq!(ts2.input_buffer, ts1.input_buffer);
        assert_eq!(ts2.input_cursor, ts1.input_cursor);
        assert_eq!(ts2.history, ts1.history);
        assert_eq!(ts2.history_pos, ts1.history_pos);
        assert_eq!(ts2.history_prefix, ts1.history_prefix);
        assert_eq!(ts2.history_browsing, ts1.history_browsing);
        assert_eq!(ts2.clear_on_next_key, ts1.clear_on_next_key);
        assert_eq!(ts2.text_selected, ts1.text_selected);
    }

    #[test]
    fn test_restore_default_clears_state() {
        let mut ts = TerminalState::new(80, 24);
        // 先设置非默认状态
        ts.input_buffer = "something".to_string();
        ts.input_cursor = 9;
        ts.history = vec!["cmd".to_string()];
        ts.history_pos = 1;
        ts.history_browsing = true;
        ts.clear_on_next_key = true;
        ts.text_selected = true;

        // 恢复默认状态（新 session 的初始状态）
        ts.restore_input_state(&InputState::default());

        assert!(ts.input_buffer.is_empty());
        assert_eq!(ts.input_cursor, 0);
        assert!(ts.history.is_empty());
        assert_eq!(ts.history_pos, 0);
        assert!(!ts.history_browsing);
        assert!(!ts.clear_on_next_key);
        assert!(!ts.text_selected);
    }

    #[test]
    fn test_save_restore_does_not_affect_output() {
        let mut ts = TerminalState::new(80, 24);
        ts.push_output("output line 1");
        ts.push_output("output line 2");
        ts.scroll_offset = 1;

        let saved = ts.save_input_state();
        assert_eq!(ts.output_lines.len(), 2);
        assert_eq!(ts.scroll_offset, 1);

        // 恢复输入状态不应影响输出
        let mut ts2 = TerminalState::new(80, 24);
        ts2.push_output("different output");
        ts2.restore_input_state(&saved);
        assert_eq!(ts2.output_lines.len(), 1);
        assert_eq!(ts2.output_lines[0], "different output");
    }

    #[test]
    fn test_two_sessions_independent_input_states() {
        // 模拟两个 session 各自的输入状态
        let mut session_a = InputState::default();
        session_a.input_buffer = "kill guard".to_string();
        session_a.input_cursor = 10;
        session_a.history = vec!["look".to_string(), "kill guard".to_string()];

        let mut session_b = InputState::default();
        session_b.input_buffer = "say hello".to_string();
        session_b.input_cursor = 9;
        session_b.history = vec!["wave".to_string(), "say hello".to_string()];

        // 模拟切换：保存 A 的状态到 TerminalState，恢复 B 的状态
        let mut ts = TerminalState::new(80, 24);
        ts.restore_input_state(&session_a);
        assert_eq!(ts.input_buffer, "kill guard");
        assert_eq!(ts.history.len(), 2);

        // 切换到 B
        let saved_a = ts.save_input_state();
        ts.restore_input_state(&session_b);
        assert_eq!(ts.input_buffer, "say hello");
        assert_eq!(ts.history.len(), 2);
        assert_eq!(ts.history[0], "wave");

        // 切换回 A
        let _saved_b = ts.save_input_state();
        ts.restore_input_state(&saved_a);
        assert_eq!(ts.input_buffer, "kill guard");
        assert_eq!(ts.history[0], "look");
    }

    #[test]
    fn test_save_restore_with_empty_input() {
        let ts = TerminalState::new(80, 24);
        // 空输入状态
        let saved = ts.save_input_state();
        assert!(saved.input_buffer.is_empty());
        assert_eq!(saved.input_cursor, 0);

        // 恢复空状态不应出错
        let mut ts2 = TerminalState::new(80, 24);
        ts2.input_buffer = "to be cleared".to_string();
        ts2.input_cursor = 14;
        ts2.restore_input_state(&saved);
        assert!(ts2.input_buffer.is_empty());
        assert_eq!(ts2.input_cursor, 0);
    }

    #[test]
    fn test_state_handle_key_ctrl_u_clear_to_start() {
        let mut state = TerminalState::new(80, 24);
        state.input_buffer = "hello world".to_string();
        state.input_cursor = 5; // 光标在 'hello' 之后
        let _ = state.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(state.input_buffer, " world");
        assert_eq!(state.input_cursor, 0);
    }

    #[test]
    fn test_state_handle_key_ctrl_u_at_start() {
        let mut state = TerminalState::new(80, 24);
        state.input_buffer = "hello".to_string();
        state.input_cursor = 0;
        let _ = state.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(state.input_buffer, "hello");
        assert_eq!(state.input_cursor, 0);
    }

    #[test]
    fn test_state_handle_key_ctrl_k_clear_to_end() {
        let mut state = TerminalState::new(80, 24);
        state.input_buffer = "hello world".to_string();
        state.input_cursor = 5; // 光标在 'hello' 之后
        let _ = state.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
        assert_eq!(state.input_buffer, "hello");
        assert_eq!(state.input_cursor, 5);
    }

    #[test]
    fn test_state_handle_key_ctrl_k_at_end() {
        let mut state = TerminalState::new(80, 24);
        state.input_buffer = "hello".to_string();
        state.input_cursor = 5;
        let _ = state.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
        assert_eq!(state.input_buffer, "hello");
        assert_eq!(state.input_cursor, 5);
    }

    #[test]
    fn test_state_handle_key_ctrl_w_delete_word() {
        let mut state = TerminalState::new(80, 24);
        state.input_buffer = "hello world".to_string();
        state.input_cursor = 11;
        let _ = state.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(state.input_buffer, "hello");
        assert_eq!(state.input_cursor, 5);
    }

    #[test]
    fn test_state_handle_key_ctrl_w_with_spaces() {
        let mut state = TerminalState::new(80, 24);
        state.input_buffer = "hello   world".to_string();
        state.input_cursor = 13;
        let _ = state.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        // 先跳过空格再跳过 'hello'，所以应删除 '   world'
        assert_eq!(state.input_buffer, "hello");
        assert_eq!(state.input_cursor, 5);
    }

    #[test]
    fn test_state_handle_key_ctrl_w_at_start() {
        let mut state = TerminalState::new(80, 24);
        state.input_buffer = "hello".to_string();
        state.input_cursor = 0;
        let _ = state.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(state.input_buffer, "hello");
        assert_eq!(state.input_cursor, 0);
    }

    #[test]
    fn test_state_handle_key_ctrl_a_home() {
        let mut state = TerminalState::new(80, 24);
        state.input_buffer = "hello".to_string();
        state.input_cursor = 3;
        let _ = state.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        assert_eq!(state.input_cursor, 0);
    }

    #[test]
    fn test_state_handle_key_ctrl_e_end() {
        let mut state = TerminalState::new(80, 24);
        state.input_buffer = "hello".to_string();
        state.input_cursor = 0;
        let _ = state.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
        assert_eq!(state.input_cursor, 5);
    }

    #[test]
    fn test_state_handle_key_raw_backspace_fallback() {
        let mut state = TerminalState::new(80, 24);
        state.input_buffer = "ab".to_string();
        state.input_cursor = 2;
        // \x08 (BS) 应等同于 Backspace
        let _ = state.handle_key(KeyEvent::new(KeyCode::Char('\x08'), KeyModifiers::NONE));
        assert_eq!(state.input_buffer, "a");
        assert_eq!(state.input_cursor, 1);
    }

    #[test]
    fn test_state_handle_key_raw_del_fallback() {
        let mut state = TerminalState::new(80, 24);
        state.input_buffer = "ab".to_string();
        state.input_cursor = 0;
        // \x7f (DEL) 应等同于 Backspace（删除光标前字符，但光标在行首所以无操作）
        let _ = state.handle_key(KeyEvent::new(KeyCode::Char('\x7f'), KeyModifiers::NONE));
        // 光标在行首，backspace 无操作
        assert_eq!(state.input_buffer, "ab");
        assert_eq!(state.input_cursor, 0);
    }

    #[test]
    fn test_state_handle_key_raw_del_fallback_with_content() {
        let mut state = TerminalState::new(80, 24);
        state.input_buffer = "ab".to_string();
        state.input_cursor = 2;
        // \x7f (DEL) 在光标末尾应等同于 Backspace
        let _ = state.handle_key(KeyEvent::new(KeyCode::Char('\x7f'), KeyModifiers::NONE));
        assert_eq!(state.input_buffer, "a");
        assert_eq!(state.input_cursor, 1);
    }

    // === Panel 测试 ===

    #[test]
    fn test_set_panel_inserts_new() {
        let mut state = TerminalState::new(80, 24);
        state.set_panel(
            "stat",
            -70,
            1,
            70,
            10,
            vec!["line1".to_string(), "line2".to_string()],
            vec![],
        );
        assert_eq!(state.panels.len(), 1);
        assert_eq!(state.panels[0].name, "stat");
        assert_eq!(state.panels[0].x, -70);
        assert_eq!(state.panels[0].lines.len(), 2);
    }

    #[test]
    fn test_set_panel_updates_existing() {
        let mut state = TerminalState::new(80, 24);
        state.set_panel("stat", -70, 1, 70, 10, vec!["old".to_string()], vec![]);
        state.set_panel("stat", -50, 2, 50, 5, vec!["new".to_string()], vec![]);
        assert_eq!(state.panels.len(), 1);
        assert_eq!(state.panels[0].x, -50);
        assert_eq!(state.panels[0].lines[0], "new");
    }

    #[test]
    fn test_remove_panel() {
        let mut state = TerminalState::new(80, 24);
        state.set_panel("stat", -70, 1, 70, 10, vec!["line".to_string()], vec![]);
        state.set_panel("debug", 0, 1, 40, 5, vec!["dbg".to_string()], vec![]);
        assert_eq!(state.panels.len(), 2);
        state.remove_panel("stat");
        assert_eq!(state.panels.len(), 1);
        assert_eq!(state.panels[0].name, "debug");
    }

    #[test]
    fn test_remove_panel_nonexistent() {
        let mut state = TerminalState::new(80, 24);
        state.remove_panel("nonexistent");
        assert!(state.panels.is_empty());
    }

    #[test]
    fn test_resolve_panel_position_negative_x() {
        let state = TerminalState::new(80, 24);
        let panel = Panel {
            name: "test".to_string(),
            x: -70,
            y: 1,
            width: 70,
            height: 10,
            lines: vec![],
            buttons: vec![],
        };
        let (abs_x, _) = state.resolve_panel_position(&panel);
        // 80 + (-70) = 10
        assert_eq!(abs_x, 10);
    }

    #[test]
    fn test_resolve_panel_position_positive_x() {
        let state = TerminalState::new(80, 24);
        let panel = Panel {
            name: "test".to_string(),
            x: 5,
            y: 2,
            width: 30,
            height: 5,
            lines: vec![],
            buttons: vec![],
        };
        let (abs_x, abs_y) = state.resolve_panel_position(&panel);
        assert_eq!(abs_x, 5);
        // 输出区从第 1 行起（顶行为 session 状态栏），panel.y 相对输出区顶部：y=2 → abs_y=3
        assert_eq!(abs_y, 3);
    }

    #[test]
    fn test_resolve_panel_position_negative_y() {
        let state = TerminalState::new(80, 24);
        // output_bottom = input_row = 24 - 1(lua_status) - 1(input) = 22
        let panel = Panel {
            name: "test".to_string(),
            x: 0,
            y: -5,
            width: 30,
            height: 5,
            lines: vec![],
            buttons: vec![],
        };
        let (_, abs_y) = state.resolve_panel_position(&panel);
        // 22 + (-5) = 17
        assert_eq!(abs_y, 17);
    }

    // ---- wrap_line_to_width 测试 ----

    #[test]
    fn test_wrap_short_line() {
        let segs = wrap_line_to_width("hello", 80);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0], "hello");
    }

    #[test]
    fn test_wrap_long_line() {
        let line = "abcdefghij"; // 10 chars
        let segs = wrap_line_to_width(line, 4);
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0], "abcd");
        assert_eq!(segs[1], "efgh");
        assert_eq!(segs[2], "ij");
    }

    #[test]
    fn test_wrap_ansi_zero_width() {
        // ANSI 序列不计入宽度
        let line = "\x1b[31mred\x1b[0m text";
        let segs = wrap_line_to_width(line, 80);
        assert_eq!(segs.len(), 1);
        // 宽度 = "red text" = 8
    }

    #[test]
    fn test_wrap_ansi_state_preserved() {
        // 长红色文本跨段应保持颜色
        let line = format!("\x1b[31m{}\x1b[0m", "a".repeat(10));
        let segs = wrap_line_to_width(&line, 4);
        assert_eq!(segs.len(), 3);
        // 第一段：红色 + 4字符 + reset
        assert!(segs[0].starts_with("\x1b[31m"));
        assert!(segs[0].ends_with("\x1b[0m"));
        // 第二段：应以红色开头（恢复颜色）
        assert!(segs[1].starts_with("\x1b[31m"));
        assert!(segs[1].ends_with("\x1b[0m"));
        // 第三段：同样
        assert!(segs[2].starts_with("\x1b[31m"));
        assert!(segs[2].ends_with("\x1b[0m"));
    }

    #[test]
    fn test_wrap_reset_variant_0_0m() {
        // 重置变体 \x1b[0;0m 应被识别为重置：重置后的文本折行时续段不应重新上色
        // 修复前：\x1b[0;0m 被追加进 sgr_state，续段被补上 "\x1b[31m\x1b[0;0m" 脏状态
        let line = format!("\x1b[31mAB\x1b[0;0m{}", "c".repeat(6));
        let segs = wrap_line_to_width(&line, 4);
        // AB(2) + cc(2) 填满第一段，剩余 4 个 c 为第二段（处于重置后区域）
        assert_eq!(segs.len(), 2);
        assert!(segs[0].starts_with("\x1b[31m"));
        // 第二段不应携带任何颜色恢复前缀
        assert_eq!(segs[1], "cccc");
    }

    #[test]
    fn test_wrap_cjk_chars() {
        // 中文每个占 2 cell，宽度 4 可放 2 个汉字
        let line = "你好世界测试"; // 6 汉字 = 12 cell
        let segs = wrap_line_to_width(line, 4);
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0], "你好");
        assert_eq!(segs[1], "世界");
        assert_eq!(segs[2], "测试");
    }

    #[test]
    fn test_wrap_exact_boundary() {
        // 恰好等于宽度，应为 1 段
        let segs = wrap_line_to_width("abcd", 4);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0], "abcd");
    }

    #[test]
    fn test_wrap_empty_line() {
        let segs = wrap_line_to_width("", 80);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0], "");
    }

    #[test]
    fn test_wrap_width_zero() {
        let line = "hello world";
        let segs = wrap_line_to_width(line, 0);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0], line);
    }

    #[test]
    fn test_wrap_non_sgr_csi() {
        // 非 SGR CSI 序列（如清屏 \x1b[2J）不应过度消费后续字符
        let line = "\x1b[2JHello"; // \x1b[2J 是清屏，Hello 是普通文本
        let segs = wrap_line_to_width(line, 3);
        // \x1b[2J 不计入宽度，Hello = 5 字符，宽度 3 应拆为 2 段
        assert_eq!(segs.len(), 2);
        // 第一段应包含 CSI 序列 + Hel
        assert!(segs[0].contains("\x1b[2J"));
        assert!(segs[0].contains("Hel"));
        // 第二段应包含 lo
        assert!(segs[1].contains("lo"));
    }

    #[test]
    fn test_wrap_mixed_csi_and_sgr() {
        // 非 SGR CSI + SGR 混合
        let line = "\x1b[?25h\x1b[31mRedText\x1b[0m";
        let segs = wrap_line_to_width(line, 3);
        // \x1b[?25h（显光标）和 \x1b[31m（红色）不计入宽度
        // RedText = 7 字符，宽度 3 应拆为 3 段
        assert_eq!(segs.len(), 3);
        // 第一段应包含两个 CSI 序列 + Red
        assert!(segs[0].contains("\x1b[?25h"));
        assert!(segs[0].contains("\x1b[31m"));
        assert!(segs[0].contains("Red"));
        // 后续段应保持红色
        assert!(segs[1].starts_with("\x1b[31m"));
    }

    // === truncate_ansi_to_width 测试 ===

    #[test]
    fn test_truncate_ansi_plain() {
        assert_eq!(truncate_ansi_to_width("hello", 3), "hel");
        assert_eq!(truncate_ansi_to_width("hi", 5), "hi");
        assert_eq!(truncate_ansi_to_width("", 5), "");
    }

    #[test]
    fn test_truncate_ansi_zero_width() {
        assert_eq!(truncate_ansi_to_width("hello", 0), "");
    }

    #[test]
    fn test_truncate_ansi_cjk() {
        // CJK 字符宽度为 2
        assert_eq!(truncate_ansi_to_width("你好", 3), "你");
        assert_eq!(truncate_ansi_to_width("你好", 2), "你");
        assert_eq!(truncate_ansi_to_width("你好", 1), "");
    }

    #[test]
    fn test_truncate_ansi_preserves_sgr() {
        // ANSI SGR 序列不计入宽度，应完整保留
        let result = truncate_ansi_to_width("\x1b[31mhello\x1b[0m", 3);
        assert_eq!(result, "\x1b[31mhel");
        // 完整宽度时保留全部内容和 reset
        let full = truncate_ansi_to_width("\x1b[31mhi\x1b[0m", 2);
        assert_eq!(full, "\x1b[31mhi\x1b[0m");
    }

    #[test]
    fn test_truncate_ansi_256_color() {
        // 256 色序列不计入宽度
        let result = truncate_ansi_to_width("\x1b[38;5;196mred\x1b[0m", 3);
        assert_eq!(result, "\x1b[38;5;196mred\x1b[0m");
    }

    #[test]
    fn test_truncate_ansi_non_sgr_csi() {
        // 非 SGR 的 CSI 序列（如 \x1b[?25h）也不计入宽度
        let result = truncate_ansi_to_width("\x1b[?25habc", 2);
        assert_eq!(result, "\x1b[?25hab");
    }

    #[test]
    fn test_truncate_ansi_mixed_cjk_and_color() {
        // ANSI 颜色 + CJK 混合
        let result = truncate_ansi_to_width("\x1b[32m你好世界\x1b[0m", 5);
        assert_eq!(result, "\x1b[32m你好");
    }

    // === panel_coverage_mask 测试 ===

    #[test]
    fn test_panel_mask_no_panels() {
        let state = TerminalState::new(80, 24);
        let mask = state.panel_coverage_mask();
        // output_height = 24 - 1 - 1 - 1 = 21
        assert_eq!(mask.len(), 21);
        assert!(mask.iter().all(|m| m.is_none()));
    }

    #[test]
    fn test_panel_mask_single_right_aligned() {
        let mut state = TerminalState::new(80, 24);
        // 右上角面板：x=-20 → abs_x=60, y=0 → abs_y=1, width=20, height=5
        state.set_panel("stat", -20, 0, 20, 5, vec!["line".to_string()], vec![]);
        let mask = state.panel_coverage_mask();
        // output_height = 21, 面板覆盖 output 行 0..4 (abs_y=1, output_top=1, idx=0..4)
        assert_eq!(mask.len(), 21);
        for (i, m) in mask.iter().enumerate().take(5) {
            assert_eq!(*m, Some((60, 80)), "row {} should be covered", i);
        }
        for (i, m) in mask.iter().enumerate().skip(5) {
            assert_eq!(*m, None, "row {} should not be covered", i);
        }
    }

    #[test]
    fn test_panel_mask_multiple_overlapping() {
        let mut state = TerminalState::new(80, 24);
        // 面板A: abs_x=50, width=20 → [50,70), 面板B: abs_x=55, width=15 → [55,70)
        state.set_panel("a", -30, 0, 20, 3, vec![], vec![]);
        state.set_panel("b", -25, 0, 15, 3, vec![], vec![]);
        let mask = state.panel_coverage_mask();
        // 两面板重叠行 0..2：min_x=50, max_end=70
        for (i, m) in mask.iter().enumerate().take(3) {
            assert_eq!(*m, Some((50, 70)), "row {}", i);
        }
    }

    #[test]
    fn test_panel_mask_clipped_to_output_area() {
        let mut state = TerminalState::new(80, 24);
        // 面板超出输出区底部：height=50, 但 output_height=21
        state.set_panel("big", -20, 0, 20, 50, vec![], vec![]);
        let mask = state.panel_coverage_mask();
        // 应被裁剪到 output_height
        assert_eq!(mask.len(), 21);
        for (i, m) in mask.iter().enumerate() {
            assert_eq!(*m, Some((60, 80)), "row {}", i);
        }
    }

    #[test]
    fn test_panel_mask_negative_y() {
        let mut state = TerminalState::new(80, 24);
        // output_bottom = 24 - 1(lua) - 1(input) = 22；session 状态栏占顶行，output_top = 1
        // y=-5 → abs_y = 22 - 5 = 17, output idx = 17 - 1(output_top) = 16
        state.set_panel("stat", -20, -5, 20, 3, vec![], vec![]);
        let mask = state.panel_coverage_mask();
        for (i, m) in mask.iter().enumerate().take(16) {
            assert_eq!(*m, None, "row {} should not be covered", i);
        }
        for (i, m) in mask.iter().enumerate().skip(16).take(3) {
            assert_eq!(*m, Some((60, 80)), "row {} should be covered", i);
        }
    }
}
