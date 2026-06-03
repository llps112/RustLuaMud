use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use chrono::Local;

/// 简单日志记录器（Phase 1 基础版，Phase 4 完善轮转）
pub struct Logger {
    log_dir: PathBuf,
    max_size_mb: u64,
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

    /// 写入一行日志
    pub fn log(&self, session_name: &str, line: &str) {
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.log_path(session_name))
        {
            let timestamp = Local::now().format("%H:%M:%S");
            let _ = writeln!(file, "[{}] {}", timestamp, line.trim_end());
        }
    }
}
