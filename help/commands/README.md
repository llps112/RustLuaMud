# 客户端命令

RustLuaMud 提供了一系列内置命令，用于控制客户端行为和 MUD 游戏操作。

- [CLUI 操作指南](clui.md) — 终端界面操作说明

---

## 命令输入

在终端输入命令后按回车即可发送。命令分为三类：

1. **MUD 命令**: 直接发送到游戏服务器，如 `look`, `north`
2. **客户端内置命令**: 以 `/` 开头，由客户端处理后执行，如 `/lua`, `/load`, `/set`
3. **Lua 执行**: 以 `/lua` 开头的命令作为 Lua 代码执行，如 `/lua print("hello")`

---

## 广播命令 `/all`

`/all` 将一条命令同时发送到**所有已连接 session**，有两种工作模式：

### 1. 发送 MUD 命令（广播到游戏服务器）

```
/all look
/all north
/all eat apple
```

所有 session 向各自的 MUD 服务器发送 `look`，适用于**多名角色同时做同一动作**。

### 2. 发送客户端命令（逐 session 执行）

`/all` 后跟以 `/` 开头的客户端命令时，会**在每个 session 上分别执行该客户端命令**。

| `/all` 子命令 | 行为 |
|------|------|
| `/all /lua <代码>` | 每个 session 的 Lua 引擎分别执行代码 |
| `/all /reload` | 每个 session 重载脚本（保留变量与连接状态） |
| `/all /load reload` | 同上 |
| `/all /load <路径>` | 每个 session 加载指定脚本 |
| `/all /list` | 列出所有连接 |
| `/all /sw` | ❌ **拒绝** |
| `/all /close` | ❌ **拒绝** |
| `/all /disconnect` | ❌ **拒绝** |
| `/all /connect` | ❌ **拒绝** |
| `/all /profile` | ❌ **拒绝** |
| `/all /set` | ❌ **拒绝** |

**安全白名单**：只有 `/lua`、`/reload`、`/load`、`/list` 允许广播。其它客户端命令（如 `/switch`、`/close`、`/profile`）在广播语境下无意义或危险，会被拒绝并提示允许的命令列表。

### 用法示例

```
/all look                      所有角色同时 look
/all /lua print(have.gold)     查看所有角色的金币
/all /reload                   所有角色重载脚本
/all /list                     列出所有连接
/all /sw 1                     拒绝：切换连接不能广播
```
