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

#[cfg(test)]
mod tests {
    use super::*;

    fn ctrl_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn alt_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT)
    }

    fn normal_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn test_ctrl_c_quits() {
        let result = InputHandler::handle_key_event(ctrl_key('c'));
        assert!(matches!(result, Some(InputEvent::Quit)));
    }

    #[test]
    fn test_ctrl_d_quits() {
        let result = InputHandler::handle_key_event(ctrl_key('d'));
        assert!(matches!(result, Some(InputEvent::Quit)));
    }

    #[test]
    fn test_alt_1_switches_to_connection_0() {
        let result = InputHandler::handle_key_event(alt_key('1'));
        assert!(matches!(result, Some(InputEvent::SwitchConnection(0))));
    }

    #[test]
    fn test_alt_9_switches_to_connection_8() {
        let result = InputHandler::handle_key_event(alt_key('9'));
        assert!(matches!(result, Some(InputEvent::SwitchConnection(8))));
    }

    #[test]
    fn test_alt_0_unhandled() {
        let result = InputHandler::handle_key_event(alt_key('0'));
        assert!(result.is_none());
    }

    #[test]
    fn test_alt_letter_unhandled() {
        let result = InputHandler::handle_key_event(alt_key('a'));
        assert!(result.is_none());
    }

    #[test]
    fn test_enter_returns_command() {
        let result =
            InputHandler::handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(result, Some(InputEvent::Command(_))));
    }

    #[test]
    fn test_normal_char_unhandled() {
        let result = InputHandler::handle_key_event(normal_key('a'));
        assert!(result.is_none());
    }

    #[test]
    fn test_escape_unhandled() {
        let result =
            InputHandler::handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(result.is_none());
    }
}
