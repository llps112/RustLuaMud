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

| 常量 | 值 | 说明 |
|------|-----|------|
| `alias_flag.Enabled` | 1 | 启用别名 |
| `alias_flag.Replace` | 1024 | 同名替换 |
| `alias_flag.RegularExpression` | 2048 | 启用正则表达式匹配 |
| `alias_flag.IgnoreCase` | 4096 | 忽略大小写 |

**组合使用**:
```lua
AddAlias("go_n", "^n$", "north", 
    alias_flag.Enabled + alias_flag.Replace)

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

自定义颜色常量（预留给 MUSHclient 兼容）。

- `custom_colour.foreground` — 前景色
- `custom_colour.background` — 背景色

---

## error_code

错误码常量。

| 常量 | 值 | 说明 |
|------|-----|------|
| `error_code.BadArgs` | -1 | 参数错误 |
| `error_code.TypeMismatch` | -2 | 类型不匹配 |
| `error_code.NotFound` | -3 | 未找到 |

---

## error_desc

错误描述常量（错误码对应的文字描述表）。

```lua
print(error_desc[error_code.NotFound])  -- 输出错误描述
```
