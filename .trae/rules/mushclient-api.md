# MushClient API 兼容规则

## 重要规则

所有 MushClient 兼容的 API（`GetInfo`、`GetTriggerInfo`、`GetTimerInfo`、`AddTrigger`、`AddTimer`、`GetPluginInfo` 等）的 **code 映射和参数签名必须 100% 匹配官方文档**。

## 离线参考

MushClient API 完整离线参考已保存到 `help/api/mushclient-*.md`：
- `help/api/mushclient-api-index.md` — 全部函数索引
- `help/api/mushclient-GetInfo.md` — GetInfo code 映射（300+ codes）
- `help/api/mushclient-GetTriggerInfo.md` — GetTriggerInfo code 映射（含通配符）
- `help/api/mushclient-GetTimerInfo.md` — GetTimerInfo code 映射
- `help/api/mushclient-GetPluginInfo.md` — GetPluginInfo code 映射
- `help/api/mushclient-AddTrigger.md` / `AddTriggerEx.md` — AddTrigger/AddTriggerEx 参数与标志位
- `help/api/mushclient-AddTimer.md` — AddTimer 参数与标志位
- `help/api/mushclient-SetTriggerOption.md` / `SetTimerOption.md` — 选项名与值
- `help/api/mushclient-GetAliasInfo.md` — GetAliasInfo code 映射
- `help/api/mushclient-GroupFunctions.md` — EnableGroup/EnableTriggerGroup/EnableTimerGroup
- `help/api/mushclient-Variables.md` — GetVariable/SetVariable/DeleteVariable
- `help/api/mushclient-OutputFunctions.md` — Note/ColourNote/ColourTell/Send/Execute/DoAfter
- `help/api/mushclient-ListFunctions.md` — GetTriggerList/GetTimerList/GetAliasList
- `help/api/mushclient-DeleteFunctions.md` — DeleteTrigger/DeleteTimer/DeleteAlias

修改涉及 MushClient API 兼容的代码之前，**先查阅这些本地文件确认官方定义**。

仅在本地文件没有涵盖所需函数时，才访问在线页面：`http://www.mushclient.com/scripts/function.php`

## 注意事项

- `GetInfo(code)` — code 编号的含义必须严格按官方定义，不得自定义映射。
- `GetTriggerInfo(name, code)` — code 编号含义严格按官方定义（如 7=Keep evaluating, 8=Enabled, 26=Group name）。
- `GetTimerInfo(name, code)` — code 编号含义严格按官方定义（如 6=Enabled, 7=One shot, 8=At time, 14=Temporary, 19=Group name）。
- `GetPluginInfo(id, code)` — code 编号含义严格按官方定义（如 1=Name, 14=Date modified, 19=Version, 20=Directory）。
- 当未实现某个特性的返回值时，返回空串 `""`（string）或 `0`（number）或 `false`（boolean），而不是自定义其他含义。

---

# MushClient API 标志位完整性规则

## 重要规则

实现 MushClient 兼容 API（`AddTrigger`、`AddTriggerEx`、`AddAlias`、`AddTimer` 等）时，**必须对照官方文档逐位检查 flags 参数的所有标志位**，不能遗漏。

## flags 标志位速查

| 标志位 | 值 | 说明 |
|--------|------|------|
| `Enabled` | 1 | 创建后立即启用 |
| `KeepEvaluating` | 8 | 匹配后继续评估后续 trigger |
| `CaseSensitive` | 16 | 区分大小写（默认开启，设 16 关闭此模式则在 Rust 端自动加 `(?i)`） |
| `RegularExpression` | 32 | 正则模式 |
| `Replace` | 1024 | 同名替换，不追加 |
| `Temporary` | 4096 | 临时 trigger，session 断开自动清除 |
| `OneShot` | 4096 | 一次性 timer（与 Temporary 同值，复用于 AddTimer 的 flags 参数） |
| `AtTime` | 65536 | 定时器到点触发 |

## 注意事项

- 每个新实现的 API 函数中，**flag 解析应完整性检查**：对已知标志位逐一处理，对未知标志位用 `// TODO` 注释记录。
- **Replace 标志（1024）是 `loadmod` 重载的正确性基石**：缺失时同名 trigger/alias 会累积，导致回调执行多次，且旧回调引用的 Lua 函数名（`addtri_XXXXX`）残留全局空间。
- 修改新增 API 前，先查阅 `help/api/mushclient-*.md` 确认参数签名和标志位定义。
