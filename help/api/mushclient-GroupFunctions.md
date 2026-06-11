# MUSHclient 常用群组操作函数

> 来源：http://www.mushclient.com/scripts/function.php (EnableGroup / EnableTriggerGroup / EnableTimerGroup / EnableAliasGroup)
> 保存日期：2026-06-11

## EnableGroup(name, enabled)

启用/禁用一组触发器、别名和定时器。

**Prototype**: `long EnableGroup(BSTR GroupName, BOOL Enabled);`

- 启用: `EnableGroup("groupname", true)`
- 禁用: `EnableGroup("groupname", false)`
- 返回值: 该组中的成员数量

## EnableTriggerGroup(name, enabled)

启用/禁用一组触发器。

**Prototype**: `long EnableTriggerGroup(BSTR GroupName, BOOL Enabled);`

## EnableTimerGroup(name, enabled)

启用/禁用一组定时器。

**Prototype**: `long EnableTimerGroup(BSTR GroupName, BOOL Enabled);`

## EnableAliasGroup(name, enabled)

启用/禁用一组别名。

**Prototype**: `long EnableAliasGroup(BSTR GroupName, BOOL Enabled);`

## 说明
- 无组的项具有空白（空）组名
- 仅在当前插件范围内生效（如果从插件调用）
- 对已启用/已禁用的项再次调用不会改变它们的状态
- 返回值是该组中成员的数量（不是实际启用/禁用的数量）
