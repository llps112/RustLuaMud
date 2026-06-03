# Phase 3 测试说明 - Lua 脚本引擎

## 测试前准备

1. 编译 release 版本：
```bash
cd /home/baiyf/RustLuaMud
cargo build --release
```

2. 确保配置文件 `configs/default.toml` 包含 `script` 字段：
```toml
[[connections]]
name = "wudaodm"
host = "ln.xkxmud.com"
port = 5555
encoding = "gbk"
script = "scripts/example.lua"
auto_connect = true
auto_reconnect = true
reconnect_delay_secs = 5
```

3. 确保 `scripts/example.lua` 存在。

---

## 测试项

### 1. 脚本自动加载

**操作**：启动程序，连接服务器

**预期**：
- 连接建立后，终端显示 `[Lua] 连接 1 脚本已加载: scripts/example.lua`
- 随后显示脚本初始化日志：
  - `[Lua] 脚本已加载: example.lua`
  - `[Lua] 可用别名: lh, gs, gn, gw, ge, gu, gd, sk, sc`
  - `[Lua] 设置自动登录: setname <名字>, setpwd <密码>`

### 2. 触发器 - 自动回答 BIG5 询问

**操作**：连接服务器后，服务器会发送 "Are you using BIG5 code?(Yes|No)"

**预期**：
- 客户端自动发送 "No"
- 终端显示 `[Lua] 自动回答 BIG5 询问`

### 3. 触发器 - 匹配服务器输出

**操作**：在游戏中触发被攻击或获得经验等事件

**预期**：
- 匹配到触发器模式时，自动执行对应操作
- `[Lua]` 前缀的日志信息出现在终端和日志文件中

### 4. 别名 - 简化输入

**操作**：输入 `lh`

**预期**：
- 自动发送 `look` 和 `hp` 两条命令
- 原始输入 `lh` 不会被发送到服务器

**操作**：输入 `gs`

**预期**：
- 自动发送 `go south`

**操作**：输入 `gn`、`gw`、`ge`、`gu`、`gd`

**预期**：
- 分别发送对应方向移动命令

### 5. 别名 - 带参数匹配

**操作**：输入 `setname mychar`

**预期**：
- 终端显示 `[Lua] 角色名已设置: mychar`
- 后续如果断线重连，触发器会自动使用保存的角色名

**操作**：输入 `setpwd mypass`

**预期**：
- 终端显示 `[Lua] 密码已设置`

### 6. 定时器

**操作**：等待 60 秒

**预期**：
- 每 60 秒自动发送 `hp` 命令
- 可以在终端看到 hp 命令的输出

### 7. /lua 命令 - 手动加载脚本

**操作**：输入 `/lua scripts/example.lua`

**预期**：
- 终端显示 `[Lua] 脚本已加载: scripts/example.lua`
- 脚本中的触发器、别名、定时器重新注册

### 8. /lua reload - 重新加载脚本

**操作**：修改 `scripts/example.lua`（例如修改定时器间隔），然后输入 `/lua reload`

**预期**：
- 终端显示 `[Lua] 脚本已重新加载: scripts/example.lua`
- 修改后的脚本生效

### 9. 变量持久化（会话内）

**操作**：
1. 输入 `setname testchar`
2. 触发断线重连
3. 服务器发送 "请输入你的名字" 时

**预期**：
- 触发器自动发送保存的角色名 `testchar`
- 终端显示 `[Lua] 自动输入角色名: testchar`

### 10. 无脚本运行

**操作**：将配置文件中 `script` 字段删除或留空，启动程序

**预期**：
- 程序正常运行，无 Lua 相关日志
- 所有 Phase 2 功能正常
- 输入的命令直接发送到服务器（无别名拦截）

### 11. 脚本语法错误

**操作**：创建一个有语法错误的 Lua 脚本，用 `/lua` 加载

**预期**：
- 终端显示 `[Lua] 脚本加载失败: ...` 及错误信息
- 程序不崩溃，继续正常运行

### 12. 多连接独立脚本

**操作**：配置多个连接，每个连接指定不同脚本

**预期**：
- 每个连接有独立的 Lua 引擎实例
- 触发器、别名、变量互不影响
- 切换前台连接后，别名匹配使用当前连接的脚本

---

## Lua API 参考

| API | 说明 | 示例 |
|-----|------|------|
| `send(cmd)` | 发送命令到服务器 | `send("look")` |
| `log(msg)` | 记录日志（终端+文件） | `log("触发成功")` |
| `trigger(pattern, callback)` | 注册触发器 | `trigger("^你好", function() end)` |
| `alias(pattern, callback)` | 注册别名 | `alias("^lh$", function() send("look") end)` |
| `timer(interval, callback)` | 注册定时器（秒） | `timer(30, function() send("hp") end)` |
| `get(key)` | 获取变量 | `local name = get("char_name")` |
| `set(key, value)` | 设置变量 | `set("char_name", "mychar")` |

**触发器 callback 参数**：`matches` 表，`matches[1]` 为第一个捕获组
**别名 callback 参数**：原始输入字符串（带捕获组时为 `matches` 表）

**注意**：
- `trigger` 和 `alias` 的 pattern 使用正则表达式（Rust regex 语法）
- 特殊字符需要转义，如 `%?` 转义 `?`
- `timer` 的间隔单位为秒
