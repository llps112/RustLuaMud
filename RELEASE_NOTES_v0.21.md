# RustLuaMud v0.21 Release Notes

**发布日期**: 2026-06-16
**版本类型**: Release

---

## 版本概述

v0.21 专注于提升 session 切换的便捷性和终端兼容性。新增四种切换方式，确保在不同终端环境（xterm、tmux、XShell 等）下都能流畅操作。

---

## 新增功能

### 1. 鼠标点击切换 session
- 点击顶部状态栏的 session 标签即可切换
- 使用 SGR 鼠标模式，兼容主流终端
- 程序退出时自动释放鼠标捕获

### 2. Alt+方向键循环切换
- **Alt+←**: 切换到前一个 session（循环）
- **Alt+→**: 切换到后一个一个 session（循环）
- 在所有终端和 tmux 中行为一致，无兼容性问题

### 3. `/switch` 命令
- `/switch 3` — 按编号切换
- `/switch 角色名` — 按名称切换（不区分大小写）
- 适用于 session 数量超过 10 个的场景

### 4. xterm 8-bit 模式兼容
- 修复 xterm 中 Alt+数字 发送高位字符（U+00B0~U+00B9）无法识别的问题

---

## 修复

- Alt+方向键修饰符比较从精确匹配改为包含匹配，避免 NUM_LOCK 等额外修饰符导致失效
- 鼠标捕获改用 crossterm 标准 API，替代手写 ANSI 转义序列

---

## 完整变更日志

| 提交 | 说明 |
|------|------|
| c0cc646 | feat: 添加 /switch 命令，支持按编号或名称切换 session |
| edfe656 | style: auto-fix formatting |
| 869c21d | fix: Alt+方向键修饰符比较改用contains，鼠标捕获改用crossterm API |
| a233e87 | feat: 支持鼠标点击状态栏 session 标签切换连接 |
| 938262b | feat: 添加 Alt+←/→ 循环切换 session，兼容不同终端 |
| ac066ae | fix: 支持 xterm 8-bit 模式的 Alt+数字切换 session |

---

## 升级说明

```bash
git pull
cargo build --release
```

---

## 相关链接

- **项目主页**: https://github.com/llps112/RustLuaMud
- **问题反馈**: https://github.com/llps112/RustLuaMud/issues
