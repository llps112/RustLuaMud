# MUSHclient Send / Execute 官方文档

> 保存日期：2026-06-12

## Send

Sends text to the MUD.

**Prototype**: `void Send(BSTR Text);`

### 描述

发送文本到 MUD 服务器。自动追加换行符。

### Lua 示例
```lua
Send("look")
Send("kill goblin")
```

---

## Execute

Executes a command as if you had typed it into the command window.

**Prototype**: `void Execute(BSTR Command);`

### 描述

执行命令，如同在命令窗口中输入一样。与 Send 不同，Execute 会经过别名匹配等完整处理流程。

### Lua 示例
```lua
Execute("look")      -- 发送给 MUD
Execute("/lua print('hello')")  -- 执行 Lua 代码
Execute("#cfg status")  -- 执行脚本别名
```
