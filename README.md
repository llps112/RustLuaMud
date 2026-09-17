# RustLuaMud

基于 Rust + LuaJIT 的终端 MUD 客户端，面向 7x24 小时无 GUI 挂机场景，兼容 MUSHclient 脚本 API。

---

## 特性

**MUSHclient 兼容层**
- 常用 API 类别全覆盖：触发器、别名、定时器、变量、日志、数据库、样式查询
- 触发器 `wildcards[0]`（完整匹配文本）与 MUSHclient 行为完全一致
- 多行触发器、颜色样式回调（`GetStyle`）、模拟输出（`Simulate`）
- 触发器正则基于 Rust `regex` 引擎（PCRE 语法子集：自动转换 `\Z`/`\z`，不支持反向引用与前后查找，不兼容的模式在注册时即时报错）
- 参考 `help/api/` 目录查阅完整 API 文档
- 接口稳定性承诺（哪些冻结、哪些可变、如何弃用）见 [COMPATIBILITY.md](COMPATIBILITY.md)

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
- Rust 侧令牌桶限速（burst_size + cmds_per_sec + cmd_interval_ms 三参数）钉住长期速率
- 叠加滑动窗口（window_limit + window_duration_ms）封顶突发密度，任意 2 秒内 ≤ 60 条
- 默认：突发 10 条、每秒 20 条、最小间隔 50ms、窗口 60 条/2 秒
- 不依赖与服务端 tick 对齐，可防住 GPS 寻路重试等场景的多次突发跨 drain 周期累积
- 安全前提：`cmds_per_sec ≤ 20`（服务端 drain 速率）且 `burst_size + 2×cmds_per_sec ≤ 60`；
  滑动窗口封顶的是突发密度而非长期速率，两条不等式必须同时成立才能保证 cnt ≤ 60
- 配置解析时会校验上述不等式，不安全的参数组合在启动与 `/profile load` 时告警

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

> **Windows 说明**：Windows 平台已提供预编译 `RustLuaMud-windows-x86_64.zip`（stable 与 nightly 均附带），**仅支持 64 位系统**，暂不支持 32 位与 ARM64。本项目的 ANSI 样式、CJK 对齐与浮动面板依赖现代终端的 VT/ANSI 支持，启动时还会切换到备用屏幕缓冲区（`CSI ?1049h`）：**最低 Windows 10 version 1607 / Windows Server 2016**（同为 build 14393，已在 Server 2016 实机连续运行验证），**推荐 1809 及以上**（体验完整）；更早的 1511 只有 VT 基础开关、解析不了备用屏序列，Win7/8/XP 连开关都没有，均不支持。终端推荐使用 Windows Terminal（传统 conhost 已做宽度自适应，但滚动条等固有缺陷无法完全消除）。

> **国内镜像加速**：`--gitee` 参数从 Gitee 下载，支持稳定版和 Nightly 两种版本：
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
│   ├── example.toml     # 示例配置
│   └── .env.example     # 凭据模板（复制为 .env 后密码无需写进 toml）
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

> 示例配置与凭据模板**只在文件不存在时创建**，重跑初始化脚本不会覆盖你改过的配置。
> 如果升级后想参考新增的配置项，删掉这两个文件重跑脚本，或直接从仓库拉取新版：
>
> ```bash
> cd ~/RustLuaMud/profiles
> # GitHub
> curl -LO https://raw.githubusercontent.com/llps112/RustLuaMud/main/profiles/example.toml
> curl -LO https://raw.githubusercontent.com/llps112/RustLuaMud/main/profiles/.env.example
> # Gitee（国内更快）
> curl -LO https://gitee.com/bai-yifei180/RustLuaMud/raw/main/profiles/example.toml
> curl -LO https://gitee.com/bai-yifei180/RustLuaMud/raw/main/profiles/.env.example
> ```
>
> Gitee 侧代码与 GitHub 同步，两个源均可拉到最新模板（已实测）。

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

> 装完首次启动若弹窗「由于找不到 VCRUNTIME140.dll，无法继续执行代码」，不是下载损坏，是系统缺 VC++ 运行库：
> 见下方「故障排查」小节，装一次即可。

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
# 支持环境变量占位符避免明文，见下方「凭据安全保存」
username = "your_character_name"
password = "your_password"

# SOCKS5 代理（可选）
socks5_enable = false
socks5_host = "127.0.0.1"
socks5_port = 1080
# socks5_username = "user"
# socks5_password = "pass"

# 命令发送限速（可选）
# 令牌桶控制速率与突发手感，滑动窗口封顶突发密度
# burst_size = 10          # 令牌桶容量（突发上限），默认 10
# cmds_per_sec = 20        # 每秒令牌补充速率，默认 20
# cmd_interval_ms = 50     # 命令间最小间隔（毫秒），默认 50，范围 20~200
# window_limit = 60        # 滑动窗口内最大命令数，默认 60，范围 1~1000
# window_duration_ms = 2000 # 滑动窗口时长（毫秒），默认 2000，范围 2000~10000
# 推荐值：cmd_interval_ms 50（普通玩家）、80（轻度延迟）、120（保守安全）
#
# 安全不等式（服务端 LPC cmd.c：每 2 秒 drain 40，cnt > 60 雷劈）：
#   1) cmds_per_sec ≤ 20              —— 长期速率不得超过 drain 速率，
#      否则 cnt 逐周期净增，长时间挂机必然雷劈
#   2) burst_size + 2×cmds_per_sec ≤ 60 —— 单次突发 + 随后 2 秒匀速的峰值
# 注意 cmd_interval_ms 不构成长期速率上限：它只约束非突发模式的相邻间隔，
# 富余令牌会累积到 burst_size 再以突发形式花掉，长期速率由 cmds_per_sec 决定。
#
# 滑动窗口封顶的是突发密度（半开区间），不封顶长期速率：window_limit = 60
# 只在上面两条不等式成立时才安全。若要无条件兜底，可把 window_limit 设为 40
# （= 服务端每周期 drain 量），此时即使令牌桶参数配错，cnt 也恒 ≤ 40。
# 参数组合不安全时，启动与 /profile load 都会输出告警。

# 渲染控制（可选）
# render_interval = 1000   # 渲染间隔（毫秒），范围 [50, 10000]
# realtime = false          # 实时渲染开关

# 日志保留数量（可选，默认 24，保留最近 24 小时日志文件）
# log_rotation_count = 24
```

> 如需临时禁用某个角色配置，将文件后缀改为非 `.toml`（如 `.bak`）即可。

### 凭据安全保存（密码不写进配置）

`username` / `password` / `socks5_username` / `socks5_password` 支持环境变量占位符：字段值整体写成 `${变量名}` 时，启动时自动替换为同名环境变量的值。分享/备份 profiles 目录时不再携带密码。

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

> **兼容性提示**：若你的脚本使用了未实现的 API（`Accelerator`、`AddFont`、`ArrayCreate` 等），将无法正常运行。使用前请确认脚本中调用的所有 API 都在兼容范围内。

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
| `rex` | 正则模块（Rust `regex` 引擎，PCRE 语法子集，不支持反向引用与前后查找） |
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

常用值（三表数值一致的）：`Enabled=1`、`KeepEvaluating=8`、`Replace=1024`、`Temporary=16384`；`RegularExpression=32`（触发器）/ `128`（别名，定时器无此项）；`OneShot=32768`（触发器/别名）/ `4`（定时器）。完整定义以 `trigger_flag` / `alias_flag` / `timer_flag` 表为准。

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
│   ├── app/               # app 子模块（session / commands / events / parse）
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
| 操作系统 | Linux（已测试）/ macOS / Windows 10 1607+ 或 Windows Server 2016+（仅 64 位，推荐 1809+） |
| CPU | x86_64、i686（仅 Linux）或 aarch64 |
| 内存 | 最低 512MB，推荐 2GB（10 连接） |
| 终端 | 支持 UTF-8 + ANSI 转义序列；Windows 推荐 Windows Terminal |
| Windows 运行库 | **需 [VC++ 2015-2022 可再发行组件 x64](https://aka.ms/vs/17/release/vc_redist.x64.exe)**：预编译 exe 动态链接 MSVC CRT，缺失时启动弹窗报「由于找不到 VCRUNTIME140.dll，无法继续执行代码」 |
| Rust 编译 | 1.70+（edition 2021） |

### 32 位平台（i686）

预编译二进制已支持。从源码编译需安装 32 位工具链：

```bash
sudo dpkg --add-architecture i386 && sudo apt update && sudo apt install gcc-multilib g++-multilib
scripts/build.sh --arch i686
```

> 32 位 LuaJIT 整数上限 2^31，MUD 脚本中的经验值、HP 等数值不受影响。

---

## 故障排查

崩溃堆栈**默认开启**：程序启动时若检测到 `RUST_BACKTRACE` 未设置，会自动补默认值 `1`（无人值守场景下崩溃时人不在现场，默认带栈才能排障）。panic 时会自动打印堆栈并写入对应连接日志文件（`[PNC]` 前缀）。

如需覆盖该默认（程序仅在变量未设置时补默认，显式设置一律尊重）：

```bash
export RUST_BACKTRACE=0      # 关闭堆栈输出（减小日志体积）
export RUST_BACKTRACE=full   # 输出完整堆栈（含标准库帧）
./RustLuaMud
```

### Windows 启动弹窗「由于找不到 VCRUNTIME140.dll，无法继续执行代码」

系统缺少 VC++ 运行库，**不是程序损坏或下载出错**。下载安装
[vc_redist.x64.exe](https://aka.ms/vs/17/release/vc_redist.x64.exe)（约 25 MB，VC++ 2015-2022
统一版），完成后重新运行即可，无需重启系统。英文系统的对应消息为
`The code execution cannot proceed because VCRUNTIME140.dll was not found.`。

需装运行库的只有 `VCRUNTIME140.dll` 这一项：按导入表核对，其余 CRT 依赖全部走
UCRT（`api-ms-win-crt-*`，Windows 10 起随系统提供），产物也不链 C++ 标准库，
因此不会提示缺 `MSVCP140.dll` 或 `VCRUNTIME140_1.dll`。

成因：Windows 预编译产物按 `x86_64-pc-windows-msvc` 的默认方式**动态链接 MSVC CRT**（构建
环境自带运行库，目标机器不一定带）。因此无论是从 Release 下载，还是从另一台 Windows
机器**手工复制** exe 过去，只要那台机器没装过 Visual Studio / 其他依赖 VC++ 运行库的软件，
都会报同一个错。

> 可改为静态链接 CRT（target 级 `rustflags = ["-C", "target-feature=+crt-static"]`）彻底消除
> 这个依赖。当前方案保持动态链接 + 文档声明前置：静态化会连带影响 `rusqlite`（bundled）与
> `mlua`（vendored LuaJIT）的 C 编译方式（需确认它们是否同步切到 `/MT`），必须经一次真实的
> Windows 构建 + 干净目标机验证才敢发 Release，
> 详见 [docs/windows-build-plan.md](docs/windows-build-plan.md)。

---

## CI/CD

项目使用 GitHub Actions 实现自动化：

- **CI** — 每次 push/PR 自动运行测试、clippy、fmt 检查
- **Release** — 打 tag 自动构建 GitHub Release
- **Nightly** — 每次 push main 自动构建并同步到 [GitHub](https://github.com/llps112/RustLuaMud/releases/tag/nightly) 和 [Gitee](https://gitee.com/bai-yifei180/RustLuaMud/releases) Release
- **Audit** — 每周自动依赖安全审计

---

## 版本历史

### v0.9.9 (2026-09-17)
- 修复重连竞态：旧连接残留的 Disconnected 事件不再污染新连接（连接代际校验 + 拨号前取消旧读任务），杜绝重复重连顶号
- 心跳超时改主动断开并显式排期重连，不再依赖 FIN→EOF 事件链（网络静默死亡时原路径永不重连）
- TCP keepalive 跨平台化（socket2），Windows/macOS 一并生效，空闲断链可被探测
- Lua 看门狗覆盖补全：触发器、别名、脚本加载、命令执行的死循环不再卡死整个客户端，并支持嵌套调用
- `terminal.json` 路径跟随 `--profiles` 目录，多实例配置互不干扰
- 文档修正：`alias_flag.RegularExpression` 实为 128（原述"三表一致"有误）；`rex` 明确为 Rust regex 子集

### v0.9.8 (2026-09-16)
- 修复 Linux 一键部署产物落后：`bootstrap.sh` 内嵌模板补齐到与权威源同等（凭据占位符、五项限速参数、`.env.example` 生成）
- 新增三份模板防漂移守卫测试（`config.rs`）并接入 CI 触发路径，改权威源漏同步内嵌副本会被拦住
- 更正限速说明：`cmd_interval_ms` 不构成长期速率上限，长期速率由 `cmds_per_sec` 决定
- Windows 启动脚本设置 conhost 窗口 160×56 并居中（`console_setup.ps1`），划清与 `ps_config.ps1` 的窗口职责边界
- Windows 最低版本统一为 1607 / Server 2016，补充预编译产物依赖 VC++ 运行库的前置说明
- 依赖升级 `encoding_rs` 0.8.35 → 0.8.41

### v0.9.7 (2026-09-11)
- 凭据改由 `.env` 环境变量注入，profile 不再明文存密码；`/profile load` 支持运行时热加载 `.env`
- 凭据缺失或展开失败改为显式告警并中止加载，不再静默置空导致脚本崩溃
- 系统消息三写（终端 + session 回看缓冲 + 日志文件），切前台不再丢失告警
- 动态新增连接增加重名保护，杜绝同名 session 互相顶号导致的无限重连

### v0.9.6 (2026-09-04)
- 限速器叠加滑动窗口硬兜底，杜绝多次突发跨 drain 周期累积导致的「雷劈」
- release changelog 过滤纯元数据提交，不再重复列「更新子模块指针」

### v0.9.5 (2026-09-02)
- 修复面板点击回调后未 flush：补 `drain_lua_logs`，Lua 输出不再滞留到下次 MUD 输出或定时器 tick 才落盘
- 规则/文档更正：子模块隐私收窄为 L1/L2/L3 分级、`print` 亦写入日志、`iconv -o` 与 `core.hooksPath` 前置说明

### v0.9.4 (2026-08-30)
- Lua 状态栏按显示宽度截断，超宽内容不再写穿末行触发整屏上滚
- `.trae/rules` 改为指向 `.qoder/rules` 的符号链接，两 IDE 共用单一规则源
- `bootstrap.ps1` 补齐凭据占位符/限速/日志示例并生成 `.env.example`

### v0.9.3 (2026-08-29)
- 修复 Linux 下无法点击面板/切换标签（鼠标捕获按平台分治）
- 底行状态栏改灰白底黑字，并修复输入行绿字与 SGR 前景色的可读性问题
- 取消末行避让，将 Lua 角色统计栏移到底行并整行蓝底高亮
- release CI 抓取完整 tag 历史，确保 changelog 完整

### v0.9.2 (2026-08-29)
- 稳定 conhost 布局：修复状态栏漂移与 CJK 字形重叠
- 凭据支持环境变量占位符 + `.env` 加载

### v0.9.1 (2026-08-27)
- 将 conhost 宽字符处理闸门收窄至经典控制台（以 `WT_SESSION` 区分 Windows Terminal）
- 修复 panic hook 测试的可移植性

### v0.9.0 (2026-08-27)
- Windows 平台正式支持：x86_64 编译/运行适配，conhost 可用宽度自适应（避让滚动条遮挡右对齐 UI）
- release/nightly CI 并行构建 `RustLuaMud-windows-x86_64.zip`，GitHub 与 Gitee Release 同步附带
- 新增 `scripts/bootstrap.ps1` Windows 一键部署（默认 `%USERPROFILE%\RustLuaMud`，免管理员权限）
- 启动自检：校验日志与 profiles 目录可写，失败即时退出并报告真实原因

### v0.8.0 (2026-08-22)
- ANSI 颜色继承状态机统一为「行末最后 SGR 决定」语义，消除跨模块对颜色延续的歧义

### v0.7.8 (2026-08-22)
- ANSI 颜色继承移除单行限制：颜色持续到服务端发送 reset，修复多行连续消息颜色被截断

### v0.7.7 (2026-08-22)
- 新增 `--version` 参数（打印版本号后退出，不启动客户端）
- ANSI 调试增强：每行原始 ANSI 日志 + 跨包上下文捕获
- 适配 CI clippy 1.98 的 `drain_collect` 检查（`drain(..).collect()` → `std::mem::take`）

### v0.7.5 (2026-08-22)
- ANSI 行首继承色补全 + 重置变体识别修正
- `bootstrap` 的 Gitee API 增加 `per_page=100`，避免 release 列表分页截断

### v0.7.4 (2026-08-22)
- `/reload` 崩溃修复 + ANSI 调试日志增强

### v0.7.3 (2026-08-21)
- 热重载完全清理：显式取消旧 timer task，`/all reload` 仅重启成功 session 的 timer
- 守护进程模式 `--daemon` / `stop` / `status`（仅 Unix）
- `app.rs` 拆分为 `app/` 子模块

### v0.7.0 ~ v0.7.2 (2026-08-20)
- 连接稳定性改造：指数退避重连 `min(base*2^attempt, max_secs)`（成功即重置）；空闲心跳检测（`idle_timeout` 发心跳、`heartbeat_timeout` 主动断连）
- 新增 Lua API：`GetSessionStats()`、`OnDisconnect(reason)` 回调 + 8 个新 `GetInfo` 编号
- `[DCN]`/`[RCN]` 断连/重连日志标签 + `disconnect_time` 停机时长追踪
- 新增 26 个测试（834→860），clippy 零警告
- v0.7.1 / v0.7.2 为补丁发布，无客户端代码变更

### v0.6.6 (2026-08-19)
- 新增 Lua `wait.stop_all` 函数
- 依赖升级：rusqlite 0.40.1→0.40.2、futures 0.3.33→0.3.34
- 补充项目文档与规则文件

### v0.6.5 (2026-08-17)
- 命令通道发送错误对齐去重，防止 TCP 半死时同类错误刷屏
- 空闲心跳节流防发送队列填满，原始数据发送错误去重防刷屏

### v0.6.4 (2026-08-16)
- 修复 `parse_style_runs` 与 `strip_ansi` 处理非 CSI 序列不一致导致的 panic
- session 发送队列满时输出诊断日志到 stderr
- README 修正标志位常量值、补充项目结构与 OneShot 文档

### v0.6.3 (2026-08-13)
- 修复浮动面板闪烁问题：`draw_output_area` 跳过面板覆盖区域，消除「擦除→重画」中间态
- 新增 `truncate_ansi_to_width` 辅助函数，安全截断含 ANSI 转义的输出文本
- 新增 `panel_coverage_mask` 方法，计算每行面板覆盖范围

### v0.6.2 (2026-08-04)
- 看门狗分段睡眠优化，测试从 220s 提速到 5s（42x）
- 新增 `test_engine_drop_is_fast` 测试守护 drop 性能

### v0.6.1 (2026-08-04)
- 新增 `RegisterPanelHandler` API，解耦客户端与脚本的面板点击回调
- GBK 自动同步 pre-commit 钩子
- 命名空间遗漏检测工具 `tools/check_ns_leak.py`

### v0.6.0 (2026-08-02)
- 命令限速迁移至 Rust 令牌桶（burst_size + cmds_per_sec + min_interval）
- 定时器系统优化，标记式禁用取代 closeclass 延迟
- engine.rs 拆分，trigger/alias/timer 新增 name 索引和 group 索引
- 修复终端渲染行间颜色泄漏、Lua 打印行切换 session 后丢失
- 后台 session 面板增量写入，修复 pending_panels 内存泄漏

### v0.5.6 (2026-07-29)
- 修复 Lua 打印行在切换 session 后丢失
- 修复 `test_panic_hook_writes_log_on_panic` 在 CI 并行环境下失败

### v0.5.5 (2026-07-28)
- 浮动面板按钮功能（Rust 侧）
- CI 改用 `cargo-nextest` 并限制并发线程数（减少 runner OOM 风险）
- 依赖升级：libc 0.2.186→0.2.189、serde 1.0.228→1.0.229

### v0.5.4 (2026-07-28)
- 补丁发布，仅同步子模块脚本指针，无客户端代码变更

### v0.5.3 (2026-07-27)
- 长行自动换行 + 浮动面板 Lua `SetPanel`/`RemovePanel` API 完善

### v0.5.2 (2026-07-26)
- 新增 GBK 尾部文本 `omit_from_output` 匹配测试

### v0.5.1 (2026-07-24)
- 修复多 session 断线重连时客户端卡顿与错误刷屏

### v0.5.0 (2026-07-23)
- `omit_from_output` 文本过滤功能
- 浮动面板 `SetPanel`/`RemovePanel` API + 长行自动换行
- bootstrap 支持 Gitee stable 下载
- CI 工作流添加 paths 过滤，nightly 构建限制为 Rust 源码变更时触发

### v0.4.0 (2026-07-23)
- Rust 侧命令发送物理限速（`cmd_interval_ms` 配置项），配合 Lua 侧 burst 控制形成双层限速保护
- Gitee Release 同步加固（curl 重试与超时控制）
- README 文档全面重构
- panic 日志捕获功能，panic 写入日志文件
- Lua 定时器看门狗线程，防止无限循环永久卡死
- 移除 Lua 侧 server_watch 模块，服务器响应追踪迁移到 Rust 侧

### v0.3.0 (2026-07-19)
- 新增 Gitee Release 自动同步（Nightly 构建）
- 新增 i686 架构预编译构建
- 新增 i686 架构构建脚本 `scripts/build.sh --arch i686`
- 优化命令限速算法：从漏桶算法回归 burst 计数 + 动态补偿等待

### v0.2.2 (2026-07-15)
- 隔离 `delayed_commands` 延迟队列，修复延迟期内命令被 `process_output` 清空的问题

### v0.2.1 (2026-07-15)
- 修复 `connect_delay_ms` 延迟触发机制

### v0.2.0 (2026-07-10)
- `bootstrap.sh` 改为一键初始化脚本
- 新增游戏脚本自动部署支持
- 修复目录/文件冲突处理

### v0.1.9 (2026-07-06)
- 修复 `/close` 中间 session 后 ID 递补导致 `/profile load` 重连静默断开
- 拆分规则文件为独立模块，补充加载清单同步、运行时数据提交等规范

### v0.1.8 (2026-07-04)
- 实现可配置渲染频率功能，分离渲染间隔与实时模式配置
- 修复节流模式下 Lua 日志与 MUD 数据不同步

### v0.1.7 (2026-07-01)
- 实现 `AddTriggerEx`/`AddTrigger`/`AddAlias` 的 Replace 标志 + 综合测试
- styles 表增加 text 字段；Lua 错误不再静默丢弃

### v0.1.6 (2026-06-27)
- 日志文件名加日期后缀 `YYMMDD_HH`，滚动保留最近 24 个文件
- 新增 `/reconnect` 命令，开放 `/all /disconnect` 白名单

### v0.1.5 (2026-06-26)
- 实现 `GetStyle`/`RGBColourToName` API，触发器回调新增 styles 第 4 参数
- 独立 session 输入缓冲区；修复 `/close` 级联重连与 dofile 递归限制

### v0.1.4 (2026-06-22)
- 新增 `/profile load` / `/profile list` 运行时角色加载命令
- 修复翻看输出历史时缓冲区 drain 导致视口上移、channel-closed 错误刷屏

### v0.1.0 ~ v0.1.3（无 tag，2026-06-02 ~ 2026-06-21）
- 项目奠基：完整实现 MUSHclient 兼容 API
- 多连接管理、SOCKS5 代理、输出历史滚动
- ANSI SGR 解析、GBK 编码兼容
- SQLite3 集成、JSON 序列化
- 可配置渲染频率、连接延迟
- 827 单元测试

---

## 外部程序集成

Lua 脚本可通过 `json_encode` / `json_decode` API 与外部程序交换 JSON 数据；二次开发时也可在 Rust 侧直接调用：

```rust
// Rust 侧获取 Lua 数据
let json = engine.eval_to_string("return json_encode(my_table)");
```
