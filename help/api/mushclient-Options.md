# MUSHclient SetOption / GetOption / SetAlphaOption / GetAlphaOption 官方文档

> 来源：http://www.mushclient.com/scripts/function.php
> 保存日期：2026-06-12

## SetOption

Sets the value of a named world option.

**Prototype**: `long SetOption(BSTR OptionName, BSTR Value);`

### 描述

设置一个世界选项的值。选项名和值的定义见 MUSHclient 世界选项列表。

### 返回值
- 0: eOK
- 非零: 错误码

---

## GetOption

Gets the value of a named world option.

**Prototype**: `BSTR GetOption(BSTR OptionName);`

### 描述

获取指定世界选项的当前值。返回选项值的字符串表示。

---

## SetAlphaOption

Sets the value of a named "alpha" option.

**Prototype**: `long SetAlphaOption(BSTR OptionName, BSTR Value);`

### 描述

设置一个 alpha 选项的值。Alpha 选项是 MUSHclient 的扩展选项集。

---

## GetAlphaOption

Gets the value of a named "alpha" option.

**Prototype**: `BSTR GetAlphaOption(BSTR OptionName);`

### 描述

获取指定 alpha 选项的当前值。返回选项值的字符串表示。
