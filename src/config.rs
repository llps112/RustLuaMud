use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::{OnceLock, RwLock, RwLockReadGuard, RwLockWriteGuard};

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

/// 与上方 serde 的 `#[serde(default = "default_*")]` 共用同一批 `default_*()` 函数，
/// 保证「不带字段的 TOML 解析结果」与「代码内构造的默认实例」两条默认值来源同源。
/// 一致性由 tests::test_default_impl_matches_serde_defaults 逐字段钉死（name/host/port
/// 无 serde 默认值，不参与比对），但它只拦「单侧分叉」：某字段被换成另一个
/// `default_*()`、或被改成独立字面量时会失败。某个 `default_*()` 自身的返回值改动
/// 会同时作用于两侧，测试不会（也无法）察觉。
impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            host: String::new(),
            port: 0,
            encoding: None,
            script: None,
            auto_connect: default_true(),
            auto_reconnect: default_true(),
            reconnect_delay_secs: default_reconnect_delay(),
            username: None,
            password: None,
            socks5_enable: false,
            socks5_host: None,
            socks5_port: default_socks5_port(),
            socks5_username: None,
            socks5_password: None,
            log_rotation_count: None,
            render_interval: default_render_interval(),
            realtime: false,
            connect_delay_ms: default_connect_delay(),
            cmd_interval_ms: default_cmd_interval_ms(),
            burst_size: default_burst_size(),
            cmds_per_sec: default_cmds_per_sec(),
            window_limit: default_window_limit(),
            window_duration_ms: default_window_duration_ms(),
            reconnect_max_secs: default_reconnect_max_secs(),
            idle_timeout_secs: default_idle_timeout_secs(),
            heartbeat_cmd: String::new(),
            heartbeat_timeout_secs: default_heartbeat_timeout_secs(),
        }
    }
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
    /// 启动路径专用：展开告警直接 eprintln（此时尚未进入 raw mode，stderr 可见）。
    ///
    /// 凭据展开失败（missing 非空）时**不跳过**该 profile：跳过会静默减少连接数，
    /// 玩家只看到「少了一个角色」却不知原因；而且建连时 init_lua_for_session 还会
    /// 就 char_name 缺失再告警一次。这里只负责把补救办法说清楚。
    pub fn from_toml_str(content: &str) -> Result<Self, String> {
        let mut warns = Vec::new();
        let mut missing = Vec::new();
        let cfg = Self::from_toml_str_with_warnings(content, &mut warns, &mut missing)?;
        // warns 已逐项说清「哪个 profile 的哪个字段引用的哪个变量未设置」，
        // 所以补救办法只汇总说一次 —— 逐项再说一遍会与 warns 的文案前半句
        // 几乎逐字重复，启动时多个 profile 都有问题就会刷一屏相似告警
        for w in &warns {
            eprintln!("警告: {}", w);
        }
        if !missing.is_empty() {
            eprintln!(
                "提示: {} 有 {} 项凭据占位符未展开（具体变量名见上方告警），在 profiles 目录的 \
                 .env 中补齐后执行 /profile load {} 即可生效（无需重启客户端）",
                cfg.name,
                missing.len(),
                cfg.name
            );
        }
        Ok(cfg)
    }

    /// 带告警收集的解析入口：环境变量缺失、限速参数不安全等告警追加到 warns，
    /// 凭据占位符展开失败项追加到 missing，由调用方决定输出渠道与是否中止。
    /// 运行时 /profile load 时终端处于 raw mode，stderr 不可见，必须由终端 UI 展示。
    ///
    /// warns 与 missing 的分工：warns 是「可以继续」的告警（限速参数、以及展开失败的
    /// 事实陈述），missing 是「必须拦下」的配置错误 —— 只包含 TOML 里确实写了 `${VAR}`
    /// 却查不到的字段，不含「字段本来就没配」（后者是合法的手动输入语义）。
    pub fn from_toml_str_with_warnings(
        content: &str,
        warns: &mut Vec<String>,
        missing: &mut Vec<CredentialMiss>,
    ) -> Result<Self, String> {
        let mut cfg: Self = toml::from_str(content).map_err(|e| e.to_string())?;
        cfg.resolve_credential_env(warns, missing);
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
    fn resolve_credential_env(
        &mut self,
        warns: &mut Vec<String>,
        missing: &mut Vec<CredentialMiss>,
    ) {
        Self::expand_opt("username", &mut self.username, &self.name, warns, missing);
        Self::expand_opt("password", &mut self.password, &self.name, warns, missing);
        Self::expand_opt(
            "socks5_username",
            &mut self.socks5_username,
            &self.name,
            warns,
            missing,
        );
        Self::expand_opt(
            "socks5_password",
            &mut self.socks5_password,
            &self.name,
            warns,
            missing,
        );
    }

    fn expand_opt(
        field: &'static str,
        holder: &mut Option<String>,
        profile: &str,
        warns: &mut Vec<String>,
        missing: &mut Vec<CredentialMiss>,
    ) {
        let Some(raw) = holder.as_deref() else { return };
        match expand_credential_placeholder(raw) {
            Ok(v) => *holder = Some(v),
            Err(var) => {
                warns.push(format!(
                    "{} 的 {} 引用的环境变量 {} 未设置，该字段按空处理",
                    profile, field, var
                ));
                missing.push(CredentialMiss {
                    field,
                    var: var.clone(),
                });
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
            return lookup_credential_var(inner).ok_or_else(|| inner.to_string());
        }
    }
    Ok(value.to_string())
}

/// 凭据占位符展开失败项：TOML 里写了 `${VAR}` 但该变量未定义。
///
/// 必须与「字段本来就没配」区分开：后者是既有的「留待手动输入」语义，不该拦；
/// 前者会让 Lua 侧 `char_name` 变成 nil，脚本顶层拼接（如
/// `include("config_"..me.charid..".lua")`）时才崩溃 —— 报错点离真正原因很远，
/// 必须在建 session 之前就拦下。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialMiss {
    /// 出问题的字段名（username / password / socks5_username / socks5_password）
    pub field: &'static str,
    /// 未能解析的环境变量名
    pub var: String,
}

/// .env 的内存态快照，替代运行时 `std::env::set_var`。
#[derive(Default)]
struct EnvStore {
    /// .env 提供、且启动时真实环境中不存在的键 —— 可被运行时刷新覆盖
    values: HashMap<String, String>,
    /// 启动时真实环境已存在的键（setx/系统变量）—— 永不被 .env 覆盖，也不进 values
    system_owned: HashSet<String>,
    /// 凡由 .env 提供过的键，**只增不减**。
    ///
    /// 归 .env 管辖的键即使后来从 .env 里删掉，也必须留在本集合中：启动时
    /// `load_env_file` 已把它 `set_var` 进真实进程环境，若查找时无条件回退
    /// `std::env::var`，那份残留会让「删键」形同虚设 —— 实测表现为「删掉
    /// .env 里的凭据键后 `/profile load` 仍用旧值成功登录，并因重名与
    /// 原有 session 互相顶号进入无限重连循环」。
    env_owned: HashSet<String>,
}

static ENV_STORE: OnceLock<RwLock<EnvStore>> = OnceLock::new();

fn env_store() -> &'static RwLock<EnvStore> {
    ENV_STORE.get_or_init(|| RwLock::new(EnvStore::default()))
}

/// 锁中毒时取出内部值继续用：凭据查找在配置解析与日志路径上，
/// 不该因为别的线程 panic 过就把整个客户端带崩。
fn read_lock(lock: &RwLock<EnvStore>) -> RwLockReadGuard<'_, EnvStore> {
    lock.read().unwrap_or_else(|e| e.into_inner())
}

fn write_lock(lock: &RwLock<EnvStore>) -> RwLockWriteGuard<'_, EnvStore> {
    lock.write().unwrap_or_else(|e| e.into_inner())
}

/// 凭据占位符的变量查找：归 .env 管辖的键只认内存态快照，其余键回退真实进程环境。
///
/// 两条分支各自保证的语义：
/// - `env_owned` 命中 → 只查 `values`，**绝不回退** `std::env::var`。这样运行时
///   刷新的新值能生效，且从 .env 删键会立即失效（否则启动时 `set_var` 写进
///   真实环境的旧值会被回退分支命中，删除操作形同虚设）。
/// - `env_owned` 未命中 → 回退 `std::env::var`。保证 setx/系统变量不被 .env 覆盖
///   （这类键记在 `system_owned`、从不进 `env_owned`），且测试或外部工具直接用
///   `std::env::set_var` 设的变量仍然可见。
///
/// 大小写敏感（HashMap/HashSet 语义，向 Linux 看齐）。已核对现有 profiles 的占位符
/// 与 .env 键精确匹配；回退分支在 Windows 上仍大小写不敏感，故无行为回退。
pub fn lookup_credential_var(key: &str) -> Option<String> {
    {
        let store = read_lock(env_store());
        if store.env_owned.contains(key) {
            return store.values.get(key).cloned();
        }
    }
    std::env::var(key).ok()
}

/// 解析 dotenv 文本为键值对，行级问题（缺 `=`、非法变量名）带行号写入 warns。
///
/// 只做语法解析，**不做**「真实环境优先」判断 —— 那是 load/reload 各自的策略。
fn parse_env_content(content: &str, warns: &mut Vec<String>) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for (idx, raw_line) in content.lines().enumerate() {
        // trim_start_matches 处理记事本等工具写入的 UTF-8 BOM（仅首行可能带）
        let line = raw_line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            warns.push(format!(".env 第 {} 行缺少 '='，已跳过", idx + 1));
            continue;
        };
        let key = key.trim();
        if !is_env_var_name(key) {
            warns.push(format!(
                ".env 第 {} 行变量名 '{}' 非法，已跳过",
                idx + 1,
                key
            ));
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
        pairs.push((key.to_string(), value));
    }
    pairs
}

/// 启动期加载 dotenv 格式的凭据文件（约定路径 `<profiles目录>/.env`）：
/// 同时写入进程环境与内存态 [`EnvStore`]。
///
/// 规则：
/// - 语法解析见 [`parse_env_content`]（`KEY=VALUE`、引号剥离、BOM/注释/非法行）
/// - 真实环境变量优先：同名变量已存在时不覆盖（setx/系统变量 > .env），
///   该键记入 `system_owned`，此后运行时刷新也不会改动它
///
/// 返回实际写入的变量数量。调用方须保证在解析 profile（`${VAR}` 展开）之前执行。
///
/// 这里调用 `set_var` 是安全的，因为本函数只在启动期单线程执行（`AppConfig::load_default`）。
/// **运行期刷新必须走 [`reload_env_file`]**，它绝不触碰进程环境。
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
    let mut warns = Vec::new();
    let pairs = parse_env_content(&content, &mut warns);
    for w in &warns {
        eprintln!("警告: {}", w);
    }

    let mut loaded = 0;
    let mut store = write_lock(env_store());
    for (key, value) in pairs {
        // 真实环境优先：已被 setx/系统设置的同名变量不被 .env 覆盖。
        // 记入 system_owned 而不只是跳过，否则后续 reload 会把它当成 .env 自有键刷新
        if std::env::var_os(&key).is_some() {
            store.system_owned.insert(key);
            continue;
        }
        std::env::set_var(&key, &value);
        // 登记归 .env 管辖：即使日后从 .env 删掉，lookup 也不再回退到
        // 上面这行 set_var 写进真实环境的残留值
        store.env_owned.insert(key.clone());
        store.values.insert(key, value);
        loaded += 1;
    }
    loaded
}

/// 运行时重读 .env，只刷新内存态 [`EnvStore`]，**绝不触碰进程环境**。
///
/// 为什么不用 `set_var`：Linux/glibc 的 `setenv` 会 realloc `environ` 数组，而
/// `/profile load` 发生时进程是重度多线程的（tokio runtime、每 session 任务、
/// 每引擎看门狗线程），任何线程此刻的 `getenv`（`chrono::Local::now` 触发的 tzset、
/// 脚本的 `os.getenv`）都可能读到已释放的指针 → SIGSEGV。Windows 侧虽只表现为
/// 「读不到新值」（`SetEnvironmentVariableW` 只改 Win32 块，CRT `_environ` 快照不动），
/// 但两端统一走内存表可彻底消除这类平台差异。
///
/// 返回「新增或值发生变化」的键数量：内容未变时返回 0，避免每次 `/profile load` 都刷屏。
/// 行级解析告警追加到 `warns`，由调用方经终端 UI 展示（raw mode 下 stderr 不可见）。
///
/// 从 .env **删除某行**会立即生效（无需重启）：被删的键仍留在 `env_owned` 中，
/// [`lookup_credential_var`] 因此不再回退到启动时 `set_var` 写进真实环境的残留值，
/// 展开失败会由调用方（`/profile load`）转为「已中止加载」的可操作提示。
///
/// **已知限制**：删掉整个文件不适用上述结论。本函数读不到文件时只告警并返回 0，
/// 不改动 `values`（故意如此：把「读失败」当成「已清空」会在权限/编码故障时误删全部凭据），
/// 而调用方 `/profile load` 又以 `path.exists()` 做了前置门控 —— 于是旧快照会一直用到重启。
pub fn reload_env_file(path: &Path, warns: &mut Vec<String>) -> usize {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            warns.push(format!(
                "无法读取 {} ({})，若为编码问题请以 UTF-8 保存",
                path.display(),
                e
            ));
            return 0;
        }
    };
    let pairs = parse_env_content(&content, warns);

    let mut store = write_lock(env_store());
    // 整体重建 values：.env 里删掉的键不再由内存表提供，且因仍留在 env_owned 中，
    // 也不会被 std::env::var 的启动期残留兜住 —— 删键立即生效
    let mut values = HashMap::new();
    let mut changed = 0;
    for (key, value) in pairs {
        // system_owned 启动后不再重算：真实环境不会自行变化，
        // 重算反而会让「先 setx 后写 .env」的键在两路径下表现不一致
        if store.system_owned.contains(&key) {
            continue;
        }
        if store.values.get(&key) != Some(&value) {
            changed += 1;
        }
        // env_owned 只增不减，见字段文档
        store.env_owned.insert(key.clone());
        values.insert(key, value);
    }
    store.values = values;
    changed
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

    /// 钉死两条默认值来源：serde 的 `#[serde(default = "default_*")]` 与手写的
    /// `impl Default for ConnectionConfig`。两侧都调用同一批 `default_*()`，改函数体
    /// 会同时生效；本测试拦的是「单侧分叉」——某字段被换成另一个 `default_*()`、
    /// 或被改成独立字面量。name/host/port 无 serde 默认值，不参与比对。
    #[test]
    fn test_default_impl_matches_serde_defaults() {
        // 只给必填字段，其余全走 serde 默认值函数（形状照抄 test_connection_config_deserialize）
        let toml_str = r#"
            name = "dflt"
            host = "dflt.example.com"
            port = 4000
        "#;
        let from_toml = ConnectionConfig::from_toml_str(toml_str).unwrap();
        let d = ConnectionConfig::default();

        assert_eq!(from_toml.encoding, d.encoding, "encoding 默认值分叉");
        assert_eq!(from_toml.script, d.script, "script 默认值分叉");
        assert_eq!(
            from_toml.auto_connect, d.auto_connect,
            "auto_connect 默认值分叉"
        );
        assert_eq!(
            from_toml.auto_reconnect, d.auto_reconnect,
            "auto_reconnect 默认值分叉"
        );
        assert_eq!(
            from_toml.reconnect_delay_secs, d.reconnect_delay_secs,
            "reconnect_delay_secs 默认值分叉"
        );
        assert_eq!(from_toml.username, d.username, "username 默认值分叉");
        assert_eq!(from_toml.password, d.password, "password 默认值分叉");
        assert_eq!(
            from_toml.socks5_enable, d.socks5_enable,
            "socks5_enable 默认值分叉"
        );
        assert_eq!(
            from_toml.socks5_host, d.socks5_host,
            "socks5_host 默认值分叉"
        );
        assert_eq!(
            from_toml.socks5_port, d.socks5_port,
            "socks5_port 默认值分叉"
        );
        assert_eq!(
            from_toml.socks5_username, d.socks5_username,
            "socks5_username 默认值分叉"
        );
        assert_eq!(
            from_toml.socks5_password, d.socks5_password,
            "socks5_password 默认值分叉"
        );
        assert_eq!(
            from_toml.log_rotation_count, d.log_rotation_count,
            "log_rotation_count 默认值分叉"
        );
        assert_eq!(
            from_toml.render_interval, d.render_interval,
            "render_interval 默认值分叉"
        );
        assert_eq!(from_toml.realtime, d.realtime, "realtime 默认值分叉");
        assert_eq!(
            from_toml.connect_delay_ms, d.connect_delay_ms,
            "connect_delay_ms 默认值分叉"
        );
        assert_eq!(
            from_toml.cmd_interval_ms, d.cmd_interval_ms,
            "cmd_interval_ms 默认值分叉"
        );
        assert_eq!(from_toml.burst_size, d.burst_size, "burst_size 默认值分叉");
        assert_eq!(
            from_toml.cmds_per_sec, d.cmds_per_sec,
            "cmds_per_sec 默认值分叉"
        );
        assert_eq!(
            from_toml.window_limit, d.window_limit,
            "window_limit 默认值分叉"
        );
        assert_eq!(
            from_toml.window_duration_ms, d.window_duration_ms,
            "window_duration_ms 默认值分叉"
        );
        assert_eq!(
            from_toml.reconnect_max_secs, d.reconnect_max_secs,
            "reconnect_max_secs 默认值分叉"
        );
        assert_eq!(
            from_toml.idle_timeout_secs, d.idle_timeout_secs,
            "idle_timeout_secs 默认值分叉"
        );
        assert_eq!(
            from_toml.heartbeat_cmd, d.heartbeat_cmd,
            "heartbeat_cmd 默认值分叉"
        );
        assert_eq!(
            from_toml.heartbeat_timeout_secs, d.heartbeat_timeout_secs,
            "heartbeat_timeout_secs 默认值分叉"
        );
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
        ConnectionConfig::from_toml_str_with_warnings(toml_str, &mut warns, &mut Vec::new())
            .unwrap();
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
        // 告警收集：缺失变量的信息交给调用方决定输出渠道（启动 eprintln / TUI 终端 UI）
        let toml_str = r#"
            name = "warned"
            host = "example.com"
            port = 4000
            password = "${RLM_TEST_WARN_MISSING_VAR}"
        "#;
        let mut warns = Vec::new();
        let mut missing = Vec::new();
        let cfg = ConnectionConfig::from_toml_str_with_warnings(toml_str, &mut warns, &mut missing)
            .unwrap();
        assert_eq!(warns.len(), 1);
        assert!(warns[0].contains("RLM_TEST_WARN_MISSING_VAR"));
        assert!(warns[0].contains("warned"), "告警应含角色名便于定位");
        assert_eq!(cfg.password, None);
        // missing 与 warns 并行上报：前者供运行时调用方判定是否中止加载
        assert_eq!(
            missing,
            vec![CredentialMiss {
                field: "password",
                var: "RLM_TEST_WARN_MISSING_VAR".to_string()
            }]
        );
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

    // ===== 运行时 .env 刷新（EnvStore）=====
    //
    // 这批测试针对的线上事故：.env 原先只在启动时读一次，启动后新写入的键
    // 对进程不可见 → /profile load 把凭据置 None 并静默建 session → Lua 侧
    // char_name 为 nil → 脚本顶层拼接时崩溃。
    //
    // 注意：这些用例**故意不清空全局 EnvStore**。清空会与其他测试的
    // 「load → lookup」窗口竞争（尤其 system_owned 被抹后系统键会被误当成
    // .env 自有键刷新），反而引入 flaky。隔离靠每个用例独占的 RLM_ENV_R_* 键名。

    #[test]
    fn test_reload_env_file_picks_up_key_added_after_startup() {
        // 主回归用例：复现「启动后才往 .env 里加键」的线上时序
        let dir = TempDir::new().unwrap();
        let env_path = dir.path().join(".env");
        // 启动瞬间 .env 里只有 A
        fs::write(&env_path, "RLM_ENV_R_A=pw-a\n").unwrap();
        assert_eq!(load_env_file(&env_path), 1);
        assert_eq!(lookup_credential_var("RLM_ENV_R_B"), None, "B 此时应不可见");

        // 启动之后才追加 B
        fs::write(&env_path, "RLM_ENV_R_A=pw-a\nRLM_ENV_R_B=pw-b\n").unwrap();
        let mut warns = Vec::new();
        assert_eq!(
            reload_env_file(&env_path, &mut warns),
            1,
            "新增键应计入更新数"
        );
        assert!(warns.is_empty(), "合法内容不应告警，实际 {:?}", warns);

        assert_eq!(
            lookup_credential_var("RLM_ENV_R_B").as_deref(),
            Some("pw-b")
        );
        // 端到端：占位符现在能展开，而不是 Err(变量名)
        assert_eq!(
            expand_credential_placeholder("${RLM_ENV_R_B}"),
            Ok("pw-b".to_string())
        );
    }

    #[test]
    fn test_reload_env_file_refreshes_changed_value() {
        // 改值同样生效，且必须做到「不碰进程环境」（避开 Linux setenv 竞态）
        let dir = TempDir::new().unwrap();
        let env_path = dir.path().join(".env");
        fs::write(&env_path, "RLM_ENV_R_C=old\n").unwrap();
        load_env_file(&env_path);

        fs::write(&env_path, "RLM_ENV_R_C=new\n").unwrap();
        let mut warns = Vec::new();
        assert_eq!(
            reload_env_file(&env_path, &mut warns),
            1,
            "改值应计入更新数"
        );
        // 内存表优先于启动时 set_var 写进真实环境的旧值
        assert_eq!(lookup_credential_var("RLM_ENV_R_C").as_deref(), Some("new"));
        assert_eq!(
            std::env::var("RLM_ENV_R_C").unwrap(),
            "old",
            "reload 不得调 set_var，否则 Linux 下会与并发 getenv 竞态"
        );
    }

    #[test]
    fn test_reload_env_file_returns_zero_when_unchanged() {
        // 内容未变时返回 0，否则每次 /profile load 都会刷一行「已更新 N 个凭据键」
        let dir = TempDir::new().unwrap();
        let env_path = dir.path().join(".env");
        fs::write(&env_path, "RLM_ENV_R_D=same\n").unwrap();
        load_env_file(&env_path);

        let mut warns = Vec::new();
        assert_eq!(reload_env_file(&env_path, &mut warns), 0);
        // 反复 reload 仍为 0（幂等）
        assert_eq!(reload_env_file(&env_path, &mut warns), 0);
        assert!(warns.is_empty());
    }

    #[test]
    fn test_reload_env_file_does_not_override_system_owned() {
        // setx/系统变量优先的语义在刷新后必须继续成立
        std::env::set_var("RLM_ENV_R_SYS", "from_system");
        let dir = TempDir::new().unwrap();
        let env_path = dir.path().join(".env");
        fs::write(&env_path, "RLM_ENV_R_SYS=from_envfile\n").unwrap();
        assert_eq!(load_env_file(&env_path), 0, "系统键不计入写入数");

        let mut warns = Vec::new();
        assert_eq!(
            reload_env_file(&env_path, &mut warns),
            0,
            "系统键不可被 .env 刷新"
        );
        assert_eq!(
            lookup_credential_var("RLM_ENV_R_SYS").as_deref(),
            Some("from_system")
        );
    }

    #[test]
    fn test_reload_env_file_collects_parse_warnings() {
        // 行级告警走 warns 而非 eprintln：raw mode 下 stderr 不可见
        let dir = TempDir::new().unwrap();
        let env_path = dir.path().join(".env");
        fs::write(&env_path, "RLM_ENV_R_E=ok\nbad name=x\nnoequals\n").unwrap();

        let mut warns = Vec::new();
        assert_eq!(reload_env_file(&env_path, &mut warns), 1, "合法行仍应生效");
        assert_eq!(warns.len(), 2, "两条非法行各一条告警，实际 {:?}", warns);
        assert!(
            warns[0].contains("第 2 行") && warns[0].contains("非法"),
            "首条应指回非法变量名，实际 {:?}",
            warns[0]
        );
        assert!(
            warns[1].contains("第 3 行") && warns[1].contains("缺少"),
            "次条应指回缺等号，实际 {:?}",
            warns[1]
        );
        assert_eq!(lookup_credential_var("RLM_ENV_R_E").as_deref(), Some("ok"));
    }

    #[test]
    fn test_reload_env_file_missing_file_returns_zero() {
        let mut warns = Vec::new();
        let n = reload_env_file(Path::new("nonexistent_rlm_test_dir/.env"), &mut warns);
        assert_eq!(n, 0);
        assert_eq!(warns.len(), 1);
        assert!(
            warns[0].contains("无法读取") && warns[0].contains("UTF-8"),
            "应给出可操作提示（记事本存 GBK 是常见起因），实际 {:?}",
            warns[0]
        );
    }

    /// 主回归：从 .env 删掉某键后必须立即失效，不能被启动期 `set_var` 的残留兜住。
    ///
    /// 2026-09-11 实测事故：删掉 MUD_LPSSX_USER 后 `/profile load lpssx` 仍用旧值
    /// 成功登录，建出第二个同名 session 与原 session 互相顶号，双方 auto_reconnect
    /// 触发无限重连循环（日志 ≈127 KB/min）。
    #[test]
    fn test_reload_env_file_invalidates_key_deleted_from_env() {
        let dir = TempDir::new().unwrap();
        let env_path = dir.path().join(".env");
        fs::write(&env_path, "RLM_ENV_DEL_USER=alice\nRLM_ENV_DEL_KEEP=k\n").unwrap();
        load_env_file(&env_path);
        assert_eq!(
            lookup_credential_var("RLM_ENV_DEL_USER").as_deref(),
            Some("alice")
        );

        // 删掉 USER 行，保留 KEEP 行
        fs::write(&env_path, "RLM_ENV_DEL_KEEP=k\n").unwrap();
        let mut warns = Vec::new();
        reload_env_file(&env_path, &mut warns);

        assert_eq!(
            lookup_credential_var("RLM_ENV_DEL_USER"),
            None,
            "从 .env 删键后必须立即失效，不得回退到 set_var 的残留值"
        );
        assert_eq!(
            std::env::var("RLM_ENV_DEL_USER").unwrap(),
            "alice",
            "残留确实还在进程环境里 —— 是 lookup 主动屏蔽了它，而非它消失了"
        );
        // 展开随之失败，/profile load 才能转为「已中止加载」而不是建出重复 session
        assert!(expand_credential_placeholder("${RLM_ENV_DEL_USER}").is_err());
        // 未被删的键不受影响
        assert_eq!(
            lookup_credential_var("RLM_ENV_DEL_KEEP").as_deref(),
            Some("k")
        );
    }

    /// 删掉后再加回来要能恢复（可带新值），全程不需重启
    #[test]
    fn test_reload_env_file_restores_key_readded_to_env() {
        let dir = TempDir::new().unwrap();
        let env_path = dir.path().join(".env");
        fs::write(&env_path, "RLM_ENV_READD=v1\n").unwrap();
        load_env_file(&env_path);

        fs::write(&env_path, "").unwrap();
        let mut warns = Vec::new();
        reload_env_file(&env_path, &mut warns);
        assert_eq!(lookup_credential_var("RLM_ENV_READD"), None);

        fs::write(&env_path, "RLM_ENV_READD=v2\n").unwrap();
        assert_eq!(reload_env_file(&env_path, &mut warns), 1);
        assert_eq!(
            lookup_credential_var("RLM_ENV_READD").as_deref(),
            Some("v2")
        );
    }

    /// 回退分支不能被 `env_owned` 屏蔽逻辑破坏：从未经 .env 提供的键
    /// （测试直接 set_var 的、用户 setx 的）仍必须可见
    #[test]
    fn test_lookup_credential_var_falls_back_for_keys_never_in_env() {
        std::env::set_var("RLM_ENV_EXT_ONLY", "external");
        assert_eq!(
            lookup_credential_var("RLM_ENV_EXT_ONLY").as_deref(),
            Some("external")
        );
        // 从未定义过的键返回 None，交给调用方转为 CredentialMiss
        assert_eq!(
            lookup_credential_var("RLM_ENV_NEVER_DEFINED_ANYWHERE"),
            None
        );
    }

    #[test]
    fn test_from_toml_str_with_warnings_no_miss_for_literal_credentials() {
        // 字面值凭据不查环境，必须零 missing —— 否则 /profile load 会被中止逻辑误伤
        let toml_str = r#"
            name = "literal"
            host = "example.com"
            port = 4000
            username = "plain-user"
            password = "plain-pw"
        "#;
        let mut warns = Vec::new();
        let mut missing = Vec::new();
        let cfg = ConnectionConfig::from_toml_str_with_warnings(toml_str, &mut warns, &mut missing)
            .unwrap();
        assert!(missing.is_empty(), "字面值不该报缺失，实际 {:?}", missing);
        assert!(warns.is_empty(), "安全配置不该告警，实际 {:?}", warns);
        assert_eq!(cfg.username.as_deref(), Some("plain-user"));
    }

    #[test]
    fn test_from_toml_str_with_warnings_no_miss_when_field_absent() {
        // TOML 里根本没写 username/password 是既有的「留待手动输入」语义，
        // 与「写了 ${VAR} 却查不到」是两回事，绝不能触发中止
        let toml_str = r#"
            name = "nocrd"
            host = "example.com"
            port = 4000
        "#;
        let mut warns = Vec::new();
        let mut missing = Vec::new();
        let cfg = ConnectionConfig::from_toml_str_with_warnings(toml_str, &mut warns, &mut missing)
            .unwrap();
        assert_eq!(cfg.username, None);
        assert_eq!(cfg.password, None);
        assert!(
            missing.is_empty(),
            "字段未配置不等于展开失败，实际 {:?}",
            missing
        );
    }

    // ==================== 模板副本防漂移守卫 ====================
    //
    // profiles/ 下的权威模板与两份一键部署脚本的内嵌副本是同一内容的三份拷贝
    // （bootstrap.sh 用 heredoc、bootstrap.ps1 用 here-string 自带模板，不读仓库）。
    // 改权威源而忘同步副本已实际发生三次：08-29 的 .env 凭据体系只补了 ps1、
    // 09-04 的滑动窗口参数两份都没补，导致 Linux 一键部署出的机器上根本没有
    // .env.example，example.toml 也停在两个功能周期之前。
    //
    // 措辞允许多语言（sh 中文 / ps1 英文），因此按「配置键集合被覆盖」校验，
    // 而不是全文比对：新增一个配置项而漏任一份模板，就是本测试负责拦住的情形。

    const PROFILE_EXAMPLE_TOML: &str = include_str!("../profiles/example.toml");
    const PROFILE_ENV_EXAMPLE: &str = include_str!("../profiles/.env.example");
    const BOOTSTRAP_SH: &str = include_str!("../scripts/bootstrap.sh");
    const BOOTSTRAP_PS1: &str = include_str!("../scripts/bootstrap.ps1");

    /// 收集扁平 TOML 的顶层配置键（跳过注释与空行）
    fn toplevel_toml_keys(src: &str) -> Vec<String> {
        let mut keys: Vec<String> = Vec::new();
        for line in src.lines() {
            let t = line.trim_start();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            if let Some(eq) = t.find('=') {
                let key = t[..eq].trim();
                let valid = !key.is_empty()
                    && !key.contains('.')
                    && key.chars().all(|c| c.is_ascii_lowercase() || c == '_')
                    && !keys.iter().any(|k| k == key);
                if valid {
                    keys.push(key.to_string());
                }
            }
        }
        keys
    }

    /// 模板副本里是否存在 `key = ...` 形式的赋值行
    fn has_toml_assignment(src: &str, key: &str) -> bool {
        let prefix = format!("{key} =");
        src.lines()
            .any(|l| l.trim_start().starts_with(prefix.as_str()))
    }

    /// 收集 .env 示例里非注释态的变量名（仅取 MUD_ 前缀的示例条目）
    fn env_example_var_names(src: &str) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for line in src.lines() {
            let t = line.trim_start();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            if let Some((name, _)) = t.split_once('=') {
                let name = name.trim();
                let valid = name.starts_with("MUD_")
                    && name.len() > 4
                    && name
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                    && !names.iter().any(|n| n == name);
                if valid {
                    names.push(name.to_string());
                }
            }
        }
        names
    }

    /// 模板副本里是否存在 `NAME=value` 形式的条目
    fn has_env_entry(src: &str, name: &str) -> bool {
        let prefix = format!("{name}=");
        src.lines()
            .any(|l| l.trim_start().starts_with(prefix.as_str()))
    }

    /// 两份 bootstrap 脚本，用于统一遍历断言
    fn bootstrap_scripts() -> [(&'static str, &'static str); 2] {
        [
            ("scripts/bootstrap.sh", BOOTSTRAP_SH),
            ("scripts/bootstrap.ps1", BOOTSTRAP_PS1),
        ]
    }

    #[test]
    fn profile_example_toml_keys_have_non_empty_baseline() {
        // 守卫自身依赖的提取逻辑不能默不作声地退化：关键参数必须被解析到
        let keys = toplevel_toml_keys(PROFILE_EXAMPLE_TOML);
        for expect in [
            "name",
            "host",
            "script",
            "password",
            "burst_size",
            "cmds_per_sec",
            "window_limit",
            "window_duration_ms",
        ] {
            assert!(
                keys.iter().any(|k| k == expect),
                "toplevel_toml_keys 未能从 profiles/example.toml 解析出 {expect}，提取逻辑或文件格式已变动"
            );
        }
    }

    #[test]
    fn bootstrap_templates_cover_all_profile_keys() {
        let keys = toplevel_toml_keys(PROFILE_EXAMPLE_TOML);
        assert!(
            !keys.is_empty(),
            "未能从 profiles/example.toml 解析出配置键"
        );
        for (path, script) in bootstrap_scripts() {
            let missing: Vec<&str> = keys
                .iter()
                .filter(|k| !has_toml_assignment(script, k.as_str()))
                .map(String::as_str)
                .collect();
            assert!(
                missing.is_empty(),
                "{path} 内嵌的 example.toml 模板缺少配置键 {missing:?}；\
                 请同步该脚本（权威源：profiles/example.toml）"
            );
        }
    }

    #[test]
    fn bootstrap_templates_create_env_example() {
        let vars = env_example_var_names(PROFILE_ENV_EXAMPLE);
        assert!(
            vars.len() >= 2,
            "未能从 profiles/.env.example 解析出示例变量名"
        );
        for (path, script) in bootstrap_scripts() {
            assert!(
                script.contains(".env.example"),
                "{path} 完全没有生成 .env.example，用它部署的机器上凭据模板会缺失"
            );
            let missing: Vec<&str> = vars
                .iter()
                .filter(|v| !has_env_entry(script, v.as_str()))
                .map(String::as_str)
                .collect();
            assert!(
                missing.is_empty(),
                "{path} 内嵌的 .env.example 模板缺少变量 {missing:?}；\
                 请同步该脚本（权威源：profiles/.env.example）"
            );
        }
    }
}
