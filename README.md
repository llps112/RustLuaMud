# RustLuaMud

基于 Rust + LuaJIT 的终端 MUD 客户端，面向无 GUI 环境下 7x24 小时挂机，兼容 MushClient 脚本。

A terminal MUD client built with Rust + LuaJIT, designed for 24/7 headless operation, with MushClient script compatibility.

---

## 特性

- **MushClient 脚本兼容** — 完整实现 MushClient API（AddTrigger/AddAlias/AddTimer/SetStatus/Simulate 等），从 MushClient 拷贝的脚本可无缝运行
- **多连接管理** — 单实例同时管理最多 10 个角色连接，支持前台/后台切换
- **前台/后台渲染** — 仅前台连接渲染终端输出，后台连接静默记录日志
- **ANSI 颜色** — 完整解析 ANSI SGR 转义序列，终端彩色显示
- **LuaJIT 脚本引擎** — 触发器、别名、定时器、变量管理，支持 `wait.lua` 协程库
- **GBK 编码兼容** — 自动检测并转码 GBK 编码的脚本文件和服务器输出；触发器正则支持 GBK 字节模式和 UTF-8 模式
- **SQLite3 集成** — Lua 脚本可直接操作 SQLite3 数据库（地图查询等），支持 GBK 文本解码
- **触发器 w[0] 兼容** — 触发器回调的 `wildcards` 表包含 `w[0]`（完整匹配文本），与 MUSHclient 行为完全一致
- **自动重连** — 断线自动重连，可配置延迟
- **日志系统** — 按连接分文件记录，带时间戳，支持按大小轮转
- **Profile 管理** — 从 `profiles/` 目录加载角色配置（TOML），自动注入登录凭证
- **终端设置持久化** — `keep_command` 等终端选项自动保存到 JSON 文件
- **状态栏** — 实时显示角色名、任务状态、版本号等信息（SetStatus API）
- **Simulate API** — 模拟服务器输出触发 Lua 触发器，支持多行匹配
- **内置命令** — `/connect`、`/disconnect`、`/load`、`/lua`、`/list` 等
- **极低资源占用** — J1800 + 2GB 内存可流畅运行 10 连接

---

## 快速开始

### 编译

```bash
cargo build --release
```

编译产物位于 `target/release/rust-lua-mud`。

### 配置

在 `profiles/` 目录下创建角色配置文件（如 `mychar.toml`）：

```toml
# 角色连接配置
# 文件名即为角色标识，建议用角色名命名

name = "角色名"
host = "ln.xkxmud.com"
port = 5555
encoding = "gbk"

# Lua 脚本路径（相对于程序运行目录）
script = "scripts/example.lua"

# 连接行为
auto_connect = true
auto_reconnect = true
reconnect_delay_secs = 5

# 登录凭证（启动时自动注入 Lua 变量 char_name / char_password）
# 留空则不注入，需手动输入或通过 Lua 脚本 setname/setpwd 设置
username = "your_character_name"
password = "your_password"
```

> `profiles/example.toml` 为示例文件，程序启动时自动跳过，不会加载。

程序启动时会自动扫描 `profiles/` 目录，加载所有 `.toml` 配置文件（`example.toml` 除外），按文件名排序依次连接。

### 运行

```bash
./target/release/rust-lua-mud
```

---

## 快捷键

| 快捷键 | 功能 |
|--------|------|
| `Alt+1~9` | 切换到对应编号的连接 |
| `Alt+0` | 切换到第 10 个连接 |
| `Ctrl+C` / `Ctrl+D` | 退出程序 |

---

## 内置命令

| 命令 | 说明 |
|------|------|
| `/connect <名> <主机:端口>` | 添加并连接新角色 |
| `/connect <名> <主机> <端口>` | 同上 |
| `/disconnect [编号]` | 断开连接（保留 session） |
| `/close [编号]` | 彻底关闭并移除 session |
| `/list` | 列出所有连接及状态 |
| `/load <脚本路径>` | 为前台连接加载 Lua 脚本 |
| `/load reload` | 重新加载前台连接的 Lua 脚本（保留变量状态） |
| `/lua <Lua 代码>` | 直接执行 Lua 代码 |
| `/set keep_command on\|off` | 设置 Enter 后是否保留命令栏输入内容 |

---

## MushClient 兼容 API

### 触发器

| API | 说明 |
|-----|------|
| `AddTrigger(name, match, response, flags, colour, wildcards, sound, script, send_to, sequence)` | 注册触发器 |
| `AddTriggerEx(...)` | 扩展版触发器注册 |
| `DeleteTrigger(name)` | 删除触发器 |
| `EnableTrigger(name, enable)` | 启用/禁用触发器 |
| `EnableTriggerGroup(group, enable)` | 按组启用/禁用触发器 |
| `GetTriggerList()` | 获取触发器名称列表 |
| `GetTriggerInfo(name, code)` | 获取触发器信息 |
| `SetTriggerOption(name, option, value)` | 设置触发器选项 |

**触发器回调签名**: `function(name, line, wildcards)`
- `wildcards[0]` = 完整匹配文本（MUSHclient 兼容）
- `wildcards[1]` = 第一个捕获组，依此类推
- 支持多行触发器（`multi_line` + `lines_to_match`）
- 支持 `omit_from_output` 选项（匹配行不显示到终端）

### 别名

| API | 说明 |
|-----|------|
| `AddAlias(name, match, response, flags, script, sequence)` | 注册别名（支持 `*` 和 `?` 通配符） |
| `DeleteAlias(name)` | 删除别名 |
| `SetAliasOption(name, option, value)` | 设置别名选项 |

**别名回调签名**: `function(name, line, wildcards)`
- `wildcards[0]` = 原始输入
- `wildcards[1]` = 第一个捕获组

### 定时器

| API | 说明 |
|-----|------|
| `AddTimer(name, h, m, s, command, flags, script, sequence)` | 注册定时器 |
| `DeleteTimer(name)` | 删除定时器 |
| `EnableTimer(name, enable)` | 启用/禁用定时器 |
| `EnableTimerGroup(group, enable)` | 按组启用/禁用定时器 |
| `GetTimerList()` | 获取定时器名称列表 |
| `GetTimerInfo(name, code)` | 获取定时器信息 |
| `SetTimerOption(name, option, value)` | 设置定时器选项 |

### 命令与输出

| API | 说明 |
|-----|------|
| `send(cmd)` / `Execute(cmd)` | 发送命令到服务器 |
| `Note(msg)` | 输出文本 |
| `Tell(msg)` | 内联输出 |
| `ColourNote(fg, bg, msg)` | 彩色输出 |
| `log(msg)` | 记录日志 |
| `Simulate(text)` | 模拟服务器输出，触发 Lua 触发器 |

### 变量

| API | 说明 |
|-----|------|
| `GetVariable(name)` | 获取变量 |
| `SetVariable(name, value)` | 设置变量 |
| `DeleteVariable(name)` | 删除变量 |
| `GetVariableList()` | 获取所有变量（key-value 表） |

### 状态栏

| API | 说明 |
|-----|------|
| `SetStatus(text)` | 设置终端底部状态栏文本 |

### 配置与信息

| API | 说明 |
|-----|------|
| `GetInfo(code)` | 获取客户端信息（code=35 返回脚本目录） |
| `SetOption(key, value)` | 设置选项 |
| `GetOption(key)` | 获取选项 |
| `SetAlphaOption(key, value)` | 设置字符串选项 |
| `GetAlphaOption(key)` | 获取字符串选项 |
| `IsConnected()` | 是否已连接 |
| `Connect()` | 请求连接 |
| `Disconnect()` | 请求断开 |
| `GetUniqueNumber()` | 获取唯一编号 |
| `Trim(str)` | 去除首尾空白 |

### 数据库

| API | 说明 |
|-----|------|
| `sqlite3.open(path)` | 打开数据库 |
| `db:exec(sql)` | 执行 SQL |
| `db:prepare(sql)` | 预编译 SQL |
| `stmt:step()` | 执行一步 |
| `stmt:run(...)` | 运行带参数语句 |
| `db:close()` | 关闭数据库 |
| `DatabaseClose(db)` | 兼容 MushClient 的关闭接口 |
| `conn:set_gbk(true)` | 设置数据库文本字段为 GBK 解码 |

### 常量表

| 常量表 | 说明 |
|--------|------|
| `trigger_flag` | 触发器标志位（enabled=1, omit=2, regex=32 等） |
| `alias_flag` | 别名标志位 |
| `timer_flag` | 定时器标志位 |
| `custom_colour` | 自定义颜色 |
| `error_code` / `error_desc` | 错误码与描述 |

### 简写 API

| API | 说明 |
|-----|------|
| `trigger(pattern, callback)` | 快速注册触发器 |
| `alias(pattern, callback)` | 快速注册别名 |
| `timer(interval, callback)` | 快速注册定时器（秒） |
| `get(key)` | 获取变量 |
| `set(key, value)` | 设置变量 |

### 脚本编码

脚本文件支持 UTF-8 和 GBK 编码，客户端自动检测并转码。`dofile()` 和 `load_script()` 自动处理 GBK 转码和 Windows 路径分隔符兼容。

**注意**：`scripts/class/` 目录下的 `.lua` 文件使用 GBK 编码（从 MushClient 直接拷贝的原始脚本），修改时应先编辑 `scripts/class-utf8/` 中的 UTF-8 副本，再用 `iconv` 转码覆盖 GBK 版本。

---

## 数据交换接口（外部程序集成）

客户端内置了 JSON 序列化和配置读写 API，外部程序可通过引擎的 `eval_to_string` 接口实现配置读写，无需直接解析日志或模拟输入。

### 全局 API

| API | 说明 |
|-----|------|
| `json_encode(value)` | 将 Lua 值序列化为 JSON 字符串（支持 nil、boolean、number、string、table 嵌套） |
| `json_decode(json_str)` | 将 JSON 字符串反序列化为 Lua 值 |

### 配置 API

| API | 说明 |
|-----|------|
| `cfg.data()` | 导出完整配置数据，包含字段定义 + 当前值 |
| `cfg.update({...})` | 批量更新配置项，自动类型校验和范围检查 |
| `cfg.save()` | 将当前配置持久化到文件 |

### 调用示例

```rust
// 获取配置
let json = engine.eval_to_string("return json_encode(cfg.data())");

// 修改配置
let result = engine.eval_to_string(
    "return json_encode(cfg.update({idle=true, neili_job=80}))"
);
```

---

## 目录结构

```
├── profiles/              # 角色配置文件（一个 .toml 一个角色）
│   └── example.toml       # 示例配置（自动跳过）
├── scripts/               # Lua 脚本
│   ├── example.lua        # 示例脚本
│   ├── michen_xkx.lua     # 侠客行挂机主脚本入口
│   ├── class/             # MushClient 原始脚本（GBK 编码）
│   ├── class-utf8/        # MushClient 脚本 UTF-8 副本（仅供查阅）
│   └── lua/               # Lua 依赖库（wait.lua 等）
├── logs/                  # 日志文件（按连接分文件）
├── src/
│   ├── main.rs            # 入口
│   ├── app.rs             # 应用主逻辑（终端 UI、命令处理、连接管理）
│   ├── config.rs          # 配置解析（TOML profile 加载）
│   ├── connection/        # 连接管理
│   │   ├── manager.rs     # 连接管理器（多连接、重连）
│   │   └── session.rs     # 单个会话（TCP、Lua 引擎绑定）
│   ├── ui/                # 终端 UI
│   │   ├── terminal.rs    # 终端渲染（屏幕缓冲、状态栏）
│   │   ├── input.rs       # 输入处理
│   │   └── ansi.rs        # ANSI SGR 解析器
│   ├── log/               # 日志系统
│   │   └── logger.rs      # 按连接分文件、大小轮转
│   └── lua/               # Lua 脚本引擎
│       └── engine.rs      # LuaJIT 引擎、MushClient API 实现
├── .github/               # GitHub Actions CI/CD
│   └── workflows/
│       ├── ci.yml         # 自动测试 + clippy + fmt
│       ├── release.yml    # 打 tag 自动发布
│       └── audit.yml      # 每周安全审计
├── xkxMAP.db              # GPS 地图数据库（SQLite3）
└── Cargo.toml
```

---

## 技术栈

| 组件 | 库 |
|------|-----|
| 异步运行时 | tokio |
| 终端控制 | crossterm |
| Lua 引擎 | mlua (LuaJIT) |
| 正则匹配 | regex |
| 数据库 | rusqlite |
| 配置解析 | toml + serde |
| 编码处理 | encoding_rs |
| 日志时间 | chrono |

---

## 系统要求

| 项目 | 要求 |
|------|------|
| 操作系统 | Linux（已测试）/ macOS / Windows（理论上支持） |
| CPU | x86_64 或 aarch64（LuaJIT 需要 JIT 支持的平台） |
| 内存 | 最低 2GB（10 连接流畅运行） |
| 终端 | 支持 UTF-8 和 ANSI 转义序列的终端（如 xterm、GNOME Terminal、iTerm2、Windows Terminal） |
| Rust | 1.70+（edition 2021） |

---

## CI/CD

项目使用 GitHub Actions 实现自动化工作流：

- **CI** — 每次 push/PR 自动运行测试、clippy 检查、fmt 格式化验证
- **Release** — 打 tag 后自动构建并发布二进制
- **Audit** — 每周自动进行依赖安全审计
- **Dependabot** — 依赖自动更新 PR

---

## 版本历史

### v0.1.0 (2026-06-10)

- 完整实现 MushClient 兼容 API（触发器、别名、定时器、变量、数据库等）
- 触发器 `wildcards` 表支持 `w[0]` 完整匹配文本（MUSHclient 兼容）
- 多行触发器支持（`multi_line` + `lines_to_match`）
- GBK 字节模式正则匹配 + UTF-8 正则匹配双模式
- SQLite3 数据库集成，支持 GBK 文本解码
- Simulate API（模拟服务器输出触发触发器）
- SetStatus API（状态栏文本）
- ANSI SGR 解析器（终端彩色显示）
- 终端设置持久化（`keep_command` 选项）
- `/load reload` 保留 Lua 变量状态
- 26+ 单元测试覆盖触发器、别名、配置解析等核心模块
