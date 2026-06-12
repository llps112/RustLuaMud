# MUSHclient SendPkt 官方文档

> 来源：http://www.mushclient.com/scripts/function.php?name=SendPkt
> 保存日期：2026-06-12

Send a low-level packet of data to the MUD.

**Prototype**: `long SendPkt(BSTR Packet);`

## 描述

- 发送指定原始文本到 MUD
- **不附加**换行符
- 如果文本包含 IAC 字符 (0xFF)，**不会**自动插入第二个 IAC 字符（普通 Send 会）
- 文本**不会**通过 OnPluginSend / OnPluginSent 回调
- 文本**不会**回显到屏幕
- 仅用于底层数据发送，如 telnet 协商选项

## 返回值

| 值 | 说明 |
|----|------|
| 0 | eOK — 发送成功 |
| 1 | eWorldClosed — 世界未连接 |

**引入版本**: 3.81
