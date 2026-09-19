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
- **tick 内检测**（`src/app/session.rs`，与心跳检测同块，此处已能拿到 `engine`）：
  ```rust
  if session.prompt_idle_ms > 0
      && session.state == SessionState::Connected
      && !session.prompt_settled
      && session.last_recv_time.elapsed()
          >= std::time::Duration::from_millis(session.prompt_idle_ms)
  {
      session.prompt_settled = true;
      if let Some(engine) = session.lua_engine.as_ref() {
          engine.notify_prompt("idle");   // 见 3.4
      }
  }
  ```

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

## 4. 与 GBK / 协议层重构的关系

- OnPrompt **不需要** telnet 协商状态机这个大改造作为前置：Phase 1 完全寄生在既有 tick 上。
- Phase 2 的字节级 GA 探测是「协商层识别 IAC」的**最小切片**，与未来可能做的
  GBK/UTF-8 解码解耦（协商层产字节流 + 按 `encoding` 分派的增量解码器）方向一致，
  但不构成对该重构的依赖。
- 性能：两条路径均为 O(1) 时间戳比较 / 单字节滑动窗口，在本项目 ≤10 连接、约 300 行/秒的量级下可忽略。

## 5. 实施顺序与验证

1. **先做 Phase 1**：不依赖任何未知事实，立刻可把脚本盲等替换为有锚点的等待。
2. **GA 探测（Phase 2）前，先核实目标服是否发 GA**——普通日志看不到（`0xFF` 已被 strip），
   需用原始字节探测：起一个 raw TCP 客户端，抓取登录并触发一次命令后的字节流，检查提示行之后
   是否出现 `FF F9`。
   - 抓到 → 铺 Phase 2，GA 提供比 idle 更快更准的落定信号。
   - 抓不到 → 只上 Phase 1，`OnPrompt` 恒以 `"idle"` 触发；脚本逻辑不变（它本就不该关心 `source`），
     将来服若开了 GA 再补 Phase 2 也无感。

## 6. 工作量与风险

| 阶段 | 工作量（含测试） | 风险 | 说明 |
|------|-----------------|------|------|
| Phase 1（idle-settled） | ~0.5–1 人日 | 低 | 复用 `last_recv_time` + tick，不新增跨重连状态 |
| Phase 2（GA 快速路径） | ~0.5 人日 + 一次实测 | 中 | 改到热读路径，须保持重连/代际/cancel 逻辑不变；去重靠共享闩 |

**注意事项**：

- 属 Rust 改动，实施时按项目规范走 `cargo fmt` → `cargo clippy -D warnings` → `cargo nextest run`，
  并为 `notify_prompt`、GA 字节探测、`prompt_settled` 去重各补单元测试。
- OnPrompt 在登录握手期、无脚本覆盖（默认 no-op）、realtime 会话等场景下须安全无副作用。
- 严禁把 idle 阈值配得接近限速/心跳节奏，避免与 flood 保护、心跳检测相互干扰。

## 7. 未决事项

1. 目标服是否发送 `IAC GA` —— 需实测确认（决定 Phase 2 是否有意义）。
2. `prompt_idle_ms` 默认值取值 —— 需在真实网络往返下标定（过小会误报「未落定」，过大拖慢吞吐）。
3. 是否向脚本层同时提供「主动查询当前是否已落定」的同步 API（如 `IsPromptSettled()`），
   供不方便用回调的流程使用 —— 待定。
