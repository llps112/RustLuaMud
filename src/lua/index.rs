//! 触发器/别名/定时器的索引维护
//!
//! 为 Vec<Trigger>/<Alias>/<TimerDef> 提供 name 索引和 group 索引，
//! 优化按 name 查找（O(1)）和按 group 批量操作（O(k)）。
//!
//! 删除策略：swap_remove + 索引更新，保证索引一致性。

use super::types::{Alias, ScriptState, TimerDef, Trigger};

impl ScriptState {
    // ============= Trigger 索引 =============

    /// 添加 trigger 并更新索引
    pub fn add_trigger(&mut self, trigger: Trigger) {
        let idx = self.triggers.len();
        let name = trigger.name.clone();
        let group = trigger.group.clone();
        self.triggers.push(trigger);
        self.trigger_by_name.insert(name, idx);
        if !group.is_empty() {
            self.trigger_groups.entry(group).or_default().push(idx);
        }
    }

    /// 删除 trigger 并更新索引（swap_remove 策略）
    /// 返回 true 表示找到并删除
    pub fn delete_trigger(&mut self, name: &str) -> bool {
        let idx = match self.trigger_by_name.remove(name) {
            Some(i) => i,
            None => return false,
        };
        let group = self.triggers[idx].group.clone();
        if !group.is_empty() {
            if let Some(list) = self.trigger_groups.get_mut(&group) {
                list.retain(|&i| i != idx);
                if list.is_empty() {
                    self.trigger_groups.remove(&group);
                }
            }
        }
        self.swap_remove_trigger(idx);
        true
    }

    /// swap_remove trigger 并更新被移动元素的索引
    fn swap_remove_trigger(&mut self, idx: usize) {
        let last_idx = self.triggers.len() - 1;
        if idx != last_idx {
            // 先记录被移动元素（最后一个）的 name/group，再 swap_remove
            let moved_name = self.triggers[last_idx].name.clone();
            let moved_group = self.triggers[last_idx].group.clone();
            self.triggers.swap_remove(idx);
            // 更新被移动元素的 name 索引
            self.trigger_by_name.insert(moved_name, idx);
            // 更新被移动元素的 group 索引
            if !moved_group.is_empty() {
                if let Some(list) = self.trigger_groups.get_mut(&moved_group) {
                    for i in list.iter_mut() {
                        if *i == last_idx {
                            *i = idx;
                            break;
                        }
                    }
                }
            }
        } else {
            self.triggers.pop();
        }
    }

    /// 按 group 批量启用/禁用 trigger
    pub fn enable_trigger_group(&mut self, group: &str, enable: bool) {
        if let Some(indices) = self.trigger_groups.get(group) {
            for &i in indices {
                self.triggers[i].enabled = enable;
            }
        }
    }

    /// 更新 trigger 的 group 索引（SetTriggerOption 修改 group 时调用）
    pub fn update_trigger_group(&mut self, idx: usize, new_group: &str) {
        let old_group = self.triggers[idx].group.clone();
        if old_group == new_group {
            return;
        }
        // 从旧 group 移除
        if !old_group.is_empty() {
            if let Some(list) = self.trigger_groups.get_mut(&old_group) {
                list.retain(|&i| i != idx);
                if list.is_empty() {
                    self.trigger_groups.remove(&old_group);
                }
            }
        }
        // 加入新 group
        if !new_group.is_empty() {
            self.trigger_groups
                .entry(new_group.to_string())
                .or_default()
                .push(idx);
        }
        self.triggers[idx].group = new_group.to_string();
    }

    // ============= Alias 索引 =============

    /// 添加 alias 并更新索引
    pub fn add_alias(&mut self, alias: Alias) {
        let idx = self.aliases.len();
        let name = alias.name.clone();
        let group = alias.group.clone();
        self.aliases.push(alias);
        self.alias_by_name.insert(name, idx);
        if !group.is_empty() {
            self.alias_groups.entry(group).or_default().push(idx);
        }
    }

    /// 删除 alias 并更新索引
    pub fn delete_alias(&mut self, name: &str) -> bool {
        let idx = match self.alias_by_name.remove(name) {
            Some(i) => i,
            None => return false,
        };
        let group = self.aliases[idx].group.clone();
        if !group.is_empty() {
            if let Some(list) = self.alias_groups.get_mut(&group) {
                list.retain(|&i| i != idx);
                if list.is_empty() {
                    self.alias_groups.remove(&group);
                }
            }
        }
        self.swap_remove_alias(idx);
        true
    }

    fn swap_remove_alias(&mut self, idx: usize) {
        let last_idx = self.aliases.len() - 1;
        if idx != last_idx {
            let moved_name = self.aliases[last_idx].name.clone();
            let moved_group = self.aliases[last_idx].group.clone();
            self.aliases.swap_remove(idx);
            self.alias_by_name.insert(moved_name, idx);
            if !moved_group.is_empty() {
                if let Some(list) = self.alias_groups.get_mut(&moved_group) {
                    for i in list.iter_mut() {
                        if *i == last_idx {
                            *i = idx;
                            break;
                        }
                    }
                }
            }
        } else {
            self.aliases.pop();
        }
    }

    /// 按 group 批量启用/禁用 alias
    #[allow(dead_code)]
    pub fn enable_alias_group(&mut self, group: &str, enable: bool) {
        if let Some(indices) = self.alias_groups.get(group) {
            for &i in indices {
                self.aliases[i].enabled = enable;
            }
        }
    }

    /// 更新 alias 的 group 索引（SetAliasOption 修改 group 时调用）
    pub fn update_alias_group(&mut self, idx: usize, new_group: &str) {
        let old_group = self.aliases[idx].group.clone();
        if old_group == new_group {
            return;
        }
        if !old_group.is_empty() {
            if let Some(list) = self.alias_groups.get_mut(&old_group) {
                list.retain(|&i| i != idx);
                if list.is_empty() {
                    self.alias_groups.remove(&old_group);
                }
            }
        }
        if !new_group.is_empty() {
            self.alias_groups
                .entry(new_group.to_string())
                .or_default()
                .push(idx);
        }
        self.aliases[idx].group = new_group.to_string();
    }

    // ============= Timer 索引 =============

    /// 添加 timer 并更新索引
    pub fn add_timer(&mut self, timer: TimerDef) {
        let idx = self.timers.len();
        let name = timer.name.clone();
        let group = timer.group.clone();
        self.timers.push(timer);
        self.timer_by_name.insert(name, idx);
        if !group.is_empty() {
            self.timer_groups.entry(group).or_default().push(idx);
        }
    }

    /// 添加 DoAfter 系列一次性定时器（共用构造模式）
    pub fn add_doafter_timer(
        &mut self,
        name_prefix: &str,
        interval_millis: u64,
        send_text: String,
    ) {
        self.unique_counter += 1;
        let name = format!("{}_{}", name_prefix, self.unique_counter);
        self.add_timer(TimerDef {
            name,
            interval_millis,
            callback: None,
            enabled: true,
            group: String::new(),
            one_shot: true,
            at_time: false,
            send_text,
            next_fire: std::time::Instant::now()
                + std::time::Duration::from_millis(interval_millis),
        });
    }

    /// 删除 timer 并更新索引
    pub fn delete_timer(&mut self, name: &str) -> bool {
        let idx = match self.timer_by_name.remove(name) {
            Some(i) => i,
            None => return false,
        };
        let group = self.timers[idx].group.clone();
        if !group.is_empty() {
            if let Some(list) = self.timer_groups.get_mut(&group) {
                list.retain(|&i| i != idx);
                if list.is_empty() {
                    self.timer_groups.remove(&group);
                }
            }
        }
        self.swap_remove_timer(idx);
        true
    }

    fn swap_remove_timer(&mut self, idx: usize) {
        let last_idx = self.timers.len() - 1;
        if idx != last_idx {
            let moved_name = self.timers[last_idx].name.clone();
            let moved_group = self.timers[last_idx].group.clone();
            self.timers.swap_remove(idx);
            self.timer_by_name.insert(moved_name, idx);
            if !moved_group.is_empty() {
                if let Some(list) = self.timer_groups.get_mut(&moved_group) {
                    for i in list.iter_mut() {
                        if *i == last_idx {
                            *i = idx;
                            break;
                        }
                    }
                }
            }
        } else {
            self.timers.pop();
        }
    }

    /// 按 group 批量启用/禁用 timer
    pub fn enable_timer_group(&mut self, group: &str, enable: bool) {
        if let Some(indices) = self.timer_groups.get(group) {
            for &i in indices {
                self.timers[i].enabled = enable;
            }
        }
    }

    /// 更新 timer 的 group 索引（SetTimerOption 修改 group 时调用）
    pub fn update_timer_group(&mut self, idx: usize, new_group: &str) {
        let old_group = self.timers[idx].group.clone();
        if old_group == new_group {
            return;
        }
        if !old_group.is_empty() {
            if let Some(list) = self.timer_groups.get_mut(&old_group) {
                list.retain(|&i| i != idx);
                if list.is_empty() {
                    self.timer_groups.remove(&old_group);
                }
            }
        }
        if !new_group.is_empty() {
            self.timer_groups
                .entry(new_group.to_string())
                .or_default()
                .push(idx);
        }
        self.timers[idx].group = new_group.to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_trigger(name: &str, group: &str) -> Trigger {
        use mlua::Lua;
        use regex::Regex;
        let lua = Lua::new();
        let cb = lua.create_function(|_, ()| Ok(())).unwrap();
        Trigger {
            name: name.to_string(),
            pattern: super::super::types::TriggerPattern::Utf8(Regex::new("test").unwrap()),
            callback: cb,
            enabled: true,
            group: group.to_string(),
            sequence: 0,
            temporary: false,
            multiline: false,
            lines_to_match: 0,
            omit_from_output: false,
            one_shot: false,
            send_text: String::new(),
        }
    }

    #[test]
    fn test_add_and_find_trigger() {
        let mut state = ScriptState {
            triggers: Vec::new(),
            aliases: Vec::new(),
            timers: Vec::new(),
            trigger_by_name: HashMap::new(),
            trigger_groups: HashMap::new(),
            alias_by_name: HashMap::new(),
            alias_groups: HashMap::new(),
            timer_by_name: HashMap::new(),
            timer_groups: HashMap::new(),
            variables: HashMap::new(),
            pending_commands: Vec::new(),
            pending_raw: Vec::new(),
            pending_logs: Vec::new(),
            tell_buffer: String::new(),
            recent_lines: Vec::new(),
            unique_counter: 0,
            connected: false,
            connect_requested: false,
            disconnect_requested: false,
            host: String::new(),
            port: 0,
            world_name: String::new(),
            char_name: String::new(),
            packet_count: 0,
            status_text: String::new(),
            current_encoding: super::super::types::ScriptEncoding::Utf8,
            last_server_data: std::time::Instant::now(),
            pending_panels: Vec::new(),
            panel_handlers: HashMap::new(),
        };
        state.add_trigger(make_trigger("t1", "group_a"));
        state.add_trigger(make_trigger("t2", "group_a"));
        state.add_trigger(make_trigger("t3", "group_b"));

        assert_eq!(state.trigger_by_name.get("t1"), Some(&0));
        assert_eq!(state.trigger_by_name.get("t2"), Some(&1));
        assert_eq!(state.trigger_by_name.get("t3"), Some(&2));
        assert_eq!(state.trigger_groups.get("group_a").unwrap().len(), 2);
        assert_eq!(state.trigger_groups.get("group_b").unwrap().len(), 1);
    }

    #[test]
    fn test_delete_trigger_swap_remove() {
        let mut state = make_test_state();
        state.add_trigger(make_trigger("t1", "g1"));
        state.add_trigger(make_trigger("t2", "g1"));
        state.add_trigger(make_trigger("t3", "g1"));

        // 删除中间的 t2，t3 应 swap_remove 到 idx=1
        assert!(state.delete_trigger("t2"));
        assert_eq!(state.triggers.len(), 2);
        assert_eq!(state.triggers[0].name, "t1");
        assert_eq!(state.triggers[1].name, "t3"); // t3 被移动
        assert_eq!(state.trigger_by_name.get("t1"), Some(&0));
        assert_eq!(state.trigger_by_name.get("t3"), Some(&1)); // 索引更新
        assert!(state.trigger_by_name.get("t2").is_none());
        // group 索引更新
        let g1 = state.trigger_groups.get("g1").unwrap();
        assert!(g1.contains(&0));
        assert!(g1.contains(&1));
        assert!(!g1.contains(&2));
    }

    #[test]
    fn test_enable_trigger_group() {
        let mut state = make_test_state();
        state.add_trigger(make_trigger("t1", "g1"));
        state.add_trigger(make_trigger("t2", "other"));
        state.add_trigger(make_trigger("t3", "g1"));

        state.enable_trigger_group("g1", false);
        assert!(!state.triggers[0].enabled);
        assert!(state.triggers[1].enabled); // other 组不受影响
        assert!(!state.triggers[2].enabled);

        state.enable_trigger_group("g1", true);
        assert!(state.triggers[0].enabled);
        assert!(state.triggers[2].enabled);
    }

    use std::collections::HashMap;

    fn make_test_state() -> ScriptState {
        ScriptState {
            triggers: Vec::new(),
            aliases: Vec::new(),
            timers: Vec::new(),
            trigger_by_name: HashMap::new(),
            trigger_groups: HashMap::new(),
            alias_by_name: HashMap::new(),
            alias_groups: HashMap::new(),
            timer_by_name: HashMap::new(),
            timer_groups: HashMap::new(),
            variables: HashMap::new(),
            pending_commands: Vec::new(),
            pending_raw: Vec::new(),
            pending_logs: Vec::new(),
            tell_buffer: String::new(),
            recent_lines: Vec::new(),
            unique_counter: 0,
            connected: false,
            connect_requested: false,
            disconnect_requested: false,
            host: String::new(),
            port: 0,
            world_name: String::new(),
            char_name: String::new(),
            packet_count: 0,
            status_text: String::new(),
            current_encoding: super::super::types::ScriptEncoding::Utf8,
            last_server_data: std::time::Instant::now(),
            pending_panels: Vec::new(),
            panel_handlers: HashMap::new(),
        }
    }
}
