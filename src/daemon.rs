// 守护进程模式：fork/setsid/PTY 伪终端/PID 文件管理
//
// 设计要点：
// - daemonize 在 tokio runtime 创建之前调用（单线程阶段 fork 是安全的）
// - 用 openpty 创建伪终端接管 stdio，使终端渲染 API（raw mode/ANSI 输出）
//   在 headless 环境下仍正常工作，App/Terminal 代码无需感知 daemon 模式
// - drain 线程持续读空 pty master，防止输出缓冲区写满阻塞渲染
// - 不 chdir("/")：本程序依赖相对路径（profiles/ scripts/ logs/）
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// PID 文件路径（位于 profiles 目录下，随 --profiles 参数变化）
pub fn pid_file_path(profiles_dir: &str) -> PathBuf {
    Path::new(profiles_dir).join("daemon.pid")
}

/// 写入 PID 文件
pub fn write_pid_file(path: &Path, pid: u32) -> io::Result<()> {
    fs::write(path, pid.to_string())
}

/// 读取 PID 文件；文件不存在或内容非法时返回 None
pub fn read_pid_file(path: &Path) -> io::Result<Option<u32>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path)?;
    Ok(content.trim().parse::<u32>().ok())
}

/// 检查进程是否存活（sig=0 仅探测，不实际发送信号）
pub fn is_process_alive(pid: u32) -> bool {
    let pid_i = pid as i32;
    // 防御 u32 → i32 转换出负值（kill(-1, ...) 会广播到所有进程）
    if pid_i <= 0 {
        return false;
    }
    let rc = unsafe { libc::kill(pid_i, 0) };
    if rc == 0 {
        return true;
    }
    match io::Error::last_os_error().raw_os_error() {
        Some(libc::ESRCH) => false, // 进程不存在
        Some(libc::EPERM) => true,  // 进程存在但无权限发信号
        _ => false,
    }
}

/// 守护进程化：fork + setsid + PTY 接管 stdio
///
/// 父进程写入 PID 文件后打印提示并退出；子进程返回 Ok(()) 继续主流程。
pub fn daemonize(pid_path: &Path) -> io::Result<()> {
    // 检查是否已有守护进程在运行
    if let Some(pid) = read_pid_file(pid_path)? {
        if is_process_alive(pid) {
            return Err(io::Error::other(format!(
                "守护进程已在运行 (pid={})，请先执行 --daemon stop",
                pid
            )));
        }
        // 清理过期的 PID 文件
        let _ = fs::remove_file(pid_path);
    }

    // 创建 PTY 伪终端，使终端渲染 API 在 headless 环境下正常工作
    let mut master: libc::c_int = 0;
    let mut slave: libc::c_int = 0;
    let rc = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }

    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(io::Error::last_os_error());
    }
    if pid > 0 {
        // 父进程：写 PID 文件、打印提示后退出
        unsafe {
            libc::close(master);
            libc::close(slave);
        }
        write_pid_file(pid_path, pid as u32)?;
        println!(
            "守护进程已启动 (pid={})，PID 文件: {}",
            pid,
            pid_path.display()
        );
        println!("停止命令: RustLuaMud --daemon stop");
        std::process::exit(0);
    }

    // 子进程：脱离控制终端，ssh 断开不再收到 SIGHUP
    if unsafe { libc::setsid() } < 0 {
        return Err(io::Error::last_os_error());
    }
    // 设置 pty 窗口尺寸为 80x24：新建 pty 的 winsize 为 0x0，
    // 不设置会导致 terminal::size() 返回 (0,0)，渲染代码减法溢出 panic
    let mut ws = libc::winsize {
        ws_row: 24,
        ws_col: 80,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    unsafe {
        libc::ioctl(slave, libc::TIOCSWINSZ, &mut ws);
    }
    // 用 pty slave 替换 stdio（终端 API 需要真实 tty 文件描述符）
    for fd in 0..3 {
        unsafe {
            libc::dup2(slave, fd);
        }
    }
    unsafe {
        libc::close(slave);
    }
    // drain 线程：持续读空 pty master，防止输出缓冲区写满阻塞渲染
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            let n = unsafe { libc::read(master, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
            if n <= 0 {
                break;
            }
        }
        unsafe {
            libc::close(master);
        }
    });
    Ok(())
}

/// 停止守护进程：发送 SIGTERM 并等待退出（最长 10 秒）
pub fn stop_daemon(pid_path: &Path) -> io::Result<String> {
    let pid = match read_pid_file(pid_path)? {
        Some(p) if is_process_alive(p) => p,
        Some(_) => {
            let _ = fs::remove_file(pid_path);
            return Ok("守护进程未在运行（已清理过期 PID 文件）".to_string());
        }
        None => return Ok("守护进程未在运行（PID 文件不存在）".to_string()),
    };
    unsafe {
        libc::kill(pid as i32, libc::SIGTERM);
    }
    for _ in 0..100 {
        if !is_process_alive(pid) {
            let _ = fs::remove_file(pid_path);
            return Ok(format!("守护进程已停止 (pid={})", pid));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err(io::Error::other(format!(
        "守护进程 10 秒内未退出 (pid={})，可尝试 kill -9 {}",
        pid, pid
    )))
}

/// 查询守护进程状态
pub fn status_daemon(pid_path: &Path) -> String {
    match read_pid_file(pid_path) {
        Ok(Some(pid)) if is_process_alive(pid) => format!("守护进程运行中 (pid={})", pid),
        Ok(Some(_)) => "守护进程未在运行（存在过期 PID 文件）".to_string(),
        _ => "守护进程未在运行".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pid_file_path() {
        assert_eq!(
            pid_file_path("profiles"),
            Path::new("profiles").join("daemon.pid")
        );
        assert_eq!(
            pid_file_path("profiles2"),
            Path::new("profiles2").join("daemon.pid")
        );
    }

    #[test]
    fn test_pid_file_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.pid");
        write_pid_file(&path, 12345).unwrap();
        assert_eq!(read_pid_file(&path).unwrap(), Some(12345));
    }

    #[test]
    fn test_read_pid_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no-such-file.pid");
        assert_eq!(read_pid_file(&path).unwrap(), None);
    }

    #[test]
    fn test_read_pid_file_invalid_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.pid");
        fs::write(&path, "not-a-pid").unwrap();
        assert_eq!(read_pid_file(&path).unwrap(), None);
        // 带空白/换行的合法数字仍可解析
        fs::write(&path, " 4242\n").unwrap();
        assert_eq!(read_pid_file(&path).unwrap(), Some(4242));
    }

    #[test]
    fn test_is_process_alive_self() {
        assert!(is_process_alive(std::process::id()));
    }

    #[test]
    fn test_is_process_alive_nonexistent() {
        // 远超系统 pid_max（通常 <= 4194304），必然不存在
        assert!(!is_process_alive(2_000_000_000));
    }

    #[test]
    fn test_is_process_alive_invalid_zero() {
        // 0 会转成 kill(0, ...) 发给整个进程组，必须拒绝
        assert!(!is_process_alive(0));
    }

    #[test]
    fn test_stop_daemon_no_pid_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.pid");
        let msg = stop_daemon(&path).unwrap();
        assert!(msg.contains("未在运行"));
    }

    #[test]
    fn test_stop_daemon_stale_pid_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.pid");
        write_pid_file(&path, 2_000_000_000).unwrap();
        let msg = stop_daemon(&path).unwrap();
        assert!(msg.contains("未在运行"));
        // 过期 PID 文件应被清理
        assert!(!path.exists());
    }

    #[test]
    fn test_status_daemon_not_running() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.pid");
        assert_eq!(status_daemon(&path), "守护进程未在运行");
    }

    #[test]
    fn test_status_daemon_stale_pid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.pid");
        write_pid_file(&path, 2_000_000_000).unwrap();
        let msg = status_daemon(&path);
        assert!(msg.contains("过期 PID 文件"));
    }

    #[test]
    fn test_status_daemon_running() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.pid");
        let self_pid = std::process::id();
        write_pid_file(&path, self_pid).unwrap();
        let msg = status_daemon(&path);
        assert!(msg.contains("运行中"));
        assert!(msg.contains(&self_pid.to_string()));
    }

    #[test]
    fn test_daemonize_refuses_when_running() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.pid");
        // 写入当前进程 pid 模拟"已在运行"
        write_pid_file(&path, std::process::id()).unwrap();
        let err = daemonize(&path).unwrap_err();
        assert!(err.to_string().contains("已在运行"));
    }
}
