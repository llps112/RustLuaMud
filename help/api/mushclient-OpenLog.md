# MUSHclient OpenLog / CloseLog / IsLogOpen 官方文档

> 来源：http://www.mushclient.com/scripts/function.php
> 保存日期：2026-06-12

## OpenLog

Starts logging to a log file.

**Prototype**: `long OpenLog(BSTR FileName, BOOL Append);`

### 描述

- **FileName**: 日志文件路径
- **Append**: TRUE 追加到已有文件，FALSE 覆盖已有文件

### 返回值
- 0: eOK — 成功
- 非零: 错误码

---

## IsLogOpen

Tests if the log file is open.

**Prototype**: `boolean IsLogOpen();`

### 描述

如果日志文件当前已打开则返回 TRUE，否则返回 FALSE。

---

## CloseLog

Closes the log file.

**Prototype**: `void CloseLog();`

### 描述

关闭当前打开的日志文件。
