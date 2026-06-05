/// ANSI 转义序列解析器（Phase 1: 简单透传，Phase 2 完善解析）
pub struct AnsiParser;

impl AnsiParser {
    /// Phase 1: 直接返回原始文本，不做 ANSI 处理
    /// 后续 Phase 2 会实现完整的 ANSI 解析与颜色映射
    pub fn strip_ansi(text: &str) -> String {
        // 简单移除 ANSI 转义序列，避免终端混乱
        let mut result = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '\x1b' {
                // ESC 序列
                if chars.peek() == Some(&'[') {
                    chars.next(); // 消费 '['
                                  // 跳过参数字节 (0x30-0x3f) 和中间字节 (0x20-0x2f)
                    while let Some(&next) = chars.peek() {
                        if ('\x30'..='\x3f').contains(&next) || ('\x20'..='\x2f').contains(&next) {
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    // 消费最终字节 (0x40-0x7e)
                    if chars.peek().is_some_and(|c| ('\x40'..='\x7e').contains(c)) {
                        chars.next();
                    }
                }
            } else {
                result.push(c);
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_plain_text() {
        assert_eq!(AnsiParser::strip_ansi("hello world"), "hello world");
    }

    #[test]
    fn test_strip_basic_color() {
        // \x1b[31m = 红色前景
        assert_eq!(AnsiParser::strip_ansi("\x1b[31mred text"), "red text");
    }

    #[test]
    fn test_strip_reset() {
        assert_eq!(AnsiParser::strip_ansi("\x1b[0mnormal"), "normal");
    }

    #[test]
    fn test_strip_bold() {
        assert_eq!(AnsiParser::strip_ansi("\x1b[1mbold"), "bold");
    }

    #[test]
    fn test_strip_256_color() {
        // \x1b[38;5;196m = 256色前景
        assert_eq!(AnsiParser::strip_ansi("\x1b[38;5;196mcolor256"), "color256");
    }

    #[test]
    fn test_strip_rgb_color() {
        // \x1b[38;2;255;0;0m = RGB前景
        assert_eq!(AnsiParser::strip_ansi("\x1b[38;2;255;0;0mrgb"), "rgb");
    }

    #[test]
    fn test_strip_mixed_text_and_ansi() {
        let input = "\x1b[31mhello\x1b[0m \x1b[1mworld\x1b[0m";
        assert_eq!(AnsiParser::strip_ansi(input), "hello world");
    }

    #[test]
    fn test_strip_empty_string() {
        assert_eq!(AnsiParser::strip_ansi(""), "");
    }

    #[test]
    fn test_strip_bare_escape() {
        // ESC 后面没有 '['，ESC 被消费，后续字符保留
        assert_eq!(AnsiParser::strip_ansi("\x1bX"), "X");
    }

    #[test]
    fn test_strip_incomplete_sequence() {
        // ESC[ 后面没有最终字节
        assert_eq!(AnsiParser::strip_ansi("\x1b["), "");
    }

    #[test]
    fn test_strip_multiple_codes() {
        // \x1b[31;1m = 红色+粗体
        assert_eq!(AnsiParser::strip_ansi("\x1b[31;1mtext"), "text");
    }

    #[test]
    fn test_preserve_newlines() {
        assert_eq!(AnsiParser::strip_ansi("line1\nline2"), "line1\nline2");
    }

    #[test]
    fn test_strip_ansi_empty_input() {
        assert_eq!(AnsiParser::strip_ansi(""), "");
    }

    #[test]
    fn test_strip_csi_with_question_mark() {
        // CSI ? 25 h (show cursor)
        assert_eq!(AnsiParser::strip_ansi("\x1b[?25htext"), "text");
    }

    #[test]
    fn test_strip_mixed_ansi_and_text() {
        assert_eq!(
            AnsiParser::strip_ansi("hello \x1b[32mworld\x1b[0m!"),
            "hello world!"
        );
    }
}
