use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

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

/// 简单日志记录器（Phase 1 基础版，Phase 4 完善轮转）
pub struct Logger {
    log_dir: PathBuf,
    #[allow(dead_code)]
    max_size_mb: u64,
    #[allow(dead_code)]
    max_files: usize,
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
        }
    }

    /// 获取连接对应的日志文件路径
    fn log_path(&self, session_name: &str) -> PathBuf {
        let date = Local::now().format("%Y%m%d");
        let filename = format!("{}_{}.log", session_name, date);
        self.log_dir.join(filename)
    }

    /// 写入一行日志（带分类标签）
    pub fn log(&self, session_name: &str, line: &str) {
        self.log_cat(session_name, LogCategory::Output, line);
    }

    /// 写入分类日志
    pub fn log_cat(&self, session_name: &str, category: LogCategory, line: &str) {
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.log_path(session_name))
        {
            let timestamp = Local::now().format("%H:%M:%S%.3f");
            let _ = writeln!(file, "[{}] [{}] {}", timestamp, category.tag(), line.trim_end());
        }
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

        let date = Local::now().format("%Y%m%d");
        let log_file = dir.path().join(format!("session1_{}.log", date));
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

        let date = Local::now().format("%Y%m%d");
        let log_file = dir.path().join(format!("sess_{}.log", date));
        let content = fs::read_to_string(&log_file).unwrap();
        assert!(content.contains("line1"));
        assert!(content.contains("line2"));
    }

    #[test]
    fn test_logger_trims_trailing_whitespace() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 10, 5);
        logger.log("sess", "hello   ");

        let date = Local::now().format("%Y%m%d");
        let log_file = dir.path().join(format!("sess_{}.log", date));
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

        let date = Local::now().format("%Y%m%d");
        let log_file = dir.path().join(format!("sess_{}.log", date));
        let content = fs::read_to_string(&log_file).unwrap();
        // 时间戳格式 [HH:MM:SS]
        let re = regex::Regex::new(r"\[\d{2}:\d{2}:\d{2}\]").unwrap();
        assert!(re.is_match(&content));
    }

    #[test]
    fn test_logger_different_sessions() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 10, 5);
        logger.log("session_a", "msg_a");
        logger.log("session_b", "msg_b");

        let date = Local::now().format("%Y%m%d");
        let file_a = dir.path().join(format!("session_a_{}.log", date));
        let file_b = dir.path().join(format!("session_b_{}.log", date));

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
        let date = Local::now().format("%Y%m%d");
        let file = dir.path().join(format!("session_{}.log", date));
        assert!(file.exists());
    }

    #[test]
    fn test_logger_unicode_message() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 10, 5);
        logger.log("session", "你好世界 🌍");
        let date = Local::now().format("%Y%m%d");
        let file = dir.path().join(format!("session_{}.log", date));
        let content = fs::read_to_string(&file).unwrap();
        assert!(content.contains("你好世界 🌍"));
    }

    #[test]
    fn test_logger_long_session_name() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 10, 5);
        let long_name = "a".repeat(200);
        logger.log(&long_name, "msg");
        let date = Local::now().format("%Y%m%d");
        let file = dir
            .path()
            .join(format!("a{}_{}.log", "a".repeat(199), date));
        assert!(file.exists());
    }
}
