use std::io::{self, Write};
use crossterm::{
    cursor,
    event::{KeyCode, KeyEvent, KeyModifiers},
    execute, queue,
    style::{self, Color, Print, SetForegroundColor},
    terminal::{self, ClearType},
};

use crate::connection::SessionState;

/// 终端 UI 渲染器
pub struct Terminal {
    /// 输出缓冲区（滚动回看用）
    output_lines: Vec<String>,
    /// 当前输入行内容
    input_buffer: String,
    /// 输入光标位置
    input_cursor: usize,
    /// 命令历史
    history: Vec<String>,
    /// 历史浏览位置
    history_pos: usize,
    /// 终端宽度
    width: u16,
    /// 终端高度
    height: u16,
    /// 状态栏高度
    status_height: u16,
    /// 输入行高度
    input_height: u16,
}

impl Terminal {
    pub fn new() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let (width, height) = terminal::size()?;
        Ok(Self {
            output_lines: Vec::new(),
            input_buffer: String::new(),
            input_cursor: 0,
            history: Vec::new(),
            history_pos: 0,
            width,
            height,
            status_height: 1,
            input_height: 1,
        })
    }

    /// 获取输出区可用行数
    fn output_height(&self) -> u16 {
        self.height.saturating_sub(self.status_height + self.input_height)
    }

    /// 初始化屏幕
    pub fn init_screen(&mut self) -> io::Result<()> {
        let mut stdout = io::stdout();
        execute!(stdout, terminal::EnterAlternateScreen, terminal::Clear(ClearType::All))?;
        self.refresh_all(&mut stdout)?;
        Ok(())
    }

    /// 完整刷新屏幕
    fn refresh_all(&self, stdout: &mut io::Stdout) -> io::Result<()> {
        // 状态栏
        queue!(stdout, cursor::MoveTo(0, 0))?;
        // 输出区
        let output_height = self.output_height() as usize;
        let start = if self.output_lines.len() > output_height {
            self.output_lines.len() - output_height
        } else {
            0
        };
        for (i, line) in self.output_lines[start..].iter().enumerate() {
            queue!(stdout, cursor::MoveTo(0, self.status_height + i as u16))?;
            queue!(stdout, terminal::Clear(ClearType::CurrentLine))?;
            // 截断超出宽度的行
            let display: String = line.chars().take(self.width as usize).collect();
            queue!(stdout, Print(&display))?;
        }
        // 输入行
        self.draw_input_line(stdout)?;
        stdout.flush()?;
        Ok(())
    }

    /// 绘制状态栏
    pub fn draw_status_bar(
        &self,
        stdout: &mut io::Stdout,
        session_name: &str,
        session_state: &SessionState,
        session_count: usize,
        foreground_id: usize,
    ) -> io::Result<()> {
        let state_icon = match session_state {
            SessionState::Connected => "●",
            SessionState::Disconnected => "○",
            SessionState::Connecting => "◎",
            SessionState::Reconnecting => "⟳",
        };
        let status_text = format!(
            " [{}]{} {}/{} RustLuaMud ",
            foreground_id + 1,
            session_name,
            state_icon,
            session_count,
        );
        queue!(stdout, cursor::MoveTo(0, 0))?;
        queue!(stdout, terminal::Clear(ClearType::CurrentLine))?;
        queue!(stdout, SetForegroundColor(Color::Cyan), Print(status_text))?;
        queue!(stdout, style::ResetColor)?;
        stdout.flush()?;
        Ok(())
    }

    /// 追加一行输出
    pub fn append_output(&mut self, line: &str) -> io::Result<()> {
        // 处理 \r\n 和 \n
        for part in line.split_inclusive(|c| c == '\n') {
            let trimmed = part.trim_end_matches('\r').trim_end_matches('\n');
            if !trimmed.is_empty() {
                self.output_lines.push(trimmed.to_string());
            }
        }

        // 限制缓冲区大小
        let max_lines = 5000;
        if self.output_lines.len() > max_lines {
            let drain_count = self.output_lines.len() - max_lines;
            self.output_lines.drain(..drain_count);
        }

        let mut stdout = io::stdout();
        self.refresh_all(&mut stdout)?;
        Ok(())
    }

    /// 绘制输入行
    fn draw_input_line(&self, stdout: &mut io::Stdout) -> io::Result<()> {
        let input_y = self.height.saturating_sub(1);
        queue!(stdout, cursor::MoveTo(0, input_y))?;
        queue!(stdout, terminal::Clear(ClearType::CurrentLine))?;
        queue!(stdout, SetForegroundColor(Color::Green), Print("> "))?;
        queue!(stdout, style::ResetColor)?;

        // 只显示光标位置附近能放入终端宽度的内容
        let prompt_len = 2; // "> "
        let avail_width = self.width as usize - prompt_len;
        let display_start = if self.input_cursor > avail_width {
            self.input_cursor - avail_width + 1
        } else {
            0
        };
        let display_end = std::cmp::min(display_start + avail_width, self.input_buffer.len());
        let display_str: String = self.input_buffer.chars()
            .skip(display_start)
            .take(display_end - display_start)
            .collect();
        queue!(stdout, Print(&display_str))?;

        // 设置光标位置
        let cursor_x = (prompt_len + self.input_cursor - display_start) as u16;
        queue!(stdout, cursor::MoveTo(cursor_x, input_y))?;
        Ok(())
    }

    /// 处理键盘事件，返回是否需要发送命令
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        match (key.modifiers, key.code) {
            // Ctrl+C / Ctrl+D: 退出信号
            (KeyModifiers::CONTROL, KeyCode::Char('c')) |
            (KeyModifiers::CONTROL, KeyCode::Char('d')) => {
                None // 退出由 app 层处理
            }

            // Enter: 提交命令
            (KeyModifiers::NONE, KeyCode::Enter) => {
                let cmd = self.input_buffer.clone();
                if !cmd.is_empty() {
                    self.history.push(cmd.clone());
                    self.history_pos = self.history.len();
                }
                self.input_buffer.clear();
                self.input_cursor = 0;
                let _ = self.refresh_all(&mut io::stdout());
                Some(cmd)
            }

            // Backspace
            (KeyModifiers::NONE, KeyCode::Backspace) => {
                if self.input_cursor > 0 {
                    self.input_cursor -= 1;
                    self.input_buffer.remove(self.input_cursor);
                    let _ = self.refresh_all(&mut io::stdout());
                }
                None
            }

            // Delete
            (KeyModifiers::NONE, KeyCode::Delete) => {
                if self.input_cursor < self.input_buffer.len() {
                    self.input_buffer.remove(self.input_cursor);
                    let _ = self.refresh_all(&mut io::stdout());
                }
                None
            }

            // Left arrow
            (KeyModifiers::NONE, KeyCode::Left) => {
                if self.input_cursor > 0 {
                    self.input_cursor -= 1;
                    let _ = self.refresh_all(&mut io::stdout());
                }
                None
            }

            // Right arrow
            (KeyModifiers::NONE, KeyCode::Right) => {
                if self.input_cursor < self.input_buffer.len() {
                    self.input_cursor += 1;
                    let _ = self.refresh_all(&mut io::stdout());
                }
                None
            }

            // Up arrow: 历史上翻
            (KeyModifiers::NONE, KeyCode::Up) => {
                if self.history_pos > 0 {
                    self.history_pos -= 1;
                    self.input_buffer = self.history[self.history_pos].clone();
                    self.input_cursor = self.input_buffer.len();
                    let _ = self.refresh_all(&mut io::stdout());
                }
                None
            }

            // Down arrow: 历史下翻
            (KeyModifiers::NONE, KeyCode::Down) => {
                if self.history_pos < self.history.len() {
                    self.history_pos += 1;
                    if self.history_pos < self.history.len() {
                        self.input_buffer = self.history[self.history_pos].clone();
                    } else {
                        self.input_buffer.clear();
                    }
                    self.input_cursor = self.input_buffer.len();
                    let _ = self.refresh_all(&mut io::stdout());
                }
                None
            }

            // Home: 光标到行首
            (KeyModifiers::NONE, KeyCode::Home) => {
                self.input_cursor = 0;
                let _ = self.refresh_all(&mut io::stdout());
                None
            }

            // End: 光标到行尾
            (KeyModifiers::NONE, KeyCode::End) => {
                self.input_cursor = self.input_buffer.len();
                let _ = self.refresh_all(&mut io::stdout());
                None
            }

            // 普通字符输入
            (KeyModifiers::NONE, KeyCode::Char(c)) => {
                self.input_buffer.insert(self.input_cursor, c);
                self.input_cursor += 1;
                let _ = self.refresh_all(&mut io::stdout());
                None
            }

            _ => None,
        }
    }

    /// 获取当前输入缓冲区内容
    pub fn input_buffer(&self) -> &str {
        &self.input_buffer
    }

    /// 处理终端大小变化
    pub fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        let _ = self.refresh_all(&mut io::stdout());
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), terminal::LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}
