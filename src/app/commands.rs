// 内置命令执行（/connect /load /lua /set /switch /profile /all 等）
// 从 app.rs 拆分而来

use std::fs;
use std::io;
use std::path::Path;

use crate::config::{AppConfig, ConnectionConfig};
use crate::connection::SessionId;
use crate::log::Logger;

use super::events::push_session_output_capped;
use super::parse::{format_lua_error, parse_builtin_command, BuiltinCommand, ProfileSubcommand};
use super::session::{ConnectRequest, ReconnectRequest};
use super::{App, TermSettings};

/// 注入 session 登录凭证到新建引擎（与 init_lua_for_session 对齐）：
/// 脚本顶层 me.charid=char_name 依赖 char_name 全局变量。
///
/// 返回凭据缺失告警而不自行输出：本函数只持有 engine，拿不到终端与日志，
/// 由调用方经 `sys_output` 三写。**静默跳过是线上事故的成因之一** ——
/// 玩家只看到「charid 为 nil」的远距离崩溃，看不到「凭据根本没注入」这个真因。
/// 四个调用点（/load、/reload、/all reload、/all /load）紧接着都会 load_script，
/// 所以无需像 init_lua_for_session 那样再按「是否配了脚本」做门控。
fn inject_session_credentials(
    engine: &mut crate::lua::LuaEngine,
    username: &Option<String>,
    password: &Option<String>,
) -> Vec<String> {
    let mut warns = Vec::new();
    match username.as_deref().filter(|u| !u.is_empty()) {
        Some(uname) => {
            engine.set_variable("char_name", uname);
            engine.set_global("char_name", uname);
            engine.set_char_name(uname);
        }
        None => warns.push("未配置 username，脚本中 char_name 将为 nil".to_string()),
    }
    match password.as_deref().filter(|p| !p.is_empty()) {
        Some(pwd) => {
            engine.set_variable("char_password", pwd);
            engine.set_global("char_password", pwd);
        }
        None => warns.push("未配置 password，脚本中 char_password 将为 nil".to_string()),
    }
    warns
}

/// 构造 /connect 动态建连所用的配置。
///
/// 只显式给出 5 个字段，其余走 `ConnectionConfig::default()`（与 serde 的
/// `default_*()` 同源，见 config.rs 的 Default 实现）：抽出本函数是为了让
/// /connect 的默认值语义可被单测锁定，避免这段内联字面量随 config.rs 漂移。
fn dynamic_connect_config(name: &str, host: &str, port: u16) -> ConnectionConfig {
    ConnectionConfig {
        name: name.to_string(),
        host: host.to_string(),
        port,
        encoding: Some("gbk".to_string()),
        // /connect 由本函数自行发 ConnectRequest（见下方 connect_tx），
        // 而 ConnectionConfig 的默认 auto_connect 是 true（default_true），
        // 不显式置 false 会重复建连。
        auto_connect: false,
        ..Default::default()
    }
}

impl App {
    /// 排空引擎在脚本加载期间 run/Execute 压入的命令并分发：
    /// '/' 前缀按 Lua 代码执行（嵌套产生的命令继续排空），其余发给服务端。
    /// 必须排到零残留，否则下次 timer tick 会触发
    /// debug_assert!(pending_commands.is_empty()) panic（timers.rs）。
    fn drain_engine_commands(
        &mut self,
        session_id: SessionId,
        engine: &mut crate::lua::LuaEngine,
        name: &str,
    ) -> io::Result<()> {
        let mut queue: std::collections::VecDeque<String> = engine.drain_commands().into();
        while let Some(cmd) = queue.pop_front() {
            if let Some(lua_code) = cmd.strip_prefix('/') {
                if let Err(e) = engine.eval_code(lua_code) {
                    self.terminal
                        .append_output(&format!("[Lua] 执行排队命令失败: {}", e))?;
                } else {
                    // eval_code 内部可能再次 Execute/run 入队，继续排空防残留
                    queue.extend(engine.drain_commands());
                }
            } else {
                self.logger.log_command(name, &cmd);
                self.send_cmd_checked(session_id, &cmd)?;
            }
        }
        Ok(())
    }

    /// 处理内置命令（基于 parse_builtin_command 分发）
    pub(crate) fn handle_builtin_command(&mut self, cmd: &str) -> io::Result<()> {
        match parse_builtin_command(cmd) {
            BuiltinCommand::Connect { name, host, port } => {
                let conn_config = dynamic_connect_config(&name, &host, port);

                let session_id = match self.manager.add_connection_dynamic(&conn_config) {
                    Ok(id) => id,
                    Err(e) => {
                        self.terminal.append_output(&format!("[错误] {}", e))?;
                        return Ok(());
                    }
                };
                // 启动渲染定时器（非实时模式且配置了 render_interval > 0）
                if !conn_config.realtime && conn_config.render_interval > 0 {
                    self.start_render_tick_timer(session_id, conn_config.render_interval);
                }
                self.update_status_bar()?;
                let _ = self.connect_tx.try_send(ConnectRequest { session_id });
                let display_pos = self.manager.display_number_of(session_id);
                self.terminal.append_output(&format!(
                    "[系统] 正在连接 {} ({}) → {}:{}",
                    display_pos, name, host, port
                ))?;
            }

            BuiltinCommand::Disconnect { id } => {
                if let Some(id) = id {
                    if let Some(session_id) = self.manager.session_id_by_display_number(id) {
                        if let Some(session) = self.manager.get_mut_by_id(session_id) {
                            session.disconnect();
                            session.state = crate::connection::SessionState::Disconnected;
                        }
                        let name = self
                            .manager
                            .get_by_id(session_id)
                            .map(|s| s.name.clone())
                            .unwrap_or_default();
                        self.update_status_bar()?;
                        self.terminal
                            .append_output(&format!("[系统] 已断开连接 {} ({})", id, name))?;
                    } else {
                        self.terminal
                            .append_output(&format!("[错误] 连接 {} 不存在", id))?;
                    }
                } else {
                    let fg_id = self.manager.foreground_id;
                    if self.manager.get_by_id(fg_id).is_some() {
                        if let Some(session) = self.manager.get_mut_by_id(fg_id) {
                            session.disconnect();
                            session.state = crate::connection::SessionState::Disconnected;
                        }
                        self.update_status_bar()?;
                        let name = self
                            .manager
                            .get_by_id(fg_id)
                            .map(|s| s.name.clone())
                            .unwrap_or_default();
                        let display_pos = self.manager.display_number_of(fg_id);
                        self.terminal.append_output(&format!(
                            "[系统] 已断开连接 {} ({})",
                            display_pos, name
                        ))?;
                    }
                }
            }

            BuiltinCommand::Reconnect { id } => {
                let session_id = if let Some(id) = id {
                    if let Some(sid) = self.manager.session_id_by_display_number(id) {
                        sid
                    } else {
                        self.terminal
                            .append_output(&format!("[错误] 连接 {} 不存在", id))?;
                        return Ok(());
                    }
                } else {
                    self.manager.foreground_id
                };
                if self.manager.get_by_id(session_id).is_none() {
                    self.terminal.append_output("[错误] 连接不存在")?;
                    return Ok(());
                }
                let name = self
                    .manager
                    .get_by_id(session_id)
                    .map(|s| s.name.clone())
                    .unwrap_or_default();
                if let Some(session) = self.manager.get_mut_by_id(session_id) {
                    session.disconnect();
                    session.state = crate::connection::SessionState::Disconnected;
                }
                let display_pos = self.manager.display_number_of(session_id);
                self.terminal
                    .append_output(&format!("[系统] 正在重连 {} ({})...", display_pos, name))?;
                self.update_status_bar()?;
                let _ = self.reconnect_tx.try_send(ReconnectRequest { session_id });
            }

            BuiltinCommand::Close { id } => {
                let session_id = if let Some(id) = id {
                    if let Some(sid) = self.manager.session_id_by_display_number(id) {
                        sid
                    } else {
                        self.terminal
                            .append_output(&format!("[错误] 连接 {} 不存在", id))?;
                        return Ok(());
                    }
                } else {
                    self.manager.foreground_id
                };
                let display_pos = self.manager.display_number_of(session_id);
                // 清理定时器：停止目标 session 的渲染刷新定时器
                self.stop_render_tick_timer(session_id);
                // 清理该 session 的发送错误去重标记
                self.raw_send_err_shown.remove(&session_id);
                self.cmd_send_err_shown.remove(&session_id);

                match self.manager.remove_session(session_id) {
                    Ok(name) => {
                        // L8 回收：仅当无其它同名 session 存活时清除按名登记，
                        // 避免 M3 允许的同名并存下误删仍在用 session 的脱敏凭据
                        let name_still_used =
                            self.manager.ordered_session_ids().iter().any(|&id| {
                                self.manager
                                    .get_by_id(id)
                                    .map(|s| s.name == name)
                                    .unwrap_or(false)
                            });
                        if !name_still_used {
                            self.logger.forget_session(&name);
                        }
                        self.update_status_bar()?;
                        if self.manager.session_count() > 0 {
                            self.switch_foreground(self.manager.foreground_id)?;
                        } else {
                            self.terminal.replace_output(&Vec::new())?;
                        }
                        self.terminal.append_output(&format!(
                            "[系统] 已关闭连接 {} ({})",
                            display_pos, name
                        ))?;
                    }
                    Err(e) => {
                        self.terminal.append_output(&format!("[错误] {}", e))?;
                    }
                }
            }

            BuiltinCommand::List => {
                for &sid in self.manager.ordered_session_ids() {
                    if let Some(s) = self.manager.get_by_id(sid) {
                        let state_str = match s.state {
                            crate::connection::SessionState::Connected => "已连接",
                            crate::connection::SessionState::Disconnected => "已断开",
                            crate::connection::SessionState::Connecting => "连接中...",
                            crate::connection::SessionState::Reconnecting => "重连中...",
                        };
                        let marker = if sid == self.manager.foreground_id {
                            "★"
                        } else {
                            " "
                        };
                        let display_num = self.manager.display_number_of(sid);
                        self.terminal.append_output(&format!(
                            "{} [{}] {} - {}",
                            marker, display_num, s.name, state_str
                        ))?;
                    }
                }
            }

            BuiltinCommand::Load { path } => {
                let fg_id = self.manager.foreground_id;
                if self.manager.get_by_id(fg_id).is_none() {
                    self.terminal.append_output("[错误] 无前台连接")?;
                    return Ok(());
                }
                let fg_name = self
                    .manager
                    .get_by_id(fg_id)
                    .map(|s| s.name.clone())
                    .unwrap_or_default();
                // /load 与连接路径对齐：恢复连接状态 + 注入凭证 + log_dir，
                // 否则脚本顶层 char_name 为 nil 且加载期入队命令无人排空
                let saved_conn_state = self
                    .manager
                    .get_by_id(fg_id)
                    .and_then(|s| s.lua_engine.as_ref())
                    .map(|e| e.get_connection_state());
                let (fg_username, fg_password) = self
                    .manager
                    .get_by_id(fg_id)
                    .map(|s| (s.username.clone(), s.password.clone()))
                    .unwrap_or_default();
                match crate::lua::LuaEngine::new() {
                    Ok(mut engine) => {
                        if let Some(ref conn_state) = saved_conn_state {
                            engine.restore_connection_state(conn_state);
                        }
                        for w in inject_session_credentials(&mut engine, &fg_username, &fg_password)
                        {
                            self.sys_output(&format!("[警告] [{}] {}", fg_name, w))?;
                        }
                        engine.set_log_dir(&self.config.general.log_dir);
                        match engine.load_script(&path) {
                            Ok(()) => {
                                self.drain_engine_commands(fg_id, &mut engine, &fg_name)?;
                                if let Some(session) = self.manager.get_mut_by_id(fg_id) {
                                    session.lua_engine = Some(engine);
                                }
                                self.drain_lua_logs(fg_id)?;
                                self.terminal.append_output(&format!(
                                    "\x1b[36m[Lua] 脚本已加载: {}\x1b[0m",
                                    path
                                ))?;
                                self.start_timers_for_session(fg_id);
                            }
                            Err(e) => {
                                let err_msg = e.to_string();
                                for line in format_lua_error(&err_msg) {
                                    self.terminal
                                        .append_output(&format!("\x1b[36m[Lua] {}\x1b[0m", line))?;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        self.terminal.append_output(&format!(
                            "\x1b[36m[Lua] 引擎初始化失败: {}\x1b[0m",
                            e
                        ))?;
                    }
                }
            }

            BuiltinCommand::LoadReload => {
                let fg_id = self.manager.foreground_id;
                if self.manager.get_by_id(fg_id).is_none() {
                    self.terminal.append_output("[错误] 无前台连接")?;
                    return Ok(());
                }
                let script_path = self
                    .manager
                    .get_by_id(fg_id)
                    .and_then(|s| s.lua_engine.as_ref())
                    .and_then(|e| e.script_path());
                // 保存原 engine 的连接状态（host/port/char_name 等，供 GetInfo 返回）
                let saved_conn_state = self
                    .manager
                    .get_by_id(fg_id)
                    .and_then(|s| s.lua_engine.as_ref())
                    .map(|e| e.get_connection_state());
                let fg_name = self
                    .manager
                    .get_by_id(fg_id)
                    .map(|s| s.name.clone())
                    .unwrap_or_default();
                // 保存 session 的登录凭证（reload 后需重新注入 Lua 全局变量 char_name/char_password）
                let (fg_username, fg_password) = self
                    .manager
                    .get_by_id(fg_id)
                    .map(|s| (s.username.clone(), s.password.clone()))
                    .unwrap_or_default();
                if let Some(path) = script_path {
                    match crate::lua::LuaEngine::new() {
                        Ok(mut engine) => {
                            // 恢复连接状态（GetInfo 需要），不恢复 variables——脚本顶层代码会重新初始化
                            if let Some(ref conn_state) = saved_conn_state {
                                engine.restore_connection_state(conn_state);
                            }
                            // 重新注入登录凭证（与 init_lua_for_session 一致）：
                            // 脚本顶层 me.charid=char_name 依赖该全局变量
                            for w in
                                inject_session_credentials(&mut engine, &fg_username, &fg_password)
                            {
                                self.sys_output(&format!("[警告] [{}] {}", fg_name, w))?;
                            }
                            // 重新注入日志目录（供 GetInfo(58) 返回，不在 ConnectionState 中）
                            engine.set_log_dir(&self.config.general.log_dir);
                            match engine.load_script(&path) {
                                Ok(()) => {
                                    // 排空脚本加载期间 run/Execute 压入的命令，避免下次 timer tick
                                    // 触发 debug_assert!(pending_commands.is_empty()) 崩溃
                                    self.drain_engine_commands(fg_id, &mut engine, &fg_name)?;
                                    // 排空 Lua 日志（drain_lua_logs 会处理日志写入和终端输出）
                                    if let Some(session) = self.manager.get_mut_by_id(fg_id) {
                                        session.lua_engine = Some(engine);
                                    }
                                    self.drain_lua_logs(fg_id)?;
                                    self.terminal.append_output(&format!(
                                        "\x1b[36m[Lua] 脚本已重新加载: {}\x1b[0m",
                                        path
                                    ))?;
                                    self.start_timers_for_session(fg_id);
                                }
                                Err(e) => {
                                    let err_msg = e.to_string();
                                    for line in format_lua_error(&err_msg) {
                                        self.terminal.append_output(&format!(
                                            "\x1b[36m[Lua] {}\x1b[0m",
                                            line
                                        ))?;
                                    }
                                    // 脚本加载错误也写入日志
                                    for line in format_lua_error(&err_msg) {
                                        self.logger.log_debug(&fg_name, &line);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            self.terminal.append_output(&format!(
                                "\x1b[36m[Lua] 引擎初始化失败: {}\x1b[0m",
                                e
                            ))?;
                        }
                    }
                } else {
                    self.terminal
                        .append_output("\x1b[36m[Lua] 未找到之前加载的脚本路径\x1b[0m")?;
                }
            }

            BuiltinCommand::Lua { code } => {
                let fg_id = self.manager.foreground_id;
                if self.manager.get_by_id(fg_id).is_none() {
                    self.terminal.append_output("[错误] 无前台连接")?;
                    return Ok(());
                }
                let name = self
                    .manager
                    .get_by_id(fg_id)
                    .map(|s| s.name.clone())
                    .unwrap_or_default();
                self.logger.log_lua(&name, &code);
                if let Some(engine) = self
                    .manager
                    .get_by_id(fg_id)
                    .and_then(|s| s.lua_engine.as_ref())
                {
                    match engine.eval_code(&code) {
                        Ok(_) => {
                            let commands = engine.drain_commands();
                            self.send_lua_commands(fg_id, commands)?;
                            self.send_lua_raw(fg_id)?;
                            self.drain_lua_logs(fg_id)?;
                            // /lua 命令可能调用了 SetStatus，刷新状态栏
                            self.update_status_bar()?;
                        }
                        Err(e) => {
                            self.terminal.append_output(&format!("[Lua 错误] {}", e))?;
                        }
                    }
                } else {
                    self.terminal
                        .append_output("[错误] 未加载 Lua 引擎，请先加载脚本")?;
                }
            }

            BuiltinCommand::Set { option, value } => match option.as_str() {
                "keep_command" => {
                    let enabled = matches!(value.as_str(), "on" | "1" | "true" | "yes");
                    self.terminal.state_mut().keep_command = enabled;
                    let status = if enabled { "已启用" } else { "已关闭" };
                    TermSettings {
                        keep_command: enabled,
                    }
                    .save(&self.config.general.profile_dir);
                    self.terminal
                        .append_output(&format!("[系统] 保留命令栏输入: {} (已保存)", status))?;
                }
                "render_interval" => {
                    let fg_id = self.manager.foreground_id;
                    if self.manager.get_by_id(fg_id).is_none() {
                        self.terminal.append_output("[错误] 无前台连接")?;
                        return Ok(());
                    }
                    match value.parse::<u64>() {
                        Ok(ms) => {
                            // 限制范围：[50, 10000]ms
                            let clamped = ms.clamp(50, 10000);
                            let is_realtime = self
                                .manager
                                .get_by_id(fg_id)
                                .map(|s| s.realtime)
                                .unwrap_or(false);
                            if let Some(session) = self.manager.get_mut_by_id(fg_id) {
                                session.render_interval = clamped;
                            }
                            // 仅在非实时模式下重启定时器
                            if !is_realtime {
                                self.start_render_tick_timer(fg_id, clamped);
                            }
                            self.terminal.append_output(&format!(
                                "[系统] 渲染间隔已设置为: {}ms (当前连接)",
                                clamped
                            ))?;
                        }
                        Err(_) => {
                            self.terminal.append_output(&format!(
                                "[错误] render_interval 必须是正整数（毫秒），当前值: {}",
                                value
                            ))?;
                        }
                    }
                }
                "realtime" => {
                    let fg_id = self.manager.foreground_id;
                    if self.manager.get_by_id(fg_id).is_none() {
                        self.terminal.append_output("[错误] 无前台连接")?;
                        return Ok(());
                    }
                    let enabled = matches!(value.as_str(), "on" | "1" | "true" | "yes");
                    if let Some(session) = self.manager.get_mut_by_id(fg_id) {
                        session.realtime = enabled;
                    }
                    // 根据新模式调整定时器
                    if enabled {
                        // 实时模式：停止定时器
                        self.stop_render_tick_timer(fg_id);
                    } else {
                        // 节流模式：启动定时器
                        let interval = self
                            .manager
                            .get_by_id(fg_id)
                            .map(|s| s.render_interval)
                            .unwrap_or(0);
                        if interval > 0 {
                            self.start_render_tick_timer(fg_id, interval);
                        }
                    }
                    let status = if enabled { "实时" } else { "节流" };
                    self.terminal.append_output(&format!(
                        "[系统] 渲染模式已切换为: {} (当前连接)",
                        status
                    ))?;
                }
                _ => {
                    self.terminal.append_output(&format!(
                        "[错误] 未知设置选项: {}。可用选项: keep_command, render_interval, realtime",
                        option
                    ))?;
                }
            },

            BuiltinCommand::Switch { target } => {
                // 尝试解析为数字
                if let Ok(id) = target.parse::<usize>() {
                    if let Some(session_id) = self.manager.session_id_by_display_number(id) {
                        self.switch_foreground(session_id)?;
                        let name = self
                            .manager
                            .get_by_id(session_id)
                            .map(|s| s.name.clone())
                            .unwrap_or_default();
                        self.terminal
                            .append_output(&format!("[系统] 已切换到连接 {} ({})", id, name))?;
                    } else {
                        self.terminal
                            .append_output(&format!("[错误] 连接 {} 不存在", id))?;
                    }
                } else {
                    // 按名称查找
                    let target_name = target.to_lowercase();
                    if let Some(&session_id) =
                        self.manager.ordered_session_ids().iter().find(|&&sid| {
                            self.manager
                                .get_by_id(sid)
                                .map(|s| s.name.to_lowercase() == target_name)
                                .unwrap_or(false)
                        })
                    {
                        self.switch_foreground(session_id)?;
                        let display_num = self.manager.display_number_of(session_id);
                        let name = self
                            .manager
                            .get_by_id(session_id)
                            .map(|s| s.name.clone())
                            .unwrap_or_default();
                        self.terminal.append_output(&format!(
                            "[系统] 已切换到连接 {} ({})",
                            display_num, name
                        ))?;
                    } else {
                        self.terminal
                            .append_output(&format!("[错误] 未找到角色 '{}'", target))?;
                    }
                }
            }

            BuiltinCommand::Profile { sub } => match sub {
                ProfileSubcommand::List => {
                    let profile_dir = &self.config.general.profile_dir;
                    match AppConfig::load_profiles(profile_dir) {
                        (profiles, _) if profiles.is_empty() => {
                            self.terminal
                                .append_output("[系统] profiles/ 目录下没有可用角色配置")?;
                        }
                        (profiles, _) => {
                            self.terminal.append_output("[系统] 可用角色配置:")?;
                            for p in &profiles {
                                let loaded =
                                    self.manager.ordered_session_ids().iter().any(|&sid| {
                                        self.manager
                                            .get_by_id(sid)
                                            .map(|s| s.name == p.name)
                                            .unwrap_or(false)
                                    });
                                let marker = if loaded { " (已加载)" } else { "" };
                                self.terminal.append_output(&format!(
                                    "  {} — {}:{}{}",
                                    p.name, p.host, p.port, marker
                                ))?;
                            }
                        }
                    }
                }
                ProfileSubcommand::Load { name } => {
                    // /profile load 与 load_profiles 一致，拒绝加载示例配置
                    if name.eq_ignore_ascii_case("example") {
                        self.sys_output("[错误] 不能加载示例配置文件 (example.toml)")?;
                        return Ok(());
                    }
                    // 必须 clone：下面多处 sys_output 需要 &mut self，
                    // 而 profile_dir 在构造 env_path 时还要再读一次
                    let profile_dir = self.config.general.profile_dir.clone();
                    let profile_path = Path::new(&profile_dir).join(format!("{}.toml", name));
                    if !profile_path.exists() {
                        self.sys_output(&format!(
                            "[错误] 角色配置不存在: {}",
                            profile_path.display()
                        ))?;
                        return Ok(());
                    }
                    let content = match fs::read_to_string(&profile_path) {
                        Ok(c) => c,
                        Err(e) => {
                            self.sys_output(&format!(
                                "[错误] 无法读取配置文件 {}: {}",
                                profile_path.display(),
                                e
                            ))?;
                            return Ok(());
                        }
                    };

                    // 先刷新 .env 再解析占位符。.env 原先只在启动时读一次，启动后
                    // 新增的键对本进程不可见 → 凭据被置 None → Lua 侧 char_name 为 nil
                    // → 脚本顶层拼接时崩溃。刷新只走内存态 EnvStore，绝不调 set_var
                    // （Linux 下 setenv 会 realloc environ，与并发 getenv 竞态可致段错误）。
                    let env_path = Path::new(&profile_dir).join(".env");
                    let env_exists = env_path.exists();
                    let mut env_warns = Vec::new();
                    // 文件不存在时故意不刷新：reload 会把整个内存表当成「.env 已清空」重建，
                    // 而 EnvStore 是进程级全局态，清空会连带抹掉其他测试/其他 profile 正在用的键
                    // （测试靠独占键名前缀隔离，一个「清全部」的入口会直接破坏该策略）。
                    // 代价：运行时删掉单行立即生效，删掉**整个文件**则需重启才失效 ——
                    // 此时仍用旧凭据登录。因重名保护已独立修好，这只是意外而非危害。
                    let updated = if env_exists {
                        crate::config::reload_env_file(&env_path, &mut env_warns)
                    } else {
                        0
                    };
                    for w in env_warns {
                        self.sys_output(&format!("[警告] .env 解析: {}", w))?;
                    }
                    if updated > 0 {
                        self.sys_output(&format!(
                            "[系统] 已重新读取 {}，更新 {} 个凭据键",
                            env_path.display(),
                            updated
                        ))?;
                    }

                    // TUI 运行中 stderr 不可见，展开告警须通过终端 UI 展示给玩家，
                    // 否则环境变量缺失时密码静默置空，只见登录失败无从排查。
                    let mut warns = Vec::new();
                    let mut missing = Vec::new();
                    let conn_config =
                        match crate::config::ConnectionConfig::from_toml_str_with_warnings(
                            &content,
                            &mut warns,
                            &mut missing,
                        ) {
                            Ok(c) => c,
                            Err(e) => {
                                self.sys_output(&format!("[错误] 配置文件格式错误: {}", e))?;
                                return Ok(());
                            }
                        };

                    // 展开失败必须在建 session 之前拦下：session 一旦建起来就会自动
                    // connect → switch_foreground，凭据为 nil 的脚本必然崩，而且崩在
                    // 离真因很远的 include("config_"..charid..".lua")。
                    // 注意与「TOML 里根本没写凭据」区分：后者是合法的手动输入语义，missing 为空。
                    if !missing.is_empty() {
                        for m in &missing {
                            self.sys_output(&format!(
                                "[错误] 凭据展开失败: {} 的 {} 引用的环境变量 {} 未定义",
                                conn_config.name, m.field, m.var
                            ))?;
                        }
                        let vars: Vec<&str> = missing.iter().map(|m| m.var.as_str()).collect();
                        // 文件不存在时说「补齐」会误导（没文件可补），得说「创建」
                        let fix_hint = if env_exists {
                            format!("请在 {} 中补齐 {}", env_path.display(), vars.join(" / "))
                        } else {
                            format!(
                                "请创建 {}（UTF-8 保存）并写入 {}",
                                env_path.display(),
                                vars.join(" / ")
                            )
                        };
                        self.sys_output(&format!(
                            "[错误] 已中止加载 '{}': {} 后重新执行 /profile load {}（无需重启客户端）",
                            name, fix_hint, name
                        ))?;
                        return Ok(());
                    }

                    // 非致命告警既要当场可见，也要在自动切前台后依然可见 ——
                    // 所以同时收集起来，等 session 建好后播种进它的回看缓冲。
                    //
                    // 走到这里的 warns **只可能是限速类配置告警**：凭据展开失败会同时
                    // 进 missing，已在上方整体中止。所以前缀不能写「凭据展开」（原文案
                    // 从旧代码沿用而来，当时 warns 确实混着两类），否则限速告警被
                    // 标成凭据问题，玩家会去查 .env 而不是查 cmds_per_sec
                    let mut load_notes: Vec<String> = Vec::new();
                    for w in &warns {
                        let line = format!("[警告] 配置: {}", w);
                        self.sys_output(&line)?;
                        load_notes.push(line);
                    }

                    let session_id = match self.manager.add_connection_dynamic(&conn_config) {
                        Ok(id) => id,
                        Err(e) => {
                            self.sys_output(&format!("[错误] {}", e))?;
                            return Ok(());
                        }
                    };

                    // 启动渲染定时器（非实时模式且配置了 render_interval > 0）
                    if !conn_config.realtime && conn_config.render_interval > 0 {
                        self.start_render_tick_timer(session_id, conn_config.render_interval);
                    }

                    // 设置日志保留数量
                    if let Some(count) = conn_config.log_rotation_count {
                        self.logger.set_session_max_files(&conn_config.name, count);
                    }
                    // 登记凭据，避免脚本发送登录命令时明文落盘
                    self.logger
                        .set_session_secrets(&conn_config.name, &conn_config.credential_secrets());

                    let loading_msg = format!(
                        "[系统] 正在从配置文件加载角色 '{}' 并连接 ({}:{})",
                        conn_config.name, conn_config.host, conn_config.port
                    );

                    // 播种进新 session 的回看缓冲：connect 成功后 perform_connect 会自动
                    // switch_foreground，而它用新 session 的 output_lines 整体 replace_output ——
                    // 上面这些消息此刻只存在于旧前台缓冲，不播种就会被抹掉。
                    // 必须在 try_send 之前完成：否则 connect 事件一旦被处理就来不及了。
                    load_notes.push(loading_msg.clone());
                    let cap = self.config.general.scroll_buffer;
                    if let Some(session) = self.manager.get_mut_by_id(session_id) {
                        for note in &load_notes {
                            push_session_output_capped(&mut session.output_lines, note, cap);
                        }
                    }
                    // try_send 前置到所有可失败的 stdout 输出之前（update_status_bar 与
                    // sys_output 内部都要 flush stdout，管道断裂/写失败时返回 Err），只为保证
                    // connect 请求已入队。注意这不是为了防「孤儿 session 阻断重试」：Err 会一路
                    // 传播到 run() 触发整个 app 退出（本 app 对 stdout 断裂的全局设计就是退出），
                    // 重启后 profile 会重新加载。update_status_bar 一并移到 try_send 之后，是因为
                    // 它排在前面时一旦失败，connect 连入队都做不到。
                    let _ = self.connect_tx.try_send(ConnectRequest { session_id });
                    self.update_status_bar()?;
                    self.sys_output(&loading_msg)?;
                }
            },

            BuiltinCommand::All { cmd } => {
                // 判断是否为客户端命令（以 / 开头）
                if cmd.starts_with('/') {
                    self.handle_all_client_command(&cmd)?;
                } else {
                    // 普通命令，直接发送到所有连接的服务器
                    let results = self.manager.send_to_all(&cmd);
                    let count = results.len();
                    let mut ok_count = 0;
                    for (_session_id, name, result) in &results {
                        match result {
                            Ok(()) => ok_count += 1,
                            Err(e) => {
                                self.terminal.append_output(&format!(
                                    "[错误] 向 {} 发送命令失败: {}",
                                    name, e
                                ))?;
                            }
                        }
                    }
                    self.terminal.append_output(&format!(
                        "[系统] /all: 已向 {}/{} 个连接发送指令",
                        ok_count, count
                    ))?;
                    self.logger.log_command(Logger::BROADCAST_SESSION, &cmd);
                }
            }

            BuiltinCommand::Unknown => {
                self.terminal.append_output("内置命令:")?;
                self.terminal
                    .append_output("  /connect <名> <主机:端口>   添加并连接新角色")?;
                self.terminal
                    .append_output("  /connect <名> <主机> <端口> 同上")?;
                self.terminal
                    .append_output("  /disconnect [编号]           断开连接（保留 session）")?;
                self.terminal
                    .append_output("  /reconnect [编号]           断开并重新连接")?;
                self.terminal
                    .append_output("  /close [编号]               彻底关闭并移除 session")?;
                self.terminal
                    .append_output("  /list                       列出所有连接")?;
                self.terminal
                    .append_output("  /load <脚本路径>            为前台连接加载 Lua 脚本")?;
                self.terminal
                    .append_output("  /load reload                重新加载前台连接的 Lua 脚本")?;
                self.terminal
                    .append_output("  /lua <Lua 代码>             直接执行 Lua 代码")?;
                self.terminal
                    .append_output("  /set keep_command on|off     执行后保留命令栏输入")?;
                self.terminal
                    .append_output("  /set realtime on|off          实时/节流渲染模式切换")?;
                self.terminal.append_output(
                    "  /set render_interval <毫秒>  设置渲染间隔（0=实时，默认1000）",
                )?;
                self.terminal
                    .append_output("  /switch <编号或名称>        切换到指定连接")?;
                self.terminal
                    .append_output("  /sw <编号或名称>            切换到指定连接 (简写)")?;
                self.terminal
                    .append_output("  /profile list               列出 profiles/ 下可用角色")?;
                self.terminal.append_output(
                    "  /profile load <角色名>      从 profiles/ 加载角色配置并连接",
                )?;
                self.terminal
                    .append_output("  /all <命令>                  向所有连接发送指令")?;
                self.terminal
                    .append_output("  Alt+0~9                     切换前台连接 (最多10个)")?;
                self.terminal
                    .append_output("  Alt+←/→                     循环切换前台连接")?;
            }
        }
        Ok(())
    }
    /// 处理 /all 后的客户端命令（以 / 开头），逐 session 执行
    pub(crate) fn handle_all_client_command(&mut self, cmd: &str) -> io::Result<()> {
        let inner = cmd.strip_prefix('/').unwrap_or("");
        let parts: Vec<&str> = inner.split_whitespace().collect();
        if parts.is_empty() {
            return Ok(());
        }

        // 注：新增子命令须同步此白名单与下方分发 match（否则会落到兵底错误分支）。
        let safe = match parts[0] {
            "lua" | "reload" | "disconnect" | "reconnect" => true,
            "load" if parts.len() >= 2 => true,
            "list" => true,
            _ => false,
        };

        if !safe {
            self.terminal.append_output(&format!(
                "[错误] /all 不允许广播客户端命令 /{}。允许: /lua, /reload, /load, /list, /disconnect, /reconnect",
                parts[0]
            ))?;
            return Ok(());
        }

        let session_count = self.manager.session_count();
        let session_ids: Vec<SessionId> = self.manager.ordered_session_ids().to_vec();

        match parts[0] {
            "lua" => {
                let code = inner.strip_prefix("lua").map(|s| s.trim()).unwrap_or("");
                if code.is_empty() {
                    self.terminal
                        .append_output("[错误] /all /lua 需要 Lua 代码参数")?;
                    return Ok(());
                }
                let mut executed = 0usize;
                let mut skipped = 0usize;
                for &sid in &session_ids {
                    let name = self
                        .manager
                        .get_by_id(sid)
                        .map(|s| s.name.clone())
                        .unwrap_or_default();
                    if let Some(engine) = self
                        .manager
                        .get_by_id(sid)
                        .and_then(|s| s.lua_engine.as_ref())
                    {
                        self.logger.log_lua(&name, code);
                        match engine.eval_code(code) {
                            Ok(_) => {
                                let _ = self.send_lua_commands(sid, engine.drain_commands());
                                let _ = self.send_lua_raw(sid);
                                let _ = self.drain_lua_logs(sid);
                                executed += 1;
                            }
                            Err(e) => {
                                self.terminal.append_output(&format!(
                                    "[错误] /all /lua [{}]: {}",
                                    name, e
                                ))?;
                            }
                        }
                    } else {
                        self.terminal
                            .append_output(&format!("[错误] /all /lua [{}]: 未加载脚本", name))?;
                        skipped += 1;
                    }
                }
                self.update_status_bar()?;
                let extra = if skipped > 0 {
                    format!("，{} 个未加载脚本被跳过", skipped)
                } else {
                    String::new()
                };
                self.terminal.append_output(&format!(
                    "[系统] /all /lua: 在 {}/{} 个连接上执行{}",
                    executed, session_count, extra
                ))?;
            }
            "reload" | "load" => {
                let is_reload =
                    parts[0] == "reload" || parts.get(1).is_some_and(|&p| p == "reload");
                let mut executed = 0usize;
                for &sid in &session_ids {
                    let name = self
                        .manager
                        .get_by_id(sid)
                        .map(|s| s.name.clone())
                        .unwrap_or_default();
                    if is_reload {
                        let path = self
                            .manager
                            .get_by_id(sid)
                            .and_then(|s| s.lua_engine.as_ref())
                            .and_then(|e| e.script_path());
                        if let Some(path) = path {
                            // 保存连接状态（host/port/char_name 等，供 GetInfo 返回）
                            let saved_conn = self
                                .manager
                                .get_by_id(sid)
                                .and_then(|s| s.lua_engine.as_ref())
                                .map(|e| e.get_connection_state());
                            // 保存 session 的登录凭证（reload 后需重新注入 Lua 全局变量）
                            let (s_username, s_password) = self
                                .manager
                                .get_by_id(sid)
                                .map(|s| (s.username.clone(), s.password.clone()))
                                .unwrap_or_default();
                            match crate::lua::LuaEngine::new() {
                                Ok(mut engine) => {
                                    // 恢复连接状态（GetInfo 需要），不恢复 variables——脚本顶层代码会重新初始化
                                    if let Some(ref conn) = saved_conn {
                                        engine.restore_connection_state(conn);
                                    }
                                    // 重新注入登录凭证（与 init_lua_for_session 一致）：
                                    // 脚本顶层 me.charid=char_name 依赖该全局变量
                                    for w in inject_session_credentials(
                                        &mut engine,
                                        &s_username,
                                        &s_password,
                                    ) {
                                        self.sys_output(&format!("[警告] [{}] {}", name, w))?;
                                    }
                                    // 重新注入日志目录（供 GetInfo(58) 返回，不在 ConnectionState 中）
                                    engine.set_log_dir(&self.config.general.log_dir);
                                    match engine.load_script(&path) {
                                        Ok(()) => {
                                            // 排空脚本加载期间 run/Execute 压入的命令，避免下次
                                            // timer tick 触发 debug_assert! 崩溃
                                            self.drain_engine_commands(sid, &mut engine, &name)?;
                                            // 排空脚本加载期间的 Lua 日志
                                            if let Some(session) = self.manager.get_mut_by_id(sid) {
                                                session.lua_engine = Some(engine);
                                            }
                                            self.drain_lua_logs(sid)?;
                                            // 立即重启该 session 的 timer：即使后续 session 处理
                                            // 中途出错提前返回，已 reload 成功的定时器不受影响
                                            self.start_timers_for_session(sid);
                                            executed += 1;
                                        }
                                        Err(e) => {
                                            self.terminal.append_output(&format!(
                                                "[错误] /all /reload [{}]: {}",
                                                name, e
                                            ))?;
                                        }
                                    }
                                }
                                Err(e) => {
                                    self.terminal.append_output(&format!(
                                        "[错误] /all /reload [{}]: {}",
                                        name, e
                                    ))?;
                                }
                            }
                        } else {
                            self.terminal.append_output(&format!(
                                "[错误] /all /reload [{}]: 无已加载脚本",
                                name
                            ))?;
                        }
                    } else {
                        let path = parts[1].to_string();
                        // /all /load 与连接路径对齐：恢复连接状态 + 注入凭证 + log_dir + drain
                        let saved_conn = self
                            .manager
                            .get_by_id(sid)
                            .and_then(|s| s.lua_engine.as_ref())
                            .map(|e| e.get_connection_state());
                        let (l_username, l_password) = self
                            .manager
                            .get_by_id(sid)
                            .map(|s| (s.username.clone(), s.password.clone()))
                            .unwrap_or_default();
                        match crate::lua::LuaEngine::new() {
                            Ok(mut engine) => {
                                if let Some(ref conn) = saved_conn {
                                    engine.restore_connection_state(conn);
                                }
                                for w in inject_session_credentials(
                                    &mut engine,
                                    &l_username,
                                    &l_password,
                                ) {
                                    self.sys_output(&format!("[警告] [{}] {}", name, w))?;
                                }
                                engine.set_log_dir(&self.config.general.log_dir);
                                match engine.load_script(&path) {
                                    Ok(()) => {
                                        self.drain_engine_commands(sid, &mut engine, &name)?;
                                        if let Some(session) = self.manager.get_mut_by_id(sid) {
                                            session.lua_engine = Some(engine);
                                        }
                                        self.drain_lua_logs(sid)?;
                                        self.start_timers_for_session(sid);
                                        executed += 1;
                                    }
                                    Err(e) => {
                                        self.terminal.append_output(&format!(
                                            "[错误] /all /load [{}]: {}",
                                            name, e
                                        ))?;
                                    }
                                }
                            }
                            Err(e) => {
                                self.terminal.append_output(&format!(
                                    "[错误] /all /load [{}]: {}",
                                    name, e
                                ))?;
                            }
                        }
                    }
                }
                self.terminal.append_output(&format!(
                    "[系统] /all /{}: 在 {}/{} 个连接上执行",
                    parts[0], executed, session_count
                ))?;
                self.update_status_bar()?;
            }
            "list" => {
                return self.handle_builtin_command("/list");
            }
            "disconnect" => {
                for &sid in &session_ids {
                    if let Some(session) = self.manager.get_mut_by_id(sid) {
                        session.disconnect();
                        session.state = crate::connection::SessionState::Disconnected;
                    }
                }
                self.update_status_bar()?;
                self.terminal.append_output(&format!(
                    "[系统] /all /disconnect: 已断开 {} 个连接",
                    session_count
                ))?;
            }
            "reconnect" => {
                for &sid in &session_ids {
                    let name = self
                        .manager
                        .get_by_id(sid)
                        .map(|s| s.name.clone())
                        .unwrap_or_default();
                    if let Some(session) = self.manager.get_mut_by_id(sid) {
                        session.disconnect();
                        session.state = crate::connection::SessionState::Disconnected;
                    }
                    let display_pos = self.manager.display_number_of(sid);
                    self.terminal
                        .append_output(&format!("[系统] 正在重连 {} ({})...", display_pos, name))?;
                    let _ = self
                        .reconnect_tx
                        .try_send(ReconnectRequest { session_id: sid });
                }
                self.update_status_bar()?;
            }
            _ => {
                // 防御性兵底：理论上白名单与上方分发 match 已对齐，不会走到这里；
                // 万一两侧不同步，给优雅错误提示而非 unreachable!() panic。
                self.terminal
                    .append_output(&format!("[错误] /all 不支持的子命令: {}", parts[0]))?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{dynamic_connect_config, inject_session_credentials};
    use crate::lua::LuaEngine;

    /// /connect 动态建连的语义：GBK 编码、不自建连（本函数已发 ConnectRequest）、
    /// 断线自动重连、日志保留数走全局默认（None）。这些字段以前以内联字面量写死，
    /// 抽成 dynamic_connect_config 后由本测试锁定，避免随 config.rs 的默认值漂移。
    #[test]
    fn test_dynamic_connect_config_semantics() {
        let cfg = dynamic_connect_config("char1", "mud.example.com", 4000);
        assert_eq!(cfg.encoding.as_deref(), Some("gbk"), "动态建连应固定 GBK");
        assert!(
            !cfg.auto_connect,
            "auto_connect 必须为 false，否则与 connect_tx 重复建连"
        );
        assert!(cfg.auto_reconnect, "动态建连应默认自动重连");
        assert_eq!(
            cfg.log_rotation_count, None,
            "未指定时日志保留数应走全局默认"
        );
        assert_eq!(cfg.name, "char1");
        assert_eq!(cfg.host, "mud.example.com");
        assert_eq!(cfg.port, 4000);
    }

    /// P1 回归：凭据缺失必须产出告警而不是静默跳过。
    /// 线上事故的成因之一即「静默跳过」——玩家只看到脚本里 charid 为 nil 的
    /// 远距离崩溃，看不到「凭据根本没注入」这个真因。
    #[test]
    fn test_inject_credentials_warns_when_username_missing() {
        let mut engine = LuaEngine::new().unwrap();
        let warns = inject_session_credentials(&mut engine, &None, &Some("secret".to_string()));
        assert_eq!(warns.len(), 1, "只应告警 username 一项: {:?}", warns);
        assert!(
            warns[0].contains("username") && warns[0].contains("char_name"),
            "告警应点名缺失字段与受影响的全局变量: {}",
            warns[0]
        );
        // password 不应被连带清空
        assert_eq!(
            engine.eval_to_string("return char_password").unwrap(),
            "secret"
        );
        // char_name 确实未注入（nil），脚本顶层拼接会崩 —— 这正是必须告警的原因
        assert!(
            engine.eval_to_string("return char_name").is_err(),
            "未注入时 char_name 应为 nil"
        );
    }

    /// 空字符串与 None 同等对待：`.env` 里写了 `MUD_X_USER=`（值为空）时
    /// 占位符展开成功但内容为空，同样会让脚本拿到 nil，必须告警。
    #[test]
    fn test_inject_credentials_warns_when_credentials_empty() {
        let mut engine = LuaEngine::new().unwrap();
        let warns =
            inject_session_credentials(&mut engine, &Some(String::new()), &Some(String::new()));
        assert_eq!(warns.len(), 2, "username 与 password 都应告警: {:?}", warns);
        assert!(warns.iter().any(|w| w.contains("char_name")));
        assert!(warns.iter().any(|w| w.contains("char_password")));
    }

    #[test]
    fn test_inject_credentials_no_warn_when_present() {
        let mut engine = LuaEngine::new().unwrap();
        let warns = inject_session_credentials(
            &mut engine,
            &Some("fcriar".to_string()),
            &Some("secret".to_string()),
        );
        assert!(warns.is_empty(), "凭据齐全时不应产生告警: {:?}", warns);
        assert_eq!(engine.eval_to_string("return char_name").unwrap(), "fcriar");
        assert_eq!(
            engine.eval_to_string("return char_password").unwrap(),
            "secret"
        );
        // set_char_name 同步进 ConnectionState，GetInfo(3) 才能返回角色名
        assert_eq!(
            engine.eval_to_string("return GetInfo(3)").unwrap(),
            "fcriar"
        );
    }
}
