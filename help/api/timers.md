# 定时器系统 API

定时器用于在指定时间间隔后自动执行脚本函数。

---

## AddTimer(name, hour, min, sec, response_text, flags, [script])

添加一个定时器。

- **参数**:
  | 参数 | 类型 | 必填 | 说明 |
  |------|------|------|------|
  | name | string | 是 | 定时器名称（唯一） |
  | hour | number | 是 | 小时（结合 min, sec 计算总间隔） |
  | min | number | 是 | 分钟 |
  | sec | number | 是 | 秒（支持浮点数如 0.1） |
  | response_text | string | 是 | 响应文本（当前未使用） |
  | flags | integer | 是 | 标志位 |
  | script | string | 否 | 回调脚本函数名 |

- **间隔计算**: 总间隔 = `hour * 3600 + min * 60 + sec` 秒。若总间隔 ≤ 0，则默认为 1 秒

- **标志位 (timer_flag)**:
  | 标志 | 值 | 说明 |
  |------|-----|------|
  | Enabled | 1 | 启用定时器 |
  | Replace | 1024 | 同名替换（若旧定时器被禁用，新定时器继承禁用状态） |
  | OneShot | 4 | 一次性定时器 |
  | AtTime | 8 | 指定时刻触发 |

- **回调执行**: `script` 参数为函数名（如 `"module.function"`）。触发时将调用此函数，传入定时器名称作为参数

- **示例**:
  ```lua
  -- 周期性定时器，每 60 秒执行一次
  AddTimer("my_timer", 0, 1, 0, "", 
      timer_flag.Enabled + timer_flag.Replace, 
      "my_module.my_callback")
  SetTimerOption("my_timer", "group", "my_group")

  -- 一次性定时器，1 秒后执行
  AddTimer("temp_timer", 0, 0, 1, "", 
      timer_flag.Enabled + timer_flag.Replace + timer_flag.OneShot, 
      "my_callback")
  
  -- 指定时刻触发（每天 23:50）
  AddTimer("reset_timer", 23, 50, 0, "", 
      timer_flag.Enabled + timer_flag.Replace + timer_flag.AtTime, 
      "daily_reset")
  ```

- **注意**: `Replace` 标志的行为——若旧同名定时器因 `EnableTimerGroup` 被禁用，新定时器继承禁用状态，直至脚本显式调用 `EnableTimer` 重新启用

---

## DeleteTimer(name)

删除指定名称的定时器。

- **参数**: `name` (string) - 定时器名称
- **返回值**: 无
- **示例**:
  ```lua
  DeleteTimer("temp_timer")
  ```

---

## GetTimerList()

获取所有定时器的名称列表。

- **参数**: 无
- **返回值**: `table` - 定时器名称数组
- **示例**:
  ```lua
  local list = GetTimerList()
  for i, name in ipairs(list) do
      print(name)
  end
  ```

---

## GetTimerInfo(name, info_type)

获取定时器的属性信息。

- **参数**:
  | info_type | 说明 | 返回值类型 |
  |-----------|------|-----------|
  | 1 | 名称 | string |
  | 2 | 小时 | number |
  | 3 | 分钟 | number |
  | 4 | 秒 | number |
  | 5 | 标志位 | integer |
  | 6 | 是否启用 | boolean |
  | 7 | 所属组 | string |
  | 8 | 间隔（秒） | number |

- **示例**:
  ```lua
  local enabled = GetTimerInfo("t1", 6)  -- 获取启用状态
  local group = GetTimerInfo("t1", 7)    -- 获取所属组
  ```

---

## SetTimerOption(name, option, value)

设置定时器的选项。

- **参数**:
  | option | 说明 | value 类型 |
  |--------|------|-----------|
  | "group" | 设置所属组 | string |
  | "timer_timestamp" | 设置时间戳 | number |

- **示例**:
  ```lua
  SetTimerOption("t1", "group", "my_group")
  ```

---

## EnableTimerGroup(group_name, enable)

启用或禁用整个定时器组。

- **参数**: `group_name` (string), `enable` (boolean)
- **返回值**: 无
- **示例**:
  ```lua
  EnableTimerGroup("my_group", false)  -- 禁用 my_group 组的所有定时器
  EnableTimerGroup("my_group", true)    -- 启用 my_group 组的所有定时器
  ```

---

## EnableTimer(name, enable)

启用或禁用单个定时器。

- **参数**: `name` (string), `enable` (boolean)
- **返回值**: `0` (成功) / `1` (定时器不存在)
- **示例**:
  ```lua
  EnableTimer("temp_timer", false)
  ```

---

## ResetTimer(name)

重置定时器的计时，使其从当前时间开始重新计时。

- **参数**: `name` (string) - 定时器名称
- **返回值**: `0` (成功) / `1` (定时器不存在)
- **示例**:
  ```lua
  ResetTimer("my_timer")  -- 重置计时器，重新开始计时
  ```
