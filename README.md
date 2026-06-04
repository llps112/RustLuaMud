# RustLuaMud

基于 Rust + LuaJIT 的终端 MUD 客户端，面向无 GUI 环境下 7×24 小时挂机。

## 特性

- **多连接管理**：单实例同时管理最多 10 个角色连接
- **前台/后台切换**：仅前台连接渲染输出，后台连接静默记录日志
- **ANSI 颜色**：完整支持 ANSI 转义序列，终端彩色显示
- **LuaJIT 脚本**：触发器、别名、定时器、变量管理
- **自动重连**：断线自动重连，可配置延迟
- **日志系统**：按连接分文件记录，支持日志轮转
- **Profile 管理**：从 `profiles/` 目录加载角色配置，自动注入登录凭证
- **GBK 兼容**：自动检测并转码 GBK 编码的 MushClient 脚本
- **极低资源占用**：J1800 + 2GB 内存可流畅运行 10 连接

## 快速开始

### 编译

```bash
cargo build --release
```

### 配置

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

```toml
name = "侠客行"
host = "ln.xkxmud.com"
port = 5555
encoding = "gbk"
script = "scripts/example.lua"
auto_connect = true
auto_reconnect = true
reconnect_delay_secs = 5
username = "角色名"
password = "密码"
```

### 运行

```bash
./target/release/rust-lua-mud
```

## 快捷键

| 快捷键 | 功能 |
|--------|------|
| `Alt+0~9` | 切换前台连接 |
| `Ctrl+C` | 退出程序 |

## 命令

| 命令 | 说明 |
|------|------|
| `/connect <host> <port> <name>` | 动态添加连接 |
| `/disconnect` | 断开当前前台连接 |
| `/close` | 彻底移除当前连接 |
| `/reconnect` | 重连当前连接 |
| `/lua <script>` | 手动加载 Lua 脚本 |
| `/lua reload` | 重新加载当前脚本 |

## Lua API

| API | 说明 | 示例 |
|-----|------|------|
| `send(cmd)` | 发送命令到服务器 | `send("look")` |
| `log(msg)` | 记录日志 | `log("触发成功")` |
| `trigger(pattern, callback)` | 注册触发器 | `trigger("^你好", function() end)` |
| `alias(pattern, callback)` | 注册别名 | `alias("^lh$", function() send("look") end)` |
| `timer(interval, callback)` | 注册定时器（秒） | `timer(30, function() send("hp") end)` |
| `get(key)` | 获取变量 | `get("char_name")` |
| `set(key, value)` | 设置变量 | `set("char_name", "mychar")` |

### 回调参数

- `trigger` 回调：`matches` table，`matches[1]` = 第一个捕获组
- `alias` 回调：`matches` table，`matches[0]` = 原始输入，`matches[1]` = 第一个捕获组

### 脚本编码

脚本文件支持 UTF-8 和 GBK 编码，客户端自动检测。建议新脚本使用 UTF-8。

## 目录结构

```
├── configs/          # 配置文件
├── profiles/         # 角色配置文件（每个角色一个 .toml）
├── scripts/          # Lua 脚本
├── logs/             # 日志文件（按连接分文件）
├── src/
│   ├── main.rs       # 入口
│   ├── app.rs        # 应用主逻辑
│   ├── config.rs     # 配置解析
│   ├── connection/   # 连接管理
│   │   ├── manager.rs
│   │   └── session.rs
│   ├── ui/           # 终端 UI
│   │   └── terminal.rs
│   ├── log/          # 日志系统
│   │   └── logger.rs
│   └── lua/          # Lua 脚本引擎
│       └── engine.rs
└── Cargo.toml
```

## 技术栈

- **异步运行时**：tokio
- **终端控制**：crossterm
- **Lua 引擎**：mlua (LuaJIT)
- **配置解析**：toml + serde
- **编码处理**：encoding_rs
- **正则匹配**：regex
