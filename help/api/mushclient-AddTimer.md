# MUSHclient AddTimer 官方文档

> 来源：http://www.mushclient.com/scripts/function.php?name=AddTimer
> 保存日期：2026-06-11

Adds a timer to the list of timers.

**Prototype**: `long AddTimer(BSTR TimerName, short Hour, short Minute, double Second, BSTR ResponseText, long Flags, BSTR ScriptName);`

## 参数
- TimerName: 定时器名称，可为空
- Hour: 触发小时 (0~23)
- Minute: 触发分钟 (0~59)
- Second: 触发秒 (0~59.9999)，支持浮点数（需启用子秒定时器）
- Response_text: 发送到世界的文本
- Flags: 标志位组合
- ScriptName: 执行的脚本函数名

## Flags
| 常量 | 值 | 说明 |
|------|----|------|
| eEnabled | 1 | 定时器启用 |
| eAtTime | 2 | 指定时刻触发（否则为间隔触发） |
| eOneShot | 4 | 一次性（触发一次后删除） |
| eTimerSpeedWalk | 8 | 触发时执行 speedwalk |
| eTimerNote | 16 | 触发时执行 world.note |
| eActiveWhenClosed | 32 | 断开连接时仍然触发 |
| eReplace | 1024 | 替换同名定时器 |
| eTemporary | 16384 | 临时定时器（不保存到世界文件） |

Lua 常量：使用 `timer_flag` 表，如 `timer_flag.Enabled`, `timer_flag.Replace`

## 名称规则
- 以字母 (A-Z) 开头
- 后跟字母 (A-Z)、数字 (0-9) 或下划线 (_)

## 返回值
- eInvalidObjectLabel: 定时器名称无效
- eTimerAlreadyExists: 同名定时器已存在
- eScriptNameNotLocated: 无法定位脚本函数
- eTimeInvalid: 时间无效
- eOK: 添加成功
