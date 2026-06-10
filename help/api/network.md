# 网络 API

连接管理和连接回调接口。

---

## IsConnected()

检查当前是否已连接到 MUD 服务器。

- **参数**: 无
- **返回值**: `boolean` - `true` 已连接，`false` 未连接
- **示例**:
  ```lua
  if IsConnected() then
      send("look")
  else
      print("未连接到服务器")
  end
  ```

---

## Connect()

发起连接到 MUD 服务器。

- **参数**: 无
- **返回值**: 无
- **说明**: 请求连接服务器，连接参数由配置决定。若已连接则忽略
- **示例**:
  ```lua
  if not IsConnected() then
      Connect()
  end
  ```

---

## Disconnect()

断开与 MUD 服务器的连接。

- **参数**: 无
- **返回值**: 无
- **示例**:
  ```lua
  Disconnect()  -- 断开当前连接
  ```

---

## OnConnect()

连接回调抽象接口，连接建立时自动调用。

- **参数**: 无
- **返回值**: 无
- **说明**: 默认空函数（安全无操作），由 Lua 脚本覆盖实现自定义连接初始化逻辑
- **覆盖示例**:
  ```lua
  OnConnect = function()
      print("连接已建立")
      -- 在此添加自定义初始化逻辑
  end
  ```
- **触发时机**: TCP 连接成功建立后自动调用
- **注意**: `OnConnect()` 是**全局函数**，直接在全局作用域中覆盖即可，无需 local 声明
