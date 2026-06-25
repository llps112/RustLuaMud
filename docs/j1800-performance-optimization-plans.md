# J1800 瘦客户机部署性能优化计划

> 部署目标：J1800 双核 Atom 2.4GHz / 2GB RAM，10 个 session 并发
> 分析日期：2026-06-23

---

## 基准数据

10 session 并发时每 session 的资源概况：

| 指标 | 数量/session | 10 session 合计 |
|------|-------------|-----------------|
| Trigger（宏注册） | ~500+ | **5000+** |
| Timer（常活跃） | ~30 | **~300** |
| 高频 Timer (≥1Hz) | 2~3 | **20~30** |
| `findstring` 调用/秒 | 数百~数千 | **数千~数万** |

---

## 优化项目清单

### ~~`[P0-1]` flush_exp 中禁用 iconv 子进程~~ ✅ 已完成

**改动**：`war_members.lua` 新增 `DEBUG_MODE` 开关 + 包裹 `iconv` 调用。当前为 `true`，部署时改 `false`。

---

### ~~`[P0-2]` flush_exp 写入频率降频~~ ✅ 已完成

**改动**：`flush_exp()` 改用文件锁（`.lock`）实现跨 session 限流，每 60 秒最多一次写入。10 session 合计从 2 次/s 降至 1 次/60s。

---

### ~~`[P1-1]` cmdcount_timer 降频~~ ✅ 已完成

**改动**：`cmdcount_timer` 间隔 0.1s → 0.5s，`cmd.numsdecrease` 递减量 -2 → -10，衰减速率保持 20/s 不变，限速行为完全一致。10 session timer 触发从 100次/s 降至 20次/s。

---

### ~~`[P1-2]` 减少 `run()` 中的 `openclass("cmdcount")` 重复调用~~ ✅ 已完成

**改动**：`run()` 中 `openclass("cmdcount")` 改为先检查 `GetTimerInfo("cmdcount_timer", 6)`，已启用时跳过。命令流动期间每次 `run()` 从 2 次 API 调用降为 1 次。

---

### `[P1-3]` 减少 `calc_cmdcount` 的 `openclass/closeclass` 调用量

**收益**：低  
**风险**：低  
**难度**：低

**说明**：
与 P1-2 类似，`cmd.numsdecrease` 中 `closeclass("cmdcount")` 后，下次 `run()` 又 `openclass`。可以考虑将 `cmdcount` 设计为始终开启，只在逻辑上控制心跳包触发。

**影响**：
- ✅ 减少 API 调用
- ⚠️ 需要确认 `cmdcount` class 的其他用途

---

### `[P2-1]` war_start_timer 间隔检查

**收益**：中  
**风险**：低  
**难度**：低

**现状**：
`war_refactor.lua` 中 `war_start_timer` 默认间隔为 `war.request_interval`。如果该值为 0.1s 或 1s，在 J1800 上频率过高。

**方案**：
在 `war.update()` 或部署配置中调高间隔。建议最小不低于 2s：

```lua
-- 在配置中设置（或添加兜底）
war.request_interval = math.max(war.request_interval or 2, 2)
```

**影响**：
- ✅ 减少组队阶段高频 timer
- ⚠️ 组队命令发送间隔变长，但不影响成功率

---

### `[P2-2]` 滑动窗口算法缓存（`create_optimal_team`）

**收益**：中  
**风险**：低  
**难度**：中

**现状**：
`create_optimal_team` 的滑动窗口是 O(n² × 21)。30 人候选池时约 465 次 `form_team_in_window` 调用，每次包含排序（O(w log w)）+ `is_team_valid` 验证。如果组队重试频繁，会形成 CPU 尖峰。

**方案**：
对候选池计算 hash（根据 eng_id + exp 拼接），相同 hash 直接返回上次结果：

```lua
-- 在 create_optimal_team 入口
local hash = ""
for _, m in ipairs(candidates) do
    hash = hash .. m.eng_id .. ":" .. m.exp .. "|"
end
if _team_cache and _team_cache.hash == hash then
    return _team_cache.result
end
-- ... 滑动窗口算法 ...
_team_cache = { hash = hash, result = { true, team_str } }
```

**影响**：
- ✅ 组队重试时不重复计算
- ⚠️ 候选池 EXP 变化时会重新计算（正确性不变）

---

### ~~`[P3-1]` findstring 调用链路优化~~ ✅ 已完成

**改动**：`findstring` 中用 `select` 替代 `ipairs{...}` 消除临时表分配，并缓存 `string.find` 到局部变量。

**方案**：
在高频路径上，用局部变量引用替代重复创建：

```lua
-- 改前
if findstring(l, "pattern1", "pattern2", ...) then

-- 改后（将模式列表定义为常量局部变量）
local WATCH_PATTERNS = { "pattern1", "pattern2", ... }
-- 然后在 dosomething1 内用 findstrlist(l, WATCH_PATTERNS)
```

**影响**：
- ✅ 减少每次触发的表构造
- ⚠️ 改动分散在多个函数中

---

### ~~`[P3-2]` `table_getn` 替换为 `#` 运算符~~ ✅ 已完成

**改动**：`war_refactor.lua`、`gps_lib.lua`、`michen_system.lua`、`michen_xkx.lua` 中所有 `table.getn`/`table_getn` 替换为 `#`，并删除 `table_getn` 函数定义。

---

### `[P3-3]` `cmdcount_timer` 对象引用缓存

**收益**：极低  
**风险**：低  
**难度**：极低

**现状**：
每次 Timer 回调都通过函数名查找 `cmd.numsdecrease`。可以将函数引用直接缓存为局部变量。

**影响**：
- ✅ 减少字符串查找开销
- ⚠️ 收益极微小，可做可不做

---

## 分阶段执行建议

### 阶段一 — 部署前必须做（预计 10 分钟）

| 顺序 | 项目 | 预计耗时 | 说明 |
|------|------|---------|------|
| 1 | `[P0-1]` flush_exp 禁用 iconv | 5min | 避免 J1800 上进程 fork |
| 2 | `[P1-1]` cmdcount_timer 降频 | 1min | 减少最高频 timer |
| 3 | `[P0-2]` flush_exp 写入降频 | 5min | 减少文件 I/O |

### 阶段二 — 部署后第一周观察

- 运行 `top` 观察 CPU 使用率
  - CPU < 30%：无需进一步优化
  - CPU 30%~60%：实施 `[P2-1]` war timer 间隔调整
  - CPU > 60%：实施 `[P1-2]` + `[P1-3]` openclass 优化

### 阶段三 — 按需优化

- 组队重试导致卡顿 → 实施 `[P2-2]` 算法缓存
- 每次组队耗时过长 → 优化 `findstring` 调用
- 无明显问题 → 保持现状

---

## 注意事项

1. **先部署，再优化**：1 个 session 在 J1800 上几乎无压力，10 个才是考验。先跑起来再看数据。
2. **不要在开发机（PC）上过度优化**：开发机 CPU 强 J1800 一个数量级，开发机感觉不到的卡顿在 J1800 上可能明显。
3. **Lua GC 是隐藏问题**：如果 10 session 都频繁创建临时表（`findstring` 内部的 `ipairs{...}`），Lua GC 可能成为瓶颈。表现为 CPU 不高但游戏响应间歇性卡顿。
