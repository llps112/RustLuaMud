# 标志常量

API 调用中使用的标志位常量定义。

---

## trigger_flag

触发器标志位，用于 `AddTrigger` / `AddTriggerEx`。

| 常量 | 值 | 说明 |
|------|-----|------|
| `trigger_flag.Enabled` | 1 | 启用触发器 |
| `trigger_flag.Replace` | 1024 | 同名替换（删除已有同名触发器） |
| `trigger_flag.RegularExpression` | 2048 | 启用正则表达式匹配 |
| `trigger_flag.OneShot` | 4096 | 一次性触发器（触发后自动删除） |
| `trigger_flag.OmitFromOutput` | 65536 | 匹配行不在输出窗口显示 |

**组合使用**:
```lua
AddTrigger("t1", "^(%w+):(%d+)", "", 
    trigger_flag.Enabled + trigger_flag.Replace + trigger_flag.RegularExpression,
    0, 0, "", "callback", 0, 10)
```

---

## alias_flag

别名标志位，用于 `AddAlias`。

| 常量 | 官方值 | 说明 |
|------|--------|------|
| `alias_flag.Enabled` | 1 | 启用别名 |
| `alias_flag.KeepEvaluating` | 8 | 保持继续评估 |
| `alias_flag.IgnoreAliasCase` | 32 | 忽略大小写 |
| `alias_flag.OmitFromLogFile` | 64 | 不在日志文件中记录 |
| `alias_flag.RegularExpression` | **128** | 启用正则表达式匹配 |
| `alias_flag.ExpandVariables` | 512 | 展开 @direction 等变量 |
| `alias_flag.Replace` | 1024 | 同名替换 |
| `alias_flag.AliasSpeedWalk` | 2048 | 将发送串解释为 speedwalk 串 |
| `alias_flag.AliasQueue` | 4096 | 按 speedwalk 延迟间隔排队发送 |
| `alias_flag.AliasMenu` | 8192 | 此别名出现在别名菜单上 |
| `alias_flag.Temporary` | 16384 | 临时别名，不保存到世界文件 |

> **注意**：本项目 Lua 引擎实际 `alias_flag.RegularExpression = 32`（借用 trigger_flag 的值），以兼容现有脚本中硬编码的 `33`（`Enabled + RegularExpression`）。如需使用官方值 128，需同步修改脚本中所有硬编码 `33` 为 `129`。

**组合使用**:
```lua
AddAlias("go_n", "^n$", "north", 
    alias_flag.Enabled + alias_flag.Replace + alias_flag.RegularExpression)

AddAlias("goto", "^goto (.+)$", "go %1",
    alias_flag.Enabled + alias_flag.Replace + alias_flag.RegularExpression)
```

---

## timer_flag

定时器标志位，用于 `AddTimer`。

| 常量 | 值 | 说明 |
|------|-----|------|
| `timer_flag.Enabled` | 1 | 启用定时器 |
| `timer_flag.Replace` | 1024 | 同名替换（旧定时器禁用时继承禁用状态） |
| `timer_flag.OneShot` | 4 | 一次性定时器（触发后自动删除） |
| `timer_flag.AtTime` | 8 | 指定时刻触发 |

**组合使用**:
```lua
-- 周期性定时器
AddTimer("t60", 0, 1, 0, "", 
    timer_flag.Enabled + timer_flag.Replace, "my_callback")

-- 一次定时器
AddTimer("once", 0, 0, 5, "", 
    timer_flag.Enabled + timer_flag.Replace + timer_flag.OneShot, "my_callback")
```

---

## custom_colour

MUSHclient 兼容的颜色常量（预留给脚本使用）。

| 常量 | 值 | 说明 |
|------|-----|------|
| `custom_colour.Black` | 0 | 黑色 |
| `custom_colour.Maroon` | 1 | 栗色 |
| `custom_colour.Green` | 2 | 绿色 |
| `custom_colour.Olive` | 3 | 橄榄色 |
| `custom_colour.Navy` | 4 | 藏青色 |
| `custom_colour.Purple` | 5 | 紫色 |
| `custom_colour.Teal` | 6 | 青色 |
| `custom_colour.Silver` | 7 | 银色 |
| `custom_colour.Grey` | 8 | 灰色 |
| `custom_colour.Red` | 9 | 红色 |
| `custom_colour.Lime` | 10 | 亮绿色 |
| `custom_colour.Yellow` | 11 | 黄色 |
| `custom_colour.Blue` | 12 | 蓝色 |
| `custom_colour.Fuchsia` | 13 | 紫红色 |
| `custom_colour.Aqua` | 14 | 浅绿色 |
| `custom_colour.White` | 15 | 白色 |

---

## error_code

错误码常量。

| 常量 | 值 | 说明 |
|------|-----|------|
| `error_code.eOK` | 0 | 成功 |
| `error_code.eUnknownObject` | 1 | 未知对象 |
| `error_code.eItemAlreadyExists` | 2 | 项目已存在 |
| `error_code.eBadRegularExpression` | 3 | 无效正则表达式 |
| `error_code.eWildcardNotFound` | 4 | 通配符未找到 |
| `error_code.eCommandCancelled` | 5 | 命令已取消 |
| `error_code.eNoSuchCommand` | 6 | 无此命令 |
| `error_code.eInvalidObjectLabel` | 7 | 无效对象标签 |
| `error_code.eAmbiguousObjectName` | 8 | 歧义对象名 |

## error_desc

错误描述常量（错误码对应的文字描述表）。

```lua
print(error_desc.eUnknownObject)  -- 输出 "Unknown object"
```
