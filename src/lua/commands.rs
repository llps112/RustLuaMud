//! 命令队列与状态访问器
//!
//! 提供对 Lua 引擎内部状态的访问接口，包含：
//! - 命令队列管理（drain_commands / drain_raw / drain_logs / drain_panels）
//! - 状态设置器（set_variable / set_host / set_world_name / set_char_name 等）
//! - 连接状态管理（set_connected / restore_connection_state / check_pending_on_connect）
//! - 面板点击处理（handle_panel_click）
//! - 统计信息（trigger_count / alias_count / timer_count）

use std::panic::AssertUnwindSafe;

use super::types::{ConnectionState, LuaEngine, PanelUpdate};

impl LuaEngine {
    /// 取出待发送的命令
    pub fn drain_commands(&self) -> Vec<String> {
        self.state.borrow_mut().pending_commands.drain(..).collect()
    }

    /// 取出待发送的原始数据包（SendPkt 压入的）
    pub fn drain_raw(&self) -> Vec<Vec<u8>> {
        self.state.borrow_mut().pending_raw.drain(..).collect()
    }

    /// 设置 Lua 变量（内部 HashMap，通过 GetVariable 访问）
    pub fn set_variable(&mut self, key: &str, value: &str) {
        self.state
            .borrow_mut()
            .variables
            .insert(key.to_string(), value.to_string());
    }

    /// 设置连接主机地址（供 GetInfo(1) 返回）
    pub fn set_host(&self, host: &str) {
        self.state.borrow_mut().host = host.to_string();
    }

    /// 设置端口（本引擎扩展，非 MushClient 标准 GetInfo）
    #[allow(dead_code)]
    pub fn set_port(&self, port: u16) {
        self.state.borrow_mut().port = port;
    }

    /// 设置世界名称（供 GetInfo(2) 返回）
    pub fn set_world_name(&self, name: &str) {
        self.state.borrow_mut().world_name = name.to_string();
    }

    /// 设置角色名（供 GetInfo(3) 返回）
    pub fn set_char_name(&self, name: &str) {
        self.state.borrow_mut().char_name = name.to_string();
    }

    /// 设置 Lua 全局变量（脚本中可直接按名引用）
    pub fn set_global(&self, name: &str, value: &str) {
        let globals = self.lua.globals();
        let _ = globals.set(name, value);
    }

    /// 获取连接状态（用于 reload 时恢复）
    pub fn get_connection_state(&self) -> ConnectionState {
        let state = self.state.borrow();
        ConnectionState {
            connected: state.connected,
            host: state.host.clone(),
            port: state.port,
            world_name: state.world_name.clone(),
            char_name: state.char_name.clone(),
            status_text: state.status_text.clone(),
        }
    }

    /// 恢复连接状态（用于 reload 后）
    pub fn restore_connection_state(&mut self, conn_state: &ConnectionState) {
        let mut state = self.state.borrow_mut();
        state.connected = conn_state.connected;
        state.host = conn_state.host.clone();
        state.port = conn_state.port;
        state.world_name = conn_state.world_name.clone();
        state.char_name = conn_state.char_name.clone();
        state.status_text = conn_state.status_text.clone();
    }

    /// 设置连接状态，连接成功时自动调用 OnConnect()（由 Lua 脚本覆盖实现）
    pub fn set_connected(&mut self, connected: bool) {
        let was_connected = self.state.borrow().connected;
        self.state.borrow_mut().connected = connected;
        // 连接刚建立时，调用 OnConnect() 抽象接口
        // Lua 脚本可通过覆盖 OnConnect() 实现连接后的初始化逻辑
        if connected && !was_connected {
            self.state.borrow_mut().connect_time = Some(std::time::Instant::now());
            self.state.borrow_mut().reconnecting = false;
            // 延迟触发机制：由定时器轮询检查并执行
            // 延迟时间从 Session 配置获取，通过 set_connect_delay 设置
            // 若未设置延迟（delay_ms == 0），则立即执行
            if self.connect_delay_ms == 0 {
                let lua_result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    if let Err(e) = self.eval_code("OnConnect()") {
                        self.log_error(&format!("OnConnect() 执行失败: {}", e));
                    }
                }));
                if lua_result.is_err() {
                    self.log_error("OnConnect() 执行中发生 panic，已捕获以防止崩溃");
                }
            } else {
                // 延迟路径：先立即执行 OnConnect() 以设置 accessing=1、重建触发器等 Lua 状态
                // 确保后续服务器数据到达时 trigger 能正确判断（如 accessing>0）
                // 命令（Execute 压入的 pending_commands）留到延迟到期后再排空发送
                let lua_result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    if let Err(e) = self.eval_code("OnConnect()") {
                        self.log_error(&format!("OnConnect() 执行失败: {}", e));
                    }
                }));
                if lua_result.is_err() {
                    self.log_error("OnConnect() 执行中发生 panic，已捕获以防止崩溃");
                }
                // 将 OnConnect 产生的命令移入延迟队列，避免被 process_output_inner 清空
                self.delayed_commands
                    .borrow_mut()
                    .extend(self.state.borrow_mut().pending_commands.drain(..));
                // 设置命令排空的延迟触发时间
                self.pending_on_connect = Some(
                    std::time::Instant::now()
                        + std::time::Duration::from_millis(self.connect_delay_ms),
                );
            }
        } else if !connected && was_connected {
            // 连接断开时，清除待触发标记及延迟命令队列
            self.pending_on_connect = None;
            self.delayed_commands.borrow_mut().clear();
        }
    }

    /// 设置 OnConnect 延迟触发时间（毫秒）
    pub fn set_connect_delay(&mut self, delay_ms: u64) {
        self.connect_delay_ms = delay_ms;
    }

    /// 通知 Lua 引擎连接已断开，并调用 OnDisconnect(reason) 回调
    pub fn notify_disconnect(&mut self, reason: &str) {
        {
            let mut state = self.state.borrow_mut();
            state.connected = false;
            state.connect_time = None;
            state.reconnecting = true;
            state.last_disconnect_reason = Some(reason.to_string());
            self.pending_on_connect = None;
            self.delayed_commands.borrow_mut().clear();
        }
        // 调用 OnDisconnect(reason)
        let code = format!(
            "if type(OnDisconnect) == 'function' then OnDisconnect('{}') end",
            reason.replace('\'', "\\'")
        );
        let lua_result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            if let Err(e) = self.eval_code(&code) {
                self.log_error(&format!("OnDisconnect() 执行失败: {}", e));
            }
        }));
        if lua_result.is_err() {
            self.log_error("OnDisconnect() 执行中发生 panic，已捕获以防止崩溃");
        }
    }

    /// 从 Session 层同步连接统计数据到 Lua 引擎
    pub fn update_session_stats(
        &mut self,
        reconnect_count: u64,
        reconnect_attempt: u32,
        reconnecting: bool,
        next_retry_secs: u64,
        bytes_recv: u64,
        bytes_sent: u64,
    ) {
        let mut state = self.state.borrow_mut();
        state.reconnect_count = reconnect_count;
        state.reconnect_attempt = reconnect_attempt;
        state.reconnecting = reconnecting;
        state.next_retry_secs = next_retry_secs;
        state.bytes_recv = bytes_recv;
        state.bytes_sent = bytes_sent;
    }

    /// 检查延迟排空是否到期，到期则返回 true
    /// OnConnect() 已在 set_connected(true) 中立即执行，此处只触发命令排空
    pub fn check_pending_on_connect(&mut self) -> bool {
        if let Some(target_time) = self.pending_on_connect {
            if std::time::Instant::now() >= target_time {
                self.pending_on_connect = None;
                return true;
            }
        }
        false
    }

    /// 是否有待处理的延迟 OnConnect 排空（延迟期尚未结束）
    pub fn has_pending_delayed_on_connect(&self) -> bool {
        self.pending_on_connect.is_some()
    }

    /// 将当前 pending_commands 移入延迟队列（延迟期内 trigger 命令暂存）
    pub fn drain_commands_to_delayed(&self) {
        let cmds: Vec<String> = self.state.borrow_mut().pending_commands.drain(..).collect();
        if !cmds.is_empty() {
            self.delayed_commands.borrow_mut().extend(cmds);
        }
    }

    /// 取出延迟队列中的所有命令
    pub fn drain_delayed_commands(&self) -> Vec<String> {
        self.delayed_commands.borrow_mut().drain(..).collect()
    }

    #[allow(dead_code)]
    /// 取出连接请求标志（一次性消费）
    pub fn take_connect_requested(&self) -> bool {
        let val = self.state.borrow_mut().connect_requested;
        if val {
            self.state.borrow_mut().connect_requested = false;
        }
        val
    }

    #[allow(dead_code)]
    /// 取出断开请求标志（一次性消费）
    pub fn take_disconnect_requested(&self) -> bool {
        let val = self.state.borrow_mut().disconnect_requested;
        if val {
            self.state.borrow_mut().disconnect_requested = false;
        }
        val
    }

    /// 取出待发送的日志消息
    pub fn drain_logs(&self) -> Vec<String> {
        let mut state = self.state.borrow_mut();
        // flush 残留的 tell_buffer（合并到 pending_logs 末尾）
        let buffered = std::mem::take(&mut state.tell_buffer);
        if !buffered.is_empty() {
            if let Some(last) = state.pending_logs.last_mut() {
                last.push_str(&buffered);
            } else {
                state.pending_logs.push(buffered);
            }
        }
        state.pending_logs.drain(..).collect()
    }

    /// 取出待处理的面板更新
    pub fn drain_panels(&self) -> Vec<PanelUpdate> {
        let mut state = self.state.borrow_mut();
        state.pending_panels.drain(..).collect()
    }

    /// 处理面板按钮点击事件，调用通过 RegisterPanelHandler 注册的回调
    ///
    /// 设计: 客户端不硬编码脚本侧函数名，而是从 `panel_handlers` 注册表查找。
    /// 脚本通过 `RegisterPanelHandler(panel_name, callback)` 主动注册回调，
    /// 与 AddTrigger/AddAlias/AddTimer 的注册模式一致。
    pub fn handle_panel_click(&self, panel_name: &str, action: &str) {
        // 从注册表查找回调（clone 出来避免跨 RefCell 借用调用 Lua）
        let callback_opt = {
            let state = self.state.borrow();
            state.panel_handlers.get(panel_name).cloned()
        };
        match callback_opt {
            Some(func) => {
                if let Err(e) = func.call::<()>((panel_name, action)) {
                    self.log_error(&format!(
                        "[Lua] 面板 '{}' 点击回调中发生错误: {}",
                        panel_name, e
                    ));
                }
            }
            None => {
                // 未注册回调时记录调试信息，便于排查（不再静默失败）
                self.log_error(&format!(
                    "[Lua] 面板 '{}' 未注册点击回调（请调用 RegisterPanelHandler 注册）",
                    panel_name
                ));
            }
        }
    }

    /// 获取 SetStatus 设置的状态栏文本
    pub fn status_text(&self) -> String {
        self.state.borrow().status_text.clone()
    }

    /// 获取定时器列表（interval_millis）
    #[allow(dead_code)]
    pub fn timer_intervals(&self) -> Vec<u64> {
        self.state
            .borrow()
            .timers
            .iter()
            .filter(|t| t.enabled)
            .map(|t| t.interval_millis)
            .collect()
    }

    #[allow(dead_code)]
    /// 获取触发器数量
    pub fn trigger_count(&self) -> usize {
        self.state.borrow().triggers.len()
    }

    #[allow(dead_code)]
    /// 获取别名数量
    pub fn alias_count(&self) -> usize {
        self.state.borrow().aliases.len()
    }

    #[allow(dead_code)]
    /// 获取定时器数量
    pub fn timer_count(&self) -> usize {
        self.state.borrow().timers.len()
    }
}
