# MUSHclient AddTriggerEx 官方文档

> 来源：http://www.mushclient.com/scripts/function.php?name=AddTriggerEx
> 保存日期：2026-06-11

Adds a trigger - extended arguments. 可用版本：3.18+

**Prototype**: `long AddTriggerEx(BSTR TriggerName, BSTR MatchText, BSTR ResponseText, long Flags, short Colour, short Wildcard, BSTR SoundFileName, BSTR ScriptName, short SendTo, short Sequence);`

## 参数
- Name: 触发器名称，可为空
- Match_text: 匹配模式
- Response_text: 发送文本
- Flags: 标志位组合
- Colour: 触发文本颜色 (NOCHANGE=-1, custom1~16=0~15)
- Wildcard: 复制到剪贴板的通配符编号 (0=不复制, 1~10)
- SoundFileName: 播放的声音文件
- ScriptName: 执行的脚本函数名
- Sequence: 序列号 (0~10000)，越小越先匹配
- SendTo: 发送目标

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

## Flags
| 常量 | 值 | 说明 |
|------|----|------|
| eEnabled | 1 | Enable trigger |
| eOmitFromLog | 2 | Omit from log file |
| eOmitFromOutput | 4 | Omit trigger from output |
| eKeepEvaluating | 8 | Keep evaluating |
| eIgnoreCase | 16 | Ignore case when matching |
| eTriggerRegularExpression | 32 | Trigger uses regular expression |
| eExpandVariables | 512 | Expand variables like @direction |
| eReplace | 1024 | Replace existing trigger of same name |
| eTemporary | 16384 | Temporary - do not save to world file |

**注意**: AddTriggerEx 没有 eTriggerOneShot (32768) 标志，应使用 AddTrigger 来添加一次性触发器。

## 返回值
- eInvalidObjectLabel, eTriggerAlreadyExists, eTriggerCannotBeEmpty
- eScriptNameNotLocated, eBadRegularExpression
- eTriggerSequenceOutOfRange (sequence not in 0~10000)
- eTriggerSendToInvalid (send_to not in 0~9)
- eOK
