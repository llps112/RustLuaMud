# 命令执行 API

向 MUD 服务器发送命令、模拟服务器输出、管理命令队列的接口。

---

## send(command)

向 MUD 服务器发送一条命令。

- **参数**: `command` (string) - 要发送的命令文本
- **返回值**: 无
- **使用场景**: 发送 MUD 指令到服务器
- **示例**:
  ```lua
  send("look")
  send("north")
  ```

---

## Execute(command)

MUSHclient 兼容的发送命令接口。

- **参数**: `command` (string) - 要发送的命令文本
- **返回值**: `0` (integer)
- **使用场景**: 与 MUSHclient 脚本的兼容接口
- **示例**:
  ```lua
  Execute("n")       -- 向北移动
  Execute("hp")      -- 查看状态
  ```
- **注意**: 与 `send()` 功能相同，仅在返回值上区别

---

## DiscardQueue()

清空所有待发送的命令队列。

- **参数**: 无
- **返回值**: 无
- **使用场景**: 取消所有已入队但尚未发送的命令
- **示例**:
  ```lua
  DiscardQueue()  -- 清空命令队列
  ```

---

## Simulate(text...)

模拟 MUD 服务器输出，触发匹配的触发器。

- **参数**: `text` (string, 可变参数) - 要模拟的文本，多参数会自动拼接
- **返回值**: 无
- **使用场景**: 测试触发器匹配、模拟服务器响应
- **示例**:
  ```lua
  Simulate("你走进了一个黑暗的洞穴。")
  Simulate("HP:100", " MP:50")  -- 多参数拼接
  ```
- **注意**: 文本按换行符分割，逐行匹配触发器，不会清空命令队列

---

## DeleteTemporaryTimers()

删除所有临时定时器。

- **参数**: 无
- **返回值**: 无
- **使用场景**: 清理一次性定时器
- **示例**:
  ```lua
  DeleteTemporaryTimers()
  ```
