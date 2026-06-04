# RustLuaMud

基于 Rust + LuaJIT 的终端 MUD 客户端，面向无 GUI 环境下 7×24 小时挂机。

A terminal MUD client built with Rust + LuaJIT, designed for 24/7 headless operation.

---

## 特性 / Features

- **多连接管理** — 单实例同时管理最多 10 个角色连接
- **Multi-Connection** — Manage up to 10 character connections in a single instance
- **前台/后台切换** — 仅前台连接渲染输出，后台连接静默记录日志
- **Foreground/Background** — Only foreground connection renders; background connections log silently
- **ANSI 颜色** — 完整支持 ANSI 转义序列，终端彩色显示
- **ANSI Colors** — Full ANSI escape sequence support for terminal color rendering
- **LuaJIT 脚本** — 触发器、别名、定时器、变量管理
- **LuaJIT Scripting** — Triggers, aliases, timers, and variable management
- **自动重连** — 断线自动重连，可配置延迟
- **Auto-Reconnect** — Automatic reconnection with configurable delay
- **日志系统** — 按连接分文件记录，支持日志轮转
- **Logging** — Per-connection log files with rotation support
- **Profile 管理** — 从 `profiles/` 目录加载角色配置，自动注入登录凭证
- **Profile Management** — Load character configs from `profiles/` directory with auto credential injection
- **GBK 兼容** — 自动检测并转码 GBK 编码的 MushClient 脚本
- **GBK Compatible** — Auto-detect and decode GBK-encoded MushClient scripts
- **极低资源占用** — J1800 + 2GB 内存可流畅运行 10 连接
- **Minimal Resource Usage** — Runs 10 connections smoothly on J1800 + 2GB RAM

---

## 快速开始 / Quick Start

### 编译 / Build

```bash
cargo build --release
```

### 配置 / Configuration

编辑 `configs/default.toml`：

```toml
[general]
scroll_buffer = 5000
log_dir = "logs"
profile_dir = "profiles"
log_rotation_size_mb = 10
log_rotation_count = 5
```

在 `profiles/` 目录下创建角色配置文件（如 `mychar.toml`）：

Create character profile files in `profiles/` (e.g., `mychar.toml`):

```toml
name = "MyCharacter"
host = "ln.xkxmud.com"
port = 5555
encoding = "gbk"
script = "scripts/example.lua"
auto_connect = true
auto_reconnect = true
reconnect_delay_secs = 5
username = "your_character_name"
password = "your_password"
```

### 运行 / Run

```bash
./target/release/rust-lua-mud
```

---

## 快捷键 / Shortcuts

| 快捷键 / Shortcut | 功能 / Function |
|--------|------|
| `Alt+0~9` | 切换前台连接 / Switch foreground connection |
| `Ctrl+C` | 退出程序 / Quit |

---

## 命令 / Commands

| 命令 / Command | 说明 / Description |
|------|------|
| `/connect <host> <port> <name>` | 动态添加连接 / Add connection dynamically |
| `/disconnect` | 断开当前前台连接 / Disconnect foreground connection |
| `/close` | 彻底移除当前连接 / Remove connection entirely |
| `/reconnect` | 重连当前连接 / Reconnect current connection |
| `/lua <script>` | 手动加载 Lua 脚本 / Load Lua script manually |
| `/lua reload` | 重新加载当前脚本 / Reload current script |

---

## Lua API

| API | 说明 / Description | 示例 / Example |
|-----|------|------|
| `send(cmd)` | 发送命令到服务器 / Send command to server | `send("look")` |
| `log(msg)` | 记录日志 / Log message | `log("triggered")` |
| `trigger(pattern, callback)` | 注册触发器 / Register trigger | `trigger("^你好", function() end)` |
| `alias(pattern, callback)` | 注册别名 / Register alias | `alias("^lh$", function() send("look") end)` |
| `timer(interval, callback)` | 注册定时器（秒）/ Register timer (seconds) | `timer(30, function() send("hp") end)` |
| `get(key)` | 获取变量 / Get variable | `get("char_name")` |
| `set(key, value)` | 设置变量 / Set variable | `set("char_name", "mychar")` |

### 回调参数 / Callback Arguments

- `trigger` 回调 / callback: `matches` table, `matches[1]` = 第一个捕获组 / first capture group
- `alias` 回调 / callback: `matches` table, `matches[0]` = 原始输入 / original input, `matches[1]` = 第一个捕获组 / first capture group

### 脚本编码 / Script Encoding

脚本文件支持 UTF-8 和 GBK 编码，客户端自动检测。建议新脚本使用 UTF-8。

Script files support UTF-8 and GBK encoding with auto-detection. UTF-8 is recommended for new scripts.

---

## 目录结构 / Directory Structure

```
├── configs/          # 配置文件 / Configuration files
├── profiles/         # 角色配置文件 / Character profiles (one .toml per character)
├── scripts/          # Lua 脚本 / Lua scripts
├── logs/             # 日志文件 / Log files (per-connection)
├── src/
│   ├── main.rs       # 入口 / Entry point
│   ├── app.rs        # 应用主逻辑 / Application main logic
│   ├── config.rs     # 配置解析 / Configuration parsing
│   ├── connection/   # 连接管理 / Connection management
│   │   ├── manager.rs
│   │   └── session.rs
│   ├── ui/           # 终端 UI / Terminal UI
│   │   └── terminal.rs
│   ├── log/          # 日志系统 / Logging system
│   │   └── logger.rs
│   └── lua/          # Lua 脚本引擎 / Lua script engine
│       └── engine.rs
└── Cargo.toml
```

---

## 技术栈 / Tech Stack

- **异步运行时 / Async Runtime**: tokio
- **终端控制 / Terminal**: crossterm
- **Lua 引擎 / Lua Engine**: mlua (LuaJIT)
- **配置解析 / Config Parsing**: toml + serde
- **编码处理 / Encoding**: encoding_rs
- **正则匹配 / Regex**: regex
