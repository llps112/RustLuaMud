// 终端输入与连接事件处理
// 从 app.rs 拆分而来：键盘/鼠标事件、ManagerEvent、前台切换、状态栏

use std::io::{self, Write};

use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};

use crate::connection::{ManagerEvent, SessionId, SessionState};
use crate::ui::AnsiParser;

use super::parse::split_commands;
use super::session::ReconnectRequest;
use super::App;

impl App {
    /// 处理鼠标事件
    pub(crate) fn handle_mouse_event(
        &mut self,
        mouse: crossterm::event::MouseEvent,
    ) -> io::Result<()> {
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
            // 1. 先检查面板按钮（优先级高于状态栏）
            let fg_id = self.manager.foreground_id;
            if let Some((panel_name, action)) =
                self.terminal.panel_hit_test(mouse.column, mouse.row)
            {
                let commands = self
                    .manager
                    .get_by_id(fg_id)
                    .and_then(|s| s.lua_engine.as_ref())
                    .map(|engine| {
                        engine.handle_panel_click(&panel_name, &action);
                        engine.drain_commands()
                    })
                    .unwrap_or_default();
                if !commands.is_empty() {
                    self.send_lua_commands(fg_id, commands)?;
                }
                // 这是唯一一处 send_lua_commands 之后没有跟随 drain_lua_logs 的调用点
                // （其余 6 处：events.rs 的用户输入与 MUD 输出、commands.rs 的 /lua 与
                // /all /lua、session.rs 的两处定时器 tick 都紧跟 drain）。漏排空会让面板
                // 点击回调里的 print/Note 一直挂在引擎 pending_logs，直到下一次 MUD 输出
                // 或定时器 tick 才落盘，日志时间戳与点击动作错位，排障时对不上因果。
                // drain_lua_logs 末尾已调用 drain_lua_panels，此处不再重复。
                self.send_lua_raw(fg_id)?;
                self.drain_lua_logs(fg_id)?;
                return Ok(());
            }

            // 2. 再检查状态栏 tab（session 状态栏固定在屏幕顶行，行号由布局给出）
            if mouse.row == self.terminal.status_row() {
                let x = mouse.column;
                for region in self.terminal.click_regions() {
                    if x >= region.start_x && x < region.end_x {
                        if self.manager.get_by_id(region.session_id).is_some() {
                            self.switch_foreground(region.session_id)?;
                        }
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    /// 处理键盘事件
    pub(crate) fn handle_key_event(&mut self, key: crossterm::event::KeyEvent) -> io::Result<()> {
        // Windows 上 crossterm 对同一按键会产生 Press 和 Release 两个事件，
        // 不过滤会导致每个按键被处理两次（表现为输入重复）。
        // Linux 的 raw 模式下只产生 Press/Repeat 事件，此判断跨平台安全。
        if key.kind == KeyEventKind::Release {
            return Ok(());
        }
        // Ctrl+C / Ctrl+D: 退出
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && (key.code == KeyCode::Char('c') || key.code == KeyCode::Char('d'))
        {
            self.running = false;
            return Ok(());
        }

        // Alt+1~9: 切换到第 1~9 个连接, Alt+0: 切换到第 10 个连接
        // 支持两种模式：
        // 1. 标准模式：带 ALT 修饰符的数字键
        // 2. xterm 8-bit 模式：Alt+数字 发送高位字符 (U+00B0~U+00B9)
        if key.modifiers.contains(KeyModifiers::ALT) {
            if let KeyCode::Char(c) = key.code {
                if let Some(digit) = c.to_digit(10) {
                    let display_num = if digit == 0 { 10 } else { digit as usize };
                    if let Some(session_id) = self.manager.session_id_by_display_number(display_num)
                    {
                        self.switch_foreground(session_id)?;
                    }
                    return Ok(());
                }
            }
        }

        // xterm 8-bit 模式：Alt+数字 发送高位字符 (0x30 | 0x80 = 0xB0)
        // U+00B0 (°) = Alt+0, U+00B1 (±) = Alt+1, ..., U+00B9 (¹) = Alt+9
        if let KeyCode::Char(c) = key.code {
            if let Some(digit) = Self::parse_xterm_alt_digit(c) {
                let display_num = if digit == 0 { 10 } else { digit as usize };
                if let Some(session_id) = self.manager.session_id_by_display_number(display_num) {
                    self.switch_foreground(session_id)?;
                }
                return Ok(());
            }
        }

        // Alt+Left: 切换到前一个连接 (循环), Alt+Right: 切换到后一个连接 (循环)
        if self.manager.session_count() > 0 {
            if key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Left {
                if let Some(new_id) = self.manager.cycle_foreground(-1) {
                    self.switch_foreground(new_id)?;
                }
                return Ok(());
            }
            if key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Right {
                if let Some(new_id) = self.manager.cycle_foreground(1) {
                    self.switch_foreground(new_id)?;
                }
                return Ok(());
            }
        }

        // 其他按键交给终端处理
        if let Some(cmd) = self.terminal.handle_key(key) {
            // 用户按了 Enter，提交命令
            if !cmd.is_empty() {
                self.terminal
                    .append_output(&format!("> \x1b[38;2;255;235;59m{}\x1b[0m", cmd))?;
                // 处理内置命令（以 / 开头）
                if cmd.starts_with('/') {
                    self.handle_builtin_command(&cmd)?;
                } else {
                    // 检查是否包含分号，如果有则拆分处理
                    let commands = if cmd.contains(';') {
                        split_commands(&cmd)
                    } else {
                        vec![cmd]
                    };

                    // 逐条处理命令
                    for single_cmd in commands {
                        // 先尝试别名匹配
                        let fg_id = self.manager.foreground_id;
                        let alias_handled = if let Some(engine) = self
                            .manager
                            .get_by_id(fg_id)
                            .and_then(|s| s.lua_engine.as_ref())
                        {
                            let handled = engine.process_input(&single_cmd);
                            if handled {
                                let commands = engine.drain_commands();
                                self.send_lua_commands(fg_id, commands)?;
                                self.drain_lua_logs(fg_id)?;
                            } else {
                                self.drain_lua_logs(fg_id)?;
                            }
                            handled
                        } else {
                            false
                        };

                        if !alias_handled {
                            // 无别名匹配，发送到前台连接
                            if let Some(fg) = self.manager.get_by_id(self.manager.foreground_id) {
                                self.logger.log_command(&fg.name, &single_cmd);
                            }
                            if let Err(e) = self.manager.send_to_foreground(&single_cmd) {
                                self.terminal.append_output(&format!("[错误] {}", e))?;
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// 解析 xterm 8-bit 模式的 Alt+数字
    /// xterm 在 8-bit 模式下，Alt+数字 会发送 U+00B0~U+00B9 范围的字符
    /// 例如：Alt+1 → U+00B1 (±), Alt+2 → U+00B2 (²)
    pub(crate) fn parse_xterm_alt_digit(c: char) -> Option<u8> {
        let code = c as u32;
        if (0x00B0..=0x00B9).contains(&code) {
            Some((code - 0x00B0) as u8)
        } else {
            None
        }
    }
    /// 处理连接管理器事件
    pub(crate) fn handle_manager_event(&mut self, event: ManagerEvent) -> io::Result<()> {
        match event {
            ManagerEvent::Data(id, data) => {
                if self.manager.get_by_id(id).is_none() {
                    return Ok(());
                }
                // 更新心跳计时器
                if let Some(session) = self.manager.get_mut_by_id(id) {
                    session.last_recv_time = std::time::Instant::now();
                    session.heartbeat_sent = None;
                }
                let is_realtime = self
                    .manager
                    .get_by_id(id)
                    .map(|s| s.realtime)
                    .unwrap_or(false);
                let max_lines = self.config.general.scroll_buffer;

                // ===== 阶段 1：触发器处理（收集 omit 标志 + 命令 + Lua 日志）=====
                // 先运行触发器，得到每行是否被 omit_from_output 抑制，
                // 再据此决定阶段 2 的显示/入缓冲行为。
                let mut omit_flags: Vec<bool> = Vec::new();
                let mut all_cmds: Vec<String> = Vec::new();
                let mut pending_lua_logs: Vec<String> = Vec::new();
                if let Some(engine) = self
                    .manager
                    .get_by_id(id)
                    .and_then(|s| s.lua_engine.as_ref())
                {
                    // 对每行数据分别匹配触发器
                    for part in data.split_inclusive('\n') {
                        let trimmed = part.trim_end_matches(['\r', '\n']);
                        if trimmed.is_empty() {
                            // 空行占位，保持与阶段 2 索引对齐
                            omit_flags.push(false);
                            continue;
                        }
                        let omitted = engine.process_output(trimmed);
                        omit_flags.push(omitted);
                        // 延迟期内 trigger 命令放入延迟队列，等到期后统一发送
                        if engine.has_pending_delayed_on_connect() {
                            engine.drain_commands_to_delayed();
                        } else {
                            all_cmds.extend(engine.drain_commands());
                        }
                    }
                    // 收集 Lua 日志（写入文件 + 暂存待终端输出）
                    pending_lua_logs = engine.drain_logs();
                    let name = self
                        .manager
                        .get_by_id(id)
                        .map(|s| s.name.clone())
                        .unwrap_or_default();
                    for msg in &pending_lua_logs {
                        let clean = crate::ui::AnsiParser::strip_ansi(msg);
                        self.logger.log(&name, &clean);
                    }
                }

                // ===== 阶段 2：显示 + 回看缓冲（按 omit 标志逐行决定）=====
                // omit_from_output=true 的行：跳过 output_lines 入栈 + 跳过终端显示
                // 日志文件不受 omit_from_output 影响（MUSHclient 语义，omit_from_log 才影响日志）

                for (i, part) in data.split_inclusive('\n').enumerate() {
                    let trimmed = part.trim_end_matches(['\r', '\n']);
                    if trimmed.is_empty() {
                        continue;
                    }

                    let omitted = omit_flags.get(i).copied().unwrap_or(false);
                    if omitted {
                        continue;
                    }
                    // 服务端可能下发 ESC[2J（conhost 执行为整屏上滚）等危险转义序列，
                    // 原样渲染会物理顶出状态栏造成布局永久错位；入回看缓冲/渲染前过滤（保留 SGR 颜色）
                    let safe = crate::ui::terminal::strip_unsafe_escapes(trimmed);
                    // 滚动回看缓冲（所有 session）
                    if let Some(session) = self.manager.get_mut_by_id(id) {
                        session.output_lines.push(safe.clone());
                    }
                    // 仅渲染前台连接的数据
                    if id == self.manager.foreground_id {
                        if is_realtime {
                            // 实时渲染模式：逐行追加（支持逐行 omit）
                            self.terminal.append_output(&safe)?;
                        } else {
                            // 节流渲染模式：缓冲数据，等待定时器刷新
                            if let Some(session) = self.manager.get_mut_by_id(id) {
                                session.pending_data.push(safe);
                                session.render_dirty = true;
                            }
                        }
                    }
                }
                // 限制回看缓冲区大小
                if let Some(session) = self.manager.get_mut_by_id(id) {
                    if session.output_lines.len() > max_lines {
                        let drain_count = session.output_lines.len() - max_lines;
                        session.output_lines.drain(..drain_count);
                    }
                }

                // 所有连接数据写入日志（omit_from_output 不抑制日志）
                self.log_session_data(id, &data);

                // 发送触发器产生的命令
                self.send_lua_commands(id, all_cmds)?;

                // 处理 Lua 日志（写入日志文件已在阶段 1 完成）
                // 节流模式下缓冲到 pending_data，实时模式直接输出
                if !pending_lua_logs.is_empty() && id == self.manager.foreground_id {
                    if !is_realtime {
                        if let Some(session) = self.manager.get_mut_by_id(id) {
                            for msg in pending_lua_logs {
                                session
                                    .pending_data
                                    .push(format!("\x1b[36m[Lua] {}\x1b[0m", msg));
                            }
                            session.render_dirty = true;
                        }
                    } else {
                        for msg in pending_lua_logs {
                            let formatted = format!("\x1b[36m[Lua] {}\x1b[0m", msg);
                            // 保存到 session 的 output_lines，切换 session 时不会丢失
                            if let Some(session) = self.manager.get_mut_by_id(id) {
                                session.output_lines.push(formatted.clone());
                            }
                            self.terminal.append_output(&formatted)?;
                        }
                        // 限制回看缓冲区大小
                        if let Some(session) = self.manager.get_mut_by_id(id) {
                            if session.output_lines.len() > max_lines {
                                let drain_count = session.output_lines.len() - max_lines;
                                session.output_lines.drain(..drain_count);
                            }
                        }
                    }
                } else if !pending_lua_logs.is_empty() {
                    // 非前台 session 的 Lua 日志也需写入 session.output_lines
                    if let Some(session) = self.manager.get_mut_by_id(id) {
                        for msg in &pending_lua_logs {
                            session
                                .output_lines
                                .push(format!("\x1b[36m[Lua] {}\x1b[0m", msg));
                        }
                        if session.output_lines.len() > max_lines {
                            let drain_count = session.output_lines.len() - max_lines;
                            session.output_lines.drain(..drain_count);
                        }
                    }
                }
                // 排空引擎中剩余的 Lua 日志（触发器处理后又产生的）
                self.drain_lua_logs(id)?;
                // 发送 SendPkt 压入的原始数据包
                self.send_lua_raw(id)?;
                // 触发器中可能调用了 SetStatus，刷新状态栏
                if id == self.manager.foreground_id {
                    self.update_status_bar()?;
                }
            }
            ManagerEvent::StateChange(id, state) => {
                // 检查 session 是否仍然存在（可能已被 /close 移除）
                if self.manager.get_by_id(id).is_none() {
                    return Ok(());
                }
                if let Some(session) = self.manager.get_mut_by_id(id) {
                    session.state = state.clone();
                    // 同步 Lua 引擎的连接状态（同步到对应 session，不限于前台）
                    if let Some(ref mut engine) = session.lua_engine {
                        if state == SessionState::Connected {
                            engine.set_connected(true);
                        } else if state == SessionState::Disconnected {
                            let reason = session
                                .last_disconnect_reason
                                .clone()
                                .unwrap_or_else(|| "disconnected".to_string());
                            engine.notify_disconnect(&reason);
                        }
                    }
                    // 断线时清理发送通道，避免 Lua 引擎通过已关闭通道发送数据触发“channel closed”错误刷屏
                    if state == SessionState::Disconnected {
                        session.send_tx = None;
                        session.send_raw_tx = None;
                    }
                }
                if id == self.manager.foreground_id {
                    self.update_status_bar()?;
                }
                let state_str = match &state {
                    SessionState::Connected => "已连接",
                    SessionState::Disconnected => "已断开",
                    SessionState::Connecting => "连接中...",
                    SessionState::Reconnecting => "重连中...",
                };
                let name = self
                    .manager
                    .get_by_id(id)
                    .map(|s| s.name.clone())
                    .unwrap_or_default();
                let display_pos = self.manager.display_number_of(id);
                self.terminal.append_output(&format!(
                    "[系统] 连接 {} ({}) {}",
                    display_pos, name, state_str
                ))?;

                // 自动重连：断开时启动延迟重连任务
                if state == SessionState::Disconnected {
                    // 仅在未预设原因时设置默认断线原因（心跳超时等场景已预设）
                    if let Some(session) = self.manager.get_mut_by_id(id) {
                        if session.last_disconnect_reason.is_none() {
                            session.set_disconnect_reason("disconnected".to_string());
                        }
                    }
                    let (backoff, auto_reconnect) = self
                        .manager
                        .get_by_id(id)
                        .map(|s| (s.current_backoff_secs(), s.auto_reconnect))
                        .unwrap_or((5, false));
                    // 记录断线日志 [DCN]
                    if let Some(session) = self.manager.get_by_id(id) {
                        let reason = session
                            .last_disconnect_reason
                            .as_deref()
                            .unwrap_or("unknown");
                        self.logger.log_disconnect(&name, reason, backoff);
                    }
                    if auto_reconnect {
                        self.terminal.append_output(&format!(
                            "[系统] {} 秒后尝试重连 {}...",
                            backoff, name
                        ))?;
                        // 启动延迟重连任务
                        let tx = self.reconnect_tx.clone();
                        tokio::spawn(async move {
                            tokio::time::sleep(tokio::time::Duration::from_secs(backoff)).await;
                            let _ = tx.send(ReconnectRequest { session_id: id }).await;
                        });
                    }
                }
            }
            ManagerEvent::Error(id, err) => {
                let name = self
                    .manager
                    .get_by_id(id)
                    .map(|s| s.name.clone())
                    .unwrap_or_else(|| "未知".to_string());
                let display_pos = self.manager.display_number_of(id);
                self.terminal
                    .append_output(&format!("[错误] 连接 {} ({}): {}", display_pos, name, err))?;
            }
        }
        Ok(())
    }

    /// 更新状态栏（包括 session 状态栏和 Lua 状态栏）
    pub(crate) fn update_status_bar(&mut self) -> io::Result<()> {
        let mut stdout = io::stdout();
        let infos = self.manager.session_infos();
        let fg = self.manager.foreground_id;
        self.terminal.draw_status_bar(&mut stdout, &infos, fg)?;
        self.terminal.draw_lua_status_bar(&mut stdout, &infos, fg)?;
        // 将光标定位到输入行（draw_lua_status_bar 不再内部 flush）
        self.terminal.draw_input_line(&mut stdout)?;
        stdout.flush()?;
        Ok(())
    }
    /// 记录连接数据到日志文件（剥离 ANSI 码）
    pub(crate) fn log_session_data(&self, session_id: SessionId, data: &str) {
        let name = match self.manager.get_by_id(session_id) {
            Some(s) => &s.name,
            None => return,
        };
        let clean = AnsiParser::strip_ansi(data);
        self.logger.log(name, &clean);
    }

    /// 切换前台连接，恢复目标连接的输出缓冲区
    pub(crate) fn switch_foreground(&mut self, session_id: SessionId) -> io::Result<()> {
        // 保存当前前台 session 的输入状态和面板
        let old_id = self.manager.foreground_id;
        if self.manager.get_by_id(old_id).is_some() {
            let saved = self.terminal.save_input_state();
            let saved_panels = self.terminal.save_panels();
            if let Some(session) = self.manager.get_mut_by_id(old_id) {
                session.input_state = saved;
                session.panels = saved_panels;
            }
        }

        self.manager.switch_foreground(session_id).ok();
        self.update_status_bar()?;

        // 恢复目标 session 的输入状态和面板
        if let Some(saved) = self
            .manager
            .get_by_id(session_id)
            .map(|s| s.input_state.clone())
        {
            self.terminal.restore_input_state(&saved);
        }
        let saved_panels = self
            .manager
            .get_by_id(session_id)
            .map(|s| s.panels.clone())
            .unwrap_or_default();
        self.terminal.restore_panels(&saved_panels);

        // 恢复目标连接的输出缓冲区到终端
        // 预提取 display_pos 和 foreground_name，避免后续与 terminal 借用冲突
        let display_pos = self.manager.display_number_of(session_id);
        let fg_name = self.manager.foreground_name().to_string();
        // 更新 panic hook 上下文中的 session name，使 panic 信息写入正确的日志文件
        crate::log::panic_hook::set_current_session(&fg_name);
        // 拆分借用：manager（不可变）和 terminal（可变）是 App 的不同字段
        let empty = Vec::new();
        let output: &[String] = self
            .manager
            .get_by_id(session_id)
            .map(|s| s.output_lines.as_slice())
            .unwrap_or(&empty);
        self.terminal.replace_output(output)?;
        self.terminal
            .append_output(&format!("[系统] 切换到连接 {} ({})", display_pos, fg_name))?;

        // 立即排空新前台 session 的 pending_data，避免切换后显示延迟
        let pending = self
            .manager
            .get_mut_by_id(session_id)
            .map(|s| {
                s.render_dirty = false;
                std::mem::take(&mut s.pending_data)
            })
            .unwrap_or_default();
        if !pending.is_empty() {
            let mut combined = String::new();
            for line in &pending {
                combined.push_str(line);
                combined.push('\n');
            }
            if !combined.is_empty() {
                self.terminal.append_output(&combined)?;
            }
        }

        Ok(())
    }
}
