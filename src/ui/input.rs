use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// 输入事件
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum InputEvent {
    /// 用户输入了一行命令
    Command(String),
    /// 请求退出
    Quit,
    /// 切换到指定编号的连接 (0-based)
    SwitchConnection(usize),
}

/// 输入处理器
pub struct InputHandler;
#[allow(dead_code)]
impl InputHandler {
    /// 从终端事件中解析出应用级输入事件
    pub fn handle_key_event(key: KeyEvent) -> Option<InputEvent> {
        match (key.modifiers, key.code) {
            // Ctrl+C / Ctrl+D: 退出
            (KeyModifiers::CONTROL, KeyCode::Char('c'))
            | (KeyModifiers::CONTROL, KeyCode::Char('d')) => Some(InputEvent::Quit),

            // Alt+1~9: 切换连接
            (KeyModifiers::ALT, KeyCode::Char(c)) if ('1'..='9').contains(&c) => {
                Some(InputEvent::SwitchConnection((c as usize) - ('1' as usize)))
            }

            // Enter: 提交当前输入行（由 app 层管理输入缓冲区）
            (KeyModifiers::NONE, KeyCode::Enter) => Some(InputEvent::Command(String::new())),

            _ => None,
        }
    }
}
