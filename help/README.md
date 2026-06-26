# RustLuaMud 客户端文档

基于 Rust 的 MUSHclient 兼容 MUD 客户端，集成 LuaJIT 脚本引擎。

## 目录结构

```
help/
├── README.md                    # 本文档索引
├── api/                         # Lua API 接口文档
│   ├── README.md                # API 概览与分类
│   ├── command-execution.md     # 命令执行 (send, Execute, DiscardQueue, Simulate)
│   ├── output.md                # 输出 (ColourNote, Note, print, Tell, SetStatus)
│   ├── json.md                  # JSON 序列化 (json_encode, json_decode)
│   ├── triggers.md              # 触发器系统 (AddTrigger, AddTriggerEx, ...)
│   ├── aliases.md               # 别名系统 (AddAlias, DeleteAlias, ...)
│   ├── timers.md                # 定时器系统 (AddTimer, DeleteTimer, ...)
│   ├── config.md                # 配置 (GetInfo, SetOption, GetOption)
│   ├── network.md               # 网络 (IsConnected, Connect, Disconnect, OnConnect)
│   ├── variables.md             # 变量系统 (GetVariable, SetVariable, ...)
│   ├── logging.md               # 日志系统 (OpenLog, IsLogOpen, CloseLog)
│   ├── database.md              # 数据库 (DatabaseClose, sqlite3)
│   ├── constants.md             # 标志常量 (trigger_flag, alias_flag, timer_flag)
│   ├── utilities.md             # 工具函数 (GetUniqueNumber, Trim, MakeRegularExpression, ...)
│   └── internal.md              # 内部函数 (dofile, rex, trigger, alias, timer, get, set)
└── commands/                    # 客户端命令文档
    ├── README.md                # 命令概览
    └── clui.md                  # CLUI 界面操作指南
```

## 快速链接

| 分类 | 说明 |
|------|------|
| [API 概览](api/README.md) | 所有 Lua API 接口分类索引 |
| [命令概览](commands/README.md) | 客户端命令使用说明 |
| [CLUI 操作指南](commands/clui.md) | 终端界面操作说明 |
