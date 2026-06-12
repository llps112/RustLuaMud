# MUSHclient IsConnected / Connect / Disconnect 官方文档

> 来源：http://www.mushclient.com/scripts/function.php
> 保存日期：2026-06-12

## IsConnected

Tests to see if the world is connected to the MUD server.

**Prototype**: `boolean IsConnected();`

### 描述

返回 TRUE 表示当前已连接。连接有多个阶段，因为连接不是同步的。

连接阶段：
| 阶段 | 说明 |
|------|------|
| 0 | 未连接（且未尝试连接） |
| 1 | MUD 名称解析（将 MUD 域名转为 IP） |
| 2 | 代理服务器名称解析 |
| 3 | 连接到 MUD（建立 TCP/IP 连接） |
| 4 | 连接到代理服务器 |
| 5 | 代理阶段 1（发送 SOCKS 认证方式） |
| 6 | 代理阶段 2（发送 SOCKS 用户名/密码） |
| 7 | 代理阶段 3（发送 SOCKS 连接详情） |
| 8 | 已连接到 MUD（完全连接） |

严格来说，`IsConnected` 在阶段 8 返回 TRUE，其他阶段返回 FALSE。

### 返回值

- TRUE: 世界已连接（非零值）
- FALSE: 未连接（值 0）

---

## Connect

Connects the world to the MUD server.

**Prototype**: `void Connect();`

### 描述

发起向 MUD 服务器的连接。

---

## Disconnect

Disconnects the world from the MUD server.

**Prototype**: `void Disconnect();`

### 描述

断开与 MUD 服务器的连接。
