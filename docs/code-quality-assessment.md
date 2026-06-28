# 脚本代码质量评估报告

> 评估日期：2026-06-20
> 评估范围：`scripts/class-utf8/` 下 38 个 Lua 文件，共 33,751 行
> 评估状态：已完成一轮低质量代码修复后的全面复查

---

## 一、总体指标

| 指标 | 数值 | 说明 |
|------|------|------|
| 文件数 | 38 个 | 含主逻辑文件 + 各门派任务脚本 |
| 总行数 | 33,751 行 | — |
| 语法正确率 | **100%** | 全部通过 `luac -p` 语法校验 ✅ |
| TODO/FIXME/HACK 标记 | **0** | 已全部清理 ✅ |
| 函数定义总数 | 998 个 | 含全局函数和局部函数 |
| `print`/`Note` 调用 | 580 次 | 部分可能为遗留调试信息 |
| `tonumber()` 调用 | 520 次 | 类型转换极为密集，暗示字符串/数字类型混用严重 |
| `pcall` 使用 | 17 次 | 运行时错误保护覆盖不足 |
| `_G` 动态访问 | ~20 处 | 全局命名空间滥用 |
| 注释代码 | 31 处 `--[[` 块注释 | 大量注释掉的遗留代码 |

---

## 二、文件结构总览

| 文件名 | 行数 | 函数数 | 主要职责 |
|--------|------|--------|---------|
| `michen_alias.lua` | 5,510 | ~300 | 核心业务逻辑（别名、寻路、战斗、任务调度） |
| `gps.lua` | 2,350 | 67 | GPS 寻路系统 |
| `Entrance_table.lua` | 2,228 | — | 入口房间数据表 |
| `check.lua` | 1,904 | 29 | 触发器回调（物品处理、状态检查） |
| `always.lua` | 1,585 | 45 | 常驻状态管理、限时判断 |
| `common.lua` | 1,447 | 27 | 通用功能（登录、打坐、船、学技能） |
| `michen_mp_gb.lua` | 1,249 | 18 | 丐帮门派任务 |
| `fj.lua` | 1,152 | 70 | 护镖（FJ）任务 |
| `michen_var.lua` | 1,148 | 2 | 全局变量声明（核心数据） |
| `war.lua` | 1,130 | 56 | 战争（WAR）任务 |
| `michen_yb.lua` | 1,127 | 18 | 押镖（YB）任务 |
| `gps_lib.lua` | 1,074 | 32 | 寻路算法库 |
| `skills.lua` | 1,014 | 6 | 技能练习管理 |
| `michen_system.lua` | 542 | 17 | 系统函数（trigger 管理、字符串工具） |
| `michen_config.lua` | 458 | 31 | 配置命令接口（`#cfg` 系列） |
| `dummy.lua` | 449 | 29 | 机器人挂机辅助 |
| 各门派 `michen_mp_*.lua` | 200-800 | 2-23 | 各门派任务逻辑（15 个文件） |
| 其他（`xinfa`, `kill`, `perform` 等） | 28-498 | 1-12 | 辅助功能 |

---

## 三、问题清单

### 🔴 严重（Severity 1 — 需优先处理）

#### 1. 全局命名空间严重污染

- **分类**：可维护性
- **位置**：
  - [michen_var.lua](file:///home/baiyf/RustLuaMud/scripts/class-utf8/michen_var.lua)：全局声明 100+ 个表变量（`me`, `hp`, `workflow`, `stat`, `add`, `have`, `sum`, `mark` 等全部顶层命名空间）
  - [always.lua](file:///home/baiyf/RustLuaMud/scripts/class-utf8/always.lua#L30-L58)：裸全局变量 `de_bug`, `tunanum`, `st`, `rekill`, `pfmid`, `wieldweapon` 等 30+ 个
  - [michen_system.lua](file:///home/baiyf/RustLuaMud/scripts/class-utf8/michen_system.lua#L24-L109)：核心函数 `table_getn`, `maketri`, `addtri`, `isopen`, `findstring`, `linktri`, `linktri2` 等全部全局声明，未归入命名空间表
- **影响**：全局命名空间高度拥挤，任何脚本不小心就能覆盖其他脚本的变量；排查问题困难（无法确定变量在哪里被修改）；不支持模块化加载
- **建议**：将所有核心函数收束到 `system` 表；全局数据表保持但增加前缀保护；新代码用 `local` 限制作用域

#### 2. `_G` 动态访问泛滥

- **分类**：可维护性
- **位置**：
  - [michen_alias.lua#L859](file:///home/baiyf/RustLuaMud/scripts/class-utf8/michen_alias.lua#L859)：`_G["killskill"]`
  - [always.lua#L762](file:///home/baiyf/RustLuaMud/scripts/class-utf8/always.lua#L762)：`_G["notconnect"]`
  - [common.lua#L343](file:///home/baiyf/RustLuaMud/scripts/class-utf8/common.lua#L343)：`_G[workflow.nowjob.."weapon"]`
  - [michen_system.lua#L522-L526](file:///home/baiyf/RustLuaMud/scripts/class-utf8/michen_system.lua#L522)：序列化遍历 `_G[v]`
  - [kill.lua#L87](file:///home/baiyf/RustLuaMud/scripts/class-utf8/kill.lua#L87)：`_G["killskill"]`
  - [gps.lua#L266](file:///home/baiyf/RustLuaMud/scripts/class-utf8/gps.lua#L266)：`_G[workflow.nowjob.."weapon"]`
- **影响**：动态字符串拼接去读写全局变量，拼写错误不会报错而是静默返回 `nil`；调试困难；IDE 无法提供补全
- **建议**：创建 `gbl = {}` 全局配置表，用 `gbl.killskill` 替代 `_G["killskill"]`；动态 key 访问用 `gbl[workflow.nowjob .. "_weapon"]` 并补上默认值检查

#### 3. 大量魔法数字（Magic Numbers）

- **分类**：可维护性
- **位置**（覆盖全项目）：
  - 时间常量：3600 秒（任务限时）、3400 秒（War 限时）、2000 秒（YB 等待限时）、30 秒（空闲阈值）
  - 经验阈值：[always.lua#L172](file:///home/baiyf/RustLuaMud/scripts/class-utf8/always.lua#L172)：`30000`/`3000000`
  - 技能等级阈值：`500`（凌波微步/最大技能等级）、`320`（天地风雷剑）、`299`（先天功）、`79`（玄阴真气）、`100`/`30`（literate 阶段）
  - 房间 ID 硬编码：`1101`, `1094`, `1902`, `1945`, `1933`, `1387` 等散布在 20+ 文件中
  - 武器类型常量：`"zhongjian"`, `"sword"`, `"blade"` 等以字符串形式散布
- **影响**：修改数值需要全局搜索替换，极易遗漏；新人无法理解数字含义
- **建议**：
  - 时间常量提取为 `JOB_COOLDOWN = 3600`
  - 经验阈值提取为 `EXP_FJ_LOWER = 30000`, `EXP_FJ_UPPER = 3000000`
  - 房间 ID 提取为 `ROOM_KAIFENG = 1101` 等
  - 集中到 `constants.lua` 或 `michen_config.lua` 头部

#### 4. 特长函数与超高圈复杂度

- **分类**：可维护性
- **位置**：
  - [michen_alias.lua](file:///home/baiyf/RustLuaMud/scripts/class-utf8/michen_alias.lua#L4170)：`alias.startworkflow()` — 500+ 行单函数，逻辑分支难以追踪
  - [michen_alias.lua#L4130](file:///home/baiyf/RustLuaMud/scripts/class-utf8/michen_alias.lua#L4130)：`alias.choose_liansk()` — 约 300 行，if-else 链嵌套超 15 层
  - [michen_alias.lua#L3887](file:///home/baiyf/RustLuaMud/scripts/class-utf8/michen_alias.lua#L3887)：`alias.setmpLimitedMark()` — 承担了 5 种不同任务的限时重置逻辑
  - [michen_alias.lua#L3950-L3980](file:///home/baiyf/RustLuaMud/scripts/class-utf8/michen_alias.lua#L3950)：`alias.lianwu()` 单行条件超 300 字符
- **影响**：无法单测；修改一个分支可能影响其他分支；新人理解成本极高
- **建议**：
  - 将 `alias.startworkflow()` 拆分为按任务分派的小函数
  - 将 `alias.choose_liansk()` 的 if-else 链改为数据驱动（用一个技能优先级配置表）
  - 每个函数控制在 50 行以内，单一职责

---

### 🟠 高（Severity 2 — 建议尽早处理）

#### 5. 大量注释掉的遗留代码

- **分类**：可读性
- **位置**：
  - [michen_alias.lua#L3827-L3848](file:///home/baiyf/RustLuaMud/scripts/class-utf8/michen_alias.lua#L3827)：`alias.startliaoshang()` 注释掉的旧条件逻辑 20+ 行
  - [michen_alias.lua#L497](file:///home/baiyf/RustLuaMud/scripts/class-utf8/michen_alias.lua#L497) `--function alias.ch()` 整个函数被注释
  - [michen_mp_gm.lua#L282](file:///home/baiyf/RustLuaMud/scripts/class-utf8/michen_mp_gm.lua#L282)：注释掉的限时检查
  - 各文件共计 31 处 `--[[` 块注释
- **影响**：干扰代码阅读；长期无用的注释会误导维护者
- **建议**：确认无用后删除注释代码；如果确有保留价值，加 `-- [KEPT_FOR_REF]` 标记并说明原因

#### 6. `linktri` 与 `linktri2` 函数重复

- **分类**：代码重复（DRY）
- **位置**：[michen_system.lua#L142-L195](file:///home/baiyf/RustLuaMud/scripts/class-utf8/michen_system.lua#L142) 和 [michen_system.lua#L164-L195](file:///home/baiyf/RustLuaMud/scripts/class-utf8/michen_system.lua#L164)
- **差异**：`linktri2` 只比 `linktri` 少了对字符串类型的特殊处理，核心拼接逻辑完全一样
- **建议**：合并为一个函数，用参数控制行为

#### 7. 模块加载顺序耦合

- **分类**：可扩展性
- **位置**：
  - [always.lua#L30](file:///home/baiyf/RustLuaMud/scripts/class-utf8/always.lua#L30)：`if me==nil then me={} end` — 隐式依赖 `me` 在别的脚本中已定义
  - 各 `michen_mp_*.lua` 隐式依赖 `michen_var.lua` 的全局变量
- **影响**：改变加载顺序就可能导致 `nil` 错误；无法单独加载/测试一个模块
- **建议**：添加 `require` 机制或统一入口脚本明确加载顺序；模块内部做前置依赖检查

#### 8. 变量命名不一致且缺乏语义

- **分类**：可读性
- **位置**（全项目）：
  - 多风格混用：snake_case（`roomno_now`, `flytoareastartid`）、camelCase（`KillNextNPC`, `SafeEntrance`）、拼音（`liaoshang`, `yinyun`, `chongmai`）
  - 无意义变量名：[michen_alias.lua](file:///home/baiyf/RustLuaMud/scripts/class-utf8/michen_alias.lua) 大量出现 `_tb`, `_f`, `_t`, `_t1`, `a`, `b`, `c`, `_str` 等临时变量名
  - 缩写歧义：`fj`（护镖）、`mp`（门派任务）、`ftb`（送信）、`yb`（押镖）、`qz`（全真）、`wd`（武当）等内部缩写，对新人不够直观
- **影响**：降低代码自文档化程度；拼音名对非中文读者不可理解
- **建议**：
  - 统一使用 snake_case：`room_no_now`, `fly_to_area_start_id`
  - 缩写名在文件头部加中文-英文对照注释
  - 无意义变量名在重构时逐步替换为有语义的名称

---

### 🟡 中（Severity 3 — 值得在处理时修正）

#### 9. `local` 作用域缺失

- **分类**：正确性
- **位置**：
  - [michen_alias.lua](file:///home/baiyf/RustLuaMud/scripts/class-utf8/michen_alias.lua#L3950+)：多数函数内 `_tb`, `_f`, `_t` 等变量未加 `local`，被泄漏到全局
  - 各文件中 `for k, v in pairs()` 中的 `k`, `v` 在某些上下文中会泄漏
- **影响**：变量意外覆盖或不期望的跨函数数据共享
- **建议**：使用 `luacheck` 扫描并补全 `local`；严格遵循"所有变量先 `local` 声明"

#### 10. 参数校验不充分

- **分类**：健壮性
- **位置**：
  - [michen_alias.lua#L73-L78](file:///home/baiyf/RustLuaMud/scripts/class-utf8/michen_alias.lua#L73)：`alias.setproxy()` 部分校验
  - [michen_alias.lua#DZ](file:///home/baiyf/RustLuaMud/scripts/class-utf8/michen_alias.lua#DZ)：`alias.dz(a)` 中 `tonumber(a)` 无 nil 保护
  - [gps.lua](file:///home/baiyf/RustLuaMud/scripts/class-utf8/gps.lua)：部分 `gps_cmd` 调用前未校验参数有效性
- **影响**：非法参数传入时静默出错或抛出难追查的异常
- **建议**：函数入口做类型断言或守卫子句，失败时输出清晰日志

#### 11. 错误处理覆盖不足

- **分类**：健壮性
- **位置**：
  - 只有 17 处 `pcall` 使用，集中在文件 I/O（`dummy.lua`）和 `gps_cmd`
  - trigger/alias/timer 回调执行无错误传播机制
  - 文件写入（`michen_system.lua#L459`）虽然有 `pcall`，但失败仅记录，无重试或 fallback
- **影响**：某个回调中的 Lua 错误会导致整条处理链中断，且不显示错误信息
- **建议**：在关键路径（文件 I/O、网络请求、用户输入处理）增加 `pcall`/`xpcall`；重要操作提供重试机制

#### 12. 硬编码敏感信息

- **分类**：安全
- **位置**：
  - [war.lua#L5-L41](file:///home/baiyf/RustLuaMud/scripts/class-utf8/war.lua#L5)：`war_trust_a/b/c` 表中包含游戏账号名（`icrien`, `ecrikq`, `lsjmj`, `zorro` 等）和 IP 注释
- **影响**：代码分发时泄露账号信息
- **建议**：将玩家账号信息移到配置文件中，`war.lua` 只引用配置键名

#### 13. `findstring` 使用 Lua 模式匹配的歧义

- **分类**：正确性
- **位置**：
  - [check.lua#L82](file:///home/baiyf/RustLuaMud/scripts/class-utf8/check.lua#L82)：`findstring(l,".+倒在地上")` — `.+` 在 Lua 模式中匹配"一个或多个任意字符"
  - 多处类似调用，混合了「字面字符串查找」和「模式匹配」两种意图
- **影响**：`findstring` 内部使用 `string.find`，当调用者传入含 `%`, `.`, `+`, `*` 等 Lua 模式特殊字符时，行为不符合"字面查找"的直觉
- **建议**：拆分两个函数：`str_contains(str, literal)` 用 `string.find(str, literal, 1, true)` 做纯字面查找；`str_matches(str, pattern)` 做模式匹配

#### 14. `math.randomseed` 在非主文件调用

- **分类**：正确性
- **位置**：[michen_var.lua#L7](file:///home/baiyf/RustLuaMud/scripts/class-utf8/michen_var.lua#L7)：`math.randomseed(os.time())`
- **影响**：变量声明文件可能被多次 `dofile` 或重载，导致随机种子被重置，降低随机性
- **建议**：将种子初始化放在主入口脚本（如 `always.lua` 或 `michen_alias.lua`），并加保护只执行一次

---

### 🟢 低（Severity 4 — 在常规迭代中逐步改进）

#### 15. 长行可读性差

- **分类**：可读性
- **位置**：
  - [michen_alias.lua#L4000](file:///home/baiyf/RustLuaMud/scripts/class-utf8/michen_alias.lua#L4000)：单行条件判断超 300 字符
  - 各文件多处类似的长复合条件表达式
- **影响**：在窄屏或 diff 中难以阅读
- **建议**：用中间变量拆分复杂条件，每行不超过 120 字符

#### 16. 中文拼音命名影响可读性

- **分类**：可读性
- **位置**：散布在 30+ 文件中
  - `liaoshang`（疗伤）、`yinyun`（吟韵）、`chongmai`（冲脉）
  - `xuemaster`（学 master）、`dazuo`（打坐）、`fangqi`（放弃）
  - `shizhe`（使者）、`aoyao`（熬药）、`putizi`（菩提子）
- **影响**：非中文读者无法理解函数用途
- **建议**：文件头部加拼音-英文对照表；新代码避免生产拼音命名

#### 17. `table.getn` 已废弃

- **分类**：兼容性
- **位置**：[michen_system.lua#L24](file:///home/baiyf/RustLuaMud/scripts/class-utf8/michen_system.lua#L24)：自定义 `table_getn` 用 `pairs` 计数
- **影响**：性能差于 `#` 操作符（O(n) vs O(1)），且使用已废弃的 Lua 5.0 API 风格
- **建议**：直接用 `#` 操作符获取序列长度；如果需要获取表大小，使用更明确的自定义函数名

#### 18. 文件组织结构待优化

- **分类**：可扩展性
- **位置**：
  - `michen_var.lua`：混合了变量声明、随机种子初始化、全局配置
  - `michen_system.lua`：混合了 trigger 管理、字符串工具、方向映射、序列化函数
- **影响**：难以定位功能代码；责任不清晰
- **建议**：按职责拆分出头文件：
  - `constants.lua`：所有常量与魔法数字
  - `system/triggers.lua`：trigger/timer/alias 管理
  - `system/strings.lua`：字符串工具函数
  - `system/directions.lua`：方向映射

---

## 四、综合评分

| 维度 | 评分（满分5） | 说明 |
|------|:-----------:|------|
| **代码可读性** | ⭐⭐⭐ | 改进后多数函数逻辑清晰，但拼音命名和特长函数降低了可读性 |
| **可维护性** | ⭐⭐⭐ | 全局命名空间和魔法数字是主要维护障碍 |
| **可扩展性** | ⭐⭐ | 模块间隐式耦合较重，新增门派需要修改多个核心文件 |
| **健壮性** | ⭐⭐⭐ | 语法正确且所有文件通过编译，但运行时错误保护不足 |
| **性能** | ⭐⭐⭐⭐ | MUD 场景下性能充裕。`tonumber` 密集调用存在微小优化空间 |
| **安全性** | ⭐⭐⭐ | 核心逻辑安全，但账号信息硬编码需关注 |

### 总体评语

经过一轮低质量代码修复后，语法层面已完全干净（0 语法错误），技术债务注释已清理。当前代码可以稳定运行，但**架构层面的长期积累问题**依然突出。全局命名空间污染、魔法数字、特长函数是最大的三项技术债务。建议按优先级逐步修复，**优先处理 🔴 严重级别问题**。

---

## 五、修复路线图建议

| 阶段 | 内容 | 预估工作量 |
|------|------|-----------|
| **Phase 1** | 删除注释代码、合并 `linktri`/`linktri2`、`math.randomseed` 迁移 | 1 天 |
| **Phase 2** | 提取常量（`constants.lua`）、修复 `findstring` 歧义、补 `local` | 2 天 |
| **Phase 3** | 拆分特长函数（`startworkflow`、`choose_liansk`） | 3 天 |
| **Phase 4** | 修复 `_G` 访问、收束全局函数到命名空间 | 3 天 |
| **Phase 5** | 分离敏感信息到配置、增加参数校验和错误处理 | 1 天 |
| **Phase 6** | 重命名拼音命名、优化文件结构 | 2 天（渐进式） |
