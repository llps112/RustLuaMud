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
