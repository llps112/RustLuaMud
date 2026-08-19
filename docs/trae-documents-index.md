# Trae IDE 历史文档索引

> ⚠️ 注意：这些文档是 Trae IDE 时期产出的技术方案记录，部分代码引用和行号可能已过时，仅供参考，以实际代码为准。

本文档对 `.trae/documents/` 目录下的所有技术方案文档进行分类索引，便于快速查找历史设计决策。

---

## 一、命令限速与服务器通信（14 篇）

与命令发送限速、令牌桶/漏桶算法、滑动窗口、服务器响应监控相关的方案。

| 文档 | 摘要 | 状态 |
|------|------|------|
| [lua_cmd_ratelimit_optimization.md](.trae/documents/lua_cmd_ratelimit_optimization.md) | Lua 侧命令限速优化方案（初版） | `已废弃` |
| [lua_cmd_ratelimit_optimization_v2.md](.trae/documents/lua_cmd_ratelimit_optimization_v2.md) | 命令限速优化 v2：补偿等待机制 | `已废弃` |
| [lua_cmd_ratelimit_optimization_v3.md](.trae/documents/lua_cmd_ratelimit_optimization_v3.md) | 命令限速优化 v3：最终方案 | `已实施` |
| [sliding-window-rate-limiter.md](.trae/documents/sliding-window-rate-limiter.md) | 滑动窗口限速算法方案 | `已废弃` |
| [漏桶算法替换滑动窗口.md](.trae/documents/漏桶算法替换滑动窗口.md) | 从滑动窗口回归漏桶算法的决策 | `已废弃` |
| [fix-leaky-bucket-capacity.md](.trae/documents/fix-leaky-bucket-capacity.md) | 漏桶容量修复方案 | `已实施` |
| [补偿等待限速方案.md](.trae/documents/补偿等待限速方案.md) | 补偿等待限速策略详细设计 | `已实施` |
| [命令限速重构实施方案.md](.trae/documents/命令限速重构实施方案.md) | 命令限速迁移至 Rust 令牌桶的完整方案 | `已实施` |
| [burst_count_validation_report.md](.trae/documents/burst_count_validation_report.md) | burst 计数验证报告 | `已实施` |
| [server-watch-probe-no-drop.md](.trae/documents/server-watch-probe-no-drop.md) | server_watch 探测不丢包方案 | `已实施` |
| [michen_system_server_unresponsive_solution.md](.trae/documents/michen_system_server_unresponsive_solution.md) | 服务器无响应问题解决方案 | `已实施` |
| [halt-insertion-rate-fix.md](.trae/documents/halt-insertion-rate-fix.md) | 命令插入速率修复 | `已实施` |
| [wart-eam-bypass-server-watch-plan.md](.trae/documents/wart-eam-bypass-server-watch-plan.md) | teamwith 指令绕过 server_watch 方案 | `已实施` |
| [最佳优化方案_Execute直发+0.7s等待.md](.trae/documents/最佳优化方案_Execute直发+0.7s等待.md) | Execute 直发 + 等待的最优组合方案 | `已实施` |

---

## 二、连接与重连（5 篇）

与连接延迟、断线重连、冻结修复相关的方案。

| 文档 | 摘要 | 状态 |
|------|------|------|
| [connect-delay-plan.md](.trae/documents/connect-delay-plan.md) | 连接建立后延迟发送命令方案 | `已实施` |
| [connect-delay-reconnect-fix.md](.trae/documents/connect-delay-reconnect-fix.md) | connect_delay 重连修复 | `已实施` |
| [fix-reconnect-freeze.md](.trae/documents/fix-reconnect-freeze.md) | 重连冻结问题修复方案 | `已实施` |
| [fix-checkbili-jinhua-reconnect.md](.trae/documents/fix-checkbili-jinhua-reconnect.md) | 比力/进化重连修复 | `已实施` |
| [yb-disconnect-recovery-optimization.md](.trae/documents/yb-disconnect-recovery-optimization.md) | 押镖断线恢复优化 | `已实施` |

---

## 三、Lua 脚本架构与重构（12 篇）

与命名空间重构、别名拆分、工作流重构、全局变量收编相关的方案。

| 文档 | 摘要 | 状态 |
|------|------|------|
| [lua-global-variable-namespace-refactor.md](.trae/documents/lua-global-variable-namespace-refactor.md) | Lua 全局变量命名空间重构方案（总纲） | `迭代中` |
| [裸全局函数命名空间化实施方案.md](.trae/documents/裸全局函数命名空间化实施方案.md) | 裸全局函数收编到命名空间的实施方案 | `迭代中` |
| [裸全局变量残留收编实施方案.md](.trae/documents/裸全局变量残留收编实施方案.md) | 裸全局变量残留收编方案 | `迭代中` |
| [变量命名空间化重构-修复与验证计划.md](.trae/documents/变量命名空间化重构-修复与验证计划.md) | 命名空间化重构的修复与验证 | `迭代中` |
| [michen_alias_split_plan.md](.trae/documents/michen_alias_split_plan.md) | michen_alias.lua 拆分方案 | `迭代中` |
| [workflow-refactor-plan.md](.trae/documents/workflow-refactor-plan.md) | 工作流重构方案 | `迭代中` |
| [任务调度框架双轨并行重构方案.md](.trae/documents/任务调度框架双轨并行重构方案.md) | 任务调度框架双轨并行重构 | `迭代中` |
| [lua-refactor-deep-review-plan.md](.trae/documents/lua-refactor-deep-review-plan.md) | Lua 重构深度审查计划 | `迭代中` |
| [check-exp-refactor-plan.md](.trae/documents/check-exp-refactor-plan.md) | checkexp 重构方案 | `迭代中` |
| [replace-skillslist-de_bug-globals.md](.trae/documents/replace-skillslist-de_bug-globals.md) | 替换 skillslist/de_bug 等全局变量 | `已实施` |
| [group-HashMap-trigger-索引重构实施方案.md](.trae/documents/group-HashMap-trigger-索引重构实施方案.md) | trigger 的 group HashMap 索引重构 | `已实施` |
| [清理注释代码分析计划.md](.trae/documents/清理注释代码分析计划.md) | 清理注释代码的分析计划 | `已实施` |

---

## 四、游戏任务逻辑（13 篇）

与乞讨、护镖、押镖、门派任务等具体游戏玩法相关的方案。

| 文档 | 摘要 | 状态 |
|------|------|------|
| [beg2优先策略实施方案.md](.trae/documents/beg2优先策略实施方案.md) | 丐帮 beg2 优先策略实施 | `已实施` |
| [ftb-auto-skip-plan.md](.trae/documents/ftb-auto-skip-plan.md) | FTB 自动跳过方案 | `已实施` |
| [ftb-task-improvement.md](.trae/documents/ftb-task-improvement.md) | FTB 任务改进方案 | `已实施` |
| [fix-fj-false-alarm-loop.md](.trae/documents/fix-fj-false-alarm-loop.md) | 护镖误报警循环修复 | `已实施` |
| [fj-重名房间抓取bug修复方案.md](.trae/documents/fj-重名房间抓取bug修复方案.md) | 护镖重名房间抓取 bug 修复 | `已实施` |
| [desert-maze-yb-fix.md](.trae/documents/desert-maze-yb-fix.md) | 沙漠迷宫押镖修复 | `已实施` |
| [hs-yb-quick-submit-plan.md](.trae/documents/hs-yb-quick-submit-plan.md) | 华山押镖快速提交方案 | `已实施` |
| [yb-guohe-quote-fix.md](.trae/documents/yb-guohe-quote-fix.md) | 押镖过河引号修复 | `已实施` |
| [yb-recovery-final-sync.md](.trae/documents/yb-recovery-final-sync.md) | 押镖恢复最终同步 | `已实施` |
| [ybjob-ybroom-analysis.md](.trae/documents/ybjob-ybroom-analysis.md) | 押镖任务房间号分析 | `已实施` |
| [押镖房间号丢失修复方案.md](.trae/documents/押镖房间号丢失修复方案.md) | 押镖房间号丢失修复 | `已实施` |
| [mp_dl_fanhua_overflow_plan.md](.trae/documents/mp_dl_fanhua_overflow_plan.md) | 大理门派任务返回溢出方案 | `已实施` |
| [mp_dl_fanhua_overflow_v2.md](.trae/documents/mp_dl_fanhua_overflow_v2.md) | 大理返回溢出方案 v2 | `已实施` |
| [fix-dl-getMpCycleData.md](.trae/documents/fix-dl-getMpCycleData.md) | 大理门派周期数据获取修复 | `已实施` |
| [华山交令两波策略实施方案.md](.trae/documents/华山交令两波策略实施方案.md) | 华山交令两波策略 | `已实施` |
| [fix-xxpfm-recognition.md](.trae/documents/fix-xxpfm-recognition.md) | 星星派 FM 识别修复 | `已实施` |

---

## 五、Rust 引擎架构（8 篇）

与 engine.rs 拆分、定时器优化、panic 处理、session 管理相关的方案。

| 文档 | 摘要 | 状态 |
|------|------|------|
| [engine-rs-拆分与panic防御清理实施方案.md](.trae/documents/engine-rs-拆分与panic防御清理实施方案.md) | engine.rs 拆分与 panic 防御清理 | `已实施` |
| [timer-optimization-refactor.md](.trae/documents/timer-optimization-refactor.md) | 定时器系统优化重构 | `已实施` |
| [timer-followup-improvements.md](.trae/documents/timer-followup-improvements.md) | 定时器后续改进 | `已实施` |
| [lua-timer-watchdog-plan.md](.trae/documents/lua-timer-watchdog-plan.md) | Lua 定时器看门狗方案 | `已实施` |
| [panic-log-capture-plan.md](.trae/documents/panic-log-capture-plan.md) | panic 日志捕获方案 | `已实施` |
| [fix-session-id-vec-shift-bug.md](.trae/documents/fix-session-id-vec-shift-bug.md) | session ID vec shift bug 修复 | `已实施` |
| [timing_misalignment_optimization.md](.trae/documents/timing_misalignment_optimization.md) | 定时器时间对齐优化 | `已实施` |
| [war-start-timer-flood-fix.md](.trae/documents/war-start-timer-flood-fix.md) | war_start_timer 洪水修复 | `已实施` |

---

## 六、UI 与终端渲染（8 篇）

与浮动面板、行换行、输入处理、渲染优化相关的方案。

| 文档 | 摘要 | 状态 |
|------|------|------|
| [floating-stat-panel.md](.trae/documents/floating-stat-panel.md) | 浮动统计面板实现方案 | `已实施` |
| [panel-buttons-implementation.md](.trae/documents/panel-buttons-implementation.md) | 面板按钮交互实现方案 | `已实施` |
| [sc1-dark-gold-panel-color.md](.trae/documents/sc1-dark-gold-panel-color.md) | SC1 暗金色面板配色 | `已实施` |
| [line-wrapping-implementation.md](.trae/documents/line-wrapping-implementation.md) | 长行自动换行实现方案 | `已实施` |
| [backspace-input-handling-plan.md](.trae/documents/backspace-input-handling-plan.md) | 退格键输入处理方案 | `已实施` |
| [render_interval_realtime_separation.md](.trae/documents/render_interval_realtime_separation.md) | 渲染间隔与实时渲染分离 | `已实施` |
| [stat-table-enhancement.md](.trae/documents/stat-table-enhancement.md) | 统计表增强 | `已实施` |
| [omit_from_output_implementation.md](.trae/documents/omit_from_output_implementation.md) | omit_from_output 文本过滤实现 | `已实施` |

---

## 七、Lua 日志与渲染（4 篇）

与 Lua 日志节流、一致性修复相关的方案。

| 文档 | 摘要 | 状态 |
|------|------|------|
| [lua-log-throttle-fix.md](.trae/documents/lua-log-throttle-fix.md) | Lua 日志节流修复（初版） | `已废弃` |
| [lua-log-throttling-fix.md](.trae/documents/lua-log-throttling-fix.md) | Lua 日志节流修复（迭代版） | `已废弃` |
| [fix-lua-log-throttle-consistency.md](.trae/documents/fix-lua-log-throttle-consistency.md) | Lua 日志节流一致性修复 | `已实施` |
| [war_late_exit_optimization.md](.trae/documents/war_late_exit_optimization.md) | War 延迟退出优化 | `已实施` |

---

## 八、其他修复与分析（12 篇）

包括加密、多架构构建、GPS 死锁、配置兼容等各类修复方案。

| 文档 | 摘要 | 状态 |
|------|------|------|
| [script-encryption-implementation-plan.md](.trae/documents/script-encryption-implementation-plan.md) | 脚本加密实现方案 | `已实施` |
| [multi-arch-build-release-plan.md](.trae/documents/multi-arch-build-release-plan.md) | 多架构构建与发布方案 | `已实施` |
| [i686-32bit-compatibility-fix.md](.trae/documents/i686-32bit-compatibility-fix.md) | i686 32 位兼容性修复 | `已实施` |
| [gps_deadlock_fix.md](.trae/documents/gps_deadlock_fix.md) | GPS 死锁修复 | `已实施` |
| [config-compat-plan.md](.trae/documents/config-compat-plan.md) | 配置兼容性方案 | `已实施` |
| [refactor-pfm-registration.md](.trae/documents/refactor-pfm-registration.md) | PFM 注册重构 | `已实施` |
| [plan-fix-bitxo-attack-trigger.md](.trae/documents/plan-fix-bitxo-attack-trigger.md) | 白驼山攻击触发器修复 | `已实施` |
| [check_battleidle_lifecycle_optimization.md](.trae/documents/check_battleidle_lifecycle_optimization.md) | 战斗空闲检查生命周期优化 | `已实施` |
| [重构michen_system_lua漏桶算法为原始版本.md](.trae/documents/重构michen_system_lua漏桶算法为原始版本.md) | michen_system.lua 漏桶算法回退 | `已废弃` |
| [war-start-timer-flood-fix.md](.trae/documents/war-start-timer-flood-fix.md) | war_start_timer 洪水修复 | `已实施` |

---

## 文档统计

| 分类 | 数量 |
|------|------|
| 命令限速与服务器通信 | 14 |
| 连接与重连 | 5 |
| Lua 脚本架构与重构 | 12 |
| 游戏任务逻辑 | 16 |
| Rust 引擎架构 | 8 |
| UI 与终端渲染 | 8 |
| Lua 日志与渲染 | 4 |
| 其他修复与分析 | 12 |
| **总计** | **79** |

> 注：部分文档可能跨多个分类，按主要归属分类。

---

## 状态标签说明

| 标签 | 含义 |
|------|------|
| `已实施` | 方案已实现并合并到主分支 |
| `已废弃` | 方案被后续迭代替代或放弃（如 v1/v2 被 v3 替代） |
| `迭代中` | 方案正在实施或分阶段推进中（常见于命名空间重构系列） |
