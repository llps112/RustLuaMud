# MushClient 脚本移植指南 / MushClient Script Migration Guide

## 概述 / Overview

将 MushClient 的 Lua 脚本（`scripts/class/`、`scripts/lua/`）移植到 RustLuaMud 客户端。

**移植策略：直接在 Rust 客户端中实现 MushClient 同名 API，不修改原始脚本，无需兼容层。**

---

## 一、MushClient API 使用统计（排除注释，精确统计）

### 1.1 触发器/别名/定时器管理

| API | 次数 | 说明 |
|---|---|---|
| `AddTimer` | 59 | 添加定时器 |
| `SetTimerOption` | 52 | 设置定时器选项 |
| `SetAliasOption` | 17 | 设置别名选项 |
| `SetTriggerOption` | 15 | 设置触发器选项（含 group/multi_line/omit_from_output） |
| `AddAlias` | 14 | 添加别名 |
| `AddTriggerEx` | 8 | 扩展版添加触发器 |
| `AddTrigger` | 3 | 添加触发器 |
| `DeleteTimer` | 6 | 删除定时器 |
| `GetTriggerList` | 4 | 获取触发器列表 |
| `GetTimerList` | 4 | 获取定时器列表 |
| `GetAliasList` | 2 | 获取别名列表 |
| `DeleteTrigger` | 1 | 删除触发器 |
| `DeleteAlias` | 1 | 删除别名 |
| `GetTriggerInfo` | 2 | 获取触发器信息（info 8=enabled, 26=group） |
| `GetTimerInfo` | 2 | 获取定时器信息（info 6=enabled, 19=group） |
| `EnableTriggerGroup` | 2 | 启用/禁用触发器组 |
| `EnableTimerGroup` | 2 | 启用/禁用定时器组 |

### 1.2 命令执行

| API | 次数 | 说明 |
|---|---|---|
| `Execute` | 6 | 发送命令到 MUD |

> `run` 在 `michen_system.lua` 中定义为 `run=function(str)...`，内部调用 `Execute`，不是 MushClient API。

### 1.3 输出

| API | 次数 | 说明 |
|---|---|---|
| `ColourNote` | 6 | 彩色输出 |

### 1.4 配置/选项

| API | 次数 | 说明 |
|---|---|---|
| `GetInfo` | 13 | 获取系统信息（code=35 脚本路径） |
| `SetOption` | 2 | 设置客户端选项 |
| `SetAlphaOption` | 1 | 设置高级选项 |

### 1.5 连接状态

| API | 次数 | 说明 |
|---|---|---|
| `Connect` | 4 | 连接服务器 |
| `IsConnected` | 4 | 检查连接状态 |
| `Disconnect` | 1 | 断开连接 |

### 1.6 工具函数

| API | 次数 | 说明 |
|---|---|---|
| `GetUniqueNumber` | 3 | 获取唯一编号 |
| `Trim` | 4 | 去除首尾空格 |

> `findstring`（1599次）和 `strexplit`（4次）在 `michen_system.lua` 中定义为 Lua 函数，不是 MushClient API，无需在 Rust 中实现。

### 1.7 wait 协程库

| API | 次数 | 说明 |
|---|---|---|
| `wait.time` | 160 | 等待指定秒数 |
| `wait.make` | 129 | 启动协程 |
| `wait.regexp` | 1 | 等待正则匹配 |

### 1.8 日志

| API | 次数 | 说明 |
|---|---|---|
| `OpenLog` | 1 | 打开日志 |
| `IsLogOpen` | 1 | 检查日志状态 |

### 1.9 数据库

| API | 次数 | 说明 |
|---|---|---|
| `sqlite3.open` | 1 | 已实现 |
| `DatabaseClose` | 1 | 脚本层兼容，映射到 `db:close()` |

### 1.10 不需要实现的 API

| API | 原因 |
|---|---|
| `DoAfterSpecial` | 全部在注释中，实际未使用 |
| `DoAfter` | 仅1次，用 wait.time 替代 |
| `Send` | 0次（排除注释后） |
| `Note` | 0次（排除注释后） |
| `EnableTrigger` | 0次（排除注释后） |
| `EnableTimer` | 0次（排除注释后） |
| `GetAlphaOption` | 0次（排除注释后） |
| `utils.*` | GUI 窗格设置，将来改为命令控制 |
| `require "serialize"/"gauge"/"InfoBox"/"movewindow"` | GUI 相关，已删除 |

---

## 二、移植步骤 / Migration Steps

### Phase 1: 基础设施 [x]

#### 1.1 脚本编码处理 [x]

- [x] 已实现：自动检测 UTF-8/GBK 编码并转换

#### 1.2 模块加载机制 [x]

- [x] `GetInfo(35)` — 返回脚本所在目录路径（Windows 风格带 `\`，兼容 MushClient 脚本正则匹配）
- [x] `dofile()` — 覆盖为支持 GBK 自动转码的版本，自动将 `\` 替换为 `/`
- [x] 路径分隔符兼容：dofile 中 `\` 自动替换为 `/`

---

### Phase 2: MushClient API 实现 [x]

在 `src/lua/engine.rs` 的 `register_api()` 中直接注册 MushClient 同名 API。

#### 2.1 触发器 API [x]

- [x] `AddTrigger` — 注册命名触发器
- [x] `AddTriggerEx` — 扩展版
- [x] `DeleteTrigger` — 按名称删除
- [x] `GetTriggerList` — 返回触发器名称列表
- [x] `GetTriggerInfo` — 获取触发器属性（8=enabled, 26=group）
- [x] `SetTriggerOption` — 设置选项（group/multi_line/lines_to_match/omit_from_output/enabled/send）
- [x] `EnableTriggerGroup` — 按组启用/禁用

#### 2.2 别名 API [x]

- [x] `AddAlias` — 注册命名别名（自动转换 * 通配符为正则）
- [x] `DeleteAlias` — 按名称删除
- [x] `GetAliasList` — 返回别名名称列表
- [x] `SetAliasOption` — 设置选项（group/enabled）

#### 2.3 定时器 API [x]

- [x] `AddTimer` — 注册命名定时器
- [x] `DeleteTimer` — 按名称删除
- [x] `GetTimerList` — 返回定时器名称列表
- [x] `GetTimerInfo` — 获取定时器属性（6=enabled, 19=group）
- [x] `SetTimerOption` — 设置选项（group/enabled/send_to）
- [x] `EnableTimerGroup` — 按组启用/禁用

#### 2.4 命令执行 API [x]

- [x] `Execute(command)` — 发送命令到 MUD
- [x] `send(command)` — 保留原有 API

#### 2.5 输出 API [x]

- [x] `ColourNote(fg, bg, text)` — 彩色输出（ANSI 颜色码）
- [x] `Note(text)` — 普通输出
- [x] `Tell(text)` — 内联输出

#### 2.6 配置 API [x]

- [x] `SetOption(name, value)` — 存储到内部 options table
- [x] `GetOption(name)` — 获取选项值
- [x] `SetAlphaOption(name, value)` — 存储到内部 alpha_options table
- [x] `GetAlphaOption(name)` — 获取高级选项值
- [x] `GetInfo(code)` — 实现常用 code（1=版本, 35=脚本路径）

#### 2.7 连接状态 API [x]

- [x] `IsConnected()` — 查询连接状态
- [x] `Connect()` — 触发连接
- [x] `Disconnect()` — 断开连接

#### 2.8 工具函数 [x]

- [x] `GetUniqueNumber()` — 递增计数器
- [x] `Trim(string)` — 去除首尾空格

#### 2.9 常量表 [x]

- [x] `trigger_flag.*` — 触发器标志（Enabled/OmitFromLog/OmitFromOutput/KeepEvaluating/IgnoreCase/RegularExpression/ExpandVariables/Replace/LowercaseWildcard/Temporary/OneShot）
- [x] `alias_flag.*` — 别名标志（Enabled/IgnoreCase/RegularExpression/ExpandVariables/Replace/Temporary）
- [x] `timer_flag.*` — 定时器标志（Enabled/Temporary/OneShot/ActiveWhenClosed/Replace）
- [x] `custom_colour.*` — 自定义颜色
- [x] `error_code.*` — 错误代码
- [x] `error_desc.*` — 错误描述

#### 2.10 日志 API [x]

- [x] `OpenLog(filename, append)` — 简单实现（客户端已有日志系统）
- [x] `IsLogOpen()` — 返回 true

#### 2.11 变量 API [x]

- [x] `GetVariable(name)` — 获取变量
- [x] `SetVariable(name, value)` — 设置变量
- [x] `DeleteVariable(name)` — 删除变量
- [x] `GetVariableList()` — 获取所有变量

#### 2.12 数据库 API [x]

- [x] `DatabaseClose(dbname)` — 脚本层兼容

---

### Phase 3: wait.lua 协程库 [x]

**依赖项：**

- [x] `check` 模块 — `require "check"` 返回 check 函数，依赖 error_code/error_desc 常量（已注册）
- [x] `bit` 库 — `bit.bor/band/bxor/bnot/lshift/rshift` 位运算（已实现）
- [x] `MakeRegularExpression` — 将通配符模式转为正则（已实现）
- [x] `GetPluginID` / `GetPluginInfo` — 桩实现（已实现）
- [x] `wait.make(func)` — 纯 Lua 实现，依赖 coroutine（wait.lua 原始脚本可用）
- [x] `wait.regexp(pattern, timeout)` — 纯 Lua 实现，依赖 AddTriggerEx（wait.lua 原始脚本可用）
- [x] `wait.time(seconds)` — 纯 Lua 实现，依赖 AddTimer（wait.lua 原始脚本可用）
- [x] `wait.match(match, timeout, flags)` — 纯 Lua 实现，依赖 MakeRegularExpression（wait.lua 原始脚本可用）
- [x] `require` 路径设置 — package.path 包含 scripts/lua/ 目录

**实现说明：**

1. `check.lua` 已有，依赖的 error_code/error_desc 常量已在 Rust 中注册
2. `bit` 库在 Rust 中实现 `bit.bor`、`bit.band`、`bit.bxor`、`bit.bnot`、`bit.lshift`、`bit.rshift`
3. `MakeRegularExpression` 在 Rust 中实现（将 * 和 ? 通配符转为正则）
4. `GetPluginID` 返回空字符串，`GetPluginInfo` 返回桩数据
5. wait.lua 本身是纯 Lua 脚本，依赖 AddTriggerEx/AddTimer/DeleteTrigger 等已实现的 API
6. 定时器触发时通过 `wait.timer_resume` 恢复协程，触发器命中时通过 `wait.trigger_resume` 恢复协程

---

### Phase 4: 多行触发器支持 [x]

- [x] Trigger 结构添加 `multiline` 和 `lines_to_match` 字段
- [x] `process_output` 中维护最近 N 行输出的缓冲区（max_recent_lines=20）
- [x] 对多行触发器使用合并文本进行正则匹配

---

### Phase 5: SQLite3 兼容性确认 [x]

- [x] `sqlite3.open` 已实现
- [x] `db:exec`、`db:prepare`、`stmt:step`、`stmt:run` 已实现
- [x] `DatabaseClose` — 已在 Rust 中注册为全局函数
- [x] `db:close` — 已在 LuaDb UserData 中实现

---

### Phase 6: 入口脚本适配与集成测试 [x]

- [x] `GetInfo(35)` 返回 Windows 风格路径（兼容 `string.match(GetInfo(35), "^.*\\")`）
- [x] `include("config_"..me.charid..".lua")` — dofile 自动处理路径分隔符和 GBK 编码
- [x] `char_name`、`char_password` 变量已通过 profile 注入（app.rs init_lua_for_session）
- [x] Lua 5.x 兼容性补丁：`table.getn`、`table.foreach`、`table.foreachi`、`math.mod`、`math.pow`
- [x] 编译通过（cargo check / cargo build --release）

---

## 三、风险与注意事项

1. **GBK 编码** — 脚本文件为 GBK 编码，已实现自动转码
2. **协程与异步** — `wait.lua` 的协程模型需要与 Rust 的异步运行时协调
3. **触发器性能** — 150+ 个触发器同时运行，需确保匹配效率
4. **内存限制** — J1800 + 2G 内存环境，需注意缓冲区大小
5. **Windows 路径** — 原始脚本使用 `\` 分隔符，需兼容处理
6. **MushClient 专有功能** — MXP、Pueblo、声音播放等在终端客户端中无法实现，需跳过
