# War 重构分析报告

> 分析日期：2026-06-21  
> 文件：`war_members.lua` (232行) + `war_refactor.lua` (1256行)  
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
| 队伍组建 | 优先级排队+IP冲突的在线算法 | 基于 EXP 计算的离线择优算法 |
| 数据管理 | 无 | 增删改查、CSV 导入导出、热重载 |

### 1.2 当前状态：将近完成，但核心新逻辑未接入触发流程

新写的队伍组建算法 (`calculate_team_exp` / `team_meets_conditions` / `create_optimal_team` / `form_war_team`) 全部被定义但**从未被任何 trigger 或 timer 调用**。所有的 trigger 和 timer 仍然调用旧流程（`alias.start_teamwith` → 旧 `war_start_timer` → `run("war")`）。

---

## 二、缺陷清单

### 🔴 严重 (阻塞级)

#### B1. 新的队伍组建算法完全未被集成

**文件**：`war_refactor.lua` 第 222-500 行（定义完整组队算法）  
**问题**：以下函数被定义但从未被调用：
- `calculate_team_exp()` — 队伍经验计算
- `team_meets_conditions()` — 三条件校检
- `is_team_valid()` — 队伍有效性检测
- `create_optimal_team()` — 最优队伍创建
- `form_war_team()` — 组队入口

**现状**：触发器仍然调用旧的 `alias.start_teamwith()`（定义在 `michen_alias.lua`），其内部只做了：
```lua
function alias.start_teamwith()
    if shouldActivateTeamwith() then
        openclass("war_start_timer")
        closeclass("war_pre_time")
    end
end
```
`war_start_timer` 的 callback 是 `run("war")` → 触发旧的基于 `war_trust_*` 表的组队逻辑。

**影响**：重构的核心价值（EXP 溢出匹配算法）完全无法生效。

**修复方向**：
1. `alias.start_teamwith()` 中需要调用 `form_war_team()` 而不是 `openclass("war_start_timer")`
2. 或者修改 `war_start.timer()` 来调用 `form_war_team()`

> **用户反馈 (06-21)**：已知，新算法打算先完善到差不多再合并到主脚本进行实际测试。此条保留为待办事项。

---

#### B2. `haveyd()` 函数不存在

**文件**：`war_refactor.lua` 第 979 行  
**代码**：
```lua
if haveyd() and war.waring==0 and war.late>15 then
```
**问题**：`haveyd()` 在整个脚本体系中**不存为函数**。原始 `war.lua` 使用的是 `mj_need_yudi()`（定义在 `always.lua` 第 10 行）。

**影响**：运行时 Lua 错误，触发此分支时脚本崩溃。

**修复**：改为 `mj_need_yudi()`（定义在 `always.lua:10`）

> **用户反馈 (06-21)**：已确认，`haveyd()` 是旧写法，忘记更新。

---

#### B3. `random_team_command()` 未定义 → ✅ 已解决

**文件**：`war_refactor.lua` 第 629 行（原）  
**代码**（原）：
```lua
random_team_command(2000)
```
**分析**：已确认为旧版随机碰运气组队策略的草稿。`06-21` 已替换为 `form_war_team()`，同时 `create_optimal_team()` 已改为三池制（P1锚点 + P2全收 + P5补位），比随机策略更可控且保证自己的ID优先。

> **修复 (06-21)**：公告牌 trigger 改为调用 `form_war_team()`，算法改为三池锚点制。此条已解决。

---

### 🟡 中等 (功能/逻辑)

#### M1. `configure_war_settings()` 调用时机问题

**文件**：`war_refactor.lua` 第 83 行（顶层调用）  
**问题**：`configure_war_settings()` 在脚本加载时立即执行，依赖全局变量 `WarMember`。但 `WarMember` 是由 `war_members.lua` 返回的模块对象。如果 `war_members.lua` 的加载晚于 `war_refactor.lua`，则会因 `WarMember` 为 nil 而失败。

**修复**：
1. 在 `michen_xkx.lua` 的 `loadlua_list` 中确保 `"war_members.lua"` 在 `"war_refactor.lua"` 之前加载
2. 可选：包裹 `pcall` 做防御性初始化：
   ```lua
   local ok, err = pcall(configure_war_settings)
   if not ok then print("[War] 初始化失败: " .. tostring(err)) end
   ```
3. 推荐方案（用户个人偏好）：仔细安排加载顺序确保可用，不包 pcall，这样出问题能第一时间暴露。

> **用户反馈 (06-21)**：原本想法是仔细安排加载顺序确保可用。

---

#### M2. `create_optimal_team()` 组队策略 — ✅ 已重构

~~原问题：只使用贪心排序取前缀，不尝试组合。~~

**修复 (06-21)**：已重构为三池锚点制：
- P1池（自己的重要ID，1-2个锚点）
- P2池（朋友的重要ID，全收）
- P5池（小号，从多到少逐级补位直到凑出有效队伍）
- 锚点策略保证自己的 ID 优先，P2 最大化收益，P5 灵活填位

---

#### M3. `calculate_team_exp()` 中 `actual_min10` 从未赋值

**文件**：`war_refactor.lua` 第 268 行  
**代码**：
```lua
actual_min10 = 0,   -- 32位有符号整数表示的最小exp*10
```
**问题**：字段声明了但从不填充，始终为 0。

**影响**：如果后续代码依赖此字段会得到错误值。

---

#### M4. 旧的 `war_trust_*` 数据结构与新的 `WarMember` 数据结构不兼容

**问题**：
- 旧版：五级信任表 (A/B/C/D/E)，各级有不同 VIP 数量和替换逻辑
- 新版：`WarMember` 只用 `priority` 字段（1-5 的数字），失去了 A/B/C/D/E 的语义

**影响**：
- 旧的 VVIP 逻辑（memberIDs_a 检查）完全丢失
- `max_vip` 配置变为全局计数而非按级控制
- 如果其他脚本（如 `war_start` 相关的 kill 流程）依赖旧数据结构，会出错

---

#### M5. `is_team_valid()` 是死代码

**文件**：`war_refactor.lua` 第 380-387 行  
**问题**：此函数定义了但从**未被调用**。`create_optimal_team()` 内部直接调用 `team_meets_conditions()` 而不是 `is_team_valid()`。

---

#### M6. `table.size()` 是死代码

**文件**：`war_refactor.lua` 第 303-307 行  
**问题**：定义了但未被调用。`war_members.lua` 中已经用内联循环 `for _ in pairs(...) do count = count + 1 end` 计数。

---

### 🔵 轻微 (代码质量)

#### L1. 重复触发流代码

`war_refactor.lua` 复制了大量原 `war.lua` 的触发处理代码（war_pre / war_start 部分），导致两者高度重复。如果未来 `war.lua` 的触发部分有更新，两边需要同步维护。

---

#### L2. `alias.start_teamwith()` 定义在外部文件

`alias.start_teamwith()` 定义在 `michen_alias.lua` 第 3781 行，`war_refactor.lua` 通过 timer callback 调用它。这种跨文件隐式依赖容易在重构时被遗漏。

---

#### L3. `\\Z` 正则标志位兼容性

**文件**：`war_refactor.lua` 第 841 行  
**模式**：`"...蒙古大军攻陷了南阳！\\Z"` + `maketri(..., 2)`

**问题**：`\\Z` 在 Lua 字符串中等价于字面 `\Z`。这在 PCRE 中匹配字符串末尾。该 trigger 使用了 flag=2（可能表示 multi-line 模式）。如果 Rust 端的 `regex` crate 不原生支持 `\Z` 且 flag 2 的行为不一致，此 trigger 可能无法正确匹配。

---

#### L4. 魔术数字 `2147483647` 多处出现

`MAX_INT32` 已定义为常量，但 `create_optimal_team()` 第 450 行又硬编码了 `2147483647`，应统一使用常量。

---

#### L5. `findstring` 的参数列表可能不兼容

**文件**：`war_refactor.lua` 第 1140-1150 行  
**代码**：
```lua
if findstring(c,"蒙古兵","十夫长","百夫长","千夫长") then
```
**问题**：`findstring` 的标准签名可能是 `findstring(str, pattern)`（见 workspace rules），多个 pattern 参数是否支持不明确。

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

**评估**：逻辑正确。返回溢出后的值和是否溢出。

**注意**：`product = value * 10` 在 Lua 5.3+ 中使用 64 位整数（如果 value 是整数），才能正确判断溢出。Lua 5.1/5.2 中数字是 double，大数乘法可能有精度损失。MushClient 使用 Lua 5.1，在 EXP 值接近 2^31 (~21 亿) 时，`value * 10` 在 double 中仍然是精确的（double 有 53 位精度），所以**此处安全**。

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

新旧文件都注册了相同名称的 trigger class（`war_pre_doth`, `war_start_doth`），**绝对不能同时加载**，否则触发器会重复注册，每个事件触发两次。

> **用户反馈 (06-21)**：不用担心，新的 war 脚本测试阶段会直接替换原有的 `war.lua` 文件。

---

## 五、Python风格的技术债务

### 5.1 snake_case 与驼峰混用

```lua
calculate_team_exp()      -- snake_case
shouldActivateTeamwith()  -- camelCase  (历史函数)
deepCopy()                -- camelCase
WarMember.find_member()   -- 模块方法
```

### 5.2 全局变量污染

`CURRENT_PRESENT_MEMBERS`、`MEMBER_DATA`、`maketri_num` 等直接放在全局作用域。

### 5.3 注释风格不统一

有 `--@shana` 署名，`--核心部分` 中文注释，以及 `看不懂千万别动` 警告，新旧代码注释量差异大。

---

## 六、优先修复顺序建议

| 优先级 | 编号 | 问题 | 类型 |
|--------|------|------|------|
| P0 | B1 | 新算法未接入触发流程 | 架构 |
| P0 | B2 | `haveyd()` 不存在 | Bug |
| P0 | B3 | ~~`random_team_command` 未定义~~ | ✅ 已解决 |
| P1 | M1 | `configure_war_settings` 调用时机 | 架构 |
| P1 | M2 | ~~贪心策略不尝试组合~~ | ✅ 已重构 |
| P2 | M3 | `actual_min10` 未赋值 | 清理 |
| P2 | M4 | 旧数据结构兼容 | 兼容 |
| P2 | M5 | `is_team_valid` 死代码 | 清理 |
| P2 | M6 | `table.size` / `select_random_members` 死代码 | 清理 |
| P3 | L1-L5 | 轻微问题 | 代码质量 |

---

## 七、附录：文件关系图

```
war_members.lua (模块)
  ├── WarMember.qt = {id → {ip, exp, priority}}
  ├── WarMember.ln = {...}
  ├── WarMember.find_member()
  ├── WarMember.add_member() / update_member()
  ├── WarMember.stats() / export_csv() / import_csv()
  └── return WarMember

war_refactor.lua (主逻辑, 试图替代 war.lua)
  ├── [新] 站点配置层
  │   ├── get_current_site_info()
  │   └── configure_war_settings()
  ├── [新] 32位溢出算法
  │   ├── add32()
  │   ├── multi10_with_overflow()
  │   └── calculate_team_exp()
  ├── [新] 队伍验证 (未接入触发)
  │   ├── team_meets_conditions()
  │   ├── is_team_valid() (死代码)
  │   └── select_random_members()
  ├── [新] 组队逻辑 (未接入触发)
  │   ├── create_optimal_team()
  │   └── form_war_team()
  ├── [新] 热重载
  │   └── reload_war_member()
  ├── [复制] war_pre 触发 (60-70% 与旧一致)
  ├── [复制] war_start 触发 (90% 与旧一致)
  └── [保留] war_start/restart timer (仍用旧逻辑)

michen_alias.lua (外部依赖)
  └── alias.start_teamwith() ← 被 timer callback 引用
      └── 仅 openclass("war_start_timer")，未调用新算法

always.lua (外部依赖)
  └── mj_need_yudi() ← war_refactor 中误写为 haveyd()
```

---

## 八、复盘新发现（N1-N15）

> 复盘日期：2026-06-22  
> 基于完整代码审查发现的遗漏问题，含修复方案与确认事项

### 🔴 严重（阻塞级）

#### N1. 玩家自身经验值未计入队伍计算

**位置**：`war_refactor.lua` `create_optimal_team()` → `try_build()` 内部

**问题描述**：  
`create_optimal_team()` 将 `my_id` 从所有池中排除，`try_build()` 构建队伍时从不包含玩家自身。但 `run("teamwith "..result)` 执行后，服务器上的队伍 = 当前角色 + 被邀请者，服务器校验条件时**包含玩家自身 exp**。

**影响**：
- 计算认为"有效"的队伍，加上玩家自身 exp 后可能变得"无效"（溢出后条件不再满足）
- 计算认为"无效"的队伍，加上玩家自身 exp 后可能实际"有效"（错失组队机会）

**修复方案**（已实施）：
1. 在 `create_optimal_team()` 顶部构建 `my_member` 结构体
2. 在 `try_build()` 中将 `my_member` 作为队伍第一个成员
3. `used_ips` 初始化时标记 `my_ip` 为已使用
4. 构建 `team_str` 时跳过 `my_id`（teamwith 命令不含自身）

---

#### N2. `war.teamwith` 和 `war.realteam` 未被新代码赋值

**位置**：`war_refactor.lua` `form_war_team()` 第 547-580 行

**问题描述**：  
太守府 trigger 设置 `war.teamwith = {}`，公告牌 trigger 设置 `war.realteam = {}`，但 `form_war_team()` 从不更新这两个变量，只设置了 `war.current_team` 和 `war.team_ids`（新字段）。

**影响**：以下依赖 `war.teamwith` 的 trigger 全部失效：
1. "懦夫"移除逻辑（第 660 行）：检查 `war.teamwith` 并从中移除懦夫玩家 → 永远不执行
2. 打坐完毕判断（第 710 行）：`if type(war.teamwith)=="table" and #war.teamwith>3` → 永远为 false
3. `warteam()` 函数（第 195 行）：依赖 `war.realteam` → 永远报"条件不足"

**修复方案**（已实施）：  
在 `form_war_team()` 成功后同步设置：
```lua
war.teamwith = {}  -- 拆分ID字符串为表
for id in string.gmatch(result, "%S+") do
    table.insert(war.teamwith, id)
end
war.realteam = deepCopy(war.teamwith)
```

---

#### N3. `reload_war_member()` 文件名拼写错误

**位置**：`war_refactor.lua` 第 108-113 行

**问题描述**：  
实际文件名是 `war_members.lua`（带 s），但代码中写为 `loadmod("war_member.lua")` 和 `package.loaded["war_member"]`。

**影响**：热重载功能完全失效。

**修复方案**（已实施）：  
改为 `loadmod("war_members.lua")` 和 `package.loaded["war_members"]`。

---

#### N4. `tonumber(war.waring>0)` 恒为 nil（Lua 5.1）

**位置**：`war_refactor.lua` 第 1009 行（原）

**问题描述**：  
在 Lua 5.1（MushClient 使用）中，`war.waring>0` 求值为 boolean `true`/`false`。`tonumber(true)` 返回 `nil`，`tonumber(false)` 也返回 `nil`。因此此条件**永远为 false**，整个 if 块是死代码。

**影响**：当 `war.waring>0` 且玩家在城外遇到"城外太危险了"时，不会执行 `alias.close_war()` + `alias.startworkflow()` 清理逻辑。

**修复方案**（已实施）：  
改为 `if war.waring>0 then`。

---

### 🟡 中等（功能/逻辑）

#### N5. `string.trim()` 不存在，`import_csv()` 会崩溃

**位置**：`war_members.lua` 第 165-170 行（`import_csv` 函数内）

**问题描述**：  
Lua 标准库没有 `string.trim()`。全局搜索 `scripts/class-utf8/` 未找到任何 `string.trim` 的定义。调用此方法会抛出 `attempt to call method 'trim' (a nil value)`。

**影响**：`WarMember.import_csv()` 完全不可用。

**修复方案**（已实施）：  
在 `war_members.lua` 顶部添加 `string.trim` 定义：
```lua
if not string.trim then
    function string.trim(s)
        return (s:gsub("^%s+", ""):gsub("%s+$", ""))
    end
end
```

---

#### N6. `try_build` 不尝试不同 P5 组合

**位置**：`war_refactor.lua` `try_build()` 第 425-465 行

**问题描述**：  
`try_build` 对每个队伍规模只尝试一种 P5 组合——始终取 `p5_avail[1..fill]`。如果 10 个 P5 在场需要选 3 个，只尝试前 3 个，不尝试其他组合。

**影响**：在场 P5 较多时，成功率远低于理论最优。

**修复方案**（已实施）：  
在 `try_build` 中实现双策略尝试：
1. 策略 `low_exp`：按 exp 升序取前 N 个 P5
2. 策略 `high_exp`：按 exp 降序取前 N 个 P5（反向遍历）

两种策略覆盖不同的 exp 分布场景，显著提升组队成功率。

---

#### N7. P5 池顺序不确定（`pairs` 迭代）

**位置**：`war_refactor.lua` 第 405-420 行

**问题描述**：  
`for eng_id, data in pairs(CURRENT_PRESENT_MEMBERS)` 的迭代顺序在 Lua 中不确定。P5 池的填充顺序每次运行可能不同，导致：
- 同一组在场玩家，不同次运行组出不同队伍
- 调试时难以复现问题

**修复方案**（已实施）：  
在分池完成后对 P5 池按 exp 升序排序：
```lua
table.sort(p5_pool, function(a, b) return a.exp < b.exp end)
```

---

#### N8. `war_start.timer()` 和 `war_restart.timer()` 仍调用旧逻辑

**位置**：`war_refactor.lua` 第 980-995 行

**问题描述**：  
两个 timer callback 仍调用 `run("war")`（服务器端 war 命令），不调用 `form_war_team()`。

**处理意见**：维持现有实现逻辑。  
`run("war")` 继续调用 `warteam()` 函数处理组队指令发送，`form_war_team()` 函数仅保留组队结果显示和历史记录功能，以避免 print flood 和频繁数据库操作。

**状态**：已确认，无需修改。`form_war_team()` 已改为不发送 `teamwith` 命令，仅设置 `war.teamwith`/`war.realteam` 供 `warteam()` 使用。

---

#### N9. 服务器平均值计算方式可能与模拟不一致

**位置**：`war_refactor.lua` `calculate_team_exp()` 第 280 行

**问题描述**：  
当前模拟的计算顺序是 `floor(sum / count) × 10`。服务器可能用 `sum × 10 / count`（先乘后除），两者在溢出场景下结果可能不同。

**处理意见**：已确认服务器 LPC 代码中 `max_exp > (avg_exp = total/war->query("total_players"))*10` 的逻辑正确性，与模拟一致。

**状态**：已确认，无需修改。

---

#### N10. `\\Z` 正则可能不被 Rust regex crate 支持

**位置**：`war_refactor.lua` 第 841 行

**问题描述**：`\\Z` 在 PCRE 中匹配字符串末尾，但 Rust `regex` crate 用 `\z`。

**处理意见**：已确认 Rust regex crate 到 PCRE 正则的移植适配已完成且功能正常。

**状态**：已确认，无需修改。

---

#### N11. 魔术数字 `9` 和 `4` 硬编码

**位置**：`war_refactor.lua` `try_build()` 内 `math.min(9, ...)` 和 `math.max(4, ...)`

**问题描述**：硬编码了最大/最小队伍人数，应使用 `war.max_partner` 和配置的最小人数。

**处理意见**：维持队伍人数硬编码实现，考虑到游戏长期未进行版本更新。

**状态**：已确认，无需修改。

---

#### N12. `WarMember.last_updated` 日期过时

**位置**：`war_members.lua` 第 79 行

**问题描述**：日期硬编码为 `"2023-11-15"`，实际已是 2026 年。

**修复方案**（已实施）：  
更新为 `"2026-06-22"`。

---

#### N13. `find_member()` 模糊匹配有 Lua 模式注入风险

**位置**：`war_members.lua` `find_member()` 函数

**问题描述**：  
```lua
if string.lower(id) == eng_id or string.find(id, eng_id) then
```
`string.find` 的第二参数是 Lua 模式。如果 `eng_id` 含 `(`、`.`、`%` 等特殊字符，会报错或误匹配。

**修复方案**（已实施）：  
改为 `string.find(id, eng_id, 1, true)`（第四参数 `plain=true` 表示纯文本匹配）。

---

### 🔵 轻微（代码质量）

#### N14. `war.team_history` 无限增长

**位置**：`war_refactor.lua` `form_war_team()` 第 577 行

**问题描述**：`form_war_team()` 每次成功都 `table.insert(war.team_history, ...)`，从不清理。

**处理意见**：暂时保留当前历史记录功能。

**修复方案**（已实施）：  
添加注释标记：`-- N14: 代码调试结束后可以考虑移除历史记录功能`

---

#### N15. `actual_min10` 变量未赋值问题

**位置**：`war_refactor.lua` `calculate_team_exp()` 第 268 行

**问题描述**：  
`actual_min10` 在 result 表中声明为 0，但从未被赋值。`team_meets_conditions()` 条件3需要此值，却自己重新计算了一遍 `min_exp10`，导致：
1. result 表中的 `actual_min10` 字段是误导性的（始终为 0）
2. 计算逻辑重复，违反 DRY 原则

**作用域与使用场景分析**：
- `actual_min10` 是 `calculate_team_exp()` 返回的 result 表中的字段
- 使用场景：`team_meets_conditions()` 条件3判断 `min_exp10 < average_exp`
- 当前 `team_meets_conditions()` 不引用此字段，自行重新计算

**两种修改方案**：

| 方案 | 描述 | 优点 | 缺点 |
|------|------|------|------|
| **方案A** | 移除 `actual_min10` 字段，`team_meets_conditions()` 保持自行计算 | 简洁，避免未使用字段 | 计算逻辑重复，result 表不完整 |
| **方案B（推荐）** | 在 `calculate_team_exp()` 中正确计算 `actual_min10`，`team_meets_conditions()` 直接引用 | result 表完整，消除重复计算，单一数据源 | 需同时修改两个函数 |

**推荐理由**：方案B 消除了 `calculate_team_exp()` 和 `team_meets_conditions()` 之间的重复计算逻辑，保证 result 表作为唯一数据源，符合函数封装原则。

**修复方案**（已实施，方案B）：
1. 在 `calculate_team_exp()` 末尾添加 `actual_min10` 计算：
   ```lua
   local min_exp = math.huge
   for i, member in ipairs(selected_members) do
       if member.exp < min_exp then
           min_exp = member.exp
       end
   end
   if min_exp ~= math.huge then
       result.actual_min10 = multi10_with_overflow(min_exp)
   end
   ```
2. `team_meets_conditions()` 条件3改为直接引用：
   ```lua
   if team_result.actual_min10 < team_result.average_exp then
       return false, "条件3未满足: ..."
   end
   ```

---

## 九、修复记录汇总

| 编号 | 严重度 | 修复状态 | 修改文件 |
|------|--------|----------|----------|
| N1 | 🔴 | ✅ 已修复 | war_refactor.lua |
| N2 | 🔴 | ✅ 已修复 | war_refactor.lua |
| N3 | 🔴 | ✅ 已修复 | war_refactor.lua |
| N4 | 🔴 | ✅ 已修复 | war_refactor.lua |
| N5 | 🟡 | ✅ 已修复 | war_members.lua |
| N6 | 🟡 | ✅ 已修复 | war_refactor.lua |
| N7 | 🟡 | ✅ 已修复 | war_refactor.lua |
| N8 | 🟡 | ⏳ 已确认维持 | — |
| N9 | 🟡 | ⏳ 已确认无需修改 | — |
| N10 | 🟡 | ⏳ 已确认无需修改 | — |
| N11 | 🔵 | ⏳ 已确认维持 | — |
| N12 | 🔵 | ✅ 已修复 | war_members.lua |
| N13 | 🔵 | ✅ 已修复 | war_members.lua |
| N14 | 🔵 | ✅ 已添加注释 | war_refactor.lua |
| N15 | 🔵 | ✅ 已修复（方案B） | war_refactor.lua |
