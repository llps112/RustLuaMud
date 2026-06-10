# 触发器系统 API

触发器用于自动响应 MUD 服务器的输出文本，是脚本自动化的核心机制。

---

## AddTrigger(name, match_str, response, flags, colour, wildcard, sound, script, send_to, sequence)

添加一个触发器。

- **参数**:
  | 参数 | 类型 | 必填 | 说明 |
  |------|------|------|------|
  | name | string | 是 | 触发器名称（唯一） |
  | match_str | string | 是 | 匹配模式（支持正则） |
  | response | string | 是 | 响应文本（当前未使用） |
  | flags | integer | 是 | 标志位 |
  | colour | integer | 否 | 颜色 |
  | wildcard | integer | 否 | 通配符 |
  | sound | string | 否 | 声音文件 |
  | script | string | 是 | 回调脚本函数名 |
  | send_to | integer | 否 | 发送目标 |
  | sequence | integer | 否 | 执行顺序 |

- **标志位 (trigger_flag)**:
  | 标志 | 值 | 说明 |
  |------|-----|------|
  | Enabled | 1 | 启用触发器 |
  | Replace | 1024 | 同名替换 |
  | RegularExpression | 2048 | 正则匹配 |
  | OneShot | 4096 | 一次性 |
  | OmitFromOutput | 65536 | 不显示在输出中 |

- **示例**:
  ```lua
  -- 添加一个正则触发器
  AddTrigger("status_trigger", "^(%w+):(%d+)", "", 
      trigger_flag.Enabled + trigger_flag.Replace + trigger_flag.RegularExpression,
      0, 0, "", "status_callback", 0, 10)
  SetTriggerOption("status_trigger", "group", "monitor")
  ```

---

## AddTriggerEx(name, match_str, response_text, flags, [colour], [wildcard], [sound], [script], [send_to], [sequence])

AddTrigger 的扩展版本，支持可变参数。

- **参数**: 同 AddTrigger，中间参数可选（可传 nil）
- **示例**:
  ```lua
  AddTriggerEx("t1", "匹配文本", "", trigger_flag.Enabled + trigger_flag.Replace)
  AddTriggerEx("t1", "匹配文本", "", trigger_flag.Enabled, 0, 0, "", "callback", 0, 10)
  ```

---

## DeleteTrigger(name)

删除指定名称的触发器。

- **参数**: `name` (string) - 触发器名称
- **返回值**: 无
- **示例**:
  ```lua
  DeleteTrigger("status_trigger")
  ```

---

## GetTriggerList()

获取所有触发器的名称列表。

- **参数**: 无
- **返回值**: `table` - 触发器名称数组
- **示例**:
  ```lua
  local list = GetTriggerList()
  for i, name in ipairs(list) do
      print(name)
  end
  ```

---

## GetTriggerInfo(name, info_type)

获取触发器的属性信息。

- **参数**:
  | info_type | 说明 | 返回值类型 |
  |-----------|------|-----------|
  | 1 | 名称 | string |
  | 2 | 匹配文本 | string |
  | 3 | 响应文本 | string |
  | 4 | 标志位 | integer |
  | 5 | 发送目标 | integer |
  | 6 | 序列 (sequence) | integer |
  | 7 | 是否启用 | boolean |
  | 8 | 所属组 | string |
  | 9 | 正则表达式 | string |

- **示例**:
  ```lua
  local name = GetTriggerInfo("t1", 1)      -- 获取名称
  local enabled = GetTriggerInfo("t1", 7)    -- 获取启用状态
  local group = GetTriggerInfo("t1", 8)      -- 获取所属组
  ```

---

## SetTriggerOption(name, option, value)

设置触发器的选项。

- **参数**:
  | option | 说明 | value 类型 |
  |--------|------|-----------|
  | "group" | 设置所属组 | string |
  | "regexp" | 设置正则表达式 | string |
  | "sequence" | 设置执行顺序 | number |

- **示例**:
  ```lua
  SetTriggerOption("t1", "group", "my_group")
  SetTriggerOption("t1", "sequence", 10)
  ```

---

## EnableTriggerGroup(group_name, enable)

启用或禁用整个触发器组。

- **参数**: `group_name` (string), `enable` (boolean)
- **返回值**: 无
- **示例**:
  ```lua
  EnableTriggerGroup("login", true)   -- 启用 login 组
  EnableTriggerGroup("login", false)  -- 禁用 login 组
  ```

---

## EnableTrigger(name, enable)

启用或禁用单个触发器。

- **参数**: `name` (string), `enable` (boolean)
- **返回值**: `0` (成功) / `1` (触发不存在)
- **示例**:
  ```lua
  EnableTrigger("t1", false)  -- 禁用触发器
  ```
