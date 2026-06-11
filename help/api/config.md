# 配置 API

获取客户端环境信息和配置项。

---

## GetInfo(code)

获取客户端环境信息。

- **参数**: `code` (integer) - 信息代码
- **返回值**: 根据 code 不同返回不同类型
- **支持的 code**（MushClient 标准）:

  | code | 说明 | 返回值类型 | 示例 |
  |------|------|-----------|------|
  | 1 | 服务器地址 (Server name) | string | "mud.example.com" |
  | 2 | 世界名称 (World name) | string | "我的世界" |
  | 3 | 角色名 (Character name) | string | "张三" |
  | 35 | 脚本文件名 (Script file name) | string | "scripts\\myscript.lua" |
  | 56 | MUSHclient 应用程序路径 | string | 本引擎不支持，返回空串 |
  | 58 | 日志文件默认目录 | string | "scripts\\" |
  | 204 | 已接收数据包数 | integer | 网络数据包计数 |

- **示例**:
  ```lua
  local host = GetInfo(1)               -- 获取服务器地址
  local world_name = GetInfo(2)         -- 获取世界名称
  local char_name = GetInfo(3)          -- 获取角色名
  local script_file = GetInfo(35)       -- 获取脚本文件名
  local log_dir = GetInfo(58)           -- 获取日志目录
  local packets = GetInfo(204)          -- 获取数据包计数
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
