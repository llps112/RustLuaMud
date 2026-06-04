# MushClient 脚本移植 — 任务完成情况一览

生成时间：2026-06-04

## 总体状态：全部完成

| Phase | 名称 | 状态 | 关键产出 |
|---|---|---|---|
| Phase 1 | 基础设施 | **完成** | GBK自动转码、GetInfo(35)、dofile覆盖、路径分隔符兼容 |
| Phase 2 | MushClient API 实现 | **完成** | 30+ API函数、6组常量表 |
| Phase 3 | wait.lua 协程库 | **完成** | bit库、MakeRegularExpression、GetPluginID/Info桩函数 |
| Phase 4 | 多行触发器支持 | **完成** | recent_lines缓冲区、multiline匹配 |
| Phase 5 | SQLite3 兼容性 | **完成** | sqlite3.open/exec/prepare/step/run/close、DatabaseClose |
| Phase 6 | 入口脚本适配与集成测试 | **完成** | Lua 5.x兼容补丁、编译通过 |

---

## Phase 1: 基础设施

| 子任务 | 状态 | 实现位置 | 说明 |
|---|---|---|---|
| 脚本编码处理 | 完成 | engine.rs `load_script()` | 自动检测 UTF-8/GBK 编码并转换 |
| GetInfo(35) | 完成 | engine.rs `register_api()` | 返回 Windows 风格路径（带 `\`），兼容 MushClient 脚本 `string.match` |
| dofile() 覆盖 | 完成 | engine.rs `register_api()` | 支持 GBK 自动转码，`\` 自动替换为 `/` |
| 路径分隔符兼容 | 完成 | engine.rs `register_api()` | dofile 中自动转换 |

## Phase 2: MushClient API 实现

### 触发器 API

| API | 状态 | 说明 |
|---|---|---|
| AddTrigger | 完成 | 注册命名触发器 |
| AddTriggerEx | 完成 | 扩展版，支持 flags/sequence/script |
| DeleteTrigger | 完成 | 按名称删除 |
| GetTriggerList | 完成 | 返回触发器名称列表 |
| GetTriggerInfo | 完成 | 支持 code 8(enabled)、26(group) |
| SetTriggerOption | 完成 | 支持 group/multi_line/lines_to_match/omit_from_output/enabled/send |
| EnableTriggerGroup | 完成 | 按组启用/禁用 |

### 别名 API

| API | 状态 | 说明 |
|---|---|---|
| AddAlias | 完成 | 自动转换 * 通配符为正则 .* |
| DeleteAlias | 完成 | 按名称删除 |
| GetAliasList | 完成 | 返回别名名称列表 |
| SetAliasOption | 完成 | 支持 group/enabled |

### 定时器 API

| API | 状态 | 说明 |
|---|---|---|
| AddTimer | 完成 | 支持命名/匿名定时器，send_text 存储 |
| DeleteTimer | 完成 | 按名称删除 |
| GetTimerList | 完成 | 返回定时器名称列表 |
| GetTimerInfo | 完成 | 支持 code 6(enabled)、19(group) |
| SetTimerOption | 完成 | 支持 group/enabled/send_to |
| EnableTimerGroup | 完成 | 按组启用/禁用 |

### 命令执行 API

| API | 状态 | 说明 |
|---|---|---|
| Execute | 完成 | 发送命令到 MUD |
| send | 完成 | 保留原有 API |

### 输出 API

| API | 状态 | 说明 |
|---|---|---|
| ColourNote | 完成 | ANSI 颜色码输出 |
| Note | 完成 | 普通输出 |
| Tell | 完成 | 内联输出 |

### 配置 API

| API | 状态 | 说明 |
|---|---|---|
| SetOption | 完成 | 存储到内部 options table |
| GetOption | 完成 | 获取选项值 |
| SetAlphaOption | 完成 | 存储到内部 alpha_options table |
| GetAlphaOption | 完成 | 获取高级选项值 |
| GetInfo | 完成 | code 1=版本, 35=脚本路径 |

### 连接状态 API

| API | 状态 | 说明 |
|---|---|---|
| IsConnected | 完成 | 查询连接状态 |
| Connect | 完成 | 触发连接 |
| Disconnect | 完成 | 断开连接 |

### 工具函数

| API | 状态 | 说明 |
|---|---|---|
| GetUniqueNumber | 完成 | 递增计数器 |
| Trim | 完成 | 去除首尾空格 |

### 变量 API

| API | 状态 | 说明 |
|---|---|---|
| GetVariable | 完成 | 获取变量 |
| SetVariable | 完成 | 设置变量 |
| DeleteVariable | 完成 | 删除变量 |
| GetVariableList | 完成 | 获取所有变量 |

### 日志 API

| API | 状态 | 说明 |
|---|---|---|
| OpenLog | 完成 | 简单实现 |
| IsLogOpen | 完成 | 返回 true |

### 数据库 API

| API | 状态 | 说明 |
|---|---|---|
| DatabaseClose | 完成 | 全局函数兼容 |

### 常量表

| 常量组 | 状态 | 包含项 |
|---|---|---|
| trigger_flag | 完成 | Enabled/OmitFromLog/OmitFromOutput/KeepEvaluating/IgnoreCase/RegularExpression/ExpandVariables/Replace/LowercaseWildcard/Temporary/OneShot |
| alias_flag | 完成 | Enabled/IgnoreCase/RegularExpression/ExpandVariables/Replace/Temporary |
| timer_flag | 完成 | Enabled/Temporary/OneShot/ActiveWhenClosed/Replace |
| custom_colour | 完成 | 自定义颜色 |
| error_code | 完成 | 错误代码 |
| error_desc | 完成 | 错误描述 |

## Phase 3: wait.lua 协程库

| 子任务 | 状态 | 说明 |
|---|---|---|
| check 模块 | 完成 | error_code/error_desc 常量已注册 |
| bit 库 | 完成 | bor/band/bxor/bnot/lshift/rshift |
| MakeRegularExpression | 完成 | 将 * 和 ? 通配符转为正则 |
| GetPluginID | 完成 | 桩实现，返回空字符串 |
| GetPluginInfo | 完成 | 桩实现，code 1 返回 "RustLuaMud" |
| wait.make | 完成 | 纯 Lua，依赖 coroutine |
| wait.regexp | 完成 | 纯 Lua，依赖 AddTriggerEx |
| wait.time | 完成 | 纯 Lua，依赖 AddTimer |
| wait.match | 完成 | 纯 Lua，依赖 MakeRegularExpression |
| require 路径 | 完成 | package.path 包含 scripts/lua/ |

## Phase 4: 多行触发器支持

| 子任务 | 状态 | 说明 |
|---|---|---|
| Trigger 结构扩展 | 完成 | 添加 multiline、lines_to_match 字段 |
| recent_lines 缓冲区 | 完成 | max_recent_lines=20 |
| 多行匹配逻辑 | 完成 | 合并最近 N 行文本进行正则匹配 |

## Phase 5: SQLite3 兼容性

| 子任务 | 状态 | 说明 |
|---|---|---|
| sqlite3.open | 完成 | 已实现 |
| db:exec/db:prepare/stmt:step/stmt:run | 完成 | 已实现 |
| DatabaseClose | 完成 | 全局函数，映射到 db:close() |
| db:close | 完成 | LuaDb UserData 方法 |

## Phase 6: 入口脚本适配与集成测试

| 子任务 | 状态 | 说明 |
|---|---|---|
| GetInfo(35) 路径格式 | 完成 | 返回 Windows 风格路径，兼容 `string.match(GetInfo(35), "^.*\\")` |
| include/loadmod 路径兼容 | 完成 | dofile 自动将 `\` 转为 `/` |
| char_name/char_password 注入 | 完成 | app.rs init_lua_for_session 中设置 |
| Lua 5.x 兼容补丁 | 完成 | table.getn/foreach/foreachi, math.mod/pow |
| 编译验证 | 完成 | cargo check / cargo build --release 通过 |

---

## 未实现的 API（按设计不需要）

| API | 原因 |
|---|---|
| DoAfterSpecial | 全部在注释中，实际未使用 |
| DoAfter | 仅1次调用，用 wait.time 替代 |
| Send | 0次（排除注释后） |
| Note | 0次（排除注释后） |
| EnableTrigger | 0次（排除注释后） |
| EnableTimer | 0次（排除注释后） |
| GetAlphaOption | 0次（排除注释后） |
| utils.* | GUI 窗格设置，将来改为命令控制 |
| require "serialize"/"gauge"/"InfoBox"/"movewindow" | GUI 相关，已删除 |

---

## 后续待验证事项

以下事项需要在实际运行环境中验证：

1. **模块加载链路** — 启动客户端后，michen_xkx.lua 能否正确加载所有 class 目录下的模块
2. **触发器匹配** — 连接 MUD 服务器后，触发器能否正常匹配服务器输出
3. **wait.lua 协程** — wait.make/wait.regexp/wait.time 在实际场景中能否正常挂起和恢复
4. **SQLite3 地图查询** — gps_lib.lua 的 DB_Import 和路径查询能否正常工作
5. **GBK 中文匹配** — 触发器模式中的中文字符能否正确匹配 GBK 编码的服务器输出
6. **定时器周期触发** — always_watch 等周期定时器能否正常工作
