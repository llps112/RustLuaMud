# MUSHclient AddAlias 官方文档

> 来源：http://www.mushclient.com/scripts/function.php?name=AddAlias
> 保存日期：2026-06-12

Adds an alias to the list of aliases.

**Prototype**: `long AddAlias(BSTR AliasName, BSTR MatchText, BSTR ResponseText, long Flags, BSTR ScriptName);`

## 参数

- **AliasName**: 别名名称，可以为空（见名称规则）
- **MatchText**: 匹配文本
- **ResponseText**: 发送到 MUD 的响应文本
- **Flags**: 标志位组合，见下方常量
- **ScriptName**: 执行的脚本子程序名称（Lua 中可选）

## Flags

官方 Lua `alias_flag` 表定义：

| Lua 常量 | 值 | 说明 |
|----------|-----|------|
| `alias_flag.Enabled` | 1 | 启用别名 |
| `alias_flag.KeepEvaluating` | 8 | 保持继续评估 |
| `alias_flag.IgnoreAliasCase` | 32 | 忽略大小写 |
| `alias_flag.OmitFromLogFile` | 64 | 不在日志文件中记录 |
| `alias_flag.RegularExpression` | **128** | 使用正则表达式匹配 |
| `alias_flag.ExpandVariables` | 512 | 展开 @direction 等变量 |
| `alias_flag.Replace` | 1024 | 替换同名已有别名 |
| `alias_flag.AliasSpeedWalk` | 2048 | 将发送串解释为 speedwalk 串 |
| `alias_flag.AliasQueue` | 4096 | 按 speedwalk 延迟间隔排队发送 |
| `alias_flag.AliasMenu` | 8192 | 此别名出现在别名菜单上 |
| `alias_flag.Temporary` | 16384 | 临时别名，不保存到世界文件 |

## Lua 示例

```lua
AddAlias("food_alias", "eat", "eat food", alias_flag.Enabled, "")
```

## Lua 备注

- ScriptName 参数是可选的
- 如 `ResponseText` 非空且 `ScriptName` 为空，`Send To` 默认为 12（执行 Lua 代码）
- 如 `ScriptName` 非空，`Send To` 默认为 0（发送到世界）

## 返回值

| 返回值 | 说明 |
|--------|------|
| 0 | eOK — 添加成功 |
| 1 | eInvalidObjectLabel — 别名名称无效 |
| 2 | eAliasAlreadyExists — 同名别名已存在 |
| 3 | eAliasCannotBeEmpty — 匹配文本不能为空 |
| 4 | eScriptNameNotLocated — 找不到脚本函数 |
| 5 | eBadRegularExpression — 正则表达式无效 |

## 别名名称规则

- 以字母 (A-Z) 开头
- 后跟字母 (A-Z)、数字 (0-9) 或下划线 (_)
