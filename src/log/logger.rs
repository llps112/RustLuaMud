use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::Local;

/// 日志分类
#[derive(Clone, Copy)]
pub enum LogCategory {
    /// 服务器输出
    Output,
    /// 脚本发送的指令
    Command,
    /// /lua 指令
    Lua,
    /// 调试信息
    Debug,
    /// Rust panic 信息
    Panic,
    /// 断线事件
    Disconnect,
    /// 重连事件
    Reconnect,
}

impl LogCategory {
    fn tag(&self) -> &'static str {
        match self {
            LogCategory::Output => "OUT",
            LogCategory::Command => "CMD",
            LogCategory::Lua => "LUA",
            LogCategory::Debug => "DBG",
            LogCategory::Panic => "PNC",
            LogCategory::Disconnect => "DCN",
            LogCategory::Reconnect => "RCN",
        }
    }
}

/// 日志记录器，按小时分割，最多保留 max_files 个历史文件。
pub struct Logger {
    log_dir: PathBuf,
    max_files: usize,
    /// 按 session 覆盖的保留数量（session_name -> count）
    per_session_max_files: Mutex<HashMap<String, usize>>,
    /// 上次执行目录清理时的时间后缀（session_name -> `YYMMDD_HH`）
    ///
    /// 旧文件只会在跨小时产生新文件时出现，故清理只需在新小时的首条日志执行一次。
    /// 没有这张表，每条日志都要重新编译正则、扫描并排序整个日志目录（实测占单条
    /// 写入开销的 98%）。
    last_cleanup_suffix: Mutex<HashMap<String, String>>,
}

impl Logger {
    pub fn new(log_dir: &str, max_files: usize) -> Self {
        let log_dir = PathBuf::from(log_dir);
        // 确保日志目录存在
        let _ = fs::create_dir_all(&log_dir);
        Self {
            log_dir,
            max_files,
            per_session_max_files: Mutex::new(HashMap::new()),
            last_cleanup_suffix: Mutex::new(HashMap::new()),
        }
    }

    /// 获取当前日志目录（测试断言用）
    pub fn log_dir(&self) -> &Path {
        &self.log_dir
    }

    /// 设置指定 session 的日志保留数量，覆盖全局 max_files
    pub fn set_session_max_files(&self, session_name: &str, count: usize) {
        if let Ok(mut map) = self.per_session_max_files.lock() {
            map.insert(session_name.to_string(), count);
        }
    }

    /// 获取当前时间后缀，格式: YYMMDD_HH，例如 250626_14
    fn timestamp_suffix() -> String {
        Local::now().format("%y%m%d_%H").to_string()
    }

    /// 获取当前时间对应的日志文件路径
    /// 格式: `<session>_<YYMMDD_HH>.log`，例如 `mud_250626_14.log`，每小时滚动
    fn log_path(&self, session_name: &str, suffix: &str) -> PathBuf {
        self.log_dir
            .join(format!("{}_{}.log", session_name, suffix))
    }

    /// 判断文件名是否为该 session 的日志文件
    ///
    /// 命名固定为 `<session>_<YYMMDD_HH>.log`，前缀必须紧跟 `_`，且 `_` 之后、
    /// `.log` 之前必须严格是时间戳段（6 位数字 + `_` + 2 位数字）。仅校验「前缀 +
    /// `_` + 后缀」会让 `mud` 误配到 `mud_alt_260918_10.log`（另一会话 `mud_alt`
    /// 的日志），进而在清理时把它当自己的旧文件删掉，故这里对时间戳段做精确校验。
    fn is_session_log(file_name: &str, session_name: &str) -> bool {
        let Some(rest) = file_name
            .strip_prefix(session_name)
            .and_then(|r| r.strip_prefix('_'))
            .and_then(|r| r.strip_suffix(".log"))
        else {
            return false;
        };
        // rest 必须是 `YYMMDD_HH`：恰好 9 字节，第 7 字节为 `_`，其余为 ASCII 数字
        let bytes = rest.as_bytes();
        bytes.len() == 9
            && bytes[..6].iter().all(u8::is_ascii_digit)
            && bytes[6] == b'_'
            && bytes[7..].iter().all(u8::is_ascii_digit)
    }

    /// 清理同 session 的旧日志文件，只保留最新的 max_files 个
    fn cleanup_old_logs(&self, session_name: &str) {
        let max_files = self
            .per_session_max_files
            .lock()
            .ok()
            .and_then(|map| map.get(session_name).copied())
            .unwrap_or(self.max_files);

        let mut entries: Vec<(String, PathBuf)> = match fs::read_dir(&self.log_dir) {
            Ok(rd) => rd
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                .filter_map(|e| {
                    let name = e.file_name();
                    let name = name.to_string_lossy();
                    Self::is_session_log(&name, session_name).then(|| (name.into_owned(), e.path()))
                })
                .collect(),
            Err(_) => return,
        };

        // 按文件名（即时间）降序，最新的排前面
        entries.sort_by(|a, b| b.0.cmp(&a.0));

        // 删除超出 max_files 的旧文件
        for (_, path) in entries.iter().skip(max_files) {
            let _ = fs::remove_file(path);
        }
    }

    /// 仅当该 session 跨入新的小时（时间后缀变化）时清理一次旧日志
    ///
    /// 代价从「每条日志一次目录扫描」降为「每 session 每小时一次」。副作用：
    /// 已有 session 的 `max_files` 若在整点之间被下调，新上限要到下个整点才生效。
    fn cleanup_if_rolled_over(&self, session_name: &str, suffix: &str) {
        let rolled_over = match self.last_cleanup_suffix.lock() {
            Ok(mut map) => match map.get(session_name) {
                Some(last) if last == suffix => false,
                _ => {
                    map.insert(session_name.to_string(), suffix.to_string());
                    true
                }
            },
            // 锁中毒（持锁线程 panic）时跳过清理，不能影响正常写入
            Err(_) => false,
        };
        if rolled_over {
            self.cleanup_old_logs(session_name);
        }
    }

    /// 写入一行日志（带分类标签）
    pub fn log(&self, session_name: &str, line: &str) {
        self.log_cat(session_name, LogCategory::Output, line);
    }

    /// 写入分类日志
    pub fn log_cat(&self, session_name: &str, category: LogCategory, line: &str) {
        let suffix = Self::timestamp_suffix();
        let path = self.log_path(session_name, &suffix);
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            let timestamp = Local::now().format("%H:%M:%S%.3f");
            let _ = writeln!(
                file,
                "[{}] [{}] {}",
                timestamp,
                category.tag(),
                line.trim_end()
            );
        }
        // 跨入新小时时才清理旧文件（而非每条日志都扫描目录）
        self.cleanup_if_rolled_over(session_name, &suffix);
    }

    /// 记录脚本发送的指令
    pub fn log_command(&self, session_name: &str, cmd: &str) {
        self.log_cat(session_name, LogCategory::Command, cmd);
    }

    /// 记录 /lua 指令
    pub fn log_lua(&self, session_name: &str, code: &str) {
        self.log_cat(session_name, LogCategory::Lua, code);
    }

    /// 记录调试信息
    pub fn log_debug(&self, session_name: &str, msg: &str) {
        self.log_cat(session_name, LogCategory::Debug, msg);
    }

    /// 记录断线事件
    pub fn log_disconnect(&self, session_name: &str, reason: &str, retry_secs: u64) {
        self.log_cat(
            session_name,
            LogCategory::Disconnect,
            &format!("reason={}, retry_in={}s", reason, retry_secs),
        );
    }

    /// 记录重连成功事件
    pub fn log_reconnect(&self, session_name: &str, downtime_secs: u64) {
        self.log_cat(
            session_name,
            LogCategory::Reconnect,
            &format!("reconnected, downtime={}s", downtime_secs),
        );
    }

    /// 记录严重错误信息（附带详情行，通常是 backtrace）
    ///
    /// 第三参数是「详情」槽位而非严格意义的 backtrace：真实 panic 传栈回溯，
    /// 看门狗超时等非 panic 事件传说明文字（那种场景栈不可采样）。
    /// 不执行 cleanup_old_logs，避免在 panic hook 中触发新的 panic
    pub fn log_panic(&self, session_name: &str, panic_msg: &str, detail: &str) {
        let path = self.log_path(session_name, &Self::timestamp_suffix());
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            let timestamp = Local::now().format("%H:%M:%S%.3f");
            let tag = LogCategory::Panic.tag();
            let _ = writeln!(
                file,
                "[{}] [{}] Rust panic: {}",
                timestamp,
                tag,
                panic_msg.trim_end()
            );
            for line in detail.lines() {
                let _ = writeln!(file, "[{}] [{}] {}", timestamp, tag, line);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn ts() -> String {
        Local::now().format("%y%m%d_%H").to_string()
    }

    #[test]
    fn test_logger_creates_directory() {
        let dir = TempDir::new().unwrap();
        let log_subdir = dir.path().join("test_logs");
        let _logger = Logger::new(log_subdir.to_str().unwrap(), 5);
        assert!(log_subdir.exists());
    }

    #[test]
    fn test_logger_writes_line() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 5);
        logger.log("session1", "hello world");

        let log_file = dir.path().join(format!("session1_{}.log", ts()));
        assert!(log_file.exists());

        let content = fs::read_to_string(&log_file).unwrap();
        assert!(content.contains("hello world"));
    }

    #[test]
    fn test_logger_appends() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 5);
        logger.log("sess", "line1");
        logger.log("sess", "line2");

        let log_file = dir.path().join(format!("sess_{}.log", ts()));
        let content = fs::read_to_string(&log_file).unwrap();
        assert!(content.contains("line1"));
        assert!(content.contains("line2"));
    }

    #[test]
    fn test_logger_trims_trailing_whitespace() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 5);
        logger.log("sess", "hello   ");

        let log_file = dir.path().join(format!("sess_{}.log", ts()));
        let content = fs::read_to_string(&log_file).unwrap();
        // 行尾空白被trim，但换行符由writeln添加
        assert!(content.contains("hello\n"));
        assert!(!content.contains("hello   "));
    }

    #[test]
    fn test_logger_timestamp_format() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 5);
        logger.log("sess", "test");

        let log_file = dir.path().join(format!("sess_{}.log", ts()));
        let content = fs::read_to_string(&log_file).unwrap();
        // 时间戳格式 [HH:MM:SS.mmm]
        let re = regex::Regex::new(r"\[\d{2}:\d{2}:\d{2}\.\d{3}\]").unwrap();
        assert!(re.is_match(&content));
    }

    #[test]
    fn test_logger_different_sessions() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 5);
        logger.log("session_a", "msg_a");
        logger.log("session_b", "msg_b");

        let file_a = dir.path().join(format!("session_a_{}.log", ts()));
        let file_b = dir.path().join(format!("session_b_{}.log", ts()));

        assert!(file_a.exists());
        assert!(file_b.exists());
        assert!(fs::read_to_string(&file_a).unwrap().contains("msg_a"));
        assert!(fs::read_to_string(&file_b).unwrap().contains("msg_b"));
    }

    #[test]
    fn test_logger_empty_message() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 5);
        logger.log("session", "");
        let file = dir.path().join(format!("session_{}.log", ts()));
        assert!(file.exists());
    }

    #[test]
    fn test_logger_unicode_message() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 5);
        logger.log("session", "你好世界 🌍");
        let file = dir.path().join(format!("session_{}.log", ts()));
        let content = fs::read_to_string(&file).unwrap();
        assert!(content.contains("你好世界 🌍"));
    }

    #[test]
    fn test_logger_long_session_name() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 5);
        let long_name = "a".repeat(200);
        logger.log(&long_name, "msg");
        let file = dir
            .path()
            .join(format!("a{}_{}.log", "a".repeat(199), ts()));
        assert!(file.exists());
    }

    #[test]
    fn test_logger_cleanup_old_files() {
        let dir = TempDir::new().unwrap();
        // max_files = 3，保留最近 3 个
        let logger = Logger::new(dir.path().to_str().unwrap(), 3);

        // 创建 5 个旧文件模拟不同时间（过去日期 + 不同小时，确保按文件名排序正确）
        let names = [
            "sess_250625_10.log",
            "sess_250625_11.log",
            "sess_250625_12.log",
            "sess_250626_13.log",
            "sess_250626_14.log",
        ];
        for name in &names {
            let path = dir.path().join(name);
            fs::write(&path, "dummy").unwrap();
        }

        // 写入当前小时，触发 cleanup
        logger.log("sess", "current");

        // 只应保留 3 个文件，最早的两个（250625_10, 250625_11）应被删除
        let remaining: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("sess_"))
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();

        assert_eq!(remaining.len(), 3);
        // 直接验证最早的两个文件已被删除（不依赖当前时间，避免 flaky）
        assert!(!dir.path().join("sess_250625_10.log").exists());
        assert!(!dir.path().join("sess_250625_11.log").exists());
    }

    #[test]
    fn test_logger_cleanup_different_sessions_isolated() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 1);

        // session_a 有 3 个旧文件（不同时间）
        let a_names = [
            "session_a_250624_10.log",
            "session_a_250625_11.log",
            "session_a_250626_12.log",
        ];
        for name in &a_names {
            let path = dir.path().join(name);
            fs::write(&path, "dummy").unwrap();
        }
        // session_b 有 2 个旧文件
        let b_names = ["session_b_250625_10.log", "session_b_250626_11.log"];
        for name in &b_names {
            let path = dir.path().join(name);
            fs::write(&path, "dummy").unwrap();
        }

        // 写 session_a 触发其 cleanup
        logger.log("session_a", "current");

        // session_a 只保留 1 个（当前时间）
        let a_count = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("session_a_"))
            .count();
        assert_eq!(a_count, 1);

        // session_b 不受影响，仍有 2 个
        let b_count = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("session_b_"))
            .count();
        assert_eq!(b_count, 2);
    }

    #[test]
    fn test_logger_cleanup_when_zero_files() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 5);
        // 首次写入，没有旧文件，不应报错
        logger.log("sess", "first line");
        let log_file = dir.path().join(format!("sess_{}.log", ts()));
        assert!(log_file.exists());
    }

    #[test]
    fn test_log_disconnect_writes_dcn_tag() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 5);
        logger.log_disconnect("mud", "heartbeat_timeout", 60);

        let log_file = dir.path().join(format!("mud_{}.log", ts()));
        let content = fs::read_to_string(&log_file).unwrap();
        assert!(content.contains("[DCN]"));
        assert!(content.contains("reason=heartbeat_timeout"));
        assert!(content.contains("retry_in=60s"));
    }

    #[test]
    fn test_log_reconnect_writes_rcn_tag() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 5);
        logger.log_reconnect("mud", 120);

        let log_file = dir.path().join(format!("mud_{}.log", ts()));
        let content = fs::read_to_string(&log_file).unwrap();
        assert!(content.contains("[RCN]"));
        assert!(content.contains("reconnected"));
        assert!(content.contains("downtime=120s"));
    }

    #[test]
    fn test_is_session_log_matches_exact_prefix_only() {
        assert!(Logger::is_session_log("mud_250626_14.log", "mud"));
        // 相似前缀不得误配（前缀后必须紧跟 `_`）
        assert!(!Logger::is_session_log("mud10_250626_14.log", "mud"));
        assert!(!Logger::is_session_log("mud250626_14.log", "mud"));
        // 名字出现在中段不算本 session 的日志
        assert!(!Logger::is_session_log("xmud_250626_14.log", "mud"));
        // 非 .log 后缀不算
        assert!(!Logger::is_session_log("mud_250626_14.log.txt", "mud"));
        assert!(!Logger::is_session_log("mud_", "mud"));
        // 会话名本身含分隔符（另一会话 `mud_alt`）不得被 `mud` 误配
        assert!(!Logger::is_session_log("mud_alt_250626_14.log", "mud"));
        // 时间戳段格式非法（非 6 位数字 + '_' + 2 位数字）不得匹配
        assert!(!Logger::is_session_log("mud_x250626_14.log", "mud"));
        assert!(!Logger::is_session_log("mud_250626_1.log", "mud"));
        assert!(!Logger::is_session_log("mud_25062614.log", "mud"));
    }

    #[test]
    fn test_cleanup_runs_once_per_hour_not_per_line() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 1);

        // 首条日志建立该 session 的清理记录
        logger.log("sess", "first");
        // 之后人为塞入一个更旧的文件，模拟目录里出现的过期日志
        let stale = dir.path().join("sess_250101_00.log");
        fs::write(&stale, "dummy").unwrap();

        // 同一小时内的后续写入不应再扫描目录，过期文件保持不变
        logger.log("sess", "second");
        assert!(stale.exists(), "同一小时内的写入不应重复触发目录扫描与清理");

        // 新实例（等价于进程重启后的首次写入）应执行一次清理
        let fresh = Logger::new(dir.path().to_str().unwrap(), 1);
        fresh.log("sess", "third");
        assert!(!stale.exists(), "新实例的首条日志应触发一次清理");
    }

    #[test]
    fn test_cleanup_scope_limited_to_exact_session_prefix() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 1);

        let other_session = dir.path().join("sess10_250101_00.log");
        let not_log = dir.path().join("sess_250101_00.txt");
        fs::write(&other_session, "dummy").unwrap();
        fs::write(&not_log, "dummy").unwrap();

        logger.log("sess", "line");

        assert!(other_session.exists(), "`sess` 不应波及 `sess10_...`");
        assert!(not_log.exists(), "非 .log 文件不应被清理");
    }

    #[test]
    fn test_cleanup_does_not_touch_prefixed_session() {
        // 回归护栏：会话 `mud_alt` 的名字以 `mud_` 开头，`mud` 的清理不得误删其日志
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 1);

        let other_session = dir.path().join("mud_alt_250101_00.log");
        fs::write(&other_session, "dummy").unwrap();

        // 写 `mud` 触发其清理（max_files=1）
        logger.log("mud", "line");

        assert!(
            other_session.exists(),
            "`mud` 的清理不应波及前缀型会话 `mud_alt` 的日志"
        );
    }
}
