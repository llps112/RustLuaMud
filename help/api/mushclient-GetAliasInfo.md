# MUSHclient GetAliasInfo 官方文档

> 来源：http://www.mushclient.com/scripts/function.php?name=GetAliasInfo
> 保存日期：2026-06-11

Gets details about an alias.

**Prototype**: `VARIANT GetAliasInfo(BSTR AliasName, short InfoType);`

## GetAliasInfo code 映射

| code | 说明 | 返回值类型 |
|------|------|-----------|
| 1 | What to match on | string |
| 2 | What to send | string |
| 3 | Script procedure name | string |
| 4 | Omit from log | boolean |
| 5 | Omit from output | boolean |
| 6 | Enabled | boolean |
| 7 | Regular expression | boolean |
| 8 | Ignore case | boolean |
| 9 | Expand variables | boolean |
| 10 | Invocation count | long |
| 11 | Times matched | long |
| 12 | Menu | boolean |
| 13 | Date/time alias last matched | date |
| 14 | 'temporary' flag | boolean |
| 15 | Alias was included from an include file | boolean |
| 16 | Group name | string |
| 17 | Variable name to set | string |
| 18 | Send-to location | long |
| 19 | 'keep-evaluating' flag | boolean |
| 20 | Sequence number | long |
| 21 | 'echo alias' flag | boolean |
| 22 | 'omit from command history' flag | boolean |
| 23 | User option value | long |
| 24 | Number of matches to regular expression (most recent match) | long |
| 25 | The string we matched against | string |
| 26 | Executing-script flag | boolean |
| 27 | Script is valid flag | boolean |
| 28 | Error number from PCRE when evaluating last match | long |
| 29 | 'one shot' flag | boolean |
| 30 | Time taken (in seconds) to test aliases | double |
| 31 | Number of attempts to match this alias | long |
| 101~110 | Wildcard %1~%0 from last time it matched | string |
