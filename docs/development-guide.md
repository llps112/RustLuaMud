# RustLuaMud 开发指南

> 本文档面向项目开发者，涵盖编码规范、Git 工作流、调试指南、测试指南等实践内容。

---

## 一、编码规范

### 1.1 脚本编码规则

`scripts/class-utf8/` 是**主要开发源**（UTF-8 编码），所有修改在此进行。

`scripts/class/` 是 GBK 编码的运行时版本，由 `iconv` 从 UTF-8 版本生成，游戏实际加载此目录。

**绝对禁止**：
- 使用 SearchReplace、Write 等工具编辑 `scripts/class/` 中的文件时，不要把文件内容当作 UTF-8 文本处理
- 这会导致 GBK 中文字节被替换为 `U+FFFD`（），破坏所有包含中文的触发器正则、注释和字符串

**正确的修改流程**：
1. 修改 `scripts/class-utf8/` 中的 UTF-8 版本
2. 用 `iconv` 转码覆盖 GBK 版本（用重定向，**不要用 `-o`**：Windows 的 GNU libiconv 独立版不支持该选项，Linux 的 glibc iconv 支持，重定向则两端通用）：
   ```bash
   iconv -f utf-8 -t gbk scripts/class-utf8/xxx.lua > scripts/class/xxx.lua
   ```
3. 提交时 pre-commit 钩子会自动检测并同步（需先在子模块内 `git config core.hooksPath hooks`；这是 per-machine 的 local 配置，不随仓库传播，每台机器与每次重新 clone 各需一次）

### 1.2 调试输出规范

| 输出方式 | 可见范围 | 日志文件 | 适用场景 |
|---------|---------|---------|---------|
| `Note("msg")` | 终端 ✅ | 写入 ✅ | 排障首选，可追溯 |
| `print("msg")` | 终端 ✅ | 不写入 ❌ | 仅临时终端输出 |

**必须使用 `Note()` 进行调试输出**，因为 `print()` 不写入日志文件，无法事后追溯。

### 1.3 字符串拼接规范

**禁止在 `print`/`Note` 中用逗号分隔多参数**（会插入制表符，间距过大）。**优先用 `string.format` 替代 `..` 拼接**：

```lua
-- ❌ 差：逗号分隔导致制表符间距过大
print("hp:", hp.qi, "/", hp.maxqi)

-- ❌ 一般：多次 .. 拼接产生中间临时字符串
print("hp: "..hp.qi.."/"..hp.maxqi)

-- ✅ 好：string.format 一次成型，GC 友好
print(string.format("hp: %s/%s", hp.qi, hp.maxqi))
```

### 1.4 DEBUG 输出格式

每步 DEBUG 输出应有唯一标记前缀，方便 grep 过滤：

```lua
Note("[DEBUG 模块名_函数名] 具体描述")
```

分层输出示例：
1. `触发器被触发`
2. `关键参数值`（如 `l=`, `w[1]=`, `col=`）
3. `API 调用结果`（如 `GetStyle 返回`, `pcall ok=false`）
4. `错误详情`（错误信息、nil 字段等）

**调试完成清理**：确认修复后必须清除所有 `[DEBUG]` 输出，**单独提交**清理 commit。

### 1.5 Lua debug 模块不可用

Lua `debug` 库未加载，禁止使用 `debug.traceback()`、`debug.getinfo()` 等函数。

**替代方案**：`pcall(error, "", 3)` 获取调用位置：

```lua
function alias.checkche()
    local _, err = pcall(error, "", 3)
    Note("[DEBUG checkche] 被调用, "..tostring(err):gsub("%s+$", ""))
end
```

`level=3` 的溯源逻辑：`error` 内部 → 第1层 → `pcall`（C 函数跳过） → 第2层 → 当前函数 → 第3层 → **调用者位置**。

---

## 二、Git 工作流

### 2.1 子模块独立提交

`scripts/private` 是独立子模块，主仓库和子模块各自独立提交和推送：
- 改脚本 → 只在 `scripts/private/` 里 commit + push
- 改主仓库代码 → 只在根目录 commit + push

### 2.2 子模块指针更新

子模块提交后，需要更新主仓库的子模块指针：

```bash
git add scripts/private
git commit -m "chore: 更新子模块指针"
```

**注意事项**：
- 子模块指针更新**只改 commit hash**，不涉及 Rust 代码变更
- **不需要触发 CI**（cargo fmt/clippy/test）
- **不需要发布新 release**
- commit message 统一用 `chore: 更新子模块指针`

### 2.3 war_members_data.lua 提交规则

`war_members_data.lua` 在脚本运行时会**实时写入**（成员经验值等数据持续更新）。

**提交原则**：
- 每次版本发布或功能变更时，**附带提交一次**即可
- **短时间内不要重复提交**该文件的纯数据变更
- 如果距离上次提交该文件不到 **24 小时**，且没有功能性代码变更，**跳过**该文件的提交

### 2.4 Release 发布流程

1. Bump 版本号（`Cargo.toml`）
2. `git commit` + `git push`
3. 创建 tag `vX.Y.Z` + `git push --tags`
4. `gh run list` 确认 GitHub Actions release workflow **已触发并开始构建**即完成
5. GitHub Actions 自动完成二进制编译、Release 创建和 Gitee 同步

### 2.5 michen_xkx.lua 同步规则

`scripts/private/michen_xkx.lua` 是子模块中的加载清单（**唯一源文件**），`scripts/michen_xkx.lua` 是主仓库中的本地配置副本。

**修改原则**：
- **只修改** `scripts/private/michen_xkx.lua`，**禁止**直接编辑 `scripts/michen_xkx.lua`
- 修改完成后，**必须立即同步**到 `scripts/michen_xkx.lua`：
  ```bash
  cp scripts/private/michen_xkx.lua scripts/michen_xkx.lua
  ```
- **不同步的后果**：运行时加载的本地副本缺少新增文件，对应全局变量为 nil，触发 `attempt to index global 'xxx' (a nil value)` 错误

**往期事故**：
- 2026-08-13：子模块源文件已添加 `workflow_shadow.lua`，但未同步到本地配置副本，运行时 `workflow_shadow` 全局变量为 nil，导致押镖流程中断。

### 2.6 子模块私有化规则

`scripts/private` 是**私有仓库**，主仓库是**公开仓库**。

**禁止行为**：
- 在 `bootstrap.sh`、README 或其他公开文档中引用私有仓库的 raw URL
- 在公开文档中描述私有仓库的内部结构
- 在公开代码或脚本中引用私有仓库的 clone URL
- 在 CI/CD 配置中引用私有仓库

---

## 三、代码质量检查流程

### 3.1 区分语言，避免做无用功

| 变更类型 | 需要执行的检查 |
|---------|--------------|
| **仅改 Lua 脚本** | 只需做 GBK 同步（`iconv`）。**严禁**跑 cargo 命令 |
| **仅更新子模块指针** | 按子模块指针更新规则操作 |
| **改 Rust 代码** | 执行下方三项检查 + 编译 |

### 3.2 Rust 代码完整检查流程

每次修改或新增 Rust 代码后，**在提交推送之前**必须依次执行：

#### 1. 格式化检查

```bash
cargo fmt --all -- --check
```

如果不通过，先执行 `cargo fmt --all` 自动修正。

#### 2. Clippy Lint 检查

```bash
cargo clippy -- -D warnings
```

必须**零 warning、零 error** 才允许提交。`-D warnings` 将 warning 视为 error，和 CI 行为一致。

#### 3. 测试

```bash
# 本地开发机：默认线程数
cargo nextest run

# CI 环境：限制 3 线程避免资源争抢
cargo nextest run --test-threads=3
```

必须全部通过。

### 3.3 新增代码必须附带单元测试

- **新增函数** → 同时添加该函数的单元测试（正常路径 + 边界情况）
- **修改逻辑** → 检查现有测试是否覆盖了改动的场景，没有则补充
- **Bug 修复** → 先写一个能复现该 bug 的测试，确认失败 → 修代码 → 确认测试通过
- 测试放在文件末尾的 `#[cfg(test)] mod tests { ... }` 块中

### 3.4 完整提交流程

1. 修改代码
2. 依次运行：`cargo fmt` → `cargo clippy -- -D warnings` → `cargo nextest run`
3. 全部通过后，`git add` + `git commit` + `git push`
4. Push 后确认 GitHub Actions CI 也绿色通过

---

## 四、调试指南

### 4.1 启用调试信息

```bash
export RUST_BACKTRACE=1
./RustLuaMud
```

panic 时会自动打印堆栈并写入对应连接日志文件（`[PNC]` 前缀）。

### 4.2 Lua 侧调试

使用 `Note()` 输出调试信息（会写入日志文件）：

```lua
-- 分层输出，逐步追踪
Note("[DEBUG fj_dosomething1] 触发器被触发")
Note(string.format("[DEBUG fj_dosomething1] l=%s", l))
Note(string.format("[DEBUG fj_dosomething1] w[1]=%s, w[2]=%s", tostring(w[1]), tostring(w[2])))
```

### 4.3 获取调用堆栈

由于 `debug` 库不可用，使用 `pcall(error, "", level)` 获取调用位置：

```lua
local _, err = pcall(error, "", 3)
Note("[DEBUG 函数名] 调用来源: "..tostring(err):gsub("%s+$", ""))
```

### 4.4 日志文件位置

日志文件位于 `logs/` 目录，按角色和小时分割：
- 格式：`{角色名}_{YYMMDD}_{HH}.log`
- 保留数量：默认 24 小时（可配置 `log_rotation_count`）

---

## 五、测试指南

### 5.1 运行测试

```bash
# 推荐：使用 cargo-nextest（更快，更好的输出）
cargo nextest run

# 标准方式
cargo test

# 运行特定测试
cargo nextest run test_name_filter

# CI 环境（限制线程数）
cargo nextest run --test-threads=3
```

### 5.2 测试配置

`Cargo.toml` 中的测试配置：

```toml
[profile.test]
# opt-level=1 保留调试符号，测试二进制更小、链接更快
opt-level = 1
```

### 5.3 测试覆盖要求

- 新增函数必须有单元测试
- Bug 修复必须先写复现测试
- 测试放在文件末尾的 `#[cfg(test)] mod tests { ... }` 块中
- 当前项目有 809+ 测试用例（`tests.rs` 215.5KB）

---

## 六、正则表达式注意事项

### 6.1 两种引擎的区分

脚本中混用了两种正则引擎，转义方式完全不同：

| 注册方式 | 引擎 | 转义方式 |
|---------|------|---------|
| `AddTriggerEx`（经 `linktri→addtri` 注册的 trigger） | PCRE（Rust `regex` crate） | Lua 字符串中写 `\\-` → PCRE 收到 `\-` |
| `string.find`（Lua 模式匹配） | Lua 模式引擎 | Lua 字符串中写 `%-` 转义 |
| `findstring`（自定义函数） | Lua 模式引擎 | 同 `string.find`，用 `%-` |

### 6.2 快速判断

- **trigger 模式字符串**（`addtri` 的 `regexp` 参数）→ PCRE，用 `\\` 转义
- **trigger 回调内部的 `string.find`/`findstring`** → Lua 模式，用 `%` 转义

### 6.3 PCRE 双层转义

PCRE 的正则模式写在 Lua 字符串中，需要**双层转义**：
- Lua 字符串层：`\\` → 实际字符 `\`
- PCRE 层：`\-` → 字面连字符

### 6.4 `[...]` 字符集与多字节汉字

**永远不要在 Lua `string.find`/`string.match` 的 `[...]` 字符集中放多字节 UTF-8 汉字。**

Lua 的 `[...]` 是**单字节字符集**，而 UTF-8 中每个汉字占 3 字节。把汉字放进字符集时，实际加入的是该汉字的所有**字节**，而非整个汉字。

```lua
-- ❌ 错误：字符集包含的是字节，不是字符
string.find(l, "[> ]*(.+)给你一[颗|块]+(.+).")
-- 会吞掉后续汉字的首字节

-- ✅ 正确：拆成两次独立匹配
a,b,c,d = string.find(l, "[> ]*(.+)给你一块(.+)。")
if not (c and d) then
    a,b,c,d = string.find(l, "[> ]*(.+)给你一颗(.+)。")
end
```

---

## 七、常见问题与历史事故

### 7.1 编码事故（直接编辑 GBK 文件导致中文损坏）

**日期**：2026-06-09

**事件**：修复 `always.lua` 正则时用 SearchReplace 直接编辑 GBK 文件，导致所有中文字节被 corrupt。

**后果**：score 触发器无法匹配中文名，`me.charname` 始终为空。

**教训**：永远不要直接编辑 `scripts/class/` 中的 GBK 文件。所有修改必须在 UTF-8 源文件中进行，然后通过 `iconv` 转码。

### 7.2 中文引号被静默转换

**日期**：2026-07-07

**事件**：`michen_yb.lua` 中的中文双引号 `"` `"` (U+201C/U+201D) 被编辑工具静默"归一化"为英文双引号 `"` (U+0022)。

**后果**：`findstring` 中匹配的文本必须精确对应游戏服务器返回的原文。trigger 模式字符串使用中文双引号，而回调中被改成英文双引号，导致 `eat guoheout` 未执行，角色在船上发呆直到掉线。

**教训**：编辑 `.lua` 文件中的中文文本时，务必用 `hexdump` 或 `python3 -c "print(repr(open('file').readlines()[N]))"` 验证 Unicode 字符是否被工具静默转换。肉眼检查不可靠——中文双引号和英文双引号在屏幕上看起来几乎一样。

### 7.3 子模块指针未更新

**事件**：子模块提交后忘记更新主仓库的子模块指针。

**后果**：Release 版本指向旧的子模块 commit，用户下载的版本缺少最新脚本修复。

**教训**：子模块提交后，立即执行 `git add scripts/private` + commit 更新指针。

### 7.4 子模块加载清单未同步

**日期**：2026-08-13

**事件**：子模块源文件已添加 `workflow_shadow.lua`，但未同步到 `scripts/michen_xkx.lua` 本地副本。

**后果**：运行时 `workflow_shadow` 全局变量为 nil，导致 `michen_alias_workflow.lua:93` 调用 `workflow_shadow.run()` 时崩溃，押镖流程中断。

**教训**：修改 `scripts/private/michen_xkx.lua` 后，必须立即执行 `cp scripts/private/michen_xkx.lua scripts/michen_xkx.lua`。

### 7.5 正则字符集中的多字节字符问题

**日期**：2026-07-04

**事件**：`michen_mp_gb.lua` 中 `[> ]*(.+)给你一[颗|块]+(.+).` 在匹配"给你一块和田玉"时，`[颗|块]+` 吞掉了"和"的第一字节 `E5`。

**后果**：`d` 捕获到损坏的 UTF-8 `田玉`，`alias.goldid` 无法识别。

**教训**：永远不要在 Lua 模式的 `[...]` 中放多字节汉字。拆成多次独立匹配。

---

## 八、开发工具与快捷方式

### 8.1 多实例运行

```bash
# 实例一（默认 profiles/ 目录）
./target/release/RustLuaMud

# 实例二（使用不同配置目录）
./target/release/RustLuaMud --profiles profiles2
```

### 8.2 临时禁用角色配置

将文件后缀改为非 `.toml`（如 `.bak`）即可。

### 8.3 内置调试命令

| 命令 | 说明 |
|------|------|
| `/lua <代码>` | 直接执行 Lua 代码 |
| `/load <脚本路径>` | 为前台连接加载 Lua 脚本 |
| `/load reload` / `/reload` | 重新加载前台脚本（保留变量状态） |

### 8.4 命名空间遗漏检测

```bash
python3 tools/check_ns_leak.py
```

检测脚本中是否有遗漏的裸全局变量。
