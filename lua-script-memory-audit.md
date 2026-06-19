# Lua 脚本内存增长问题评估报告

> 评估日期：2026-06-17
> 评估范围：`scripts/class-utf8/` 目录下所有脚本
> 目标：识别可能导致长时间挂机内存增长的低质量代码模式

---

## 一、Table 无限增长 / 未清理（高风险）

最可能导致内存持续增长的原因。某些全局 table 在特定条件下只增不减，或重置条件不严密。

| # | 文件 | 位置 | 问题描述 | 改进难度 | 风险 | 收益 |
|---|------|------|---------|---------|------|------|
| 1.1 | `always.lua` | L696 | `table.insert(always.bei, c)` — 虽有 `always.bei={}` 的重置（L689），但依赖特定文本触发。如果触发器未匹配到结束文本，列表会一直增长。 | 低 | 中 | 高 |
| 1.2 | `always.lua` | L645 | `table.insert(skillslist_need_up, w[3])` — 在技能列表采集时插入。如果 `skillslist_get` 状态切换异常，可能导致重复插入。 | 低 | 中 | 高 |
| 1.3 | `michen_mp_gm.lua` | L331 | `table.insert(findpath, {})` — 在找 NPC 过程中不断向 `findpath` 添加新房间记录。如果任务中断（如被杀、掉线），`findpath` 可能残留大量数据。 | 低 | 中 | 高 |
| 1.4 | `gps_lib.lua` | L84-120 | `Room_table`, `Room_index`, `Room_index_nodir` — 在 `xkxGPS.init()` 中构建。如果 `init` 被多次调用（如脚本重载），旧数据虽然会被覆盖，但如果没有显式清理，会造成内存峰值和 GC 压力。 | 低 | 低 | 中 |
| 1.5 | `michen_mp_mj.lua` | 多处 | `mj.killlist` — 击杀列表在任务结束后是否有完整清理逻辑，需确认。 | 低 | 中 | 高 |

### 修复建议

**1.1 `always.bei`**：在 `always_skills.dosomething8` 开头强制重置 `always.bei = {}`，或在确认采集完成后立即清理。

**1.2 `skillslist_need_up`**：在技能采集流程结束时显式置空 `skillslist_need_up = {}`。

**1.3 `findpath`**：在任务结束、中断或失败的所有出口，确保 `findpath = {}`。

**1.4 `gps_lib`**：在 `xkxGPS.init()` 开头显式置空旧表：
```lua
Room_table, xkxGPS.Room_index, xkxGPS.Room_index_nodir = {}, {}, {}
```

**1.5 `mj.killlist`**：确认任务结束后有 `mj.killlist = {}` 清理。

---

## 二、文件句柄泄漏 / 低效 IO（高风险）

频繁打开/关闭文件，或在异常路径下未关闭文件句柄。

| # | 文件 | 位置 | 问题描述 | 改进难度 | 风险 | 收益 |
|---|------|------|---------|---------|------|------|
| 2.1 | `dummy.lua` | L72, L248 | 每次触发谣言/聊天时都 `io.open` 然后 `f:close()`。高频触发下效率极低；如果脚本在 `write` 和 `close` 之间崩溃，句柄泄漏。 | 中 | 低 | 高 |
| 2.2 | `michen_system.lua` | L452, L471 | `addlog` 和 `addlog2` 使用 `pcall` 包裹 `write`，但如果 `io.open` 成功而 `pcall` 前发生错误，句柄可能未关闭。 | 低 | 低 | 中 |

### 修复建议

**2.1 `dummy.lua`**：使用 `pcall` 确保 `close` 被调用：
```lua
local f = io.open(path, "a+")
if f then
    pcall(function() f:write(msg .. "\n") end)
    f:close()
end
```

**2.2 `michen_system.lua`**：确保 `f:close()` 在所有路径下执行，可使用 `pcall` 包裹整个 IO 操作。

---

## 三、字符串拼接与临时对象（中风险）

大量使用 `..` 进行字符串拼接，特别是在高频触发器中，会产生大量临时字符串，增加 GC 压力。

| # | 文件 | 位置 | 问题描述 | 改进难度 | 风险 | 收益 |
|---|------|------|---------|---------|------|------|
| 3.1 | 全局 | 多处 | `print("..." .. var .. "...")` 和 `run("cmd " .. var)` — 每秒多次的触发器（如 `always.lua`, `gps.lua`）会生成大量短命对象。 | 高 | 低 | 中 |
| 3.2 | `michen_alias.lua` | L92-133 | 频繁调用 `os.time()` 并进行数学运算和字符串拼接。 | 中 | 低 | 低 |

### 修复建议

**3.1** 对于长字符串拼接，使用 `table.concat`：
```lua
-- 差
local s = ""
for i, v in ipairs(t) do s = s .. v end
-- 好
local s = table.concat(t)
```

**3.2** 在循环或高频触发中，缓存 `os.time()` 的结果。

---

## 四、全局变量污染 / 状态管理混乱（中风险）

大量使用全局变量共享状态，导致 `_G` 表膨胀，且难以追踪变量生命周期。

| # | 文件 | 位置 | 问题描述 | 改进难度 | 风险 | 收益 |
|---|------|------|---------|---------|------|------|
| 4.1 | 全局 | 多处 | `skillslist`, `master_skillslist`, `always`, `ybjob`, `mj`, `findpath` 等均为全局表。`_G` 持有所有引用，阻止 GC；状态重置不彻底时，旧表可能残留。 | 极高 | 极高 | 中 |

### 修复建议

**短期**：确保在任务结束/重置时，显式置空大表：
```lua
skillslist, master_skillslist = {}, {}
```

**长期**：重构为模块（`local M = {}`），但工作量巨大，不建议在现有架构下强行修改。

---

## 五、Timer/Trigger 泄漏（低风险）

`AddTimer` 和 `AddTrigger` 使用后未清理。

| # | 文件 | 位置 | 问题描述 | 改进难度 | 风险 | 收益 |
|---|------|------|---------|---------|------|------|
| 5.1 | 全局 | 81处 | `AddTimer` 使用较多，`DeleteTimer` 也较多，但 `DeleteTrigger` 相对较少。如果存在动态添加的临时触发器未删除，会累积。 | 中 | 中 | 低 |

### 修复建议

检查所有 `AddTrigger` 调用，确认是否有对应的 `DeleteTrigger`。对于一次性触发器，使用 `FLAG_ONE_SHOT` 标志或手动删除。

---

## 修复优先级总览

### 第一优先级（立即修复）

| 编号 | 文件 | 操作 |
|------|------|------|
| 1.1 | `always.lua` | 修复 `always.bei` 清理逻辑 |
| 1.2 | `always.lua` | 修复 `skillslist_need_up` 清理逻辑 |
| 1.3 | `michen_mp_gm.lua` | 确保 `findpath` 在所有出口被清理 |
| 2.1 | `dummy.lua` | 优化文件 IO，防止句柄泄漏 |

### 第二优先级（近期优化）

| 编号 | 文件 | 操作 |
|------|------|------|
| 1.4 | `gps_lib.lua` | 在 `init` 开头显式清理旧表 |
| 1.5 | `michen_mp_mj.lua` | 确认 `mj.killlist` 清理逻辑 |
| 2.2 | `michen_system.lua` | 完善 `addlog` 的异常处理 |

### 第三优先级（长期重构）

| 编号 | 文件 | 操作 |
|------|------|------|
| 3.1-3.2 | 全局 | 字符串拼接优化 |
| 4.1 | 全局 | 全局变量模块化 |
| 5.1 | 全局 | Timer/Trigger 泄漏排查 |

---

## 修复进度跟踪

### 第一优先级（立即修复）

- [x] 1.1 `always.bei` 清理逻辑
- [x] 1.2 `skillslist_need_up` 清理逻辑
- [x] 1.3 `findpath` 所有出口清理
- [x] 1.4 `gps_lib` init 显式清理
- [x] 1.5 `mj.killlist` 清理确认（确认无需修改）
- [x] 2.1 `dummy.lua` 文件 IO 优化
- [x] 2.2 `michen_system.lua` addlog 异常处理

### 第二优先级（近期优化）

- [x] R1 `war.lua` 修复 `war.teamwith` 遍历中 `k` 未定义 bug，并统一 5 处遍历为正序 → 倒序
- [x] R5 `michen_yb.lua` 在 `ybfight=0` 时同步清理 `ybjob.killlist`
- [x] R6 `kill.lua` 为 `stat.qiankun` 添加上限检查（防止无限增长）→ **无需修改**，生命周期完整
- [x] R7 `always.lua` 为 `stat.cross` 添加上限检查（防止无限增长）→ **无需修改**，生命周期完整

### 第三优先级（长期重构）

- [ ] 3.1 高频触发器字符串拼接优化
- [ ] 3.2 `os.time()` 缓存优化
- [ ] 4.1 全局变量模块化（长期）
- [ ] 5.1 Timer/Trigger 泄漏排查
