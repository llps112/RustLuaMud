# Git 提交规则

## 子模块独立提交

`scripts/private` 是独立子模块，主仓库不跟踪子模块版本。

**主仓库**（Rust 客户端）和 **子模块**（Lua 脚本）各自独立提交和推送，互不关联：
- 改脚本 → 只在 `scripts/private/` 里 commit + push
- 改主仓库代码 → 只在根目录 commit + push
- **禁止**在主仓库提交"更新子模块指针"这类与 Rust 代码无关的 commit

## war_members_data.lua 提交规则

`war_members_data.lua` 在脚本运行时会**实时写入**（成员经验值等数据持续更新），属于运行时数据文件而非代码变更。

**提交原则**：
- 每次版本发布或功能变更时，**附带提交一次**即可
- **短时间内不要重复提交**该文件的纯数据变更（经验值数字变动）
- 如果一次提交中已经包含了其他功能性修改（如 always.lua、michen_yb.lua 等），可以顺带包含 `war_members_data.lua` 的更新
- 如果距离上次提交该文件不到 24 小时，且没有功能性代码变更，**跳过**该文件的提交

## michen_xkx.lua 同步规则

`scripts/private/michen_xkx.lua` 是子模块中的加载清单（**唯一源文件**），`scripts/michen_xkx.lua` 是主仓库中的本地配置副本。两者需要保持同步。

**修改原则**：
- **只修改** `scripts/private/michen_xkx.lua`，**禁止**直接编辑 `scripts/michen_xkx.lua`
- 修改完成后，**必须同步**到 `scripts/michen_xkx.lua`
- 同步方式：直接复制文件内容（注意 `scripts/michen_xkx.lua` 不在 git 跟踪中，是本地配置）
- 同步时机：在子模块提交前完成同步，确保本地测试环境使用最新的加载清单

**执行命令**：
```bash
cp scripts/private/michen_xkx.lua scripts/michen_xkx.lua
```
