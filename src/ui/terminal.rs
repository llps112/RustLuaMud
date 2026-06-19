use crossterm::{
    cursor,
    event::{KeyCode, KeyEvent, KeyModifiers},
    execute, queue,
    style::{self, Color, Print, SetForegroundColor},
    terminal::{self, ClearType},
};
use std::io::{self, Write};

use crate::connection::{SessionInfo, SessionState};
use crate::ui::{ensure_ansi_reset, AnsiParser};

/// 可点击区域（状态栏上的 session 标签）
#[derive(Debug, Clone)]
pub struct ClickRegion {
    pub start_x: u16,
    pub end_x: u16,
    pub session_id: usize,
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
/// 终端驱动会按当前光标列位置执行 TAB 跳格，与 MushClient 行为一致
fn expand_tabs(s: &str) -> String {
    s.to_string()
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
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + cw > max_width {
            break;
        }
        result.push(ch);
        width += cw;
    }
    result
}

/// 计算字符串的可见宽度（忽略 ANSI 转义序列）
fn visible_width(s: &str) -> usize {
    let stripped = AnsiParser::strip_ansi(s);
    stripped
        .chars()
        .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0))
        .sum()
}

/// 构建 session 状态栏字符串（纯逻辑，无 IO 依赖）
/// 返回 (状态栏字符串, 可点击区域列表)
fn build_status_bar(
    sessions: &[SessionInfo],
    foreground_id: usize,
    total_width: usize,
) -> (String, Vec<ClickRegion>) {
    let mut bar = String::new();
    let mut regions = Vec::new();
    for (i, info) in sessions.iter().enumerate() {
        let state_icon = match info.state {
            SessionState::Connected => "\x1b[32m●\x1b[0m",
            SessionState::Disconnected => "\x1b[90m○\x1b[0m",
            SessionState::Connecting => "\x1b[33m◎\x1b[0m",
            SessionState::Reconnecting => "\x1b[35m⟳\x1b[0m",
        };
        // 记录当前 x 位置（不包括 ANSI 码的可见宽度）
        let start_x = visible_width(&bar) as u16;
        if i == foreground_id {
            bar.push_str(&format!(
                "\x1b[33m[{}]{}\x1b[0m{} ",
                i + 1,
                info.name,
                state_icon
            ));
        } else {
            bar.push_str(&format!("[{}]{}\x1b[0m{} ", i + 1, info.name, state_icon));
        }
        let end_x = visible_width(&bar) as u16;
        regions.push(ClickRegion {
            start_x,
            end_x,
            session_id: i,
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
    foreground_id: usize,
    total_width: usize,
) -> String {
    if let Some(fg) = sessions.get(foreground_id) {
        if !fg.status_text.is_empty() {
            let truncated: String = fg.status_text.chars().take(total_width).collect();
            return truncated;
        }
    }
    String::new()
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
    /// 最近一次看到的 ANSI SGR 颜色序列，用于跨行颜色继承
    pub last_ansi_sgr: String,
    /// 输出区滚动偏移（0 = 底部，即最新输出）
    pub scroll_offset: usize,
    /// 状态栏可点击区域
    pub status_bar_regions: Vec<ClickRegion>,
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
            last_ansi_sgr: String::new(),
            scroll_offset: 0,
            status_bar_regions: Vec::new(),
        }
    }

    /// 获取输出区可用行数
    pub fn output_height(&self) -> u16 {
        self.height
            .saturating_sub(self.status_height + self.lua_status_height + self.input_height)
    }

    /// 追加输出行到缓冲区（纯逻辑，不涉及 IO）
    /// 追踪最近一次看到的 ANSI SGR 颜色序列，对有文本但无自身 ANSI 的行
    /// 自动补上颜色前缀，实现行间颜色继承（如服务器在 ">" 行设置红色，
    /// 下一行"面色凝重"无 ANSI，自动继承红色）
    pub fn push_output(&mut self, line: &str) {
        let old_len = self.output_lines.len();

        for part in line.split_inclusive('\n') {
            let trimmed = part.trim_end_matches('\n').trim_end_matches('\r');
            if !trimmed.is_empty() {
                let stripped = AnsiParser::strip_ansi(trimmed);
                // 提取本行的 SGR 序列和 reset 标记
                let last_sgr = extract_last_sgr(trimmed);
                let has_reset = trimmed.contains("\x1b[0m");

                if stripped.is_empty() {
                    // 纯 ANSI 行（不可见）：只更新状态，不加入输出
                    if has_reset {
                        self.last_ansi_sgr.clear();
                    } else if let Some(sgr) = last_sgr {
                        self.last_ansi_sgr = sgr;
                    }
                } else if last_sgr.is_some() {
                    // 有可见文本且自身带 ANSI：保存颜色，加入输出（附 reset）
                    if !has_reset {
                        if let Some(sgr) = &last_sgr {
                            self.last_ansi_sgr = sgr.clone();
                        }
                    } else {
                        self.last_ansi_sgr.clear();
                    }
                    self.output_lines.push(ensure_ansi_reset(trimmed));
                } else if !self.last_ansi_sgr.is_empty() {
                    // 可见文本，无自身 ANSI，但有继承的颜色：补上颜色
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
        let max_lines = 5000;
        let drained = if self.output_lines.len() > max_lines {
            let drain_count = self.output_lines.len() - max_lines;
            self.output_lines.drain(..drain_count);
            drain_count
        } else {
            0
        };

        // 历史浏览模式（scroll_offset > 0）：调整偏移量保持视口稳定
        // 新行追加到底部 → scroll_offset 增加；旧行从顶部移除 → scroll_offset 减少
        if self.scroll_offset > 0 && (new_lines > 0 || drained > 0) {
            self.scroll_offset = self
                .scroll_offset
                .saturating_add(new_lines)
                .saturating_sub(drained);
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
                    // 保留文本，光标回到行首，下次按键替换旧内容
                    self.input_cursor = 0;
                    self.clear_on_next_key = true;
                } else {
                    self.input_buffer.clear();
                    self.input_cursor = 0;
                    self.history_prefix.clear();
                    self.history_browsing = false;
                }
                Some(cmd)
            }

            (KeyModifiers::NONE, KeyCode::Backspace) => {
                self.clear_on_next_key = false;
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
                if self.input_cursor > 0 {
                    self.input_cursor -= 1;
                }
                None
            }

            (KeyModifiers::NONE, KeyCode::Right) => {
                self.clear_on_next_key = false;
                if self.input_cursor < self.input_buffer.chars().count() {
                    self.input_cursor += 1;
                }
                None
            }

            (KeyModifiers::NONE, KeyCode::Up) => {
                self.clear_on_next_key = false;
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
                self.input_cursor = 0;
                None
            }

            (KeyModifiers::NONE, KeyCode::End) => {
                self.clear_on_next_key = false;
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
                // 向下滚动半屏
                let scroll_amount = (self.output_height() / 2) as usize;
                self.scroll_offset = self.scroll_offset.saturating_sub(scroll_amount);
                None
            }

            (KeyModifiers::SHIFT, KeyCode::Char(c)) | (KeyModifiers::NONE, KeyCode::Char(c)) => {
                // 全选替换：若 clear_on_next_key 为真，先清空输入
                if self.clear_on_next_key {
                    self.input_buffer.clear();
                    self.input_cursor = 0;
                    self.clear_on_next_key = false;
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
    pub fn update_status_bar(&mut self, sessions: &[SessionInfo], foreground_id: usize) {
        let (bar, regions) = build_status_bar(sessions, foreground_id, self.width as usize);
        self.status_bar_cache = Some(bar);
        self.status_bar_regions = regions;
    }

    /// 更新 Lua 状态栏缓存（纯逻辑）
    pub fn update_lua_status_bar(&mut self, sessions: &[SessionInfo], foreground_id: usize) {
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

        let start = if end > output_height {
            end - output_height
        } else {
            0
        };

        &self.output_lines[start..end]
    }

    /// 获取输入行显示内容（考虑滚动）
    pub fn input_display(&self) -> (String, usize) {
        use unicode_width::UnicodeWidthChar;

        let prompt_len: usize = 2; // "> "
        let avail_width = self.width as usize - prompt_len;
        let chars: Vec<char> = self.input_buffer.chars().collect();
        let total_chars = chars.len();

        // 计算每个字符的显示宽度
        let char_widths: Vec<usize> = chars.iter().map(|c| c.width().unwrap_or(0)).collect();

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
        for i in display_start..total_chars {
            if display_col + char_widths[i] > avail_width {
                display_end = i;
                break;
            }
            display_col += char_widths[i];
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
}

impl Terminal {
    pub fn new() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let (width, height) = terminal::size()?;
        // 启用鼠标点击追踪（仅 ?1000h，不含 ?1002h 拖拽追踪）
        // 终端处于鼠标应用模式时，按住 Shift 拖拽可绕过应用模式进行原生文本选中
        write!(io::stdout(), "\x1b[?1000h\x1b[?1006h")?;
        io::stdout().flush()?;
        Ok(Self {
            state: TerminalState::new(width, height),
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
        self.refresh_all(&mut stdout)?;
        Ok(())
    }

    /// 完整刷新屏幕（包括状态栏 + 输出区 + 输入行）
    fn refresh_all(&self, stdout: &mut io::Stdout) -> io::Result<()> {
        // session 状态栏（顶部）
        if let Some(ref bar) = self.state.status_bar_cache {
            queue!(stdout, cursor::MoveTo(0, 0))?;
            queue!(stdout, terminal::Clear(ClearType::CurrentLine))?;
            queue!(stdout, Print(bar))?;
        }

        self.draw_output_area(stdout)?;

        // Lua 状态栏（输出区下方、输入行上方）
        let lua_bar_y = self
            .state
            .height
            .saturating_sub(self.state.input_height + self.state.lua_status_height);
        if let Some(ref text) = self.state.lua_status_cache {
            queue!(stdout, cursor::MoveTo(0, lua_bar_y))?;
            queue!(stdout, terminal::Clear(ClearType::CurrentLine))?;
            queue!(stdout, Print(text))?;
        } else {
            queue!(stdout, cursor::MoveTo(0, lua_bar_y))?;
            queue!(stdout, terminal::Clear(ClearType::CurrentLine))?;
        }

        self.draw_input_line(stdout)?;
        stdout.flush()?;
        Ok(())
    }

    /// 仅刷新输出区和输入行（不重绘状态栏，避免闪烁）
    fn refresh_output_area(&self, stdout: &mut io::Stdout) -> io::Result<()> {
        self.draw_output_area(stdout)?;
        self.draw_input_line(stdout)?;
        stdout.flush()?;
        Ok(())
    }

    /// 绘制输出区所有可见行
    fn draw_output_area(&self, stdout: &mut io::Stdout) -> io::Result<()> {
        let output_height = self.state.output_height() as usize;
        let visible = self.state.visible_output_lines();
        for (i, line) in visible.iter().enumerate() {
            let row = self.state.status_height + i as u16;
            queue!(stdout, cursor::MoveTo(0, row))?;
            queue!(stdout, terminal::Clear(ClearType::CurrentLine))?;
            let expanded = expand_tabs(line);
            queue!(stdout, Print(&expanded))?;
        }
        for i in visible.len()..output_height {
            let row = self.state.status_height + i as u16;
            queue!(stdout, cursor::MoveTo(0, row))?;
            queue!(stdout, terminal::Clear(ClearType::CurrentLine))?;
        }
        Ok(())
    }

    /// 绘制 session 状态栏（顶部）
    pub fn draw_status_bar(
        &mut self,
        stdout: &mut io::Stdout,
        sessions: &[SessionInfo],
        foreground_id: usize,
    ) -> io::Result<()> {
        self.state.update_status_bar(sessions, foreground_id);
        if let Some(ref bar) = self.state.status_bar_cache {
            queue!(stdout, cursor::MoveTo(0, 0))?;
            queue!(stdout, terminal::Clear(ClearType::CurrentLine))?;
            queue!(stdout, Print(bar))?;
        }
        Ok(())
    }

    /// 绘制 Lua 状态栏（输出区下方、输入行上方）
    pub fn draw_lua_status_bar(
        &mut self,
        stdout: &mut io::Stdout,
        sessions: &[SessionInfo],
        foreground_id: usize,
    ) -> io::Result<()> {
        self.state.update_lua_status_bar(sessions, foreground_id);
        let lua_bar_y = self
            .state
            .height
            .saturating_sub(self.state.input_height + self.state.lua_status_height);
        if let Some(ref text) = self.state.lua_status_cache {
            queue!(stdout, cursor::MoveTo(0, lua_bar_y))?;
            queue!(stdout, terminal::Clear(ClearType::CurrentLine))?;
            queue!(stdout, style::SetForegroundColor(style::Color::DarkGreen))?;
            queue!(stdout, Print(text))?;
            queue!(stdout, style::ResetColor)?;
        } else {
            queue!(stdout, cursor::MoveTo(0, lua_bar_y))?;
            queue!(stdout, terminal::Clear(ClearType::CurrentLine))?;
        }
        Ok(())
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
        let input_y = self.state.height.saturating_sub(1);
        queue!(stdout, cursor::MoveTo(0, input_y))?;
        queue!(stdout, terminal::Clear(ClearType::CurrentLine))?;
        queue!(stdout, SetForegroundColor(Color::Green), Print("> "))?;
        queue!(stdout, style::ResetColor)?;

        let (display_str, cursor_x) = self.state.input_display();
        queue!(stdout, Print(&display_str))?;
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
        self.state.width = width;
        self.state.height = height;
        let _ = self.refresh_all(&mut io::stdout());
    }

    /// 替换整个输出缓冲区（切换前台连接时使用）
    pub fn replace_output(&mut self, lines: &[String]) -> io::Result<()> {
        self.state.output_lines = lines.to_vec();
        self.state.last_ansi_sgr.clear(); // 切换连接时清除累积的颜色前缀
        let mut stdout = io::stdout();
        self.refresh_all(&mut stdout)?;
        Ok(())
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = write!(io::stdout(), "\x1b[?1000l\x1b[?1006l");
        let _ = io::stdout().flush();
        let _ = execute!(io::stdout(), terminal::LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

/// 获取状态栏可点击区域
impl Terminal {
    pub fn click_regions(&self) -> &[ClickRegion] {
        &self.state.status_bar_regions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_tabs_passthrough() {
        assert_eq!(expand_tabs("hello\tworld"), "hello\tworld");
        assert_eq!(expand_tabs("no_tabs"), "no_tabs");
        assert_eq!(expand_tabs(""), "");
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
    fn test_state_output_height() {
        let state = TerminalState::new(80, 24);
        assert_eq!(state.output_height(), 21); // 24 - 1 (status) - 1 (lua_status) - 1 (input)
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
        let visible_before: Vec<String> = state.visible_output_lines().iter().cloned().collect();

        // 添加新输出
        state.push_output("new line");

        // scroll_offset 应增加，保持视口内容不变
        assert_eq!(state.scroll_offset, offset_before + 1);

        // 视口内容应保持相同
        let visible_after: Vec<String> = state.visible_output_lines().iter().cloned().collect();
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
        // 光标回到行首
        assert_eq!(state.input_cursor, 0);
        assert!(state.clear_on_next_key);
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
        let _ = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(state.clear_on_next_key);
        // 按方向键取消全选状态
        let _ = state.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert!(!state.clear_on_next_key);
        // clear_on_next_key 已取消，正常插入（光标此时在位置 1，即 "e" 之前）
        let _ = state.handle_key(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE));
        assert_eq!(state.input_buffer, "hXello");
        // End 再到末尾
        let _ = state.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        // 清除 clear_on_next_key
        let _ = state.handle_key(KeyEvent::new(KeyCode::Char('!'), KeyModifiers::NONE));
        assert_eq!(state.input_buffer, "hXello!");
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
        let (bar, regions) = build_status_bar(&[], 0, 80);
        assert!(bar.contains("RustLuaMud"));
        assert!(regions.is_empty());
    }

    #[test]
    fn test_build_status_bar_with_sessions() {
        let sessions = vec![
            SessionInfo {
                name: "mud1".to_string(),
                state: SessionState::Connected,
                status_text: String::new(),
            },
            SessionInfo {
                name: "mud2".to_string(),
                state: SessionState::Disconnected,
                status_text: String::new(),
            },
        ];
        let (bar, regions) = build_status_bar(&sessions, 0, 80);
        assert!(bar.contains("mud1"));
        assert!(bar.contains("mud2"));
        assert!(bar.contains("RustLuaMud"));
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0].session_id, 0);
        assert_eq!(regions[1].session_id, 1);
        assert!(regions[1].start_x >= regions[0].end_x);
    }

    #[test]
    fn test_build_status_bar_foreground_highlight() {
        let sessions = vec![SessionInfo {
            name: "mud1".to_string(),
            state: SessionState::Connected,
            status_text: String::new(),
        }];
        let (bar, _regions) = build_status_bar(&sessions, 0, 80);
        // Foreground should have yellow highlight
        assert!(bar.contains("\x1b[33m[1]"));
    }

    #[test]
    fn test_state_update_status_bar() {
        let mut state = TerminalState::new(80, 24);
        let sessions = vec![SessionInfo {
            name: "test".to_string(),
            state: SessionState::Connected,
            status_text: String::new(),
        }];
        state.update_status_bar(&sessions, 0);
        assert!(state.status_bar_cache.is_some());
        let bar = state.status_bar_cache.as_ref().unwrap();
        assert!(bar.contains("test"));
        // 验证可点击区域
        assert_eq!(state.status_bar_regions.len(), 1);
        assert_eq!(state.status_bar_regions[0].session_id, 0);
        assert!(state.status_bar_regions[0].end_x > state.status_bar_regions[0].start_x);
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
        // Home 取消标志并回到开头
        let _ = state.handle_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
        assert!(!state.clear_on_next_key);
        assert_eq!(state.input_cursor, 0);
        // End 到末尾
        let _ = state.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        assert_eq!(state.input_cursor, 5);
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
}
