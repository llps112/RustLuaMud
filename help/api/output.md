# 输出 API

在客户端输出窗口显示信息的接口。

---

## ColourNote(fg, bg, text)

在输出窗口显示带颜色的文本。

- **参数**:
  - `fg` (string) - 前景色名称
  - `bg` (string) - 背景色名称
  - `text` (string) - 显示文本
- **返回值**: 无
- **使用场景**: 在界面中以彩色文字显示提示信息
- **支持的颜色**: `red`, `green`, `blue`, `yellow`, `cyan`, `magenta`, `white`, `black`, `orange`, `pink`, `purple`, `grey`, `lightred`, `lightgreen`, `lightblue`, `lightyellow`, `lightcyan`, `lightmagenta`
- **示例**:
  ```lua
  ColourNote("red", "black", "错误信息")
  ColourNote("green", "black", "成功提示")
  ColourNote("yellow", "blue", "警告信息")
  ```

---

## Note(text)

在输出窗口显示普通文本。

- **参数**: `text` (string) - 显示文本
- **返回值**: 无
- **使用场景**: 显示不带颜色的提示信息
- **示例**:
  ```lua
  Note("这是一条提示信息")
  ```

---

## print(...)

标准 Lua print 函数，重定向到输出窗口。

- **参数**: 可变参数，支持 string/number/boolean/table/nil
- **返回值**: 无
- **使用场景**: 在输出窗口打印调试信息
- **示例**:
  ```lua
  print("Hello", "World")    -- 输出: Hello\tWorld
  print("n=", 42, "b=", true)
  ```
- **注意**: 参数间用 `\t` 分隔，末尾自动换行

---

## Tell(text)

在输出窗口显示文本（不带额外格式）。

- **参数**: `text` (string) - 显示文本
- **返回值**: 无
- **使用场景**: 直接输出文本内容
- **示例**:
  ```lua
  Tell("这是一条消息")
  ```

---

## SetStatus(text)

设置状态栏文本。

- **参数**: `text` (string) - 状态栏显示文本
- **返回值**: 无
- **使用场景**: 更新 UI 状态栏信息
- **示例**:
  ```lua
  SetStatus("等待输入...")
  ```

---

## log(message)

在输出窗口显示日志消息。

- **参数**: `message` (string) - 日志文本
- **返回值**: 无
- **使用场景**: 输出日志信息
- **示例**:
  ```lua
  log("这是一条日志消息")
  ```
