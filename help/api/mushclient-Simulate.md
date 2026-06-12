# MUSHclient Simulate 官方文档

> 来源：http://www.mushclient.com/scripts/function.php?name=Simulate
> 保存日期：2026-06-12

Simulate input from the MUD, for debugging purposes.

**Prototype**: `void Simulate(BSTR Text);`

## 描述

- 参数文本会被 MUSHclient 当做从 MUD 发送过来的数据进行处理
- 用于调试触发器，类似于 Game → Test Trigger 菜单选项
- **注意**：在脚本中谨慎使用。如果在触发器内调用 Simulate 导致另一个触发器触发，可能造成栈溢出

## Lua 示例

```lua
Simulate("Exits: north\n")
```

**引入版本**: 3.73
