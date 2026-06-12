# MUSHclient ResetTimer 官方文档

> 来源：http://www.mushclient.com/scripts/function.php?name=ResetTimer
> 保存日期：2026-06-12

Resets a named timer.

**Prototype**: `long ResetTimer(BSTR TimerName);`

## 描述

重置指定的定时器，使其从初始值重新开始计时。

**注意**：未启用的定时器不会被重置。如需重置，先使用 `EnableTimer` 启用定时器。

另见 `ResetTimers` 重置所有定时器。

## 返回值

| 值 | 说明 |
|----|------|
| 0 | eOK — 重置成功 |
| 1 | eInvalidObjectLabel — 定时器名称无效 |
| 2 | eTimerNotFound — 找不到指定的定时器 |
