//! MushClient 兼容 Lua API 注册
//!
//! 本目录实现 `LuaEngine::register_api`，注册所有 MushClient 兼容的 Lua API
//! （send/Execute/Note/ColourNote/AddTrigger/AddTimer/AddAlias/GetInfo/...）。
//!
//! 各分节的注册代码按拆分前 `api.rs` 的分节拆分到本目录下的子模块：
//!
//! - `commands`：命令执行、工具函数
//! - `output`：输出、ANSI 样式、日志
//! - `variables`：JSON 序列化桥接、配置、连接状态、变量、数据库
//! - `trigger_api`：触发器
//! - `alias_api`：别名
//! - `timer_api`：定时器
//! - `module_loader`：wait.lua 依赖、模块加载机制、Lua 兼容性补丁
//! - `constants`：常量表
//! - `legacy`：原始 API（保留兼容）

mod alias_api;
mod commands;
mod constants;
mod legacy;
mod module_loader;
mod output;
mod timer_api;
mod trigger_api;
mod variables;

use mlua::Result as LuaResult;

use crate::lua::types::LuaEngine;

impl LuaEngine {
    /// 注册全部 MushClient 兼容 Lua API
    ///
    /// 调用顺序与拆分前 `api.rs` 中 18 个分节的出现顺序一一对应，以保证 87 处
    /// `globals.set` 的先后顺序与拆分前完全一致（因此不按文件聚合调用，而是按
    /// 分节逐个调用）。
    pub(super) fn register_api(&mut self) -> LuaResult<()> {
        self.register_commands_api()?; // 1. 命令执行
        self.register_output_api()?; // 2. 输出
        self.register_json_api()?; // 3. JSON 序列化桥接
        self.register_trigger_api()?; // 4. 触发器 API
        self.register_alias_api()?; // 5. 别名 API
        self.register_timer_api()?; // 6. 定时器 API
        self.register_config_api()?; // 7. 配置 API
        self.register_connection_api()?; // 8. 连接状态 API
        self.register_utils_api()?; // 9. 工具函数
        self.register_ansi_api()?; // 10. ANSI 样式 API
        self.register_variables_api()?; // 11. 变量 API
        self.register_log_api()?; // 12. 日志 API
        self.register_database_api()?; // 13. 数据库 API
        self.register_constants_api()?; // 14. 常量表
        self.register_wait_api()?; // 15. wait.lua 依赖
        self.register_module_loader_api()?; // 16. 模块加载机制
        self.register_compat_api()?; // 17. Lua 兼容性补丁
        self.register_legacy_api()?; // 18. 原始 API（保留兼容）
        Ok(())
    }
}
