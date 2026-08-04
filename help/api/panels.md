# 浮动面板 API

> RustLuaMud 扩展 API（非 MUSHclient 标准）

浮动面板（Floating Panel）是绘制在终端输出区之上的 overlay 层，不随输出文本滚动。适合显示需要持续可见的实时信息，如统计数据、小地图、调试信息等。

---

## 坐标系统

面板使用 **相对坐标** 定位，支持负数锚定到终端边缘：

| 坐标 | 正数 | 负数 |
|------|------|------|
| `x` | 从左边缘往右的列数（0 = 最左列） | 从右边缘往左的列数（-1 = 最右列） |
| `y` | 从输出区顶部往下的行数（0 = 紧贴状态栏下方） | 从输出区底部往上的行数（-1 = 紧贴 Lua 状态栏上方） |

```
终端布局示意（80x24）：

Row 0:  ┌─────────── 状态栏 ───────────┐
        │                                │
Row 1:  │  输出区                  ┌───┐ │  ← y=0（面板顶部）
        │                          │面板│ │
        │                          │   │ │
        │                          └───┘ │  ← y=面板高度
        │                                │
Row 22: ├─────────── Lua 状态栏 ─────────┤  ← y=-1（面板底部）
Row 23: ├─────────── 输入行 ────────────┤
        └────────────────────────────────┘

面板 x=-30, y=0, width=30, height=5
→ 绝对位置：x = 80-30 = 50, y = 1（紧贴状态栏下方）
```

**Resize 自适应**：终端窗口大小改变时，负坐标的面板会自动重新定位到新的边缘位置。

---

## SetPanel(name, x, y, width, height, text)

创建或更新浮动面板。同名面板会被覆盖更新。

- **参数**:
  - `name` (string) - 面板唯一标识符，相同 name 的后续调用会更新而非新建
  - `x` (number) - 列位置（正数=从左，负数=从右）
  - `y` (number) - 行位置（正数=从上，负数=从下）
  - `width` (number) - 面板宽度（列数）
  - `height` (number) - 面板高度（行数）
  - `text` (string) - 面板内容，行间用 `\n` 分隔
- **返回值**: 无
- **渲染规则**:
  - 面板内容样式（前景色、背景色）完全由 `text` 中的 ANSI 转义序列控制，Rust 渲染器仅负责定位和打印，不附加任何额外样式
  - 每行内容按可见宽度截断到 `width`，不足部分用空格填充；空格的视觉样式取决于当前行的 ANSI 状态（如已设置背景色则保持背景色填充）
  - 每行渲染完成后自动输出 `\x1b[0m` 重置所有样式，避免影响下一行
  - 内容行数少于 `height` 时，剩余行以空背景填充
  - 面板超出终端可见区域的部分会被自动裁剪
  - 面板存在时，每次输出区重绘后都会重绘面板（因为输出区重绘会覆盖面板区域）
- **设计原则**: Rust 渲染器对面板内容是**透明的**——不修改、不添加、不剥离任何 ANSI 码。这允许 Lua 脚本完全控制面板的视觉外观，包括：
  - 每行独立的背景色（交替行颜色、高亮行等）
  - 行内不同段的文字颜色
  - 边框样式、分隔符颜色等复杂绘制
- **使用场景**:
  - 固定显示实时统计信息（EXP、任务状态等）
  - 显示小地图或导航信息
  - 调试信息面板
- **示例**:

  ```lua
  -- 基本用法：在右上角显示一个 5 行 30 列的面板
  SetPanel("info", -30, 0, 30, 5, "第1行\n第2行\n第3行")

  -- 带颜色的面板内容
  SetPanel("combat", -40, 0, 40, 3,
    "\x1b[31mHP: 500/1000\x1b[0m\n" ..
    "\x1b[32mMP: 800/800\x1b[0m\n" ..
    "\x1b[33mEXP: 1.2M\x1b[0m"
  )

  -- 更新已有面板（同名覆盖）
  SetPanel("info", -30, 0, 30, 5, "新内容")

  -- 左上角面板
  SetPanel("debug", 0, 0, 40, 8, debug_info_text)
  ```

---

## RemovePanel(name)

移除浮动面板。面板移除后，底层被遮盖的输出文本会自然恢复显示。

- **参数**: `name` (string) - 要移除的面板标识符
- **返回值**: 无
- **示例**:

  ```lua
  RemovePanel("info")    -- 移除名为 "info" 的面板
  RemovePanel("combat")  -- 移除名为 "combat" 的面板
  ```

---

## RegisterPanelHandler(panel_name, callback)

注册面板按钮点击回调。当用户点击面板中定义的按钮区域时，客户端通过此注册表查找对应面板的回调并调用。

- **参数**:
  - `panel_name` (string) - 面板名称（与 `SetPanel` 的 `name` 参数一致）
  - `callback` (function) - 点击回调函数，签名为 `function(panel_name, action)`
    - `panel_name` (string) - 被点击的面板名称
    - `action` (string) - 被点击按钮的 `action` 标识（在 `SetPanel` 的 `buttonDefs` 中定义）
- **返回值**: 无
- **设计意图**:
  - 客户端**不硬编码**脚本侧函数名，而是通过注册表查找回调
  - 与 `AddTrigger`/`AddAlias`/`AddTimer` 的注册模式一致，实现客户端与脚本解耦
  - 脚本可自由重构回调函数的命名空间（如 `common.on_panel_click`），只需更新注册调用即可
- **注册时机**: 在脚本加载阶段调用一次即可，无需在每次 `SetPanel` 时重复注册。重复注册同一 `panel_name` 会覆盖之前的回调。
- **示例**:

  ```lua
  -- 定义回调函数
  function common.on_panel_click(panel_name, action)
    if panel_name == "stat" then
      if action == "go" then
        start_workflow()
      elseif action == "stop" then
        stop_workflow()
      end
    end
  end

  -- 注册回调（脚本加载时执行一次）
  RegisterPanelHandler("stat", common.on_panel_click)

  -- 创建带按钮的面板
  SetPanel("stat", -70, 0, 70, 10, stat_text, {
    { row = 9, start_col = 5, end_col = 12, action = "go" },
    { row = 9, start_col = 15, end_col = 22, action = "stop" },
  })
  ```

- **按钮定义**: `SetPanel` 的第 7 个参数 `buttonDefs` 是一个表数组，每个元素包含：
  - `row` (number) - 按钮所在行（0-indexed，相对于面板顶部）
  - `start_col` (number) - 按钮起始列（0-indexed，相对于面板左边）
  - `end_col` (number) - 按钮结束列
  - `action` (string) - 按钮标识，点击时传递给回调函数的 `action` 参数
- **未注册回调时的行为**: 如果用户点击了面板按钮但该面板未通过 `RegisterPanelHandler` 注册回调，客户端会记录一条错误日志（`[Lua] 面板 'xxx' 未注册点击回调`），不会 panic 或中断。

---

## 兼容性说明

- `SetPanel` / `RemovePanel` / `RegisterPanelHandler` 是 RustLuaMud 扩展 API，**不在标准 MUSHclient 中**
- 在不支持此 API 的客户端上调用会报错（`attempt to call global 'SetPanel' (a nil value)`）
- 建议使用前检查 API 是否存在：

  ```lua
  if SetPanel then
    SetPanel("stat", -70, 0, 70, 10, stat_text)
  else
    -- 降级：用 print 输出到主输出区
    print(stat_text)
  end
  ```

---

## 多面板管理

- 可同时显示多个面板，按创建顺序绘制（后创建的覆盖先创建的）
- 面板间不应重叠，否则会产生视觉混乱
- 每个面板有独立的 name，互不影响
- `SetPanel` 更新某个面板时不会影响其他面板

```lua
-- 同时显示统计面板和调试面板
SetPanel("stat", -70, 0, 70, 10, stat_text)     -- 右上角
SetPanel("debug", 0, 0, 40, 5, debug_text)       -- 左上角

-- 仅更新统计面板
SetPanel("stat", -70, 0, 70, 10, new_stat_text)

-- 移除调试面板（统计面板不受影响）
RemovePanel("debug")
```

---

## 性能说明

- **无面板时零开销**：`draw_panels()` 在面板列表为空时直接返回
- **面板内容更新频率**：面板内容仅在调用 `SetPanel` 时更新，建议在定时器中刷新（如每 10 秒），无需每帧更新
- **渲染开销**：面板存在时，每次输出区重绘后会重绘面板（因为 `draw_output_area` 的 `Clear + Print` 会覆盖面板区域）。单个面板的渲染开销约为 `height` 次 `MoveTo + Print`，在 10 行面板下可忽略不计
