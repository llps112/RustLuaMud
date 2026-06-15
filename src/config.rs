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

fn default_scroll_buffer() -> usize {
    5000
}
fn default_log_dir() -> String {
    "logs".to_string()
}
fn default_profile_dir() -> String {
    "profiles".to_string()
}
fn default_log_rotation_size_mb() -> u64 {
    10
}
fn default_log_rotation_count() -> usize {
    5
}

#[allow(clippy::derivable_impls)]
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

fn default_true() -> bool {
    true
}
fn default_reconnect_delay() -> u64 {
    5
}

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub connections: Vec<ConnectionConfig>,
}

impl AppConfig {
    pub fn load_default(profiles_dir: &str) -> Self {
        // 从 profiles 目录加载所有角色配置作为默认连接
        let (profiles, skipped) = Self::load_profiles(profiles_dir);
        if !profiles.is_empty() {
            if skipped > 0 {
                eprintln!("警告: {} 个角色配置加载失败", skipped);
            }
            return Self {
                general: GeneralConfig::default(),
                connections: profiles,
            };
        }

        eprintln!("警告: {} 目录未找到角色配置，使用默认配置", profiles_dir);
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
                Ok(content) => match toml::from_str::<ConnectionConfig>(&content) {
                    Ok(config) => {
                        eprintln!("已加载角色配置: {} ({})", config.name, path.display());
                        profiles.push(config);
                    }
                    Err(e) => {
                        eprintln!("警告: 角色配置 {} 格式错误: {}", path.display(), e);
                        skipped += 1;
                    }
                },
                Err(e) => {
                    eprintln!("警告: 无法读取 {}: {}", path.display(), e);
                    skipped += 1;
                }
            }
        }

        (profiles, skipped)
    }
}

#[allow(clippy::derivable_impls)]
impl Default for AppConfig {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            connections: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_general_config_defaults() {
        let config = GeneralConfig::default();
        assert_eq!(config.scroll_buffer, 5000);
        assert_eq!(config.log_dir, "logs");
        assert_eq!(config.profile_dir, "profiles");
        assert_eq!(config.log_rotation_size_mb, 10);
        assert_eq!(config.log_rotation_count, 5);
    }

    #[test]
    fn test_app_config_default() {
        let config = AppConfig::default();
        assert!(config.connections.is_empty());
        assert_eq!(config.general.scroll_buffer, 5000);
    }

    #[test]
    fn test_connection_config_deserialize() {
        let toml_str = r#"
            name = "test"
            host = "example.com"
            port = 4000
        "#;
        let config: ConnectionConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.name, "test");
        assert_eq!(config.host, "example.com");
        assert_eq!(config.port, 4000);
        assert!(config.auto_connect);
        assert!(config.auto_reconnect);
        assert_eq!(config.reconnect_delay_secs, 5);
    }

    #[test]
    fn test_connection_config_with_optional_fields() {
        let toml_str = r#"
            name = "mud"
            host = "mud.example.com"
            port = 3000
            encoding = "gbk"
            script = "michen_xkx.lua"
            auto_connect = false
            auto_reconnect = false
            reconnect_delay_secs = 10
            username = "user1"
            password = "pass1"
        "#;
        let config: ConnectionConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.encoding.as_deref(), Some("gbk"));
        assert_eq!(config.script.as_deref(), Some("michen_xkx.lua"));
        assert!(!config.auto_connect);
        assert!(!config.auto_reconnect);
        assert_eq!(config.reconnect_delay_secs, 10);
        assert_eq!(config.username.as_deref(), Some("user1"));
        assert_eq!(config.password.as_deref(), Some("pass1"));
    }

    #[test]
    fn test_load_profiles_empty_dir() {
        let dir = TempDir::new().unwrap();
        let (profiles, skipped) = AppConfig::load_profiles(dir.path().to_str().unwrap());
        assert!(profiles.is_empty());
        assert_eq!(skipped, 0);
    }

    #[test]
    fn test_load_profiles_nonexistent_dir() {
        let (profiles, skipped) = AppConfig::load_profiles("/nonexistent/path");
        assert!(profiles.is_empty());
        assert_eq!(skipped, 0);
    }

    #[test]
    fn test_load_profiles_skips_example() {
        let dir = TempDir::new().unwrap();
        let example_path = dir.path().join("example.toml");
        let mut f = fs::File::create(&example_path).unwrap();
        writeln!(
            f,
            r#"name = "example"
host = "example.com"
port = 4000"#
        )
        .unwrap();

        let (profiles, skipped) = AppConfig::load_profiles(dir.path().to_str().unwrap());
        assert!(profiles.is_empty());
        assert_eq!(skipped, 0);
    }

    #[test]
    fn test_load_profiles_valid_config() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("mud.toml");
        let mut f = fs::File::create(&config_path).unwrap();
        writeln!(
            f,
            r#"name = "mud"
host = "mud.example.com"
port = 3000"#
        )
        .unwrap();

        let (profiles, skipped) = AppConfig::load_profiles(dir.path().to_str().unwrap());
        assert_eq!(profiles.len(), 1);
        assert_eq!(skipped, 0);
        assert_eq!(profiles[0].name, "mud");
        assert_eq!(profiles[0].host, "mud.example.com");
        assert_eq!(profiles[0].port, 3000);
    }

    #[test]
    fn test_load_profiles_invalid_toml() {
        let dir = TempDir::new().unwrap();
        let bad_path = dir.path().join("bad.toml");
        fs::write(&bad_path, "not valid toml {{{{").unwrap();

        let (profiles, skipped) = AppConfig::load_profiles(dir.path().to_str().unwrap());
        assert!(profiles.is_empty());
        assert_eq!(skipped, 1);
    }

    #[test]
    fn test_load_profiles_skips_non_toml() {
        let dir = TempDir::new().unwrap();
        let txt_path = dir.path().join("readme.txt");
        fs::write(&txt_path, "not a config").unwrap();

        let (profiles, skipped) = AppConfig::load_profiles(dir.path().to_str().unwrap());
        assert!(profiles.is_empty());
        assert_eq!(skipped, 0);
    }

    #[test]
    fn test_load_profiles_multiple_configs() {
        let dir = TempDir::new().unwrap();

        let path1 = dir.path().join("alpha.toml");
        let mut f1 = fs::File::create(&path1).unwrap();
        writeln!(
            f1,
            r#"name = "alpha"
host = "alpha.com"
port = 1000"#
        )
        .unwrap();

        let path2 = dir.path().join("beta.toml");
        let mut f2 = fs::File::create(&path2).unwrap();
        writeln!(
            f2,
            r#"name = "beta"
host = "beta.com"
port = 2000"#
        )
        .unwrap();

        let (profiles, skipped) = AppConfig::load_profiles(dir.path().to_str().unwrap());
        assert_eq!(profiles.len(), 2);
        assert_eq!(skipped, 0);
        // 按文件名排序：alpha < beta
        assert_eq!(profiles[0].name, "alpha");
        assert_eq!(profiles[1].name, "beta");
    }

    #[test]
    fn test_load_profiles_mixed_valid_invalid() {
        let dir = TempDir::new().unwrap();

        let good_path = dir.path().join("good.toml");
        let mut f = fs::File::create(&good_path).unwrap();
        writeln!(
            f,
            r#"name = "good"
host = "good.com"
port = 5000"#
        )
        .unwrap();

        let bad_path = dir.path().join("bad.toml");
        fs::write(&bad_path, "invalid {{{{").unwrap();

        let (profiles, skipped) = AppConfig::load_profiles(dir.path().to_str().unwrap());
        assert_eq!(profiles.len(), 1);
        assert_eq!(skipped, 1);
    }

    #[test]
    fn test_connection_config_with_all_optional_fields() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("full.toml");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"name = "full"
host = "mud.example.com"
port = 4000
encoding = "gbk"
script = "/path/to/script.lua"
auto_connect = true
auto_reconnect = false
reconnect_delay_secs = 10
username = "player"
password = "secret""#
        )
        .unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let config: ConnectionConfig = toml::from_str(&content).unwrap();
        assert_eq!(config.name, "full");
        assert_eq!(config.encoding, Some("gbk".to_string()));
        assert_eq!(config.script, Some("/path/to/script.lua".to_string()));
        assert!(config.auto_connect);
        assert!(!config.auto_reconnect);
        assert_eq!(config.reconnect_delay_secs, 10);
        assert_eq!(config.username, Some("player".to_string()));
        assert_eq!(config.password, Some("secret".to_string()));
    }

    #[test]
    fn test_load_default_with_custom_dir() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("custom.toml");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"name = "custom"
host = "custom.com"
port = 6000"#
        )
        .unwrap();

        let config = AppConfig::load_default(dir.path().to_str().unwrap());
        assert_eq!(config.connections.len(), 1);
        assert_eq!(config.connections[0].name, "custom");
        assert_eq!(config.connections[0].host, "custom.com");
        assert_eq!(config.connections[0].port, 6000);
    }

    #[test]
    fn test_load_default_with_nonexistent_dir() {
        // 目录不存在时应该返回默认配置
        let config = AppConfig::load_default("/nonexistent/path/that/does/not/exist");
        assert!(config.connections.is_empty());
    }
}
