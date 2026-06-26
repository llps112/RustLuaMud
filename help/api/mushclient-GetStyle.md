# GetStyle

## 概要

在触发器回调中查询 styles 表中指定位置的样式信息。

## 原型

```lua
style = GetStyle(styles, position)
```

## 参数

| 参数 | 类型 | 说明 |
|------|------|------|
| styles | table | 触发器回调第 4 个参数，包含所有样式运行片段 |
| position | number | 基于 1 的字节位置（通常来自 `string.find` 的返回值） |

## 返回值

返回一个 table，包含以下字段：

| 字段 | 类型 | 说明 |
|------|------|------|
| start | number | 在 clean_line 中的起始字节偏移（0-based） |
| length | number | 区间长度（字节数） |
| textcolour | number | 前景色 ANSI 色号（0-15） |
| backcolour | number | 背景色 ANSI 色号（0-15） |
| bold | boolean | 是否粗体 |
| italic | boolean | 是否斜体 |
| underline | boolean | 是否下划线 |

如果指定的 position 不在任何已有样式区间内，返回 `nil`。

## 说明

该函数用于在触发器回调中判断 MUD 服务器输出的颜色信息：
- `\x1b[31m` 等 SGR 序列被引擎解析为样式运行片段
- 位置参数使用 1-based 字节索引（与 Lua `string.find` 返回值一致）
- 无 ANSI 颜色的纯文本包含一个默认样式运行（textcolour=7 "silver"，backcolour=0 "black"）

## 示例

```lua
-- 触发器回调：检查指定位置的颜色
function daytime_check(n, l, w, s)
    local col = string.find(l, w[1])
    if not col then return end
    local style = GetStyle(s, col)
    local color = RGBColourToName(style.textcolour)
    if color == "silver" and style.backcolour == 0 then
        return  -- 系统消息颜色，跳过
    end
    -- 玩家频道颜色，处理文本
end
```

## 参见

[RGBColourToName](file:///help/api/mushclient-RGBColourToName.md)
