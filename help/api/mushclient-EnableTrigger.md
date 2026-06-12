# MUSHclient EnableTrigger / EnableTimer 官方文档

> 保存日期：2026-06-12

## EnableTrigger

Enables or disables a trigger.

**Prototype**: `long EnableTrigger(BSTR TriggerName, BOOL Enabled);`

### 描述

启用或禁用指定的触发器。禁用的触发器不会对服务器输出进行匹配。

### 返回值
- 0: eOK
- 1: eInvalidObjectLabel — 触发器名称无效
- 2: eTriggerNotFound — 找不到触发器

---

## EnableTimer

Enables or disables a timer.

**Prototype**: `long EnableTimer(BSTR TimerName, BOOL Enabled);`

### 描述

启用或禁用指定的定时器。禁用的定时器不会触发。

### 返回值
- 0: eOK
- 1: eInvalidObjectLabel — 定时器名称无效
- 2: eTimerNotFound — 找不到定时器
