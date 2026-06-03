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
                    if chars.peek().map_or(false, |c| ('\x40'..='\x7e').contains(c)) {
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
