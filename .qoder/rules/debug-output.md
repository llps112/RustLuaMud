---
trigger: always_on
---
# 调试输出规范

## 核心原则

`print` 和 `Note` **都会写入日志文件**，两者在可追溯性上没有任何区别。

| 输出方式 | 终端 | 日志文件 | 参数处理 |
|---------|------|---------|---------|
| `Note("msg")` | ✅ 前台显示，带 `[Lua]` 前缀 | ✅ 写入 | 单个字符串参数 |
| `print("msg")` | ✅ 同上 | ✅ 写入 | 多参数用 `\t` 连接 |

两者共用同一条落盘链路：`print`/`Note` → 引擎 `pending_logs`（`src/lua/api.rs:358-414`）→ `drain_lua_logs` 逐条写文件（`src/app/session.rs:682-692`）。写入时按**消息前缀**分类，与用哪个函数无关：

| 消息前缀 | 日志标签 |
|---|---|
| `[DEBUG`、`[GPS`、`[GPS-MATCH]` | `[DBG]` |
| 其他 | `[OUT]` |

> 本规则曾写着「`print` 不写入日志文件」，那是错的——实测两者进的是同一个 `pending_logs` 队列。这个错误结论会让人在排障时误判 `print` 的输出查不到，从而漏看已有线索。

**仍然推荐用 `Note`**，但理由不是落盘差异，而是：与 MUSHclient API 保持一致、单参数不会被 `\t` 拼接、便于统一加 `[DEBUG 模块_函数]` 前缀从而落到 `[DBG]` 标签。

## DEBUG 输出规范

1. **唯一前缀**：每步 DEBUG 输出应有唯一标记前缀，方便 grep 过滤。格式：
   ```lua
   Note("[DEBUG 模块名_函数名] 具体描述")
   ```

2. **分层输出**：在关键节点逐步输出，不要一次性全部打印。典型排查链路：
   - `触发器被触发`
   - `关键参数值`（如 `l=`, `w[1]=`, `col=`）
   - `API 调用结果`（如 `GetStyle 返回`, `pcall ok=false`）
   - `错误详情`（错误信息、nil 字段等）

3. **调试完成清理**：确认修复后必须清除所有 `[DEBUG]` 输出，**单独提交**清理 commit，不与功能修改混在一起。

### 字符串拼接规范

**禁止在 `print`/`Note` 中用逗号分隔多参数**（`print` 的多参数由 `src/lua/api.rs:401` 的 `parts.join("\t")` 拼接，会插入制表符，间距过大）。**优先用 `string.format` 替代 `..` 拼接**：

```lua
-- ❌ 差：逗号分隔导致制表符间距过大
print("hp:", hp.qi, "/", hp.maxqi)

-- ❌ 一般：多次 .. 拼接产生中间临时字符串
print("hp: "..hp.qi.."/"..hp.maxqi)

-- ✅ 好：string.format 一次成型，无中间变量，紧凑美观
print(string.format("hp: %s/%s", hp.qi, hp.maxqi))
```

`string.format` 的优势：
- 无中间临时字符串，GC 友好
- 格式整齐，一眼可读
- 和 C 的 `printf` 习惯一致，迁移成本低

## Lua debug 模块不可用

**Lua `debug` 库未加载**，禁止使用 `debug.traceback()`、`debug.getinfo()` 等函数，会报错：
```
attempt to index global 'debug' (a nil value)
```

### 替代方案：`pcall(error, ..., 3)` 获取调用位置

Lua 的 `error(msg, level)` 内置了调用位置标注功能（不依赖 `debug` 库），`level` 参数指定向上追溯的调用层级。搭配 `pcall` 捕获错误消息，即可定位调用者：

```lua
function alias.checkche()
    local _, err = pcall(error, "", 3)
    Note("[DEBUG checkche] 被调用, "..tostring(err):gsub("%s+$", ""))
    -- ...
end
```

输出示例：
```
[DEBUG checkche] 被调用, [string "scripts/class/michen_yb.lua"]:567:
```

`level=3` 的溯源逻辑：`pcall(error, "", 3)` 中，`error` 内部 → 第1层 → `pcall`（C 函数跳过） → 第2层 → 当前函数（如 `checkche`） → 第3层 → **调用者位置**（目标）。

与 `caller` 参数标记法相比，此方案：
- 无需修改每个调用点传参
- 对闭包/协程友好（`wait.time` 延迟调用也能正确显示）
- 适合临时 debug，清理时只需删 `checkche` 内的两行即可
