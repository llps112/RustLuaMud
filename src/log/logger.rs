use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use chrono::Local;
use regex::Regex;

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
}

impl LogCategory {
    fn tag(&self) -> &'static str {
        match self {
            LogCategory::Output => "OUT",
            LogCategory::Command => "CMD",
            LogCategory::Lua => "LUA",
            LogCategory::Debug => "DBG",
        }
    }
}

/// 日志记录器，按小时分割，最多保留 max_files 个历史文件。
pub struct Logger {
    log_dir: PathBuf,
    #[allow(dead_code)]
    max_size_mb: u64,
    max_files: usize,
    /// 按 session 覆盖的保留数量（session_name -> count）
    per_session_max_files: Mutex<HashMap<String, usize>>,
}

impl Logger {
    pub fn new(log_dir: &str, max_size_mb: u64, max_files: usize) -> Self {
        let log_dir = PathBuf::from(log_dir);
        // 确保日志目录存在
        let _ = fs::create_dir_all(&log_dir);
        Self {
            log_dir,
            max_size_mb,
            max_files,
            per_session_max_files: Mutex::new(HashMap::new()),
        }
    }

    /// 设置指定 session 的日志保留数量，覆盖全局 max_files
    pub fn set_session_max_files(&self, session_name: &str, count: usize) {
        if let Ok(mut map) = self.per_session_max_files.lock() {
            map.insert(session_name.to_string(), count);
        }
    }

    /// 获取当前小时对应的日志文件路径
    /// 格式: `<session>_<HH>.log`，例如 `mud_14.log`，每小时滚动
    fn log_path(&self, session_name: &str) -> PathBuf {
        let hour = Local::now().format("%H");
        let filename = format!("{}_{}.log", session_name, hour);
        self.log_dir.join(filename)
    }

    /// 清理同 session 的旧日志文件，只保留最新的 max_files 个
    fn cleanup_old_logs(&self, session_name: &str) {
        let max_files = self
            .per_session_max_files
            .lock()
            .ok()
            .and_then(|map| map.get(session_name).copied())
            .unwrap_or(self.max_files);

        let pattern = format!("{}_.*\\.log", regex::escape(session_name));

        let re = match Regex::new(&pattern) {
            Ok(r) => r,
            Err(_) => return,
        };

        let mut entries: Vec<_> = match fs::read_dir(&self.log_dir) {
            Ok(rd) => rd
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                .filter(|e| re.is_match(&e.file_name().to_string_lossy()))
                .collect(),
            Err(_) => return,
        };

        // 按文件名（即时间）排序，最新的排前面
        entries.sort_by_key(|b| std::cmp::Reverse(b.file_name()));

        // 删除超出 max_files 的旧文件
        for entry in entries.iter().skip(max_files) {
            let _ = fs::remove_file(entry.path());
        }
    }

    /// 写入一行日志（带分类标签）
    pub fn log(&self, session_name: &str, line: &str) {
        self.log_cat(session_name, LogCategory::Output, line);
    }

    /// 写入分类日志
    pub fn log_cat(&self, session_name: &str, category: LogCategory, line: &str) {
        let path = self.log_path(session_name);
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
        // 文件写入成功后清理旧文件
        self.cleanup_old_logs(session_name);
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn hour() -> String {
        Local::now().format("%H").to_string()
    }

    #[test]
    fn test_logger_creates_directory() {
        let dir = TempDir::new().unwrap();
        let log_subdir = dir.path().join("test_logs");
        let _logger = Logger::new(log_subdir.to_str().unwrap(), 10, 5);
        assert!(log_subdir.exists());
    }

    #[test]
    fn test_logger_writes_line() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 10, 5);
        logger.log("session1", "hello world");

        let log_file = dir.path().join(format!("session1_{}.log", hour()));
        assert!(log_file.exists());

        let content = fs::read_to_string(&log_file).unwrap();
        assert!(content.contains("hello world"));
    }

    #[test]
    fn test_logger_appends() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 10, 5);
        logger.log("sess", "line1");
        logger.log("sess", "line2");

        let log_file = dir.path().join(format!("sess_{}.log", hour()));
        let content = fs::read_to_string(&log_file).unwrap();
        assert!(content.contains("line1"));
        assert!(content.contains("line2"));
    }

    #[test]
    fn test_logger_trims_trailing_whitespace() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 10, 5);
        logger.log("sess", "hello   ");

        let log_file = dir.path().join(format!("sess_{}.log", hour()));
        let content = fs::read_to_string(&log_file).unwrap();
        // 行尾空白被trim，但换行符由writeln添加
        assert!(content.contains("hello\n"));
        assert!(!content.contains("hello   "));
    }

    #[test]
    fn test_logger_timestamp_format() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 10, 5);
        logger.log("sess", "test");

        let log_file = dir.path().join(format!("sess_{}.log", hour()));
        let content = fs::read_to_string(&log_file).unwrap();
        // 时间戳格式 [HH:MM:SS.mmm]
        let re = regex::Regex::new(r"\[\d{2}:\d{2}:\d{2}\.\d{3}\]").unwrap();
        assert!(re.is_match(&content));
    }

    #[test]
    fn test_logger_different_sessions() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 10, 5);
        logger.log("session_a", "msg_a");
        logger.log("session_b", "msg_b");

        let file_a = dir.path().join(format!("session_a_{}.log", hour()));
        let file_b = dir.path().join(format!("session_b_{}.log", hour()));

        assert!(file_a.exists());
        assert!(file_b.exists());
        assert!(fs::read_to_string(&file_a).unwrap().contains("msg_a"));
        assert!(fs::read_to_string(&file_b).unwrap().contains("msg_b"));
    }

    #[test]
    fn test_logger_empty_message() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 10, 5);
        logger.log("session", "");
        let file = dir.path().join(format!("session_{}.log", hour()));
        assert!(file.exists());
    }

    #[test]
    fn test_logger_unicode_message() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 10, 5);
        logger.log("session", "你好世界 🌍");
        let file = dir.path().join(format!("session_{}.log", hour()));
        let content = fs::read_to_string(&file).unwrap();
        assert!(content.contains("你好世界 🌍"));
    }

    #[test]
    fn test_logger_long_session_name() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 10, 5);
        let long_name = "a".repeat(200);
        logger.log(&long_name, "msg");
        let file = dir
            .path()
            .join(format!("a{}_{}.log", "a".repeat(199), hour()));
        assert!(file.exists());
    }

    #[test]
    fn test_logger_cleanup_old_files() {
        let dir = TempDir::new().unwrap();
        // max_files = 3，保留最近 3 个
        let logger = Logger::new(dir.path().to_str().unwrap(), 10, 3);

        // 创建一些旧文件模拟不同小时
        let names = [
            "sess_10.log",
            "sess_11.log",
            "sess_12.log",
            "sess_13.log",
            "sess_14.log",
        ];
        for name in &names {
            let path = dir.path().join(name);
            fs::write(&path, "dummy").unwrap();
        }

        // 写入当前小时，触发 cleanup
        logger.log("sess", "current");

        // 只应保留最近 3 个（13, 14, 当前小时）
        let mut remaining: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("sess_"))
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        remaining.sort();

        assert_eq!(remaining.len(), 3);
        // 最早的两个（10, 11）已被删除
        assert!(remaining.iter().any(|n| n == "sess_13.log"));
        assert!(remaining.iter().any(|n| n == "sess_14.log"));
        // 当前小时的文件存在
        assert!(remaining.iter().any(|n| n.contains(&hour())));
    }

    #[test]
    fn test_logger_cleanup_different_sessions_isolated() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 10, 1);

        // session_a 有 3 个旧文件
        for h in 10..=12 {
            let path = dir.path().join(format!("session_a_{}.log", h));
            fs::write(&path, "dummy").unwrap();
        }
        // session_b 有 2 个旧文件
        for h in 10..=11 {
            let path = dir.path().join(format!("session_b_{}.log", h));
            fs::write(&path, "dummy").unwrap();
        }

        // 写 session_a 触发其 cleanup
        logger.log("session_a", "current");

        // session_a 只保留 1 个（当前小时）
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
        let logger = Logger::new(dir.path().to_str().unwrap(), 10, 5);
        // 首次写入，没有旧文件，不应报错
        logger.log("sess", "first line");
        let log_file = dir.path().join(format!("sess_{}.log", hour()));
        assert!(log_file.exists());
    }
}
