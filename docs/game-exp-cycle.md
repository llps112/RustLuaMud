# 游戏任务 EXP 周期机制

## 通用概念

MP/YB/War 共用一套周期机制：**任务每获得一次 EXP，都会重置周期计时器**。周期内获得的 EXP 有上限，达到上限后无法再获得，要等周期结束才能继续。

> **FTB 不使用这套 MarkTime 周期机制**，它由服务端 `time_rewards` + 独立的 `adjust_rate()` 动态衰减控制，参见第四节。

```
周期示意：
MarkTime                          MarkTime+3600
  |------------------------------------|
  第一次获得 EXP             周期结束，上限解除
```

- **MarkTime**：每个周期第一次获得 EXP 时的 `os.time()` 时间戳
- **周期时长**：3600 秒（War 检测时用 3400s，但实际也是 3600s 一周期）
- **EXP 上限**：各任务各门派不同

## 统一流程

所有任务类型的 EXP 统计都通过 `check.lua` 中的 trigger 触发，走同一套流程：

```
游戏返回 EXP 数据 → check.lua trigger 捕获
    → alias.checkexp("任务类型") 设置游戏变量
    → 游戏返回 checkexpover=任务类型
    → 各任务 handler 处理上限判断
    → check.lua 中 w[3]=="类型" 分支累计 EXP
```

## 自动周期重置

每 10 秒由 `always_watch_timer10` 调用 `alias.setmpLimitedMark()` 检查并重置过期的周期。

详见 `michen_alias.lua` 的 `alias.setmpLimitedMark()` 函数。

## 一、MP（门派任务）

### 核心变量

| 变量 | 类型 | 说明 |
|------|------|------|
| `mpLimited.MarkTime` | number | 本周期开始时间戳 |
| `mpLimited.mpexp` | number | 本周期累计 EXP |
| `mpLimited.MarkExp` | number | 本周期 EXP 上限值 |
| `mpLimited.type` | string | 当前 MP 子类型（如 "yudi"、"beg1" 等） |
| `mpJobLimited` | 0/1 | 1=周期 EXP 已达上限，停止接 MP 任务 |

### 门派差异（mpLimited 结构体字段）

| 门派 | 周期 EXP 字段 | 周期 MarkTime 字段 |
|------|-------------|-------------------|
| 丐帮 (gb) | `beg1` / `beg2` | `MarkTimebeg1` / `MarkTimebeg2` |
| 大理 (dl) | `arrest` | `MarkTimearrest` |
| 其他（bt/em/gm/hs/mj/qz/sl/th/wd/xx/xs） | `mpexp` | `MarkTime` |

### EXP 上限

各门派的上限值由游戏变量 `me.menpaiLimited` 决定，不同门派/不同等级上限不同。

> 丐帮的 beg1/beg2 上限为 7000（硬编码检测 `>=7000`）。

### EXP 累积路径

1. `check.lua` 检测到 `w[3]=="mp"` 分支
2. 先调用 `alias.setmpReCountTime()` 做反应式周期重置
3. 再 `mpLimited.mpexp = mpLimited.mpexp + add.exp`

> 丐帮：`beg1/beg2` 各自累积 + `mpexp` 也会同时累积
> 大理：`arrest` 单独累积（不走 `mpexp`）

### 周期重置

**反应式**（`alias.setmpReCountTime()`）：
```
if add.exp > 20 then
    if (mpJobLimited==0 and (MarkTime+3600 < os.time())) or mpJobLimited>0 then
        mpexp = 0
        MarkTime = os.time()
        -- 新周期开始
    end
end
```

**主动式**（`alias.setmpLimitedMark()` 每 10 秒）：
```
-- mpJobLimited>0 时（上限达到后）
if MarkTime+3600 < os.time() or mpexp < MarkExp then
    mpJobLimited = 0
end
if mpJobLimited == 0 then
    mpexp = 0
    -- MarkTime 不更新，等待下次获得 EXP 时由 setmpReCountTime 更新
end

-- mpJobLimited==0 时（上限未到但周期自然过期）
if MarkTime+3600 < os.time() then
    mpexp = 0
end
```

### 上限触发

各门派在 `checkexpover=mp` trigger 中判断。典型逻辑：
```
if add.exp < 10 or mpexp >= me.menpaiLimited then
    mpJobLimited = 1
    if MarkTime < os.time()-3600 then
        MarkTime = os.time()-3600+120  -- 过期但 busy，推后 2 分钟
    end
end
```

### 特殊门派

- **丐帮 (gb)**：两套独立周期（beg1=讨饭1, beg2=讨饭2），各自有 `Limited1/Limited2` 标记
- **明教 (mj)**：御敌任务（yudi）有独立的周期检测：御敌击杀敌人时检查 `MarkTime+3600 < os.time()` → 主动清零

---

## 二、YB（押镖）

### 核心变量

| 变量 | 类型 | 说明 |
|------|------|------|
| `ybLimitedMarkTime` | number | 本周期开始时间戳 |
| `ybexp` | number | 本周期累计 EXP |
| `ybLimited` | 0/1 | 1=周期 EXP 已达上限 |

### EXP 上限

5000（硬编码检测 `ybexp>=5000`）。

### EXP 累积路径

`check.lua` 中 `workflow.nowjob=="yb"` 分支，先调用 `alias.setybReCountTime()`，再 `ybexp = ybexp + add.exp`。

### 周期重置

**反应式**（`alias.setybReCountTime()`）：
```
if add.exp > 100 then
    if (ybLimited==0 and (MarkTime+3600 < os.time())) or ybLimited>0 then
        ybexp = 0
        MarkTime = os.time()
    end
end
```

**主动式**（`alias.setmpLimitedMark()` 每 10 秒）：
```
if (ybexp > 100 or ybLimited > 0) and (now - MarkTime) >= 3600 then
    ybLimited = 0
    ybexp = 0
end
```

### 上限触发

`michen_yb.lua` 中 `checkexpover=yb` trigger：
```
if add.exp < 200 or ybexp >= 5000 then
    ybLimited = 1
    if MarkTime < os.time()-3600 then
        MarkTime = os.time()-3600+120  -- 过期但 busy，推后 2 分钟
    end
end
```

---

## 三、War（守城）

### 核心变量

| 变量 | 类型 | 说明 |
|------|------|------|
| `WarLimitedMarkTime` | number | 本周期开始时间戳 |
| `WarExp` | number | 本周期累计 EXP |
| `WarLimited` | 0/1 | 1=周期 EXP 已达上限 |
| `WarTotalExp` | number | 当天 0:00 开始的总 EXP（不计周期） |

### EXP 上限

13000（硬编码检测 `WarExp>=13000`）。

### EXP 累积路径

`check.lua` 中 `w[3]=="war"` 分支，先调用 `alias.setWarReCountTime()`，再 `WarExp = WarExp + add.exp`，同时 `WarTotalExp = WarTotalExp + add.exp`。

### 周期重置

**反应式**（`alias.setWarReCountTime()`）：
```
if add.exp > 1000 then
    if (WarLimited==0 and (MarkTime+3600 < os.time())) or WarLimited>0 then
        WarExp = 0
        MarkTime = os.time()
    end
end
```

**主动式**（`alias.setmpLimitedMark()` 每 10 秒）：
```
if (WarExp > 1000 or WarLimited > 0) and (now - MarkTime) >= 3400 then
    -- 3400s 而非 3600s，提前 200s 重置
    WarLimited = 0
    WarExp = 0
end
```

### 上限触发

`war_refactor.lua` 中 `checkexpover=war` trigger：
```
if add.exp < 500 or WarExp >= 13000 then
    WarLimited = 1
    if MarkTime < os.time()-3600 then
        MarkTime = os.time()-3600+120
    end
end
```

---

## 四、FTB（护镖／傅镖）

FTB 的 EXP 机制与其他任务截然不同，由服务端 `ftb_zhu.c`（NPC 程进福）全权控制。Lua 脚本只负责显示统计数据（`count.ftb`、`stat.avgftb`），不参与 EXP 周期管理。

### 全流程

```
ask_job() → assign_job() → start_job("pub_ftb") → 做任务 → tell_job() → adjust_rate() → time_rewards()
```

### Reward 计算（`tell_job()` 内）

位于 `LPC/ftb_zhu.c` ~L463-L523，在调用 `time_rewards()` 之前先算出 `exp_rate` 和 `rate`。

**基础 rate（pot 系数）**：
```
rate = 50 + range × 5          // range ∈ [4, 10]
```

**时间补偿（搜寻耗时越长，系数越低）**：
```
// 假设 1 分钟找路 + 每个怪 2 分钟击杀
if range > 5:
    rate = rate × (240 + obj_num × (240 + 10×(range-5))) / (120 + used_time)
else:
    rate = rate × (240 + obj_num × 240) / (120 + used_time)
```

**完成比例**：
```
exp_rate = rate × kill_num / obj_num    // 杀得越少，exp 越少
```

**随机因子**：
```
rate = rate × (60 + random(20)) / 60    // ±16.7% 浮动
```

**献祭加成**：`sacrifice > 0` 时，`rate += 20, exp_rate += 20`（消耗一次献祭次数）。

**高经验衰减**：
| 经验范围 | 衰减 |
|---------|------|
| > 50M | exp_rate 和 rate 各 ÷2 |
| > 30M | ×2/3 |
| > 10M | ×3/4 |

### 动态 EXP 上限（`adjust_rate()`）

位于 `LPC/ftb_zhu.c` ~L291-L335，**每次交任务前调用**，这是一个独立于 `time_rewards` 周期之外的独立衰减机制。

**`player->query("job_limit/ftb/exp_limit")` 初始值**：10000

**衰减（距离上次完成 ≤ 20 分钟）**：
```
exp_limit -= (exp_limit / 100 + random(50))
// 约 -1% + -0~49
// 下限 1000，上限 15000
```

**恢复（距离上次完成 > 20 分钟）**：
```
rate = (time() - last_job) / 86400         // 按天算的浮点数
exp_limit += to_int(exp_limit × rate)
// 20 分钟 ≈ 恢复 1.4%，休息越久恢复越多
// 上限 15000
```

**恢复示例**：
| 休息时间 | 恢复比例 |
|---------|---------|
| 20 分钟 | ~1.4% |
| 1 小时 | ~4.2% |
| 4 小时 | ~16.7% |
| 24 小时 | 回到上限 15000 |

### `time_rewards()` 参数固定

每次交任务时在 `tell_job()` 中固定设置（~L521）：
```c
player->set("JOBD/"+JOB_NAME+"/start", time() - 60);
```

即 `time_ratio` 固定为 `60 / 3600 ≈ 0.0167`。

然后调用：
```c
JOB_D->time_rewards(player, 1, 1, "pub_ftb", 0, exp_rate, rate);
```

- **exp_cat = 1**：category 1，`max_exp[0] = 13000`
- **pots_cat = 1**：category 1，`max_pots[0] = 9000`
- **exp_limit = 0**：不按总经验拒绝
- **exp_factor = exp_rate**：由 `tell_job()` 计算的任务表现系数
- **pots_factor = rate**：由 `tell_job()` 计算的 pot 系数

### `time_rewards()` 中的上限判定

`time_rewards` 内部仍用自己的 1 小时周期（`/check` + `/exp`）做二次上限截断：

```
chk_interval = time() - JOBD/pub_ftb/check
exp_accumulated = JOBD/pub_ftb/exp

if chk_interval > 3600:
    记录速度到数据库
    重置 /check 和 /exp

if chk_interval > 3600:
    if grant_exp × 3600 / chk_interval > maxexp → grant_exp = 0
else:
    if exp_accumulated > maxexp or grant_exp > maxexp → grant_exp = 0
```

这里的 `maxexp` 就是 `adjust_rate()` 设定好的 `get_exp_limit("pub_ftb")`。

### 最终收益公式

```
grant_exp = (maxexp × e_ratio - age_adj) × (60/3600) × exp_rate

其中:
- maxexp  = adjust_rate() 衰减后的 exp_limit（动态值）
- e_ratio = exp_ratio(me, 1) = max(0.75, (19 - exp/3M)/15)，cat1 最低 1.0
- age_adj = age > 100 时，(age-100)×(1+random(10))
- exp_rate = tell_job() 基于任务表现算出的系数
```

### 持续做 FTB 的收益变化曲线

```
exp_limit（动态上限）
    ^
15000|  ┌──── 初始/充分休息后
    |  │
    |  │       每次做完衰减约 1%+random(50)
    |  │       ┌── 继续做 → 继续降
10000|──┼──初始值
    |  │       │
    |  │       └── 休息 20 分钟 → 开始恢复
1000 |──┼────────────── 下限
    |  │
    +──┴─────────────────────────→ 时间
     做→做→做→做→ 休息→做→做→...
```

### Lua 端 FTB 周期追踪

Lua 侧不追踪 FTB 周期，只记录统计数据：
- `count.ftb`：完成次数
- `stat.avgftb`：平均每小时经验
- `mpLimited` 结构体中的 FTB 相关字段（`ftbtime`、`ftbexp`、`ftbLimitedMarkTime`、`ftbexpnow`）用于 `#stat` 显示

---

## 周期图示

### 正常周期（上限达到）
```
MarkTime                 MarkTime+3600
  |------ 做任务赚 EXP ------|---- 等重置 ----|
  mpexp ↗ 增长              mpJobLimited=1    mpJobLimited=0
  首次 EXP → MarkTime       触发 checkexpover  定时器检测到过期
```

### 正常周期（上限未到，切换其他任务）
```
MarkTime                      MarkTime+3600
  |--- 做任务赚 EXP ---|-- 切 YB/War/发呆 --|
  mpexp ↗ 增长           mpexp 停留          定时器检测到过期
  首次 EXP → MarkTime                         mpexp=0，MarkTime 不变
```

### BUSY 过期（checkexpover 触发时 MarkTime 已过期）
```
MarkTime   MarkTime+3600  MarkTime+3600+120
  |---------- 超时 ---------|-推后 2min-|
                            重新校准
```

---

## 关键函数索引

| 函数 | 位置 | 作用 |
|------|------|------|
| `alias.setmpLimitedMark()` | `michen_alias.lua` ~3928 | 主动周期重置，每 10 秒执行 |
| `alias.setmpReCountTime()` | `michen_alias.lua` ~3968 | 反应式 MP 周期重置，获得 EXP 时触发 |
| `alias.setybReCountTime()` | `michen_alias.lua` ~3990 | 反应式 YB 周期重置 |
| `alias.setWarReCountTime()` | `michen_alias.lua` ~3999 | 反应式 War 周期重置 |
| `alias.checkexp(type)` | `michen_alias.lua` ~651 | 向游戏发送 EXP 检测请求 |
| `mpLimited.stat()` | `always.lua` ~1 | 检测 MP 周期是否有效（被注释的旧方案） |
| `always_watch.timer10()` | `always.lua` ~723 | 10 秒定时器，驱动主动重置 |
| `check.lua` w[3] 分支 | `check.lua` ~1024 | EXP 累积入口 |
| `tell_job()` | `LPC/ftb_zhu.c` ~420 | FTB 任务完成处理（奖励计算 + adjust_rate + time_rewards） |
| `adjust_rate()` | `LPC/ftb_zhu.c` ~291 | FTB 动态 exp_limit 衰减/恢复 |
| `time_rewards()` | `LPC/jobd.c` ~140 | 服务端奖励计算引擎（FTB 使用 / 其他任务不使用） |
| `event_rewards()` | `LPC/jobd.c` ~280 | 服务端事件型奖励计算引擎（MP/YB/War 使用） |

## 注意事项

1. **mpexp 清零 ≠ MarkTime 更新**：定时器清零 mpexp 时不会更新 MarkTime。MarkTime 只在 `setmpReCountTime/setybReCountTime/setWarReCountTime` 中更新（即**获得新周期第一笔 EXP 时**）。
2. **War 的主动检测用 3400s** 而非 3600s，比其他任务提前 200s 重置，推测是为了 War 组队调度的提前量。
3. **丐帮有独立周期**：beg1/beg2 各自独立计时和上限，但 `mpexp` 也会同步累积（用于 `mpLimited.stat()` 检测）。
4. **白驼 (bt) 额外清理**：当 `mpJobLimited` 被重置时，bt 门派还会额外清零 `xy.xyLimited`、`xy.limitedResume`、`xy.biteLimited`，并重置 `xy.checkfirstbite=1`。
5. **雪山 (xs) 额外清零**：游戏消息"烧了太多尸体"时直接 `mpexp=0; mpJobLimited=1`。
6. **FTB 不参与 MarkTime 周期**：FTB 的 EXP 衰减由服务端 `adjust_rate()` 控制（每次做完 ~1%+random），与 MP/YB/War 的 3600s MarkTime 周期完全无关。
7. **FTB 的 `time_rewards` 有二次上限截断**：即使 `adjust_rate()` 的 exp_limit 还没降到底，`time_rewards` 内部的 1 小时 `JOBD/pub_ftb/exp` 累积也可能导致 grant=0。
8. **FTB 的 `/start` 是假的**：`tell_job()` 调用 `time_rewards` 前硬编码设置为 `time()-60`，不反映真实任务耗时。`time_ratio` 始终 ≈ 1/60。
