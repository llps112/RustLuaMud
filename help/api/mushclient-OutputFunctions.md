# MUSHclient 输出/通知函数

## Note

> 来源：http://www.mushclient.com/scripts/function.php?name=Note
> 保存日期：2026-06-11

发送消息到输出窗口（自动换行）。

**Prototype**: `void Note(BSTR Message);`

## ColourNote

> 来源：http://www.mushclient.com/scripts/function.php?name=ColourNote
> 保存日期：2026-06-11

在指定颜色中发送消息到输出窗口。可用版本：3.23+

**Prototype**: `void ColourNote(BSTR TextColour, BSTR BackgroundColour, BSTR Text);`

颜色名可用 `Debug("colours")` 查看，也支持 HTML 格式如 "#FAEBD7"。留空表示不改变。

## ColourTell

> 来源：http://www.mushclient.com/scripts/function.php?name=ColourTell
> 保存日期：2026-06-11

与 ColourNote 相同但不追加换行符。

**Prototype**: `void ColourTell(BSTR TextColour, BSTR BackgroundColour, BSTR Text);`

## Send / Execute

## Send

> 来源：http://www.mushclient.com/scripts/function.php?name=Send
> 保存日期：2026-06-11

发送文本到 MUD。不评估别名、speedwalk、脚本前缀等。

**Prototype**: `long Send(BSTR Message);`

## Execute

> 来源：http://www.mushclient.com/scripts/function.php?name=Execute
> 保存日期：2026-06-11

执行命令，如同在命令窗口中输入。会处理别名、speedwalk 等。可用版本：3.35+

**Prototype**: `long Execute(BSTR Command);`

## DoAfter

> 来源：http://www.mushclient.com/scripts/function.php?name=DoAfter
> 保存日期：2026-06-11

添加一次性临时定时器，在指定秒数后触发。可用版本：3.18+

**Prototype**: `long DoAfter(double Seconds, BSTR SendText);`

- Seconds 范围: 0.1~86399（23小时59分59秒）
- 使用子秒定时器需要启用全局配置 Timers → Timer Interval = 0
- 最大精度 0.1 秒
