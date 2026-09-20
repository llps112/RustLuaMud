// Session 操作：连接/重连执行、Lua 引擎初始化、定时器、命令发送、日志/面板排空
// 从 app.rs 拆分而来

use std::collections::HashSet;
use std::io;

use tokio::sync::oneshot;

use crate::connection::{SessionId, SessionState};
use crate::lua::PanelUpdate;
use crate::ui::terminal::Panel;

use super::parse::{format_lua_error, split_commands};
use super::App;

/// 重连请求
pub(crate) struct ReconnectRequest {
    pub(crate) session_id: SessionId,
}

/// 动态连接请求
pub(crate) struct ConnectRequest {
    pub(crate) session_id: SessionId,
}

/// 定时器触发请求
pub(crate) struct TimerRequest {
    pub(crate) session_id: SessionId,
}

/// 渲染刷新请求
pub(crate) struct RenderTickRequest {
    pub(crate) session_id: SessionId,
}

impl App {
    /// 执行重连
    pub(crate) async fn perform_reconnect(&mut self, session_id: SessionId) -> io::Result<()> {
        let name = self
            .manager
            .get_by_id(session_id)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "未知".to_string());
        let display_pos = self.manager.display_number_of(session_id);

        match self.manager.connect_session(session_id).await {
            Ok(()) => {
                let msg = format!("[系统] 连接 {} ({}) 重连成功", display_pos, name);
                self.terminal.append_output(&msg)?;
                // 如果 Lua 引擎已存在（重连前已加载脚本），不重建引擎，
                // 保留 Lua 变量状态（stat.* 等统计数据）。仅通知引擎已连接以触发 OnConnect。
                // 若引擎不存在（首次连接或未加载脚本），则执行标准初始化流程。
                let has_engine = self
                    .manager
                    .get_by_id(session_id)
                    .map(|s| s.lua_engine.is_some())
                    .unwrap_or(false);
                if has_engine {
                    let connect_delay_ms = self
                        .manager
                        .get_by_id(session_id)
                        .map(|s| s.connect_delay_ms)
                        .unwrap_or(0);
                    let queued_cmds = {
                        let engine = self
                            .manager
                            .get_mut_by_id(session_id)
                            .and_then(|s| s.lua_engine.as_mut())
                            .unwrap();
                        engine.set_connect_delay(connect_delay_ms);
                        engine.set_connected(true);
                        if connect_delay_ms == 0 {
                            engine.drain_commands()
                        } else {
                            Vec::new()
                        }
                    };
                    for cmd in &queued_cmds {
                        self.logger.log_command(&name, cmd);
                        self.send_cmd_checked(session_id, cmd)?;
                    }
                    self.drain_lua_logs(session_id)?;
                    self.send_lua_raw(session_id)?;
                } else {
                    self.init_lua_for_session(session_id)?;
                }
                // 重连后刷新状态栏（Lua 脚本可能调用了 SetStatus）
                if session_id == self.manager.foreground_id {
                    self.update_status_bar()?;
                }
                // 重连成功，记录停机时长并重置退避计时
                if let Some(session) = self.manager.get_mut_by_id(session_id) {
                    let downtime = session.downtime_secs();
                    self.logger.log_reconnect(&name, downtime);
                    session.on_connect_success();
                    // 复位心跳状态：断线期间 last_recv_time / heartbeat_sent 停滞，
                    // 不复位的话重连成功后首个 tick 就会用旧时间戳误判心跳超时
                    session.last_recv_time = std::time::Instant::now();
                    session.heartbeat_sent = None;
                }
            }
            Err(e) => {
                let msg = format!("[系统] 重连 {} ({}) 失败: {}", display_pos, name, e);
                self.terminal.append_output(&msg)?;
                // 状态回退到 Disconnected
                if let Some(session) = self.manager.get_mut_by_id(session_id) {
                    session.state = crate::connection::SessionState::Disconnected;
                }
                // 计算指数退避延迟
                let (backoff, auto_reconnect) = self
                    .manager
                    .get_by_id(session_id)
                    .map(|s| (s.current_backoff_secs(), s.auto_reconnect))
                    .unwrap_or((5, false));
                if auto_reconnect {
                    if let Some(session) = self.manager.get_mut_by_id(session_id) {
                        session.on_reconnect_failure();
                    }
                    self.terminal.append_output(&format!(
                        "[系统] {} 秒后再次尝试重连 {}...",
                        backoff, name
                    ))?;
                    let tx = self.reconnect_tx.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(tokio::time::Duration::from_secs(backoff)).await;
                        let _ = tx.send(ReconnectRequest { session_id }).await;
                    });
                }
            }
        }
        Ok(())
    }

    /// 执行动态连接
    pub(crate) async fn perform_connect(&mut self, session_id: SessionId) -> io::Result<()> {
        let (name, host, port) = match self.manager.get_by_id(session_id) {
            Some(s) => (s.name.clone(), s.host.clone(), s.port),
            None => {
                self.sys_output_to(session_id, "[错误] 无效的连接 ID")?;
                return Ok(());
            }
        };
        let display_pos = self.manager.display_number_of(session_id);

        match self.manager.connect_session(session_id).await {
            Ok(()) => {
                let msg = format!(
                    "[系统] 连接 {} ({}) → {}:{} 已建立",
                    display_pos, name, host, port
                );
                // 三写而非只写终端：紧随其后的 switch_foreground 会 replace_output，
                // 只存在于终端缓冲的「已建立」会被抹掉
                self.sys_output_to(session_id, &msg)?;
                self.init_lua_for_session(session_id)?;
                // 自动切换到新连接
                self.switch_foreground(session_id)?;
            }
            Err(e) => {
                // 失败时不切前台，消息本来就能看见；走 sys_output_to 是为了落日志，
                // 否则挂机时无人盯屏，事后连「什么时候连不上」都查不到
                let msg = format!("[系统] 连接失败 ({}:{}): {}", host, port, e);
                self.sys_output_to(session_id, &msg)?;
            }
        }
        Ok(())
    }

    /// 为指定连接初始化 Lua 引擎并加载脚本
    pub(crate) fn init_lua_for_session(&mut self, session_id: SessionId) -> io::Result<()> {
        // 从 Session 自身获取配置
        let (name, script_path, username, password, host) = match self.manager.get_by_id(session_id)
        {
            Some(s) => (
                s.name.clone(),
                s.script_path.clone(),
                s.username.clone(),
                s.password.clone(),
                s.host.clone(),
            ),
            None => return Ok(()),
        };
        let display_pos = self.manager.display_number_of(session_id);

        match crate::lua::LuaEngine::new() {
            Ok(mut engine) => {
                // 注入主机地址（供 GetInfo(1) 返回）
                engine.set_host(&host);
                // 注入世界名称（供 GetInfo(2) 返回）
                engine.set_world_name(&name);
                // 注入日志目录（供 GetInfo(58) 返回）
                engine.set_log_dir(&self.config.general.log_dir);

                // 注入登录凭证到 Lua 变量和全局变量。
                // 缺失时必须显式告警，不能静默跳过：脚本顶层 me.charid = char_name 会得到 nil，
                // 直到 include("config_"..me.charid..".lua") 拼接时才崩，报错点离真因很远。
                // 仅在配了脚本时告警 —— 无脚本则无人读 char_name，告警是噪音。
                let has_script = script_path.is_some();
                match username.as_deref().filter(|u| !u.is_empty()) {
                    Some(uname) => {
                        engine.set_variable("char_name", uname);
                        engine.set_global("char_name", uname);
                        engine.set_char_name(uname); // 供 GetInfo(3) 返回
                    }
                    None if has_script => {
                        self.sys_output_to(
                            session_id,
                            &format!(
                                "[警告] 连接 {} ({}) 未配置 username，脚本中 char_name 将为 nil",
                                display_pos, name
                            ),
                        )?;
                    }
                    None => {}
                }
                match password.as_deref().filter(|p| !p.is_empty()) {
                    Some(pwd) => {
                        engine.set_variable("char_password", pwd);
                        engine.set_global("char_password", pwd);
                    }
                    None if has_script => {
                        self.sys_output_to(
                            session_id,
                            &format!(
                                "[警告] 连接 {} ({}) 未配置 password，脚本中 char_password 将为 nil",
                                display_pos, name
                            ),
                        )?;
                    }
                    None => {}
                }

                // 加载脚本
                if let Some(ref path) = script_path {
                    match engine.load_script(path) {
                        Ok(()) => {
                            // 排空脚本加载期间 Execute 等压入的命令（如 "/set_dl()"、"score" 等）
                            let queued_cmds = engine.drain_commands();
                            for cmd in &queued_cmds {
                                if let Some(lua_code) = cmd.strip_prefix('/') {
                                    if let Err(e) = engine.eval_code(lua_code) {
                                        self.sys_output_to(
                                            session_id,
                                            &format!("[Lua] 执行排队命令失败: {}", e),
                                        )?;
                                    }
                                } else {
                                    self.logger.log_command(&name, cmd);
                                    self.send_cmd_checked(session_id, cmd)?;
                                }
                            }

                            // 排空脚本加载期间的 Lua 日志
                            self.drain_lua_logs(session_id)?;

                            // 同样走三写：这条消息也在 switch_foreground 之前，只写终端会被抹掉
                            let msg = format!("[Lua] 连接 {} 脚本已加载: {}", display_pos, path);
                            self.sys_output_to(session_id, &msg)?;
                        }
                        Err(e) => {
                            // sys_output_to 已覆盖终端与日志，不需要再写两遍
                            for line in format_lua_error(&e.to_string()) {
                                self.sys_output_to(session_id, &line)?;
                            }
                        }
                    }
                }

                if let Some(s) = self.manager.get_mut_by_id(session_id) {
                    s.lua_engine = Some(engine);
                }
                // 同步连接状态：session.connect() 在创建事件通道前已设置 state，
                // 初始 Connected 状态不会通过 StateChange 事件到达 engine，
                // 此处手动同步，确保 engine 知道当前已连接并触发 alias.atconnect()
                {
                    let is_connected = self
                        .manager
                        .get_by_id(session_id)
                        .map(|s| matches!(s.state, crate::connection::SessionState::Connected))
                        .unwrap_or(false);
                    if is_connected {
                        let connect_delay_ms = self
                            .manager
                            .get_by_id(session_id)
                            .map(|s| s.connect_delay_ms)
                            .unwrap_or(0);
                        let queued_cmds = {
                            match self
                                .manager
                                .get_mut_by_id(session_id)
                                .and_then(|s| s.lua_engine.as_mut())
                            {
                                Some(eng) => {
                                    eng.set_connect_delay(connect_delay_ms);
                                    eng.set_connected(true);
                                    if connect_delay_ms == 0 {
                                        eng.drain_commands()
                                    } else {
                                        Vec::new()
                                    }
                                }
                                None => Vec::new(),
                            }
                        };
                        for cmd in &queued_cmds {
                            self.logger.log_command(&name, cmd);
                            self.send_cmd_checked(session_id, cmd)?;
                        }
                        self.drain_lua_logs(session_id)?;
                        self.send_lua_raw(session_id)?;
                    }
                }
                // 启动定时器
                self.start_timers_for_session(session_id);
            }
            Err(e) => {
                let msg = format!("[Lua] 连接 {} 引擎初始化失败: {}", display_pos, e);
                self.sys_output_to(session_id, &msg)?;
            }
        }
        Ok(())
    }

    /// 为指定连接启动定时器任务
    pub(crate) fn start_timers_for_session(&mut self, session_id: SessionId) {
        let session = match self.manager.get_mut_by_id(session_id) {
            Some(s) => s,
            None => return,
        };
        // 显式取消旧的定时器任务，避免新旧任务短暂并存
        if let Some(tx) = session.timer_cancel_tx.take() {
            let _ = tx.send(());
        }
        let (timer_cancel_tx, mut timer_cancel_rx) = oneshot::channel();
        session.timer_cancel_tx = Some(timer_cancel_tx);

        // 使用轮询方式：单个 tokio 任务定期检查所有定时器
        // 这解决了动态创建的定时器（如 wait.time 创建的）无法触发的问题
        let timer_tx = self.timer_tx.clone();
        tokio::spawn(async move {
            // 轮询间隔 50ms，确保定时器精度
            let poll_interval = tokio::time::Duration::from_millis(50);
            loop {
                tokio::select! {
                    _ = &mut timer_cancel_rx => { break; }
                    _ = tokio::time::sleep(poll_interval) => {
                        if timer_tx
                            .send(TimerRequest { session_id })
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        });
    }

    /// 发送 Lua 引擎产生的命令，拦截 / 开头的命令作为 Lua 代码执行
    pub(crate) fn send_lua_commands(
        &mut self,
        session_id: SessionId,
        commands: Vec<String>,
    ) -> io::Result<()> {
        let name = match self.manager.get_by_id(session_id) {
            Some(s) => s.name.clone(),
            None => return Ok(()),
        };
        // 使用队列处理命令，别名匹配可能产生新命令需要继续处理
        // 先按 ; 拆分，保证带多个分号分隔的命令被逐条处理
        let mut queue: std::collections::VecDeque<String> = commands
            .into_iter()
            .flat_map(|cmd| {
                if cmd.contains(';') {
                    split_commands(&cmd)
                } else {
                    vec![cmd]
                }
            })
            .collect();
        // 防止无限递归：限制别名匹配的嵌套深度
        let mut depth = 0;
        let max_depth = 10;

        while let Some(cmd) = queue.pop_front() {
            if let Some(lua_code) = cmd.strip_prefix('/') {
                // / 开头的命令作为 Lua 代码执行
                // 去掉前导 /
                self.logger.log_lua(&name, lua_code);
                if let Some(engine) = self
                    .manager
                    .get_by_id(session_id)
                    .and_then(|s| s.lua_engine.as_ref())
                {
                    match engine.eval_code(lua_code) {
                        Ok(_) => {
                            let sub_commands = engine.drain_commands();
                            for sub_cmd in sub_commands {
                                self.logger.log_command(&name, &sub_cmd);
                                if let Err(e) = self.manager.send_to(session_id, &sub_cmd) {
                                    self.terminal.append_output(&format!("[Lua 错误] {}", e))?;
                                }
                            }
                        }
                        Err(e) => {
                            self.terminal.append_output(&format!("[Lua 错误] {}", e))?;
                        }
                    }
                }
            } else if depth < max_depth {
                // 非 / 开头的命令：先尝试别名匹配（与 MUSHclient Execute 行为一致）
                let alias_handled = if let Some(engine) = self
                    .manager
                    .get_by_id(session_id)
                    .and_then(|s| s.lua_engine.as_ref())
                {
                    let handled = engine.process_input(&cmd);
                    if handled {
                        let sub_commands = engine.drain_commands();
                        if !sub_commands.is_empty() {
                            // 别名匹配成功，产生的命令加入队列继续处理
                            for sub_cmd in sub_commands {
                                queue.push_front(sub_cmd);
                            }
                        }
                    }
                    handled
                } else {
                    false
                };

                if !alias_handled {
                    // 无别名匹配，直接发送到 MUD
                    self.logger.log_command(&name, &cmd);
                    if self.is_session_connected(session_id) {
                        self.send_cmd_checked(session_id, &cmd)?;
                    }
                }
                depth += 1;
            } else {
                // 超过嵌套深度，直接发送防止无限递归
                self.logger.log_command(&name, &cmd);
                if self.is_session_connected(session_id) {
                    self.send_cmd_checked(session_id, &cmd)?;
                }
            }
        }
        Ok(())
    }

    /// 检查指定 session 是否处于已连接状态（用于断线保护守卫）
    pub(crate) fn is_session_connected(&self, session_id: SessionId) -> bool {
        self.manager
            .get_by_id(session_id)
            .map(|s| s.state == crate::connection::SessionState::Connected)
            .unwrap_or(false)
    }

    /// 发送命令到服务器，失败时按 session 去重输出错误
    /// TCP 半死时写任务阻塞，命令队列（256）填满后连续失败，
    /// 仅首次失败输出一条 [发送错误]，防止刷屏；发送恢复后重置
    pub(crate) fn send_cmd_checked(&mut self, session_id: SessionId, cmd: &str) -> io::Result<()> {
        let result = self.manager.send_to(session_id, cmd);
        if update_send_err_state(&mut self.cmd_send_err_shown, session_id, result.is_ok()) {
            if let Err(e) = result {
                self.terminal.append_output(&format!("[发送错误] {}", e))?;
            }
        }
        Ok(())
    }

    /// 发送 Lua 引擎产生的原始数据包（SendPkt 压入的）
    pub(crate) fn send_lua_raw(&mut self, session_id: SessionId) -> io::Result<()> {
        // 断线 session 跳过原始包发送，静默丢弃
        if !self.is_session_connected(session_id) {
            return Ok(());
        }
        let raw_packets = self
            .manager
            .get_by_id(session_id)
            .and_then(|s| s.lua_engine.as_ref())
            .map(|engine| engine.drain_raw())
            .unwrap_or_default();
        for data in raw_packets {
            let result = self.manager.send_raw(session_id, data);
            if update_send_err_state(&mut self.raw_send_err_shown, session_id, result.is_ok()) {
                if let Err(e) = result {
                    self.terminal
                        .append_output(&format!("[发送原始数据错误] {}", e))?;
                }
            }
        }
        Ok(())
    }

    /// 处理定时器触发（轮询模式：检查所有到期定时器）
    pub(crate) fn handle_timer(&mut self, session_id: SessionId) -> io::Result<()> {
        if self.manager.get_by_id(session_id).is_none() {
            return Ok(());
        }
        // 断线 session 跳过所有 Lua 定时器处理，避免通过已关闭通道发送数据产生错误刷屏
        if !self.is_session_connected(session_id) {
            return Ok(());
        }
        let mut any_fired = false;

        // 检查延迟 OnConnect 是否到期
        {
            let on_connect_fired = self
                .manager
                .get_mut_by_id(session_id)
                .and_then(|s| s.lua_engine.as_mut())
                .map(|engine| engine.check_pending_on_connect())
                .unwrap_or(false);
            if on_connect_fired {
                any_fired = true;
            }
        }
        if any_fired {
            let commands = self
                .manager
                .get_by_id(session_id)
                .and_then(|s| s.lua_engine.as_ref())
                .map(|engine| {
                    let mut cmds = engine.drain_commands();
                    cmds.extend(engine.drain_delayed_commands());
                    cmds
                })
                .unwrap_or_default();
            if !commands.is_empty() {
                self.send_lua_commands(session_id, commands)?;
            }
            // 处理 SendPkt 压入的原始数据包
            self.send_lua_raw(session_id)?;
            self.drain_lua_logs(session_id)?;
        }

        // 批量触发所有到期定时器（MushClient 兼容：一轮扫描收集，逐个触发）
        let due_names = self
            .manager
            .get_by_id(session_id)
            .and_then(|s| s.lua_engine.as_ref())
            .map(|engine| engine.fire_due_timers())
            .unwrap_or_default();
        if !due_names.is_empty() {
            any_fired = true;
            let commands = self
                .manager
                .get_by_id(session_id)
                .and_then(|s| s.lua_engine.as_ref())
                .map(|engine| engine.drain_commands())
                .unwrap_or_default();
            if !commands.is_empty() {
                self.send_lua_commands(session_id, commands)?;
            }
            self.send_lua_raw(session_id)?;
            self.drain_lua_logs(session_id)?;
        }
        // 空闲心跳检测：服务器静默超过 30 秒时发送 IAC NOP
        if let Some(engine) = self
            .manager
            .get_by_id(session_id)
            .and_then(|s| s.lua_engine.as_ref())
        {
            engine.fire_keepalive_if_idle();
        }
        self.send_lua_raw(session_id)?;

        // 应用层心跳检测：空闲超时发送心跳，超时未响应则断连
        if let Some(session) = self.manager.get_by_id(session_id) {
            if session.state == SessionState::Connected && !session.heartbeat_cmd.is_empty() {
                let idle_secs = session.last_recv_time.elapsed().as_secs();
                let idle_timeout = session.idle_timeout_secs;
                let hb_timeout = session.heartbeat_timeout_secs;
                let hb_sent = session.heartbeat_sent;
                let hb_cmd = session.heartbeat_cmd.clone();

                if let Some(sent_at) = hb_sent {
                    // 已发送心跳，检查响应超时
                    if sent_at.elapsed().as_secs() >= hb_timeout {
                        if let Some(session) = self.manager.get_mut_by_id(session_id) {
                            session.set_disconnect_reason("heartbeat_timeout".to_string());
                            // 不用 disconnect()：它依赖「写半关闭 → FIN → EOF」事件链
                            // 驱动重连，网络静默死亡（无 FIN/RST，且 TCP keepalive
                            // 未覆盖的平台）时 EOF 永不到来，重连永远不会触发。
                            // 改用 shutdown() 直接取消读任务，断线后处理
                            // （[DCN] 日志 / 重连排期）由下方 after_disconnect 统一执行。
                            // 定时器轮询任务与连接解耦（MUSHclient ActiveWhenClosed
                            // 语义，断线期间 wait.time 等仍需继续走），跨 shutdown 保留
                            let keep_timer = session.timer_cancel_tx.take();
                            session.shutdown();
                            session.timer_cancel_tx = keep_timer;
                            if let Some(ref mut engine) = session.lua_engine {
                                engine.notify_disconnect("heartbeat_timeout");
                            }
                        }
                        let name = self
                            .manager
                            .get_by_id(session_id)
                            .map(|s| s.name.clone())
                            .unwrap_or_default();
                        let display_pos = self.manager.display_number_of(session_id);
                        self.terminal.append_output(&format!(
                            "[系统] 连接 {} ({}) 心跳超时 ({}s)，主动断开",
                            display_pos, name, hb_timeout
                        ))?;
                        // 统一断线后处理：记录日志 + 按退避排期自动重连
                        self.after_disconnect(session_id)?;
                        // shutdown() 静默取消读任务，无 Disconnected 事件触发
                        // 状态栏刷新，此处前台会话需显式刷新一次
                        if session_id == self.manager.foreground_id {
                            self.update_status_bar()?;
                        }
                    }
                } else if idle_secs >= idle_timeout {
                    // 空闲超时，发送心跳
                    if let Some(session) = self.manager.get_mut_by_id(session_id) {
                        session.heartbeat_sent = Some(std::time::Instant::now());
                        let _ = session.send(&hb_cmd);
                    }
                }
            }
        }

        // OnPrompt idle-settled 检测：连接态下静默超过阈值且本轮未触发过，则回调一次。
        // 寄生在已有 last_recv_time + 每会话 tick 上，不新增跨重连状态（闩在收到新
        // Data / 迁入 Connected 时复位）。阈值 0 = 禁用，存量配置零影响。
        // 关键：OnPrompt 的典型用途是唤醒协程后立即 Send 下一条命令，故必须与
        // OnConnect/OnDisconnect 一样在回调后排空命令/原始包/日志——否则命令残留到
        // 下一 tick 触发 fire_due_timers 的 debug_assert（dev panic），release 下若本
        // 会话再无到期 timer / 服务器静默，则该命令永不发出，脚本与服务器互等死锁。
        let prompt_fired = {
            match self.manager.get_mut_by_id(session_id) {
                // 仅在确有引擎接收时才消耗闩并派发；无引擎（未配脚本/初始化失败）则
                // 落入 `_ => false`，不置位，留待下一 tick 重试，避免信号「被消耗却无人接收」
                Some(session)
                    if session.prompt_should_fire(std::time::Instant::now())
                        && session.lua_engine.is_some() =>
                {
                    session.prompt_settled = true;
                    if let Some(ref mut engine) = session.lua_engine {
                        engine.notify_prompt("idle");
                    }
                    true
                }
                _ => false,
            }
        };
        if prompt_fired {
            let commands = self
                .manager
                .get_by_id(session_id)
                .and_then(|s| s.lua_engine.as_ref())
                .map(|engine| engine.drain_commands())
                .unwrap_or_default();
            if !commands.is_empty() {
                self.send_lua_commands(session_id, commands)?;
            }
            self.send_lua_raw(session_id)?;
            self.drain_lua_logs(session_id)?;
            any_fired = true;
        }

        // 仅在定时器真正触发时才刷新状态栏（避免每 50ms 写终端，破坏鼠标选中）
        if any_fired && session_id == self.manager.foreground_id {
            self.update_status_bar()?;
        }
        Ok(())
    }

    /// 停止指定 session 的渲染刷新定时器
    pub(crate) fn stop_render_tick_timer(&mut self, session_id: SessionId) {
        if let Some(cancel_tx) = self.render_tick_cancels.remove(&session_id) {
            let _ = cancel_tx.send(());
        }
    }

    /// 启动渲染刷新定时器：按指定间隔定期发送刷新请求
    pub(crate) fn start_render_tick_timer(&mut self, session_id: SessionId, interval_ms: u64) {
        // 先停止旧的定时器
        self.stop_render_tick_timer(session_id);

        let tx = self.render_tick_tx.clone();
        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        self.render_tick_cancels.insert(session_id, cancel_tx);

        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(tokio::time::Duration::from_millis(interval_ms));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if tx.send(RenderTickRequest { session_id }).await.is_err() {
                            break;
                        }
                    }
                    _ = &mut cancel_rx => {
                        break;
                    }
                }
            }
        });
    }

    /// 处理渲染刷新请求：将缓冲的待渲染数据一次性输出到终端
    pub(crate) fn handle_render_tick(&mut self, session_id: SessionId) -> io::Result<()> {
        if self.manager.get_by_id(session_id).is_none() {
            return Ok(());
        }
        // 仅当前台 session 且有待渲染数据时才刷新
        if session_id != self.manager.foreground_id {
            if let Some(session) = self.manager.get_mut_by_id(session_id) {
                session.render_dirty = false;
            }
            return Ok(());
        }
        if !self
            .manager
            .get_by_id(session_id)
            .map(|s| s.render_dirty)
            .unwrap_or(false)
        {
            return Ok(());
        }
        let pending = self
            .manager
            .get_mut_by_id(session_id)
            .map(|s| std::mem::take(&mut s.pending_data))
            .unwrap_or_default();
        if let Some(session) = self.manager.get_mut_by_id(session_id) {
            session.render_dirty = false;
        }
        if pending.is_empty() {
            return Ok(());
        }
        // 合并所有待渲染行，一次性输出
        let mut combined = String::new();
        for line in &pending {
            combined.push_str(line);
            combined.push('\n');
        }
        if !combined.is_empty() {
            self.terminal.append_output(&combined)?;
        }
        Ok(())
    }

    /// 处理 Lua 引擎产生的日志
    pub(crate) fn drain_lua_logs(&mut self, session_id: SessionId) -> io::Result<()> {
        let logs = self
            .manager
            .get_by_id(session_id)
            .and_then(|s| s.lua_engine.as_ref())
            .map(|engine| engine.drain_logs())
            .unwrap_or_default();
        let name = self
            .manager
            .get_by_id(session_id)
            .map(|s| s.name.clone())
            .unwrap_or_default();
        let is_foreground = session_id == self.manager.foreground_id;
        // 节流模式下将 Lua 日志缓冲到 pending_data，与 MUD 数据一起刷新
        let buffer = is_foreground
            && !self
                .manager
                .get_by_id(session_id)
                .map(|s| s.realtime)
                .unwrap_or(false);
        for msg in logs {
            // 日志写入文件（剥离 ANSI 码），根据前缀分类
            let clean = crate::ui::AnsiParser::strip_ansi(&msg);
            if clean.starts_with("[GPS-MATCH]")
                || clean.starts_with("[GPS]")
                || clean.starts_with("[DEBUG")
            {
                self.logger.log_debug(&name, &clean);
            } else {
                self.logger.log(&name, &clean);
            }
            // 如果是前台连接，也在终端显示（保留 ANSI 码以显示颜色）
            if is_foreground {
                if buffer {
                    if let Some(s) = self.manager.get_mut_by_id(session_id) {
                        s.pending_data.push(format!("\x1b[36m[Lua] {}\x1b[0m", msg));
                        s.render_dirty = true;
                    }
                } else {
                    // 保存到 session 的 output_lines，切换 session 时不会丢失
                    if let Some(s) = self.manager.get_mut_by_id(session_id) {
                        s.output_lines.push(format!("\x1b[36m[Lua] {}\x1b[0m", msg));
                    }
                    self.terminal
                        .append_output(&format!("\x1b[36m[Lua] {}\x1b[0m", msg))?;
                }
            }
        }
        // 面板更新需在日志输出前处理，确保 append_output 触发渲染时面板已是最新状态
        // 实时模式：先 drain 面板再 append，refesh_output_area 使用正确面板数据
        // 缓冲模式：先 drain 面板再加 pending，handle_render_tick 刷新时使用正确面板数据
        self.drain_lua_panels(session_id);
        Ok(())
    }

    /// 排空 Lua 面板更新并应用到终端
    ///
    /// 所有 session 的 pending_panels 都会在此排空，避免后台 session 的面板更新无限累积。
    /// 前台 session 的更新直接应用到终端；后台 session 的更新增量写入 session.panels，
    /// 使切换 session 时 restore_panels 能恢复最新状态。
    pub(crate) fn drain_lua_panels(&mut self, session_id: SessionId) {
        let updates = self
            .manager
            .get_by_id(session_id)
            .and_then(|s| s.lua_engine.as_ref())
            .map(|engine| engine.drain_panels())
            .unwrap_or_default();
        if session_id != self.manager.foreground_id {
            // 后台 session：将面板更新增量写入 session.panels，切换时即可恢复最新状态
            if !updates.is_empty() {
                if let Some(session) = self.manager.get_mut_by_id(session_id) {
                    for update in updates {
                        match update {
                            PanelUpdate::Set {
                                name,
                                x,
                                y,
                                width,
                                height,
                                lines,
                                buttons,
                            } => {
                                if let Some(existing) =
                                    session.panels.iter_mut().find(|p| p.name == name)
                                {
                                    existing.x = x;
                                    existing.y = y;
                                    existing.width = width;
                                    existing.height = height;
                                    existing.lines = lines;
                                    existing.buttons = buttons;
                                } else {
                                    session.panels.push(Panel {
                                        name,
                                        x,
                                        y,
                                        width,
                                        height,
                                        lines,
                                        buttons,
                                    });
                                }
                            }
                            PanelUpdate::Remove { name } => {
                                session.panels.retain(|p| p.name != name);
                            }
                        }
                    }
                }
            }
            return;
        }
        for update in updates {
            match update {
                PanelUpdate::Set {
                    name,
                    x,
                    y,
                    width,
                    height,
                    lines,
                    buttons,
                } => {
                    self.terminal
                        .set_panel(&name, x, y, width, height, lines, buttons);
                }
                PanelUpdate::Remove { name } => {
                    self.terminal.remove_panel(&name);
                }
            }
        }
    }
}

/// 发送错误去重状态更新（命令通道与原始数据通道共用）
///
/// 成功时清除该 session 的已展示标记（后续失败会再次报告）；
/// 失败时仅首次（标记不存在）返回 true，表示应输出错误。
/// TCP 半死时发送队列填满导致连续失败，借此防止以轮询频率刷屏。
fn update_send_err_state(
    err_shown: &mut HashSet<SessionId>,
    session_id: SessionId,
    is_ok: bool,
) -> bool {
    if is_ok {
        err_shown.remove(&session_id);
        false
    } else {
        err_shown.insert(session_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 首次失败返回 true（应输出），连续失败返回 false（静默）
    #[test]
    fn test_send_err_state_first_failure_only() {
        let mut shown = HashSet::new();
        let sid = SessionId(1);
        // 首次失败：输出
        assert!(update_send_err_state(&mut shown, sid, false));
        // 连续失败：静默（TCP 半死刷屏场景）
        assert!(!update_send_err_state(&mut shown, sid, false));
        assert!(!update_send_err_state(&mut shown, sid, false));
    }

    /// 成功发送后清除标记，后续失败会再次输出（每轮故障报一次）
    #[test]
    fn test_send_err_state_reset_on_success() {
        let mut shown = HashSet::new();
        let sid = SessionId(1);
        assert!(update_send_err_state(&mut shown, sid, false));
        // 恢复成功：不输出且清除标记
        assert!(!update_send_err_state(&mut shown, sid, true));
        // 再次失败：重新输出
        assert!(update_send_err_state(&mut shown, sid, false));
    }

    /// 持续成功时始终不输出
    #[test]
    fn test_send_err_state_stays_silent_on_success() {
        let mut shown = HashSet::new();
        let sid = SessionId(1);
        assert!(!update_send_err_state(&mut shown, sid, true));
        assert!(!update_send_err_state(&mut shown, sid, true));
    }

    /// 多 session 隔离：一个 session 的失败不影响另一个 session 的首次输出
    #[test]
    fn test_send_err_state_session_isolation() {
        let mut shown = HashSet::new();
        let sid_a = SessionId(1);
        let sid_b = SessionId(2);
        assert!(update_send_err_state(&mut shown, sid_a, false));
        // sid_a 已展示，sid_b 仍是首次失败：应输出
        assert!(!update_send_err_state(&mut shown, sid_a, false));
        assert!(update_send_err_state(&mut shown, sid_b, false));
    }
}
