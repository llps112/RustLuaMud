# 节流渲染模式下切换前台导致服务器行重复插入 —— 修复方案

> 状态：**方案待评审，未改动任何代码**。
> 关联现象：玩家反馈非实时（节流）渲染时，屏幕偶尔把"一小段时间前的服务器行"重新插回渲染队列，时序看着被打乱。

## 1. 背景与判据

### 1.1 两种渲染模式

- 实时模式（`realtime = true` 或 `render_interval = 0`）：服务器每行数据到达即 `append_output` 落屏。
- 节流模式（默认，`render_interval = 1000ms`）：服务器行先缓冲，由 `start_render_tick_timer` 每 1000ms 触发一次 `handle_render_tick` 批量刷新（`src/app/session.rs:678-746`）。

命令本身是实时发送执行的（见前次分析），节流只影响**下行文本的显示时机**。

### 1.2 关键数据结构：服务器行存在两份独立缓冲

节流模式下，每条前台服务器行会**同时写入两处**（`src/app/events.rs:273-289`）：

| 缓冲 | 写入时机 | 用途 |
|---|---|---|
| `session.output_lines` | **到达即写**（L274-276） | 该连接的回看存档；切换前台时用它整体重建屏幕 |
| `session.pending_data` | 到达即写（L285），**1000ms tick 才刷到终端并清空**（`src/app/session.rs:725-744`） | 待渲染队列 |

两者对"服务器行"的内容**重叠**：`pending_data` 里还没刷新的那批行，其实**已经在 `output_lines` 里了**。

## 2. 根因

`switch_foreground`（`src/app/events.rs:544-582`）在切换前台连接时连续做了两件事，把上述两份重叠缓冲**叠加渲染**：

```text
① replace_output(session.output_lines)   // L554 整表赋值：屏幕 = 存档全部行（含未刷新的最近行）
② 取 pending_data 并 append_output(...)  // L564-582："立即排空 pending，避免切换后延迟"
```

`replace_output` 是**整表替换**语义（`src/ui/terminal.rs:2015-2022`：`self.state.output_lines = lines.to_vec()`），它铺进去的存档**已经包含**了 `pending_data` 里那批"距上次 tick 不足 1000ms 的最近行"。紧接着第 ② 步又把同一批 `pending_data` 用 `append_output` 追加到队尾。

结果：屏幕末尾出现形如 `[..., L1, L2, L3, L2, L3]` 的重复——`L2/L3` 这些"一小段时间前的行"被重新插回渲染队列，看着像时序被打乱。

### 2.1 为什么精确匹配"偶尔 / 只服务器行 / 只非实时"

- **只非实时**：实时模式下 `output_lines` 与终端是同步 `append_output` 的，无此落差；`pending_data` 恒空，第 ② 步是 no-op。
- **只服务器行**：只有服务器行走"双缓冲（`output_lines` + `pending_data`）"这条链路。
- **偶尔**：只有切换瞬间 `pending_data` 恰好非空（即落在 1000ms 渲染间隔的尾窗内）才复现；若刚好在 tick 之后切换则 `pending` 为空，不重复。
- **触发点**：`Alt+0~9`、`Alt+←/→`、`/switch`、`/sw`、以及 `/profile load` 建连后自动切前台等所有 `switch_foreground` 调用。

## 3. 附带缺口（同一链路，建议一并处理）

`drain_lua_logs` 的节流分支只把 `[Lua]` 日志 push 进 `pending_data`，**未同步写入 `session.output_lines`**（`src/app/session.rs:782-786`）；而 `handle_render_tick` 刷新的又是终端缓冲、不回写存档。

后果：节流模式下 Lua 日志行**只存在于终端缓冲**，一旦切换前台，`replace_output(output_lines)` 取不到它们，切回时这些 `[Lua]` 行丢失。这与第 2 节是同一个"两份缓冲职责不清"的连锁问题。

> 注：本节为旁证缺陷，是否纳入本次修复由评审决定；若只做第 2 节的最小修复，此处可作为后续项。

## 4. 修复方案

### 4.1 主修：切换前台不再重复追加 pending

`switch_foreground` 中，`replace_output(output_lines)` 已经把服务器行（含未刷新的最近行）全部渲染。因此第 ② 步应改为**只丢弃 pending、不再 append**，避免二次追加；丢弃同时也防止后续周期性 tick 把同一批行刷第二遍。

修改位置：`src/app/events.rs:564-582`。

改前（示意）：
```text
取 pending_data → 非空则逐行拼 combined → append_output(combined)
```
改后（示意）：
```text
session.pending_data.clear();
session.render_dirty = false;   // pending 内容已在 output_lines 中呈现，直接丢弃
```

要点：
- 保留"切换后不留显示延迟"的原意——因为 `output_lines` 已领先包含这批行，`replace_output` 当场就显示了，无需再排空。
- 不改动 `handle_render_tick`（正常节流刷新路径本身无重复问题）。

### 4.2 辅修（若纳入）：节流分支的 Lua 日志同步入档

`drain_lua_logs` 节流分支在 push `pending_data` 的同时，也 `push` 进 `session.output_lines`（走既有的容量裁剪路径），使切换前台的 `replace_output` 能拿到 `[Lua]` 行。

需与 4.1 协调：一旦 `[Lua]` 行进入 `output_lines`，`handle_render_tick` 从 `pending_data` 刷新时**不能再次写入 `output_lines`**（当前 `handle_render_tick` 只 `append_output` 终端、不动 `output_lines`，与服务器行一致），否则终端与存档会各自累积。保持"存档=唯一真相，终端=渲染视图"的单向关系即可。

## 5. 影响面与风险

- 仅影响**节流模式下的前台切换显示**，不触碰网络发送、限速、触发器/别名执行时序。
- 4.1 为纯显示逻辑收敛，行为向后兼容（切换后画面少掉的是重复行）。
- 风险点：需确认所有进入 `pending_data` 的行都在 `output_lines` 里有对应副本（服务器行：是，见 L274-276；Lua 行：当前否，故 4.2 需要与 4.1 配套，否则 4.1 会让未刷新的 `[Lua]` 行在切换时丢失——但这类行本就只短暂显示，权衡后取"入档"更一致）。

## 6. 测试计划

在 `src/app/` 相应测试模块新增/补充：

1. **回归：切换前台不重复渲染最近行**
   - 构造 session 处于节流模式；喂入若干服务器行，使其中一部分仍滞留在 `pending_data`（未到渲染 tick）。
   - 触发 `switch_foreground`（先切走再切回，或直接对目标 session 调用）。
   - 断言：目标 session 的终端缓冲中，那批"最近行"**各只出现一次**（当前实现会出现两次）。

2. **切换前台后 pending 被正确清空**
   - 切换后断言 `session.pending_data.is_empty()` 且 `render_dirty == false`，确保后续 tick 不再二次追加。

3. **（若纳入 4.2）节流模式 Lua 日志切换后不丢失**
   - 节流模式下产生 `[Lua]` 日志，切换到别的 session 再切回。
   - 断言终端缓冲仍包含该 `[Lua]` 行（当前会丢）。

4. **实时模式不受影响**
   - `realtime = true` 时重复一遍用例 1，断言行为不变（pending 恒空，无重复）。

## 7. 验证与交付流程

1. 依方案改 `src/app/events.rs`（4.1），（如纳入）改 `src/app/session.rs`（4.2）。
2. 新增/补充上述单元测试。
3. 依次执行：`cargo fmt --all` → `cargo clippy -- -D warnings` → `cargo nextest run`，全部零告警零失败。
4. 人工挂机验证：多连接 + 节流 1000ms，频繁 `Alt+←/→` 来回切换，观察最近行不再重复插入；切回含 `[Lua]` 行的连接，验证 `[Lua]` 不丢。
5. 提交（仅主仓库 Rust 改动，触发 CI）。

## 8. 待评审确认项

- 4.2（Lua 日志入档）是否纳入本次修复，还是单独排期。
- "丢弃 pending"与"是否仍需把 `handle_render_tick` 语义收紧以防其它入口二次写入"的边界是否认可。
