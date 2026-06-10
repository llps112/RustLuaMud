# 别名系统 API

别名用于将用户输入的命令转换为其他命令或脚本调用。

---

## AddAlias(name, match_text, response, flags, [script])

添加一个别名。

- **参数**:
  | 参数 | 类型 | 必填 | 默认值 | 说明 |
  |------|------|------|--------|------|
  | name | string | 是 | - | 别名名称（唯一） |
  | match_text | string | 是 | - | 匹配模式 |
  | response | string | 是 | - | 替换文本 |
  | flags | integer | 是 | - | 标志位 |
  | script | string | 否 | "" | 回调脚本函数名 |

- **标志位 (alias_flag)**:
  | 标志 | 值 | 说明 |
  |------|-----|------|
  | Enabled | 1 | 启用别名 |
  | Replace | 1024 | 同名替换 |
  | RegularExpression | 2048 | 正则匹配 |
  | IgnoreCase | 4096 | 忽略大小写 |

- **匹配模式**: 当使用 `RegularExpression` 标志时，`match_text` 为正则表达式，可用 `%1`、`%2` 引用捕获组

- **示例**:
  ```lua
  -- 简单替换别名
  AddAlias("go_n", "^n$", "north", alias_flag.Enabled + alias_flag.Replace)
  
  -- 正则别名，带参数捕获
  AddAlias("goto_room", "^goto (.+)$", "go %1", 
      alias_flag.Enabled + alias_flag.Replace + alias_flag.RegularExpression)
  
  -- 脚本回调别名（指定 script 参数）
  AddAlias("my_alias", "^hello$", "", 
      alias_flag.Enabled + alias_flag.Replace + alias_flag.RegularExpression, 
      "my_module.my_handler")
  ```

---

## DeleteAlias(name)

删除指定名称的别名。

- **参数**: `name` (string) - 别名名称
- **返回值**: 无
- **示例**:
  ```lua
  DeleteAlias("go_n")
  ```

---

## GetAliasList()

获取所有别名的名称列表。

- **参数**: 无
- **返回值**: `table` - 别名名称数组
- **示例**:
  ```lua
  local list = GetAliasList()
  for i, name in ipairs(list) do
      print(name)
  end
  ```

---

## SetAliasOption(name, option, value)

设置别名的属性。

- **参数**:
  | option | 说明 | value 类型 |
  |--------|------|-----------|
  | "group" | 设置所属组 | string |
  | "regexp" | 设置正则表达式 | string |
  | "sequence" | 设置执行顺序 | number |

- **示例**:
  ```lua
  SetAliasOption("go_n", "group", "movement")
  ```
