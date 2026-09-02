---
trigger: always_on
---
# 脚本编码与修改规则

## 重要规则

`scripts/class-utf8/` 是**主要开发源**（UTF-8 编码），所有修改在此进行。

`scripts/class/` 是 GBK 编码的运行时版本，由 `iconv` 从 UTF-8 版本生成，游戏实际加载此目录。

> 开发机上 `class/` 和 `class-utf8/` 通常是 `private/class` 和 `private/class-utf8` 的符号链接。

## 编码警告（GBK ≠ UTF-8）

**绝对禁止**：
- 使用 SearchReplace、Write 等工具编辑 `scripts/class/` 中的文件时，不要把文件内容当作 UTF-8 文本处理。这会导致 GBK 中文字节被替换为 `U+FFFD`（），破坏所有包含中文的触发器正则、注释和字符串。
- 用 `Read` 查看时显示的乱码是正常的——它们是有效的 GBK 编码，Lua 引擎加载时会自动转码。

## 正确的修改流程

如需修改脚本（如修复 bug、新增功能），应按以下步骤操作：

1. **修改 `scripts/class-utf8/` 中的 UTF-8 版本**（不要直接动 `scripts/class/`）
2. **用 `iconv` 转码覆盖 GBK 版本**：
   ```bash
   iconv -f utf-8 -t gbk scripts/class-utf8/xxx.lua > scripts/class/xxx.lua
   ```

> **不要用 `iconv -o`**。Windows 上 `/usr/bin/iconv` 通常是 GNU libiconv 1.17 独立版，
> **不支持 `-o`**：它会把 `-o` 当成输入文件名报 `No such file or directory`，同时把转码
> 结果喷到 stdout，返回非 0。只有 glibc 自带的 iconv（多数 Linux）才支持 `-o`。
> 用 shell 重定向两种实现都能工作。
>
> 重定向的副作用是：iconv 中途失败会把目标文件截断成 0 字节。改动较大的文件建议
> 先写临时文件、往返校验通过后再覆盖：
> ```bash
> iconv -f utf-8 -t gbk SRC > /tmp/x.gbk \
>   && iconv -f gbk -t utf-8 /tmp/x.gbk | cmp -s - SRC \
>   && cp /tmp/x.gbk DST
> ```

## 自动同步钩子

子模块提供 `hooks/pre-commit`，提交时自动检测暂存的 `class-utf8/*.lua`，做 LuaJIT 语法检查并同步生成 GBK 版本（内部已用上面的重定向 + 往返校验写法）。

**钩子不随 clone 自动生效**，`core.hooksPath` 是 per-machine 的 local 配置，不随仓库传播，每台机器（以及每次重新 clone）都要在子模块内各设置一次：

```bash
git -C scripts/private config core.hooksPath hooks
```

未设置时钩子完全不运行，GBK 同步只能靠手动 iconv。用 `git -C scripts/private config --get core.hooksPath` 可确认当前状态（无输出即为未设置）。

> 这个状态**只能代表本机**。判断「钩子历史上到底有没有跑过」时，不要把本机的配置外推到别的开发机——不同机器各自独立。

> 即使忘记手动 iconv，钩子也会自动补全。但如果 GBK 文件已被手动修改且 UTF-8 未改动，钩子不会触发。

> 主仓库的 `githooks/post-checkout` / `post-merge` **不做编码同步**，它们只负责同步主仓库的脚本本地副本（详见 `git-workflow.md`），同样需要 `core.hooksPath` 才生效。

## 往期事故

- 2026-06-09：修复 always.lua 正则时用 SearchReplace 直接编辑 GBK 文件，导致所有中文字节被 corrupt，score 触发器无法匹配中文名，`me.charname` 始终为空。
- 2026-09-02：排查 fj 双 GPS 时发现本机（Windows）子模块 `core.hooksPath` 未设置，钩子从未运行；且原版 `hooks/pre-commit` 在 Windows 上即使启用也跑不通——`iconv -o` 在 GNU libiconv 独立版上必然失败，`luajit -bl` 因配套构建缺 `jit.bcsave` 对**合法**文件也返回 1，会把每个文件误判成语法错误并中止提交。已改为重定向 + `loadfile()`，两端通用。
- 2026-09-02（同日更正上一条的过度推论）：上面两个坑**都是 Windows 特有的**。Linux 的 iconv 来自 glibc（支持 `-o`）、发行版 LuaJIT 通常带 `jit.*` 模块，原版写法在 Linux 上可正常工作。所以「近 200 个改 `class-utf8/*.lua` 的提交零漏同步」在 Linux 侧很可能一直是钩子在起作用，而非人工纪律；其中 `class/war_members_data.lua` 单独多出的不对称恰好是钩子行为的指纹（钩子只管 `class-utf8/*.lua` 对应的 GBK，而运行时数据由程序直接写 GBK 侧、需人工 add）。把「本机未配 hooksPath」当成「钩子从未运行」是错误归因，根源是 `core.hooksPath` 是 per-machine 配置，本机查不到别的机器状态。
