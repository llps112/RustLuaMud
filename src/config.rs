use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct GeneralConfig {
    #[serde(default = "default_scroll_buffer")]
    pub scroll_buffer: usize,
    #[serde(default = "default_log_dir")]
    pub log_dir: String,
    #[serde(default = "default_profile_dir")]
    pub profile_dir: String,
    #[serde(default = "default_log_rotation_size_mb")]
    pub log_rotation_size_mb: u64,
    #[serde(default = "default_log_rotation_count")]
    pub log_rotation_count: usize,
}

fn default_scroll_buffer() -> usize { 5000 }
fn default_log_dir() -> String { "logs".to_string() }
fn default_profile_dir() -> String { "profiles".to_string() }
fn default_log_rotation_size_mb() -> u64 { 10 }
fn default_log_rotation_count() -> usize { 5 }

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            scroll_buffer: default_scroll_buffer(),
            log_dir: default_log_dir(),
            profile_dir: default_profile_dir(),
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
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
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
    pub fn load_default() -> Self {
        // 从 profiles 目录加载所有角色配置作为默认连接
        let (profiles, skipped) = Self::load_profiles("profiles");
        if !profiles.is_empty() {
            if skipped > 0 {
                eprintln!("警告: {} 个角色配置加载失败", skipped);
            }
            return Self {
                general: GeneralConfig::default(),
                connections: profiles,
            };
        }

        eprintln!("警告: profiles 目录未找到角色配置，使用默认配置");
        Self::default()
    }

    /// 从 profile 目录加载所有角色配置
    /// 返回 (profiles, skipped_count)
    pub fn load_profiles(profile_dir: &str) -> (Vec<ConnectionConfig>, usize) {
        let dir = Path::new(profile_dir);
        if !dir.exists() {
            return (Vec::new(), 0);
        }

        let mut profiles = Vec::new();
        let mut skipped = 0;

        // 读取目录中的 .toml 文件，按文件名排序保证加载顺序稳定
        let mut entries: Vec<_> = match fs::read_dir(dir) {
            Ok(rd) => rd.filter_map(|e| e.ok()).collect(),
            Err(_) => return (Vec::new(), 0),
        };
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let path = entry.path();
            // 跳过示例配置文件
            if path.file_stem().and_then(|s| s.to_str()) == Some("example") {
                continue;
            }
            if path.extension().and_then(|s| s.to_str()) != Some("toml") {
                continue;
            }

            match fs::read_to_string(&path) {
                Ok(content) => {
                    match toml::from_str::<ConnectionConfig>(&content) {
                        Ok(config) => {
                            eprintln!("已加载角色配置: {} ({})", config.name, path.display());
                            profiles.push(config);
                        }
                        Err(e) => {
                            eprintln!("警告: 角色配置 {} 格式错误: {}", path.display(), e);
                            skipped += 1;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("警告: 无法读取 {}: {}", path.display(), e);
                    skipped += 1;
                }
            }
        }

        (profiles, skipped)
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
