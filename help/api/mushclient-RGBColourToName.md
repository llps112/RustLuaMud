# RGBColourToName

## 概要

将 ANSI 色号转换为对应的颜色名称。

## 原型

```lua
name = RGBColourToName(colour)
```

## 参数

| 参数 | 类型 | 说明 |
|------|------|------|
| colour | number | ANSI 色号（0-15 标准色，或更高数值） |

## 返回值

返回颜色名称字符串。

## 颜色映射表

| 色号 | 名称 |
|------|------|
| 0 | black |
| 1 | red |
| 2 | green |
| 3 | yellow |
| 4 | blue |
| 5 | magenta |
| 6 | cyan |
| 7 | silver |
| 8 | grey |
| 9 | bright red |
| 10 | bright green |
| 11 | bright yellow |
| 12 | bright blue |
| 13 | bright magenta |
| 14 | bright cyan |
| 15 | white |

超出 0-15 范围的色号返回 `"colour_N"` 格式的字符串（如 `"colour_42"`）。

## 说明

- 该函数在 MUSHclient 中接受 RGB 颜色值（BBGGRR 格式），在本实现中接受 ANSI 色号（0-15）
- 通常与 `GetStyle` 配合使用，用于判断触发器回调中的文本颜色

## 示例

```lua
print(RGBColourToName(1))  --> "red"
print(RGBColourToName(7))  --> "silver"
print(RGBColourToName(42)) --> "colour_42"
```

## 参见

[GetStyle](file:///help/api/mushclient-GetStyle.md)
