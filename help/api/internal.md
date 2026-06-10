# 内部函数

脚本加载和内部功能相关函数。

---

## dofile(filename)

加载并执行一个 Lua 脚本文件。

- **参数**: `filename` (string) - 文件路径
- **返回值**: 取决于脚本
- **注意**: 此函数覆盖标准 Lua 的 `dofile`，用于加载游戏脚本
- **示例**:
  ```lua
  dofile("scripts/config.lua")
  ```

---

## rex

正则表达式模块，基于 PCRE 库提供增强的正则功能。

- **示例**:
  ```lua
  if rex.match("hello123", "%d+") then
      print("包含数字")
  end
  ```

---

## trigger, alias, timer

这三个函数用于在运行时动态创建触发器和别名。

### trigger(name, data)

创建或更新触发器。

- **参数**: 
  - `name` (string) - 名称
  - `data` (table) - 配置表，包含 pattern, callback, group 等字段

### alias(name, data)

创建或更新别名。

- **参数**: 同上

### timer(name, data)

创建或更新定时器。

- **示例**:
  ```lua
  trigger("my_trigger", {
      pattern = "^(%d+)",
      callback = function() print("matched") end,
      group = "my_group"
  })
  ```

---

## get/set

变量存取快捷方式。

### get(name)

获取变量值。

- **参数**: `name` (string)
- **返回值**: `string`

### set(name, value)

设置变量值。

- **参数**: `name` (string), `value` (string)
- **示例**:
  ```lua
  set("myvar", "hello")
  print(get("myvar"))  -- 输出 "hello"
  ```
