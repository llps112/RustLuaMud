mod app;
mod config;
mod connection;
mod log;
mod lua;
mod ui;

use app::App;
use config::AppConfig;

fn main() {
    // 设置环境变量以便任何逃逸的 panic 能打印堆栈信息
    std::env::set_var("RUST_BACKTRACE", "1");

    let config = AppConfig::load_default();

    let rt = tokio::runtime::Runtime::new().expect("无法创建 tokio runtime");

    rt.block_on(async {
        let mut app = match App::new(config) {
            Ok(app) => app,
            Err(e) => {
                eprintln!("初始化失败: {}", e);
                return;
            }
        };

        if let Err(e) = app.run().await {
            eprintln!("运行错误: {}", e);
        }
    });
}
