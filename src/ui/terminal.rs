use crossterm::{
    cursor,
    event::{KeyCode, KeyEvent, KeyModifiers},
    execute, queue,
    style::{self, Color, Print, SetForegroundColor},
    terminal::{self, ClearType},
};
use std::io::{self, Write};

use crate::connection::{SessionInfo, SessionState};
use crate::ui::ensure_ansi_reset;

/// 透传原始字符串，让终端原生处理制表符（\t）
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

/// 构建 session 状态栏字符串（纯逻辑，无 IO 依赖）
fn build_status_bar(sessions: &[SessionInfo], foreground_id: usize, total_width: usize) -> String {
    let mut bar = String::new();
    for (i, info) in sessions.iter().enumerate() {
        let state_icon = match info.state {
            SessionState::Connected => "\x1b[32m●\x1b[0m",
            SessionState::Disconnected => "\x1b[90m○\x1b[0m",
            SessionState::Connecting => "\x1b[33m◎\x1b[0m",
            SessionState::Reconnecting => "\x1b[35m⟳\x1b[0m",
        };
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
    }
    let right_text = "RustLuaMud";
    if bar.len() + right_text.len() + 2 < total_width {
        let padding = total_width - bar.len() - right_text.len() - 2;
        bar.extend(std::iter::repeat_n(' ', padding));
        bar.push_str(&format!("\x1b[36m{}\x1b[0m", right_text));
    }
    bar
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
            width,
            height,
            status_height: 1,
            lua_status_height: 1,
            input_height: 1,
            status_bar_cache: None,
            lua_status_cache: None,
            keep_command: true,
            clear_on_next_key: false,
        }
    }

    /// 获取输出区可用行数
    pub fn output_height(&self) -> u16 {
        self.height
            .saturating_sub(self.status_height + self.lua_status_height + self.input_height)
    }

    /// 追加输出行到缓冲区（纯逻辑，不涉及 IO）
    /// 自动确保每行行尾 ANSI 状态为 reset，防止颜色泄漏到后续行
    pub fn push_output(&mut self, line: &str) {
        for part in line.split_inclusive('\n') {
            let trimmed = part.trim_end_matches('\n').trim_end_matches('\r');
            if !trimmed.is_empty() {
                self.output_lines.push(ensure_ansi_reset(trimmed));
            }
        }
        // 限制缓冲区大小
        let max_lines = 5000;
        if self.output_lines.len() > max_lines {
            let drain_count = self.output_lines.len() - max_lines;
            self.output_lines.drain(..drain_count);
        }
    }

    /// 处理键盘事件，返回是否需要发送命令（纯逻辑）
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c'))
            | (KeyModifiers::CONTROL, KeyCode::Char('d')) => None,

            (KeyModifiers::NONE, KeyCode::Enter) => {
                let cmd = self.input_buffer.clone();
                if !cmd.is_empty() {
                    self.history.push(cmd.clone());
                    self.history_pos = self.history.len();
                }
                if self.keep_command {
                    // 保留文本，光标回到行首，下次按键替换旧内容
                    self.input_cursor = 0;
                    self.clear_on_next_key = true;
                } else {
                    self.input_buffer.clear();
                    self.input_cursor = 0;
                }
                Some(cmd)
            }

            (KeyModifiers::NONE, KeyCode::Backspace) => {
                self.clear_on_next_key = false;
                if self.input_cursor > 0 {
                    self.input_cursor -= 1;
                    let byte_pos = char_pos_to_byte_pos(&self.input_buffer, self.input_cursor);
                    self.input_buffer.remove(byte_pos);
                }
                None
            }

            (KeyModifiers::NONE, KeyCode::Delete) => {
                self.clear_on_next_key = false;
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
                if self.history_pos > 0 {
                    self.history_pos -= 1;
                    self.input_buffer = self.history[self.history_pos].clone();
                    self.input_cursor = self.input_buffer.chars().count();
                }
                None
            }

            (KeyModifiers::NONE, KeyCode::Down) => {
                self.clear_on_next_key = false;
                if self.history_pos < self.history.len() {
                    self.history_pos += 1;
                    if self.history_pos < self.history.len() {
                        self.input_buffer = self.history[self.history_pos].clone();
                    } else {
                        self.input_buffer.clear();
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
                self.input_cursor = self.input_buffer.chars().count();
                None
            }

            (KeyModifiers::NONE, KeyCode::Char(c)) => {
                // 全选替换：若 clear_on_next_key 为真，先清空输入
                if self.clear_on_next_key {
                    self.input_buffer.clear();
                    self.input_cursor = 0;
                    self.clear_on_next_key = false;
                }
                let byte_pos = char_pos_to_byte_pos(&self.input_buffer, self.input_cursor);
                self.input_buffer.insert(byte_pos, c);
                self.input_cursor += 1;
                None
            }

            _ => None,
        }
    }

    /// 更新状态栏缓存（纯逻辑）
    pub fn update_status_bar(&mut self, sessions: &[SessionInfo], foreground_id: usize) {
        let bar = build_status_bar(sessions, foreground_id, self.width as usize);
        self.status_bar_cache = Some(bar);
    }

    /// 更新 Lua 状态栏缓存（纯逻辑）
    pub fn update_lua_status_bar(&mut self, sessions: &[SessionInfo], foreground_id: usize) {
        let text = build_lua_status_text(sessions, foreground_id, self.width as usize);
        self.lua_status_cache = if text.is_empty() { None } else { Some(text) };
    }

    /// 获取当前可见的输出行
    pub fn visible_output_lines(&self) -> &[String] {
        let output_height = self.output_height() as usize;
        let start = if self.output_lines.len() > output_height {
            self.output_lines.len() - output_height
        } else {
            0
        };
        &self.output_lines[start..]
    }

    /// 获取输入行显示内容（考虑滚动）
    pub fn input_display(&self) -> (String, usize) {
        let prompt_len: usize = 2; // "> "
        let avail_width = self.width as usize - prompt_len;
        let char_count = self.input_buffer.chars().count();
        let display_start = if self.input_cursor > avail_width {
            self.input_cursor - avail_width + 1
        } else {
            0
        };
        let display_end = std::cmp::min(display_start + avail_width, char_count);
        let display_str: String = self
            .input_buffer
            .chars()
            .skip(display_start)
            .take(display_end - display_start)
            .collect();
        let cursor_x = prompt_len + self.input_cursor - display_start;
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
    /// 注：仅重绘输入行（不触发 refresh_all），Enter 后的全屏刷新由调用方负责
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        let result = self.state.handle_key(key);
        let mut stdout = io::stdout();
        let _ = self.draw_input_line(&mut stdout);
        let _ = stdout.flush();
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
        let mut stdout = io::stdout();
        self.refresh_all(&mut stdout)?;
        Ok(())
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), terminal::LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
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
    fn test_state_handle_key_home_end() {
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
        let bar = build_status_bar(&[], 0, 80);
        assert!(bar.contains("RustLuaMud"));
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
        let bar = build_status_bar(&sessions, 0, 80);
        assert!(bar.contains("mud1"));
        assert!(bar.contains("mud2"));
        assert!(bar.contains("RustLuaMud"));
    }

    #[test]
    fn test_build_status_bar_foreground_highlight() {
        let sessions = vec![SessionInfo {
            name: "mud1".to_string(),
            state: SessionState::Connected,
            status_text: String::new(),
        }];
        let bar = build_status_bar(&sessions, 0, 80);
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
    }
}
