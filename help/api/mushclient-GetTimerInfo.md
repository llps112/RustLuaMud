# MUSHclient GetTimerInfo 官方文档

> 来源：http://www.mushclient.com/scripts/function.php?name=GetTimerInfo
> 保存日期：2026-06-11

Gets details about a timer.

**Prototype**: `VARIANT GetTimerInfo(BSTR TimerName, short InfoType);`

## GetTimerInfo code 映射

| code | 说明 | 返回值类型 |
|------|------|-----------|
| 1 | The hour | short |
| 2 | The minute | short |
| 3 | The second | short |
| 4 | What to send | string |
| 5 | Script procedure name | string |
| 6 | Enabled | boolean |
| 7 | One shot timer | boolean |
| 8 | "At" timer (if false, fires "every") | boolean |
| 9 | Invocation count | long |
| 10 | Times matched | long |
| 11 | Date/time timer last fired (or was reset) | date |
| 12 | Date/time timer will fire next | date |
| 13 | Number of seconds until timer will fire next (double) | double |
| 14 | 'temporary' flag | boolean |
| 15 | 'speed walk' flag | boolean |
| 16 | 'note' flag | boolean |
| 17 | 'active when disconnected' flag | boolean |
| 18 | Timer was included from an include file | boolean |
| 19 | Group name | string |
| 20 | Send-to location | long |
| 21 | User option value | long |
| 22 | Timer label | string |
| 23 | 'Omit from output' flag | boolean |
| 24 | 'Omit from log file' flag | boolean |
| 25 | 'Executing-script' flag | boolean |
| 26 | Script is valid flag | boolean |

## 说明

- 如果指定 timer 不存在，返回 EMPTY
- 如果 timer 名称无效，返回 NULL
- 如果 InfoType 超出范围，返回 NULL
