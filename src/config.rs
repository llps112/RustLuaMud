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
    24
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
    /// SOCKS5 代理开关，默认 false（直连）
    #[serde(default)]
    pub socks5_enable: bool,
    /// SOCKS5 代理地址
    #[serde(default)]
    pub socks5_host: Option<String>,
    /// SOCKS5 代理端口，默认 1080
    #[serde(default = "default_socks5_port")]
    pub socks5_port: u16,
    /// SOCKS5 代理用户名（可选）
    #[serde(default)]
    pub socks5_username: Option<String>,
    /// SOCKS5 代理密码（可选）
    #[serde(default)]
    pub socks5_password: Option<String>,
    /// 日志文件保留数量（可选，不设置则使用全局默认值 24）
    #[serde(default)]
    pub log_rotation_count: Option<usize>,
    /// 渲染间隔（毫秒），最小值 50ms，默认 1000ms
    #[serde(default = "default_render_interval")]
    pub render_interval: u64,
    /// 实时渲染开关，true 时忽略 render_interval 直接实时渲染，默认 false
    #[serde(default)]
    pub realtime: bool,
    /// 连接建立后延迟执行 OnConnect 的毫秒数，默认 1000ms
    /// 防止连接瞬间批量发送指令触发服务器反 flood 机制
    #[serde(default = "default_connect_delay")]
    pub connect_delay_ms: u64,
    /// 命令发送最小间隔（毫秒），默认 50ms，范围 20~200ms
    /// 控制发送到 MUD 服务器的物理速率，值越小发送越快
    /// 推荐值：50ms（普通玩家）、80ms（轻度延迟）、120ms（保守安全）
    #[serde(default = "default_cmd_interval_ms")]
    pub cmd_interval_ms: u64,
    /// 令牌桶容量（突发上限），默认 10
    /// 允许短时间内发送的最大命令数，对应 Lua 侧原 max_burst
    /// 安全约束：burst_size + 2×cmds_per_sec ≤ 60
    #[serde(default = "default_burst_size")]
    pub burst_size: u64,
    /// 每秒令牌补充速率，默认 20
    /// 控制长期平均发送速率的上界，对应 Lua 侧原 cmd.setnums。
    /// 应配为服务端 drain 速率 20（= 40 条/2 秒），调高会让 cnt 逐周期净增
    #[serde(default = "default_cmds_per_sec")]
    pub cmds_per_sec: u64,
    /// 滑动窗口内允许的最大命令数，默认 60，生效范围 1~1000
    /// 对应服务端雷劈阈值 3*CMDS_PER_TICK（LPC cmd.c），令牌桶多次突发累积时的硬兜底
    /// 调小可进一步降低触发反 flood 的风险；调大到超过 60 则窗口不再具备保护作用
    #[serde(default = "default_window_limit")]
    pub window_limit: u64,
    /// 滑动窗口时长（毫秒），默认 2000，生效范围 2000~10000
    /// 对应服务端 clear_cmd_count 的 2 秒 drain 周期，与 window_limit 共同构成
    /// 「任意 window_duration_ms 内发送条数 ≤ window_limit」的约束。
    /// 不得低于 2000：短于 drain 周期时上述约束无法覆盖服务端计数窗口，兜底失效
    #[serde(default = "default_window_duration_ms")]
    pub window_duration_ms: u64,
    /// 重连退避最大间隔（秒），默认 1800（30分钟）
    /// 指数退避上限，实际等待 = min(base * 2^attempt, max_secs)
    #[serde(default = "default_reconnect_max_secs")]
    pub reconnect_max_secs: u64,
    /// 空闲超时（秒），超过此时间无服务器数据则发送心跳，默认 300（5分钟）
    #[serde(default = "default_idle_timeout_secs")]
    pub idle_timeout_secs: u64,
    /// 心跳命令内容，空字符串表示不启用心跳检测
    #[serde(default)]
    pub heartbeat_cmd: String,
    /// 心跳响应超时（秒），发送心跳后超过此时间无响应则断连，默认 60
    #[serde(default = "default_heartbeat_timeout_secs")]
    pub heartbeat_timeout_secs: u64,
}

/// 服务端 LPC cmd.c 的反 flood 常量，用于校验限速参数组合是否安全。
/// 改动前需先核对 LPC/cmd.c：`#define CMDS_PER_TICK 20` / `#define TICK 2`
const SERVER_CMDS_PER_TICK: u64 = 20;
/// 每个 drain 周期清除的计数：clear_cmd_count 中 `cnt -= 2 * CMDS_PER_TICK`
const SERVER_DRAIN_PER_CYCLE: u64 = 2 * SERVER_CMDS_PER_TICK;
/// 雷劈阈值：process_input 中 `cnt > 3 * CMDS_PER_TICK` → unconscious / 强制 quit
const SERVER_STRIKE_THRESHOLD: u64 = 3 * SERVER_CMDS_PER_TICK;
/// drain 周期（毫秒）
const SERVER_TICK_MS: u64 = 2000;

impl ConnectionConfig {
    /// 从 TOML 文本解析角色配置的统一入口（启动批量加载与运行时 /profile load 均须走此），
    /// 解析成功后对凭据类字段做 `${ENV_VAR}` 占位符展开。
    /// 启动路径专用：展开告警直接 eprintln（启动阶段终端可见）。
    pub fn from_toml_str(content: &str) -> Result<Self, String> {
        let mut warns = Vec::new();
        let cfg = Self::from_toml_str_with_warnings(content, &mut warns)?;
        for w in &warns {
            eprintln!("警告: {}", w);
        }
        Ok(cfg)
    }

    /// 带告警收集的解析入口：环境变量缺失、限速参数不安全等告警追加到 warns，
    /// 由调用方决定输出渠道。
    /// 运行时 /profile load 时终端处于 raw mode，stderr 不可见，必须由终端 UI 展示。
    pub fn from_toml_str_with_warnings(
        content: &str,
        warns: &mut Vec<String>,
    ) -> Result<Self, String> {
        let mut cfg: Self = toml::from_str(content).map_err(|e| e.to_string())?;
        cfg.resolve_credential_env(warns);
        cfg.validate_rate_limit(warns);
        Ok(cfg)
    }

    /// 校验限速参数组合是否落在服务端反 flood 的安全范围内。
    ///
    /// 滑动窗口只封顶突发密度，长期速率由 cmds_per_sec 决定，两者必须同时满足
    /// 服务端 LPC cmd.c 的约束（推导见 rate_limiter 模块文档）。burst_size 与
    /// cmds_per_sec 在 Session::new 里只做 `.max(1)`、无上限钳制，因此不安全的组合
    /// 能一路生效到写入任务，必须在解析阶段就把风险告知用户。
    ///
    /// 这里只告警不改值：参数原样保留便于与 TOML 原文比对，运行期钳制在 Session::new。
    /// 全程使用 saturating 运算：配置值可能为 u64::MAX，普通乘法会溢出 panic。
    fn validate_rate_limit(&self, warns: &mut Vec<String>) {
        // 服务端每 2 秒 drain 40，等效长期速率上限 20 条/秒
        let drain_per_sec = SERVER_DRAIN_PER_CYCLE.saturating_mul(1000) / SERVER_TICK_MS;
        if self.cmds_per_sec > drain_per_sec {
            warns.push(format!(
                "{} 的 cmds_per_sec={} 超过服务端 drain 速率 {} 条/秒，cnt 会逐周期净增，\
                 长时间挂机必然触发雷劈；window_limit 封顶的是突发密度而非长期速率，\
                 挡不住这种超速，建议改回 {}",
                self.name, self.cmds_per_sec, drain_per_sec, drain_per_sec
            ));
        }

        // 单次突发峰值：burst_size 条 0ms 间隔 + 随后 2 秒内匀速 2×cmds_per_sec 条
        let refill_per_cycle = self.cmds_per_sec.saturating_mul(2);
        let burst_peak = self.burst_size.saturating_add(refill_per_cycle);
        if burst_peak > SERVER_STRIKE_THRESHOLD {
            warns.push(format!(
                "{} 的 burst_size={} + 2×cmds_per_sec={} = {} 超过服务端雷劈阈值 {}，\
                 单次突发即可能被打晕或强制退出，建议把 burst_size 降到 {} 以下",
                self.name,
                self.burst_size,
                self.cmds_per_sec,
                burst_peak,
                SERVER_STRIKE_THRESHOLD,
                SERVER_STRIKE_THRESHOLD.saturating_sub(refill_per_cycle)
            ));
        }

        if self.window_limit > SERVER_STRIKE_THRESHOLD {
            warns.push(format!(
                "{} 的 window_limit={} 高于服务端雷劈阈值 {}，滑动窗口将失去保护作用，\
                 建议设为 {} 或更低（{} = 服务端每周期 drain 量，可无条件保证安全）",
                self.name,
                self.window_limit,
                SERVER_STRIKE_THRESHOLD,
                SERVER_STRIKE_THRESHOLD,
                SERVER_DRAIN_PER_CYCLE
            ));
        }

        if self.window_duration_ms < SERVER_TICK_MS {
            warns.push(format!(
                "{} 的 window_duration_ms={} 短于服务端 drain 周期 {}ms，\
                 「任意 2 秒 ≤ window_limit」的兜底会失效，运行时已上调到 {}",
                self.name, self.window_duration_ms, SERVER_TICK_MS, SERVER_TICK_MS
            ));
        }
    }

    /// 逐个展开凭据字段占位符。环境变量缺失时告警并置 None，
    /// 等同于未设置该凭据（留待手动输入），不会把占位符文本当密码发给服务器。
    fn resolve_credential_env(&mut self, warns: &mut Vec<String>) {
        Self::expand_opt("username", &mut self.username, &self.name, warns);
        Self::expand_opt("password", &mut self.password, &self.name, warns);
        Self::expand_opt(
            "socks5_username",
            &mut self.socks5_username,
            &self.name,
            warns,
        );
        Self::expand_opt(
            "socks5_password",
            &mut self.socks5_password,
            &self.name,
            warns,
        );
    }

    fn expand_opt(
        field: &str,
        holder: &mut Option<String>,
        profile: &str,
        warns: &mut Vec<String>,
    ) {
        let Some(raw) = holder.as_deref() else { return };
        match expand_credential_placeholder(raw) {
            Ok(v) => *holder = Some(v),
            Err(var) => {
                warns.push(format!(
                    "{} 的 {} 引用的环境变量 {} 未设置，该字段按空处理",
                    profile, field, var
                ));
                *holder = None;
            }
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_render_interval() -> u64 {
    1000
}
fn default_reconnect_delay() -> u64 {
    5
}
fn default_socks5_port() -> u16 {
    1080
}
fn default_connect_delay() -> u64 {
    1000
}
fn default_cmd_interval_ms() -> u64 {
    50
}
fn default_burst_size() -> u64 {
    10
}
fn default_cmds_per_sec() -> u64 {
    20
}
fn default_window_limit() -> u64 {
    60
}
fn default_window_duration_ms() -> u64 {
    2000
}
fn default_reconnect_max_secs() -> u64 {
    1800
}
fn default_idle_timeout_secs() -> u64 {
    300
}
fn default_heartbeat_timeout_secs() -> u64 {
    60
}

/// 判断是否为合法的环境变量名：字母/下划线开头，后接字母数字下划线
fn is_env_var_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// 凭据字段的环境变量占位符展开：
/// - 整值恰为 `${VAR_NAME}` 时替换为同名环境变量的值，变量缺失返回 Err(变量名)
/// - `$${NAME}` 为字面量转义，得到 `${NAME}`（密码本体长占位符形状时使用）
/// - 其余情况（普通密码、含部分 `${}` 的值、非法变量名）一律原样返回，
///   不做子串替换，避免误伤正常配置
fn expand_credential_placeholder(value: &str) -> Result<String, String> {
    if let Some(rest) = value.strip_prefix("$${") {
        if let Some(inner) = rest.strip_suffix('}') {
            if is_env_var_name(inner) {
                return Ok(format!("${{{}}}", inner));
            }
        }
        return Ok(value.to_string());
    }
    if let Some(inner) = value.strip_prefix("${").and_then(|v| v.strip_suffix('}')) {
        if is_env_var_name(inner) {
            return std::env::var(inner).map_err(|_| inner.to_string());
        }
    }
    Ok(value.to_string())
}

/// 加载 dotenv 格式的凭据文件（约定路径 `<profiles目录>/.env`）到进程环境变量。
///
/// 规则：
/// - 每行 `KEY=VALUE`，按第一个 `=` 分割（值内可含 `=`）；空行与 `#` 注释行忽略
/// - 键值两端空白自动去除；值被成对单/双引号包裹时去引号（保留密码中的空格）
/// - 真实环境变量优先：同名变量已存在时不覆盖（setx/系统变量 > .env）
/// - 非法变量名/缺少 `=` 的行带行号告警并跳过
///
/// 返回实际写入的变量数量。调用方须保证在解析 profile（`${VAR}` 展开）之前执行。
/// 仅启动时加载一次，修改 .env 后需重启客户端生效。
pub fn load_env_file(path: &Path) -> usize {
    // 读取失败（非 UTF-8 编码/权限等）时明确告警而非静默返回 0：
    // 目标用户是中文 Windows 玩家，记事本默认存 ANSI(GBK)，若静默失败会导致全部占位符置空且无从排查。
    // .env 必须以 UTF-8 保存（见 .env.example 提示）。
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "警告: 无法读取 {} ({}),若为编码问题请以 UTF-8 保存",
                path.display(),
                e
            );
            return 0;
        }
    };
    let mut loaded = 0;
    for (idx, raw_line) in content.lines().enumerate() {
        // trim_start_matches 处理记事本等工具写入的 UTF-8 BOM（仅首行可能带）
        let line = raw_line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            eprintln!("警告: .env 第 {} 行缺少 '='，已跳过", idx + 1);
            continue;
        };
        let key = key.trim();
        if !is_env_var_name(key) {
            eprintln!("警告: .env 第 {} 行变量名 '{}' 非法，已跳过", idx + 1, key);
            continue;
        }
        let mut value = value.trim().to_string();
        // 去除成对的首尾引号（"..." 或 '...'），保护含空格/特殊字符的密码
        let chars: Vec<char> = value.chars().collect();
        if chars.len() >= 2
            && (chars[0] == '"' || chars[0] == '\'')
            && chars[0] == chars[chars.len() - 1]
        {
            value = chars[1..chars.len() - 1].iter().collect();
        }
        // 真实环境优先：已被 setx/系统设置的同名变量不被 .env 覆盖
        if std::env::var_os(key).is_some() {
            continue;
        }
        std::env::set_var(key, value);
        loaded += 1;
    }
    loaded
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
        // 先加载 .env 凭据文件，再解析角色配置（保证 ${VAR} 展开时变量已就位）
        let env_path = Path::new(profiles_dir).join(".env");
        if env_path.exists() {
            let n = load_env_file(&env_path);
            eprintln!("已从 {} 加载 {} 个环境变量", env_path.display(), n);
        }
        // 从 profiles 目录加载所有角色配置作为默认连接
        let (profiles, skipped) = Self::load_profiles(profiles_dir);
        let general = GeneralConfig {
            profile_dir: profiles_dir.to_string(),
            ..Default::default()
        };

        if !profiles.is_empty() {
            if skipped > 0 {
                eprintln!("警告: {} 个角色配置加载失败", skipped);
            }
            return Self {
                general,
                connections: profiles,
            };
        }

        eprintln!("警告: {} 目录未找到角色配置，使用默认配置", profiles_dir);
        Self {
            general,
            connections: Vec::new(),
        }
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
                Ok(content) => match ConnectionConfig::from_toml_str(&content) {
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

/// 启动期自检：确保目录存在且可写（写探针文件再删除）。
///
/// 失败时返回携带真实原因（权限/路径/磁盘）的错误，供上层在启动瞬间暴露问题，
/// 避免挂机到运行期才发现无法写日志或保存终端设置（例如误装到 Program Files
/// 无写权限、或磁盘只读等）。跨平台可用。
pub fn verify_writable_dir(dir: &Path) -> std::io::Result<()> {
    if let Err(e) = fs::create_dir_all(dir) {
        return Err(std::io::Error::new(
            e.kind(),
            format!("无法创建目录 '{}': {}", dir.display(), e),
        ));
    }
    // 探针文件名含 pid，避免多实例并发时互相覆盖；
    // 失败时保留原始 ErrorKind（可能是只读盘/路径超长，不一定是权限问题）
    let probe = dir.join(format!(".rlm_write_probe_{}", std::process::id()));
    match fs::write(&probe, b"") {
        Ok(()) => {
            let _ = fs::remove_file(&probe);
            Ok(())
        }
        Err(e) => Err(std::io::Error::new(
            e.kind(),
            format!("目录 '{}' 不可写: {}", dir.display(), e),
        )),
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
        assert_eq!(config.log_rotation_count, 24);
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
    fn test_verify_writable_dir_ok() {
        let dir = TempDir::new().unwrap();
        // 已存在且可写的目录：返回 Ok
        assert!(verify_writable_dir(dir.path()).is_ok());
        // 缺失的子目录会被自动创建，且返回 Ok
        let sub = dir.path().join("logs");
        assert!(verify_writable_dir(&sub).is_ok());
        assert!(sub.exists());
        // 探针文件写入后应已清理，不残留
        let leftovers: Vec<_> = fs::read_dir(&sub)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(".rlm_write_probe_")
            })
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn test_verify_writable_dir_error() {
        let dir = TempDir::new().unwrap();
        // 用一个普通文件当“目录”：其父路径不是目录，create_dir_all 失败 → Err
        let file = dir.path().join("not_a_dir");
        fs::write(&file, b"x").unwrap();
        let bad = file.join("sub");
        assert!(verify_writable_dir(&bad).is_err());
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

        let dir_str = dir.path().to_str().unwrap();
        let config = AppConfig::load_default(dir_str);
        assert_eq!(config.connections.len(), 1);
        assert_eq!(config.connections[0].name, "custom");
        assert_eq!(config.connections[0].host, "custom.com");
        assert_eq!(config.connections[0].port, 6000);
        // profile_dir 应追踪传入的目录
        assert_eq!(config.general.profile_dir, dir_str);
    }

    #[test]
    fn test_load_default_with_nonexistent_dir() {
        // 目录不存在时应该返回默认配置，但 profile_dir 仍追踪参数
        let path = "/nonexistent/path/that/does/not/exist";
        let config = AppConfig::load_default(path);
        assert!(config.connections.is_empty());
        assert_eq!(config.general.profile_dir, path);
    }

    #[test]
    fn test_load_default_preserves_profiles_dir() {
        // 无 --profiles 参数时 profile_dir 保持默认值 "profiles"
        let dir = TempDir::new().unwrap();
        // 目录为空，不会有任何连接
        let dir_str = dir.path().to_str().unwrap();
        let config = AppConfig::load_default(dir_str);
        assert_eq!(config.general.profile_dir, dir_str);
        assert!(config.connections.is_empty());
    }

    #[test]
    fn test_connection_config_log_rotation_count() {
        // 不设置 log_rotation_count 时默认为 None
        let toml_str = r#"
            name = "test"
            host = "example.com"
            port = 4000
        "#;
        let config: ConnectionConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.log_rotation_count, None);

        // 显式设置
        let toml_str2 = r#"
            name = "test2"
            host = "example.com"
            port = 4000
            log_rotation_count = 48
        "#;
        let config2: ConnectionConfig = toml::from_str(toml_str2).unwrap();
        assert_eq!(config2.log_rotation_count, Some(48));
    }

    #[test]
    fn test_render_interval_default() {
        let toml_str = r#"
            name = "test"
            host = "example.com"
            port = 4000
        "#;
        let config: ConnectionConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.render_interval, 1000);
    }

    #[test]
    fn test_render_interval_custom() {
        let toml_str = r#"
            name = "test"
            host = "example.com"
            port = 4000
            render_interval = 500
        "#;
        let config: ConnectionConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.render_interval, 500);
    }

    #[test]
    fn test_render_interval_zero() {
        let toml_str = r#"
            name = "test"
            host = "example.com"
            port = 4000
            render_interval = 0
        "#;
        let config: ConnectionConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.render_interval, 0);
    }

    #[test]
    fn test_connect_delay_ms_default() {
        let toml_str = r#"
            name = "test"
            host = "example.com"
            port = 4000
        "#;
        let config: ConnectionConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.connect_delay_ms, 1000);
    }

    #[test]
    fn test_connect_delay_ms_custom() {
        let toml_str = r#"
            name = "test"
            host = "example.com"
            port = 4000
            connect_delay_ms = 2000
        "#;
        let config: ConnectionConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.connect_delay_ms, 2000);
    }

    #[test]
    fn test_connect_delay_ms_zero() {
        let toml_str = r#"
            name = "test"
            host = "example.com"
            port = 4000
            connect_delay_ms = 0
        "#;
        let config: ConnectionConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.connect_delay_ms, 0);
    }

    #[test]
    fn test_heartbeat_defaults() {
        let toml_str = r#"
            name = "test"
            host = "example.com"
            port = 4000
        "#;
        let config: ConnectionConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.idle_timeout_secs, 300);
        assert_eq!(config.heartbeat_cmd, "");
        assert_eq!(config.heartbeat_timeout_secs, 60);
    }

    #[test]
    fn test_heartbeat_custom() {
        let toml_str = r#"
            name = "test"
            host = "example.com"
            port = 4000
            idle_timeout_secs = 120
            heartbeat_cmd = "look"
            heartbeat_timeout_secs = 30
        "#;
        let config: ConnectionConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.idle_timeout_secs, 120);
        assert_eq!(config.heartbeat_cmd, "look");
        assert_eq!(config.heartbeat_timeout_secs, 30);
    }

    #[test]
    fn test_reconnect_max_secs_default() {
        let toml_str = r#"
            name = "test"
            host = "example.com"
            port = 4000
        "#;
        let config: ConnectionConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.reconnect_max_secs, 1800);
    }

    #[test]
    fn test_reconnect_max_secs_custom() {
        let toml_str = r#"
            name = "test"
            host = "example.com"
            port = 4000
            reconnect_max_secs = 600
        "#;
        let config: ConnectionConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.reconnect_max_secs, 600);
    }

    // ===== 限速参数 =====

    #[test]
    fn test_rate_limit_defaults() {
        let toml_str = r#"
            name = "test"
            host = "example.com"
            port = 4000
        "#;
        let config: ConnectionConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.cmd_interval_ms, 50);
        assert_eq!(config.burst_size, 10);
        assert_eq!(config.cmds_per_sec, 20);
        // 滑动窗口默认对齐服务端雷劈阈值：60 条 / 2 秒
        assert_eq!(config.window_limit, 60);
        assert_eq!(config.window_duration_ms, 2000);
    }

    #[test]
    fn test_sliding_window_custom() {
        let toml_str = r#"
            name = "test"
            host = "example.com"
            port = 4000
            window_limit = 40
            window_duration_ms = 3000
        "#;
        let config: ConnectionConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.window_limit, 40);
        assert_eq!(config.window_duration_ms, 3000);
    }

    #[test]
    fn test_sliding_window_out_of_range_not_clamped_at_config_layer() {
        // 配置层是纯反序列化目标，原样保留用户写入的值；运行期安全区间统一在
        // Session::new 钳制（与 cmd_interval_ms 一致）。若在此处提前钳制，
        // 配置结构体就不再反映 TOML 原文，往返比对与问题排查都会失真
        let toml_str = r#"
            name = "test"
            host = "example.com"
            port = 4000
            window_limit = 0
            window_duration_ms = 1500
        "#;
        let config: ConnectionConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.window_limit, 0);
        assert_eq!(config.window_duration_ms, 1500);
    }

    // ===== 限速参数安全校验 =====

    /// 解析并收集告警，用于验证 validate_rate_limit
    fn rate_limit_warnings(toml_str: &str) -> Vec<String> {
        let mut warns = Vec::new();
        ConnectionConfig::from_toml_str_with_warnings(toml_str, &mut warns).unwrap();
        warns
    }

    /// 包装只关心限速字段的 TOML 片段
    fn rate_limit_toml(body: &str) -> String {
        format!(
            "name = \"paojia\"\nhost = \"example.com\"\nport = 4000\n{}\n",
            body
        )
    }

    #[test]
    fn test_rate_limit_safe_config_produces_no_warning() {
        // profiles/example.toml 与生产 profile 的实际参数组合，必须零告警
        let warns = rate_limit_warnings(&rate_limit_toml(
            "burst_size = 15\ncmds_per_sec = 20\ncmd_interval_ms = 50\nwindow_limit = 60\nwindow_duration_ms = 2000",
        ));
        assert!(warns.is_empty(), "安全配置不应告警，实际 {:?}", warns);
    }

    #[test]
    fn test_rate_limit_defaults_produce_no_warning() {
        // 未显式配置时走默认值（10/20/60/2000），同样必须安全
        let warns = rate_limit_warnings(&rate_limit_toml(""));
        assert!(warns.is_empty(), "默认配置不应告警，实际 {:?}", warns);
    }

    #[test]
    fn test_rate_limit_warns_when_cmds_per_sec_exceeds_drain() {
        // cmds_per_sec=21 仅比推荐值大 1，但长期速率已超过服务端 drain，cnt 逐周期净增
        let warns = rate_limit_warnings(&rate_limit_toml("cmds_per_sec = 21"));
        assert!(
            warns
                .iter()
                .any(|w| w.contains("cmds_per_sec") && w.contains("drain")),
            "应告警长期速率超过 drain，实际 {:?}",
            warns
        );
    }

    #[test]
    fn test_rate_limit_warns_when_burst_peak_exceeds_threshold() {
        // burst_size=65：65 + 2×20 = 105 > 60，实测服务端 cnt 峰值 64 会雷劈
        let warns = rate_limit_warnings(&rate_limit_toml("burst_size = 65"));
        assert!(
            warns
                .iter()
                .any(|w| w.contains("burst_size") && w.contains("105")),
            "应告警单次突发峰值越界，实际 {:?}",
            warns
        );
    }

    #[test]
    fn test_rate_limit_warns_when_window_limit_above_threshold() {
        let warns = rate_limit_warnings(&rate_limit_toml("window_limit = 100"));
        assert!(
            warns
                .iter()
                .any(|w| w.contains("window_limit") && w.contains("失去保护")),
            "应告警窗口失去保护作用，实际 {:?}",
            warns
        );
    }

    #[test]
    fn test_rate_limit_warns_when_window_duration_below_drain_cycle() {
        let warns = rate_limit_warnings(&rate_limit_toml("window_duration_ms = 1500"));
        assert!(
            warns.iter().any(|w| w.contains("window_duration_ms")),
            "应告警窗口短于 drain 周期，实际 {:?}",
            warns
        );
    }

    #[test]
    fn test_rate_limit_extreme_values_do_not_overflow() {
        // 校验全程 saturating：u64::MAX 不得让 2×cmds_per_sec 溢出 panic
        let warns = rate_limit_warnings(&rate_limit_toml(
            "burst_size = 18446744073709551615\ncmds_per_sec = 18446744073709551615\nwindow_limit = 18446744073709551615",
        ));
        assert!(warns.len() >= 3, "极端值应逐项告警，实际 {:?}", warns);
    }

    // ===== 凭据环境变量占位符展开 =====

    #[test]
    fn test_placeholder_env_var_resolved() {
        std::env::set_var("RLM_TEST_PWD_RESOLVED", "s3cr3t");
        let got = expand_credential_placeholder("${RLM_TEST_PWD_RESOLVED}");
        assert_eq!(got, Ok("s3cr3t".to_string()));
    }

    #[test]
    fn test_placeholder_missing_var_returns_err() {
        // 未设置的变量 → Err(变量名)，由调用方告警并置空
        let got = expand_credential_placeholder("${RLM_TEST_DEFINITELY_MISSING_VAR}");
        assert_eq!(got, Err("RLM_TEST_DEFINITELY_MISSING_VAR".to_string()));
    }

    #[test]
    fn test_placeholder_literal_passthrough() {
        // 普通密码、部分含 ${}、非法变量名均原样返回，不触发环境变量查找
        assert_eq!(
            expand_credential_placeholder("pass1"),
            Ok("pass1".to_string())
        );
        assert_eq!(
            expand_credential_placeholder("a${b}c"),
            Ok("a${b}c".to_string())
        );
        assert_eq!(
            expand_credential_placeholder("${1BADNAME}"),
            Ok("${1BADNAME}".to_string())
        );
        assert_eq!(
            expand_credential_placeholder("${NO-DASH}"),
            Ok("${NO-DASH}".to_string())
        );
        assert_eq!(expand_credential_placeholder("${}"), Ok("${}".to_string()));
    }

    #[test]
    fn test_placeholder_escape() {
        // $${NAME} 转义为字面量 ${NAME}，不查找环境变量
        assert_eq!(
            expand_credential_placeholder("$${RLM_TEST_PWD_RESOLVED}"),
            Ok("${RLM_TEST_PWD_RESOLVED}".to_string())
        );
        // $$ 前缀但内部非法变量名 → 整值原样
        assert_eq!(
            expand_credential_placeholder("$${no-dash}"),
            Ok("$${no-dash}".to_string())
        );
    }

    #[test]
    fn test_from_toml_str_resolves_all_credential_fields() {
        std::env::set_var("RLM_TEST_T_PWD", "real-pw");
        std::env::set_var("RLM_TEST_T_SOCKS", "socks-pw");
        let toml_str = r#"
            name = "hero"
            host = "example.com"
            port = 4000
            username = "${RLM_TEST_T_USER}"
            password = "${RLM_TEST_T_PWD}"
            socks5_password = "${RLM_TEST_T_SOCKS}"
        "#;
        std::env::set_var("RLM_TEST_T_USER", "hero-name");
        let config = ConnectionConfig::from_toml_str(toml_str).unwrap();
        assert_eq!(config.username.as_deref(), Some("hero-name"));
        assert_eq!(config.password.as_deref(), Some("real-pw"));
        assert_eq!(config.socks5_password.as_deref(), Some("socks-pw"));
    }

    #[test]
    fn test_from_toml_str_missing_env_sets_field_none() {
        // 缺失变量的凭据字段置 None（而非把占位符文本当密码），其余字段不受影响
        let toml_str = r#"
            name = "hero2"
            host = "example.com"
            port = 4000
            username = "plain-user"
            password = "${RLM_TEST_ANOTHER_MISSING_VAR}"
        "#;
        let config = ConnectionConfig::from_toml_str(toml_str).unwrap();
        assert_eq!(config.username.as_deref(), Some("plain-user"));
        assert_eq!(config.password, None);
    }

    #[test]
    fn test_from_toml_str_plain_password_unchanged() {
        // 向后兼容：不含占位符的存量配置行为完全不变
        let toml_str = r#"
            name = "old"
            host = "example.com"
            port = 4000
            password = "${not_a_var_shape!}"
        "#;
        let config = ConnectionConfig::from_toml_str(toml_str).unwrap();
        assert_eq!(config.password.as_deref(), Some("${not_a_var_shape!}"));
    }

    // ===== .env 凭据文件加载 =====

    #[test]
    fn test_load_env_file_parses_quotes_and_skips_bad_lines() {
        let dir = tempfile::TempDir::new().unwrap();
        let env_path = dir.path().join(".env");
        // 首行带 BOM 模拟 Windows 记事本保存；含注释/空行/等号值/非法键名/缺等号行
        fs::write(
            &env_path,
            "\u{feff}# 注释行\n\
             \n\
             RLM_ENV_T_A=plain\n\
             RLM_ENV_T_B = \"with space\"\n\
             RLM_ENV_T_C=p@ss=with=equals\n\
             bad name=x\n\
             noequalsline\n\
             RLM_ENV_T_D='single'\n",
        )
        .unwrap();

        let loaded = load_env_file(&env_path);
        assert_eq!(loaded, 4, "4 个合法条目应全部写入");
        assert_eq!(std::env::var("RLM_ENV_T_A").unwrap(), "plain");
        assert_eq!(std::env::var("RLM_ENV_T_B").unwrap(), "with space");
        assert_eq!(std::env::var("RLM_ENV_T_C").unwrap(), "p@ss=with=equals");
        assert_eq!(std::env::var("RLM_ENV_T_D").unwrap(), "single");
        // 非法键名不应被写入环境
        assert!(std::env::var_os("bad name").is_none());
    }

    #[test]
    fn test_load_env_file_does_not_override_existing_env() {
        // 真实环境优先：已被 set_var/setx 设置的同名变量不被 .env 覆盖
        std::env::set_var("RLM_ENV_T_EXIST", "from_system");
        let dir = tempfile::TempDir::new().unwrap();
        let env_path = dir.path().join(".env");
        fs::write(&env_path, "RLM_ENV_T_EXIST=from_envfile\n").unwrap();

        let loaded = load_env_file(&env_path);
        assert_eq!(loaded, 0, "同名变量已存在时不应计入写入数");
        assert_eq!(std::env::var("RLM_ENV_T_EXIST").unwrap(), "from_system");
    }

    #[test]
    fn test_load_env_file_missing_file_returns_zero() {
        let loaded = load_env_file(Path::new("nonexistent_rlm_test_dir/.env"));
        assert_eq!(loaded, 0);
    }

    #[test]
    fn test_load_env_file_unreadable_path_returns_zero() {
        // 路径指向目录而非文件：read_to_string 失败 → 告警 + 返回 0，不 panic。
        // 覆盖非 UTF-8（GBK/UTF-16）文件被拒的同一失败路径。
        let dir = tempfile::TempDir::new().unwrap();
        let loaded = load_env_file(dir.path());
        assert_eq!(loaded, 0);
    }

    #[test]
    fn test_from_toml_str_with_warnings_collects_missing_var() {
        // 告警收集：缺失变量的信息交给调用方决定输出渠道（启动 eprintln / TUI append_output）
        let toml_str = r#"
            name = "warned"
            host = "example.com"
            port = 4000
            password = "${RLM_TEST_WARN_MISSING_VAR}"
        "#;
        let mut warns = Vec::new();
        let cfg = ConnectionConfig::from_toml_str_with_warnings(toml_str, &mut warns).unwrap();
        assert_eq!(warns.len(), 1);
        assert!(warns[0].contains("RLM_TEST_WARN_MISSING_VAR"));
        assert!(warns[0].contains("warned"), "告警应含角色名便于定位");
        assert_eq!(cfg.password, None);
    }

    #[test]
    fn test_load_default_reads_env_file_before_profiles() {
        // 端到端：.env 提供变量 → 同目录 toml 的 ${VAR} 占位符在启动加载中展开
        let dir = tempfile::TempDir::new().unwrap();
        fs::write(dir.path().join(".env"), "RLM_ENV_T_E2E=e2e-pw\n").unwrap();
        fs::write(
            dir.path().join("char1.toml"),
            r#"
                name = "e2e"
                host = "example.com"
                port = 4000
                password = "${RLM_ENV_T_E2E}"
            "#,
        )
        .unwrap();

        let app = AppConfig::load_default(dir.path().to_str().unwrap());
        assert_eq!(app.connections.len(), 1);
        assert_eq!(app.connections[0].password.as_deref(), Some("e2e-pw"));
    }

    #[test]
    fn test_env_file_follows_profiles_dir_for_multi_instance() {
        // 多实例场景：.env 须跟随各自的 profiles 目录加载（--profiles profiles2 → profiles2/.env），
        // 两个目录的变量互不可见。变量名必须全局唯一（进程级环境），故两目录用不同名。
        let dir1 = tempfile::TempDir::new().unwrap();
        let dir2 = tempfile::TempDir::new().unwrap();
        for (dir, tag) in [(&dir1, "one"), (&dir2, "two")].iter() {
            fs::write(
                dir.path().join(".env"),
                format!("RLM_ENV_T_MULTI_{}=pw-{}\n", tag.to_uppercase(), tag),
            )
            .unwrap();
            fs::write(
                dir.path().join("char.toml"),
                format!(
                    r#"
                        name = "inst-{}"
                        host = "example.com"
                        port = 4000
                        password = "${{RLM_ENV_T_MULTI_{}}}"
                    "#,
                    tag,
                    tag.to_uppercase()
                ),
            )
            .unwrap();
        }

        let app1 = AppConfig::load_default(dir1.path().to_str().unwrap());
        let app2 = AppConfig::load_default(dir2.path().to_str().unwrap());
        assert_eq!(app1.connections[0].password.as_deref(), Some("pw-one"));
        assert_eq!(app2.connections[0].password.as_deref(), Some("pw-two"));
    }
}
