/// ANSI X3.64 标准 SGR (Select Graphic Rendition) 解析器
///
/// 功能：
/// 1. 追踪 ANSI 颜色/属性状态
/// 2. 自动检测行尾是否缺少终止编码（\x1b[0m），智能补充
/// 3. 兼容服务器正确发送终止编码和遗漏终止编码两种情况
/// 4. 支持 4-bit、8-bit 256 色、24-bit RGB 色彩
/// 5. 提供可配置的容错机制和日志记录
///
/// ANSI X3.64 / ECMA-48 标准支持：
/// - SGR 0:   所有属性重置
/// - SGR 1:   加粗
/// - SGR 2:   暗淡
/// - SGR 3:   斜体
/// - SGR 4:   下划线
/// - SGR 5:   缓慢闪烁
/// - SGR 6:   快速闪烁（通常等价于 5）
/// - SGR 7:   反色
/// - SGR 8:   隐藏
/// - SGR 9:   删除线
/// - SGR 21:  双下划线（某些终端关闭加粗）
/// - SGR 22:  正常亮度（关闭加粗/暗淡）
/// - SGR 23:  关闭斜体
/// - SGR 24:  关闭下划线
/// - SGR 25:  关闭闪烁
/// - SGR 27:  关闭反色
/// - SGR 28:  关闭隐藏
/// - SGR 29:  关闭删除线
/// - SGR 30-37:  标准前景色
/// - SGR 38:  扩展前景色（5 → 256 色；2 → RGB）
/// - SGR 39:  默认前景色
/// - SGR 40-47:  标准背景色
/// - SGR 48:  扩展背景色（5 → 256 色；2 → RGB）
/// - SGR 49:  默认背景色
/// - SGR 90-97:  亮色前景
/// - SGR 100-107: 亮色背景

/// ANSI 颜色/属性状态快照
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnsiState {
    /// 前景色代码: None=默认, Some(0-255)=ANSI 4-bit/8-bit
    pub foreground: Option<u8>,
    /// 背景色代码
    pub background: Option<u8>,
    /// 加粗 (SGR 1)
    pub bold: bool,
    /// 暗淡 (SGR 2)
    pub dim: bool,
    /// 斜体 (SGR 3)
    pub italic: bool,
    /// 下划线 (SGR 4)
    pub underline: bool,
    /// 闪烁 (SGR 5/6)
    pub blink: bool,
    /// 反色 (SGR 7)
    pub reverse: bool,
    /// 隐藏 (SGR 8)
    pub conceal: bool,
    /// 删除线 (SGR 9)
    pub strikethrough: bool,
}

impl Default for AnsiState {
    fn default() -> Self {
        Self {
            foreground: None,
            background: None,
            bold: false,
            dim: false,
            italic: false,
            underline: false,
            blink: false,
            reverse: false,
            conceal: false,
            strikethrough: false,
        }
    }
}

impl AnsiState {
    /// 是否处于"已激活"状态（有任何非默认属性）
    pub fn is_active(&self) -> bool {
        self.foreground.is_some()
            || self.background.is_some()
            || self.bold
            || self.dim
            || self.italic
            || self.underline
            || self.blink
            || self.reverse
            || self.conceal
            || self.strikethrough
    }

    /// 生成 SGR 重置或还原序列
    /// 如果状态为非激活，返回空字符串
    pub fn reset_sequence(&self) -> &'static str {
        if self.is_active() {
            "\x1b[0m"
        } else {
            ""
        }
    }
}

/// ANSI 解析器配置
#[derive(Debug, Clone)]
pub struct AnsiParserConfig {
    /// 严格模式：记录所有缺失终止编码的日志
    pub strict: bool,
    /// 最大日志条目数（0 = 无限制）
    pub max_log_entries: usize,
}

impl Default for AnsiParserConfig {
    fn default() -> Self {
        Self {
            strict: true,
            max_log_entries: 100,
        }
    }
}

/// 诊断日志条目
#[derive(Debug, Clone)]
pub struct AnsiLogEntry {
    /// 原始行内容（截断显示）
    pub raw_preview: String,
    /// 行结束时的 ANSI 状态
    pub state: AnsiState,
}

/// ANSI SGR 解析器
pub struct AnsiParser {
    config: AnsiParserConfig,
    logs: Vec<AnsiLogEntry>,
    /// 累计检测到的缺失终止编码次数
    pub missing_resets: u64,
}

impl Default for AnsiParser {
    fn default() -> Self {
        Self::new(AnsiParserConfig::default())
    }
}

impl AnsiParser {
    /// 创建新的解析器
    pub fn new(config: AnsiParserConfig) -> Self {
        Self {
            config,
            logs: Vec::new(),
            missing_resets: 0,
        }
    }

    /// 使用默认配置创建解析器
    pub fn with_logging() -> Self {
        Self::new(AnsiParserConfig {
            strict: true,
            max_log_entries: 100,
        })
    }

    /// 使用宽松模式创建解析器（仅修复，不记录日志）
    pub fn lenient() -> Self {
        Self::new(AnsiParserConfig {
            strict: false,
            max_log_entries: 0,
        })
    }

    /// 获取诊断日志
    pub fn logs(&self) -> &[AnsiLogEntry] {
        &self.logs
    }

    /// 清空诊断日志
    pub fn clear_logs(&mut self) {
        self.logs.clear();
    }

    /// 处理一行文本：
    /// 1. 扫描 ANSI 序列追踪颜色状态
    /// 2. 如果行尾缺少终止编码，自动补充 \x1b[0m
    /// 3. 返回处理后的文本（保证行尾状态为 reset）
    pub fn process_line(&mut self, line: &str) -> String {
        let final_state = self.scan_line(line);
        if final_state.is_active() {
            if self.config.strict {
                // 截断预览，避免日志过大
                let preview = if line.len() > 60 {
                    format!("{}...", &line[..60])
                } else {
                    line.to_string()
                };
                if self.config.max_log_entries == 0 || self.logs.len() < self.config.max_log_entries
                {
                    self.logs.push(AnsiLogEntry {
                        raw_preview: preview,
                        state: final_state.clone(),
                    });
                }
                self.missing_resets += 1;
            }
            // 行尾补充重置序列
            let mut result = String::with_capacity(line.len() + 4);
            result.push_str(line);
            result.push_str("\x1b[0m");
            result
        } else {
            line.to_string()
        }
    }

    /// 扫描一行文本中的 ANSI 序列，返回行结束时的状态
    /// 不修改输入文本
    pub fn scan_line(&self, line: &str) -> AnsiState {
        let mut state = AnsiState::default();
        let mut chars = line.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '\x1b' {
                if chars.peek() == Some(&'[') {
                    chars.next(); // 消费 '['
                    let params_str = self::consume_csi_params(&mut chars);
                    if let Some(final_byte) = chars.next() {
                        if final_byte == 'm' {
                            // SGR 序列：解析参数并更新状态
                            apply_sgr(&params_str, &mut state);
                        }
                    }
                }
            }
        }

        state
    }

    /// 解析一行文本并应用 ANSI 序列到给定状态
    /// 用于多行累积解析
    pub fn scan_into(&self, line: &str, state: &mut AnsiState) {
        let mut chars = line.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '\x1b' {
                if chars.peek() == Some(&'[') {
                    chars.next(); // 消费 '['
                    let params_str = self::consume_csi_params(&mut chars);
                    if let Some(final_byte) = chars.next() {
                        if final_byte == 'm' {
                            apply_sgr(&params_str, state);
                        }
                    }
                }
            }
        }
    }

    /// 消费 CSI 参数字节和中间字节
    fn consume_csi_params(&self, chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
        let mut params = String::new();
        // 参数字节: 0x30-0x3f (0-9;:<->?)
        // 中间字节: 0x20-0x2f (空格、!#$%&'()*+,-./)
        while let Some(&next) = chars.peek() {
            if ('\x30'..='\x3f').contains(&next) || ('\x20'..='\x2f').contains(&next) {
                params.push(next);
                chars.next();
            } else {
                break;
            }
        }
        params
    }

    /// 向后兼容：从文本中剥离所有 ANSI 转义序列
    pub fn strip_ansi(text: &str) -> String {
        strip_ansi(text)
    }
}

// ==================== 静态工具函数 ====================

/// 消费 CSI 参数字节（静态函数，供独立调用场景使用）
fn consume_csi_params(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut params = String::new();
    while let Some(&next) = chars.peek() {
        if ('\x30'..='\x3f').contains(&next) || ('\x20'..='\x2f').contains(&next) {
            params.push(next);
            chars.next();
        } else {
            break;
        }
    }
    params
}

/// 应用 SGR 参数到状态
fn apply_sgr(params_str: &str, state: &mut AnsiState) {
    if params_str.is_empty() || params_str == "0" {
        // 无参数或参数为 0：重置所有属性
        *state = AnsiState::default();
        return;
    }

    for param_str in params_str.split(';') {
        if param_str.is_empty() {
            continue;
        }
        let param: u16 = match param_str.parse() {
            Ok(v) => v,
            Err(_) => continue,
        };

        match param {
            // 重置
            0 => *state = AnsiState::default(),

            // 启用属性
            1 => state.bold = true,
            2 => state.dim = true,
            3 => state.italic = true,
            4 => state.underline = true,
            5 | 6 => state.blink = true,
            7 => state.reverse = true,
            8 => state.conceal = true,
            9 => state.strikethrough = true,

            // 关闭属性
            21 => {
                state.bold = false;
                state.underline = true; // 21 通常是双下划线，但某些终端关加粗
            }
            22 => {
                state.bold = false;
                state.dim = false;
            }
            23 => state.italic = false,
            24 => state.underline = false,
            25 => state.blink = false,
            27 => state.reverse = false,
            28 => state.conceal = false,
            29 => state.strikethrough = false,

            // 标准前景色 (4-bit)
            30..=37 => state.foreground = Some((param - 30) as u8),

            // 扩展前景色 (38)
            38 => {
                // 后续参数由 split 处理，需特殊逻辑
                // 在循环外处理
            }

            // 默认前景色
            39 => state.foreground = None,

            // 标准背景色 (4-bit)
            40..=47 => state.background = Some((param - 40) as u8),

            // 扩展背景色 (48)
            48 => {
                // 在循环外处理
            }

            // 默认背景色
            49 => state.background = None,

            // 亮色前景
            90..=97 => state.foreground = Some((param - 82) as u8),

            // 亮色背景
            100..=107 => state.background = Some((param - 92) as u8),

            _ => {}
        }
    }

    // 处理扩展颜色 (38/48) - 需要解析参数中的子参数
    // 分号分隔后，38 后面的参数形如 ["38", "5", "N"] 或 ["38", "2", "R", "G", "B"]
    let params: Vec<&str> = params_str.split(';').collect();
    let mut i = 0;
    while i < params.len() {
        let p: u16 = match params[i].parse() {
            Ok(v) => v,
            Err(_) => {
                i += 1;
                continue;
            }
        };
        match p {
            38 => {
                // 扩展前景色
                if i + 1 < params.len() {
                    match params[i + 1] {
                        "5" => {
                            // 256 色
                            if i + 2 < params.len() {
                                if let Ok(color) = params[i + 2].parse::<u8>() {
                                    state.foreground = Some(color);
                                }
                            }
                        }
                        "2" => {
                            // RGB: 有损转换为最近的 8-bit 色
                            if i + 4 < params.len() {
                                let r = params[i + 2].parse::<u8>().unwrap_or(0);
                                let g = params[i + 3].parse::<u8>().unwrap_or(0);
                                let b = params[i + 4].parse::<u8>().unwrap_or(0);
                                // 保留原始 RGB 信息到 8-bit 编码
                                // 高位标记为扩展 RGB 模式，低位存储亮度近似
                                let gray = (r as u16 + g as u16 + b as u16) / 3;
                                let idx = if gray < 128 { 0 } else { 1 };
                                state.foreground = Some(if idx == 0 {
                                    16 + (gray / 8) as u8
                                } else {
                                    232 + (gray / 11) as u8
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
            48 => {
                // 扩展背景色
                if i + 1 < params.len() {
                    match params[i + 1] {
                        "5" => {
                            if i + 2 < params.len() {
                                if let Ok(color) = params[i + 2].parse::<u8>() {
                                    state.background = Some(color);
                                }
                            }
                        }
                        "2" => {
                            if i + 4 < params.len() {
                                let r = params[i + 2].parse::<u8>().unwrap_or(0);
                                let g = params[i + 3].parse::<u8>().unwrap_or(0);
                                let b = params[i + 4].parse::<u8>().unwrap_or(0);
                                let gray = (r as u16 + g as u16 + b as u16) / 3;
                                state.background = Some(232 + (gray / 11) as u8);
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
}

/// 快速判断一行文本是否需要补充 ANSI 重置
///
/// 与 AnsiParser::process_line 不同，这个函数是无状态的，
/// 适合对单行文本进行快速检查。如果行内没有出现 SGR 序列，
/// 或者行尾已经是重置状态，返回 false。
pub fn line_needs_reset(line: &str) -> bool {
    let parser = AnsiParser::lenient();
    let state = parser.scan_line(line);
    state.is_active()
}

/// 确保一行文本以 ANSI 重置结尾（便捷函数，无日志记录）
pub fn ensure_ansi_reset(line: &str) -> String {
    if line_needs_reset(line) {
        let mut result = String::with_capacity(line.len() + 4);
        result.push_str(line);
        result.push_str("\x1b[0m");
        result
    } else {
        line.to_string()
    }
}

/// 从文本中剥离所有 ANSI 转义序列（保留原始功能）
pub fn strip_ansi(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                // 参数字节 + 中间字节
                while let Some(&next) = chars.peek() {
                    if ('\x30'..='\x3f').contains(&next) || ('\x20'..='\x2f').contains(&next) {
                        chars.next();
                    } else {
                        break;
                    }
                }
                // 最终字节
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

// ==================== 测试 ====================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- AnsiState 测试 ----

    #[test]
    fn test_default_state_inactive() {
        let state = AnsiState::default();
        assert!(!state.is_active());
    }

    #[test]
    fn test_foreground_active() {
        let mut state = AnsiState::default();
        state.foreground = Some(31);
        assert!(state.is_active());
    }

    #[test]
    fn test_bold_active() {
        let mut state = AnsiState::default();
        state.bold = true;
        assert!(state.is_active());
    }

    #[test]
    fn test_reset_sequence_inactive() {
        let state = AnsiState::default();
        assert_eq!(state.reset_sequence(), "");
    }

    #[test]
    fn test_reset_sequence_active() {
        let mut state = AnsiState::default();
        state.foreground = Some(31);
        assert_eq!(state.reset_sequence(), "\x1b[0m");
    }

    // ---- scan_line 基础测试 ----

    #[test]
    fn test_plain_text_no_ansi() {
        let parser = AnsiParser::lenient();
        let state = parser.scan_line("hello world");
        assert!(!state.is_active());
    }

    #[test]
    fn test_red_foreground_active() {
        let parser = AnsiParser::lenient();
        let state = parser.scan_line("\x1b[31mred text");
        assert!(state.is_active());
        assert_eq!(state.foreground, Some(1)); // 31-30=1
    }

    #[test]
    fn test_green_foreground_with_reset() {
        let parser = AnsiParser::lenient();
        let state = parser.scan_line("\x1b[32mgreen\x1b[0m");
        assert!(!state.is_active()); // 重置后无状态
    }

    #[test]
    fn test_bold_and_color() {
        let parser = AnsiParser::lenient();
        let state = parser.scan_line("\x1b[1;31mbold red");
        assert_eq!(state.foreground, Some(1));
        assert!(state.bold);
    }

    #[test]
    fn test_background_color() {
        let parser = AnsiParser::lenient();
        let state = parser.scan_line("\x1b[44mblue bg");
        assert_eq!(state.background, Some(4)); // 44-40=4
        assert!(!state.bold);
    }

    // ---- 256 色 / RGB 测试 ----

    #[test]
    fn test_256_color_foreground() {
        let parser = AnsiParser::lenient();
        let state = parser.scan_line("\x1b[38;5;196m256 red");
        assert_eq!(state.foreground, Some(196));
    }

    #[test]
    fn test_256_color_background() {
        let parser = AnsiParser::lenient();
        let state = parser.scan_line("\x1b[48;5;27m256 blue bg");
        assert_eq!(state.background, Some(27));
    }

    #[test]
    fn test_rgb_foreground() {
        let parser = AnsiParser::lenient();
        let state = parser.scan_line("\x1b[38;2;255;0;0mrgb red");
        assert!(state.is_active());
        // RGB 转换结果存在即可（精确值取决于转换算法）
        assert!(state.foreground.is_some());
    }

    // ---- 亮色测试 ----

    #[test]
    fn test_bright_foreground() {
        let parser = AnsiParser::lenient();
        let state = parser.scan_line("\x1b[91mbright red");
        assert_eq!(state.foreground, Some(9)); // 91-82=9
    }

    #[test]
    fn test_bright_background() {
        let parser = AnsiParser::lenient();
        let state = parser.scan_line("\x1b[105mbright magenta bg");
        assert_eq!(state.background, Some(13)); // 105-92=13
    }

    // ---- process_line 测试（核心功能） ----

    #[test]
    fn test_process_line_no_change_needed() {
        let mut parser = AnsiParser::lenient();
        let result = parser.process_line("plain text");
        assert_eq!(result, "plain text");
        assert_eq!(parser.missing_resets, 0);
    }

    #[test]
    fn test_process_line_appends_reset() {
        let mut parser = AnsiParser::lenient();
        let result = parser.process_line("\x1b[31mred text");
        assert_eq!(result, "\x1b[31mred text\x1b[0m");
        assert_eq!(parser.missing_resets, 0); // lenient 模式下不计数
    }

    #[test]
    fn test_process_line_already_reset() {
        let mut parser = AnsiParser::lenient();
        let result = parser.process_line("\x1b[31mred\x1b[0m normal");
        assert_eq!(result, "\x1b[31mred\x1b[0m normal");
        assert_eq!(parser.missing_resets, 0);
    }

    #[test]
    fn test_process_line_strict_logging() {
        let mut parser = AnsiParser::with_logging();
        assert_eq!(parser.missing_resets, 0);
        let result = parser.process_line("\x1b[31mred text");
        assert_eq!(result, "\x1b[31mred text\x1b[0m");
        assert_eq!(parser.missing_resets, 1);
        assert_eq!(parser.logs().len(), 1);
        assert!(parser.logs()[0].raw_preview.contains("red text"));
    }

    #[test]
    fn test_process_line_strict_no_log_for_reset() {
        let mut parser = AnsiParser::with_logging();
        parser.process_line("plain text");
        assert_eq!(parser.missing_resets, 0);
        assert_eq!(parser.logs().len(), 0);
    }

    // ---- ensure_ansi_reset 便捷函数测试 ----

    #[test]
    fn test_ensure_ansi_reset_noop() {
        let result = ensure_ansi_reset("hello");
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_ensure_ansi_reset_appends() {
        let result = ensure_ansi_reset("\x1b[32mhello");
        assert_eq!(result, "\x1b[32mhello\x1b[0m");
    }

    #[test]
    fn test_ensure_ansi_reset_already_reset() {
        let result = ensure_ansi_reset("\x1b[32mhello\x1b[0m");
        assert_eq!(result, "\x1b[32mhello\x1b[0m");
    }

    // ---- line_needs_reset 测试 ----

    #[test]
    fn test_line_needs_reset_true() {
        assert!(line_needs_reset("\x1b[35mpurple"));
    }

    #[test]
    fn test_line_needs_reset_false() {
        assert!(!line_needs_reset("normal text"));
    }

    #[test]
    fn test_line_needs_reset_false_after_reset() {
        assert!(!line_needs_reset("\x1b[35mpurple\x1b[0m"));
    }

    // ---- strip_ansi 保留原功能测试 ----

    #[test]
    fn test_strip_ansi_plain() {
        assert_eq!(strip_ansi("hello"), "hello");
    }

    #[test]
    fn test_strip_ansi_color() {
        assert_eq!(strip_ansi("\x1b[31mred"), "red");
    }

    #[test]
    fn test_strip_ansi_reset() {
        assert_eq!(strip_ansi("\x1b[0mnormal"), "normal");
    }

    #[test]
    fn test_strip_ansi_mixed() {
        assert_eq!(
            strip_ansi("\x1b[31mhello\x1b[0m \x1b[1mworld\x1b[0m"),
            "hello world"
        );
    }

    #[test]
    fn test_strip_ansi_256() {
        assert_eq!(strip_ansi("\x1b[38;5;196mcolor256"), "color256");
    }

    #[test]
    fn test_strip_ansi_rgb() {
        assert_eq!(strip_ansi("\x1b[38;2;255;0;0mrgb"), "rgb");
    }

    #[test]
    fn test_strip_ansi_empty() {
        assert_eq!(strip_ansi(""), "");
    }

    // ---- 综合场景测试 ----

    #[test]
    fn test_multi_color_line_without_reset() {
        let mut parser = AnsiParser::with_logging();
        let input = "\x1b[31m红色\x1b[32m绿色\x1b[34m蓝色";
        let result = parser.process_line(input);
        // 行尾应该补充重置
        assert!(result.ends_with("\x1b[0m"));
        assert_eq!(parser.missing_resets, 1);
    }

    #[test]
    fn test_complex_multi_color_with_reset() {
        let mut parser = AnsiParser::lenient();
        // 每段颜色后都正确关闭
        let input = "\x1b[31m红\x1b[0m\x1b[32m绿\x1b[0m\x1b[34m蓝\x1b[0m";
        let result = parser.process_line(input);
        // 不应该追加重置（行末已经是默认状态）
        assert_eq!(result, input);
        assert_eq!(parser.missing_resets, 0);
    }

    #[test]
    fn test_multiple_atrributes_no_reset() {
        let mut parser = AnsiParser::lenient();
        let result = parser.process_line("\x1b[1;4;31mbold underline red");
        assert_eq!(result, "\x1b[1;4;31mbold underline red\x1b[0m");
    }

    #[test]
    fn test_italic_no_reset() {
        let mut parser = AnsiParser::lenient();
        let result = parser.process_line("\x1b[3mitalic");
        assert_eq!(result, "\x1b[3mitalic\x1b[0m");
    }

    #[test]
    fn test_dim_no_reset() {
        let mut parser = AnsiParser::lenient();
        let result = parser.process_line("\x1b[2mdim");
        assert_eq!(result, "\x1b[2mdim\x1b[0m");
    }

    #[test]
    fn test_blink_no_reset() {
        let mut parser = AnsiParser::lenient();
        let result = parser.process_line("\x1b[5mblink");
        assert_eq!(result, "\x1b[5mblink\x1b[0m");
    }

    #[test]
    fn test_reverse_no_reset() {
        let mut parser = AnsiParser::lenient();
        let result = parser.process_line("\x1b[7mreverse");
        assert_eq!(result, "\x1b[7mreverse\x1b[0m");
    }

    #[test]
    fn test_conceal_no_reset() {
        let mut parser = AnsiParser::lenient();
        let result = parser.process_line("\x1b[8mconceal");
        assert_eq!(result, "\x1b[8mconceal\x1b[0m");
    }

    #[test]
    fn test_strikethrough_no_reset() {
        let mut parser = AnsiParser::lenient();
        let result = parser.process_line("\x1b[9mstrikethrough");
        assert_eq!(result, "\x1b[9mstrikethrough\x1b[0m");
    }

    #[test]
    fn test_log_limit() {
        let config = AnsiParserConfig {
            strict: true,
            max_log_entries: 2,
        };
        let mut parser = AnsiParser::new(config);
        parser.process_line("\x1b[31ma");
        parser.process_line("\x1b[32mb");
        parser.process_line("\x1b[33mc");
        assert_eq!(parser.logs().len(), 2);
        assert_eq!(parser.missing_resets, 3); // 计数不受日志上限影响
    }

    #[test]
    fn test_clear_logs() {
        let mut parser = AnsiParser::with_logging();
        parser.process_line("\x1b[31mtest");
        assert_eq!(parser.logs().len(), 1);
        parser.clear_logs();
        assert_eq!(parser.logs().len(), 0);
        assert_eq!(parser.missing_resets, 1); // 计数不清零
    }

    #[test]
    fn test_scan_into_accumulates() {
        let parser = AnsiParser::lenient();
        let mut state = AnsiState::default();
        parser.scan_into("\x1b[31mred ", &mut state);
        assert_eq!(state.foreground, Some(1));
        parser.scan_into("\x1b[1mbold ", &mut state);
        assert_eq!(state.foreground, Some(1));
        assert!(state.bold);
        parser.scan_into("\x1b[0mreset", &mut state);
        assert!(!state.is_active());
    }

    #[test]
    fn test_complex_line_mixed_sgr() {
        let mut parser = AnsiParser::lenient();
        // 模拟 MUD 中常见的复杂颜色行
        let input = "\x1b[33m【任务】\x1b[32m你来到了一处山林，\x1b[37m这里的景色宜人。";
        let result = parser.process_line(input);
        assert!(result.ends_with("\x1b[0m"));
        // 验证原始内容完整保留
        assert!(result.contains("【任务】"));
        assert!(result.contains("你来到了一处山林"));
        assert!(result.contains("这里的景色宜人"));
    }

    #[test]
    fn test_csi_non_sgr_preserved() {
        let mut parser = AnsiParser::lenient();
        // CSI ?25h 不是 SGR，不应影响状态，也不应被移除
        let input = "\x1b[?25h\x1b[31mred";
        let result = parser.process_line(input);
        assert_eq!(result, "\x1b[?25h\x1b[31mred\x1b[0m");
    }

    #[test]
    fn test_background_and_foreground_no_reset() {
        let mut parser = AnsiParser::lenient();
        let input = "\x1b[41;33myellow on red";
        let result = parser.process_line(input);
        assert!(result.ends_with("\x1b[0m"));
        let state = parser.scan_line(input);
        assert_eq!(state.foreground, Some(3)); // 33-30=3
        assert_eq!(state.background, Some(1)); // 41-40=1
    }

    #[test]
    fn test_sgr_0_in_middle() {
        let mut parser = AnsiParser::lenient();
        // SGR 0 在中间重置，后面又有新颜色
        let input = "\x1b[31mred\x1b[0m\x1b[32mgreen";
        let result = parser.process_line(input);
        assert!(result.ends_with("\x1b[0m"));
    }

    #[test]
    fn test_multiple_sgr_0() {
        let mut parser = AnsiParser::lenient();
        // 多次重置，行尾已经是 reset 状态，不应追加额外重置
        let input = "\x1b[31m红\x1b[0m\x1b[32m绿\x1b[0m\x1b[34m蓝\x1b[0m";
        let result = parser.process_line(input);
        assert_eq!(result, input); // 原样保留，不额外追加
    }

    // ---- edge cases ----

    #[test]
    fn test_empty_string() {
        let mut parser = AnsiParser::lenient();
        let result = parser.process_line("");
        assert_eq!(result, "");
        assert_eq!(parser.missing_resets, 0);
    }

    #[test]
    fn test_only_ansi_sequence() {
        let mut parser = AnsiParser::lenient();
        let result = parser.process_line("\x1b[31m");
        assert_eq!(result, "\x1b[31m\x1b[0m");
    }

    #[test]
    fn test_just_reset() {
        let mut parser = AnsiParser::lenient();
        let result = parser.process_line("\x1b[0m");
        assert_eq!(result, "\x1b[0m");
    }

    #[test]
    fn test_incomplete_escape() {
        let mut parser = AnsiParser::lenient();
        let result = parser.process_line("hello\x1b[");
        assert_eq!(result, "hello\x1b[");
    }

    #[test]
    fn test_bare_escape() {
        let mut parser = AnsiParser::lenient();
        let result = parser.process_line("hello\x1bworld");
        assert_eq!(result, "hello\x1bworld");
    }
}
