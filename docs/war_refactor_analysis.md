# War 重构分析报告

> 分析日期：2026-06-22（最后更新）  
> 文件：`war_members_data.lua` (76行) + `war_members.lua` (256行) + `war_refactor.lua` (1313行)  
> 对比基准：原始 `war.lua` (~1130行)

---

## 一、总体评价

### 1.1 重构方向正确

将原来的 "大表 + 五级信任表 (A/B/C/D/E) + 硬编码 IP 冲突检查" 架构重构为 **数据与逻辑分离** 的模块化架构，方向完全正确。

| 维度 | 旧版 | 新版 |
|------|------|------|
| 成员数据 | `war_trust_a/b/c/d/e` 大表（函数内嵌，重复定义） | `WarMember.qt/ln` 模块（独立文件） |
| 站点配置 | if/elseif 硬编码，逻辑分散 | `get_current_site_info()` + `configure_war_settings()` 结构化配置 |
| 32位溢出 | 无处理 | `add32()` + `multi10_with_overflow()` 模拟溢出 |
| 队伍组建 | 优先级排队+IP冲突的在线算法 | 三池锚点制（P1+P2+P5）离线择优算法 |
| 数据管理 | 无 | 增删改查、CSV 导入导出、热重载 |

### 1.2 当前状态：新算法接入流程

新算法接入采用**职责分离**设计：

```
公告牌 trigger → form_war_team() → create_optimal_team()  ✅ 新算法
                                    ↓
                             设置 war.teamwith / war.realteam
                                    ↓
                              warteam() → teamwith 指令发送到服务器

timer 触发 → alias.start_teamwith() → war_start.timer() → run("war")  ✅ 维持旧逻辑
```

- `form_war_team()`：调用新算法计算最优队伍，打印结果和历史记录，设置 `war.teamwith`/`war.realteam`
- `warteam()`：读取 `war.realteam` 发送 `teamwith` 指令（职责分离，避免 print flood）
- timer 路径：已确认维持 `run("war")` 旧逻辑（见 N8）

---

## 二、缺陷清单

### 🔴 严重（阻塞级）

#### B1. timer 组队路径仍走 `run("war")` — 已确认维持

根据职责分离设计，`form_war_team()` 负责新算法计算 + 设置 `war.teamwith`/`war.realteam`，`warteam()` 发送 `teamwith` 指令。timer 路径保留 `run("war")` 旧逻辑，详见 N8。

---

#### B2. `haveyd()` 函数不存在 — ✅ 已修复（历史记录）

`haveyd()` 已替换为 `mj_need_yudi()`，当前代码中无此调用。

---

#### B3. `random_team_command()` 未定义 — ✅ 已解决（历史记录）

旧版随机碰运气组队策略草稿。已替换为 `form_war_team()` → 三池锚点制。

---

#### N1. 玩家自身经验值未计入队伍计算 — ✅ 已修复

**位置**：`create_optimal_team()` → `try_build()` 内部  
**问题**：`try_build()` 不包含玩家自身 exp，但服务器校验队伍时包含。  
**修复**：`try_build()` 始终插入 `my_member` 作为首个成员，`team_str` 跳过自身 ID。

---

#### N2. `war.teamwith` 和 `war.realteam` 未被新代码赋值 — ✅ 已修复

**位置**：`form_war_team()` 第 560 行  
**问题**：`form_war_team()` 成功后不更新 `war.teamwith`/`war.realteam`，导致依赖这两者的 trigger 全部失效。  
**修复**：成功后拆分 `team_str` 为表并赋值。

---

#### N3. `reload_war_member()` 文件名拼写错误 — ✅ 已修复

**位置**：`reload_war_members()` 第 108 行  
**问题**：`loadmod("war_member.lua")` 缺 s，热重载完全失效。  
**修复**：改为 `loadmod("war_members.lua")`，函数名同步改为 `reload_war_members()`。

---

#### N4. `tonumber(war.waring>0)` 恒为 nil — ✅ 已修复

**位置**：原第 1009 行（"城外太危险" trigger）  
**问题**：Lua 5.1 中 `tonumber(true)` 返回 nil，条件永远 false，清理逻辑不执行。  
**修复**：改为 `if war.waring>0 then`。

---

### 🟡 中等（功能/逻辑）

#### M1. `configure_war_settings()` 调用时机问题

**位置**：`war_refactor.lua` 第 94 行（顶层调用）  
**问题**：脚本加载时立即执行，依赖全局变量 `WarMember`。如果 `war_members.lua` 加载晚于 `war_refactor.lua`，会因 `WarMember` 为 nil 而失败。

**修复**：在 `michen_xkx.lua` 的 `loadlua_list` 中确保 `"war_members.lua"` 在 `"war_refactor.lua"` 之前加载。

> **用户意见**：仔细安排加载顺序确保可用，不包 pcall。

---

#### M2. `create_optimal_team()` 组队策略 — ✅ 已重构

~~原问题：只使用贪心排序取前缀，不尝试组合。~~  
已重构为三池锚点制 + 双策略 P5 组合尝试。

---

#### M3. `calculate_team_exp()` 中 `actual_min10` 从未赋值 — 已合并至 N15

此问题已在复盘修复中一并处理，见 N15。

---

#### M4. 旧的 `war_trust_*` 数据结构与新的 `WarMember` 数据结构映射

**映射关系（按设计意图）**：
| 旧版 | 新版 | 说明 |
|------|------|------|
| 信任等级 A | P1 | 自己的重要 ID |
| 信任等级 B/C/D | P2 | 朋友的重要 ID（B/C/D 合并简化） |
| 信任等级 E | P5 | 小号凑人头 |

**影响**：
- B/C/D 三级合并为 P2 后，失去了旧版内部的替换优先级差异（旧版中 B > C > D）
- `memberIDs_a`（VVIP 检查逻辑）已转换为 P1 优先级判断
- `war.have_vip`/`war.have_vvip` 变量尚未在新的组队算法中维护（仍为旧流程所用）

---

#### M5. `is_team_valid()` — ✅ 已移除

**位置**：原 `war_refactor.lua` 第 383 行  
**问题**：定义了但从未被调用。`create_optimal_team()` 内部直接调用 `team_meets_conditions()`。  
**处理**：已确认死代码，直接移除。

---

#### M6. `table.size()` / `select_random_members()` 是死代码

**位置**：`war_refactor.lua` 第 314 / 321 行  
**问题**：定义了但未被调用。`war_members.lua` 中已用内联循环计数。

---

#### N5. `string.trim()` 不存在 — ✅ 已修复

**位置**：`war_members.lua` `import_csv()`  
**问题**：Lua 标准库无 `string.trim()`，调用会崩溃。  
**修复**：在 `war_members.lua` 顶部添加 `string.trim` 定义。

---

#### N6. `try_build` 不尝试不同 P5 组合 — ✅ 已修复

**位置**：`try_build()` 内部  
**问题**：对每个队伍规模只尝试一种 P5 组合。  
**修复**：实现双策略（low_exp 升序 + high_exp 降序），覆盖不同 exp 分布。

---

#### N7. P5 池顺序不确定 — ✅ 已修复

**位置**：`create_optimal_team()` 分池后  
**问题**：`pairs` 迭代顺序不确定，导致不同次运行组队结果不同。  
**修复**：分池后对 `p5_pool` 按 exp 升序排序。

---

#### N8. `war_start.timer()` 仍调用旧逻辑 — 已确认维持

**位置**：`war_refactor.lua` 第 1079 行  
**处理**：维持现状。`form_war_team()` 仅做结果显示和历史记录，不发送 `teamwith` 命令。  
由 `warteam()` 处理指令发送，避免 print flood。

---

#### N9. 服务器平均值计算不一致 — 已确认无需修改

**处理**：已确认服务器 LPC 代码 `max_exp > (avg_exp = total/total_players)*10` 与模拟一致。

---

#### N10. `\\Z` 正则兼容性 — 已确认无需修改

**处理**：Rust `regex` crate 到 PCRE 的移植适配已完成且功能正常。

---

#### N11. 魔术数字 `9` 和 `4` 硬编码 — 已确认维持

**处理**：考虑到游戏长期未更新，维持硬编码。

---

#### N12. `WarMember.last_updated` 日期过时 — ✅ 已修复

更新为 `"2026-06-22"`。

---

#### N13. `find_member()` Lua 模式注入风险 — ✅ 已修复

**修复**：`string.find(id, eng_id, 1, true)` 使用纯文本匹配。

---

### 🔵 轻微（代码质量）

#### L1. 重复触发流代码

`war_refactor.lua` 复制了大量原 `war.lua` 的触发处理代码（war_pre / war_start 部分），两者高度重复。

**建议**：保持现状，待实测阶段替换 `war.lua` 后自然消除。

---

#### L2. `alias.start_teamwith()` 定义在外部文件

定义在 `michen_alias.lua:3781`。`alias.*` 本就是跨文件共享的全局函数命名空间，属于正常设计模式，无需修改。

**状态**：✅ 已确认维持

---

#### L3. `\\Z` 正则标志位兼容性

**位置**：第 841 行 `"...蒙古大军攻陷了南阳！\\Z"` + `maketri(..., 2)`  
**状态**：已确认 Rust 端适配完成。✅ 已关闭

---

#### L4. 魔术数字 `2147483647` 多处出现 — ✅ 已修复

`MAX_INT32` 已定义为常量，但 `is_team_valid()` 中仍有硬编码。已统一替换为 `MAX_INT32`。

---

#### L5. `findstring` 多参数调用

```lua
if findstring(c,"蒙古兵","十夫长","百夫长","千夫长") then
```
已在 [michen_system.lua:208](file:///home/baiyf/RustLuaMud/scripts/class-utf8/michen_system.lua) 确认 `findstring(str, ...)` 支持变长参数。✅ 已关闭

---

#### N14. `war.team_history` 无限增长 — 已添加注释

**修复**：添加注释 `-- N14: 代码调试结束后可以考虑移除历史记录功能`。

---

#### N15. `actual_min10` 变量未赋值 — ✅ 已修复

**位置**：`calculate_team_exp()` 第 268 行  
**修复方案（方案B）**：
1. 在 `calculate_team_exp()` 中正确计算 `actual_min10`（最小 exp×10，经溢出处理）
2. `team_meets_conditions()` 条件3直接引用 `result.actual_min10`，消除重复计算

---

## 三、32 位溢出算法分析

### 3.1 核心函数

```lua
function add32(a, b)
    local result = (a + b) % MODULO          -- MODULO = 2^32
    if result > MAX_INT32 then                -- MAX_INT32 = 2^31-1
        result = result - MODULO
    elseif result < MIN_INT32 then            -- MIN_INT32 = -2^31
        result = result + MODULO
    end
    return result
end
```

**评估**：逻辑正确，模拟了 32 位有符号整数加法溢出。

### 3.2 `multi10_with_overflow()`

```lua
function multi10_with_overflow(value)
    local product = value * 10
    local result = product % MODULO
    if result > MAX_INT32 then result = result - MODULO
    elseif result < MIN_INT32 then result = result + MODULO end
    local overflowed = (result ~= product)
    return result, overflowed
end
```

**评估**：逻辑正确。MushClient 使用 Lua 5.1，double 有 53 位精度，EXP 值接近 2^31 时 `value * 10` 仍精确，**此函数安全**。

### 3.3 未测试问题

溢出算法**仅在理论上正确**，缺少以下验证：
- 从未在 MUD 服务器上实际验证过 32 位溢出后的行为是否与模拟一致
- 没有单元测试覆盖溢出边界值（如 EXP = 2000000000, 2147483647 等）
- 没有测试不同组合下 `team_meets_conditions` 三个条件的实际通过率

---

## 四、与旧系统的兼容性风险

### 4.1 全局变量兼容

| 变量 | 旧 war.lua | 新 war_refactor.lua | 状态 |
|------|-----------|---------------------|------|
| `war_trust_a/b/c/d/e` | 定义（函数 set_war_trust_*） | 未定义 | ❌ 丢失 |
| `memberIDs_a/b/c/d/e` | 定义（哈希表） | 未定义 | ❌ 丢失 |
| `memberToIPMap` | 定义 | 未定义 | ❌ 丢失 |
| `war.trust_tables` | 依赖 | 未使用 | ⚠️ 可能影响 else 流程 |
| `war.have_vip` | 维护 | 未维护 | ⚠️ 空值 |
| `war.have_vvip` | 维护 | 未维护 | ⚠️ 空值 |
| `CURRENT_PRESENT_MEMBERS` | 无 | 新增 | ✅ 新版使用 |
| `MEMBER_DATA` | 无 | 新增 | ✅ 新版使用 |
| `npcAssassin*` | 定义 | 定义 | ✅ 一致 |

### 4.2 触发器冲突

新旧文件都注册了相同名称的 trigger class（`war_pre_doth`, `war_start_doth`），**绝对不能同时加载**，否则触发器会重复注册。

> **用户确认**：测试阶段直接替换原有 `war.lua` 文件，不会同时加载。

---

## 五、代码质量观察

### 5.1 命名风格混用

```lua
calculate_team_exp()      -- snake_case
shouldActivateTeamwith()  -- camelCase（历史函数）
deepCopy()                -- camelCase
WarMember.find_member()   -- 模块方法
```

### 5.2 全局变量污染

`CURRENT_PRESENT_MEMBERS`、`MEMBER_DATA`、`maketri_num` 等直接放在全局作用域。

### 5.3 注释风格不统一

有 `--@shana` 署名，`--核心部分` 中文注释，以及 `看不懂千万别动` 警告。

---

## 六、优先修复顺序建议

| 优先级 | 编号 | 问题 | 类型 | 状态 |
|--------|------|------|------|------|
| P1 | M1 | `configure_war_settings` 调用时机 | 架构 | ⏳ 待安排 |
| — | M5 | `is_team_valid` 模块化调用 | 架构 | ✅ 已恢复 |
| P2 | M6 | `table.size` / `select_random_members` 死代码 | 清理 | 🔧 可移除 |
| P3 | L1 | 重复触发流代码 | 代码质量 | 📝 过渡期正常现象 |
| — | L2 | `alias.start_teamwith()` 外部定义 | 代码质量 | ✅ 已确认维持 |
| — | L3 | `\Z` 正则兼容性 | 代码质量 | ✅ 已关闭 |
| — | L4 | 魔术数字 `2147483647` | 代码质量 | ✅ 已修复 |
| — | L5 | `findstring` 多参数 | 代码质量 | ✅ 已关闭 |
| — | B1 | timer 路径维持旧逻辑（已确认） | 架构 | ✅ 已确认 |
| — | M4 | 旧数据结构映射为 P1/P2/P5（已确认设计） | 兼容 | ✅ 已确认 |
| — | N8 | timer 维持旧逻辑（已确认） | 架构 | ✅ 已确认 |
| — | N9/N10 | 服务器确认、正则适配（已确认） | — | ✅ 已确认 |
| — | N11 | 硬编码维持（已确认） | — | ✅ 已确认 |

### ✅ 已修复汇总

| 编号 | 问题 | 修复日期 |
|------|------|----------|
| B2 | `haveyd()` 不存在 | 06-21 |
| B3 | `random_team_command` 未定义 | 06-21 |
| M2 | 贪心策略不尝试组合（三池锚点制） | 06-21 |
| M3/N15 | `actual_min10` 未赋值 | 06-22 |
| N1 | 玩家自身 exp 未计入 | 06-22 |
| N2 | `war.teamwith`/`war.realteam` 未赋值 | 06-22 |
| N3 | `reload_war_member()` 文件名缺 s | 06-22 |
| N4 | `tonumber(war.waring>0)` 恒 nil | 06-22 |
| N5 | `string.trim()` 不存在 | 06-22 |
| N6 | P5 组合单一 | 06-22 |
| N7 | P5 池顺序不确定 | 06-22 |
| N12 | 日期过时 | 06-22 |
| N13 | Lua 模式注入风险 | 06-22 |
| N14 | 注释标记 | 06-22 |
| M5 | `is_team_valid` 模块化调用 | 06-22 |
| L4 | 魔术数字 `2147483647` 硬编码 | 06-22 |
| — | war_members 数据/逻辑文件拆分 | 06-22 |
| — | EXP 持久化（mark_dirty/flush_exp/dirty_count） | 06-22 |
| — | have_vip/have_vvip 维护（P1+1两者，P2+1仅have_vip） | 06-22 |
| — | 旧文件名引用修正（war_member.lua → war_members_data.lua） | 06-22 |
| — | 移除死代码 table.size() / select_random_members() | 06-22 |
| — | 加载顺序文档化（war_members.lua 必须先于 war_refactor.lua） | 06-22 |

---

### 数据文件拆分 — ✅ 已完成

**背景**：运行时 EXP 反写需要写成员数据文件，但函数代码混在一起，反写可能误覆盖。

**拆分结果**：
- `war_members_data.lua`（76行）：纯数据文件，qt/ln 成员表，运行时反写只动此文件
- `war_members.lua`（256行）：仅保留函数代码（含 EXP 持久化函数），通过 `dofile(luapath .. "class\\war_members_data.lua")` 加载数据

---

## 七、EXP 持久化方案 — ✅ 已实现

### 需求

抓取 `war_members_data.lua` 中已有 ID 的 EXP，忽略不在清单中的玩家。将抓取到的缺失 EXP 反写到 `war_members_data.lua` 文件中持久保存。

### 设计原则

1. **只动纯数据文件**：反写只写 `war_members_data.lua`，不碰 `war_members.lua` 和 `war_refactor.lua`
2. **文本级替换**：不反序列化/重新序列化 Lua 表，将文件当纯文本逐行处理，只替换 `exp =` 后的数值
3. **脏表批处理**：内存累积变更，在关键时机统一刷盘，避免频繁 IO
4. **原子写入**：写 `.tmp` → `os.rename` 替换，崩溃不损坏原文件

### 已实现函数（war_members.lua）

| 函数 | 作用 | 位置 |
|------|------|------|
| `WarMember.mark_dirty(eng_id, exp)` | 标记某个 ID 的 EXP 已变更（存内存脏表） | war_members.lua:191 |
| `WarMember.flush_exp()` | 将脏表所有变更一次性写入文件，返回写入条数 | war_members.lua:203 |
| `WarMember.dirty_count()` | 返回当前脏表条目数（用于调试） | war_members.lua:197 |

### 调用集成 — ✅ 已完成

**采集端**（`war_refactor.lua` EXP trigger，第614行）：
```lua
if MEMBER_DATA[eng_id] then
    MEMBER_DATA[eng_id].exp = tonumber(exp_value)
    -- ...
    WarMember.mark_dirty(eng_id, tonumber(exp_value))  -- 标记脏
end
```

**刷盘时机**（`war_refactor.lua` `form_war_team()` 成功后，第577行）：
```lua
if success then
    -- ...
    local n = WarMember.flush_exp()
    if n > 0 then print("已保存 "..n.." 名玩家 EXP") end
end
```

### flush_exp 实现细节

1. 读取 `war_members_data.lua` 全文
2. 对脏表中每个 `eng_id`，用 `string.gsub` 匹配 `eng_id = {ip = ..., exp = 旧值, ...}` 行
3. 在匹配行内替换 `exp = 数字` 为 `exp = 新值`
4. 写入 `.tmp` 临时文件 → `os.remove` 旧文件 → `os.rename` 临时文件
5. 清空脏表

### 文件关系

```
war_members_data.lua ←── 文本替换 ── WarMember.flush_exp() ←── 脏表 ── mark_dirty()
                                                                         ↑
                                                              EXP trigger 捕获
```

---

## 八、附录：文件关系图

```
war_members_data.lua (纯数据, 76行, 可安全反写)
  └── return { qt = {...}, ln = {...}, version, last_updated }

war_members.lua (数据管理函数, 256行)
  ├── dofile → war_members_data.lua (加载纯数据)
  ├── string.trim (工具函数)
  ├── find_member() / stats() / export_csv() / import_csv()
  ├── add_member() / update_member()
  ├── mark_dirty() / flush_exp() / dirty_count() (EXP 持久化)
  └── return WarMember

war_refactor.lua (主逻辑, 1291行, 试图替代 war.lua)
  ├── [新] 站点配置层
  │   ├── get_current_site_info()
  │   └── configure_war_settings()
  ├── [新] 32位溢出算法
  │   ├── add32()
  │   ├── multi10_with_overflow()
  │   └── calculate_team_exp()
  ├── [新] 队伍验证
  │   ├── team_meets_conditions()
  │   └── is_team_valid()
  ├── [新] 组队逻辑 (三池锚点制)
  │   ├── create_optimal_team()  ← 含 N1/N6/N7 修复 + have_vip/vvip 维护
  │   └── form_war_team()        ← 含 N2/N8/N14 修复 + flush_exp 刷盘
  ├── [新] 热重载
  │   └── reload_war_members()   ← 含 N3 修复
  ├── [复制] war_pre 触发 (60-70% 与旧一致)
  ├── [复制] war_start 触发 (90% 与旧一致)
  └── [保留] war_start/restart timer (仅发送 teamwith 指令，不调用新算法)

外部依赖
  ├── michen_alias.lua:3781  alias.start_teamwith()
  │   └── 仅 openclass("war_start_timer")，未调用新算法
  └── always.lua:10          mj_need_yudi() ← 确保已替换 haveyd()
```

### 加载顺序（部署时修改）

当前 `scripts/private/class/michen_xkx.lua` 中的 `loadlua_list` 加载的是旧 `war.lua`。
部署新算法时，需将 `"war.lua"` 替换为以下**两条**（顺序不能颠倒）：

```lua
-- 旧（当前）：
"war.lua",

-- 新（部署时）：
"war_members.lua",      -- 必须先加载（提供 WarMember 全局对象和数据）
"war_refactor.lua",     -- 后加载（依赖 WarMember，顶层调用 configure_war_settings()）
```

**加载顺序约束**：`war_members.lua` 必须在 `war_refactor.lua` 之前加载，因为：
1. `war_refactor.lua` 顶层立即调用 `configure_war_settings()`，该函数依赖 `WarMember` 全局对象
2. `war_members.lua` 通过 `dofile` 加载 `war_members_data.lua`，提供 `WarMember.qt`/`WarMember.ln` 数据
3. 若顺序颠倒，`configure_war_settings()` 会因 `WarMember` 为 nil 而崩溃
