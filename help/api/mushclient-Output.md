# MUSHclient Note / ColourNote / ColourTell / Tell / print 官方文档

> 保存日期：2026-06-12

## Note

Sends a message to the output window.

**Prototype**: `void Note(BSTR Text);`

### 描述

在输出窗口显示一条消息，自动追加换行。

### Lua 示例
```lua
Note("hello world")
Note("当前血量: " .. hp)
```

## ColourNote

Sends a message to the output window in specified colours.

**Prototype**: `void ColourNote(BSTR TextColour, BSTR BackgroundColour, BSTR Text);`

### 描述

以指定前景色和背景色在输出窗口显示文本，自动追加换行。

颜色参数：颜色名称（如 `"red"`、`"blue"`）或 RGB 十六进制值。

### Lua 示例
```lua
ColourNote("red", "black", "危险！")
ColourNote("white", "blue", "信息提示")
```

## ColourTell

Sends a message to the output window in specified colours — not terminated by a newline.

**Prototype**: `void ColourTell(BSTR TextColour, BSTR BackgroundColour, BSTR Text);`

### 描述

与 ColourNote 类似，但**不追加换行**，可用于构建内联输出。

## Tell

Sends a message to the output window — not terminated by a newline.

**Prototype**: `void Tell(BSTR Text);`

### 描述

在输出窗口显示文本，**不追加换行**。与 Note 的区别在于不自动换行。

## print

Lua 标准函数，MUSHclient 兼容实现。

**Prototype**: `void print(...);`

### 描述

Lua 标准 print 函数，输出到 MUSHclient 的输出窗口。接受多个参数，自动追加换行。

### Lua 示例
```lua
print("变量值:", x, y, z)
local t = {a=1, b=2}
print(t)
```
