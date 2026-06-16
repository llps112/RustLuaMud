# RustLuaMud v0.20 Pre-release Notes

**发布日期**: 2026-06-16  
**版本类型**: Pre-release (预发布)  
**版本状态**: Beta 测试阶段

---

## 📋 版本概述

v0.20 是一个重要的功能增强版本，新增了 SOCKS5 代理支持、输出历史滚动、多实例运行等实用功能，同时修复了多个稳定性问题。本版本已在 Ubuntu 22.04 LTS 上经过 30+ 小时的实际运行测试，功能稳定可靠。

**注意**: 这是预发布版本，主要功能已稳定，但可能在其他操作系统上存在兼容性问题。建议在测试环境验证后再用于生产。

---

## ✨ 新增功能

### 1. SOCKS5 代理支持
- 每个角色可独立配置 SOCKS5 代理服务器
- 支持带认证和不认证两种模式
- 通过 `socks5_enable` 开关控制，默认关闭
- **用途**: 多开挂机时规避服务器同 IP 数量限制

**配置示例**:
```toml
socks5_enable = true
socks5_host = "127.0.0.1"
socks5_port = 1080
socks5_username = "user"  # 可选
socks5_password = "pass"  # 可选
```

### 2. 输出历史滚动
- **PageUp / PageDown**: 翻页查看历史输出（每次滚动半屏）
- **End**: 输入框为空时回到底部，有内容时光标移到行尾
- 新输出不会自动回底，保持当前浏览位置
- 支持 5000 行历史缓冲

### 3. 多实例运行
- 新增 `--profiles` 命令行参数
- 支持指定不同的角色配置目录
- 可同时运行多个独立实例

**使用示例**:
```bash
# 实例 1: 使用 profiles/ 目录
./RustLuaMud

# 实例 2: 使用 profiles2/ 目录
./RustLuaMud --profiles profiles2
```

### 4. /close 命令
- 彻底关闭并移除 session
- 释放相关资源（TCP 连接、Lua 引擎等）
- 修复了关闭 session 后可能导致崩溃的问题

---

## 🔧 重要改进

### 稳定性增强
1. **修复 /close 崩溃问题**
   - 问题: 关闭 session 后，异步任务仍在发送事件导致数组越界
   - 解决: 在 `handle_manager_event` 中添加边界检查
   
2. **修复 /load reload 状态丢失**
   - 问题: 重载脚本后连接状态（connected/host/port 等）丢失，导致指令失效
   - 解决: 新增 `ConnectionState` 结构体，reload 时保存并恢复连接状态

3. **内存占用稳定**
   - 10 个 session 运行 30+ 小时，内存稳定在 364MB
   - 无内存泄漏问题

### 代码质量
- 清理所有编译警告（20 个）
- 为预留 API 添加 `#[allow(dead_code)]` 标记
- 保留对外 API 完整性

---

## 📊 测试覆盖

- **单元测试**: 629 个测试全部通过
- **集成测试**: 2 个实例 × 14 个 session，全脚本加载，运行 30+ 小时无异常
- **测试场景**:
  - 多连接并发
  - 脚本重载
  - Session 关闭
  - 断线重连
  - SOCKS5 代理连接

---

## 🐛 已知问题

### 1. 跨平台兼容性
**状态**: 仅在 Ubuntu 22.04 LTS 上测试

**潜在风险**:
- **路径分隔符**: 代码中使用 `"/"` 硬编码，Windows 上可能需要调整
- **信号处理**: Unix 信号（SIGTERM 等）在 Windows 上不支持
- **终端兼容**: crossterm 库理论上支持跨平台，但未实际验证

**建议**:
- ✅ Linux (Ubuntu 22.04): 已验证，推荐使用
- ⚠️ macOS: 理论上兼容，未测试
- ⚠️ Windows: 可能存在路径和信号问题，未测试
- ⚠️ 其他 Linux 发行版: 理论上兼容，未测试

### 2. 编译警告
**状态**: 已清理所有警告

部分代码标记为 `#[allow(dead_code)]`，这些是预留给 Web UI 的 API 接口：
- `eval_to_string`, `set_port`, `timer_intervals`
- `with_logging`, `logs`, `clear_logs`, `process_line`, `scan_into`, `consume_csi_params`
- `reset_sequence`

---

## ⚠️ 注意事项

### 1. 系统要求
- **操作系统**: Linux (推荐 Ubuntu 22.04+) / macOS / Windows (未测试)
- **CPU**: x86_64 或 aarch64 (需要 JIT 支持)
- **内存**: 最低 512MB，推荐 2GB (10 连接)
- **终端**: 支持 UTF-8 和 ANSI 转义序列

### 2. 依赖要求
- Rust 1.70+ (edition 2021)
- LuaJIT (通过 mlua 自动编译)
- SQLite3 (通过 rusqlite 自动编译)

### 3. 升级建议
从 v0.1.x 升级：
1. 备份 `profiles/` 目录
2. 编译新版本
3. 为需要代理的角色添加 SOCKS5 配置
4. 测试 `/load reload` 和 `/close` 命令

### 4. 回滚方案
如遇问题，可快速回滚到 v0.1.2：
```bash
git checkout v0.1.2
cargo build --release
```

---

## 📦 编译说明

### 环境准备

#### Ubuntu 22.04 LTS
```bash
# 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# 安装编译依赖
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev

# 验证安装
rustc --version  # 应该 >= 1.70
cargo --version
```

#### macOS
```bash
# 安装 Rust
brew install rust

# 或使用 rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

#### Windows (未测试)
```powershell
# 安装 Rust
winget install Rustlang.Rustup

# 或使用 rustup-init.exe
# 下载地址: https://rustup.rs/
```

### 编译步骤

```bash
# 1. 克隆仓库
git clone https://github.com/llps112/RustLuaMud.git
cd RustLuaMud

# 2. 切换到 v0.20 版本
git checkout v0.20

# 3. 编译 release 版本
cargo build --release

# 4. 编译产物
# Linux/macOS: target/release/RustLuaMud
# Windows: target/release/RustLuaMud.exe
```

### 编译选项

#### 优化编译（推荐）
```bash
# 使用 LTO 优化，减小二进制体积
RUSTFLAGS="-C lto=fat -C codegen-units=1" cargo build --release
```

#### 调试编译
```bash
# 包含调试信息
cargo build
# 产物: target/debug/RustLuaMud
```

### 常见问题

#### Q1: 编译失败 "linker 'cc' not found"
**解决**: 安装 C 编译器
```bash
# Ubuntu/Debian
sudo apt install build-essential

# macOS
xcode-select --install
```

#### Q2: LuaJIT 编译失败
**解决**: 确保有足够内存和 swap
```bash
# 检查内存
free -h

# 如内存不足，临时增加 swap
sudo fallocate -l 2G /swapfile
sudo chmod 600 /swapfile
sudo mkswap /swapfile
sudo swapon /swapfile
```

#### Q3: 编译时间过长
**说明**: 首次编译需要编译 LuaJIT 和 SQLite3，约需 5-10 分钟
**建议**: 使用 `cargo build --release -j 4` 指定并行编译数

---

## 📥 安装说明

### 安装步骤

```bash
# 1. 编译（见上文）

# 2. 复制到系统路径（可选）
sudo cp target/release/RustLuaMud /usr/local/bin/

# 或直接运行
./target/release/RustLuaMud
```

### 配置目录

```bash
# 创建配置目录
mkdir -p profiles scripts logs

# 复制示例配置
cp profiles/example.toml profiles/mychar.toml

# 编辑配置
nano profiles/mychar.toml
```

### 验证安装

```bash
# 1. 运行程序
./RustLuaMud

# 2. 检查启动信息
# 应该看到:
# - 加载的配置文件列表
# - 连接状态
# - 终端界面正常显示

# 3. 测试基本命令
/list          # 查看连接列表
/lua print("Hello")  # 测试 Lua 执行
```

### 卸载

```bash
# 1. 停止程序
# Ctrl+C 或 Ctrl+D

# 2. 删除二进制文件
rm /usr/local/bin/RustLuaMud  # 如果安装到系统路径
# 或
rm target/release/RustLuaMud  # 如果保留在编译目录

# 3. 删除配置和数据（可选）
rm -rf profiles/ scripts/ logs/
```

---

## 📝 完整变更日志

### 新增
- ✨ SOCKS5 代理支持（每个角色独立配置）
- ✨ 输出历史滚动（PageUp/PageDown/End）
- ✨ `--profiles` 命令行参数（多实例运行）
- ✨ `/close` 命令（彻底移除 session）
- ✨ 629 个单元测试

### 修复
- 🐛 `/close` 命令导致的数组越界崩溃
- 🐛 `/load reload` 后连接状态丢失
- 🐛 编译警告（20 个）

### 改进
- 🔧 代码质量提升（清理警告、添加文档）
- 🔧 内存占用稳定（30+ 小时无泄漏）
- 🔧 错误处理增强

---

## 🔗 相关链接

- **项目主页**: https://github.com/llps112/RustLuaMud
- **问题反馈**: https://github.com/llps112/RustLuaMud/issues
- **文档**: https://github.com/llps112/RustLuaMud/tree/main/help

---

## 📞 支持与反馈

如遇到问题或有建议，请：
1. 查看 [故障排查](#故障排查) 章节
2. 在 GitHub Issues 中搜索类似问题
3. 如未解决，提交新的 Issue 并附上：
   - 操作系统版本
   - Rust 版本 (`rustc --version`)
   - 错误日志（启用 `RUST_BACKTRACE=1`）
   - 复现步骤

---

**感谢使用 RustLuaMud!** 🎉
