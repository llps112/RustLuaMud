# RustLuaMud 项目设计规划

## 1. 项目概述

RustLuaMud 是一个基于 Rust + LuaJIT 的终端 MUD 客户端，面向无 GUI 的 Ubuntu 环境下 7×24 小时挂机场景。目标硬件为家用宽带上的瘦客户机（J1800 + 2GB 内存），要求极致轻量、稳定可靠。

### 核心目标

- 单实例管理约 10 个角色连接
- 仅前台连接渲染输出，后台连接仅记录日志
- 完整的 ANSI 颜色支持
- LuaJIT 脚本引擎驱动触发器/别名/自动化
- 低内存占用（目标 < 100MB 管理 10 连接）
- 断线自动重连

## 2. 硬件约束与设计决策

| 约束 | 影响 |
|------|------|
| J1800 双核 2.0GHz | 避免忙轮询，使用 async/await，最小化 CPU 占用 |
| 2GB 内存 | 每连接独立 Lua 状态需控制内存上限；滚动缓冲区限制行数 |
| 无 GUI | 纯终端 TUI，使用 crossterm 跨平台终端控制 |
| 7×24 运行 | 日志轮转防止磁盘写满；自动重连；优雅退出与恢复 |

## 3. 系统架构

```
┌─────────────────────────────────────────────────────┐
│                     main.rs                         │
│                  入口 & 事件循环                      │
├──────────┬──────────┬───────────┬───────────────────┤
│   UI     │ Connection│   Lua    │      Log          │
│  Layer   │  Manager  │  Engine  │     System        │
├──────────┼──────────┼───────────┼───────────────────┤
│terminal  │ manager   │ engine   │    logger         │
│input     │ session   │ api      │                   │
│ansi      │           │          │                   │
└──────────┴──────────┴───────────┴───────────────────┘
         │              │              │
         ▼              ▼              ▼
   crossterm      tokio TCP       mlua/LuaJIT     文件系统
```

### 3.1 模块职责

#### `main.rs` — 入口与主循环
- 解析配置，初始化各子系统
- 启动 tokio runtime
- 运行终端事件循环（键盘输入 → 分发；网络数据 → 渲染/日志）

#### `connection/` — 连接管理
- **`manager.rs`**: 管理所有 Session 的生命周期（创建/销毁/切换前台）
- **`session.rs`**: 单个 MUD 连接，封装 TCP 流、接收缓冲区、关联的 Lua 状态和日志

#### `lua/` — 脚本引擎
- **`engine.rs`**: LuaJIT 状态封装，脚本加载与执行
- **`api.rs`**: 暴露给 Lua 的 Rust API（发送命令、注册触发器、设置别名等）

#### `ui/` — 终端界面
- **`terminal.rs`**: 屏幕布局渲染（状态栏 + 输出区 + 输入行）
- **`input.rs`**: 键盘输入处理、命令历史、行编辑
- **`ansi.rs`**: ANSI 转义序列解析与渲染

#### `log/` — 日志系统
- **`logger.rs`**: 按连接分文件记录，支持日志轮转（按大小/日期）

## 4. 核心数据流

### 4.1 接收数据流

```
MUD Server
    │ TCP 数据
    ▼
Session (tokio task)
    │ 原始字节流 + ANSI 解析
    ├─► Lua 触发器匹配 ─► 触发动作（发命令/改变量/...）
    ├─► 日志记录（所有连接都记录）
    └─► 若为前台连接 ─► 渲染到终端
```

### 4.2 发送数据流

```
用户输入 / Lua 脚本
    │
    ├─► 别名匹配 ─► 替换为实际命令
    └─► Session TCP 发送
```

### 4.3 连接切换

```
用户按 Alt+1~9 / Alt+n / Alt+p
    │
    ▼
ConnectionManager::switch_foreground(id)
    │
    ├─► 旧前台 → 后台（停止渲染，仅日志）
    ├─► 新前台 → 前台（重绘缓冲区，开始渲染）
    └─► 更新状态栏
```

## 5. 终端 UI 布局

```
┌──────────────────────────────────────────────────┐
│ [1]战士● [2]法师○ [3]道士○ ...        RustLuaMud │  ← 状态栏
├──────────────────────────────────────────────────┤
│                                                  │
│  (前台连接的输出区域，支持 ANSI 颜色)              │  ← 输出区
│  滚动缓冲区，默认保留最近 5000 行                  │
│                                                  │
│                                                  │
├──────────────────────────────────────────────────┤
│ > 输入命令在这里_                                 │  ← 输入行
└──────────────────────────────────────────────────┘
```

- **状态栏**: 显示所有连接的编号、名称、状态（●已连接/○断开/◎重连中）
- **输出区**: 仅渲染前台连接，后台连接数据只写日志
- **输入行**: 支持 Emacs 风格行编辑、历史记录（上下箭头）

## 6. Lua 脚本 API 设计

每个连接拥有独立的 Lua 状态，脚本可访问以下 API：

```lua
-- 发送命令到当前连接
send("look")

-- 注册触发器：匹配正则，执行回调
trigger("^你获得了 (.+) 经验值$", function(matches)
    print("获得经验: " .. matches[1])
end)

-- 注册别名：输入时匹配替换
alias("^lh$", function()
    send("look")
    send("hp")
end)

-- 定时器
timer(5, function()
    send("hp")
end)

-- 记录日志
log("自定义日志内容")

-- 获取/设置变量
set("auto_fight", true)
if get("auto_fight") then send("kill npc") end

-- 连接控制
reconnect()   -- 重连当前连接
disconnect()  -- 断开当前连接
```

## 7. 配置文件格式

`configs/default.toml`:

```toml
[general]
scroll_buffer = 5000
log_dir = "logs"
log_rotation_size_mb = 10
log_rotation_count = 5

[[connections]]
name = "战士"
host = "mud.example.com"
port = 4000
script = "scripts/warrior.lua"
auto_connect = true
auto_reconnect = true
reconnect_delay_secs = 5

[[connections]]
name = "法师"
host = "mud.example.com"
port = 4000
script = "scripts/mage.lua"
auto_connect = true
auto_reconnect = true
reconnect_delay_secs = 5
```

## 8. 关键技术选型

| 领域 | 选型 | 理由 |
|------|------|------|
| 异步运行时 | tokio | 生态最成熟，多连接调度高效 |
| Lua 绑定 | mlua (luajit feature) | Rust 生态最完善的 Lua 绑定，LuaJIT 性能极佳 |
| 终端控制 | crossterm | 轻量、跨平台、无需后端 |
| 配置解析 | toml + serde | Rust 社区标准，人类可读 |
| 日志 | chrono + 自实现 | 按连接分文件，支持轮转 |
| ANSI 处理 | 自实现解析器 | 精确控制，避免依赖臃肿 |

## 9. 内存预算

| 组件 | 单连接估算 | 10 连接合计 |
|------|-----------|------------|
| TCP 缓冲区 | 8KB | 80KB |
| Lua 状态 | 2-5MB | 20-50MB |
| 滚动缓冲区 (5000行) | ~500KB | ~5MB |
| 日志缓冲 | 64KB | 640KB |
| Rust 开销 | - | ~5MB |
| **合计** | - | **~30-60MB** |

在 2GB 内存机器上留有充足余量。

## 10. 开发阶段规划

### Phase 1 — 基础框架
- [ ] 项目骨架与模块定义
- [ ] 配置文件加载
- [ ] 单连接 TCP 收发
- [ ] 基础终端渲染（无 ANSI）

### Phase 2 — 多连接与 UI
- [ ] 多连接管理器
- [ ] 前台/后台切换
- [ ] 状态栏与 UI 布局
- [ ] ANSI 颜色解析与渲染
- [ ] 输入行编辑与命令历史

### Phase 3 — Lua 脚本引擎
- [ ] LuaJIT 集成
- [ ] 触发器系统
- [ ] 别名系统
- [ ] 定时器系统
- [ ] Lua API 完善

### Phase 4 — 日志与稳定性
- [ ] 按连接分文件日志
- [ ] 日志轮转
- [ ] 断线自动重连
- [ ] 优雅退出与状态保存
- [ ] 7×24 稳定性测试

## 11. 项目目录结构

```
RustLuaMud/
├── Cargo.toml              # Rust 项目配置与依赖
├── LICENSE
├── docs/
│   └── design.md           # 本设计文档
├── configs/
│   └── default.toml        # 默认配置文件
├── scripts/
│   └── example.lua         # Lua 脚本示例
├── logs/                   # 日志输出目录（gitignore）
├── src/
│   ├── main.rs             # 入口：初始化 & 主事件循环
│   ├── app.rs              # 应用状态与主循环逻辑
│   ├── config.rs           # 配置文件解析
│   ├── connection/
│   │   ├── mod.rs
│   │   ├── manager.rs      # 连接管理器：创建/销毁/切换前台
│   │   └── session.rs      # 单个连接会话：TCP + 缓冲区
│   ├── lua/
│   │   ├── mod.rs
│   │   ├── engine.rs       # LuaJIT 引擎封装
│   │   └── api.rs          # 暴露给 Lua 的 Rust API
│   ├── ui/
│   │   ├── mod.rs
│   │   ├── terminal.rs     # 终端布局与渲染
│   │   ├── input.rs        # 键盘输入处理与行编辑
│   │   └── ansi.rs         # ANSI 转义序列解析
│   └── log/
│       ├── mod.rs
│       └── logger.rs       # 日志管理：分文件 & 轮转
└── .gitignore
```
