# JSON 序列化 API

Lua 值与 JSON 字符串之间的序列化/反序列化接口，供 Web UI 使用。

---

## json_encode(value)

将 Lua 值序列化为 JSON 字符串。

- **参数**: `value` (any) - 要序列化的 Lua 值
- **返回值**: `string` - JSON 字符串
- **支持类型**: nil, boolean, number, string, table, array
- **示例**:
  ```lua
  -- 成功响应
  local json = json_encode({name="张三", age=30})
  -- 返回: {"name":"张三","age":30}

  local json = json_encode({1, 2, 3})
  -- 返回: [1,2,3]
  ```
- **错误响应**:
  ```lua
  local ok, err = pcall(json_encode, function() end)
  -- err: "json_encode 失败: ..."
  ```

---

## json_decode(json_string)

将 JSON 字符串解析为 Lua 值。

- **参数**: `json_string` (string) - JSON 格式字符串
- **返回值**: `any` - 解析后的 Lua 值
- **示例**:
  ```lua
  -- 成功响应
  local t = json_decode('{"name":"张三","age":30}')
  -- t.name = "张三", t.age = 30

  local arr = json_decode('[1,2,3]')
  -- arr = {1, 2, 3}
  ```
- **错误响应**:
  ```lua
  local ok, err = pcall(json_decode, "invalid json")
  -- err: "json_decode 失败: ..."
  ```
- **类型映射**:

  | JSON 类型 | Lua 类型 |
  |-----------|----------|
  | string | string |
  | number | number |
  | boolean | boolean |
  | null | nil |
  | object | table |
  | array | table (连续索引) |
