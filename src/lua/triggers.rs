//! 触发器匹配与执行
//!
//! 处理服务器输出，匹配触发器并调用回调。包含：
//! - `process_output`: 处理一行服务器输出（含 panic 防御）
//! - `process_output_inner`: 实际匹配逻辑（GBK/UTF-8、单行/多行）
//! - `parse_style_runs`: 解析 ANSI 样式片段（供 GetStyle API 使用）

use std::panic::AssertUnwindSafe;

use super::types::{LuaEngine, StyleRun, TriggerPattern};

impl LuaEngine {
    /// 处理服务器输出，匹配触发器
    /// 返回值：是否有匹配的触发器设置了 omit_from_output（调用方据此抑制显示）
    pub fn process_output(&self, line: &str) -> bool {
        // 使用 catch_unwind 防止函数体内任何 panic 跨越 FFI 边界导致静默崩溃
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| self.process_output_inner(line)));
        match result {
            Ok(omit) => omit,
            Err(_) => {
                self.log_error("process_output 中发生 panic，已捕获以防止崩溃");
                // panic 时保守返回 false，避免静默吞掉用户输出
                false
            }
        }
    }

    /// process_output 的内部实现
    fn process_output_inner(&self, line: &str) -> bool {
        // 一次性 borrow_mut 完成多项状态更新
        {
            let mut state = self.state.borrow_mut();
            state.pending_commands.clear();
            state.last_server_data = std::time::Instant::now();
            state.packet_count += 1;
        }

        // 剥离 ANSI 码用于匹配，并去除行末 \r
        let clean_line = crate::ui::AnsiParser::strip_ansi(line);
        let clean_line = clean_line.trim_end_matches('\r').to_string();

        // 解析样式运行片段（用于 GetStyle API）
        let style_runs = Self::parse_style_runs(line);

        // 维护最近行缓冲区
        {
            let mut state = self.state.borrow_mut();
            state.recent_lines.push(clean_line.clone());
            if state.recent_lines.len() > 20 {
                state.recent_lines.remove(0);
            }
        }

        // 将 clean_line 转为 GBK 字节用于 GBK 模式匹配
        let gbk_line = encoding_rs::GBK.encode(&clean_line).0.into_owned();

        // 收集需要触发的
        let matches: Vec<(usize, String, Vec<String>, Vec<StyleRun>)> = {
            let state = self.state.borrow();
            let mut result = Vec::new();
            for (i, trigger) in state.triggers.iter().enumerate() {
                if !trigger.enabled {
                    continue;
                }

                match &trigger.pattern {
                    TriggerPattern::Gbk(gbk_re) => {
                        if trigger.multiline && trigger.lines_to_match > 1 {
                            let n = trigger.lines_to_match;
                            if state.recent_lines.len() >= n {
                                let combined: String = state
                                    .recent_lines
                                    .iter()
                                    .rev()
                                    .take(n)
                                    .rev()
                                    .cloned()
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                let gbk_combined =
                                    encoding_rs::GBK.encode(&combined).0.into_owned();
                                if let Some(caps) = gbk_re.captures(&gbk_combined) {
                                    let full_match = {
                                        let m = caps.get(0).unwrap();
                                        let (cow, _, _) = encoding_rs::GBK.decode(m.as_bytes());
                                        cow.into_owned()
                                    };
                                    let caps_list: Vec<String> = caps
                                        .iter()
                                        .skip(1)
                                        .flatten()
                                        .map(|m| {
                                            let (cow, _, _) = encoding_rs::GBK.decode(m.as_bytes());
                                            cow.into_owned()
                                        })
                                        .collect();
                                    result.push((i, full_match, caps_list, style_runs.clone()));
                                }
                            }
                        } else {
                            if let Some(caps) = gbk_re.captures(&gbk_line) {
                                let full_match = {
                                    let m = caps.get(0).unwrap();
                                    let (cow, _, _) = encoding_rs::GBK.decode(m.as_bytes());
                                    cow.into_owned()
                                };
                                let caps_list: Vec<String> = caps
                                    .iter()
                                    .skip(1)
                                    .flatten()
                                    .map(|m| {
                                        let (cow, _, _) = encoding_rs::GBK.decode(m.as_bytes());
                                        cow.into_owned()
                                    })
                                    .collect();
                                result.push((i, full_match, caps_list, style_runs.clone()));
                            }
                        }
                    }
                    TriggerPattern::Utf8(utf8_re) => {
                        if trigger.multiline && trigger.lines_to_match > 1 {
                            let n = trigger.lines_to_match;
                            if state.recent_lines.len() >= n {
                                let combined: String = state
                                    .recent_lines
                                    .iter()
                                    .rev()
                                    .take(n)
                                    .rev()
                                    .cloned()
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                if let Some(caps) = utf8_re.captures(&combined) {
                                    let full_match = caps.get(0).unwrap().as_str().to_string();
                                    let caps_list: Vec<String> = caps
                                        .iter()
                                        .skip(1)
                                        .flatten()
                                        .map(|m| m.as_str().to_string())
                                        .collect();
                                    result.push((i, full_match, caps_list, style_runs.clone()));
                                }
                            }
                        } else {
                            if let Some(caps) = utf8_re.captures(&clean_line) {
                                let full_match = caps.get(0).unwrap().as_str().to_string();
                                let caps_list: Vec<String> = caps
                                    .iter()
                                    .skip(1)
                                    .flatten()
                                    .map(|m| m.as_str().to_string())
                                    .collect();
                                result.push((i, full_match, caps_list, style_runs.clone()));
                            }
                        }
                    }
                }
            }
            result
        };

        // 构建 styles Lua 表（所有回调共享同一行数据）
        let styles_table: mlua::Value = if style_runs.is_empty() {
            // 没有样式信息，传 nil
            mlua::Value::Nil
        } else if let Ok(t) = self.lua.create_table() {
            for (i, sr) in style_runs.iter().enumerate() {
                if let Ok(entry) = self.lua.create_table() {
                    let _ = entry.set("start", sr.start);
                    let _ = entry.set("length", sr.length);
                    let _ = entry.set("textcolour", sr.textcolour);
                    let _ = entry.set("backcolour", sr.backcolour);
                    let _ = entry.set("bold", sr.bold);
                    let _ = entry.set("italic", sr.italic);
                    let _ = entry.set("underline", sr.underline);
                    // text 字段：MUSHclient 兼容，表示该样式区间的文本内容
                    // 使用 get() 防止 sr.start/end 不在 UTF-8 字符边界上时 panic
                    if sr.start < clean_line.len() {
                        let end = std::cmp::min(sr.start + sr.length, clean_line.len());
                        let text = clean_line.get(sr.start..end).unwrap_or("");
                        let _ = entry.set("text", text);
                    }
                    let _ = t.set(i + 1, entry);
                }
            }
            mlua::Value::Table(t)
        } else {
            mlua::Value::Nil
        };

        // 计算是否有匹配的触发器设置了 omit_from_output
        // 任一匹配触发器 omit 即抑制该行显示（符合 MUSHclient 语义）
        // 注意：必须在消费 matches 的 for 循环之前计算
        let any_omit = {
            let state = self.state.borrow();
            matches
                .iter()
                .any(|(idx, _, _, _)| state.triggers[*idx].omit_from_output)
        };

        // OneShot trigger 匹配后自动删除（MushClient 兼容：trigger_flag.OneShot = 32768）
        // 必须在消费 matches 的 for 循环之前收集
        let oneshot_names: Vec<String> = {
            let state = self.state.borrow();
            matches
                .iter()
                .filter_map(|(idx, _, _, _)| {
                    let t = &state.triggers[*idx];
                    if t.one_shot {
                        Some(t.name.clone())
                    } else {
                        None
                    }
                })
                .collect()
        };

        // 逐个触发
        for (idx, full_match, caps_list, _sr) in matches {
            let (callback, send_text, trigger_name) = {
                let state = self.state.borrow();
                (
                    state.triggers[idx].callback.clone(),
                    state.triggers[idx].send_text.clone(),
                    state.triggers[idx].name.clone(),
                )
            };
            // MUSHclient 触发器回调签名: function(name, line, wildcards, styles)
            if let Ok(wildcards_table) = self.lua.create_table() {
                // w[0] = 完整匹配文本（MUSHclient 兼容）
                let _ = wildcards_table.set(0, full_match.as_str());
                for (i, m) in caps_list.iter().enumerate() {
                    let _ = wildcards_table.set(i + 1, m.as_str());
                }
                // 用 catch_unwind 防止 Rust panic 跨越 Lua FFI 边界导致静默崩溃
                if std::panic::catch_unwind(AssertUnwindSafe(|| {
                    if let Err(e) = callback.call::<()>((
                        trigger_name.as_str(),
                        clean_line.as_str(),
                        wildcards_table,
                        styles_table.clone(),
                    )) {
                        self.log_error(&format!(
                            "[Lua] 触发器 '{}' 回调中发生 Lua 错误: {}",
                            trigger_name, e
                        ));
                    }
                }))
                .is_err()
                {
                    self.log_error(&format!(
                        "[Lua] 触发器 '{}' 回调中发生 panic，已捕获以防止崩溃",
                        trigger_name
                    ));
                }
            }
            if !send_text.is_empty() {
                self.state.borrow_mut().pending_commands.push(send_text);
            }
        }

        // OneShot trigger 匹配后自动删除
        if !oneshot_names.is_empty() {
            let mut state = self.state.borrow_mut();
            for name in &oneshot_names {
                state.delete_trigger(name);
            }
        }

        any_omit
    }

    /// 解析 ANSI 样式运行片段
    #[allow(unused_assignments)]
    pub(super) fn parse_style_runs(raw_line: &str) -> Vec<StyleRun> {
        let mut runs: Vec<StyleRun> = Vec::new();
        let mut chars = raw_line.chars().peekable();
        let mut clean_pos: usize = 0;

        // 当前样式状态
        let mut fg: u32 = 7;
        let mut bg: u32 = 0;
        let mut bold = false;
        let mut italic = false;
        let mut underline = false;

        // 当前正在构建的运行
        let mut run_start: usize = 0;
        let mut run_len: usize = 0;

        // 内部宏：刷新当前运行
        macro_rules! flush_run {
            () => {
                if run_len > 0 {
                    runs.push(StyleRun {
                        start: run_start,
                        length: run_len,
                        textcolour: fg,
                        backcolour: bg,
                        bold,
                        italic,
                        underline,
                    });
                    run_len = 0;
                }
            };
        }

        while let Some(&ch) = chars.peek() {
            if ch == '\x1b' {
                // ANSI 转义序列
                chars.next(); // 消耗 \x1b
                if chars.peek() == Some(&'[') {
                    chars.next(); // 消耗 '['
                                  // CSI 序列
                    let mut params = String::new();
                    while let Some(&c) = chars.peek() {
                        if ('\x30'..='\x3f').contains(&c) || ('\x20'..='\x2f').contains(&c) {
                            params.push(c);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    if chars.peek() == Some(&'m') || chars.peek() == Some(&'M') {
                        chars.next(); // 消耗 final byte

                        // 1. 先根据 SGR 参数计算新的样式值
                        let (new_fg, new_bg, new_bold, new_italic, new_underline) = {
                            if params.is_empty() || params == "0" {
                                (7u32, 0u32, false, false, false)
                            } else {
                                let mut nfg = fg;
                                let mut nbg = bg;
                                let mut nb = bold;
                                let mut ni = italic;
                                let mut nu = underline;
                                for param_str in params.split(';') {
                                    if param_str.is_empty() {
                                        continue;
                                    }
                                    let param: u16 = match param_str.parse() {
                                        Ok(v) => v,
                                        Err(_) => continue,
                                    };
                                    match param {
                                        0 => {
                                            nfg = 7;
                                            nbg = 0;
                                            nb = false;
                                            ni = false;
                                            nu = false;
                                        }
                                        1 => nb = true,
                                        3 => ni = true,
                                        4 => nu = true,
                                        22 => nb = false,
                                        23 => ni = false,
                                        24 => nu = false,
                                        30..=37 => nfg = (param - 30) as u32,
                                        38 => {} // 扩展前景色，跳过
                                        39 => nfg = 7,
                                        40..=47 => nbg = (param - 40) as u32,
                                        48 => {} // 扩展背景色，跳过
                                        49 => nbg = 0,
                                        90..=97 => nfg = (param - 82) as u32,
                                        100..=107 => nbg = (param - 92) as u32,
                                        _ => {}
                                    }
                                }
                                (nfg, nbg, nb, ni, nu)
                            }
                        };

                        // 2. 如果样式发生变化，用旧值刷新当前运行
                        if fg != new_fg
                            || bg != new_bg
                            || bold != new_bold
                            || italic != new_italic
                            || underline != new_underline
                        {
                            flush_run!();
                            run_start = clean_pos;
                        }

                        // 3. 应用新样式值
                        fg = new_fg;
                        bg = new_bg;
                        bold = new_bold;
                        italic = new_italic;
                        underline = new_underline;
                    }
                }
                continue;
            }

            // 可见字符
            let ch_len = ch.len_utf8();
            run_len += ch_len;
            clean_pos += ch_len;
            chars.next();
        }

        // 刷新最后一个运行
        flush_run!();

        runs
    }
}
