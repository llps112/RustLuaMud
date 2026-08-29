# RustLuaMud

基于 Rust + LuaJIT 的终端 MUD 客户端，面向 7x24 小时无 GUI 挂机场景，兼容 MUSHclient 脚本 API。

---

## 特性

**MUSHclient 兼容层**
- 常用 API 全覆盖：触发器、别名、定时器、变量、日志、数据库、样式查询
- 触发器 `wildcards[0]`（完整匹配文本）与 MUSHclient 行为完全一致
- 多行触发器、颜色样式回调（`GetStyle`）、模拟输出（`Simulate`）
- 参考 `help/api/` 目录查阅完整 API 文档

**脚本引擎**
- LuaJIT 引擎，协程支持（`wait.make` / `wait.time`）
- `dofile` 自动处理 GBK 转码
- 内置 JSON 序列化、正则（`rex`）、位运算（`bit`）
- SQLite3 集成，支持 GBK 文本解码

**连接管理**
- 单实例最多 10 个并发连接，前台/后台无缝切换
- 每个角色独立配置 SOCKS5 代理，支持多开规避同 IP 限制
- 自动重连，可配置延迟
- 仅前台渲染，后台静默记录日志

**限速保护**
- Rust 侧令牌桶限速（burst_size + cmds_per_sec + min_interval 三参数）
- 默认：突发 10 条、每秒 20 条、最小间隔 50ms，确保不触发服务器反 flood 机制

**编码兼容**
- GBK / UTF-8 双编码，自动检测并转码
- 触发器同时支持 GBK 字节模式与 UTF-8 正则匹配

**终端体验**
- 完整 ANSI SGR 解析，彩色输出
- PageUp/PageDown 翻页查看历史输出
- 鼠标点击状态栏切换连接
- 浮动面板：Lua 侧 `SetPanel`/`RemovePanel`/`RegisterPanelHandler` API，支持自定义数据展示和按钮交互
- 长行自动换行，CJK 宽字符正确对齐
- 极低资源占用：J1800 + 2GB 内存即可流畅运行 10 连接

**部署运维**
- 守护进程模式：`--daemon` fork + setsid，ssh 断开不中断，配合 `--daemon stop/status` 管理，适合 7x24 挂机（Linux / macOS）
- Windows 一键部署：`bootstrap.ps1` 免管理员权限，默认安装到 `%USERPROFILE%\RustLuaMud`

---

## 快速开始

两种安装方式：

| 场景 | 推荐 |
|------|------|
| x86_64 / i686 Linux，即下即用 | [预编译二进制](#方式一下载预编译二进制) |
| Windows 10/11 64 位，即下即用 | [Windows 一键部署](#windows-一键部署) |
| ARM64 / 需要改客户端代码 | [从源码编译](#方式二从源码编译) |

> **Windows 说明**：Windows 平台已提供预编译 `RustLuaMud-windows-x86_64.zip`（stable 与 nightly 均附带），**仅支持 64 位系统**，暂不支持 32 位与 ARM64。本项目的 ANSI 样式、CJK 对齐与浮动面板依赖现代终端的 VT/ANSI 支持：**最低 Windows 10 version 1511**（保底可运行），**推荐 1809 及以上**（体验完整）；Win7/8/XP 不支持。终端推荐使用 Windows Terminal（传统 conhost 已做宽度自适应，但滚动条等固有缺陷无法完全消除）。

> **国内镜像加速**：`--gitee` 参数从 Gitee 下载，支持 stable 和 nightly 两种版本：
> ```bash
> # 稳定版（推荐）
> bash <(curl -Ls https://gitee.com/bai-yifei180/RustLuaMud/raw/main/scripts/bootstrap.sh) --gitee
>
> # Nightly 版
> bash <(curl -Ls https://gitee.com/bai-yifei180/RustLuaMud/raw/main/scripts/bootstrap.sh) --nightly --gitee
> ```

### 方式一：下载预编译二进制

一键初始化脚本，自动创建目录、下载二进制、生成示例配置：

```bash
# 稳定版（推荐）
bash <(curl -Ls https://raw.githubusercontent.com/llps112/RustLuaMud/main/scripts/bootstrap.sh)

# Nightly 版（main 分支最新构建，可能不稳定）
bash <(curl -Ls https://raw.githubusercontent.com/llps112/RustLuaMud/main/scripts/bootstrap.sh) --nightly
```

初始化后目录结构：

```
~/RustLuaMud/
├── RustLuaMud           # 主程序
├── profiles/            # 角色 TOML 配置文件
│   └── example.toml     # 示例配置
├── scripts/             # Lua 脚本
│   └── example.lua      # 示例脚本
└── logs/                # 日志文件自动生成
```

配置角色并启动：

```bash
cd ~/RustLuaMud
cp profiles/example.toml profiles/mychar.toml
vim profiles/mychar.toml
./RustLuaMud
```

配置项说明见[配置](#配置)章节。

#### Windows 一键部署

在 PowerShell 中执行（默认安装到 `%USERPROFILE%\RustLuaMud`，免管理员权限）：

```powershell
iwr https://raw.githubusercontent.com/llps112/RustLuaMud/main/scripts/bootstrap.ps1 -UseBasicParsing | iex
```

需要 Gitee 加速、nightly 版本或自定义安装目录时，先下载脚本再带参数执行：

```powershell
iwr https://gitee.com/bai-yifei180/RustLuaMud/raw/main/scripts/bootstrap.ps1 -UseBasicParsing -OutFile bootstrap.ps1
Unblock-File bootstrap.ps1
.\bootstrap.ps1 -Gitee               # Gitee 稳定版
.\bootstrap.ps1 -Nightly -Gitee      # Gitee Nightly
.\bootstrap.ps1 D:\Games\RustLuaMud  # 自定义安装目录
```

初始化后目录结构：

```
%USERPROFILE%\RustLuaMud\
├── RustLuaMud.exe         # 主程序
├── start_mud.bat          # 双击启动器（自动钉死工作目录）
├── profiles\              # 角色 TOML 配置文件
│   └── example.toml       # 示例配置
├── scripts\               # Lua 脚本（放入你自己的游戏脚本）
│   └── example.lua        # 示例脚本
└── logs\                  # 日志文件自动生成
```

编辑 `profiles\` 下的角色配置后，双击 `start_mud.bat` 或在安装目录运行 `.\RustLuaMud.exe` 启动。游戏脚本不随安装包分发，放入 `scripts\` 目录并在配置中用 `script = "scripts/xxx.lua"` 引用即可。

> Nightly 版由 [nightly.yml](.github/workflows/nightly.yml) 自动构建，每次 push main 分支后自动更新。构建完成后自动同步到 [Gitee Release](https://gitee.com/bai-yifei180/RustLuaMud/releases)。支持 Linux x86_64 / i686 与 Windows x86_64 三种产物。

### 方式二：从源码编译

#### 安装 Rust

```bash
# 国内镜像安装（清华源）
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o rustup-init.sh
sed -i 's|static.rust-lang.org/rustup|mirrors.tuna.tsinghua.edu.cn/rustup/rustup|' rustup-init.sh
RUSTUP_DIST_SERVER=https://mirrors.tuna.tsinghua.edu.cn/rustup sh rustup-init.sh -y
source $HOME/.cargo/env

# 海外直连
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source $HOME/.cargo/env
```

> 编译需要 C 编译器：`sudo apt install build-essential`（Debian/Ubuntu）或 `sudo dnf groupinstall "Development Tools"`（Fedora/CentOS）。

#### 配置依赖镜像（国内）

写入 `~/.cargo/config.toml`：

```toml
[build]
jobs = 2

[source.crates-io]
replace-with = "sjtug"

[source.sjtug]
registry = "sparse+https://mirrors.sjtug.sjtu.edu.cn/crates.io-index/"
```

备选源：`mirrors.ustc.edu.cn`、`mirrors.tuna.tsinghua.edu.cn`、`repo.huaweicloud.com`。

#### 编译与运行

```bash
git clone https://github.com/llps112/RustLuaMud.git   # 或 Gitee: https://gitee.com/bai-yifei180/RustLuaMud.git
cd RustLuaMud
cargo build --release
./target/release/RustLuaMud
```

#### 多实例运行

```bash
# 实例一（默认 profiles/ 目录）
./target/release/RustLuaMud

# 实例二（使用不同配置目录）
./target/release/RustLuaMud --profiles profiles2
```

#### 守护进程模式

```bash
# 启动守护进程（fork + setsid，ssh 断开不中断，适合 7x24 挂机）
./target/release/RustLuaMud --daemon

# 查询状态 / 停止（PID 文件位于 profiles 目录）
./target/release/RustLuaMud --daemon status
./target/release/RustLuaMud --daemon stop
```

> daemon 模式同样支持 `--profiles`，每个配置目录独立一个守护进程（各自的 `daemon.pid`）。
> 停止通过 SIGTERM 优雅退出；前台模式收到 SIGTERM 也会优雅退出。
> `--daemon` 仅 Linux / macOS 提供；Windows 为前台运行，可借助任务计划程序或 `start_mud.bat` 实现开机自启。

---

## 配置

程序启动时自动扫描 `profiles/` 目录，加载所有 `.toml` 配置文件（`example.toml` 除外）。

完整配置项：

```toml
# 角色连接配置
name = "角色名"
host = "mud.example.com"
port = 6666
encoding = "gbk"

# Lua 脚本路径（相对于运行目录）
script = "scripts/myscript.lua"

# 连接行为
auto_connect = true
auto_reconnect = true
reconnect_delay_secs = 5

# 连接建立后延迟发送命令的毫秒数，默认 1000
# OnConnect() 立即执行，仅延迟后续命令的发送
# connect_delay_ms = 1000

# 登录凭证（自动注入 Lua 变量 char_name / char_password）
# 支持环境变量占位符避免明文，见下方“凭据安全保存”
username = "your_character_name"
password = "your_password"

# SOCKS5 代理（可选）
socks5_enable = false
socks5_host = "127.0.0.1"
socks5_port = 1080
# socks5_username = "user"
# socks5_password = "pass"

# 命令发送令牌桶限速（可选）
# 三参数协同控制：突发容量、补充速率、最小间隔
# burst_size = 10          # 令牌桶容量（突发上限），默认 10
# cmds_per_sec = 20        # 每秒令牌补充速率，默认 20
# cmd_interval_ms = 50     # 命令间最小间隔（毫秒），默认 50，范围 20~200
# 推荐值：cmd_interval_ms 50（普通玩家）、80（轻度延迟）、120（保守安全）

# 渲染控制（可选）
# render_interval = 1000   # 渲染间隔（毫秒），范围 [50, 10000]
# realtime = false          # 实时渲染开关

# 日志保留数量（可选，默认 24，保留最近 24 小时日志文件）
# log_rotation_count = 24
```

> 如需临时禁用某个角色配置，将文件后缀改为非 `.toml`（如 `.bak`）即可。

### 凭据安全保存（密码不写进配置）

`username` / `password` / `socks5_username` / `socks5_password` 支持环境变量占位符：字段整值写成 `${变量名}` 时，启动时自动替换为同名环境变量的值。分享/备份 profiles 目录时不再携带密码。

两种提供变量的方式：

**方式一：`.env` 文件（推荐，集中管理多个号）**

```bash
cd profiles
# Windows: copy .env.example .env    Linux/macOS: cp .env.example .env
```

编辑 `.env`，一行一个 `变量名=密码`：

```ini
MUD_MYCHAR_PWD=我的真实密码
```

角色配置中引用：

```toml
password = "${MUD_MYCHAR_PWD}"
```

**方式二：系统环境变量（设一次永久生效，不落在项目目录里）**

```powershell
setx MUD_MYCHAR_PWD "我的真实密码"   # Windows，新开终端窗口后生效
```

```bash
export MUD_MYCHAR_PWD="我的真实密码"  # Linux/macOS，或写入 ~/.bashrc 持久化
```

规则说明：

- 同名变量已存在时系统环境变量优先，`.env` 不覆盖
- 变量未提供时启动打印警告并将该字段按空处理（可手动输密码登录），不会把占位符文本发给服务器
- 密码本体恰好是 `${XXX}` 形状时用 `$` 转义：`password = "$${LITERAL}"` 代表 `${LITERAL}`
- 修改 `.env` 后需重启客户端生效；`.env` 已被 gitignore 排除，勿外传

---

## 快捷键

| 快捷键 | 功能 |
|--------|------|
| `Alt+1~9` / `Alt+0` | 切换到对应编号连接（Alt 被占用时可用 `/sw <编号>` 或鼠标点击标签） |
| `Alt+Left` / `Alt+Right` | 前一个/后一个连接（循环） |
| 鼠标点击状态栏标签 | 切换到对应连接 |
| `Ctrl+C` / `Ctrl+D` | 退出程序 |
| `↑` / `↓` | 浏览命令历史 |
| `PageUp` / `PageDown` | 向上/向下滚动查看历史输出（每次半屏） |
| `Home` | 光标移到行首 |
| `End` | 输入框为空时回到输出底部，有内容时到行尾 |
| `Ctrl+A` / `Ctrl+E` | 跳到行首 / 行尾 |
| `Ctrl+U` / `Ctrl+K` | 清除行首到光标 / 光标到行尾 |
| `Ctrl+W` | 删除光标前一个单词 |

### 文本复制

鼠标处于应用模式，按住 **Shift** 键拖拽选择文本：

- `Shift + 鼠标拖拽` 选中
- `Ctrl+Shift+C` 复制（Windows Terminal / GNOME Terminal 等）
- 鼠标右键复制（Windows Terminal 默认行为）
- Linux 下选中自动复制到选择缓冲区，鼠标中键粘贴
- `Ctrl+C` 会退出客户端，请勿用于复制

---

## 内置命令

| 命令 | 说明 |
|------|------|
| `/connect <名> <主机:端口>` | 添加并连接新角色 |
| `/disconnect [编号]` | 断开连接（保留 session） |
| `/reconnect [编号]` | 断开并重新连接 |
| `/close [编号]` | 彻底关闭并移除 session |
| `/list` | 列出所有连接及状态 |
| `/load <脚本路径>` | 为前台连接加载 Lua 脚本 |
| `/load reload` / `/reload` | 重新加载前台脚本（保留变量状态） |
| `/switch <角色名\|编号>` / `/sw` | 切换到指定连接 |
| `/profile list` | 列出可用角色配置 |
| `/profile load <角色名>` | 加载配置并连接（无需重启） |
| `/all <命令>` | 向所有连接发送指令 |
| `/lua <代码>` | 直接执行 Lua 代码 |
| `/set keep_command on\|off` | Enter 后是否保留命令栏内容 |
| `/set render_interval <毫秒>` | 设置渲染间隔（50-10000ms） |
| `/set realtime on\|off` | 切换实时渲染模式 |

---

## Lua 脚本 API

本客户端实现了 MUSHclient 的部分常用 API。完整 API 文档见 [help/api/](help/api/) 目录。

> **兼容性提示**：如你的脚本使用了未实现的 API（`Accelerator`、`AddFont`、`ArrayCreate` 等），将无法正常运行。使用前请确认脚本中调用的所有 API 都在兼容范围内。

### 触发器

| API | 说明 |
|-----|------|
| `AddTrigger` / `AddTriggerEx` | 注册触发器 |
| `DeleteTrigger(name)` | 删除触发器 |
| `EnableTrigger(group, enable)` | 启用/禁用 |
| `EnableTriggerGroup(group, enable)` | 按组启用/禁用 |
| `GetTriggerList()` | 获取名称列表 |
| `GetTriggerInfo(name, code)` | 获取信息 |
| `SetTriggerOption(name, option, value)` | 设置选项 |

回调：`function(name, line, wildcards, styles)`，`wildcards[0]` = 完整匹配文本。`OneShot`（32768）标志：触发后自动删除。

### 别名

| API | 说明 |
|-----|------|
| `AddAlias(name, match, response, flags, [script])` | 注册别名 |
| `DeleteAlias(name)` | 删除别名 |
| `GetAliasInfo(name, code)` / `GetAliasList()` | 获取信息/列表 |
| `SetAliasOption(name, option, value)` | 设置选项 |

`OneShot`（32768）标志：匹配后自动删除。

### 定时器

| API | 说明 |
|-----|------|
| `AddTimer(name, h, m, s, command, flags, [script])` | 注册定时器 |
| `DeleteTimer(name)` / `ResetTimer(name)` | 删除/重置 |
| `EnableTimer(name, enable)` / `EnableTimerGroup(...)` | 启用/禁用 |
| `GetTimerList()` / `GetTimerInfo(name, code)` | 获取信息 |
| `SetTimerOption(name, option, value)` | 设置选项 |

### 命令与输出

| API | 说明 |
|-----|------|
| `Send(cmd)` / `Execute(cmd)` | 发送命令到服务器 |
| `DiscardQueue()` | 清空命令队列 |
| `DoAfter(seconds, command)` | 延迟执行命令（支持 DoAfterNote / DoAfterSpecial / DoAfterSpeedWalk） |
| `Note(text)` / `Tell(text)` / `print(...)` | 输出文本 |
| `ColourNote(fg, bg, msg)` | 彩色输出 |
| `Simulate(text)` | 模拟服务器输出 |
| `SetStatus(text)` | 设置状态栏文本 |
| `log(msg)` | 记录日志 |

### 浮动面板

> RustLuaMud 扩展 API（非 MUSHclient 标准），详见 [浮动面板 API](help/api/panels.md)。

| API | 说明 |
|-----|------|
| `SetPanel(name, x, y, w, h, text, [buttons])` | 创建/更新浮动面板（overlay，不随输出滚动） |
| `RemovePanel(name)` | 移除浮动面板 |
| `RegisterPanelHandler(panel_name, callback)` | 注册面板按钮点击回调 |

`SetPanel` 的 `buttons` 参数定义可点击区域，点击时客户端通过 `RegisterPanelHandler` 注册的回调分发 `action`：

```lua
RegisterPanelHandler("stat", function(panel_name, action)
  if action == "go" then start() end
end)

SetPanel("stat", -70, 0, 70, 10, stat_text, {
  { row = 9, start_col = 5, end_col = 12, action = "go" },
})
```

### 变量

| API | 说明 |
|-----|------|
| `GetVariable(name)` / `SetVariable(...)` / `DeleteVariable(...)` | 变量管理 |
| `GetVariableList()` | 获取所有变量 |
| `get(key)` / `set(key, value)` | 简写接口 |

### 网络

| API | 说明 |
|-----|------|
| `IsConnected()` / `Connect()` / `Disconnect()` | 连接控制 |
| `OnConnect()` | 连接回调（由 Lua 覆盖实现） |

### 数据库

| API | 说明 |
|-----|------|
| `sqlite3.open(path)` | 打开数据库 |
| `conn:execute(sql)` / `conn:close()` | 执行 SQL / 关闭 |
| `conn:set_gbk(enable)` | 设置 GBK 解码 |

### 样式与颜色

| API | 说明 |
|-----|------|
| `GetStyle(styles_table, position)` | 从样式表查询指定位置颜色 |
| `RGBColourToName(colour)` | ANSI 色号映射颜色名 |

### 工具函数

| API | 说明 |
|-----|------|
| `GetUniqueNumber()` | 获取唯一递增编号 |
| `Trim(str)` | 去除首尾空白 |
| `GetPluginInfo(id, code)` | 获取插件信息 |
| `MakeRegularExpression(text)` | 文本转义为正则 |

### 扩展

| API | 说明 |
|-----|------|
| `dofile(filename)` | 加载 Lua 脚本（自动 GBK 转码） |
| `rex` | 正则模块 |
| `bit` | 位运算（band / bor / bxor / bnot / lshift / rshift） |
| `json_encode(val)` / `json_decode(str)` | JSON 序列化/反序列化 |
| `SendPkt(data)` | 发送原始数据包 |

### 标志位常量

| 常量表 | 说明 |
|--------|------|
| `trigger_flag` | 触发器标志位 |
| `alias_flag` | 别名标志位 |
| `timer_flag` | 定时器标志位 |
| `custom_colour` | 自定义颜色编号 |
| `sendto` | 发送目标 |
| `error_code` / `error_desc` | 错误码与描述 |

常用值：`Enabled=1`、`KeepEvaluating=8`、`RegularExpression=32`、`Replace=1024`、`Temporary=16384`（三者统一）、`OneShot=32768`（触发器/别名）/ `OneShot=4`（定时器）。

---

## 项目结构

```
├── profiles/              # 角色配置文件
├── scripts/               # Lua 脚本
│   └── lua/               # Lua 依赖库（wait.lua 等）
├── logs/                  # 日志文件
├── help/                  # 文档
│   ├── api/               # Lua API 参考
│   └── commands/          # 命令指南
├── src/
│   ├── main.rs            # 入口
│   ├── lib.rs             # 库入口（集成测试入口）
│   ├── app.rs             # 应用主逻辑
│   ├── config.rs          # 配置解析
│   ├── connection/        # 连接管理（manager / session / rate_limiter）
│   ├── ui/                # 终端 UI（terminal / input / ansi）
│   ├── log/               # 日志系统（logger / panic_hook / debug）
│   └── lua/               # Lua 引擎 + API（engine / api / triggers / aliases / timers / commands / database / helpers / index / types）
├── .github/workflows/     # CI/CD
└── Cargo.toml
```

---

## 技术栈

| 组件 | 库 |
|------|-----|
| 异步运行时 | tokio |
| 终端控制 | crossterm |
| Lua 引擎 | mlua (LuaJIT) |
| 正则 | regex |
| 数据库 | rusqlite |
| 配置解析 | toml + serde |
| 编码 | encoding_rs |
| SOCKS5 | tokio-socks |

---

## 系统要求

| 项目 | 要求 |
|------|------|
| 操作系统 | Linux（已测试）/ macOS / Windows 10 1511+（仅 64 位，推荐 1809+） |
| CPU | x86_64、i686（仅 Linux）或 aarch64 |
| 内存 | 最低 512MB，推荐 2GB（10 连接） |
| 终端 | 支持 UTF-8 + ANSI 转义序列；Windows 推荐 Windows Terminal |
| Rust 编译 | 1.70+（edition 2021） |

### 32 位平台 (i686)

预编译二进制已支持。从源码编译需安装 32 位工具链：

```bash
sudo dpkg --add-architecture i386 && sudo apt update && sudo apt install gcc-multilib g++-multilib
scripts/build.sh --arch i686
```

> 32 位 LuaJIT 整数上限 2^31，MUD 脚本中的经验值、HP 等数值不受影响。

---

## 故障排查

如需调试信息，启动前设置环境变量：

```bash
export RUST_BACKTRACE=1
./RustLuaMud
```

panic 时会自动打印堆栈并写入对应连接日志文件（`[PNC]` 前缀）。

---

## CI/CD

项目使用 GitHub Actions 实现自动化：

- **CI** — 每次 push/PR 自动运行测试、clippy、fmt 检查
- **Release** — 打 tag 自动构建 GitHub Release
- **Nightly** — 每次 push main 自动构建并同步到 [GitHub](https://github.com/llps112/RustLuaMud/releases/tag/nightly) 和 [Gitee](https://gitee.com/bai-yifei180/RustLuaMud/releases) Release
- **Audit** — 每周自动依赖安全审计

---

## 版本历史

### v0.9.0 (2026-08-27)
- Windows 平台正式支持：x86_64 编译/运行适配，conhost 可用宽度自适应（避让滚动条遮挡右对齐 UI）
- release/nightly CI 并行构建 `RustLuaMud-windows-x86_64.zip`，GitHub 与 Gitee Release 同步附带
- 新增 `scripts/bootstrap.ps1` Windows 一键部署（默认 `%USERPROFILE%\RustLuaMud`，免管理员权限）
- 启动自检：校验日志与 profiles 目录可写，失败即时退出并报告真实原因

### v0.6.3 (2026-08-13)
- 修复浮动面板闪烁问题：`draw_output_area` 跳过面板覆盖区域，消除"擦除→重画"中间态
- 新增 `truncate_ansi_to_width` 辅助函数，安全截断含 ANSI 转义的输出文本
- 新增 `panel_coverage_mask` 方法，计算每行面板覆盖范围

### v0.6.2 (2026-08-04)
- 看门狗分段睡眠优化，测试从 220s 提速到 5s（42x）
- 新增 `test_engine_drop_is_fast` 测试守护 drop 性能

### v0.6.1 (2026-08-04)
- 新增 `RegisterPanelHandler` API，解耦客户端与脚本的面板点击回调
- GBK 自动同步 pre-commit 钩子
- 命名空间遗漏检测工具 `tools/check_ns_leak.py`

### v0.6.0 (2026-07-31)
- 命令限速迁移至 Rust 令牌桶（burst_size + cmds_per_sec + min_interval）
- 定时器系统优化，标记式禁用取代 closeclass 延迟
- engine.rs 拆分，trigger/alias/timer 新增 name 索引和 group 索引
- 修复终端渲染行间颜色泄漏、Lua 打印行切换 session 后丢失
- 后台 session 面板增量写入，修复 pending_panels 内存泄漏

### v0.5.0 (2026-07-27)
- `omit_from_output` 文本过滤功能
- 浮动面板 `SetPanel`/`RemovePanel` API + 长行自动换行
- bootstrap 支持 Gitee stable 下载
- CI 工作流添加 paths 过滤，nightly 构建限制为 Rust 源码变更时触发

### v0.4.0 (2026-07-23)
- Rust 侧命令发送物理限速（`cmd_interval_ms` 配置项），配合 Lua 侧 burst 控制形成双层限速保护
- Gitee Release 自动同步
- README 文档全面重构
- panic 日志捕获功能，panic 写入日志文件
- Lua 定时器看门狗线程，防止无限循环永久卡死
- 移除 Lua 侧 server_watch 模块，服务器响应追踪迁移到 Rust 侧

### v0.3.0 (2026-07-22)
- 新增 Rust 侧命令发送物理限速（`cmd_interval_ms` 配置项），配合 Lua 侧 burst 控制形成双层限速保护
- 新增 Gitee Release 自动同步（Nightly 构建）
- 新增 i686 架构预编译构建
- 新增 i686 架构构建脚本 `scripts/build.sh --arch i686`
- 优化命令限速算法：从漏桶算法回归 burst 计数 + 动态补偿等待

### v0.2.1 (2026-07-14)
- 修复 `connect_delay_ms` 延迟触发机制

### v0.2.0 (2026-07-14)
- `bootstrap.sh` 改为一键初始化脚本
- 新增游戏脚本自动部署支持
- 修复目录/文件冲突处理

### v0.1.0 ~ v0.1.9 (2026-06-10 ~ 2026-07-14)
- 完整实现 MUSHclient 兼容 API
- 多连接管理、SOCKS5 代理、输出历史滚动
- ANSI SGR 解析、GBK 编码兼容
- SQLite3 集成、JSON 序列化
- 可配置渲染频率、连接延迟
- 827 单元测试

---

## 外部程序集成

客户端支持通过 `json_encode` / `json_decode` API 与外部程序交换数据：

```rust
// Rust 侧获取 Lua 数据
let json = engine.eval_to_string("return json_encode(my_table)");
```
