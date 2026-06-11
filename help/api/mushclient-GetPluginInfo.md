# MUSHclient GetPluginInfo 官方文档

> 来源：http://www.mushclient.com/scripts/function.php?name=GetPluginInfo
> 保存日期：2026-06-11

Gets details about a specified plugin.

**Prototype**: `VARIANT GetPluginInfo(BSTR PluginID, short InfoType);`

## GetPluginInfo code 映射

| code | 说明 | 返回值类型 |
|------|------|-----------|
| 1 | Name | string |
| 2 | Author | string |
| 3 | Description (long description) | string |
| 4 | Script contents | string |
| 5 | Script language (ie. vbscript, perlscript, jscript) | string |
| 6 | Plugin file name | string |
| 7 | Unique ID | string |
| 8 | Purpose (short description) | string |
| 9 | Number of triggers | long |
| 10 | Number of aliases | long |
| 11 | Number of timers | long |
| 12 | Number of variables | long |
| 13 | Date written | date |
| 14 | Date modified | date |
| 15 | Save state flag | boolean |
| 16 | Scripting enabled? | boolean |
| 17 | Enabled? | boolean |
| 18 | Required MUSHclient version | double |
| 19 | Version of plugin | double |
| 20 | Directory that plugin resides in | string |
| 21 | Evaluation order of plugin | long |
| 22 | Date/time plugin installed | date |
| 23 | During a CallPlugin call, the ID of the calling plugin | string |
| 24 | Time spent on scripting in this plugin (seconds) | double |
| 25 | Plugin sequence number | short |

## 说明

- 如果 InfoType 超出范围，返回 NULL
- 如果插件未安装，返回 NULL
- 可用版本：3.23+
