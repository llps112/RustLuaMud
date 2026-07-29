//! SQLite 数据库封装（Lua 用户数据）
//!
//! 提供 `LuaDb` 和 `LuaStmt` 两个 UserData 类型，供 Lua 脚本通过
//! `DatabaseOpen` / `prepare` / `step` / `run` 等 API 访问 SQLite 数据库。

use std::sync::{Arc, Mutex};

use mlua::UserData;
use rusqlite::{types::Value as SqlValue, Connection};

use super::helpers::i64_to_lua_integer;

/// SQLite 连接包装（Lua 用户数据）
pub(super) struct LuaDb {
    pub(super) conn: Arc<Mutex<Connection>>,
    pub(super) text_is_gbk: bool,
}

impl UserData for LuaDb {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("close", |_, _this, ()| Ok(()));

        methods.add_method_mut("set_gbk", |_, this, flag: bool| {
            this.text_is_gbk = flag;
            Ok(())
        });

        methods.add_method("exec", |_, this, sql: String| {
            let conn = this.conn.lock().unwrap();
            conn.execute_batch(&sql)
                .map_err(|e| mlua::Error::external(e.to_string()))
        });

        methods.add_method("prepare", |lua, this, sql: String| {
            let conn = this.conn.lock().unwrap();
            conn.prepare(&sql)
                .map_err(|e| mlua::Error::external(e.to_string()))?;
            let lua_stmt = LuaStmt {
                conn: this.conn.clone(),
                sql: sql.clone(),
            };
            let ud = lua.create_userdata(lua_stmt)?;
            Ok(ud)
        });

        methods.add_method("changes", |_, this, ()| {
            let conn = this.conn.lock().unwrap();
            Ok(conn.changes() as i64)
        });

        methods.add_method("nrows", |lua, this, sql: String| {
            // 收集所有行数据到 Vec，避免在锁内创建 Lua 对象
            let rows_data: Vec<Vec<(String, rusqlite::types::Value)>> = {
                let conn = this.conn.lock().unwrap();
                let mut stmt = conn
                    .prepare(&sql)
                    .map_err(|e| mlua::Error::external(e.to_string()))?;
                let col_names: Vec<String> =
                    stmt.column_names().iter().map(|s| s.to_string()).collect();
                let mut rows = stmt
                    .query([])
                    .map_err(|e| mlua::Error::external(e.to_string()))?;
                let mut result = Vec::new();
                while let Some(row) = rows
                    .next()
                    .map_err(|e| mlua::Error::external(e.to_string()))?
                {
                    let mut row_data = Vec::with_capacity(col_names.len());
                    for (i, col_name) in col_names.iter().enumerate() {
                        let val = row
                            .get_ref(i)
                            .ok()
                            .map(|r| match r {
                                rusqlite::types::ValueRef::Null => rusqlite::types::Value::Null,
                                rusqlite::types::ValueRef::Integer(n) => {
                                    rusqlite::types::Value::Integer(n)
                                }
                                rusqlite::types::ValueRef::Real(f) => {
                                    rusqlite::types::Value::Real(f)
                                }
                                rusqlite::types::ValueRef::Text(s) => {
                                    // 根据数据库文本编码解码
                                    // 某些 GBK 字节序列恰好也是合法 UTF-8（但对应不同字符），
                                    // 所以不能用"先尝试 UTF-8"的启发式，必须明确指定编码
                                    let text = if this.text_is_gbk {
                                        let (cow, _, _) = encoding_rs::GBK.decode(s);
                                        cow.into_owned()
                                    } else if std::str::from_utf8(s).is_ok() {
                                        std::str::from_utf8(s).unwrap().to_string()
                                    } else {
                                        let (cow, _, _) = encoding_rs::GBK.decode(s);
                                        cow.into_owned()
                                    };
                                    rusqlite::types::Value::Text(text)
                                }
                                rusqlite::types::ValueRef::Blob(b) => {
                                    rusqlite::types::Value::Blob(b.to_vec())
                                }
                            })
                            .unwrap_or(rusqlite::types::Value::Null);
                        row_data.push((col_name.clone(), val));
                    }
                    result.push(row_data);
                }
                result
            };

            // 在锁外创建 Lua 迭代器，将数据移入闭包
            let mut idx = 0usize;
            let iter_fn = lua.create_function_mut(move |lua, ()| {
                if idx >= rows_data.len() {
                    return Ok(None);
                }
                let row_data = &rows_data[idx];
                let table = lua.create_table()?;
                for (col_name, val) in row_data {
                    let lua_val = match val {
                        rusqlite::types::Value::Null => mlua::Value::Nil,
                        rusqlite::types::Value::Integer(n) => {
                            mlua::Value::Integer(i64_to_lua_integer(*n))
                        }
                        rusqlite::types::Value::Real(f) => mlua::Value::Number(*f),
                        rusqlite::types::Value::Text(s) => {
                            mlua::Value::String(lua.create_string(s)?)
                        }
                        rusqlite::types::Value::Blob(b) => {
                            mlua::Value::String(lua.create_string(b)?)
                        }
                    };
                    table.set(col_name.clone(), lua_val)?;
                }
                idx += 1;
                Ok(Some(table))
            })?;
            Ok(iter_fn)
        });

        methods.add_method("last_insert_rowid", |_, this, ()| {
            let conn = this.conn.lock().unwrap();
            Ok(conn.last_insert_rowid())
        });
    }
}

/// SQLite 预处理语句包装
pub(super) struct LuaStmt {
    pub(super) conn: Arc<Mutex<Connection>>,
    pub(super) sql: String,
}

impl UserData for LuaStmt {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("step", |lua, this, args: Option<mlua::Table>| {
            let conn = this.conn.lock().unwrap();
            let mut stmt = conn
                .prepare(&this.sql)
                .map_err(|e| mlua::Error::external(e.to_string()))?;

            let params_vec = if let Some(ref t) = args {
                let len = t.len().unwrap_or(0) as usize;
                let mut vals: Vec<SqlValue> = Vec::with_capacity(len);
                for i in 1..=len {
                    let v: String = t.get(i).unwrap_or_default();
                    vals.push(SqlValue::Text(v));
                }
                vals
            } else {
                Vec::new()
            };

            let params_refs: Vec<&dyn rusqlite::types::ToSql> = params_vec
                .iter()
                .map(|v| v as &dyn rusqlite::types::ToSql)
                .collect();

            let mut rows = stmt
                .query(params_refs.as_slice())
                .map_err(|e| mlua::Error::external(e.to_string()))?;

            if let Some(row) = rows
                .next()
                .map_err(|e| mlua::Error::external(e.to_string()))?
            {
                let lua_table = lua.create_table()?;
                let col_count = row.as_ref().column_count();
                for i in 0..col_count {
                    let val = match row.get_ref(i) {
                        Ok(r) => match r {
                            rusqlite::types::ValueRef::Null => mlua::Value::Nil,
                            rusqlite::types::ValueRef::Integer(n) => {
                                mlua::Value::Integer(i64_to_lua_integer(n))
                            }
                            rusqlite::types::ValueRef::Real(f) => mlua::Value::Number(f),
                            rusqlite::types::ValueRef::Text(s) => {
                                // 尝试 UTF-8，失败则从 GBK 转码
                                let text = if std::str::from_utf8(s).is_ok() {
                                    std::str::from_utf8(s).unwrap().to_string()
                                } else {
                                    let (cow, _, _) = encoding_rs::GBK.decode(s);
                                    cow.into_owned()
                                };
                                mlua::Value::String(lua.create_string(&text)?)
                            }
                            rusqlite::types::ValueRef::Blob(b) => {
                                mlua::Value::String(lua.create_string(b)?)
                            }
                        },
                        Err(_) => mlua::Value::Nil,
                    };
                    lua_table.set(i + 1, val)?;
                }
                return Ok(Some(lua_table));
            }

            Ok(None)
        });

        methods.add_method("run", |_, this, args: Option<mlua::Table>| {
            let conn = this.conn.lock().unwrap();
            let mut stmt = conn
                .prepare(&this.sql)
                .map_err(|e| mlua::Error::external(e.to_string()))?;

            let params_vec = if let Some(ref t) = args {
                let len = t.len().unwrap_or(0) as usize;
                let mut vals: Vec<SqlValue> = Vec::with_capacity(len);
                for i in 1..=len {
                    let v: String = t.get(i).unwrap_or_default();
                    vals.push(SqlValue::Text(v));
                }
                vals
            } else {
                Vec::new()
            };

            let params_refs: Vec<&dyn rusqlite::types::ToSql> = params_vec
                .iter()
                .map(|v| v as &dyn rusqlite::types::ToSql)
                .collect();

            stmt.execute(params_refs.as_slice())
                .map_err(|e| mlua::Error::external(e.to_string()))?;

            Ok(())
        });
    }
}
