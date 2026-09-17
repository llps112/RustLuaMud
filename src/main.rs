use std::ffi::OsStr;

use rust_lua_mud::app::App;
use rust_lua_mud::config::AppConfig;
#[cfg(unix)]
use rust_lua_mud::daemon;

/// 计算 RUST_BACKTRACE 的默认值：仅当用户未设置时返回 `Some("1")`。
///
/// `panic_hook` 依赖 `Backtrace::capture()` 记录崩溃栈，而该 API 在环境变量
/// 未设置时只返回占位文本。7×24 无人值守场景下崩溃时人不在现场，默认带栈
/// 是排障所必需的；但控制权应归用户——显式设置 `0`（关闭）或 `full`
/// （完整栈）时本函数返回 None，不做任何干预。
fn default_rust_backtrace(current: Option<&OsStr>) -> Option<&'static str> {
    current.is_none().then_some("1")
}

fn main() {
    // 用户未设置 RUST_BACKTRACE 时补默认值（用户显式设置则完全尊重）
    if let Some(v) = default_rust_backtrace(std::env::var_os("RUST_BACKTRACE").as_deref()) {
        std::env::set_var("RUST_BACKTRACE", v);
    }

    // 解析命令行参数
    let args: Vec<String> = std::env::args().collect();

    // --version: 打印版本号并退出（不启动客户端）
    if args.iter().any(|a| a == "--version") {
        println!("RustLuaMud {}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }

    // 解析 --profiles 参数
    let profiles_dir = args
        .windows(2)
        .find(|w| w[0] == "--profiles")
        .map(|w| w[1].as_str())
        .unwrap_or("profiles")
        .to_string();

    // --daemon 参数：仅 Unix 平台支持
    #[cfg(unix)]
    let daemon_mode = {
        let dm = args.iter().any(|a| a == "--daemon");
        if dm {
            let pid_path = daemon::pid_file_path(&profiles_dir);
            let sub = args
                .windows(2)
                .find(|w| w[0] == "--daemon")
                .map(|w| w[1].as_str())
                .unwrap_or("");
            match sub {
                "stop" => match daemon::stop_daemon(&pid_path) {
                    Ok(msg) => {
                        println!("{}", msg);
                        return;
                    }
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                },
                "status" => {
                    println!("{}", daemon::status_daemon(&pid_path));
                    return;
                }
                _ => {
                    // 守护进程化：父进程打印提示后退出，子进程继续主流程
                    // （必须在创建 tokio runtime 之前调用，fork 只在单线程阶段安全）
                    if let Err(e) = daemon::daemonize(&pid_path) {
                        eprintln!("守护进程化失败: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        }
        dm
    };
    #[cfg(unix)]
    let pid_path = daemon::pid_file_path(&profiles_dir);

    #[cfg(not(unix))]
    if args.iter().any(|a| a == "--daemon") {
        eprintln!("当前平台不支持守护进程模式（--daemon 仅支持 Unix 平台）");
        std::process::exit(1);
    }

    let config = AppConfig::load_default(&profiles_dir);

    // 启动期可写自检：日志目录与 profiles 目录必须可写，
    // 否则在启动瞬间报出真实原因（权限/路径/磁盘）并退出，
    // 避免挂机到运行期才发现无法写日志或保存 terminal.json。
    if let Err(e) =
        rust_lua_mud::config::verify_writable_dir(std::path::Path::new(&config.general.log_dir))
    {
        eprintln!("启动失败 - 日志目录检查未通过: {}", e);
        std::process::exit(1);
    }
    if let Err(e) = rust_lua_mud::config::verify_writable_dir(std::path::Path::new(&profiles_dir)) {
        eprintln!("启动失败 - 配置目录检查未通过: {}", e);
        std::process::exit(1);
    }

    // 初始化 panic hook，将 panic 信息和 backtrace 写入日志文件
    rust_lua_mud::log::panic_hook::init_panic_hook(
        &config.general.log_dir,
        config.general.log_rotation_size_mb,
        config.general.log_rotation_count,
    );

    let rt = tokio::runtime::Runtime::new().expect("无法创建 tokio runtime");

    rt.block_on(async {
        let mut app = match App::new(config) {
            Ok(app) => app,
            Err(e) => {
                eprintln!("初始化失败: {}", e);
                return;
            }
        };
        #[cfg(unix)]
        app.set_daemon_mode(daemon_mode);

        if let Err(e) = app.run().await {
            eprintln!("运行错误: {}", e);
        }
    });

    // daemon 模式正常退出后清理 PID 文件
    #[cfg(unix)]
    if daemon_mode {
        let _ = std::fs::remove_file(&pid_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_rust_backtrace_respects_user_setting() {
        // 纯函数测试，不读写真实环境变量，故无并发/顺序依赖。
        // 用户已设置时不干预——显式关闭（"0"）与完整栈（"full"）都必须尊重
        assert_eq!(default_rust_backtrace(Some(OsStr::new("0"))), None);
        assert_eq!(default_rust_backtrace(Some(OsStr::new("full"))), None);
        assert_eq!(default_rust_backtrace(Some(OsStr::new("1"))), None);
        // 未设置时补默认 "1"：无人值守场景崩溃时日志需带栈可查
        assert_eq!(default_rust_backtrace(None), Some("1"));
    }
}
