# 日志系统 API

日志文件管理接口，用于将游戏输出记录到文件。

---

## OpenLog(filename, append)

打开日志文件。

- **参数**:
  | 参数 | 类型 | 默认值 | 说明 |
  |------|------|--------|------|
  | filename | string | - | 日志文件名 |
  | append | boolean | false | true=追加模式，false=覆盖模式 |

- **返回值**: 无
- **示例**:
  ```lua
  OpenLog("mylog.log", true)   -- 追加模式打开日志
  OpenLog("newlog.log", false) -- 覆盖模式打开日志
  ```

---

## IsLogOpen()

检查日志文件是否已打开。

- **参数**: 无
- **返回值**: `boolean` - `true` 已打开，`false` 未打开
- **示例**:
  ```lua
  if not IsLogOpen() then
      OpenLog("session.log", true)
  end
  ```

---

## CloseLog()

关闭当前打开的日志文件。

- **参数**: 无
- **返回值**: 无
- **示例**:
  ```lua
  CloseLog()  -- 关闭日志文件
  ```

---

## 自动日志

客户端会按以下规则自动记录日志：

- 日志文件存储位置: `logs/` 目录
- 文件名格式: `<会话名>_<YYYYMMDD>-<HH>.log`（如 `mud_20260619-14.log`），按小时分割
- 轮转策略: 每个会话只保留最近 N 个日志文件（默认 24 个），超出的旧文件自动删除
- 按角色单独配置: 在角色 TOML 配置文件中设置 `log_rotation_count` 可覆盖全局默认值：
  ```toml
  # profiles/mychar.toml
  name = "mychar"
  host = "mud.example.com"
  port = 4000
  log_rotation_count = 168   # 保留 7 天
  ```
  不设置则使用全局默认值 24（保留 24 小时）。
- 日志内容: 所有输出和 Lua 日志均会自动写入
