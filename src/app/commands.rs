// 内置命令执行（/connect /load /lua /set /switch /profile /all 等）
// 从 app.rs 拆分而来

use std::fs;
use std::io;
use std::path::Path;

use crate::config::AppConfig;
use crate::connection::SessionId;

use super::parse::{format_lua_error, parse_builtin_command, BuiltinCommand, ProfileSubcommand};
use super::session::{ConnectRequest, ReconnectRequest};
use super::{App, TermSettings};

impl App {
    /// 处理内置命令（基于 parse_builtin_command 分发）
    pub(crate) fn handle_builtin_command(&mut self, cmd: &str) -> io::Result<()> {
        match parse_builtin_command(cmd) {
            BuiltinCommand::Connect { name, host, port } => {
                let conn_config = crate::config::ConnectionConfig {
                    name: name.clone(),
                    host: host.clone(),
                    port,
                    encoding: Some("gbk".to_string()),
                    script: None,
                    auto_connect: false,
                    auto_reconnect: true,
                    reconnect_delay_secs: 5,
                    username: None,
                    password: None,
                    socks5_enable: false,
                    socks5_host: None,
                    socks5_port: 1080,
                    socks5_username: None,
                    socks5_password: None,
                    log_rotation_count: None,
                    render_interval: 1000,
                    realtime: false,
                    connect_delay_ms: 1000,
                    cmd_interval_ms: 50,
                    burst_size: 10,
                    cmds_per_sec: 20,
                    reconnect_max_secs: 1800,
                    idle_timeout_secs: 300,
                    heartbeat_cmd: String::new(),
                    heartbeat_timeout_secs: 60,
                };

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
                match crate::lua::LuaEngine::new() {
                    Ok(mut engine) => match engine.load_script(&path) {
                        Ok(()) => {
                            if let Some(session) = self.manager.get_mut_by_id(fg_id) {
                                session.lua_engine = Some(engine);
                            }
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
                    },
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
                // 保存原 engine 的变量（如 char_name 等）
                let saved_vars = self
                    .manager
                    .get_by_id(fg_id)
                    .and_then(|s| s.lua_engine.as_ref())
                    .map(|e| e.get_variables());
                // 保存原 engine 的连接状态
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
                if let Some(path) = script_path {
                    match crate::lua::LuaEngine::new() {
                        Ok(mut engine) => {
                            // 恢复之前保存的变量
                            if let Some(ref vars) = saved_vars {
                                for (k, v) in vars {
                                    engine.set_variable(k, v);
                                    engine.set_global(k, v);
                                }
                            }
                            // 恢复之前保存的连接状态
                            if let Some(ref conn_state) = saved_conn_state {
                                engine.restore_connection_state(conn_state);
                            }
                            match engine.load_script(&path) {
                                Ok(()) => {
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
                    .save();
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
                        self.terminal
                            .append_output("[错误] 不能加载示例配置文件 (example.toml)")?;
                        return Ok(());
                    }
                    let profile_dir = &self.config.general.profile_dir;
                    let profile_path = Path::new(profile_dir).join(format!("{}.toml", name));
                    if !profile_path.exists() {
                        self.terminal.append_output(&format!(
                            "[错误] 角色配置不存在: {}",
                            profile_path.display()
                        ))?;
                        return Ok(());
                    }
                    let content = match fs::read_to_string(&profile_path) {
                        Ok(c) => c,
                        Err(e) => {
                            self.terminal.append_output(&format!(
                                "[错误] 无法读取配置文件 {}: {}",
                                profile_path.display(),
                                e
                            ))?;
                            return Ok(());
                        }
                    };
                    let conn_config =
                        match toml::from_str::<crate::config::ConnectionConfig>(&content) {
                            Ok(c) => c,
                            Err(e) => {
                                self.terminal
                                    .append_output(&format!("[错误] 配置文件格式错误: {}", e))?;
                                return Ok(());
                            }
                        };

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

                    // 设置日志保留数量
                    if let Some(count) = conn_config.log_rotation_count {
                        self.logger.set_session_max_files(&conn_config.name, count);
                    }

                    self.update_status_bar()?;
                    let _ = self.connect_tx.try_send(ConnectRequest { session_id });
                    self.terminal.append_output(&format!(
                        "[系统] 正在从配置文件加载角色 '{}' 并连接 ({}:{})",
                        conn_config.name, conn_config.host, conn_config.port
                    ))?;
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
                    self.logger.log_command("all", &cmd);
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
                            let saved_vars = self
                                .manager
                                .get_by_id(sid)
                                .and_then(|s| s.lua_engine.as_ref())
                                .map(|e| e.get_variables());
                            let saved_conn = self
                                .manager
                                .get_by_id(sid)
                                .and_then(|s| s.lua_engine.as_ref())
                                .map(|e| e.get_connection_state());
                            match crate::lua::LuaEngine::new() {
                                Ok(mut engine) => {
                                    if let Some(ref vars) = saved_vars {
                                        for (k, v) in vars {
                                            engine.set_variable(k, v);
                                            engine.set_global(k, v);
                                        }
                                    }
                                    if let Some(ref conn) = saved_conn {
                                        engine.restore_connection_state(conn);
                                    }
                                    match engine.load_script(&path) {
                                        Ok(()) => {
                                            // 排空脚本加载期间的 Lua 日志
                                            if let Some(session) = self.manager.get_mut_by_id(sid) {
                                                session.lua_engine = Some(engine);
                                            }
                                            self.drain_lua_logs(sid)?;
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
                        match crate::lua::LuaEngine::new() {
                            Ok(mut engine) => match engine.load_script(&path) {
                                Ok(()) => {
                                    if let Some(session) = self.manager.get_mut_by_id(sid) {
                                        session.lua_engine = Some(engine);
                                    }
                                    self.start_timers_for_session(sid);
                                    executed += 1;
                                }
                                Err(e) => {
                                    self.terminal.append_output(&format!(
                                        "[错误] /all /load [{}]: {}",
                                        name, e
                                    ))?;
                                }
                            },
                            Err(e) => {
                                self.terminal.append_output(&format!(
                                    "[错误] /all /load [{}]: {}",
                                    name, e
                                ))?;
                            }
                        }
                    }
                }
                if is_reload && executed > 0 {
                    for &sid in &session_ids {
                        self.start_timers_for_session(sid);
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
            _ => unreachable!(),
        }
        Ok(())
    }
}
