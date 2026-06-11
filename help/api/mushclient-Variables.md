# MUSHclient 变量操作函数

## GetVariable

> 来源：http://www.mushclient.com/scripts/function.php?name=GetVariable
> 保存日期：2026-06-11

获取变量的值。

**Prototype**: `VARIANT GetVariable(BSTR VariableName);`

- 如果变量不存在，返回 EMPTY
- 如果名称无效，返回 NULL
- 变量以字符串形式存储
- 从插件中调用时使用当前插件的变量

## SetVariable

> 来源：http://www.mushclient.com/scripts/function.php?name=SetVariable
> 保存日期：2026-06-11

设置变量的值。

**Prototype**: `long SetVariable(BSTR VariableName, BSTR Contents);`

- 返回值: eInvalidObjectLabel (名称无效) 或 eOK

## DeleteVariable

> 来源：http://www.mushclient.com/scripts/function.php?name=DeleteVariable
> 保存日期：2026-06-11

删除一个变量。

**Prototype**: `long DeleteVariable(BSTR VariableName);`

## 名称规则
- 以字母 (A-Z) 开头
- 后跟字母 (A-Z)、数字 (0-9) 或下划线 (_)
