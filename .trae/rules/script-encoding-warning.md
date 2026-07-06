# 脚本编码与修改规则

## 重要规则

`scripts/class/` 目录下的所有 `.lua` 文件使用 **GBK 编码**。这些文件是从 MushClient 直接拷贝的原始脚本，实际的脚本触发和执行使用这些文件。

`scripts/class-utf8/` 目录是 `scripts/class/` 的 UTF-8 编码副本，仅用于搜索查阅。

## 编码警告（GBK ≠ UTF-8）

**绝对禁止**：
- 使用 SearchReplace、Write 等工具编辑 `scripts/class/` 中的文件时，不要把文件内容当作 UTF-8 文本处理。这会导致 GBK 中文字节被替换为 `U+FFFD`（�），破坏所有包含中文的触发器正则、注释和字符串。
- 用 `Read` 查看时显示的乱码是正常的——它们是有效的 GBK 编码，Lua 引擎加载时会自动转码。

## 正确的修改流程

如需修改脚本（如修复 bug、新增功能），应按以下步骤操作：

1. **修改 `scripts/class-utf8/` 中的 UTF-8 版本**（不要直接动 `scripts/class/`）
2. **用 `iconv` 转码覆盖 GBK 版本**：
   ```bash
   iconv -f utf-8 -t gbk scripts/class-utf8/xxx.lua -o scripts/class/xxx.lua
   ```

## 往期事故

- 2026-06-09：修复 always.lua 正则时用 SearchReplace 直接编辑 GBK 文件，导致所有中文字节被 corrupt，score 触发器无法匹配中文名，`me.charname` 始终为空。

---

# MushClient API 兼容规则

## 重要规则

所有 MushClient 兼容的 API（`GetInfo`、`GetTriggerInfo`、`GetTimerInfo`、`AddTrigger`、`AddTimer`、`GetPluginInfo` 等）的 **code 映射和参数签名必须 100% 匹配官方文档**。

## 离线参考

MushClient API 完整离线参考已保存到 `help/api/mushclient-*.md`：
- `help/api/mushclient-api-index.md` — 全部函数索引
- `help/api/mushclient-GetInfo.md` — GetInfo code 映射（300+ codes）
- `help/api/mushclient-GetTriggerInfo.md` — GetTriggerInfo code 映射（含通配符）
- `help/api/mushclient-GetTimerInfo.md` — GetTimerInfo code 映射
- `help/api/mushclient-GetPluginInfo.md` — GetPluginInfo code 映射
- `help/api/mushclient-AddTrigger.md` / `AddTriggerEx.md` — AddTrigger/AddTriggerEx 参数与标志位
- `help/api/mushclient-AddTimer.md` — AddTimer 参数与标志位
- `help/api/mushclient-SetTriggerOption.md` / `SetTimerOption.md` — 选项名与值
- `help/api/mushclient-GetAliasInfo.md` — GetAliasInfo code 映射
- `help/api/mushclient-GroupFunctions.md` — EnableGroup/EnableTriggerGroup/EnableTimerGroup
- `help/api/mushclient-Variables.md` — GetVariable/SetVariable/DeleteVariable
- `help/api/mushclient-OutputFunctions.md` — Note/ColourNote/ColourTell/Send/Execute/DoAfter
- `help/api/mushclient-ListFunctions.md` — GetTriggerList/GetTimerList/GetAliasList
- `help/api/mushclient-DeleteFunctions.md` — DeleteTrigger/DeleteTimer/DeleteAlias

修改涉及 MushClient API 兼容的代码之前，**先查阅这些本地文件确认官方定义**。

仅在本地文件没有涵盖所需函数时，才访问在线页面：`http://www.mushclient.com/scripts/function.php`

## 注意事项

- `GetInfo(code)` — code 编号的含义必须严格按官方定义，不得自定义映射。
- `GetTriggerInfo(name, code)` — code 编号含义严格按官方定义（如 7=Keep evaluating, 8=Enabled, 26=Group name）。
- `GetTimerInfo(name, code)` — code 编号含义严格按官方定义（如 6=Enabled, 7=One shot, 8=At time, 14=Temporary, 19=Group name）。
- `GetPluginInfo(id, code)` — code 编号含义严格按官方定义（如 1=Name, 14=Date modified, 19=Version, 20=Directory）。
- 当未实现某个特性的返回值时，返回空串 `""`（string）或 `0`（number）或 `false`（boolean），而不是自定义其他含义。

---

# 正则引擎规则

## 两种引擎的区分

脚本中混用了两种正则引擎，转义方式完全不同：

| 注册方式 | 引擎 | 转义方式 |
|---------|------|---------|
| `AddTriggerEx`（经`linktri→addtri`注册的 trigger） | PCRE（Rust `regex` crate） | Lua 字符串中写 `\\-` → PCRE 收到 `\-` ✅ |
| `string.find`（Lua 模式匹配） | Lua 模式引擎 | Lua 字符串中写 `%-` 转义 ✅，`\-` 会被丢弃反斜杠变成量词 ❌ |
| `findstring`（自定义函数，内部用 `string.find`） | Lua 模式引擎 | 同 `string.find`，用 `%-` ✅ |

## 快速判断

看是在哪调用：
- **trigger 模式字符串**（`addtri` 的 `regexp` 参数）→ PCRE，用 `\\` 转义
- **trigger 回调内部的 `string.find`/`findstring`** → Lua 模式，用 `%` 转义

## 特别注意

PCRE 的正则模式写在 Lua 字符串中，需要**双层转义**：
- Lua 字符串层：`\\` → 实际字符 `\`
- PCRE 层：`\-` → 字面连字符

而 Lua 模式用 `%` 转义，没有双层问题。

---

# Lua 模式匹配：`[...]` 字符集与 UTF-8 多字节汉字规则

## 重要规则

**永远不要在 Lua `string.find`/`string.match` 的 `[...]` 字符集中放多字节 UTF-8 汉字。**

## 原因

Lua 的 `[...]` 是**单字节字符集**，而 UTF-8 中每个汉字占 3 字节（`E4`~`EF` 开头）。把汉字放进字符集时，实际加入的是该汉字的所有**字节**，而非整个汉字。

```lua
-- ❌ 错误：字符集包含的是字节 {E5, 9D, 97, E9, A2, 97}，不是字符 "块" 或 "颗"
string.find(l, "[> ]*(.+)给你一[颗|块]+(.+).")
-- 会吞掉后续汉字的首字节（如 "和" 的 E5）
```

```lua
-- ✅ 正确：拆成两次独立匹配，每个汉字作为完整 UTF-8 序列
a,b,c,d = string.find(l, "[> ]*(.+)给你一块(.+)。")
if not (c and d) then
    a,b,c,d = string.find(l, "[> ]*(.+)给你一颗(.+)。")
end
```

## 判断方法

| 场景 | 能否用 `[...]` | 原因 |
|------|---------------|------|
| `[abc]`、`[0-9]`、`[> ]` 等 ASCII 字符 | ✅ 可以 | ASCII 是单字节 |
| `[颗|块]`、`[些|杯]` 等多字节汉字 | ❌ **禁止** | 字节集会跨字符吞噬 |
| `[颗|块]+` 加 `+` 量词 | ❌ **尤其危险** | 贪婪匹配会吞掉相邻汉字首字节 |
| `findstring(l, "...[颗|块]+...")` | ❌ **禁止** | 同上，`findstring` 内部也是 `string.find` |

## 往期事故

- 2026-07-04：`michen_mp_gb.lua` 中 `[> ]*(.+)给你一[颗|块]+(.+).` 在匹配"给你一块和田玉"时，`[颗|块]+` 吞掉了"和"的第一字节 `E5`，导致 `d` 捕获到损坏的 UTF-8 `��田玉`，`alias.goldid` 无法识别。

---

# Git 提交规则

## 子模块独立提交

`scripts/private` 是独立子模块，主仓库不跟踪子模块版本。

**主仓库**（Rust 客户端）和 **子模块**（Lua 脚本）各自独立提交和推送，互不关联：
- 改脚本 → 只在 `scripts/private/` 里 commit + push
- 改主仓库代码 → 只在根目录 commit + push
- **禁止**在主仓库提交"更新子模块指针"这类与 Rust 代码无关的 commit

---

# MushClient API 标志位完整性规则

## 重要规则

实现 MushClient 兼容 API（`AddTrigger`、`AddTriggerEx`、`AddAlias`、`AddTimer` 等）时，**必须对照官方文档逐位检查 flags 参数的所有标志位**，不能遗漏。

## flags 标志位速查

| 标志位 | 值 | 说明 |
|--------|------|------|
| `Enabled` | 1 | 创建后立即启用 |
| `KeepEvaluating` | 8 | 匹配后继续评估后续 trigger |
| `CaseSensitive` | 16 | 区分大小写（默认开启，设 16 关闭此模式则在 Rust 端自动加 `(?i)`） |
| `RegularExpression` | 32 | 正则模式 |
| `Replace` | 1024 | 同名替换，不追加 |
| `Temporary` | 4096 | 临时 trigger，session 断开自动清除 |
| `OneShot` | 4096 | 一次性 timer（与 Temporary 同值，复用于 AddTimer 的 flags 参数） |
| `AtTime` | 65536 | 定时器到点触发 |

## 注意事项

- 每个新实现的 API 函数中，**flag 解析应完整性检查**：对已知标志位逐一处理，对未知标志位用 `// TODO` 注释记录。
- **Replace 标志（1024）是 `loadmod` 重载的正确性基石**：缺失时同名 trigger/alias 会累积，导致回调执行多次，且旧回调引用的 Lua 函数名（`addtri_XXXXX`）残留全局空间。
- 修改新增 API 前，先查阅 `help/api/mushclient-*.md` 确认参数签名和标志位定义。

---

# 调试输出规范

## 核心原则

| 输出方式 | 可见范围 | 日志文件 | 适用场景 |
|---------|---------|---------|---------|
| `Note("msg")` | 终端 ✅ | 写入 ✅ | 排障首选，可追溯 |
| `print("msg")` | 终端 ✅ | 不写入 ❌ | 仅临时终端输出，不适合排障 |

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

**禁止在 `print`/`Note` 中用逗号分隔多参数**（会插入制表符，间距过大）。**优先用 `string.format` 替代 `..` 拼接**：

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
