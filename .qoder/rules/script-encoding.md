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

**钩子不随 clone 自动生效**，`core.hooksPath` 必须在子模块内手动设置一次：

```bash
git -C scripts/private config core.hooksPath hooks
```

未设置时钩子完全不运行，GBK 同步只能靠手动 iconv。用 `git -C scripts/private config --get core.hooksPath` 可确认当前状态（无输出即为未设置）。

> 即使忘记手动 iconv，钩子也会自动补全。但如果 GBK 文件已被手动修改且 UTF-8 未改动，钩子不会触发。

> 主仓库的 `githooks/post-checkout` / `post-merge` **不做编码同步**，它们只负责同步主仓库的脚本本地副本（详见 `git-workflow.md`），同样需要 `core.hooksPath` 才生效。

## 往期事故

- 2026-06-09：修复 always.lua 正则时用 SearchReplace 直接编辑 GBK 文件，导致所有中文字节被 corrupt，score 触发器无法匹配中文名，`me.charname` 始终为空。
- 2026-09-02：排查 fj 双 GPS 时发现子模块 `core.hooksPath` 从未设置，且 `hooks/pre-commit` 里同时踩了两个坑——`iconv -o` 在 GNU libiconv 上必然失败，`luajit -bl` 因缺少 `jit.bcsave` 模块对**合法**文件也返回 1，会把每个文件误判成语法错误并中止提交。即使启用了钩子也无法工作，GBK 同步长期只能靠手动。
