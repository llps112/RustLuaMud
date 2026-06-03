use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct GeneralConfig {
    #[serde(default = "default_scroll_buffer")]
    pub scroll_buffer: usize,
    #[serde(default = "default_log_dir")]
    pub log_dir: String,
    #[serde(default = "default_log_rotation_size_mb")]
    pub log_rotation_size_mb: u64,
    #[serde(default = "default_log_rotation_count")]
    pub log_rotation_count: usize,
}

fn default_scroll_buffer() -> usize { 5000 }
fn default_log_dir() -> String { "logs".to_string() }
fn default_log_rotation_size_mb() -> u64 { 10 }
fn default_log_rotation_count() -> usize { 5 }

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            scroll_buffer: default_scroll_buffer(),
            log_dir: default_log_dir(),
            log_rotation_size_mb: default_log_rotation_size_mb(),
            log_rotation_count: default_log_rotation_count(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct ConnectionConfig {
    pub name: String,
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub encoding: Option<String>,
    #[serde(default)]
    pub script: Option<String>,
    #[serde(default = "default_true")]
    pub auto_connect: bool,
    #[serde(default = "default_true")]
    pub auto_reconnect: bool,
    #[serde(default = "default_reconnect_delay")]
    pub reconnect_delay_secs: u64,
}

fn default_true() -> bool { true }
fn default_reconnect_delay() -> u64 { 5 }

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub connections: Vec<ConnectionConfig>,
}

impl AppConfig {
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path)?;
        let config: AppConfig = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn load_default() -> Self {
        // 依次尝试：当前目录 → 可执行文件所在目录
        let candidates = vec![
            Path::new("configs/default.toml").to_path_buf(),
            std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|p| p.join("configs/default.toml")))
                .unwrap_or_default(),
        ];

        for path in &candidates {
            if path.exists() {
                match Self::load(path) {
                    Ok(config) => return config,
                    Err(e) => {
                        eprintln!("警告: 加载配置文件 {} 失败 ({})，使用默认配置", path.display(), e);
                        return Self::default();
                    }
                }
            }
        }

        eprintln!("警告: 未找到配置文件 (已搜索: {})，使用默认配置",
            candidates.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", "));
        Self::default()
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            connections: Vec::new(),
        }
    }
}
