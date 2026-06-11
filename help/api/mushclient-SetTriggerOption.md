# MUSHclient SetTriggerOption 官方文档

> 来源：http://www.mushclient.com/scripts/function.php?name=SetTriggerOption
> 保存日期：2026-06-11

Sets the value of a named trigger option. 可用版本：3.29+

**Prototype**: `long SetTriggerOption(BSTR TriggerName, BSTR OptionName, BSTR Value);`

## 常用选项
| 选项名 | 类型 | 说明 |
|--------|------|------|
| "enabled" | y/n | 启用触发器 |
| "group" | string | 组名 |
| "match" | string | 匹配文本 |
| "send" | string | 发送文本 |
| "script" | string | 脚本函数名 |
| "regexp" | y/n | 是否使用正则表达式 |
| "ignore_case" | y/n | 忽略大小写 |
| "keep_evaluating" | y/n | 继续匹配后续触发器 |
| "one_shot" | y/n | 一次性触发器 |
| "omit_from_log" | y/n | 从日志排除 |
| "omit_from_output" | y/n | 从输出排除 |
| "sequence" | number (0~10000) | 序列号 |
| "send_to" | number (0~14) | 发送目标 |
| "sound" | string | 声音文件 |
| "repeat" | y/n | 同一行重复匹配 |
| "expand_variables" | y/n | 展开变量 |
| "variable" | string | 变量名 |
| "user" | number | 用户自定义值 |
| "clipboard_arg" | number (0~10) | 复制到剪贴板的通配符 |
| "colour_change_type" | number | 颜色改变类型 (0=both, 1=foreground, 2=background) |
| "custom_colour" | number (0=no change, 1~16, 17=other) | 自定义颜色 |

## 布尔值语义
- "y", "Y", "1" → true
- "n", "N", "0" → false
- Lua 允许直接传 true/false
