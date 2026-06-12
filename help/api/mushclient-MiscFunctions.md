# MUSHclient MakeRegularExpression / GetPluginID / GetUniqueNumber / Trim / GetVariableList / DeleteTemporaryTimers 官方文档

> 保存日期：2026-06-12

## MakeRegularExpression

Converts a wildcard pattern to a regular expression.

**Prototype**: `BSTR MakeRegularExpression(BSTR Pattern);`

### 描述

将通配符模式转换为正则表达式。`*` → `(.*)`，`?` → `(.)`。

### Lua 示例
```lua
local re = MakeRegularExpression("hello * world?")
-- 返回: "hello (.*) world(.)"
```

---

## GetPluginID

Returns the ID of the current plugin.

**Prototype**: `BSTR GetPluginID();`

### 描述

返回当前插件的唯一标识符。非插件环境中调用可能返回空字符串或默认 ID。

---

## GetUniqueNumber

Returns a unique number.

**Prototype**: `long GetUniqueNumber();`

### 描述

每次调用返回一个全局唯一的递增数字。用于生成临时名称。

### Lua 示例
```lua
local id = GetUniqueNumber()
AddTrigger("temp_trig_" .. id, ...)
```

---

## Trim

Trims whitespace from a string.

**Prototype**: `BSTR Trim(BSTR Text);`

### 描述

去除字符串两端的空白字符（空格、制表符、换行等）。

### Lua 示例
```lua
local cleaned = Trim("  hello world  ")
-- 返回: "hello world"
```

---

## GetVariableList

Gets the list of all variables.

**Prototype**: `BSTR GetVariableList();`

### 描述

返回所有变量的列表。通常以换行分隔的字符串形式返回，每行包含变量名和值。

---

## DeleteTemporaryTimers

Deletes all temporary timers.

**Prototype**: `void DeleteTemporaryTimers();`

### 描述

删除所有一次性临时定时器（通过 DoAfter 或 AddTimer 创建的临时定时器）。
