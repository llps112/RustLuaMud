# MUSHclient EnableAlias 官方文档

> 来源：http://www.mushclient.com/scripts/function.php?name=EnableAlias
> 保存日期：2026-06-12

Enables or disables an alias.

**Prototype**: `long EnableAlias(BSTR AliasName, BOOL Enabled);`

## 描述

启用或禁用指定的别名。启用的别名在输入命令时会被匹配处理，禁用的别名被忽略。

## 返回值

| 值 | 说明 |
|----|------|
| 0 | eOK — 操作成功 |
| 1 | eInvalidObjectLabel — 别名名称无效 |
| 2 | eAliasNotFound — 找不到指定的别名 |
