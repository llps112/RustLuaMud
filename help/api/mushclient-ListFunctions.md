# MUSHclient 列表函数

## GetTriggerList

> 来源：http://www.mushclient.com/scripts/function.php?name=GetTriggerList
> 保存日期：2026-06-11

获取所有已命名触发器的列表。

**Prototype**: `VARIANT GetTriggerList();`

- 从插件中调用时返回当前插件的触发器
- 从 3.30 版本起返回所有触发器（包括无标签的，分配内部标签如 "*trigger42"）

## GetTimerList

> 来源：http://www.mushclient.com/scripts/function.php?name=GetTimerList
> 保存日期：2026-06-11

获取所有定时器的名称列表。

**Prototype**: `VARIANT GetTimerList();`

## GetAliasList

> 来源：http://www.mushclient.com/scripts/function.php?name=GetAliasList
> 保存日期：2026-06-11

获取所有已命名别名的列表。

**Prototype**: `VARIANT GetAliasList();`

- 从 3.40 版本起返回所有别名（包括无标签的）

## GetPluginList

> 来源：http://www.mushclient.com/scripts/function.php?name=GetPluginList
> 保存日期：2026-06-11

列出所有插件。

**Prototype**: `VARIANT GetPluginList();`

- 可用版本：3.38+
