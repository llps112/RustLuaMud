# Lua API 接口文档

RustLuaMud 提供了 MUSHclient 兼容的 Lua API，涵盖命令执行、输出显示、触发器、别名、定时器、配置、网络通信等功能。

## API 分类

| 类别 | API | 说明 |
|------|-----|------|
| **命令执行** | `send`, `Execute`, `DiscardQueue`, `Simulate`, `DeleteTemporaryTimers` | 向 MUD 发送命令或模拟服务器输出 |
| **输出** | `ColourNote`, `Note`, `print`, `Tell`, `SetStatus`, `log` | 在输出窗口显示信息 |
| **JSON** | `json_encode`, `json_decode` | Lua 与 JSON 的序列化/反序列化 |
| **触发器** | `AddTrigger`, `AddTriggerEx`, `DeleteTrigger`, `GetTriggerList`, `GetTriggerInfo`, `SetTriggerOption`, `EnableTriggerGroup`, `EnableTrigger` | 管理文本匹配和自动响应 |
| **别名** | `AddAlias`, `DeleteAlias`, `GetAliasList`, `SetAliasOption` | 管理用户输入别名 |
| **定时器** | `AddTimer`, `DeleteTimer`, `GetTimerList`, `GetTimerInfo`, `SetTimerOption`, `EnableTimerGroup`, `EnableTimer`, `ResetTimer` | 管理定时任务 |
| **配置** | `GetInfo`, `SetOption`, `GetOption`, `SetAlphaOption`, `GetAlphaOption` | 获取/设置客户端配置 |
| **网络** | `IsConnected`, `Connect`, `Disconnect`, `OnConnect` | 连接管理 |
| **变量** | `GetVariable`, `SetVariable`, `DeleteVariable`, `GetVariableList` | 持久化变量存储 |
| **日志** | `OpenLog`, `IsLogOpen`, `CloseLog` | 日志文件管理 |
| **数据库** | `DatabaseClose`, `sqlite3` | SQLite3 数据库接口 |
| **常量** | `trigger_flag`, `alias_flag`, `timer_flag`, `custom_colour`, `error_code`, `error_desc` | API 参数标志常量 |
| **工具** | `GetUniqueNumber`, `Trim`, `MakeRegularExpression`, `GetPluginID`, `GetPluginInfo`, `bit` | 辅助函数 |
| **内部** | `dofile`, `rex`, `trigger`, `alias`, `timer`, `get`, `set` | 脚本加载、正则引擎等内部功能 |

## 详细文档

- [命令执行 API](command-execution.md)
- [输出 API](output.md)
- [JSON 序列化](json.md)
- [触发器系统](triggers.md)
- [别名系统](aliases.md)
- [定时器系统](timers.md)
- [配置 API](config.md)
- [网络 API](network.md)
- [变量系统](variables.md)
- [日志系统](logging.md)
- [数据库 API](database.md)
- [标志常量](constants.md)
- [工具函数](utilities.md)
- [内部函数](internal.md)
