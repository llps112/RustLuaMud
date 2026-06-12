# MUSHclient DiscardQueue / EvaluateSpeedwalk 官方文档

> 保存日期：2026-06-12

## DiscardQueue

Discards the speed walk queue.

**Prototype**: `void DiscardQueue();`

### 描述

清空 speedwalk 队列中所有未执行的命令。

---

## EvaluateSpeedwalk

Evaluates a speed walk string.

**Prototype**: `BSTR EvaluateSpeedwalk(BSTR SpeedwalkString);`

### 描述

将 speedwalk 字符串解析为实际方向列表。例如 `"3n"` 解析为 `"n;n;n"`。

### 返回值

返回解析后的方向字符串，以分号分隔。
