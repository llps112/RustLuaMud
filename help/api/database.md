# 数据库 API

SQLite3 数据库接口。

---

## DatabaseClose()

关闭当前打开的数据库连接。

- **参数**: 无
- **返回值**: 无
- **示例**:
  ```lua
  DatabaseClose()
  ```

---

## sqlite3

SQLite3 数据库操作模块，提供类 LuaSQL 的数据库访问接口。

### sqlite3.open(filename)

打开 SQLite3 数据库文件。

- **参数**: `filename` (string) - 数据库文件路径
- **返回值**: `connection` - 数据库连接对象
- **示例**:
  ```lua
  local conn = sqlite3.open("mydata.db")
  ```

### connection:execute(sql)

执行 SQL 查询。

- **参数**: `sql` (string) - SQL 语句
- **返回值**: `cursor` - 结果游标
- **示例**:
  ```lua
  local cur = conn:execute("SELECT * FROM mytable WHERE id = 1")
  ```

### cursor:fetch(row, field)

从游标中取数据。

- **参数**: `row` (string) - 目标表名, `field` (string) - 目标字段名
- **返回值**: 取决于查询结果
- **示例**:
  ```lua
  local row = {}
  local data = cur:fetch(row, "field_name")
  ```

### connection:close()

关闭数据库连接。

- **示例**:
  ```lua
  conn:close()
  ```

### connection:set_gbk(enable)

设置 GBK 编码模式。

- **参数**: `enable` (boolean) - `true` 启用 GBK 编码，`false` 禁用
- **示例**:
  ```lua
  conn:set_gbk(true)
  ```

---

## 使用示例

```lua
-- 打开数据库
local conn = sqlite3.open("mydata.db")
conn:set_gbk(true)

-- 查询数据
local cur = conn:execute("SELECT name, value FROM mytable WHERE id = 1")

-- 处理结果
local row = {}
local result = cur:fetch(row, "name")
if result then
    print("name: " .. row.name)
    print("value: " .. row.value)
end

-- 关闭连接
conn:close()
```
