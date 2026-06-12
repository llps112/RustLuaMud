# MUSHclient SetAliasOption 官方文档

> 来源：http://www.mushclient.com/scripts/function.php?name=SetAliasOption
> 保存日期：2026-06-12

Sets the value of a named alias option.

**Prototype**: `long SetAliasOption(BSTR AliasName, BSTR OptionName, BSTR Value);`

## 参数

- **AliasName**: 已有别名的名称
- **OptionName**: 选项名称（见下方列表）
- **Value**: 选项值（字符串）

## 选项名

| 选项 | 类型 | 说明 |
|------|------|------|
| `echo_alias` | y/n | 是否在输出窗口回显别名本身 |
| `enabled` | y/n | 别名是否启用 |
| `expand_variables` | y/n | 是否展开变量（如 @target） |
| `group` | string | 组名 |
| `ignore_case` | y/n | 忽略大小写匹配 |
| `keep_evaluating` | y/n | 是否继续评估下一个别名 |
| `match` | string | 匹配文本 |
| `menu` | y/n | 是否添加到别名菜单（LH 点击） |
| `omit_from_command_history` | y/n | 是否从命令历史中排除 |
| `omit_from_log` | y/n | 是否从日志文件中排除 |
| `omit_from_output` | y/n | 是否从输出窗口中排除发送的文本 |
| `one_shot` | y/n | 别名触发后是否自动删除 |
| `regexp` | y/n | 是否使用正则表达式 |
| `script` | string | 要调用的函数名 |
| `send` | string | 发送的文本（多行） |
| `send_to` | 0-14 | 发送目标位置 |
| `sequence` | 0-10000 | 检查顺序（小的优先） |
| `user` | integer | 用户自定义数字 |
| `variable` | string | 发送到的变量名 |

## Send-to 位置

| 值 | 说明 |
|----|------|
| 0 | 发送到 MUD |
| 1 | 放入命令窗口 |
| 2 | 在输出窗口显示 |
| 3 | 放入状态栏 |
| 4 | 新建记事本 |
| 5 | 追加到记事本 |
| 6 | 放入日志文件 |
| 7 | 替换记事本 |
| 8 | 排队发送 |
| 9 | 设置变量 |
| 10 | 重新解析为命令 |
| 11 | 作为 speedwalk 发送到 MUD |
| 12 | 发送到脚本引擎 |
| 13 | 不排队直接发送 |
| 14 | 发送到脚本引擎 — 从输出中排除后 |

## Lua 备注

- 布尔值可直接传 `true`/`false`，也可传 `"y"`/`"n"`/`"1"`/`"0"`
- 数值选项（如 `sequence`）字符串会被转换为数字
- 从插件内调用时，操作的是当前插件的别名，不是全局别名

## 返回值

| 值 | 说明 |
|----|------|
| 0 | eOK — 设置成功 |
| 1 | eInvalidObjectLabel — 别名名称无效 |
| 2 | eAliasCannotBeEmpty — 匹配文本不能为空 |
| 3 | eScriptNameNotLocated — 找不到脚本函数 |
| 4 | eBadRegularExpression — 正则表达式无效 |
| 5 | ePluginCannotSetOption — 选项被标记为不可设置 |

**引入版本**: 3.29
