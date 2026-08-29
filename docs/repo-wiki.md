# RustLuaMud 项目 Wiki

> 基于 Rust + LuaJIT 的终端 MUD 客户端，面向 7x24 小时无 GUI 挂机场景，兼容 MUSHclient 脚本 API。

---

## 一、项目架构总览

RustLuaMud 采用 **Rust 引擎 + Lua 脚本** 的双层架构：

- **Rust 引擎层**：负责 TCP 连接管理、终端 UI 渲染、日志系统、Lua 引擎集成。提供高性能、低资源占用的运行时底座。
- **Lua 脚本层**：负责游戏逻辑、命令处理、NPC 交互、任务调度。通过 MUSHclient 兼容 API 与引擎层通信。

```
┌──────────────────────────────────────────────┐
│                  Lua 脚本层                    │
│  任务调度 │ 命令处理 │ NPC 交互 │ GPS 导航     │
├──────────────────────────────────────────────┤
│           MUSHclient 兼容 API 层              │
│  触发器 │ 别名 │ 定时器 │ 变量 │ 输出 │ 数据库 │
├──────────────────────────────────────────────┤
│                  Rust 引擎层                   │
│  connection │  ui  │  log  │  lua             │
├──────────────────────────────────────────────┤
│  tokio │ crossterm │ mlua(LuaJIT) │ rusqlite  │
└──────────────────────────────────────────────┘
```

核心设计目标：
- **极低资源占用**：J1800 + 2GB 内存即可流畅运行 10 连接
- **MUSHclient API 兼容**：降低现有脚本迁移成本
- **GBK/UTF-8 双编码**：自动检测并转码，兼容老游戏服务器

---

## 二、Rust 引擎模块说明

项目入口为 `src/main.rs`，库入口为 `src/lib.rs`，应用主逻辑在 `src/app.rs`（124.6KB），配置解析在 `src/config.rs`。

### 2.1 connection/ — 连接管理

| 文件 | 大小 | 职责 |
|------|------|------|
| `mod.rs` | 0.2KB | 模块声明 |
| `manager.rs` | 17.8KB | 连接管理器，处理多 session 的创建、切换、关闭 |
| `session.rs` | 56.1KB | 单个 TCP 会话，负责收发数据、自动重连、SOCKS5 代理 |
| `rate_limiter.rs` | 7.1KB | 令牌桶限速器（burst_size + cmds_per_sec + min_interval 三参数） |

关键特性：
- 单实例最多 10 个并发连接，前台/后台无缝切换
- 每个角色独立配置 SOCKS5 代理，支持多开规避同 IP 限制
- Rust 侧物理限速，确保不触发服务器反 flood 机制

### 2.2 ui/ — 终端 UI

| 文件 | 大小 | 职责 |
|------|------|------|
| `mod.rs` | 0.1KB | 模块声明 |
| `terminal.rs` | 112.6KB | 终端渲染主逻辑，含浮动面板、状态栏、输出缓冲 |
| `ansi.rs` | 33.5KB | ANSI SGR 解析器，完整彩色输出支持 |
| `input.rs` | 4.1KB | 输入框处理，命令历史浏览 |

关键特性：
- PageUp/PageDown 翻页查看历史输出
- 鼠标点击状态栏切换连接
- 浮动面板（`SetPanel`/`RemovePanel`/`RegisterPanelHandler`）
- 长行自动换行，CJK 宽字符正确对齐

### 2.3 log/ — 日志系统

| 文件 | 大小 | 职责 |
|------|------|------|
| `mod.rs` | 0.1KB | 模块声明 |
| `logger.rs` | 12.5KB | 日志记录器，按小时滚动，自动清理过期日志 |
| `panic_hook.rs` | 9.0KB | panic 捕获钩子，堆栈写入日志文件（`[PNC]` 前缀） |
| `debug.rs` | 1.2KB | 调试输出辅助 |

### 2.4 lua/ — Lua 引擎 + API

| 文件 | 大小 | 职责 |
|------|------|------|
| `mod.rs` | 0.2KB | 模块声明 |
| `engine.rs` | 9.8KB | LuaJIT 引擎管理，协程支持，`dofile` 自动 GBK 转码 |
| `api.rs` | 115.8KB | MUSHclient 兼容 API 实现（核心文件） |
| `triggers.rs` | 17.7KB | 触发器系统，含 name 索引和 group 索引 |
| `aliases.rs` | 5.1KB | 别名系统 |
| `timers.rs` | 9.2KB | 定时器系统，标记式禁用 |
| `commands.rs` | 10.6KB | 命令执行与队列管理 |
| `database.rs` | 10.2KB | SQLite3 集成，支持 GBK 文本解码 |
| `helpers.rs` | 18.4KB | 辅助函数（字符串工具、类型转换等） |
| `index.rs` | 15.4KB | name 索引和 group 索引的通用实现 |
| `types.rs` | 6.5KB | 共享类型定义 |
| `tests.rs` | 215.5KB | 集成测试（809+ 测试用例） |

---

## 三、Lua 脚本体系

### 3.1 命名空间组织

Lua 脚本采用命名空间化的全局表组织方式，核心命名空间包括：

| 命名空间 | 来源文件 | 职责 |
|----------|---------|------|
| `me` | `michen_var.lua` | 角色基础信息（门派、装备、属性） |
| `hp` | `michen_var.lua` | 生命值/内力/气血状态 |
| `workflow` | `michen_var.lua` | 任务调度状态（当前任务、工作流控制） |
| `stat` | `michen_var.lua` | 统计信息（经验、击杀、平均收益） |
| `add` | `michen_var.lua` | 增量数据（单次经验、物品变动） |
| `have` | `michen_var.lua` | 持有物品统计 |
| `sum` | `michen_var.lua` | 汇总数据 |
| `mark` | `michen_var.lua` | 标记与计数器 |
| `mpLimited` | `michen_var.lua` | 门派任务周期管理 |
| `always` / `always_watch` | `michen_var.lua` | 常驻状态管理 |
| `alias.*` | `michen_alias.lua` | 核心业务函数（寻路、战斗、任务调度） |
| `sys.*` | `michen_system.lua` | 系统函数（trigger 管理、字符串工具） |
| `w` | 触发器回调参数 | 正则通配符匹配结果 |

### 3.2 关键脚本文件

| 文件 | 行数 | 职责 |
|------|------|------|
| `michen_var.lua` | 1149 | 全局变量声明（核心数据定义） |
| `michen_alias.lua` | ~5500 | 核心业务逻辑（别名、寻路、战斗、任务调度） |
| `michen_system.lua` | 551 | 系统函数（trigger/alias 注册封装、字符串工具） |
| `check.lua` | 1904 | 触发器回调（物品处理、状态检查、EXP 统计） |
| `always.lua` | 1585 | 常驻状态管理、限时判断 |
| `common.lua` | 1448 | 通用功能（登录、打坐、船、学技能、熬药） |
| `gps.lua` / `gps_lib.lua` | 2350/1074 | GPS 寻路系统 |
| `fj.lua` | 1152 | 护镖（FJ）任务 |
| `michen_yb.lua` | 1127 | 押镖（YB）任务 |
| `war.lua` / `war_refactor.lua` | ~1130/~1313 | 守城（War）任务 |
| `skills.lua` | 1014 | 技能练习管理 |
| `michen_config.lua` | 458 | 配置命令接口（`#cfg` 系列） |
| `michen_mp_*.lua` | 200-1249 | 各门派任务逻辑（15 个门派文件） |

### 3.3 脚本加载机制

脚本通过 `michen_xkx.lua`（加载清单）统一管理加载顺序。使用 `dofile` 逐文件加载，`loadmod` 函数支持模块重载（依赖 `Replace` 标志位）。

---

## 四、编码工作流

### 4.1 UTF-8 开发 → GBK 运行时

项目采用双编码体系：

- **`scripts/class-utf8/`**：UTF-8 编码，主要开发源，所有修改在此进行
- **`scripts/class/`**：GBK 编码，运行时版本，由 `iconv` 从 UTF-8 版本生成

修改流程：
1. 编辑 `scripts/class-utf8/xxx.lua`（UTF-8 源文件）
2. 执行 `iconv -f utf-8 -t gbk scripts/class-utf8/xxx.lua -o scripts/class/xxx.lua`
3. 提交时 pre-commit 钩子自动检测并同步 GBK 版本

> **严禁**用文本编辑工具直接编辑 `scripts/class/` 中的 GBK 文件——会导致中文字节被 corrupt。

### 4.2 Pre-commit 钩子

子模块配置了 `hooks/pre-commit`（通过 `core.hooksPath hooks` 启用），提交时自动检测暂存的 `class-utf8/*.lua` 文件并同步生成 GBK 版本。

### 4.3 bootstrap.sh 一键初始化

`scripts/bootstrap.sh` 提供一键环境搭建：
- 自动检测架构（x86_64 / i686 / aarch64）
- 从 GitHub 或 Gitee 下载预编译二进制
- 创建 `~/RustLuaMud/` 数据目录（profiles/、scripts/、logs/）
- 生成示例角色配置和示例 Lua 脚本

支持参数：
- `--nightly`：下载 nightly 版
- `--gitee`：从 Gitee 镜像下载

---

## 五、开发环境搭建

### 5.1 系统要求

| 项目 | 要求 |
|------|------|
| 操作系统 | Linux（已测试）/ macOS / Windows |
| CPU | x86_64、i686 或 aarch64 |
| 内存 | 最低 512MB，推荐 2GB（10 连接） |
| 终端 | 支持 UTF-8 + ANSI 转义序列 |
| Rust | 1.70+（edition 2021） |
| C 编译器 | gcc/g++（用于编译 rusqlite 等原生依赖） |

### 5.2 从源码编译

```bash
# 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source $HOME/.cargo/env

# 安装 C 编译器
sudo apt install build-essential

# 克隆并编译
git clone https://github.com/llps112/RustLuaMud.git
cd RustLuaMud
cargo build --release

# 运行
./target/release/RustLuaMud
```

### 5.3 国内镜像加速

```toml
# ~/.cargo/config.toml
[build]
jobs = 2

[source.crates-io]
replace-with = "sjtug"

[source.sjtug]
registry = "sparse+https://mirrors.sjtug.sjtu.edu.cn/crates.io-index/"
```

### 5.4 开发工具链

```bash
# 格式化检查
cargo fmt --all -- --check

# Lint 检查（零 warning 要求）
cargo clippy -- -D warnings

# 运行测试（推荐 nextest）
cargo nextest run
```

---

## 六、CI/CD 流程

### 6.1 GitHub Actions

| 工作流 | 触发条件 | 功能 |
|--------|---------|------|
| **CI** | 每次 push/PR（有 paths 过滤） | 运行测试、clippy、fmt 检查 |
| **Release** | 创建 tag `vX.Y.Z` | 构建多架构二进制，创建 GitHub Release |
| **Nightly** | 每次 push main（仅 Rust 源码变更） | 自动构建并同步到 GitHub + Gitee Release |
| **Audit** | 每周定时 | 依赖安全审计（`cargo audit`） |

### 6.2 Gitee Go

| 工作流 | 触发条件 | 功能 |
|--------|---------|------|
| **nightly.yml** | 定时触发 | Gitee 侧 Nightly 构建 |

### 6.3 发布流程

1. Bump 版本号（`Cargo.toml`）
2. `git commit` + `git push`
3. 创建 tag `vX.Y.Z` + `git push --tags`
4. `gh run list` 确认 release workflow 已触发即完成
5. GitHub Actions 自动完成二进制编译、Release 创建和 Gitee 同步

---

## 七、API 兼容层

### 7.1 MUSHclient API 实现

`src/lua/api.rs`（115.8KB）实现了 MUSHclient 的核心 API，覆盖以下类别：

| 类别 | 主要 API |
|------|---------|
| 触发器 | `AddTrigger` / `AddTriggerEx` / `DeleteTrigger` / `EnableTrigger` / `GetTriggerInfo` / `SetTriggerOption` |
| 别名 | `AddAlias` / `DeleteAlias` / `GetAliasInfo` / `SetAliasOption` |
| 定时器 | `AddTimer` / `DeleteTimer` / `ResetTimer` / `EnableTimer` / `GetTimerInfo` |
| 命令与输出 | `Send` / `Execute` / `Note` / `Tell` / `ColourNote` / `Simulate` / `SetStatus` |
| 变量 | `GetVariable` / `SetVariable` / `DeleteVariable` / `get` / `set` |
| 网络 | `IsConnected` / `Connect` / `Disconnect` / `OnConnect` |
| 数据库 | `sqlite3.open` / `conn:execute` / `conn:set_gbk` |
| 样式 | `GetStyle` / `RGBColourToName` |
| 扩展 | `dofile`（自动 GBK 转码）、`rex`（正则）、`bit`（位运算）、`json_encode/decode` |

### 7.2 浮动面板 API（扩展）

RustLuaMud 独有的扩展 API（非 MUSHclient 标准）：
- `SetPanel(name, x, y, w, h, text, [buttons])` — 创建/更新浮动面板
- `RemovePanel(name)` — 移除浮动面板
- `RegisterPanelHandler(panel_name, callback)` — 注册面板按钮点击回调

### 7.3 API 文档

完整的 API 文档位于 `help/api/` 目录，共 50 个文件：
- `mushclient-*.md`：各 API 函数的详细签名和说明
- `mushclient-api-index.md`：API 总索引
- 分类文档：`triggers.md`、`aliases.md`、`timers.md`、`variables.md`、`database.md`、`panels.md` 等
- 常量表：`constants.md`（trigger_flag、alias_flag、timer_flag、custom_colour、sendto、error_code）

### 7.4 兼容性原则

- 所有常量表、标志位、code 映射 100% 匹配 MUSHclient GitHub 源码（`lua_methods.cpp`）
- 禁止自行扩展或遗漏 MUSHclient 已有的常量条目
- 未实现的 API 返回空串 `""` 或 `0` 或 `false`

---

## 八、游戏逻辑架构

### 8.1 门派任务系统（MP）

15 个门派各有独立的任务脚本（`michen_mp_*.lua`），共享统一的 EXP 周期管理机制：
- 每获得一次 EXP，重置 3600 秒周期计时器
- 周期内 EXP 有上限，达到后停止获取
- 特殊门派有独立逻辑：丐帮双周期（beg1/beg2）、明教御敌独立周期

### 8.2 押镖系统（YB）

`michen_yb.lua` 实现，独立 EXP 周期（上限 5000），包含：
- 路线规划与 GPS 导航集成
- 过河事件处理（中文引号匹配）
- 断线恢复机制

### 8.3 护镖系统（FJ）

`fj.lua` 实现，70 个函数的完整护镖任务逻辑，包含：
- 重名房间抓取处理
- 误报警循环修复
- 护镖路线优化

### 8.4 守城系统（War）

`war_refactor.lua`（1313 行）实现，采用三池锚点制组队算法：
- P1 池（自己的重要 ID）、P2 池（朋友的重要 ID）、P5 池（小号凑人头）
- 32 位溢出算法模拟（`add32` / `multi10_with_overflow`）
- EXP 持久化方案（脏表批处理 + 原子写入）
- 数据与逻辑分离（`war_members_data.lua` + `war_members.lua` + `war_refactor.lua`）

### 8.5 FTB 任务

`fj.lua` 中集成，EXP 机制与其他任务截然不同：
- 由服务端 LPC 全权控制（`ftb_zhu.c`）
- 动态 EXP 上限衰减（每次做完 ~1%+random）
- Lua 侧只负责统计显示，不参与周期管理

### 8.6 GPS 导航系统

`gps.lua`（2350 行）+ `gps_lib.lua`（1074 行）：
- 基于入口房间数据表（`Entrance_table.lua`，2228 行）的路径计算
- 支持跨区域寻路
- 死锁修复与容错处理

### 8.7 战斗与技能

- `kill.lua`：战斗逻辑
- `skills.lua`：技能练习管理（1014 行）
- `xinfa.lua`：心法管理
- `perform.lua`：表演系统
- `pk.lua`：PK 逻辑

---

## 九、技术栈

| 组件 | 库 | 版本 |
|------|-----|------|
| 异步运行时 | tokio | 1.x（full features） |
| 终端控制 | crossterm | 0.29（event-stream） |
| Lua 引擎 | mlua | 0.12（LuaJIT + vendored） |
| 正则 | regex | 1.x |
| 数据库 | rusqlite | 0.40（bundled） |
| 配置解析 | toml + serde | 1.1 / 1.x |
| 编码 | encoding_rs | 0.8 |
| SOCKS5 | tokio-socks | 0.5 |
| 时间 | chrono | 0.4 |
| JSON | serde_json | 1.x |
| Unicode | unicode-width | 0.2 |

---

## 十、项目结构

```
RustLuaMud/
├── src/                       # Rust 引擎源码
│   ├── main.rs                # 入口
│   ├── lib.rs                 # 库入口（集成测试入口）
│   ├── app.rs                 # 应用主逻辑
│   ├── config.rs              # 配置解析
│   ├── connection/            # 连接管理
│   │   ├── manager.rs         # 多 session 管理
│   │   ├── session.rs         # TCP 会话
│   │   └── rate_limiter.rs    # 令牌桶限速
│   ├── ui/                    # 终端 UI
│   │   ├── terminal.rs        # 渲染主逻辑
│   │   ├── ansi.rs            # ANSI SGR 解析
│   │   └── input.rs           # 输入处理
│   ├── log/                   # 日志系统
│   │   ├── logger.rs          # 日志记录器
│   │   ├── panic_hook.rs      # panic 捕获
│   │   └── debug.rs           # 调试辅助
│   └── lua/                   # Lua 引擎 + API
│       ├── engine.rs          # LuaJIT 管理
│       ├── api.rs             # MUSHclient API
│       ├── triggers.rs        # 触发器系统
│       ├── aliases.rs         # 别名系统
│       ├── timers.rs          # 定时器系统
│       ├── commands.rs        # 命令管理
│       ├── database.rs        # SQLite3
│       ├── helpers.rs         # 辅助函数
│       ├── index.rs           # 索引系统
│       ├── types.rs           # 类型定义
│       └── tests.rs           # 集成测试
├── scripts/                   # Lua 脚本
│   ├── bootstrap.sh           # 一键初始化
│   ├── class-utf8/            # UTF-8 开发源
│   ├── class/                 # GBK 运行时版本
│   ├── private/               # 私有子模块
│   └── lua/                   # Lua 依赖库
├── profiles/                  # 角色配置文件
├── logs/                      # 日志文件
├── help/                      # 文档
│   ├── api/                   # API 参考（50 文件）
│   └── commands/              # 命令指南
├── docs/                      # 项目文档
├── LPC/                       # 服务端 LPC 参考代码
├── .github/workflows/         # GitHub Actions CI/CD
├── .gitee/workflows/          # Gitee Go CI/CD
├── .qoder/rules/              # 开发规范（权威源）
├── .trae/rules/               # 已迁移，指向 .qoder/rules/
├── Cargo.toml                 # Rust 项目配置
└── README.md                  # 项目说明
```
