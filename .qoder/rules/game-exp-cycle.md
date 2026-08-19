---
trigger: model_decision
description: "游戏 EXP/YB/War 周期机制规则，涉及经验计算、押镖收益、战斗周期、门派任务收益判断时加载"
---
# 游戏 EXP 周期规则

## 关键约束

- MP/YB/War 共用 MarkTime 3600s 周期机制，每获得一次 EXP 重置计时器
- **FTB 不走 MarkTime 周期**，由服务端 `adjust_rate()` 动态衰减控制
- War 主动检测用 3400s（提前 200s），其他任务用 3600s
- `mpexp` 清零 ≠ `MarkTime` 更新：定时器清零 mpexp 时不更新 MarkTime，MarkTime 只在反应式重置中更新
- 丐帮 beg1/beg2 各自独立周期，但 `mpexp` 也同步累积

## 详细文档

完整的 EXP 周期机制、衰减曲线、函数索引详见 `docs/game-exp-cycle.md`。
