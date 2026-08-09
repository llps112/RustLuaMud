# MushClient API 兼容规则

## 权威源（单一事实来源）

所有 MushClient 兼容 API 的常量表、标志位、code 映射、参数签名**必须 100% 匹配 GitHub 源码**：

- **仓库**：`https://github.com/nickgammon/mushclient`
- **标志位定义**：`flags.h`（C++ enum 原始值）
- **Lua 表注册**：`scripting/lua_methods.cpp` 中的 `MakeFlagsTable` 调用（行 7834-7844）和各 `flags_pair` 数组（行 7211-7473）

> ⚠️ **官方网站文档（gammon.com.au / mushclient.com）可能滞后于源码**。例如 2004 年论坛帖子的 `table.foreach(alias_flag, print)` 输出不含 `OneShot`，但当前 `lua_methods.cpp` 的 `alias_flags[]` 数组已包含 `{ "OneShot", 32768 }`。**以 GitHub master 分支源码为准**。

## 禁止自行扩展或遗漏

**绝对禁止**：
- 在 `trigger_flag`/`alias_flag`/`timer_flag`/`custom_colour`/`sendto`/`error_code`/`error_desc` 等常量表中**添加 MushClient 源码不存在的条目**（自行扩展）
- **遗漏** MushClient 源码中已有的条目（即使项目暂时用不到）
- 修改任何常量的**数值**（如把 `Temporary` 从 16384 改成 4096）

**修改前必做**：打开 `lua_methods.cpp` 对应的 `flags_pair` 数组，逐条核对名称和数值。

## 离线参考

MushClient API 离线参考已保存到 `help/api/mushclient-*.md`（函数签名、code 映射等）。修改涉及 API 兼容的代码前先查阅这些文件。仅在本地文件未涵盖时才访问在线页面。

## 注意事项

- `GetInfo(code)` — code 编号含义严格按官方定义，不得自定义映射。
- `GetTriggerInfo(name, code)` — code 编号含义严格按官方定义（如 7=Keep evaluating, 8=Enabled, 26=Group name）。
- `GetTimerInfo(name, code)` — code 编号含义严格按官方定义（如 6=Enabled, 7=One shot, 8=At time, 14=Temporary, 19=Group name）。
- `GetPluginInfo(id, code)` — code 编号含义严格按官方定义（如 1=Name, 14=Date modified, 19=Version, 20=Directory）。
- 当未实现某个特性的返回值时，返回空串 `""`（string）或 `0`（number）或 `false`（boolean），而不是自定义其他含义。

---

# 常量表完整定义（对照 lua_methods.cpp）

以下 6 个表通过 `MakeFlagsTable` 注册为 Lua 全局变量，每条目为 `name(string) → number`。

## trigger_flag（lua_methods.cpp 行 7211-7227）

| 常量 | 值 | 说明 |
|------|------|------|
| `Enabled` | 1 | 启用触发器 |
| `OmitFromLog` | 2 | 不写入日志文件 |
| `OmitFromOutput` | 4 | 不显示在输出窗口 |
| `KeepEvaluating` | 8 | 匹配后继续评估后续 trigger |
| `IgnoreCase` | 16 | 忽略大小写 |
| `RegularExpression` | 32 | 正则模式 |
| `ExpandVariables` | 512 | 展开 `@variable` 变量 |
| `Replace` | 1024 | 同名替换，不追加 |
| `LowercaseWildcard` | 2048 | 通配符强制小写 |
| `Temporary` | 16384 | 临时 trigger，不保存到世界文件 |
| `OneShot` | 32768 | 一次性，触发后自动删除 |

## alias_flag（lua_methods.cpp 行 7254-7271）

| 常量 | 值 | 说明 |
|------|------|------|
| `Enabled` | 1 | 启用别名 |
| `KeepEvaluating` | 8 | 匹配后继续评估后续 alias |
| `IgnoreAliasCase` | 32 | 忽略大小写 |
| `OmitFromLogFile` | 64 | 不写入日志文件 |
| `RegularExpression` | 128 | 正则模式 |
| `ExpandVariables` | 512 | 展开 `@variable` 变量 |
| `Replace` | 1024 | 同名替换 |
| `AliasSpeedWalk` | 2048 | 发送串解释为 speedwalk |
| `AliasQueue` | 4096 | 按 speedwalk 间隔排队发送 |
| `AliasMenu` | 8192 | 出现在别名菜单 |
| `Temporary` | 16384 | 临时别名，不保存到世界文件 |
| `OneShot` | 32768 | 一次性，匹配后自动删除 |

## timer_flag（lua_methods.cpp 行 7273-7286）

| 常量 | 值 | 说明 |
|------|------|------|
| `Enabled` | 1 | 启用定时器 |
| `AtTime` | 2 | 到指定时刻触发（否则为间隔触发） |
| `OneShot` | 4 | 一次性，触发后删除 |
| `TimerSpeedWalk` | 8 | 触发时执行 speedwalk |
| `TimerNote` | 16 | 触发时执行 note |
| `ActiveWhenClosed` | 32 | 断开连接时仍然触发 |
| `Replace` | 1024 | 同名替换 |
| `Temporary` | 16384 | 临时定时器，不保存到世界文件 |

> **注意**：`OneShot` 在三个表中值不同——trigger/alias 为 32768，timer 为 4。`AtTime` 仅存在于 timer_flag。

## custom_colour（lua_methods.cpp 行 7229-7252）

| 常量 | 值 |
|------|------|
| `NoChange` | -1 |
| `Custom1` | 0 |
| `Custom2` | 1 |
| `Custom3` | 2 |
| `Custom4` | 3 |
| `Custom5` | 4 |
| `Custom6` | 5 |
| `Custom7` | 6 |
| `Custom8` | 7 |
| `Custom9` | 8 |
| `Custom10` | 9 |
| `Custom11` | 10 |
| `Custom12` | 11 |
| `Custom13` | 12 |
| `Custom14` | 13 |
| `Custom15` | 14 |
| `Custom16` | 15 |
| `CustomOther` | 16 |

## sendto（lua_methods.cpp 行 7453-7473）

| 常量 | 值 | 说明 |
|------|------|------|
| `world` | 0 | 发送到 MUD |
| `command` | 1 | 命令窗口 |
| `output` | 2 | 输出窗口 |
| `status` | 3 | 状态栏 |
| `notepad` | 4 | 记事本（新建） |
| `notepadappend` | 5 | 记事本（追加） |
| `logfile` | 6 | 日志文件 |
| `notepadreplace` | 7 | 记事本（替换） |
| `commandqueue` | 8 | 命令队列 |
| `variable` | 9 | 变量 |
| `execute` | 10 | 执行（重新解析为命令） |
| `speedwalk` | 11 | 快速行走 |
| `script` | 12 | 脚本引擎 |
| `immediate` | 13 | 立即发送（不经队列） |
| `scriptafteromit` | 14 | 脚本（omit 后执行） |

## error_code（lua_methods.cpp 行 7288-7369）

`error_code` 表为 `name(string) → number`，`error_desc` 表为 `number → description(string)`。

完整列表见 `help/api/mushclient-errors.md`。关键约束：

- 错误码范围为 `0`（eOK）和 `30001`~`30074`
- `ePluginDoesNotSaveState` 和 `ePluginCouldNotSaveState` **共用 30037**（源码如此，非笔误）
- 30038 被跳过（源码如此）
- `error_desc` 用**数字 key** 索引：`error_desc[30022]` → `"Time given to AddTimer is invalid"`

---

# flags 标志位完整性规则

## 重要规则

实现 MushClient 兼容 API（`AddTrigger`、`AddTriggerEx`、`AddAlias`、`AddTimer` 等）时，**必须对照上方常量表逐位检查 flags 参数的所有标志位**，不能遗漏。

## 注意事项

- 每个新实现的 API 函数中，**flag 解析应完整性检查**：对已知标志位逐一处理，对未知标志位用 `// TODO` 注释记录。
- **Replace 标志（1024）是 `loadmod` 重载的正确性基石**：缺失时同名 trigger/alias 会累积，导致回调执行多次，且旧回调引用的 Lua 函数名（`addtri_XXXXX`）残留全局空间。
- **OneShot 标志**：trigger/alias 的 OneShot=32768，匹配后自动删除该 trigger/alias；timer 的 OneShot=4，触发后自动删除该 timer。三者行为一致（一次性）但数值不同。
- 修改新增 API 前，先查阅 `help/api/mushclient-*.md` 确认参数签名，再对照 `lua_methods.cpp` 确认常量值。
