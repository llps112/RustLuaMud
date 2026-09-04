# RustLuaMud 项目说明

## 项目概述

- **Rust + LuaJIT 架构的终端 MUD 客户端引擎**，面向 7×24 小时无 GUI 挂机场景
- 兼容 MUSHclient 脚本 API（触发器、别名、定时器、变量、日志、数据库、样式查询等）
- UTF-8/GBK 双编码体系：UTF-8 为开发编码，GBK 运行时由 `iconv` 自动生成
- 单实例最多 10 个并发连接，前台/后台无缝切换，每个角色独立 SOCKS5 代理
- 令牌桶 + 滑动窗口双重限速保护（burst_size + cmds_per_sec + cmd_interval_ms，叠加 window_limit + window_duration_ms），防反 flood
- 完整 ANSI SGR 解析、CJK 宽字符对齐、浮动面板 API
- 极低资源占用：J1800 + 2GB 内存即可流畅运行 10 连接
- 版本：v0.9.5，Rust edition 2021

### 技术栈

| 组件 | 库 |
|------|-----|
| 异步运行时 | tokio |
| 终端控制 | crossterm |
| Lua 引擎 | mlua (LuaJIT, vendored) |
| 正则 | regex |
| 数据库 | rusqlite (bundled) |
| 配置解析 | toml + serde |
| 编码 | encoding_rs |
| SOCKS5 代理 | tokio-socks |
| JSON | serde_json |
| 时间 | chrono |

## 关键目录结构

```
src/                          — Rust 源码
├── main.rs                   — 程序入口
├── lib.rs                    — 库入口（集成测试入口）
├── app.rs                    — 应用主逻辑（事件循环、UI 调度）
├── config.rs                 — TOML 配置解析
├── connection/               — 网络连接模块
│   ├── manager.rs            — 连接管理器（多 session 调度）
│   ├── session.rs            — 单个连接会话（TCP + 编解码）
│   └── rate_limiter.rs       — 限速器（令牌桶 + 滑动窗口）
├── ui/                       — 终端 UI 渲染模块
│   ├── terminal.rs           — 终端渲染主逻辑
│   ├── input.rs              — 输入框处理
│   └── ansi.rs               — ANSI SGR 解析器
├── log/                      — 日志模块
│   ├── logger.rs             — 日志写入
│   ├── panic_hook.rs         — panic 捕获与日志记录
│   └── debug.rs              — 调试输出
└── lua/                      — Lua 脚本引擎模块
    ├── engine.rs             — LuaJIT 引擎初始化与管理
    ├── api.rs                — MUSHclient 兼容 API 实现
    ├── triggers.rs           — 触发器系统
    ├── aliases.rs            — 别名系统
    ├── timers.rs             — 定时器系统
    ├── commands.rs           — Lua 侧命令处理
    ├── database.rs           — SQLite3 集成
    ├── helpers.rs            — 辅助函数
    ├── index.rs              — name/group 索引管理
    ├── types.rs              — 类型定义与常量
    └── tests.rs              — Lua 引擎单元测试

scripts/                      — Lua 脚本目录
├── class-utf8/               — Lua 开发源（UTF-8 编码，主要编辑此目录）
├── class/                    — Lua 运行时（GBK 编码，由 iconv 自动生成，禁止直接编辑）
├── private/                  — 私有子模块（独立 Git 仓库，见 .gitmodules）
├── lua/                      — Lua 公共库（wait.lua, tprint.lua, check.lua）
├── bootstrap.sh              — 一键环境搭建脚本
├── example.lua               — 示例脚本
├── config_*.lua              — 各角色游戏配置脚本
└── michen_xkx.lua            — 游戏辅助脚本

help/                         — 文档
├── api/                      — MushClient API 文档（50 个文件）
├── commands/                 — 命令参考（2 个文件）
└── README.md                 — 文档索引

profiles/ ~ profiles4/        — 多实例运行配置（TOML 格式）
docs/                         — 项目技术文档
LPC/                          — 服务端 LPC 参考代码（本地专用，已 gitignore 不入库；核对游戏文案的权威依据）
class-utf8-old/               — 旧版 Lua 脚本存档（UTF-8）
class-gbk-old/                — 旧版 Lua 脚本存档（GBK）
.trae/documents/              — 历史技术方案文档（76 个文件）
.qoder/rules/                 — 项目开发规则文件（权威源：编码规范、API 兼容、Git 工作流等）
.trae/rules/                  — 符号链接，指向 .qoder/rules/（供 Trae 复用同一套规则）
```

## 构建与测试命令

```bash
# 构建
cargo build                           # 调试构建
cargo build --release                 # 发布构建
./target/release/RustLuaMud           # 运行程序

# 代码质量（提交前必须全部通过）
cargo fmt                             # 代码格式化
cargo clippy -- -D warnings           # 静态检查（零警告）
cargo nextest run                     # 运行测试

# 编码同步（GBK 文件由 iconv 自动生成，禁止手动编辑）
# 必须用重定向，不要用 iconv -o：GNU libiconv 独立版（Windows /usr/bin/iconv）不支持该选项
iconv -f UTF-8 -t GBK scripts/class-utf8/<file> > scripts/class/<file>
# 注：子模块的 hooks/pre-commit 会在提交时自动同步（需先 git -C scripts/private config core.hooksPath hooks）
# 主仓库的 githooks/post-checkout 与 post-merge 不做编码同步，只同步脚本本地副本
```

## 核心约束

1. **编码双轨制**：`scripts/class-utf8/` 是开发源（UTF-8），`scripts/class/` 是运行时（GBK，iconv 生成）。**绝对禁止直接编辑 GBK 文件中的中文**。
2. **MushClient API 100% 兼容**：所有 API 常量表（`trigger_flag`、`alias_flag`、`timer_flag`、`custom_colour`、`sendto`、`error_code`/`error_desc`）必须完整匹配上游源码。
3. **正则双引擎**：Rust 侧使用 PCRE（regex crate），Lua 侧使用 Lua 模式，转义方式完全不同。不可混用。
4. **子模块隐私隔离**：`scripts/private` 是私有仓库（地址见 `.gitmodules`）。公开侧按 L1/L2/L3 三级判定：工程结构（`class/`、`class-utf8/`、`hooks/`）与机制性文件名可写，**业务实现与 raw URL 禁止新增**。详见 `submodule-privacy.md`。
5. **Git 子模块工作流**：子模块独立提交，主仓库仅更新指针。子模块指针更新不触发 CI。
6. **测试覆盖**：新增功能必须附带测试，`src/lua/tests.rs` 包含大量 Lua 引擎集成测试。

详细规则见 `.qoder/rules/` 目录下的规则文件：
- `script-encoding.md` — 脚本编码规范
- `mushclient-api.md` — MushClient API 兼容规范
- `regex-pattern.md` — 正则双引擎使用规范
- `submodule-privacy.md` — 子模块隐私隔离规范
- `git-workflow.md` — Git 工作流规范
- `code-quality-workflow.md` — 代码质量工作流
- `debug-output.md` — 调试输出规范

## CI/CD 概览

| 工作流 | 触发条件 | 说明 |
|--------|----------|------|
| `ci.yml` | push / PR | 测试、clippy、fmt 检查 |
| `nightly.yml` | push main（仅 Rust 源码变更） | 每夜构建，同步 GitHub + Gitee Release |
| `release.yml` | Tag 推送 | 自动构建 GitHub Release |
| `audit.yml` | 每周定时 | 依赖安全审计 |
| `.gitee/workflows/nightly.yml` | Gitee 侧 | 国内镜像 Nightly 构建 |

## 开发工具

- `scripts/bootstrap.sh` — 一键环境搭建（支持 GitHub / Gitee 下载源）
- `tools/check_ns_leak.py` — Lua 命名空间泄漏检测
- `githooks/post-checkout` / `githooks/post-merge` — Git 钩子（自动同步 UTF-8 → GBK 文件）
- `scripts/build.sh` — 多架构构建脚本（支持 `--arch i686`）

## 路线图摘要

当前版本聚焦于核心稳定性和挂机体验，远期规划包括：
1. **守护进程模式** — ✅ 已实现（仅 Unix，`--daemon`；Windows 侧明确无此需求）
2. **远程控制 API** — HTTP/Unix Socket 接口，可编程控制
3. **告警通知** — 断线/错误事件主动推送（Telegram/钉钉等）
4. **连接健康监控** — ✅ 核心已实现（心跳检测、指数退避重连）
5. **凭据安全管理** — 环境变量替代明文密码

详见 `ROADMAP.md`。
