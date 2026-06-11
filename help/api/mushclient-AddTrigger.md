# MUSHclient AddTrigger 官方文档

> 来源：http://www.mushclient.com/scripts/function.php?name=AddTrigger
> 保存日期：2026-06-11

Adds a trigger to the list of triggers.

**Prototype**: `long AddTrigger(BSTR TriggerName, BSTR MatchText, BSTR ResponseText, long Flags, short Colour, short Wildcard, BSTR SoundFileName, BSTR ScriptName);`

**默认值**: Sequence=100, Send to=world

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
| eTriggerOneShot | 32768 | One shot - delete after firing |

Lua: 应使用 `trigger_flag` 表的常量名，如 `trigger_flag.Enabled`, `trigger_flag.Replace`

## Colours

| 常量 | 值 |
|------|----|
| NOCHANGE | -1 |
| custom1~custom16 | 0~15 |

## 名称规则
- 以字母 (A-Z) 开头
- 后跟字母 (A-Z)、数字 (0-9) 或下划线 (_)

## 返回值
- eInvalidObjectLabel: 触发器名称无效
- eTriggerAlreadyExists: 同名触发器已存在
- eTriggerCannotBeEmpty: 匹配文本不能为空
- eScriptNameNotLocated: 无法定位脚本函数
- eBadRegularExpression: 正则表达式无效
- eOK: 添加成功
