# MUSHclient DoAfter / DoAfterNote / DoAfterSpecial / DoAfterSpeedWalk 官方文档

> 来源：
> - http://www.mushclient.com/scripts/function.php?name=DoAfter
> - http://www.mushclient.com/scripts/function.php?name=DoAfterNote
> - http://www.mushclient.com/scripts/function.php?name=DoAfterSpecial
> - http://www.mushclient.com/scripts/function.php?name=DoAfterSpeedWalk
> 保存日期：2026-06-12

## DoAfter

Adds a one-shot, temporary timer — simplified interface.

**Prototype**: `long DoAfter(double Seconds, BSTR SendText);`

### 描述

添加一个无标签的、临时的、一次性的定时器，在指定秒数后触发。
第一个参数（秒数）必须在 0.1 到 86,399 之间（0.1 秒 ~ 23 小时 59 分 59 秒）。
从 3.61 版本开始支持小数间隔（如 0.5 秒、1.2 秒）。
定时器的最小粒度是 0.1 秒。

触发时将文本发送到 MUD（send_to=0）。

**警告**：DoAfter 使用临时定时器实现，如果定时器被禁用则不会工作。

### Lua 示例
```lua
DoAfter(10, "eat food")
DoAfter(20.5, "get bottle bag")
DoAfter(21, "drink water")
```

### 返回值
- 0: eOK — 添加成功
- 1: eTimeInvalid — 时间无效（不在 0.1 ~ 86,399 范围）

**引入版本**: 3.18

---

## DoAfterNote

Adds a one-shot, temporary note timer — simplified interface.

**Prototype**: `long DoAfterNote(double Seconds, BSTR NoteText);`

### 描述

与 DoAfter 类似，但触发时将文本作为 Note 输出到输出窗口（send_to=2），而不是发送到 MUD。

秒数范围同 DoAfter：0.1 ~ 86,399，支持小数。

### 返回值
- 0: eOK
- 1: eTimeInvalid

**引入版本**: 3.18

---

## DoAfterSpecial

Adds a one-shot, temporary timer to carry out some special action.

**Prototype**: `long DoAfterSpecial(double Seconds, BSTR SendText, long SendTo);`

### 描述

与 DoAfter 类似，但可以指定文本发送到的位置（SendTo 参数）。

SendTo 取值：
| 值 | 说明 |
|----|------|
| 0 | World（发送到 MUD） |
| 1 | Command window（命令窗口） |
| 2 | Output window（输出窗口） |
| 3 | Status line（状态栏） |
| 4 | Notepad new（新建记事本） |
| 5 | Notepad append（追加到记事本） |
| 6 | Log File（日志文件） |
| 7 | Notepad replace（替换记事本） |
| 8 | Command queue（命令队列） |
| 9 | Send To Variable（发送到变量） |
| 10 | Execute（重新解析为命令） |
| 11 | Speedwalk（以 speedwalk 处理） |
| 12 | Script（发送到脚本引擎） |
| 13 | Immediate（不排队直接发送到 MUD） |
| 14 | Script after omit（发送到脚本引擎，在行被省略后） |

### 返回值
- 0: eOK
- 1: eTimeInvalid
- 2: eOptionOutOfRange — SendTo 参数不在 0~14 范围

**引入版本**: 3.35

---

## DoAfterSpeedWalk

Adds a one-shot, temporary speedwalk timer — simplified interface.

**Prototype**: `long DoAfterSpeedWalk(double Seconds, BSTR SendText);`

### 描述

与 DoAfter 类似，但命令被解释为 speedwalk 序列（send_to=11）。

秒数范围同 DoAfter：0.1 ~ 86,399，支持小数。

### 返回值
- 0: eOK
- 1: eTimeInvalid

**引入版本**: 3.18
