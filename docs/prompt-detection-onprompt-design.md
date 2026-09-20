# OnPrompt 提示检测（输出落定信号）设计规划

> **状态：规划中，尚未实施。** 本文档为设计方案预案，实施前请勿据此改动代码。
> 起因：与 blightmud / TinTin++ 的对比分析中，识别出「输出落定（prompt settled）信号」是
> 对本项目无人值守可靠性收益最高、成本最低的一项可借鉴机制，故单列此规划。

## 1. 背景与目标

### 1.1 问题

脚本层的流程编排目前大量依赖 `wait.time` 这类**基于定时的盲等待**：发出一条命令后，
猜测一个固定的毫秒数再发下一条。这在网络抖动、服务器响应快慢不一时脆弱——
等待过短会在服务器仍在输出时抢先发命令（导致命令时序错乱甚至分支死循环），
等待过长则拖慢挂机吞吐。

### 1.2 目标

提供一个**确定性的「服务器本轮输出已落定、正在等待输入」信号**，暴露为 Lua 回调，
让脚本从「猜时间」转向「等信号」。参考对象：TinTin++ 的 telnet GA（Go Ahead）感知，
以及其在无 GA 服务器上的空闲计时兜底。

### 1.3 非目标

- 不实现 GMCP / MSDP / MXP 等子协商协议（详见 `ROADMAP.md` §0「明确不做」）。
- 不改动重连 / 连接代际 / 取消通道这套敏感逻辑，OnPrompt 只寄生在已有的
  `last_recv_time` 与每会话 tick 之上。

## 2. 现状盘点（代码落点）

| 能力 | 现状 | 位置 |
|------|------|------|
| 每会话「最后收到数据时间」跟踪 | ✅ 已存在 `last_recv_time`，每收到一行数据即刷新 | `src/app/events.rs`（`handle_manager_event` 的 `Data` 分支） |
| 每会话周期性 tick | ✅ 已存在心跳空闲检测块，且可拿到 `lua_engine` | `src/app/session.rs`（约 L569 起的心跳检测） |
| Lua 生命周期回调范式 | ✅ `OnConnect` / `OnDisconnect` 已确立「默认空函数 + 脚本覆盖」模式 | `src/lua/api/variables.rs`（约 L201/L280）、`notify_disconnect` |
| telnet IAC 处理 | ⚠️ `strip_telnet_iac` 是**按行**过滤、丢弃所有协商；GA（`IAC GA` = `FF F9`）被「其他 IAC 跳 2 字节」分支吞掉 | `src/connection/session.rs`（约 L167-206） |
| 脚本层「挂起-唤醒」范式 | ✅ `wait.time`/`wait.regexp` 用协程挂起流程，临时 timer/trigger 回调里 `coroutine.resume(thread)` 唤醒 | `scripts/lua/wait.lua`（`timer_resume` L126 / `trigger_resume` L162 / `wait.time` L218） |

**关键约束发现**：GA 通常在提示行的 `\r\n` **之后单独到达、自身不带行分隔符**。而当前读循环是
「逐字节攒到 `\n`/`\r` 成一行再处理」，因此孤立的 `IAC GA` 会滞留在 `byte_buf` 中，直到下一行到达
才被处理。故 GA 探测**必须在读循环内做字节级检测，不能塞进按行的 `strip_telnet_iac`**。

## 3. 设计方案

### 3.1 统一出口

两条触发路径（idle 计时 / GA 快速路径）收敛到**同一个 Lua 回调**，脚本不关心是谁触发的：

```lua
-- 新增全局回调，与 OnConnect / OnDisconnect 同级，默认空函数可被脚本覆盖
OnPrompt = function(source)
    -- source: "idle"（计时兜底） | "goahead"（telnet GA 快速路径）
    -- 语义：服务器本轮输出已落定、正在等待输入，可安全发出下一条命令
end
```

> 脚本侧如何**用**这个信号（把盲等的 `wait.time` 换下来）见 §3.6 `wait.prompt()`——
> 它是把本回调接入现有协程流程的必要一环，与 Phase 1 一同交付。

### 3.2 Phase 1 —— idle-settled（GA 无关，可先行落地）

复用现成的 `last_recv_time` + 每会话 tick，**不新增跨重连状态**，稳定性风险最低。

- **会话新增字段**（`src/connection/session.rs`，紧邻 `last_recv_time`）：
  ```rust
  pub prompt_idle_ms: u64,   // 判定阈值；0 = 禁用。建议默认 ~200ms
  pub prompt_settled: bool,  // 本次静默期是否已触发过，防重复
  ```
- **`Data` 分支复位闩**（`src/app/events.rs`，收到新数据即「又忙起来」）：
  ```rust
  session.last_recv_time = std::time::Instant::now();
  session.heartbeat_sent = None;
  session.prompt_settled = false;   // 新增
  ```
- **tick 内检测**（`src/app/session.rs`）：判据抽成 `Session::prompt_should_fire(now)` 纯函数（便于单测）；`notify_prompt` 需 `&mut self`，故在块表达式内 `get_mut_by_id` 收敛可变借用后派发。**关键：必须在回调后排空命令/原始包/日志**（与 `OnConnect`/`perform_reconnect` 同范式）——`OnPrompt` 的典型用途是唤醒协程后立即 `Send` 下一条命令，若漏排空，`pending_commands` 残留会让下一 tick 的 `fire_due_timers` 触发 `debug_assert`（dev panic），release 下与静默服务器互等死锁。且**仅在确有引擎时才置 `prompt_settled=true`**（无引擎不消耗闩，留下轮重试）：
  ```rust
  let prompt_fired = {
      match self.manager.get_mut_by_id(session_id) {
          Some(session)
              if session.prompt_should_fire(std::time::Instant::now())
                  && session.lua_engine.is_some() =>
          {
              session.prompt_settled = true;
              if let Some(ref mut engine) = session.lua_engine {
                  engine.notify_prompt("idle");   // 见 3.4
              }
              true
          }
          _ => false,
      }
  };
  if prompt_fired {
      let commands = self.manager.get_by_id(session_id)
          .and_then(|s| s.lua_engine.as_ref())
          .map(|e| e.drain_commands()).unwrap_or_default();
      if !commands.is_empty() { self.send_lua_commands(session_id, commands)?; }
      self.send_lua_raw(session_id)?;
      self.drain_lua_logs(session_id)?;
      any_fired = true;
  }
  ```
  > 闩的复位收敛到 `StateChange(Connected)` 唯一漏斗（`src/app/events.rs`），统一覆盖首连 / 手动 `/open` 复用 / 自动重连，不靠各路径单独补。

> **idle-settled 是启发式，非确定性信号**：阈值判定的是「静默了 N ms」，
> 不等于「服务器本轮输出已落定」。当服务器慢速逐行推送、行间间隔 > 阈值时，
> 会在**输出中途**误触发一次 `OnPrompt`（`prompt_settled` 闩收到新 `Data` 即复位，
> 随后还可能再触发）。这是 GA 快速路径（Phase 2）存在的根本理由——GA 才是真正的
> 确定性落定信号；若实测发现目标服发 GA，Phase 1 的阈值容错可适当放宽。

### 3.3 Phase 2 —— GA 快速路径（门控在「确认服务器发 GA」之后）

在读循环推字节处（`src/connection/session.rs` 约 L428-432）加两字节滑动窗口探测，
命中 `prev == 0xFF && cur == 0xF9` **立即**发一个不依赖行分隔符的新事件：

```rust
SessionEvent::GoAhead(generation)   // 新事件变体，携带连接代际号
```

经 `manager.rs`（约 L163 的事件转发）与 `app/events.rs`（`handle_manager_event` 的 match）
送达 app，触发 `notify_prompt("goahead")`，**与 idle 共用同一个 `prompt_settled` 闩**去重：
GA 先到则抢在 idle 之前触发，触发后 idle 不再补发——保证一次提示只回调一次。

### 3.4 引擎侧回调

`notify_prompt(source)` 照抄 `notify_disconnect` 的写法（带看门狗布防、调用 Lua 全局、缺省 no-op），
在 `src/lua/api/variables.rs` 里按 `OnDisconnect` 的模式注册默认空函数：

```rust
// OnPrompt(source) — 默认空函数，脚本可覆盖
let on_prompt_fn = lua.create_function(move |_, _source: String| Ok(()))?;
globals.set("OnPrompt", on_prompt_fn)?;
```

### 3.5 配置项

`prompt_idle_ms` 加入 `ConnectionConfig`（TOML），默认给保守值或 `0`（禁用，存量配置零影响）。
遵循凭据/开关一贯的「缺省即旧行为」原则。

### 3.6 脚本层消费：`wait.prompt()` 协程原语（Phase 1 必带，否则无法落地）

**引擎侧 `OnPrompt` 只是必要条件。** 现有脚本流程是**协程式命令式**的：`wait.time` 造临时
定时器挂起当前协程，回调里 `coroutine.resume(thread)` 唤醒（见 §2 `wait.lua` 行）。若只暴露
一个全局回调，把成片的 `wait.time(500)` 改成信号驱动，要么逐点重写为回调风格（大改），要么
降级为轮询同步查询——两者都偏离「低成本替换盲等」的初衷。

因此 Phase 1 交付范围**必须包含一个与 `wait.time` 同构的脚本层原语** `wait.prompt()`，
它直接复用 `wait` 的挂起-唤醒机制：

```lua
-- 挂起当前协程，直到 OnPrompt 触发时被 resume；可选超时兼底防死等
function wait.prompt(timeout_sec)
    -- 1) 把当前 coroutine 登记到一个待唤醒表（同 wait.time 的 threads 结构）
    -- 2) 若传 timeout_sec，额外挂一个 wait.time 同形态的兑底 timer，超时则视为未落定直接返回
    -- 3) coroutine.yield() 挂起，等 OnPrompt 里 resume
end

-- 全局回调只负责唤醒挂起的协程（与 wait.timer_resume 同构）
OnPrompt = function(source)
    wait.prompt_resume(source)   -- 唤醒登记在表的线程；无登记则 no-op
end
```

要点：
- `wait.prompt_resume` / 待唤醒表均为 `wait.lua` 局部实现，**不新增引擎 API**；引擎只负责在
  正确时机调用全局 `OnPrompt`。
- `OnPrompt` 默认 no-op、且可能被多个挂起者依赖，resume 逻辑需 `pcall` 兜底，避免单一挂起流程
  报错中断其他唤醒（对齐项目无人值守的 埋点/回调 鲁棒性惯例）。
- 无挂起者时 `OnPrompt` 必须安全空转（登录握手期、无脚本覆盖、realtime 会话等场景）。
- 同步查询版 `IsPromptSettled()`（旧未决事项 3）仍为可选补充，适合不方便挂协程的流程；
  但不作为主路径，不阻塞 Phase 1 落地。

## 4. 与 GBK / 协议层重构的关系

- OnPrompt **不需要** telnet 协商状态机这个大改造作为前置：Phase 1 完全寄生在既有 tick 上。
- Phase 2 的字节级 GA 探测是「协商层识别 IAC」的**最小切片**，与未来可能做的
  GBK/UTF-8 解码解耦（协商层产字节流 + 按 `encoding` 分派的增量解码器）方向一致，
  但不构成对该重构的依赖。
- 性能：两条路径均为 O(1) 时间戳比较 / 单字节滑动窗口，在本项目 ≤10 连接、约 300 行/秒的量级下可忽略。

## 5. 实施顺序与验证

1. **先做 Phase 1**：不依赖任何未知事实，立刻可把脚本盲等替换为有锚点的等待。
   交付含两部分：引擎侧 idle 触发 + `notify_prompt`，以及脚本侧 `wait.prompt()` 原语（§3.6）——
   二者缺一，「替换盲等」就无法落地。
2. **GA 探测（Phase 2）前，先核实目标服是否发 GA**——普通日志看不到（`0xFF` 已被 strip），
   需用原始字节探测：起一个 raw TCP 客户端，抓取登录并触发一次命令后的字节流，检查提示行之后
   是否出现 `FF F9`。
   - 抓到 → 铺 Phase 2，GA 提供比 idle 更快更准的落定信号。
   - 抓不到 → 只上 Phase 1，`OnPrompt` 恒以 `"idle"` 触发；脚本逻辑不变（它本就不该关心 `source`），
     将来服若开了 GA 再补 Phase 2 也无感。

## 6. 工作量与风险

| 阶段 | 工作量（含测试） | 风险 | 说明 |
|------|-----------------|------|------|
| Phase 1（idle-settled） | ~0.5–1 人日（引擎）+ ~0.5 人日（脚本 `wait.prompt()`） | 低 | 引擎侧复用 `last_recv_time` + tick，不新增跨重连状态；脚本侧原语与 `wait.time` 同构 |
| Phase 2（GA 快速路径） | ~0.5 人日 + 一次实测 | 中 | 改到热读路径，须保持重连/代际/cancel 逻辑不变；去重靠共享闩 |

**注意事项**：

- 属 Rust 改动，实施时按项目规范走 `cargo fmt` → `cargo clippy -D warnings` → `cargo nextest run`，
  并为 `notify_prompt`、GA 字节探测、`prompt_settled` 去重各补单元测试。
- **`wait.prompt()` 属 Lua 脚本层改动（在 `wait.lua`）**：`scripts/lua/` 是**公共库，无 GBK 孪生**（`hooks/pre-commit` 的 `iconv` 同步仅覆盖 `class-utf8/*.lua → class/*.lua`，不涉及 `lua/`），**不需也不应**对它跑 `iconv`。真正的约束是主仓库 `scripts/lua/wait.lua` 与子模块 `scripts/private/lua/wait.lua` 两份**逐字节同步**（`cp` + `cmp` 校验）。纯脚本改动**不跑 cargo**。
- OnPrompt 在登录握手期、无脚本覆盖（默认 no-op）、realtime 会话等场景下须安全无副作用。
- 严禁把 idle 阈值配得接近限速/心跳节奏，避免与 flood 保护、心跳检测相互干扰。

## 7. 未决事项

1. 目标服是否发送 `IAC GA` —— 需实测确认（决定 Phase 2 是否有意义）。
2. `prompt_idle_ms` 默认值取值 —— 需在真实网络往返下标定（过小会误报「未落定」，过大拖慢吞吐）。
3. ~~是否向脚本层同时提供「主动查询当前是否已落定」的同步 API（如 `IsPromptSettled()`）~~
   —— **已定**：主路径走 §3.6 `wait.prompt()` 协程原语（回调驱动，与 `wait.time` 同构）；
   `IsPromptSettled()` 降为可选补充，供不便挂协程的流程使用，不阻塞 Phase 1。
