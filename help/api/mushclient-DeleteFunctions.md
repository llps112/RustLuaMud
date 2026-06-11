# MUSHclient 删除函数

## DeleteTrigger

> 来源：http://www.mushclient.com/scripts/function.php?name=DeleteTrigger
> 保存日期：2026-06-11

删除指定名称的触发器。

**Prototype**: `long DeleteTrigger(BSTR TriggerName);`

**警告**: 如果触发器正在执行脚本则无法删除。如果需要从触发器自身删除它，使用 DoAfterSpecial 延迟删除。

## DeleteTimer

> 来源：http://www.mushclient.com/scripts/function.php?name=DeleteTimer
> 保存日期：2026-06-11

删除指定名称的定时器。

**Prototype**: `long DeleteTimer(BSTR TimerName);`

## DeleteAlias

> 来源：http://www.mushclient.com/scripts/function.php?name=DeleteAlias
> 保存日期：2026-06-11

删除指定名称的别名。

**Prototype**: `long DeleteAlias(BSTR AliasName);`

## 批量删除
- DeleteTriggerGroup(group_name) — 删除一组触发器
- DeleteTimerGroup(group_name) — 删除一组定时器
- DeleteAliasGroup(group_name) — 删除一组别名
- DeleteGroup(group_name) — 同时删除触发器、别名和定时器
- DeleteTemporaryTriggers() — 删除所有临时触发器
- DeleteTemporaryTimers() — 删除所有临时定时器
- DeleteTemporaryAliases() — 删除所有临时别名
