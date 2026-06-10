# 工具函数

辅助工具函数。

---

## GetUniqueNumber()

生成全局唯一的数字 ID。

- **参数**: 无
- **返回值**: `integer` - 唯一递增的数字
- **使用场景**: 为触发器、别名、定时器生成唯一名称
- **示例**:
  ```lua
  local id = GetUniqueNumber()
  AddTimer("timer_" .. id, 0, 0, 5, "", 
      timer_flag.Enabled + timer_flag.OneShot, "callback")
  ```

---

## Trim(str)

去除字符串首尾空白字符。

- **参数**: `str` (string) - 输入字符串
- **返回值**: `string` - 处理后的字符串
- **示例**:
  ```lua
  local trimmed = Trim("  hello world  ")  -- 返回 "hello world"
  ```

---

## MakeRegularExpression(text)

将普通文本转换为 Lua 正则表达式的 safe pattern。

- **参数**: `text` (string) - 普通文本
- **返回值**: `string` - 转义后的正则表达式
- **使用场景**: 在构造正则匹配时避免特殊字符干扰
- **示例**:
  ```lua
  local pattern = MakeRegularExpression("你死了。")
  -- 返回: "你死了。"
  ```
- **注意**: 此函数会转义正则中的特殊字符（如 `()`, `.`, `+`, `*` 等）

---

## GetPluginID()

获取当前插件 ID。

- **参数**: 无
- **返回值**: `string` - 插件 ID（MUSHclient 兼容，当前返回空字符串）
- **示例**:
  ```lua
  local id = GetPluginID()
  ```

---

## GetPluginInfo(plugin_id, info_type)

获取插件信息。

- **参数**:
  | info_type | 说明 | 返回值 |
  |-----------|------|--------|
  | 14 | 插件版本 | "1.0" |
  | 20 | 插件是否已安装 | boolean |

- **示例**:
  ```lua
  local installed = GetPluginInfo("some_plugin", 20)
  ```

---

## bit

位运算模块，提供按位操作函数。

| 函数 | 说明 |
|------|------|
| `bit.band(a, b)` | 按位与 |
| `bit.bor(a, b)` | 按位或 |
| `bit.bxor(a, b)` | 按位异或 |
| `bit.bnot(a)` | 按位取反 |
| `bit.lshift(a, n)` | 左移 |
| `bit.rshift(a, n)` | 右移 |

- **示例**:
  ```lua
  local flags = bit.bor(1, 1024)  -- 组合标志位
  if bit.band(flags, 1) ~= 0 then
      print("Enabled flag is set")
  end
  ```
