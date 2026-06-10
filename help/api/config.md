# 配置 API

获取客户端环境信息和配置项。

---

## GetInfo(code)

获取客户端环境信息。

- **参数**: `code` (integer) - 信息代码
- **返回值**: 根据 code 不同返回不同类型
- **支持的 code**:

  | code | 说明 | 返回值类型 | 示例 |
  |------|------|-----------|------|
  | 1 | 主机地址 | string | "mud.example.com" |
  | 2 | 端口 | integer | 6666 |
  | 3 | 是否已连接 | boolean | true/false |
  | 4 | 连接 ID | integer | 1 |
  | 5 | 插件 ID | string | "" |
  | 6 | 脚本引擎版本 | string | "Lua 5.4" |
  | 7 | 世界名称 | string | 配置名称 |
  | 35 | 脚本目录 | string | "/path/to/scripts/" |
  | 204 | 数据包计数 | integer | 网络数据包计数 |

- **示例**:
  ```lua
  local host = GetInfo(1)           -- 获取主机地址
  local port = GetInfo(2)           -- 获取端口
  local connected = GetInfo(3)      -- 获取连接状态
  local script_dir = GetInfo(35)    -- 获取脚本目录
  local packets = GetInfo(204)      -- 获取数据包计数
  ```

---

## SetOption(option_name, option_value)

设置客户端选项。

- **参数**: `option_name` (string), `option_value` (string)
- **返回值**: 无
- **示例**:
  ```lua
  SetOption("name", "myworld")
  ```

---

## GetOption(option_name)

获取客户端选项值。

- **参数**: `option_name` (string)
- **返回值**: `string` - 选项值（可能为空字符串）
- **示例**:
  ```lua
  local name = GetOption("name")
  ```

---

## SetAlphaOption(option_name, option_value)

设置字母选项（MUSHclient Alpha 选项兼容）。

- **参数**: `option_name` (string), `option_value` (string)
- **返回值**: 无
- **示例**:
  ```lua
  SetAlphaOption("scroll_lock", "0")
  ```

---

## GetAlphaOption(option_name)

获取字母选项值。

- **参数**: `option_name` (string)
- **返回值**: `string` - 选项值
- **示例**:
  ```lua
  local value = GetAlphaOption("scroll_lock")
  ```
