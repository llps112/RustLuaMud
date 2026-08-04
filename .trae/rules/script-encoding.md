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
   iconv -f utf-8 -t gbk scripts/class-utf8/xxx.lua -o scripts/class/xxx.lua
   ```

## 自动同步钩子

子模块已配置 `hooks/pre-commit`（通过 `core.hooksPath hooks` 启用），提交时自动检测暂存的 `class-utf8/*.lua` 文件并同步生成 GBK 版本。

> 即使忘记手动 iconv，钩子也会自动补全。但如果 GBK 文件已被手动修改且 UTF-8 未改动，钩子不会触发。

## 往期事故

- 2026-06-09：修复 always.lua 正则时用 SearchReplace 直接编辑 GBK 文件，导致所有中文字节被 corrupt，score 触发器无法匹配中文名，`me.charname` 始终为空。
