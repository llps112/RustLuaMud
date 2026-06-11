# MUSHclient SetTimerOption 官方文档

> 来源：http://www.mushclient.com/scripts/function.php?name=SetTimerOption
> 保存日期：2026-06-11

Sets the value of a named timer option. 可用版本：3.29+

**Prototype**: `long SetTimerOption(BSTR TimerName, BSTR OptionName, BSTR Value);`

## 常用选项
| 选项名 | 类型 | 说明 |
|--------|------|------|
| "enabled" | y/n | 启用定时器 |
| "group" | string | 组名 |
| "at_time" | y/n | 指定时刻触发（否则为间隔触发） |
| "one_shot" | y/n | 一次性定时器 |
| "hour" | number (0~23) | 触发小时 |
| "minute" | number (0~59) | 触发分钟 |
| "second" | number (0~59) | 触发秒 |
| "offset_hour" | number | 偏移小时 |
| "offset_minute" | number | 偏移分钟 |
| "offset_second" | number | 偏移秒 |
| "send" | string | 发送文本 |
| "script" | string | 脚本函数名 |
| "send_to" | number (0~14) | 发送目标 |
| "user" | number | 用户自定义值 |
| "variable" | string | 变量名 |
| "active_closed" | y/n | 断开连接时仍然触发 |
| "omit_from_log" | y/n | 从日志排除 |
| "omit_from_output" | y/n | 从输出排除 |

## 布尔值语义
- "y", "Y", "1" → true
- "n", "N", "0" → false
- Lua 允许直接传 true/false
