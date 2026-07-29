use std::io::Write;
use std::sync::OnceLock;

/// 调试日志：写入 logs/debug.log，用于追踪 Lua print 输出是否到达终端渲染
/// 启用方式：设置环境变量 RUSTLUA_DEBUG=1
pub fn debug_log(msg: &str) {
    static DEBUG_ENABLED: OnceLock<bool> = OnceLock::new();
    let enabled =
        *DEBUG_ENABLED.get_or_init(|| std::env::var("RUSTLUA_DEBUG").unwrap_or_default() == "1");
    if !enabled {
        return;
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let line = format!("[{}] {}\n", ts, msg);
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("logs/debug.log")
        .and_then(|mut f| f.write_all(line.as_bytes()));
}

/// 安全截取字符串用于日志输出（按 UTF-8 字符边界，不会 panic）
pub fn truncate_for_log(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    // 找到不超过 max_bytes 的最后一个字符边界
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &s[..end])
}
