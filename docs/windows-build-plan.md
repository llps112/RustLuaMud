# Windows 平台编译实施规划

> **状态：规划中，未来有机会再实施。** 本文档仅为可行性分析与实施方案预案，当前无实施计划。
> 项目当前目标平台为 Linux（J1800 + 2GB 挂机场景），Windows 支持属于远期可选项。

## 1. 背景与目标

将 RustLuaMud 编译为 Windows 平台 `.exe`，使其可在 Windows 主机或虚拟机上运行。
产物形态：`target/<triple>/release/RustLuaMud.exe`（`[[bin]] name = "RustLuaMud"`）。

## 2. 现状评估：平台相关代码盘点

编译为 Windows 的主要障碍集中在两处 Unix 专属代码：

| 位置 | 内容 | Windows 兼容性 | 处理方式 |
|------|------|---------------|----------|
| `src/daemon.rs` | `libc::fork` / `setsid` / `openpty` / `ioctl(TIOCSWINSZ)` / `dup2` / `kill(pid, 0)` 守护进程 + 伪终端接管 | ❌ API 在 Windows 不存在，`daemon.rs` 会直接编译失败 | 整个模块用 `#[cfg(unix)]` 隔离；Windows 侧禁用 `--daemon` 参数（提示不支持）或另做 Windows 服务方案 |
| `src/main.rs` / `src/lib.rs` | 无条件 `use rust_lua_mud::daemon` 并调用 `daemonize` | ❌ 随上条失败 | 同样加 `#[cfg(unix)]` 门控，Windows 编译路径跳过守护进程逻辑 |
| `src/connection/session.rs` (约 L330) | TCP keepalive（`SO_KEEPALIVE` / `TCP_KEEPIDLE` 等） | ⚠️ 已有 `#[cfg(target_os = "linux")]` 门控，可编译，但 Windows 上 **keepalive 功能缺失** | 可选：补一个 `#[cfg(windows)]` 分支（`SIO_KEEPALIVE_VALS` via `WSAIoctl`），或引入 `socket2` crate 统一处理 |

其余代码（tokio、crossterm、encoding_rs、regex、rusqlite、tokio-socks、tempfile、chrono）均为跨平台库，无源码级障碍。

## 3. 依赖库 Windows 兼容性分析

| 依赖 | Windows 兼容性 | 注意事项 |
|------|---------------|----------|
| `tokio` (full) | ✅ | Windows 走 IOCP，完全支持 |
| `crossterm` 0.29 | ✅ | 需要 Win10+ 控制台 VT 模式（见第 5 节） |
| `mlua` (luajit, vendored) | ⚠️ | LuaJIT 在 Windows **仅支持 x86 / x86_64**，不支持 ARM64；vendored 构建需要 C 编译器 |
| `rusqlite` (bundled) | ✅ | bundled 通过 `cc` crate 编译 SQLite C 源码，需要 C 编译器 |
| `encoding_rs` / `regex` / `serde` / `chrono` / `tempfile` / `tokio-socks` | ✅ | 纯 Rust，无障碍 |
| `libc` | ✅ | 本身跨平台，但本项目用到的 `fork`/`openpty` 等函数仅存在于 unix 目标 |

**结论**：C 编译器是硬需求（LuaJIT + SQLite 两个 vendored/bundled 依赖），
架构上只能选 `i686` 或 `x86_64`，不能选 ARM。

## 4. 编译方案

### 方案 A：Windows 本机编译（推荐，未来实施时首选）

1. 安装 [rustup](https://rustup.rs)，默认选择 `x86_64-pc-windows-msvc` 工具链
2. 安装 **Visual Studio 2022 Build Tools**，勾选「使用 C++ 的桌面开发」工作负载（提供 MSVC 编译器 + Windows SDK，满足 `cc` crate 编译 LuaJIT/SQLite 的需求）
3. 拉取代码（含子模块：`git clone --recurse-submodules`）
4. `cargo build --release`
5. 产物：`target\x86_64-pc-windows-msvc\release\RustLuaMud.exe`（或默认目标目录）

### 方案 B：Linux 交叉编译（在当前开发机产出 exe）

- `x86_64-pc-windows-gnu` 目标 + `mingw-w64-gcc`：
  ```bash
  sudo apt install mingw-w64
  rustup target add x86_64-pc-windows-gnu
  cargo build --release --target x86_64-pc-windows-gnu
  ```
  注意：mingw 下 LuaJIT vendored 编译偶有坑（需确认 `cc` crate 正确识别工具链），
  且产物运行时依赖 `libgcc`/`libwinpthread` DLL（静态链接可规避）。
- `x86_64-pc-windows-msvc` 从 Linux 交叉编译需要 `cargo-xwin` 或 `xwin` 拉取 MSVC SDK，配置成本较高，不推荐。

### 方案 C：CI 构建（建议随方案 A 一并落地）

GitHub Actions 增加 windows job（`runs-on: windows-latest`），复用现有 `ci.yml`
的 fmt/clippy/test 流程，追加 `cargo build --release` 产出 exe 到 Actions Artifacts。

## 5. 代码改造清单（实施时的最小改动集）

1. **`src/daemon.rs` + `src/main.rs` + `src/lib.rs`**：
   - `daemon` 模块声明与所有调用点加 `#[cfg(unix)]`
   - `--daemon` 参数在非 unix 目标下打印「当前平台不支持守护进程模式」并退出
   - `daemon.rs` 内的单元测试同样加 `#[cfg(unix)]`
2. **`src/connection/session.rs`**：keepalive 的 `#[cfg(target_os = "linux")]` 块保持不动即可编译；
   如需 Windows 等效功能，追加 `#[cfg(windows)]` 分支（`WSAIoctl` + `SIO_KEEPALIVE_VALS`），
   或接受降级（Windows 侧仅默认 keepalive，不设置精细参数）。
3. **控制台编码**：Windows 中文系统控制台默认代码页为 GBK (936)，程序输出为 UTF-8。
   - 应用侧通过 Unicode API（`WriteConsoleW`）写入，编码转换与代码页无关；
   - **禁止** `chcp 65001`：旧 conhost 在 UTF-8 代码页下把所有字符按 1 格存储，
     汉字/制表符/●★ 等全角字形挤进单格导致字符叠压（实测 936 下这些字符
     均为带 LVB 前导/尾随标记的 2 格，与 `gbk_full_width` 宽度表一致）；
   - 保持系统默认代码页（中文 Windows 即 936）即可，启动脚本不再设置 chcp。
4. **路径与部署**：程序依赖相对路径（`profiles/`、`scripts/`、`logs/`），
   Windows 上需将 exe 与这些目录放在同一工作目录，注意 `cmd` 中工作目录与双击运行目录的差异。
5. **测试**：按项目规范，`#[cfg]` 改动需确认现有 809 个测试在 Windows 目标下仍可编译通过
   （部分测试可能依赖 Unix 行为，需逐一门控）。

## 6. 运行注意事项（Windows）

- **守护进程模式不可用**：`--daemon` 为 Unix 专属。替代方案：
  - `start /min RustLuaMud.exe` 最小化窗口后台运行
  - 任务计划程序开机自启
  - 如需服务化，可用第三方工具（如 NSSM）包装为 Windows 服务（未经评估）
- **终端要求**：crossterm 的 raw mode 与 ANSI 渲染要求控制台支持 VT 序列：
  Windows 10 1607+ 的 cmd/PowerShell（自动启用 VT）或 Windows Terminal；
  更老的系统（Win7/Win8）不支持，**不建议支持**。
- **脚本编码**：`scripts/class/`（GBK）+ `encoding_rs` 运行时转码机制与平台无关，Windows 下无需调整。
- **防火墙**：游戏直连与 SOCKS5 代理出站需放行 `RustLuaMud.exe`。
- **日志**：`logs/` 目录按账号名 + 时间滚动写入，与 Linux 行为一致，无平台差异。

## 7. Windows 系统要求

### 编译环境要求

| 项目 | 最低要求 | 说明 |
|------|----------|------|
| 操作系统 | **Windows Server 2016** / Windows 10 19041+ (20H1) | VS 2022 Build Tools 安装要求 Windows 10 19041+ 或 Windows Server 2016+ |
| 编译工具链 | Rust stable（`x86_64-pc-windows-msvc`）+ Visual Studio 2022 Build Tools（C++ 桌面开发工作负载） | 提供 MSVC 编译器 + Windows SDK，满足 `cc` crate 编译 LuaJIT/SQLite 的需求 |
| 磁盘空间 | ≥ 15 GB | Build Tools ~8 GB + Rust 工具链与 target 目录 ~5 GB + 源码 |
| 内存 | ≥ 4 GB（建议 8 GB） | 并行编译链接较快 |

### 运行环境要求

| 项目 | 最低要求 | 说明 |
|------|----------|------|
| 操作系统 | **Windows Server 2016** / Windows 10 1607+ | Windows Server 2016 (build 14393) 与 Windows 10 1607 同内核，支持控制台 VT 序列；crossterm 的 raw mode 与 ANSI 渲染依赖此特性 |
| 架构 | x86_64（或 i686） | LuaJIT 限制，不支持 ARM64 |
| 内存 | ≥ 512 MB | 项目资源占用极低（Linux 侧 10 连接实测 2GB 整机无压力） |
| 终端 | Windows Terminal 或系统自带 cmd/PowerShell | 需支持 ANSI/VT |
| 运行库 | MSVC 运行时（随 exe 分发需 VC++ Redistributable，或静态链接 CRT 规避） | 方案 A 产物默认动态链接 CRT |

**Windows Server 版本选择理由**：
- **Windows Server 2016** (build 14393) 是最低支持版本，原因：
  1. 控制台 VT 序列支持从 Windows 10 1607 / Server 2016 开始引入（crossterm 必需）
  2. Rust 工具链（msvc 目标）与 VS 2022 Build Tools 均要求此版本或更高
  3. tokio 等依赖使用的 Windows API 最低要求此版本
- **Windows Server 2019** (build 17763) 是更稳妥的选择：
  1. VT 模式默认启用，无需额外配置
  2. 控制台渲染性能更好
  3. 长期支持版本（LTSC），适合生产环境部署

## 8. 风险与未决事项

1. **LuaJIT vendored 在 MSVC 下的构建验证**：mlua 官方声称支持，但需实际验证
   `cargo build` 在干净 Windows 环境一次通过（历史上 LuaJIT 对新版 MSVC 偶有兼容问题）。
2. **crossterm 在中文 Windows 控制台的 CJK 宽字符对齐**：Linux 侧已验证，
   Windows 控制台字体渲染差异可能导致面板错位，需实测。
3. **测试矩阵**：809 个单元测试需在 Windows 目标下跑通，
   涉及文件路径、行尾（CRLF）、编码的测试可能需要适配。
4. **分发方式**：是否随 Release 分发 exe、是否静态链接 CRT（`+crt-static`），待实施时决定。
5. **维护成本**：双平台意味着 CI 时间与问题排查面翻倍，
   与项目「J1800 Linux 挂机」的定位权衡，是暂缓实施的主要原因。

## 9. 实施步骤摘要（未来实施时参照）

1. 按第 5 节完成 `#[cfg]` 门控改造，Linux 侧 `cargo fmt` / `clippy` / `nextest` 三重验证不回归
2. 搭建 Windows 编译环境（方案 A），首次 `cargo build --release` 并记录问题
3. 修复编译错误，跑通 `cargo nextest run`
4. 实机验证：连接游戏服务器、触发器/别名/定时器、GBK 脚本加载、控制台中文显示
5. 补 `#[cfg(windows)]` keepalive 分支（可选）与 `SetConsoleOutputCP` 处理（可选）
6. GitHub Actions 增加 windows job，Release 附带 exe 产物

---

*本文档为预案性质：未来有机会再实施。在此之前，请勿依据本文档发起代码改造。*
