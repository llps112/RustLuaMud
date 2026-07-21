use std::sync::{Mutex, OnceLock};

use crate::log::logger::Logger;

/// 全局日志上下文（panic hook 可访问）
pub struct PanicContext {
    pub logger: Logger,
    pub session_name: Mutex<String>,
}

static PANIC_CONTEXT: OnceLock<PanicContext> = OnceLock::new();

/// 获取全局 PANIC_CONTEXT（供看门狗线程写入日志使用）
pub fn get_context() -> Option<&'static PanicContext> {
    PANIC_CONTEXT.get()
}

/// 初始化全局 panic 上下文（在 main 开头调用一次）
pub fn init_panic_hook(log_dir: &str, max_size_mb: u64, max_files: usize) {
    let ctx = PanicContext {
        logger: Logger::new(log_dir, max_size_mb, max_files),
        session_name: Mutex::new(String::new()),
    };
    let _ = PANIC_CONTEXT.set(ctx);

    std::panic::set_hook(Box::new(|info| {
        let panic_msg = format!("{}", info);
        let backtrace = std::backtrace::Backtrace::capture();
        let backtrace_str = format!("{}", backtrace);

        // 输出到 stderr（保持默认行为）
        let thread_name = std::thread::current()
            .name()
            .unwrap_or("<unnamed>")
            .to_string();
        let location = info.location().map(|l| l.to_string()).unwrap_or_default();
        eprintln!(
            "thread '{}' panicked at {}\n{}",
            thread_name, location, backtrace_str
        );

        // 写入日志文件
        if let Some(ctx) = PANIC_CONTEXT.get() {
            // 使用 try_lock 避免 deadlock（panic hook 中不能阻塞）
            if let Ok(guard) = ctx.session_name.try_lock() {
                let session = if guard.is_empty() {
                    "panic"
                } else {
                    guard.as_str()
                };
                ctx.logger.log_panic(session, &panic_msg, &backtrace_str);
            }
        }
    }));
}

/// 更新当前前台 session name（切换前台时调用）
pub fn set_current_session(name: &str) {
    if let Some(ctx) = PANIC_CONTEXT.get() {
        if let Ok(mut guard) = ctx.session_name.lock() {
            *guard = name.to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;
    use std::sync::Mutex;
    use tempfile::TempDir;

    /// 序列化涉及 panic hook 的测试，避免并行运行时 set_hook/take_hook 竞争
    static PANIC_HOOK_TEST_LOCK: Mutex<()> = Mutex::new(());

    /// 初始化全局 PANIC_CONTEXT（使用 TempDir，所有测试共用一个实例）
    /// OnceLock 只能设置一次，所以使用 OnceLock 内的共享 Logger
    fn ensure_context_initialized() -> &'static PanicContext {
        PANIC_CONTEXT.get_or_init(|| PanicContext {
            logger: Logger::new("/tmp/rustluamud_panic_hook_test", 10, 5),
            session_name: Mutex::new(String::new()),
        })
    }

    #[test]
    fn test_set_current_session_updates_context() {
        ensure_context_initialized();

        set_current_session("test_session");
        if let Some(ctx) = PANIC_CONTEXT.get() {
            let guard = ctx.session_name.lock().unwrap();
            assert_eq!(*guard, "test_session");
        }
    }

    #[test]
    fn test_set_current_session_empty_name() {
        ensure_context_initialized();

        set_current_session("");
        if let Some(ctx) = PANIC_CONTEXT.get() {
            let guard = ctx.session_name.lock().unwrap();
            assert_eq!(*guard, "");
        }
    }

    #[test]
    fn test_log_panic_writes_panic_msg_and_backtrace() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 10, 5);

        let panic_msg = "test panic message";
        let backtrace = "frame 0\nframe 1\nframe 2";
        logger.log_panic("panic_test_session", panic_msg, backtrace);

        let ts = Local::now().format("%y%m%d_%H").to_string();
        let log_file = dir.path().join(format!("panic_test_session_{}.log", ts));
        assert!(log_file.exists(), "日志文件应存在");

        let content = std::fs::read_to_string(&log_file).unwrap();
        assert!(
            content.contains("[PNC] Rust panic: test panic message"),
            "应包含 panic 消息"
        );
        assert!(content.contains("frame 0"), "应包含 backtrace 第 0 行");
        assert!(content.contains("frame 1"), "应包含 backtrace 第 1 行");
        assert!(content.contains("frame 2"), "应包含 backtrace 第 2 行");
    }

    #[test]
    fn test_log_panic_empty_backtrace() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 10, 5);

        logger.log_panic("empty_bt_session", "panic with no bt", "");

        let ts = Local::now().format("%y%m%d_%H").to_string();
        let log_file = dir.path().join(format!("empty_bt_session_{}.log", ts));
        assert!(log_file.exists());

        let content = std::fs::read_to_string(&log_file).unwrap();
        assert!(
            content.contains("[PNC] Rust panic: panic with no bt"),
            "应包含 panic 消息"
        );
    }

    #[test]
    fn test_log_panic_multiline_backtrace() {
        let dir = TempDir::new().unwrap();
        let logger = Logger::new(dir.path().to_str().unwrap(), 10, 5);

        let backtrace = "  0: fn_a\n  1: fn_b\n  2: fn_c\n  3: main";
        logger.log_panic("multi_bt_session", "multi-line bt test", backtrace);

        let ts = Local::now().format("%y%m%d_%H").to_string();
        let log_file = dir.path().join(format!("multi_bt_session_{}.log", ts));
        let content = std::fs::read_to_string(&log_file).unwrap();
        // 每行 backtrace 都应带 [PNC] 前缀
        assert_eq!(
            content.matches("[PNC]").count(),
            1 + backtrace.lines().count(),
            "panic 消息 1 行 + backtrace 每行都应带 [PNC] 前缀"
        );
    }

    /// 验证 init_panic_hook 能安全调用，不会 panic
    /// 注意：PANIC_CONTEXT 可能已被其他测试设置，set 会静默失败，但 panic hook 仍会被设置
    #[test]
    fn test_init_panic_hook_safe_to_call() {
        let _guard = PANIC_HOOK_TEST_LOCK.lock().unwrap();
        let dir = TempDir::new().unwrap();

        // 保存当前的 panic hook，测试后恢复
        let old_hook = std::panic::take_hook();

        // init_panic_hook 应该能安全调用
        init_panic_hook(dir.path().to_str().unwrap(), 10, 5);

        // 恢复原来的 panic hook，避免影响其他测试
        std::panic::set_hook(old_hook);

        // 如果执行到这里，说明 init_panic_hook 没有 panic
    }

    /// 验证 panic hook 触发时日志文件被正确写入
    /// 使用 ensure_context_initialized 确保 PANIC_CONTEXT 已初始化
    #[test]
    fn test_panic_hook_writes_log_on_panic() {
        let _guard = PANIC_HOOK_TEST_LOCK.lock().unwrap();
        ensure_context_initialized();
        set_current_session("hook_test");

        // 保存当前的 panic hook，测试后恢复
        let old_hook = std::panic::take_hook();

        // 重新设置 panic hook（PANIC_CONTEXT 已初始化，set 会静默失败，但 hook 会更新）
        init_panic_hook("/tmp/rustluamud_panic_hook_test", 10, 5);

        // 触发 panic（用 catch_unwind 捕获，防止测试进程崩溃）
        let result = std::panic::catch_unwind(|| {
            panic!("test panic for hook validation");
        });
        assert!(result.is_err(), "catch_unwind 应捕获到 panic");

        // 恢复原来的 panic hook
        std::panic::set_hook(old_hook);

        // 验证日志文件（PANIC_CONTEXT 的 Logger 使用 /tmp/rustluamud_panic_hook_test）
        let ts = Local::now().format("%y%m%d_%H").to_string();
        let log_file = std::path::Path::new("/tmp/rustluamud_panic_hook_test")
            .join(format!("hook_test_{}.log", ts));
        assert!(log_file.exists(), "panic 日志文件应存在");

        let content = std::fs::read_to_string(&log_file).unwrap();
        assert!(
            content.contains("test panic for hook validation"),
            "日志应包含 panic 消息"
        );
        assert!(content.contains("[PNC]"), "日志应包含 [PNC] 前缀");
    }
}
