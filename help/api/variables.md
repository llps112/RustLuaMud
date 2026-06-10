# 变量系统 API

持久化变量存储，用于在脚本执行和重载之间保存状态。

---

## GetVariable(name)

获取持久化变量的值。

- **参数**: `name` (string) - 变量名
- **返回值**: `string` - 变量值（变量不存在时返回空字符串）
- **示例**:
  ```lua
  local char_name = GetVariable("char_name")
  local char_pwd = GetVariable("char_password")
  ```

---

## SetVariable(name, value)

设置持久化变量的值。

- **参数**: `name` (string) - 变量名, `value` (string) - 变量值
- **返回值**: 无
- **示例**:
  ```lua
  SetVariable("last_location", "home")
  SetVariable("player_level", "10")
  ```

---

## DeleteVariable(name)

删除指定的持久化变量。

- **参数**: `name` (string) - 变量名
- **返回值**: 无
- **示例**:
  ```lua
  DeleteVariable("temp_data")
  ```

---

## GetVariableList()

获取所有持久化变量的名称列表。

- **参数**: 无
- **返回值**: `table` - 变量名数组
- **示例**:
  ```lua
  local vars = GetVariableList()
  for i, name in ipairs(vars) do
      print(name)
  end
  ```

---

## 全局变量注入

以下全局变量在脚本加载前由引擎自动注入：

| 变量 | 说明 | 来源 |
|------|------|------|
| `char_name` | 角色名 | 配置文件 username |
| `char_password` | 角色密码 | 配置文件 password |

- **示例**:
  ```lua
  me = {}
  me.charid = char_name
  me.pwd = char_password
  ```
