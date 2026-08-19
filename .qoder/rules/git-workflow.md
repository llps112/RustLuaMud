---
trigger: always_on
---
# Git 提交规则

## 子模块独立提交

`scripts/private` 是独立子模块，主仓库不跟踪子模块版本。

**主仓库**（Rust 客户端）和 **子模块**（Lua 脚本）各自独立提交和推送，互不关联：
- 改脚本 → 只在 `scripts/private/` 里 commit + push
- 改主仓库代码 → 只在根目录 commit + push

## 子模块指针更新

子模块提交后，需要更新主仓库的子模块指针（`git add scripts/private` → commit），使 release 版本指向正确的子模块 commit。

**注意事项**：
- 子模块指针更新**只改 `.gitmodules` 中的 commit hash**，不涉及 Rust 代码变更
- **不需要触发 CI**（cargo fmt/clippy/test）
- **不需要发布新 release**——子模块由 `deploy.sh` 独立部署
- commit message 统一用 `chore: 更新子模块指针`

## war_members_data.lua 提交规则

`war_members_data.lua` 在脚本运行时会**实时写入**（成员经验值等数据持续更新），属于运行时数据文件而非代码变更。

**提交原则**：
- 每次版本发布或功能变更时，**附带提交一次**即可
- **短时间内不要重复提交**该文件的纯数据变更（经验值数字变动）
- 如果一次提交中已经包含了其他功能性修改（如 always.lua、michen_yb.lua 等），可以顺带包含 `war_members_data.lua` 的更新
- 如果距离上次提交该文件不到 24 小时，且没有功能性代码变更，**跳过**该文件的提交

## Release 发布规则

发布流程：bump 版本号 → commit + push → 创建 tag `vX.Y.Z` → push tag。

**监控原则**：push tag 后，只要通过 `gh run list` 确认 GitHub Actions release workflow **已触发并开始构建**，即视为发布完成，无需继续等待构建结果。GitHub Actions 会自动完成二进制编译、Release 创建和 Gitee 同步。

## michen_xkx.lua 同步规则

`scripts/private/michen_xkx.lua` 是子模块中的加载清单（**唯一源文件**），`scripts/michen_xkx.lua` 是主仓库中的本地配置副本。两者需要保持同步。

**修改原则**：
- **只修改** `scripts/private/michen_xkx.lua`，**禁止**直接编辑 `scripts/michen_xkx.lua`
- 修改完成后，**必须立即同步**到 `scripts/michen_xkx.lua`，不可拖延到提交前
- 同步方式：直接复制文件内容（注意 `scripts/michen_xkx.lua` 不在 git 跟踪中，是本地配置）
- 同步时机：**修改 michen_xkx.lua 的同一个任务内必须执行同步命令**，确保本地测试环境使用最新的加载清单
- **不同步的后果**：运行时加载的本地副本缺少新增文件，对应全局变量为 nil，触发 `attempt to index global 'xxx' (a nil value)` 错误

**执行命令**：
```bash
cp scripts/private/michen_xkx.lua scripts/michen_xkx.lua
```

**往期事故**：
- 2026-08-13：子模块源文件已添加 `workflow_shadow.lua`（第46行），但未同步到本地配置副本，运行时 `workflow_shadow` 全局变量为 nil，导致 `michen_alias_workflow.lua:93` 调用 `workflow_shadow.run()` 时崩溃，押镖流程中断。
