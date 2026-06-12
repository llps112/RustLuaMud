# RustLuaMud

基于 Rust + LuaJIT 的终端 MUD 客户端，面向无 GUI 环境下 7x24 小时挂机，兼容 MUSHclient 脚本 API。

A terminal MUD client built with Rust + LuaJIT, designed for 24/7 headless operation, with MUSHclient script API compatibility.

---

## 特性

- **MUSHclient 脚本兼容** — 实现常用 MUSHclient API（AddTrigger / AddAlias / AddTimer / GetInfo / GetTriggerInfo / GetTimerInfo / GetPluginInfo / SetStatus / Simulate / SendPkt / 变量管理等），从 MUSHclient 迁移的脚本可无缝运行
- **多连接管理** — 单实例同时管理最多 10 个角色连接，支持前台/后台切换
- **仅前台渲染** — 仅前台连接渲染终端输出，后台连接静默记录日志
- **ANSI 颜色** — 完整解析 ANSI SGR 转义序列，终端彩色显示
- **LuaJIT 脚本引擎** — 触发器、别名、定时器、变量管理、协程支持
- **GBK 编码兼容** — 自动检测并转码 GBK 编码的脚本文件和服务器输出；触发器同时支持 GBK 字节模式和 UTF-8 正则匹配
- **SQLite3 集成** — Lua 脚本可直接操作 SQLite3 数据库，支持 GBK 文本解码
- **触发器 w[0] 兼容** — 触发器回调的 `wildcards` 表包含 `w[0]`（完整匹配文本），与 MUSHclient 行为完全一致
- **自动重连** — 断线自动重连，可配置延迟
- **日志系统** — 按连接分文件记录，带时间戳
- **Profile 管理** — 从 `profiles/` 目录加载角色配置（TOML），自动注入登录凭证
- **终端设置持久化** — `keep_command` 等终端选项自动保存到 JSON 文件
- **状态栏** — 实时显示角色名、连接状态、版本号等信息（SetStatus API）
- **Simulate API** — 模拟服务器输出触发 Lua 触发器，支持多行匹配
- **内置命令** — `/connect`、`/disconnect`、`/load`、`/lua`、`/list`、`/set` 等
- **极低资源占用** — J1800 + 2GB 内存可流畅运行 10 连接

> **注意**：本客户端仅实现了 MUSHclient 的部分常用 API。如果你的脚本使用了未实现的 API（如 `Accelerator`、`AddFont`、`ArrayCreate` 等），脚本将无法正常运行。使用前请确认脚本中调用的所有 API 都在本项目的兼容范围内。

---

## 快速开始

### 编译

```bash
cargo build --release
```

编译产物位于 `target/release/RustLuaMud`。

### 配置

在 `profiles/` 目录下创建角色配置文件（如 `mychar.toml`）：

```toml
# 角色连接配置
# 文件名即为角色标识，建议用角色名命名

name = "角色名"
host = "mud.example.com"
port = 6666
encoding = "gbk"

# Lua 脚本路径（相对于程序运行目录）
script = "scripts/myscript.lua"

# 连接行为
auto_connect = true
auto_reconnect = true
reconnect_delay_secs = 5

# 登录凭证（启动时自动注入 Lua 变量 char_name / char_password）
# 留空则不注入，需手动输入或通过 Lua 脚本设置
username = "your_character_name"
password = "your_password"
```

> `profiles/example.toml` 为示例文件，程序启动时自动跳过，不会加载。

程序启动时会自动扫描 `profiles/` 目录，加载所有 `.toml` 配置文件（`example.toml` 除外），按文件名排序依次连接。

### 运行

```bash
./target/release/RustLuaMud
```

### 文档

详细文档请见 [help/](help/README.md) 目录，涵盖 Lua API 接口、CLUI 操作指南等。

---

## 快捷键

| 快捷键 | 功能 |
|--------|------|
| `Alt+1~9` | 切换到对应编号的连接 |
| `Alt+0` | 切换到第 10 个连接 |
| `Ctrl+C` / `Ctrl+D` | 退出程序 |
| `↑` / `↓` | 浏览命令历史 |

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

## MUSHclient 兼容 API

### 触发器

| API | 说明 |
|-----|------|
| `AddTrigger(name, match, response, flags, ...)` | 注册触发器 |
| `AddTriggerEx(...)` | 扩展版触发器注册 |
| `DeleteTrigger(name)` | 删除触发器 |
| `EnableTrigger(name, enable)` | 启用/禁用触发器 |
| `EnableTriggerGroup(group, enable)` | 按组启用/禁用触发器 |
| `GetTriggerList()` | 获取触发器名称列表 |
| `GetTriggerInfo(name, code)` | 获取触发器信息 |
| `SetTriggerOption(name, option, value)` | 设置触发器选项 |

**回调签名**: `function(name, line, wildcards)`
- `wildcards[0]` = 完整匹配文本（MUSHclient 兼容）
- `wildcards[1]` = 第一个捕获组，依此类推
- 支持 `omit_from_output` 选项（匹配行不显示到终端）

### 别名

| API | 说明 |
|-----|------|
| `AddAlias(name, match, response, flags, [script])` | 注册别名 |
| `DeleteAlias(name)` | 删除别名 |
| `GetAliasList()` | 获取别名名称列表 |
| `SetAliasOption(name, option, value)` | 设置别名选项 |

**回调签名**: `function(name, line, wildcards)`
- `wildcards[0]` = 原始输入
- `wildcards[1]` = 第一个捕获组

### 定时器

| API | 说明 |
|-----|------|
| `AddTimer(name, h, m, s, command, flags, [script])` | 注册定时器 |
| `DeleteTimer(name)` | 删除定时器 |
| `EnableTimer(name, enable)` | 启用/禁用定时器 |
| `EnableTimerGroup(group, enable)` | 按组启用/禁用定时器 |
| `GetTimerList()` | 获取定时器名称列表 |
| `GetTimerInfo(name, code)` | 获取定时器信息 |
| `SetTimerOption(name, option, value)` | 设置定时器选项 |
| `ResetTimer(name)` | 重置定时器计时 |

### 命令与输出

| API | 说明 |
|-----|------|
| `send(cmd)` / `Execute(cmd)` | 发送命令到服务器 |
| `DiscardQueue()` | 清空命令队列 |
| `Note(msg)` | 输出文本 |
| `Tell(msg)` | 内联输出（不换行） |
| `print(...)` | 标准 Lua print，重定向到输出窗口 |
| `ColourNote(fg, bg, msg)` | 彩色输出 |
| `log(msg)` | 记录日志 |
| `Simulate(text)` | 模拟服务器输出，触发 Lua 触发器 |
| `SetStatus(text)` | 设置终端底部状态栏文本 |
| `DeleteTemporaryTimers()` | 删除所有一次性定时器 |

### 变量

| API | 说明 |
|-----|------|
| `GetVariable(name)` | 获取变量 |
| `SetVariable(name, value)` | 设置变量 |
| `DeleteVariable(name)` | 删除变量 |
| `GetVariableList()` | 获取所有变量列表 |

### 网络

| API | 说明 |
|-----|------|
| `IsConnected()` | 是否已连接 |
| `Connect()` | 请求连接 |
| `Disconnect()` | 请求断开 |
| `OnConnect()` | 连接回调（由 Lua 覆盖实现自定义初始化） |

### 配置与信息

| API | 说明 |
|-----|------|
| `GetInfo(code)` | 获取客户端信息（code=1 主机, 2 端口, 3 连接状态, 35 脚本目录等） |
| `SetOption(key, value)` | 设置选项 |
| `GetOption(key)` | 获取选项 |
| `SetAlphaOption(key, value)` | 设置字符串选项 |
| `GetAlphaOption(key)` | 获取字符串选项 |

### 日志

| API | 说明 |
|-----|------|
| `OpenLog(filename, append)` | 打开日志文件 |
| `IsLogOpen()` | 检查日志是否已打开 |
| `CloseLog()` | 关闭日志文件 |

### 数据库

| API | 说明 |
|-----|------|
| `sqlite3.open(path)` | 打开数据库 |
| `conn:execute(sql)` | 执行 SQL |
| `conn:close()` | 关闭数据库 |
| `conn:set_gbk(enable)` | 设置数据库文本字段为 GBK 解码 |
| `DatabaseClose()` | 兼容 MUSHclient 的关闭接口 |

### 工具函数

| API | 说明 |
|-----|------|
| `GetUniqueNumber()` | 获取唯一递增编号 |
| `Trim(str)` | 去除字符串首尾空白 |
| `MakeRegularExpression(text)` | 将普通文本转义为安全正则 |
| `GetPluginID()` | 获取插件 ID（兼容） |
| `GetPluginInfo(id, code)` | 获取插件信息 |

### 常量表

| 常量表 | 说明 |
|--------|------|
| `trigger_flag` | 触发器标志位 |
| `alias_flag` | 别名标志位 |
| `timer_flag` | 定时器标志位 |
| `custom_colour` | 自定义颜色 |
| `error_code` / `error_desc` | 错误码与描述 |

**trigger_flag**:
| 常量 | 值 | 说明 |
|------|-----|------|
| `Enabled` | 1 | 启用 |
| `OmitFromLog` | 2 | 不记日志 |
| `OmitFromOutput` | 4 | 不显示输出 |
| `KeepEvaluating` | 8 | 继续求值 |
| `IgnoreCase` | 16 | 忽略大小写 |
| `RegularExpression` | 32 | 正则匹配 |
| `ExpandVariables` | 64 | 展开变量 |
| `Replace` | 1024 | 同名替换 |
| `LowercaseWildcard` | 2048 | 通配符小写 |
| `Temporary` | 4096 | 临时 |
| `OneShot` | 8192 | 一次性 |

**alias_flag**:
| 常量 | 值 | 说明 |
|------|-----|------|
| `Enabled` | 1 | 启用 |
| `IgnoreCase` | 16 | 忽略大小写 |
| `RegularExpression` | 32 | 正则匹配 |
| `ExpandVariables` | 64 | 展开变量 |
| `Replace` | 1024 | 同名替换 |
| `Temporary` | 4096 | 临时 |

**timer_flag**:
| 常量 | 值 | 说明 |
|------|-----|------|
| `Enabled` | 1 | 启用 |
| `AtTime` | 4 | 指定时刻触发 |
| `Replace` | 1024 | 同名替换（继承旧定时器禁用状态） |
| `Temporary` | 4096 | 临时 |
| `OneShot` | 8192 | 一次性 |
| `ActiveWhenClosed` | 16384 | 窗口关闭时仍运行 |

### 内部与扩展

| API | 说明 |
|-----|------|
| `dofile(filename)` | 加载并执行 Lua 脚本文件（自动处理 GBK 转码） |
| `rex` | 正则表达式模块 |
| `bit` | 位运算模块（band / bor / bxor / bnot / lshift / rshift） |
| `trigger(name, data)` | 快速注册触发器 |
| `alias(name, data)` | 快速注册别名 |
| `timer(name, data)` | 快速注册定时器 |
| `get(key)` | 获取变量 |
| `set(key, value)` | 设置变量 |
| `json_encode(value)` | 将 Lua 值序列化为 JSON |
| `json_decode(json_str)` | 将 JSON 解析为 Lua 值 |

---

## 目录结构

```
├── profiles/              # 角色配置文件（一个 .toml 一个角色）
│   └── example.toml       # 示例配置（自动跳过）
├── scripts/               # Lua 脚本
│   ├── example.lua        # 示例脚本
│   └── lua/               # Lua 依赖库（wait.lua 等）
├── logs/                  # 日志文件（按连接分文件）
├── help/                  # 客户端文档
│   ├── api/               # Lua API 接口文档
│   └── commands/          # 命令和 CLUI 操作指南
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
│       └── engine.rs      # LuaJIT 引擎、MUSHclient API 实现
├── .github/               # GitHub Actions CI/CD
│   └── workflows/
│       ├── ci.yml         # 自动测试 + clippy + fmt
│       ├── release.yml    # 打 tag 自动发布
│       └── audit.yml      # 每周安全审计
└── Cargo.toml
```

---

## 数据交换接口（外部程序集成）

客户端内置了 JSON 序列化和配置读写 API，外部程序可通过引擎的 `eval_to_string` 接口实现数据交互，无需直接解析日志或模拟输入。

### JSON 序列化

| API | 说明 |
|-----|------|
| `json_encode(value)` | 将 Lua 值序列化为 JSON 字符串（支持 nil、boolean、number、string、table 嵌套） |
| `json_decode(json_str)` | 将 JSON 字符串反序列化为 Lua 值 |

### 调用示例（Rust）

```rust
// 获取数据
let json = engine.eval_to_string("return json_encode(my_table)");

// 解析 JSON 到 Lua
let result = engine.eval_to_string("return json_decode('{\"key\":\"value\"}')");
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
| 内存 | 最低 512MB（基础使用），2GB 推荐（10 连接） |
| 终端 | 支持 UTF-8 和 ANSI 转义序列的终端（如 xterm、GNOME Terminal、iTerm2、Windows Terminal） |
| Rust | 1.70+（edition 2021） |

## 故障排查

开发阶段已硬编码启用 `RUST_BACKTRACE=1`，panic 时会自动打印堆栈信息。正式版发布前会移除此设置，届时如需调试可手动设置：

```bash
export RUST_BACKTRACE=1
./RustLuaMud
```

---

## CI/CD

项目使用 GitHub Actions 实现自动化工作流：

- **CI** — 每次 push/PR 自动运行测试、clippy 检查、fmt 格式化验证
- **Release** — 打 tag 后自动构建并发布二进制
- **Audit** — 每周自动进行依赖安全审计
- **Dependabot** — 依赖自动更新 PR

---

## 版本历史

### v0.1.1 (2026-06-10)

- 新增 `help/` 文档目录，涵盖 Lua API、CLUI 操作指南等 18 个文档
- 修复 `AddTimer` Replace 标志不继承旧定时器禁用状态的问题
- 重构 `OnConnect` 回调抽象接口，替代直接调用 `alias.atconnect`
- 修复连接初始化时命令队列未及时发送的问题
- 清理调试输出和游戏脚本耦合内容

### v0.1.0 (2026-06-10)

- 完整实现 MUSHclient 兼容 API（触发器、别名、定时器、变量、数据库等）
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
