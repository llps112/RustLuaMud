# RustLuaMud

基于 Rust + LuaJIT 的终端 MUD 客户端，面向无 GUI 环境下 7×24 小时挂机，兼容 MushClient 脚本。

A terminal MUD client built with Rust + LuaJIT, designed for 24/7 headless operation, with MushClient script compatibility.

---

## 特性

- **MushClient 脚本兼容** — 直接实现 MushClient API（AddTrigger/AddAlias/AddTimer 等），从 MushClient 拷贝的脚本可无缝运行
- **多连接管理** — 单实例同时管理最多 10 个角色连接
- **前台/后台切换** — 仅前台连接渲染输出，后台连接静默记录日志
- **ANSI 颜色** — 完整支持 ANSI 转义序列，终端彩色显示
- **LuaJIT 脚本** — 触发器、别名、定时器、变量管理，支持 wait.lua 协程库
- **GBK 兼容** — 自动检测并转码 GBK 编码的脚本文件和服务器输出
- **SQLite3 集成** — Lua 脚本可直接操作 SQLite3 数据库（地图查询等）
- **自动重连** — 断线自动重连，可配置延迟
- **日志系统** — 按连接分文件记录，带时间戳
- **Profile 管理** — 从 `profiles/` 目录加载角色配置，自动注入登录凭证
- **极低资源占用** — J1800 + 2GB 内存可流畅运行 10 连接

---

## 快速开始

### 编译

```bash
cargo build --release
```

### 配置

在 `profiles/` 目录下创建角色配置文件（如 `mychar.toml`）：

```toml
name = "MyCharacter"
host = "ln.xkxmud.com"
port = 5555
encoding = "gbk"
script = "scripts/michen_xkx.lua"
auto_connect = true
auto_reconnect = true
reconnect_delay_secs = 5
username = "your_character_name"
password = "your_password"
```

> `profiles/example.toml` 为示例文件，程序启动时自动跳过，不会加载。

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
| `/load reload` | 重新加载前台连接的 Lua 脚本 |
| `/lua <Lua 代码>` | 直接执行 Lua 代码 |

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

### 别名

| API | 说明 |
|-----|------|
| `AddAlias(name, match, response, flags, script, sequence)` | 注册别名（支持 `*` 和 `?` 通配符） |
| `DeleteAlias(name)` | 删除别名 |
| `SetAliasOption(name, option, value)` | 设置别名选项 |

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

### 变量

| API | 说明 |
|-----|------|
| `GetVariable(name)` | 获取变量 |
| `SetVariable(name, value)` | 设置变量 |
| `DeleteVariable(name)` | 删除变量 |
| `GetVariableList()` | 获取所有变量（key-value 表） |

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

### 回调参数

- `trigger` 回调：`matches` 表，`matches[1]` = 第一个捕获组
- `alias` 回调：`matches` 表，`matches[1]` = 第一个捕获组

### 脚本编码

脚本文件支持 UTF-8 和 GBK 编码，客户端自动检测并转码。`dofile()` 和 `load_script()` 自动处理 GBK 转码和 Windows 路径分隔符兼容。

---

## 目录结构

```
├── profiles/         # 角色配置文件（一个 .toml 一个角色）
├── scripts/          # Lua 脚本
│   └── lua/          # Lua 依赖库（wait.lua 等）
├── class/            # MushClient 脚本（git-crypt 加密存储）
├── logs/             # 日志文件（按连接分文件）
├── src/
│   ├── main.rs       # 入口
│   ├── app.rs        # 应用主逻辑
│   ├── config.rs     # 配置解析
│   ├── connection/   # 连接管理
│   │   ├── manager.rs
│   │   └── session.rs
│   ├── ui/           # 终端 UI
│   │   ├── terminal.rs
│   │   ├── input.rs
│   │   └── ansi.rs
│   ├── log/          # 日志系统
│   │   └── logger.rs
│   └── lua/          # Lua 脚本引擎
│       └── engine.rs
├── .github/          # GitHub Actions CI/CD
│   ├── workflows/
│   │   ├── ci.yml      # 自动测试 + clippy + fmt
│   │   ├── release.yml # 打 tag 自动发布
│   │   └── audit.yml   # 每周安全审计
│   └── dependabot.yml  # 依赖自动更新
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
