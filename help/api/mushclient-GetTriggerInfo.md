# MUSHclient GetTriggerInfo 官方文档

> 来源：http://www.mushclient.com/scripts/function.php?name=GetTriggerInfo
> 保存日期：2026-06-11

Gets details about a named trigger.

**Prototype**: `VARIANT GetTriggerInfo(BSTR TriggerName, short InfoType);`

## GetTriggerInfo code 映射

| code | 说明 | 返回值类型 |
|------|------|-----------|
| 1 | What to match on | string |
| 2 | What to send | string |
| 3 | Sound to play | string |
| 4 | Script procedure name | string |
| 5 | Omit from log | boolean |
| 6 | Omit from output | boolean |
| 7 | Keep evaluating | boolean |
| 8 | Enabled | boolean |
| 9 | Regular expression | boolean |
| 10 | Ignore case | boolean |
| 11 | Repeat on same line | boolean |
| 12 | Play sound if inactive | boolean |
| 13 | Expand variables | boolean |
| 14 | Which wildcard to send to clipboard | short |
| 15 | Send to location | short |
| 16 | Sequence | short |
| 17 | Match on (colour/style) | short |
| 18 | Change to style | short |
| 19 | Change to colour | short |
| 20 | Invocation count | long |
| 21 | Times matched | long |
| 22 | Date/time trigger last matched | date |
| 23 | 'temporary' flag | boolean |
| 24 | Trigger was included from an include file | boolean |
| 25 | Make wildcards lower case flag | boolean |
| 26 | Group name | string |
| 27 | Variable name | string |
| 28 | User option value | long |
| 29 | Other colour foreground colour value | long |
| 30 | Other colour background colour value | long |
| 31 | Number of matches to regular expression (most recent match) | long |
| 32 | The string we matched against | string |
| 33 | Executing-script flag | boolean |
| 34 | Script is valid flag | boolean |
| 35 | Error number from PCRE when evaluating last match | long |
| 36 | 'one shot' flag | boolean |
| 37 | Time taken (in seconds) to test triggers | double |
| 38 | Number of attempts to match this trigger | long |
| 101 | Wildcard %1 from last time it matched | string |
| 102 | Wildcard %2 from last time it matched | string |
| 103 | Wildcard %3 from last time it matched | string |
| 104 | Wildcard %4 from last time it matched | string |
| 105 | Wildcard %5 from last time it matched | string |
| 106 | Wildcard %6 from last time it matched | string |
| 107 | Wildcard %7 from last time it matched | string |
| 108 | Wildcard %8 from last time it matched | string |
| 109 | Wildcard %9 from last time it matched | string |
| 110 | Wildcard %0 from last time it matched | string |

## 说明

- 如果指定 trigger 不存在，返回 EMPTY
- 如果 trigger 名称无效，返回 NULL
- 如果 InfoType 超出范围，返回 NULL

## Send-to 位置

| 值 | 说明 |
|----|------|
| 0 | World (send to MUD) |
| 1 | Command window |
| 2 | Output window |
| 3 | Status line |
| 4 | Notepad (new) |
| 5 | Notepad (append) |
| 6 | Log File |
| 7 | Notepad (replace) |
| 8 | Command queue |
| 9 | Send To Variable |
| 10 | Execute (re-parse as command) |
| 11 | Speedwalk |
| 12 | Script (send to script engine) |
| 13 | Immediate (send in front of speedwalk queue) |
| 14 | Script - after omit |
