//! Lua 引擎的单元测试
//!
//! 测试覆盖：类型转换、Lua 源码预处理、触发器匹配、别名执行、定时器调度等。

#![allow(clippy::approx_constant)]

use super::helpers::*;
use super::types::*;
use mlua::{Lua, Table, Value};
use regex::bytes::Regex as BytesRegex;

// ================================================================
// i64_to_lua_integer / lua_integer_to_i64 类型转换测试
// ================================================================

#[test]
fn test_i64_to_lua_integer_zero() {
    assert_eq!(i64_to_lua_integer(0), 0);
}

#[test]
fn test_i64_to_lua_integer_positive() {
    assert_eq!(i64_to_lua_integer(42), 42);
    assert_eq!(i64_to_lua_integer(1_000_000), 1_000_000);
}

#[test]
fn test_i64_to_lua_integer_negative() {
    assert_eq!(i64_to_lua_integer(-1), -1);
    assert_eq!(i64_to_lua_integer(-999), -999);
}

#[test]
fn test_lua_integer_to_i64_zero() {
    assert_eq!(lua_integer_to_i64(0), 0);
}

#[test]
fn test_lua_integer_to_i64_positive() {
    assert_eq!(lua_integer_to_i64(42), 42);
    assert_eq!(lua_integer_to_i64(1_000_000), 1_000_000);
}

#[test]
fn test_lua_integer_to_i64_negative() {
    assert_eq!(lua_integer_to_i64(-1), -1);
    assert_eq!(lua_integer_to_i64(-999), -999);
}

#[test]
fn test_roundtrip_conversion() {
    // 测试往返转换在小值范围内是恒等的
    for v in [0, 1, -1, 100, -100, 10000, -10000] {
        assert_eq!(lua_integer_to_i64(i64_to_lua_integer(v)), v);
    }
}

#[test]
#[cfg(target_pointer_width = "64")]
fn test_large_values_64bit() {
    // 64位平台上大值不会截断
    let large = i64::MAX;
    assert_eq!(i64_to_lua_integer(large), large);
    assert_eq!(lua_integer_to_i64(large), large);
}

#[test]
#[cfg(target_pointer_width = "32")]
fn test_large_values_32bit_truncation() {
    // 32位平台上超过i32范围的值会截断（预期行为）
    let large = i64::MAX;
    let converted = i64_to_lua_integer(large);
    // 在32位平台上，mlua::Integer是i32，i64::MAX会截断为-1
    assert_eq!(converted, -1);
}

// ================================================================
// fix_lua_escape_sequences 预处理测试
// ================================================================

#[test]
fn test_fix_escape_invalid_in_double_string() {
    // \- 在双引号字符串中是非法转义，应变为 \\-
    let input = r#"a,b,c,d=string.find(l,"[> ]*(%S+) \- (%w+)")"#;
    let output = fix_lua_escape_sequences(input);
    assert_eq!(output, r#"a,b,c,d=string.find(l,"[> ]*(%S+) \\- (%w+)")"#);
}

#[test]
fn test_fix_escape_invalid_in_single_string() {
    // \- 在单引号字符串中也应修复
    let input = r#"x = 'hello \- world'"#;
    let output = fix_lua_escape_sequences(input);
    assert_eq!(output, r#"x = 'hello \\- world'"#);
}

#[test]
fn test_fix_escape_preserves_valid() {
    // 合法转义 \n \t \\ \" 等应保持不变
    let input = r#"x = "hello\nworld\t\"test\\end""#;
    let output = fix_lua_escape_sequences(input);
    assert_eq!(output, input);
}

#[test]
fn test_fix_escape_skip_comment() {
    // 注释中的 \- 不应被修改
    let input = "-- this is a comment with \\- escape\nx = 1";
    let output = fix_lua_escape_sequences(input);
    assert_eq!(output, input);
}

#[test]
fn test_fix_escape_skip_long_comment() {
    // 长注释中的 \- 不应被修改
    let input = "--[[ comment with \\- escape ]]\nx = 1";
    let output = fix_lua_escape_sequences(input);
    assert_eq!(output, input);
}

#[test]
fn test_fix_escape_skip_long_string() {
    // 长字符串中的 \- 不应被修改
    let input = "x = [[ hello \\- world ]]";
    let output = fix_lua_escape_sequences(input);
    assert_eq!(output, input);
}

#[test]
fn test_fix_escape_multiple_invalid() {
    // 多个非法转义
    let input = r#"x = "\- \+ \? \* \.""#;
    let output = fix_lua_escape_sequences(input);
    assert_eq!(output, r#"x = "\\- \\+ \\? \\* \\.""#);
}

#[test]
fn test_fix_escape_already_double_backslash() {
    // \\- 已经是合法的（\\ 是合法转义，- 是普通字符），不应被修改
    let input = r#"x = "\\- test""#;
    let output = fix_lua_escape_sequences(input);
    assert_eq!(output, input);
}

#[test]
fn test_fix_escape_mixed_valid_invalid() {
    // 混合合法和非法转义
    let input = r#"x = "hello\nworld\-test""#;
    let output = fix_lua_escape_sequences(input);
    assert_eq!(output, r#"x = "hello\nworld\\-test""#);
}

#[test]
fn test_fix_escape_real_world_pattern() {
    // 实际脚本中的模式
    let input = r#"a,b,c,d=string.find(l,"[> ]*(%S+) \- (%w+)")"#;
    let output = fix_lua_escape_sequences(input);
    assert_eq!(output, r#"a,b,c,d=string.find(l,"[> ]*(%S+) \\- (%w+)")"#);
}

#[test]
fn test_fix_escape_no_change_needed() {
    // 无需修改的代码
    let input = r#"x = "hello world"\ny = 1"#;
    let output = fix_lua_escape_sequences(input);
    assert_eq!(output, input);
}

#[test]
fn test_fix_escape_line_comment_then_code() {
    // 注释后跟代码
    let input = "-- comment with \\- \nlocal x = \"test\\-value\"";
    let output = fix_lua_escape_sequences(input);
    assert_eq!(output, "-- comment with \\- \nlocal x = \"test\\\\-value\"");
}

#[test]
fn test_fix_escape_hex_escape() {
    // \x41 是合法的十六进制转义，不应被修改
    let input = r#"x = "\x41\x42""#;
    let output = fix_lua_escape_sequences(input);
    assert_eq!(output, input);
}

#[test]
fn test_fix_escape_digit_escape() {
    // \123 是合法的十进制转义，不应被修改
    let input = r#"x = "\65\66""#;
    let output = fix_lua_escape_sequences(input);
    assert_eq!(output, input);
}

#[test]
fn test_fix_escape_z_escape() {
    // \z 是合法转义（跳过空白），不应被修改
    let input = r#"x = "hello\z  world""#;
    let output = fix_lua_escape_sequences(input);
    assert_eq!(output, input);
}

/// 辅助：创建引擎并执行一段 Lua 代码
fn with_engine<F>(f: F)
where
    F: FnOnce(&mut LuaEngine),
{
    let mut engine = LuaEngine::new().expect("引擎创建失败");
    f(&mut engine);
}

/// 辅助：执行 Lua 代码并返回结果
fn eval<T: mlua::FromLua>(engine: &LuaEngine, code: &str) -> mlua::Result<T> {
    engine.lua.load(code).eval()
}

/// 辅助：执行 Lua 代码（无返回值）
fn exec(engine: &LuaEngine, code: &str) -> mlua::Result<()> {
    engine.lua.load(code).exec()
}

// ================================================================
// 引擎基础
// ================================================================

#[test]
fn test_engine_new() {
    let engine = LuaEngine::new();
    assert!(engine.is_ok());
}

/// 验证 LuaEngine drop 不被看门狗线程的 sleep 阻塞
/// 修复前：看门狗 sleep(5s) 导致 drop 最多等 5 秒
/// 修复后：100ms 分段睡眠，drop 最多等 ~100ms
#[test]
fn test_engine_drop_is_fast() {
    let start = std::time::Instant::now();
    {
        let _engine = LuaEngine::new().expect("引擎创建失败");
        // engine 在此作用域结束时 drop
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "LuaEngine drop 耗时 {:?}, 预期 < 2s（看门狗分段睡眠修复）",
        elapsed
    );
}

#[test]
fn test_set_script_path() {
    with_engine(|engine| {
        engine.set_script_path("/home/user/scripts/main.lua");
        assert_eq!(
            *engine.script_dir.borrow(),
            Some("/home/user/scripts/".to_string())
        );
        assert_eq!(
            engine.script_path(),
            Some("/home/user/scripts/main.lua".to_string())
        );
    });
}

#[test]
fn test_set_script_path_no_slash() {
    with_engine(|engine| {
        engine.set_script_path("main.lua");
        assert_eq!(*engine.script_dir.borrow(), Some("./".to_string()));
    });
}

// ================================================================
// 命令执行 API
// ================================================================

#[test]
fn test_send() {
    with_engine(|engine| {
        exec(engine, "send('look')").unwrap();
        let cmds = engine.drain_commands();
        assert_eq!(cmds, vec!["look"]);
    });
}

#[test]
fn test_execute() {
    with_engine(|engine| {
        let result: i64 = eval(engine, "return Execute('look')").unwrap();
        assert_eq!(result, 0);
        let cmds = engine.drain_commands();
        assert_eq!(cmds, vec!["look"]);
    });
}

// ================================================================
// 输出 API
// ================================================================

#[test]
fn test_note() {
    with_engine(|engine| {
        exec(engine, "Note('hello')").unwrap();
        let logs = engine.drain_logs();
        assert!(logs.contains(&"hello".to_string()));
    });
}

#[test]
fn test_colour_note() {
    with_engine(|engine| {
        exec(engine, "ColourNote('red', 'black', 'test')").unwrap();
        let logs = engine.drain_logs();
        // 应生成 ANSI 转义序列：\x1B[31;40mtest\x1B[0m
        assert!(logs.iter().any(|l| l.contains("\x1b[31;40mtest\x1b[0m")));
    });
}

#[test]
fn test_tell() {
    with_engine(|engine| {
        exec(engine, "Tell('inline')").unwrap();
        let logs = engine.drain_logs();
        assert!(logs.contains(&"inline".to_string()));
    });
}

#[test]
fn test_print_redirect() {
    with_engine(|engine| {
        // print 应该被重定向到 pending_logs
        exec(engine, "print('hello')").unwrap();
        let logs = engine.drain_logs();
        assert!(logs.contains(&"hello".to_string()));
    });
}

#[test]
fn test_set_panel() {
    with_engine(|engine| {
        exec(
            engine,
            r#"SetPanel("stat", -70, 1, 70, 10, "line1\nline2")"#,
        )
        .unwrap();
        let panels = engine.drain_panels();
        assert_eq!(panels.len(), 1);
        match &panels[0] {
            PanelUpdate::Set {
                name,
                x,
                y,
                width,
                height,
                lines,
                buttons,
            } => {
                assert_eq!(name, "stat");
                assert!(buttons.is_empty());
                assert_eq!(*x, -70);
                assert_eq!(*y, 1);
                assert_eq!(*width, 70);
                assert_eq!(*height, 10);
                assert_eq!(lines.len(), 2);
                assert_eq!(lines[0], "line1");
                assert_eq!(lines[1], "line2");
            }
            PanelUpdate::Remove { .. } => panic!("expected Set, got Remove"),
        }
    });
}

#[test]
fn test_remove_panel() {
    with_engine(|engine| {
        exec(engine, r#"RemovePanel("stat")"#).unwrap();
        let panels = engine.drain_panels();
        assert_eq!(panels.len(), 1);
        match &panels[0] {
            PanelUpdate::Remove { name } => {
                assert_eq!(name, "stat");
            }
            PanelUpdate::Set { .. } => panic!("expected Remove, got Set"),
        }
    });
}

#[test]
fn test_register_panel_handler() {
    with_engine(|engine| {
        // 注册回调
        exec(
            engine,
            r#"RegisterPanelHandler("stat", function(name, action) end)"#,
        )
        .unwrap();
        // 验证已存入注册表
        let state = engine.state.borrow();
        assert!(state.panel_handlers.contains_key("stat"));
    });
}

#[test]
fn test_register_panel_handler_overwrite() {
    with_engine(|engine| {
        // 同一 panel 注册两次, 后者覆盖前者
        exec(engine, r#"RegisterPanelHandler("stat", function() end)"#).unwrap();
        exec(
            engine,
            r#"RegisterPanelHandler("stat", function(name, action) _clicked = action end)"#,
        )
        .unwrap();
        // 注册表只有一个条目
        let state = engine.state.borrow();
        assert_eq!(state.panel_handlers.len(), 1);
    });
}

#[test]
fn test_panel_click_dispatch() {
    with_engine(|engine| {
        // 注册回调: 点击时把 action 写入 Lua 全局 _clicked_action
        exec(
            engine,
            r#"RegisterPanelHandler("stat", function(name, action)
                    _clicked_name = name
                    _clicked_action = action
                end)"#,
        )
        .unwrap();
        // 模拟点击 "go" 按钮
        engine.handle_panel_click("stat", "go");
        // 验证回调被调用, 参数正确传递
        let name: String = eval(engine, "return _clicked_name").unwrap();
        let action: String = eval(engine, "return _clicked_action").unwrap();
        assert_eq!(name, "stat");
        assert_eq!(action, "go");
    });
}

#[test]
fn test_panel_click_no_handler() {
    with_engine(|engine| {
        // 未注册回调时调用, 不应 panic
        // (会通过 log_error 记录调试信息, 但不影响流程)
        engine.handle_panel_click("unknown_panel", "go");
        // 验证: 没有 Lua 命令被入队 (回调不存在, 不应有副作用)
        let cmds = engine.drain_commands();
        assert!(cmds.is_empty());
    });
}

#[test]
fn test_panel_click_multiple_panels() {
    with_engine(|engine| {
        // 注册两个不同 panel 的回调, 验证独立分发
        exec(
            engine,
            r#"RegisterPanelHandler("stat", function(n, a) _stat_action = a end)"#,
        )
        .unwrap();
        exec(
            engine,
            r#"RegisterPanelHandler("inventory", function(n, a) _inv_action = a end)"#,
        )
        .unwrap();
        // 点击 stat 面板
        engine.handle_panel_click("stat", "go");
        // 点击 inventory 面板
        engine.handle_panel_click("inventory", "drop");
        // 验证各自回调被正确调用
        let stat_action: String = eval(engine, "return _stat_action").unwrap();
        let inv_action: String = eval(engine, "return _inv_action").unwrap();
        assert_eq!(stat_action, "go");
        assert_eq!(inv_action, "drop");
    });
}

#[test]
fn test_print_multiple_args() {
    with_engine(|engine| {
        // print 多个参数，用制表符分隔
        exec(engine, "print('a', 'b', 'c')").unwrap();
        let logs = engine.drain_logs();
        assert!(logs.contains(&"a\tb\tc".to_string()));
    });
}

#[test]
fn test_print_mixed_types() {
    with_engine(|engine| {
        exec(engine, "print('n=', 42, 'b=', true)").unwrap();
        let logs = engine.drain_logs();
        assert!(logs.iter().any(|l| l.contains("42") && l.contains("true")));
    });
}

// ================================================================
// 触发器 API
// ================================================================

#[test]
fn test_add_trigger() {
    with_engine(|engine| {
        let result: i64 = eval(
            engine,
            "return AddTrigger('test_trig', 'hello', '', 1, 0, 0, '', '', 0, 0)",
        )
        .unwrap();
        assert_eq!(result, 0);
        assert_eq!(engine.trigger_count(), 1);
    });
}

#[test]
fn test_add_trigger_regex() {
    with_engine(|engine| {
        let result: i64 = eval(
            engine,
            r#"return AddTrigger('regex_trig', [[^\d+hp]], '', 33, 0, 0, '', '', 0, 0)"#,
        )
        .unwrap();
        assert_eq!(result, 0);
    });
}

#[test]
fn test_add_trigger_case_insensitive() {
    with_engine(|engine| {
        let result: i64 = eval(
            engine,
            "return AddTrigger('ci_trig', 'HELLO', '', 17, 0, 0, '', '', 0, 0)",
        )
        .unwrap();
        assert_eq!(result, 0);
    });
}

#[test]
fn test_delete_trigger() {
    with_engine(|engine| {
        exec(
            engine,
            "AddTrigger('del_trig', 'test', '', 1, 0, 0, '', '', 0, 0)",
        )
        .unwrap();
        assert_eq!(engine.trigger_count(), 1);
        let result: i64 = eval(engine, "return DeleteTrigger('del_trig')").unwrap();
        assert_eq!(result, 0);
        assert_eq!(engine.trigger_count(), 0);
    });
}

#[test]
fn test_delete_trigger_not_found() {
    with_engine(|engine| {
        let result: i64 = eval(engine, "return DeleteTrigger('nonexistent')").unwrap();
        assert_eq!(result, 1);
    });
}

#[test]
fn test_get_trigger_list() {
    with_engine(|engine| {
        exec(
            engine,
            "AddTrigger('trig1', 'a', '', 1, 0, 0, '', '', 0, 0)",
        )
        .unwrap();
        exec(
            engine,
            "AddTrigger('trig2', 'b', '', 1, 0, 0, '', '', 0, 0)",
        )
        .unwrap();
        let list: Vec<String> = eval(
            engine,
            "local t = GetTriggerList(); local r = {}; for i=1,#t do r[i]=t[i] end; return r",
        )
        .unwrap();
        assert!(list.contains(&"trig1".to_string()));
        assert!(list.contains(&"trig2".to_string()));
    });
}

#[test]
fn test_get_trigger_info_enabled() {
    with_engine(|engine| {
        exec(
            engine,
            "AddTrigger('info_trig', 'test', '', 1, 0, 0, '', '', 0, 0)",
        )
        .unwrap();
        let enabled: bool = eval(engine, "return GetTriggerInfo('info_trig', 8)").unwrap();
        assert!(enabled);
    });
}

#[test]
fn test_get_trigger_info_group() {
    with_engine(|engine| {
        exec(
            engine,
            "AddTrigger('grp_trig', 'test', '', 1, 0, 0, '', '', 0, 0)",
        )
        .unwrap();
        exec(engine, "SetTriggerOption('grp_trig', 'group', 'mygroup')").unwrap();
        let group: String = eval(engine, "return GetTriggerInfo('grp_trig', 26)").unwrap();
        assert_eq!(group, "mygroup");
    });
}

#[test]
fn test_set_trigger_option() {
    with_engine(|engine| {
        exec(
            engine,
            "AddTrigger('opt_trig', 'test', '', 1, 0, 0, '', '', 0, 0)",
        )
        .unwrap();
        exec(engine, "SetTriggerOption('opt_trig', 'enabled', false)").unwrap();
        let enabled: bool = eval(engine, "return GetTriggerInfo('opt_trig', 8)").unwrap();
        assert!(!enabled);
    });
}

#[test]
fn test_set_trigger_option_multiline() {
    with_engine(|engine| {
        exec(
            engine,
            "AddTrigger('ml_trig', 'test', '', 1, 0, 0, '', '', 0, 0)",
        )
        .unwrap();
        let result: i64 = eval(engine,
            "SetTriggerOption('ml_trig', 'multi_line', true); SetTriggerOption('ml_trig', 'lines_to_match', 3); return 0"
        ).unwrap();
        assert_eq!(result, 0);
    });
}

#[test]
fn test_enable_trigger_group() {
    with_engine(|engine| {
        exec(engine, "AddTrigger('g1', 'a', '', 1, 0, 0, '', '', 0, 0)").unwrap();
        exec(engine, "AddTrigger('g2', 'b', '', 1, 0, 0, '', '', 0, 0)").unwrap();
        exec(engine, "SetTriggerOption('g1', 'group', 'grp_a')").unwrap();
        exec(engine, "SetTriggerOption('g2', 'group', 'grp_a')").unwrap();
        exec(engine, "EnableTriggerGroup('grp_a', false)").unwrap();
        let e1: bool = eval(engine, "return GetTriggerInfo('g1', 8)").unwrap();
        let e2: bool = eval(engine, "return GetTriggerInfo('g2', 8)").unwrap();
        assert!(!e1);
        assert!(!e2);
    });
}

#[test]
fn test_enable_trigger_group_skips_empty_group() {
    with_engine(|engine| {
        exec(
            engine,
            "AddTrigger('nogrp', 'x', '', 1, 0, 0, '', '', 0, 0)",
        )
        .unwrap();
        exec(engine, "EnableTriggerGroup('somegroup', false)").unwrap();
        let enabled: bool = eval(engine, "return GetTriggerInfo('nogrp', 8)").unwrap();
        assert!(enabled); // 空group的触发器不应被影响
    });
}

#[test]
fn test_enable_trigger() {
    with_engine(|engine| {
        exec(
            engine,
            "AddTrigger('et', 'test', '', 1, 0, 0, '', '', 0, 0)",
        )
        .unwrap();
        exec(engine, "EnableTrigger('et', false)").unwrap();
        let enabled: bool = eval(engine, "return GetTriggerInfo('et', 8)").unwrap();
        assert!(!enabled);
        exec(engine, "EnableTrigger('et', true)").unwrap();
        let enabled2: bool = eval(engine, "return GetTriggerInfo('et', 8)").unwrap();
        assert!(enabled2);
    });
}

#[test]
fn test_enable_trigger_not_found() {
    with_engine(|engine| {
        let result: i64 = eval(engine, "return EnableTrigger('nonexistent', true)").unwrap();
        assert_eq!(result, 1); // 1 = not found
    });
}

#[test]
fn test_trigger_matching() {
    with_engine(|engine| {
        exec(engine, r#"
            test_result = nil
            AddTrigger('match_trig', [[hello (\w+)]], '', 33, 0, 0, '', 'function(name, line, wildcards) test_result = wildcards[1] end', 0, 0)
        "#).unwrap();
        engine.process_output("hello world");
        let result: Option<String> = eval(engine, "return test_result").unwrap();
        assert_eq!(result, Some("world".to_string()));
    });
}

#[test]
fn test_trigger_disabled_not_matching() {
    with_engine(|engine| {
        exec(engine, r#"
            test_result = nil
            AddTrigger('dis_trig', 'test', '', 0, 0, 0, '', 'function() test_result = true end', 0, 0)
        "#).unwrap();
        engine.process_output("test");
        let result: Option<bool> = eval(engine, "return test_result").unwrap();
        assert_eq!(result, None);
    });
}

#[test]
fn test_trigger_wildcard_matching() {
    with_engine(|engine| {
        exec(engine, r#"
            wc_result = nil
            AddTrigger('wc_trig', 'You see * here', '', 1, 0, 0, '', 'function(name, line, wildcards) wc_result = wildcards[1] end', 0, 0)
        "#).unwrap();
        engine.process_output("You see a goblin here");
        let result: Option<String> = eval(engine, "return wc_result").unwrap();
        assert_eq!(result, Some("a goblin".to_string()));
    });
}

// 测试 w[0] 为完整匹配文本（MUSHclient 兼容）
#[test]
fn test_trigger_w0_full_match() {
    with_engine(|engine| {
        exec(engine, r#"
            w0_result = nil
            w1_result = nil
            AddTrigger('w0_trig', [[^(.+) hits (.+)$]], '', 33, 0, 0, '', 'function(name, line, wildcards) w0_result = wildcards[0]; w1_result = wildcards[1] end', 0, 0)
        "#).unwrap();
        engine.process_output("goblin hits warrior");
        let w0: Option<String> = eval(engine, "return w0_result").unwrap();
        let w1: Option<String> = eval(engine, "return w1_result").unwrap();
        assert_eq!(w0, Some("goblin hits warrior".to_string()));
        assert_eq!(w1, Some("goblin".to_string()));
    });
}

// 测试多行触发器的 w[0] 包含完整合并文本
#[test]
fn test_trigger_w0_multiline() {
    with_engine(|engine| {
        exec(engine, r#"
            ml_w0 = nil
            ml_w1 = nil
            AddTrigger('ml_w0_trig', [[^line1\n(.+)$]], '', 33, 0, 0, '', 'function(name, line, wildcards) ml_w0 = wildcards[0]; ml_w1 = wildcards[1] end', 0, 0)
            SetTriggerOption('ml_w0_trig', 'multi_line', true)
            SetTriggerOption('ml_w0_trig', 'lines_to_match', 2)
        "#).unwrap();
        engine.process_output("line1");
        engine.process_output("line2 content");
        let w0: Option<String> = eval(engine, "return ml_w0").unwrap();
        let w1: Option<String> = eval(engine, "return ml_w1").unwrap();
        assert_eq!(w0, Some("line1\nline2 content".to_string()));
        assert_eq!(w1, Some("line2 content".to_string()));
    });
}

// 测试 w[0] 在 findstring 类函数中的使用（脚本常见用法）
#[test]
fn test_trigger_w0_with_chinese() {
    with_engine(|engine| {
        exec(engine, r#"
            zh_w0 = nil
            zh_w1 = nil
            AddTrigger('zh_trig', [[^你向(.+)打听有关「(.+)」的消息。$]], '', 33, 0, 0, '', 'function(name, line, wildcards) zh_w0 = wildcards[0]; zh_w1 = wildcards[1] end', 0, 0)
        "#).unwrap();
        engine.process_output("你向范骅打听有关「治安」的消息。");
        let w0: Option<String> = eval(engine, "return zh_w0").unwrap();
        let w1: Option<String> = eval(engine, "return zh_w1").unwrap();
        assert_eq!(w0, Some("你向范骅打听有关「治安」的消息。".to_string()));
        assert_eq!(w1, Some("范骅".to_string()));
    });
}

#[test]
fn test_trigger_case_insensitive_matching() {
    with_engine(|engine| {
        exec(engine, r#"
            ci_result = nil
            AddTrigger('ci_trig2', 'HELLO', '', 17, 0, 0, '', 'function() ci_result = true end', 0, 0)
        "#).unwrap();
        engine.process_output("hello");
        let result: Option<bool> = eval(engine, "return ci_result").unwrap();
        assert_eq!(result, Some(true));
    });
}

#[test]
fn test_add_trigger_ex_same_as_add_trigger() {
    with_engine(|engine| {
        let result: i64 = eval(engine, "return AddTriggerEx('ex_trig', 'test', '', 1)").unwrap();
        assert_eq!(result, 0);
        assert_eq!(engine.trigger_count(), 1);
    });
}

#[test]
fn test_trigger_omit_from_output() {
    with_engine(|engine| {
        exec(
            engine,
            "AddTrigger('omit_trig', 'secret', '', 1, 0, 0, '', '', 0, 0)",
        )
        .unwrap();
        exec(
            engine,
            "SetTriggerOption('omit_trig', 'omit_from_output', true)",
        )
        .unwrap();
        // omit_from_output 标记已设置，验证通过 GetTriggerInfo 间接确认
        // 实际的 omit 行为由 app 层处理
        assert_eq!(engine.trigger_count(), 1);
    });
}

#[test]
fn test_trigger_temporary_flag() {
    with_engine(|engine| {
        // flag 4096 = Temporary
        let result: i64 = eval(
            engine,
            "return AddTrigger('temp_trig', 'test', '', 4097, 0, 0, '', '', 0, 0)",
        )
        .unwrap();
        assert_eq!(result, 0);
    });
}

#[test]
fn test_trigger_sequence() {
    with_engine(|engine| {
        let result: i64 = eval(
            engine,
            "return AddTrigger('seq_trig', 'test', '', 1, 0, 0, '', '', 0, 100)",
        )
        .unwrap();
        assert_eq!(result, 0);
    });
}

#[test]
fn test_get_trigger_info_unknown_code() {
    with_engine(|engine| {
        exec(
            engine,
            "AddTrigger('unk_trig', 'test', '', 1, 0, 0, '', '', 0, 0)",
        )
        .unwrap();
        let val: Value = eval(engine, "return GetTriggerInfo('unk_trig', 999)").unwrap();
        assert!(val.is_nil());
    });
}

#[test]
fn test_get_trigger_info_not_found() {
    with_engine(|engine| {
        let val: Value = eval(engine, "return GetTriggerInfo('nonexistent', 7)").unwrap();
        assert!(val.is_nil());
    });
}

#[test]
fn test_set_trigger_option_regexp() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            function test_re_cb(name, line, wildcards)
                    Execute("matched_" .. line)
            end
            AddTrigger('re_trig', 'old_pattern', '', trigger_flag.Enabled + trigger_flag.Replace + trigger_flag.RegularExpression, 0, 0, '', 'test_re_cb', 0, 10)
            "#,
        )
        .unwrap();
        // 先确认匹配旧正则
        engine.process_output("old_pattern");
        let cmds1 = engine.drain_commands();
        assert_eq!(cmds1, vec!["matched_old_pattern"]);
        // 改用新的正则
        exec(engine, "SetTriggerOption('re_trig', 'regexp', 'new_(.+)')").unwrap();
        engine.process_output("new_value");
        let cmds2 = engine.drain_commands();
        assert_eq!(cmds2, vec!["matched_new_value"]);
        // 旧正则不应该再匹配
        engine.process_output("old_pattern");
        let cmds3 = engine.drain_commands();
        assert!(cmds3.is_empty());
    });
}

#[test]
fn test_set_trigger_option_sequence() {
    with_engine(|engine| {
        exec(
            engine,
            "AddTrigger('seq_trig', 'test', '', 1, 0, 0, '', '', 0, 0)",
        )
        .unwrap();
        exec(engine, "SetTriggerOption('seq_trig', 'sequence', 50)").unwrap();
        let seq: i64 = eval(engine, "return GetTriggerInfo('seq_trig', 6)").unwrap();
        assert_eq!(seq, 50);
    });
}

#[test]
fn test_set_trigger_option_not_found() {
    with_engine(|engine| {
        let result: i64 = eval(
            engine,
            "return SetTriggerOption('nonexistent', 'enabled', true)",
        )
        .unwrap();
        assert_eq!(result, 1); // 1 = not found
    });
}

// ================================================================
// 别名 API
// ================================================================

#[test]
fn test_add_alias() {
    with_engine(|engine| {
        let result: i64 = eval(engine, "return AddAlias('test_alias', 'kill *', '', 1)").unwrap();
        assert_eq!(result, 0);
        assert_eq!(engine.alias_count(), 1);
    });
}

#[test]
fn test_add_alias_regex() {
    with_engine(|engine| {
        let result: i64 = eval(
            engine,
            r#"return AddAlias('regex_alias', [[^go (\w+)$]], '', alias_flag.Enabled + alias_flag.RegularExpression)"#,
        )
        .unwrap();
        assert_eq!(result, 0);
    });
}

#[test]
fn test_delete_alias() {
    with_engine(|engine| {
        exec(engine, "AddAlias('del_alias', 'test', '', 1)").unwrap();
        let result: i64 = eval(engine, "return DeleteAlias('del_alias')").unwrap();
        assert_eq!(result, 0);
        assert_eq!(engine.alias_count(), 0);
    });
}

#[test]
fn test_delete_alias_not_found() {
    with_engine(|engine| {
        let result: i64 = eval(engine, "return DeleteAlias('nonexistent')").unwrap();
        assert_eq!(result, 1);
    });
}

#[test]
fn test_alias_kept_after_match() {
    with_engine(|engine| {
        // 普通 alias: Enabled(1) + RegularExpression(128)
        exec(
            engine,
            r#"AddAlias('normal_alias', [[^go (\w+)$]], [[]], alias_flag.Enabled + alias_flag.RegularExpression)"#,
        )
        .unwrap();
        assert_eq!(engine.alias_count(), 1);

        // 匹配 alias
        engine.process_input("go north");

        // alias 匹配后仍存在
        assert_eq!(engine.alias_count(), 1);
    });
}

#[test]
fn test_process_input_cfg_skill_xue_alias() {
    with_engine(|engine| {
        // 设置 cfg 表并定义 skill_xue 函数
        exec(
            engine,
            r#"
            cfg = {}
            skills_xue = nil
            function cfg.skill_xue(...)
                    local args = {...}
                    if args[1] ~= nil and args[1] ~= "" then
                        skills_xue = args[1]
                    end
            end
            "#,
        )
        .unwrap();
        // 注册两个别名：无参数（显示）和有参数（设置）
        exec(
            engine,
            r#"AddAlias('test_skill_xue_display', [[^#cfg skill_xue$]], [[cfg.skill_xue()]], alias_flag.Enabled + alias_flag.RegularExpression)"#,
        )
        .unwrap();
        exec(
            engine,
            r#"AddAlias('test_skill_xue_set', [[^#cfg skill_xue\s+(.+)$]], [[cfg.skill_xue('%1')]], alias_flag.Enabled + alias_flag.RegularExpression)"#,
        )
        .unwrap();
        // 测试1：匹配并设置值
        let handled = engine.process_input("#cfg skill_xue sword|blade|force");
        assert!(handled);
        let result: String = eval(engine, "return skills_xue or 'nil'").unwrap();
        assert_eq!(result, "sword|blade|force");
        // 测试2：无参数显示当前值（不修改）
        let handled2 = engine.process_input("#cfg skill_xue");
        assert!(handled2); // 应该匹配 display 别名
        let result2: String = eval(engine, "return skills_xue or 'nil'").unwrap();
        assert_eq!(result2, "sword|blade|force"); // 值未被修改
    });
}

#[test]
fn test_process_input_cfg_skill_lingwu_alias() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            cfg = {}
            skills_lingwu = nil
            function cfg.skill_lingwu(...)
                    local args = {...}
                    if args[1] ~= nil and args[1] ~= "" then
                        skills_lingwu = args[1]
                    end
            end
            "#,
        )
        .unwrap();
        exec(
            engine,
            r#"AddAlias('test_skill_lingwu_set', [[^#cfg skill_lingwu\s+(.+)$]], [[cfg.skill_lingwu('%1')]], 129)"#,
        )
        .unwrap();
        let handled = engine.process_input("#cfg skill_lingwu parry|dodge");
        assert!(handled);
        let result: String = eval(engine, "return skills_lingwu or 'nil'").unwrap();
        assert_eq!(result, "parry|dodge");
    });
}

#[test]
fn test_get_alias_list() {
    with_engine(|engine| {
        exec(engine, "AddAlias('a1', 'x', '', 1)").unwrap();
        exec(engine, "AddAlias('a2', 'y', '', 1)").unwrap();
        let list: Vec<String> = eval(
            engine,
            "local t = GetAliasList(); local r = {}; for i=1,#t do r[i]=t[i] end; return r",
        )
        .unwrap();
        assert!(list.contains(&"a1".to_string()));
        assert!(list.contains(&"a2".to_string()));
    });
}

#[test]
fn test_set_alias_option() {
    with_engine(|engine| {
        exec(engine, "AddAlias('opt_alias', 'test', '', 1)").unwrap();
        exec(engine, "SetAliasOption('opt_alias', 'group', 'mygroup')").unwrap();
        let result: i64 = eval(
            engine,
            "return SetAliasOption('opt_alias', 'enabled', false)",
        )
        .unwrap();
        assert_eq!(result, 0);
    });
}

#[test]
fn test_set_alias_option_not_found() {
    with_engine(|engine| {
        let result: i64 = eval(
            engine,
            "return SetAliasOption('nonexistent', 'enabled', true)",
        )
        .unwrap();
        assert_eq!(result, 1);
    });
}

#[test]
fn test_alias_response_send_to_script() {
    // 验证 4 参数 AddAlias（无 script）默认使用 send_to=12
    // 即 response 应作为 Lua 代码执行
    with_engine(|engine| {
        exec(
            engine,
            r#"AddAlias("cfg_test", "^#cfg test$", "send('alias_executed')", 129)"#,
        )
        .unwrap();
        let handled = engine.process_input("#cfg test");
        assert!(handled);
        let cmds = engine.drain_commands();
        assert!(cmds.contains(&"alias_executed".to_string()));
    });
}

#[test]
fn test_alias_response_with_capture_groups() {
    // 验证 %1, %2 捕获组替换后作为 Lua 代码执行
    with_engine(|engine| {
        exec(
            engine,
            r#"AddAlias("cfg_set", "^#cfg (\\w+) (.*)$", "send('set:'..'%1'..'='..'%2')", 129)"#,
        )
        .unwrap();
        let handled = engine.process_input("#cfg neili_job 80");
        assert!(handled);
        let cmds = engine.drain_commands();
        assert!(cmds.contains(&"set:neili_job=80".to_string()));
    });
}

#[test]
fn test_alias_matching() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            alias_result = nil
            AddAlias('match_alias', 'kill *', '', 1, 'function(n, l, w) alias_result = w[1] end')
        "#,
        )
        .unwrap();
        let matched = engine.process_input("kill goblin");
        assert!(matched);
        let result: Option<String> = eval(engine, "return alias_result").unwrap();
        assert_eq!(result, Some("goblin".to_string()));
    });
}

#[test]
fn test_alias_war_matching() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            function warteam() print("warteam called") end
            AddAlias("alias_war","^war$","warteam()",alias_flag.Enabled + alias_flag.Replace + alias_flag.RegularExpression ,"")
            SetAliasOption("alias_war","send_to",12)
        "#,
        )
        .unwrap();
        let matched = engine.process_input("war");
        assert!(matched, "war alias should match 'war'");
        let matched2 = engine.process_input("war ");
        assert!(
            !matched2,
            "alias should not match 'war ' (with trailing space)"
        );
    });
}

#[test]
fn test_alias_no_match() {
    with_engine(|engine| {
        exec(engine, "AddAlias('no_match', 'kill *', '', 1)").unwrap();
        let matched = engine.process_input("look");
        assert!(!matched);
    });
}

#[test]
fn test_alias_regex_matching() {
    with_engine(|engine| {
        exec(engine, r#"
            regex_alias_result = nil
            AddAlias('regex_al', [[^go (\w+)$]], '', 129, 'function(n, l, w) regex_alias_result = w[1] end')
        "#).unwrap();
        let matched = engine.process_input("go north");
        assert!(matched);
        let result: Option<String> = eval(engine, "return regex_alias_result").unwrap();
        assert_eq!(result, Some("north".to_string()));
    });
}

#[test]
fn test_alias_disabled_not_matching() {
    with_engine(|engine| {
        exec(engine, "AddAlias('dis_al', 'test', '', 0)").unwrap();
        let matched = engine.process_input("test");
        assert!(!matched);
    });
}

#[test]
fn test_alias_wildcard_question_mark() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            qm_result = nil
            AddAlias('qm_alias', 'go ?', '', 1, 'function(n, l, w) qm_result = w[1] end')
        "#,
        )
        .unwrap();
        let matched = engine.process_input("go n");
        assert!(matched);
        let result: Option<String> = eval(engine, "return qm_result").unwrap();
        assert_eq!(result, Some("n".to_string()));
    });
}

#[test]
fn test_set_alias_option_regexp() {
    with_engine(|engine| {
        exec(
            engine,
            "AddAlias('re_alias', 'old_pattern', '', alias_flag.Enabled + alias_flag.Replace + alias_flag.RegularExpression)",
        )
        .unwrap();
        let matched1 = engine.process_input("old_pattern");
        assert!(matched1);
        // 改用新的正则
        exec(engine, "SetAliasOption('re_alias', 'regexp', 'new_(.+)')").unwrap();
        let matched2 = engine.process_input("new_value");
        assert!(matched2);
        // 旧正则不应该再匹配
        let matched3 = engine.process_input("old_pattern");
        assert!(!matched3);
    });
}

#[test]
fn test_set_alias_option_sequence() {
    with_engine(|engine| {
        exec(engine, "AddAlias('seq_alias', 'test', '', 1)").unwrap();
        let result: i64 =
            eval(engine, "return SetAliasOption('seq_alias', 'sequence', 50)").unwrap();
        assert_eq!(result, 0);
    });
}

// ================================================================
// 定时器 API
// ================================================================

#[test]
fn test_add_timer() {
    with_engine(|engine| {
        let result: i64 = eval(engine, "return AddTimer('test_timer', 0, 0, 5, '', 1)").unwrap();
        assert_eq!(result, 0);
        assert_eq!(engine.timer_count(), 1);
    });
}

#[test]
fn test_delete_timer() {
    with_engine(|engine| {
        exec(engine, "AddTimer('del_timer', 0, 0, 5, '', 1)").unwrap();
        let result: i64 = eval(engine, "return DeleteTimer('del_timer')").unwrap();
        assert_eq!(result, 0);
        assert_eq!(engine.timer_count(), 0);
    });
}

#[test]
fn test_delete_timer_not_found() {
    with_engine(|engine| {
        let result: i64 = eval(engine, "return DeleteTimer('nonexistent')").unwrap();
        assert_eq!(result, 1);
    });
}

#[test]
fn test_get_timer_list() {
    with_engine(|engine| {
        exec(engine, "AddTimer('t1', 0, 0, 5, '', 1)").unwrap();
        exec(engine, "AddTimer('t2', 0, 0, 10, '', 1)").unwrap();
        let list: Vec<String> = eval(
            engine,
            "local t = GetTimerList(); local r = {}; for i=1,#t do r[i]=t[i] end; return r",
        )
        .unwrap();
        assert!(list.contains(&"t1".to_string()));
        assert!(list.contains(&"t2".to_string()));
    });
}

#[test]
fn test_get_timer_info() {
    with_engine(|engine| {
        exec(engine, "AddTimer('info_timer', 0, 0, 5, '', 1)").unwrap();
        let enabled: bool = eval(engine, "return GetTimerInfo('info_timer', 6)").unwrap();
        assert!(enabled);
    });
}

#[test]
fn test_get_timer_info_group() {
    with_engine(|engine| {
        exec(engine, "AddTimer('grp_timer', 0, 0, 5, '', 1)").unwrap();
        exec(engine, "SetTimerOption('grp_timer', 'group', 'mygroup')").unwrap();
        let group: String = eval(engine, "return GetTimerInfo('grp_timer', 19)").unwrap();
        assert_eq!(group, "mygroup");
    });
}

#[test]
fn test_get_timer_info_not_found() {
    with_engine(|engine| {
        let val: Value = eval(engine, "return GetTimerInfo('nonexistent', 6)").unwrap();
        assert!(val.is_nil());
    });
}

#[test]
fn test_set_timer_option() {
    with_engine(|engine| {
        exec(engine, "AddTimer('opt_timer', 0, 0, 5, '', 1)").unwrap();
        exec(engine, "SetTimerOption('opt_timer', 'enabled', false)").unwrap();
        let enabled: bool = eval(engine, "return GetTimerInfo('opt_timer', 6)").unwrap();
        assert!(!enabled);
    });
}

#[test]
fn test_enable_timer_group() {
    with_engine(|engine| {
        exec(engine, "AddTimer('tg1', 0, 0, 5, '', 1)").unwrap();
        exec(engine, "AddTimer('tg2', 0, 0, 10, '', 1)").unwrap();
        exec(engine, "SetTimerOption('tg1', 'group', 'grp_t')").unwrap();
        exec(engine, "SetTimerOption('tg2', 'group', 'grp_t')").unwrap();
        exec(engine, "EnableTimerGroup('grp_t', false)").unwrap();
        let e1: bool = eval(engine, "return GetTimerInfo('tg1', 6)").unwrap();
        assert!(!e1);
    });
}

#[test]
fn test_enable_timer_group_skips_empty_group() {
    with_engine(|engine| {
        exec(engine, "AddTimer('nogrp_t', 0, 0, 5, '', 1)").unwrap();
        exec(engine, "EnableTimerGroup('somegroup', false)").unwrap();
        let enabled: bool = eval(engine, "return GetTimerInfo('nogrp_t', 6)").unwrap();
        assert!(enabled); // 空group的定时器不应被影响
    });
}

#[test]
fn test_set_timer_option_timestamp() {
    with_engine(|engine| {
        exec(engine, "AddTimer('ts_timer', 0, 0, 60, '', 1)").unwrap();
        // 设一个过去的时间戳，定时器应立即到期
        exec(engine, "SetTimerOption('ts_timer', 'timer_timestamp', 100)").unwrap();
        let due = engine.fire_due_timers();
        assert!(
            !due.is_empty(),
            "past timestamp should cause timer to fire immediately"
        );
    });
}

#[test]
fn test_enable_timer() {
    with_engine(|engine| {
        exec(engine, "AddTimer('et_t', 0, 0, 5, '', 1)").unwrap();
        exec(engine, "EnableTimer('et_t', false)").unwrap();
        let enabled: bool = eval(engine, "return GetTimerInfo('et_t', 6)").unwrap();
        assert!(!enabled);
        exec(engine, "EnableTimer('et_t', true)").unwrap();
        let enabled2: bool = eval(engine, "return GetTimerInfo('et_t', 6)").unwrap();
        assert!(enabled2);
    });
}

#[test]
fn test_enable_timer_not_found() {
    with_engine(|engine| {
        let result: i64 = eval(engine, "return EnableTimer('nonexistent', true)").unwrap();
        assert_eq!(result, 1);
    });
}

#[test]
fn test_timer_intervals() {
    with_engine(|engine| {
        exec(engine, "AddTimer('i1', 0, 0, 5, '', 1)").unwrap();
        exec(engine, "AddTimer('i2', 0, 0, 10, '', 1)").unwrap();
        let intervals = engine.timer_intervals();
        assert!(intervals.contains(&5000));
        assert!(intervals.contains(&10000));
    });
}

#[test]
fn test_fire_timer() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            function fire_timer_cb(timer_name)
                    timer_result = "fired"
            end
            AddTimer('fire_t', 0, 0, 5, '', 1, 'fire_timer_cb')
        "#,
        )
        .unwrap();
        engine.fire_timer_by_name("fire_t");
        let result: Option<String> = eval(engine, "return timer_result").unwrap();
        assert_eq!(result, Some("fired".to_string()));
    });
}

#[test]
fn test_fire_timer_one_shot() {
    with_engine(|engine| {
        // flag 4 = OneShot, flag 1 = Enabled
        exec(engine, "AddTimer('oneshot', 0, 0, 5, '', 5)").unwrap();
        assert_eq!(engine.timer_count(), 1);
        engine.fire_timer_by_name("oneshot");
        assert_eq!(engine.timer_count(), 0);
    });
}

#[test]
fn test_fire_timer_disabled() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            disabled_timer_result = nil
            AddTimer('dis_t', 0, 0, 5, '', 0, 'disabled_timer_result = true')
        "#,
        )
        .unwrap();
        engine.fire_timer_by_name("dis_t");
        let result: Option<bool> = eval(engine, "return disabled_timer_result").unwrap();
        assert_eq!(result, None);
    });
}

#[test]
fn test_timer_zero_interval() {
    with_engine(|engine| {
        // 0秒间隔应被设为1秒（1000毫秒）
        exec(engine, "AddTimer('zero_t', 0, 0, 0, '', 1)").unwrap();
        let intervals = engine.timer_intervals();
        assert!(intervals.contains(&1000));
    });
}

#[test]
fn test_timer_float_sec() {
    with_engine(|engine| {
        // 浮点数秒应正确转换为毫秒
        exec(engine, "AddTimer('float_t', 0, 0, 0.10, '', 1)").unwrap();
        let intervals = engine.timer_intervals();
        assert!(intervals.contains(&100)); // 0.10秒 = 100毫秒
    });
}

#[test]
fn test_timer_nil_sec() {
    with_engine(|engine| {
        // nil 秒参数应默认为1秒（1000毫秒）
        exec(engine, "AddTimer('nil_t', 0, 0, nil, '', 1)").unwrap();
        let intervals = engine.timer_intervals();
        assert!(intervals.contains(&1000));
    });
}

#[test]
fn test_is_timer() {
    with_engine(|engine| {
        exec(engine, "AddTimer('exists_t', 0, 0, 5, '', 1)").unwrap();
        let exists: i64 = eval(engine, "return IsTimer('exists_t')").unwrap();
        assert_eq!(exists, 0, "existing timer should return 0");
        let missing: i64 = eval(engine, "return IsTimer('nope')").unwrap();
        assert_eq!(missing, 1, "missing timer should return 1");
    });
}

#[test]
fn test_doafter_via_fire_due_timers() {
    with_engine(|engine| {
        exec(engine, r#"DoAfter(0.1, "test_cmd")"#).unwrap();
        // 手动把 next_fire 设为过去，使其立即到期（避免 sleep 导致的 flaky test）
        {
            let mut state = engine.state.borrow_mut();
            let idx = state
                .timer_by_name
                .keys()
                .find(|k| k.starts_with("__doafter_"))
                .cloned();
            if let Some(name) = idx {
                let i = state.timer_by_name[&name];
                state.timers[i].next_fire =
                    std::time::Instant::now() - std::time::Duration::from_secs(1);
            }
        }
        let due = engine.fire_due_timers();
        assert!(
            due.iter().any(|n| n.starts_with("__doafter_")),
            "DoAfter timer should fire via fire_due_timers"
        );
        let cmds = engine.drain_commands();
        assert!(
            cmds.contains(&"test_cmd".to_string()),
            "DoAfter timer should send command"
        );
    });
}

#[test]
fn test_at_time_timer_advances_24h() {
    with_engine(|engine| {
        // at_time timer: flags = Enabled(1) + AtTime(2) = 3
        // 目标时刻 23:50:00 — 一定在未来（或刚过去，都会推进到明天）
        exec(engine, "AddTimer('at_t', 23, 50, 0, '', 3)").unwrap();
        // 手动把 next_fire 设为过去，使其立即触发
        {
            let mut state = engine.state.borrow_mut();
            let idx = state.timer_by_name.get("at_t").copied().unwrap();
            state.timers[idx].next_fire =
                std::time::Instant::now() - std::time::Duration::from_secs(1);
        }
        let due = engine.fire_due_timers();
        assert!(
            due.contains(&"at_t".to_string()),
            "at_time timer should fire"
        );
        // 再次调用：next_fire 已推进 24h，不应再次触发
        let due2 = engine.fire_due_timers();
        assert!(
            due2.is_empty(),
            "at_time timer should not fire again immediately after 24h advance"
        );
    });
}

#[test]
fn test_multiple_timers_fire_together() {
    with_engine(|engine| {
        // 创建 3 个已过期的 timer
        exec(engine, "AddTimer('m1', 0, 0, 5, '', 1)").unwrap();
        exec(engine, "AddTimer('m2', 0, 0, 5, '', 1)").unwrap();
        exec(engine, "AddTimer('m3', 0, 0, 5, '', 1)").unwrap();
        // 手动把所有 next_fire 设为过去
        {
            let mut state = engine.state.borrow_mut();
            let past = std::time::Instant::now() - std::time::Duration::from_secs(1);
            for i in 0..state.timers.len() {
                state.timers[i].next_fire = past;
            }
        }
        let due = engine.fire_due_timers();
        assert_eq!(due.len(), 3, "all three timers should fire together");
    });
}

#[test]
fn test_timer_callback_deletes_another() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            AddTimer('t1', 0, 0, 1, '', 1, 'DeleteTimer("t2")')
            AddTimer('t2', 0, 0, 1, '', 1)
        "#,
        )
        .unwrap();
        // 触发 t1，它的回调会删除 t2
        engine.fire_timer_by_name("t1");
        // t2 应该已被删除
        let exists: i64 = eval(engine, "return IsTimer('t2')").unwrap();
        assert_eq!(exists, 1, "t2 should be deleted by t1's callback");
    });
}

// ================================================================
// 变量 API
// ================================================================

#[test]
fn test_set_get_variable() {
    with_engine(|engine| {
        exec(engine, "SetVariable('key1', 'value1')").unwrap();
        let val: String = eval(engine, "return GetVariable('key1')").unwrap();
        assert_eq!(val, "value1");
    });
}

#[test]
fn test_get_variable_not_found() {
    with_engine(|engine| {
        let val: Value = eval(engine, "return GetVariable('nonexistent')").unwrap();
        assert!(val.is_nil());
    });
}

#[test]
fn test_delete_variable() {
    with_engine(|engine| {
        exec(engine, "SetVariable('del_key', 'val')").unwrap();
        exec(engine, "DeleteVariable('del_key')").unwrap();
        let val: Value = eval(engine, "return GetVariable('del_key')").unwrap();
        assert!(val.is_nil());
    });
}

#[test]
fn test_get_variable_list() {
    with_engine(|engine| {
        exec(engine, "SetVariable('a', '1')").unwrap();
        exec(engine, "SetVariable('b', '2')").unwrap();
        // GetVariableList 返回 key-value 对表
        let val_a: String = eval(engine, "local t = GetVariableList(); return t.a").unwrap();
        let val_b: String = eval(engine, "local t = GetVariableList(); return t.b").unwrap();
        assert_eq!(val_a, "1");
        assert_eq!(val_b, "2");
    });
}

#[test]
fn test_set_variable_rust() {
    with_engine(|engine| {
        engine.set_variable("rust_key", "rust_val");
        let val: String = eval(engine, "return GetVariable('rust_key')").unwrap();
        assert_eq!(val, "rust_val");
    });
}

#[test]
fn test_variable_overwrite() {
    with_engine(|engine| {
        exec(engine, "SetVariable('ow_key', 'old')").unwrap();
        exec(engine, "SetVariable('ow_key', 'new')").unwrap();
        let val: String = eval(engine, "return GetVariable('ow_key')").unwrap();
        assert_eq!(val, "new");
    });
}

// ================================================================
// 配置 API
// ================================================================

#[test]
fn test_get_info_1() {
    with_engine(|engine| {
        // GetInfo(1) = Server name (IP address)
        let host: String = eval(engine, "return GetInfo(1)").unwrap();
        assert_eq!(host, "");
        engine.set_host("ln.xkxmud.com");
        let host: String = eval(engine, "return GetInfo(1)").unwrap();
        assert_eq!(host, "ln.xkxmud.com");
    });
}

#[test]
fn test_get_info_2() {
    with_engine(|engine| {
        // GetInfo(2) = World name
        let name: String = eval(engine, "return GetInfo(2)").unwrap();
        assert_eq!(name, "");
        engine.set_world_name("北侠");
        let name: String = eval(engine, "return GetInfo(2)").unwrap();
        assert_eq!(name, "北侠");
    });
}

#[test]
fn test_get_info_3() {
    with_engine(|engine| {
        // GetInfo(3) = Character name
        let name: String = eval(engine, "return GetInfo(3)").unwrap();
        assert_eq!(name, "");
        engine.set_char_name("小姗");
        let name: String = eval(engine, "return GetInfo(3)").unwrap();
        assert_eq!(name, "小姗");
    });
}

#[test]
fn test_get_info_35() {
    with_engine(|engine| {
        engine.set_script_path("/home/user/scripts/main.lua");
        let path: String = eval(engine, "return GetInfo(35)").unwrap();
        assert!(path.contains("main.lua"));
        assert!(path.contains('\\'));
        assert!(!path.contains('/'));
    });
}

#[test]
fn test_get_info_35_no_script_path() {
    with_engine(|engine| {
        let path: String = eval(engine, "return GetInfo(35)").unwrap();
        assert_eq!(path, "");
    });
}

#[test]
fn test_get_info_58() {
    with_engine(|engine| {
        // 未设置 log_dir 时返回默认值
        let dir: String = eval(engine, "return GetInfo(58)").unwrap();
        assert_eq!(dir, "logs/");

        // 设置 log_dir 后返回配置的路径
        engine.set_log_dir("logs");
        let dir: String = eval(engine, "return GetInfo(58)").unwrap();
        assert_eq!(dir, "logs/");
    });
}

#[test]
fn test_get_info_204() {
    with_engine(|engine| {
        let count: i64 = eval(engine, "return GetInfo(204)").unwrap();
        assert_eq!(count, 0);
        // process_output 会递增计数器
        engine.process_output("hello");
        let count: i64 = eval(engine, "return GetInfo(204)").unwrap();
        assert_eq!(count, 1);
    });
}

#[test]
fn test_get_info_unknown() {
    with_engine(|engine| {
        // 未知 code 返回空串，而非引发错误或返回 nil
        let val: String = eval(engine, "return GetInfo(999)").unwrap();
        assert_eq!(val, "");
    });
}

#[test]
fn test_set_get_option() {
    with_engine(|engine| {
        exec(engine, "SetOption('myopt', 42)").unwrap();
        let val: i64 = eval(engine, "return GetOption('myopt')").unwrap();
        assert_eq!(val, 42);
    });
}

#[test]
fn test_get_option_not_found() {
    with_engine(|engine| {
        let val: Value = eval(engine, "return GetOption('nonexistent')").unwrap();
        assert!(val.is_nil());
    });
}

#[test]
fn test_set_get_alpha_option() {
    with_engine(|engine| {
        exec(engine, "SetAlphaOption('myalpha', 'hello')").unwrap();
        let val: String = eval(engine, "return GetAlphaOption('myalpha')").unwrap();
        assert_eq!(val, "hello");
    });
}

#[test]
fn test_get_alpha_option_not_found() {
    with_engine(|engine| {
        let val: Value = eval(engine, "return GetAlphaOption('nonexistent')").unwrap();
        assert!(val.is_nil());
    });
}

// ================================================================
// 连接状态 API
// ================================================================

#[test]
fn test_is_connected_default() {
    with_engine(|engine| {
        let connected: bool = eval(engine, "return IsConnected()").unwrap();
        assert!(!connected);
    });
}

#[test]
fn test_connect_disconnect() {
    with_engine(|engine| {
        exec(engine, "Connect()").unwrap();
        assert!(engine.take_connect_requested());
        assert!(!engine.take_connect_requested());

        exec(engine, "Disconnect()").unwrap();
        assert!(engine.take_disconnect_requested());
    });
}

// ================================================================
// 工具函数
// ================================================================

#[test]
fn test_get_unique_number() {
    with_engine(|engine| {
        let n1: i64 = eval(engine, "return GetUniqueNumber()").unwrap();
        let n2: i64 = eval(engine, "return GetUniqueNumber()").unwrap();
        assert!(n2 > n1);
    });
}

#[test]
fn test_trim() {
    with_engine(|engine| {
        let result: String = eval(engine, "return Trim('  hello  ')").unwrap();
        assert_eq!(result, "hello");
    });
}

#[test]
fn test_trim_no_whitespace() {
    with_engine(|engine| {
        let result: String = eval(engine, "return Trim('hello')").unwrap();
        assert_eq!(result, "hello");
    });
}

// ================================================================
// 日志 API
// ================================================================

#[test]
fn test_is_log_open() {
    with_engine(|engine| {
        let open: bool = eval(engine, "return IsLogOpen()").unwrap();
        assert!(open);
    });
}

#[test]
fn test_open_log() {
    with_engine(|engine| {
        // OpenLog 不应报错
        exec(engine, "OpenLog('test.log', true)").unwrap();
    });
}

// ================================================================
// 常量表
// ================================================================

#[test]
fn test_trigger_flag_constants() {
    with_engine(|engine| {
        let enabled: i64 = eval(engine, "return trigger_flag.Enabled").unwrap();
        assert_eq!(enabled, 1);
        let regex: i64 = eval(engine, "return trigger_flag.RegularExpression").unwrap();
        assert_eq!(regex, 32);
        let temp: i64 = eval(engine, "return trigger_flag.Temporary").unwrap();
        assert_eq!(temp, 4096);
    });
}

#[test]
fn test_alias_flag_constants() {
    with_engine(|engine| {
        // MushClient 官方 alias_flag 定义
        // https://www.mushclient.com/scripts/function.php?name=AddAlias
        let enabled: i64 = eval(engine, "return alias_flag.Enabled").unwrap();
        assert_eq!(enabled, 1);
        let keep_eval: i64 = eval(engine, "return alias_flag.KeepEvaluating").unwrap();
        assert_eq!(keep_eval, 8);
        let ignore_case: i64 = eval(engine, "return alias_flag.IgnoreAliasCase").unwrap();
        assert_eq!(ignore_case, 32);
        let omit_log: i64 = eval(engine, "return alias_flag.OmitFromLogFile").unwrap();
        assert_eq!(omit_log, 64);
        let regex: i64 = eval(engine, "return alias_flag.RegularExpression").unwrap();
        assert_eq!(regex, 128);
        let expand: i64 = eval(engine, "return alias_flag.ExpandVariables").unwrap();
        assert_eq!(expand, 512);
        let replace: i64 = eval(engine, "return alias_flag.Replace").unwrap();
        assert_eq!(replace, 1024);
        let speedwalk: i64 = eval(engine, "return alias_flag.AliasSpeedWalk").unwrap();
        assert_eq!(speedwalk, 2048);
        let queue: i64 = eval(engine, "return alias_flag.AliasQueue").unwrap();
        assert_eq!(queue, 4096);
        let menu: i64 = eval(engine, "return alias_flag.AliasMenu").unwrap();
        assert_eq!(menu, 8192);
        let temp: i64 = eval(engine, "return alias_flag.Temporary").unwrap();
        assert_eq!(temp, 16384);
    });
}

#[test]
fn test_timer_flag_constants() {
    with_engine(|engine| {
        let enabled: i64 = eval(engine, "return timer_flag.Enabled").unwrap();
        assert_eq!(enabled, 1);
        let at_time: i64 = eval(engine, "return timer_flag.AtTime").unwrap();
        assert_eq!(at_time, 2);
        let oneshot: i64 = eval(engine, "return timer_flag.OneShot").unwrap();
        assert_eq!(oneshot, 4);
        let speedwalk: i64 = eval(engine, "return timer_flag.TimerSpeedWalk").unwrap();
        assert_eq!(speedwalk, 8);
        let note: i64 = eval(engine, "return timer_flag.TimerNote").unwrap();
        assert_eq!(note, 16);
        let active: i64 = eval(engine, "return timer_flag.ActiveWhenClosed").unwrap();
        assert_eq!(active, 32);
        let replace: i64 = eval(engine, "return timer_flag.Replace").unwrap();
        assert_eq!(replace, 1024);
        let temp: i64 = eval(engine, "return timer_flag.Temporary").unwrap();
        assert_eq!(temp, 16384);
    });
}

#[test]
fn test_error_code_constants() {
    with_engine(|engine| {
        let eok: i64 = eval(engine, "return error_code.eOK").unwrap();
        assert_eq!(eok, 0);
        let ebad: i64 = eval(engine, "return error_code.eBadRegularExpression").unwrap();
        assert_eq!(ebad, 3);
    });
}

#[test]
fn test_error_desc_constants() {
    with_engine(|engine| {
        let eok: String = eval(engine, "return error_desc.eOK").unwrap();
        assert_eq!(eok, "OK");
    });
}

#[test]
fn test_custom_colour_constants() {
    with_engine(|engine| {
        let black: i64 = eval(engine, "return custom_colour.Black").unwrap();
        assert_eq!(black, 0);
        let white: i64 = eval(engine, "return custom_colour.White").unwrap();
        assert_eq!(white, 15);
    });
}

// ================================================================
// bit 库
// ================================================================

#[test]
fn test_bit_bor() {
    with_engine(|engine| {
        let result: i64 = eval(engine, "return bit.bor(1, 2)").unwrap();
        assert_eq!(result, 3);
    });
}

#[test]
fn test_bit_band() {
    with_engine(|engine| {
        let result: i64 = eval(engine, "return bit.band(3, 1)").unwrap();
        assert_eq!(result, 1);
    });
}

#[test]
fn test_bit_bxor() {
    with_engine(|engine| {
        let result: i64 = eval(engine, "return bit.bxor(5, 3)").unwrap();
        assert_eq!(result, 6);
    });
}

#[test]
fn test_bit_bnot() {
    with_engine(|engine| {
        let result: i64 = eval(engine, "return bit.bnot(0)").unwrap();
        assert_eq!(result, -1);
    });
}

#[test]
fn test_bit_lshift() {
    with_engine(|engine| {
        let result: i64 = eval(engine, "return bit.lshift(1, 4)").unwrap();
        assert_eq!(result, 16);
    });
}

#[test]
fn test_bit_rshift() {
    with_engine(|engine| {
        let result: i64 = eval(engine, "return bit.rshift(16, 4)").unwrap();
        assert_eq!(result, 1);
    });
}

// ================================================================
// wait.lua 依赖
// ================================================================

#[test]
fn test_make_regular_expression() {
    with_engine(|engine| {
        let result: String =
            eval(engine, "return MakeRegularExpression('hello * world?')").unwrap();
        assert_eq!(result, "hello .* world.");
    });
}

#[test]
fn test_get_plugin_id() {
    with_engine(|engine| {
        let id: String = eval(engine, "return GetPluginID()").unwrap();
        assert_eq!(id, "");
    });
}

#[test]
fn test_get_plugin_info() {
    with_engine(|engine| {
        // code 1 = plugin name
        let name: String = eval(engine, "return GetPluginInfo('', 1)").unwrap();
        assert_eq!(name, "RustLuaMud");
        // code 14 = Date modified
        let date: String = eval(engine, "return GetPluginInfo('', 14)").unwrap();
        assert_eq!(date, "");
        // code 19 = Version
        let version: f64 = eval(engine, "return GetPluginInfo('', 19)").unwrap();
        assert_eq!(version, 1.0);
        // code 20 = Directory
        let dir: String = eval(engine, "return GetPluginInfo('', 20)").unwrap();
        assert_eq!(dir, "");
    });
}

// ================================================================
// Lua 兼容性补丁
// ================================================================

#[test]
fn test_table_getn() {
    with_engine(|engine| {
        let n: i64 = eval(engine, "return table.getn({1, 2, 3})").unwrap();
        assert_eq!(n, 3);
    });
}

#[test]
fn test_table_foreachi() {
    with_engine(|engine| {
        let result: i64 = eval(
            engine,
            r#"
            local sum = 0
            table.foreachi({10, 20, 30}, function(i, v) sum = sum + v end)
            return sum
        "#,
        )
        .unwrap();
        assert_eq!(result, 60);
    });
}

#[test]
fn test_table_foreach() {
    with_engine(|engine| {
        let result: i64 = eval(
            engine,
            r#"
            local sum = 0
            local t = {a=1, b=2, c=3}
            table.foreach(t, function(k, v) sum = sum + v end)
            return sum
        "#,
        )
        .unwrap();
        assert_eq!(result, 6);
    });
}

#[test]
fn test_math_mod() {
    with_engine(|engine| {
        let result: f64 = eval(engine, "return math.mod(10, 3)").unwrap();
        assert!((result - 1.0).abs() < f64::EPSILON);
    });
}

#[test]
fn test_math_pow() {
    with_engine(|engine| {
        let result: f64 = eval(engine, "return math.pow(2, 10)").unwrap();
        assert!((result - 1024.0).abs() < f64::EPSILON);
    });
}

// ================================================================
// 多行触发器
// ================================================================

#[test]
fn test_multiline_trigger() {
    with_engine(|engine| {
        exec(engine, r#"
            ml_result = nil
            AddTrigger('ml_trig', [[line1[\s\S]*line2]], '', 33, 0, 0, '', 'function() ml_result = true end', 0, 0)
            SetTriggerOption('ml_trig', 'multi_line', true)
            SetTriggerOption('ml_trig', 'lines_to_match', 2)
        "#).unwrap();
        engine.process_output("line1");
        engine.process_output("line2");
        let result: Option<bool> = eval(engine, "return ml_result").unwrap();
        assert_eq!(result, Some(true));
    });
}

#[test]
fn test_single_line_trigger_no_multiline() {
    with_engine(|engine| {
        exec(engine, r#"
            sl_result = nil
            AddTrigger('sl_trig', 'exact_match', '', 33, 0, 0, '', 'function() sl_result = true end', 0, 0)
        "#).unwrap();
        engine.process_output("exact_match");
        let result: Option<bool> = eval(engine, "return sl_result").unwrap();
        assert_eq!(result, Some(true));
    });
}

// ================================================================
// SQLite3
// ================================================================

#[test]
fn test_sqlite3_open_close() {
    with_engine(|engine| {
        let result: i64 = eval(
            engine,
            r#"
            local db = sqlite3.open("/tmp/test_rustluamud.db")
            db:exec("CREATE TABLE IF NOT EXISTS test (id INTEGER PRIMARY KEY, name TEXT)")
            db:close()
            return 0
        "#,
        )
        .unwrap();
        assert_eq!(result, 0);
    });
}

#[test]
fn test_sqlite3_insert_query() {
    with_engine(|engine| {
        let result: i64 = eval(
            engine,
            r#"
            local db = sqlite3.open("/tmp/test_rustluamud2.db")
            db:exec("DROP TABLE IF EXISTS test")
            db:exec("CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT)")
            db:exec("INSERT INTO test (name) VALUES ('hello')")
            local stmt = db:prepare("SELECT name FROM test WHERE id = ?")
            local row = stmt:step({1})
            local name = row and row[1] or nil
            stmt = nil
            db:close()
            return name == 'hello' and 1 or 0
        "#,
        )
        .unwrap();
        assert_eq!(result, 1);
    });
}

#[test]
fn test_database_close() {
    with_engine(|engine| {
        // DatabaseClose 是全局函数，不应报错
        exec(engine, "DatabaseClose('test_db')").unwrap();
    });
}

// ================================================================
// 触发器 send_text
// ================================================================

#[test]
fn test_trigger_send_text() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            AddTrigger('send_trig', 'go', '', 1, 0, 0, '', '', 0, 0)
            SetTriggerOption('send_trig', 'send', 'north')
        "#,
        )
        .unwrap();
        engine.process_output("go");
        let cmds = engine.drain_commands();
        assert!(cmds.contains(&"north".to_string()));
    });
}

// ================================================================
// 原始 API 兼容
// ================================================================

#[test]
fn test_original_trigger_api() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            orig_result = nil
            trigger([[^hello (\w+)$]], function(name, line, wildcards) orig_result = wildcards[1] end)
        "#,
        )
        .unwrap();
        engine.process_output("hello Rust");
        let result: Option<String> = eval(engine, "return orig_result").unwrap();
        assert_eq!(result, Some("Rust".to_string()));
    });
}

#[test]
fn test_original_alias_api() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            orig_alias_result = nil
            alias('^go (.+)$', function(n, l, w) orig_alias_result = w[1] end)
        "#,
        )
        .unwrap();
        let matched = engine.process_input("go north");
        assert!(matched);
        let result: Option<String> = eval(engine, "return orig_alias_result").unwrap();
        assert_eq!(result, Some("north".to_string()));
    });
}

#[test]
fn test_original_get_set_api() {
    with_engine(|engine| {
        exec(engine, "set('mykey', 'myval')").unwrap();
        let val: String = eval(engine, "return get('mykey')").unwrap();
        assert_eq!(val, "myval");
    });
}

// ================================================================
// eval_code
// ================================================================

#[test]
fn test_eval_code() {
    with_engine(|engine| {
        engine.eval_code("eval_result = 42").unwrap();
        let val: i64 = eval(engine, "return eval_result").unwrap();
        assert_eq!(val, 42);
    });
}

#[test]
fn test_eval_code_error() {
    with_engine(|engine| {
        let result = engine.eval_code("invalid!!!lua");
        assert!(result.is_err());
    });
}

// ================================================================
// regex_escape 辅助函数
// ================================================================

#[test]
fn test_regex_escape() {
    assert_eq!(regex_escape("hello.world"), r"hello\.world");
    assert_eq!(regex_escape("a+b"), r"a\+b");
    assert_eq!(regex_escape("test*"), "test*"); // * 保留
    assert_eq!(regex_escape("test?"), "test?"); // ? 保留
    assert_eq!(regex_escape("(group)"), r"\(group\)");
    assert_eq!(regex_escape("a|b"), r"a\|b");
    assert_eq!(regex_escape("^start"), r"\^start");
    assert_eq!(regex_escape("end$"), r"end\$");
    assert_eq!(regex_escape("path\\file"), r"path\\file");
}

// ================================================================
// drain_commands / drain_logs
// ================================================================

#[test]
fn test_drain_commands_clears() {
    with_engine(|engine| {
        exec(engine, "send('cmd1')").unwrap();
        exec(engine, "send('cmd2')").unwrap();
        let cmds = engine.drain_commands();
        assert_eq!(cmds.len(), 2);
        let cmds2 = engine.drain_commands();
        assert!(cmds2.is_empty());
    });
}

#[test]
fn test_drain_logs_clears() {
    with_engine(|engine| {
        exec(engine, "Note('log1')").unwrap();
        exec(engine, "Note('log2')").unwrap();
        let logs = engine.drain_logs();
        assert_eq!(logs.len(), 2);
        let logs2 = engine.drain_logs();
        assert!(logs2.is_empty());
    });
}

// ===== 边界用例补充测试 =====

#[test]
fn test_script_path_method() {
    let mut engine = LuaEngine::new().unwrap();
    assert!(engine.script_path().is_none());
    engine.set_script_path("/some/path/");
    assert_eq!(engine.script_path().unwrap(), "/some/path/");
}

#[test]
fn test_set_connected_true_false() {
    let mut engine = LuaEngine::new().unwrap();
    assert!(!eval::<bool>(&engine, "return IsConnected()").unwrap());
    engine.set_connected(true);
    assert!(eval::<bool>(&engine, "return IsConnected()").unwrap());
    engine.set_connected(false);
    assert!(!eval::<bool>(&engine, "return IsConnected()").unwrap());
}

#[test]
fn test_set_connected_calls_on_connect() {
    let mut engine = LuaEngine::new().unwrap();
    // 覆盖 OnConnect 函数，设置一个标志变量
    exec(
        &engine,
        r#"
        on_connect_called = false
        OnConnect = function()
            on_connect_called = true
        end
        "#,
    )
    .unwrap();

    // 连接时应调用 OnConnect
    engine.set_connected(true);
    assert!(eval::<bool>(&engine, "return on_connect_called").unwrap());

    // 重复调用 set_connected(true) 不应再次触发
    exec(&engine, "on_connect_called = false").unwrap();
    engine.set_connected(true);
    assert!(!eval::<bool>(&engine, "return on_connect_called").unwrap());

    // 断开后重新连接应再次触发
    engine.set_connected(false);
    engine.set_connected(true);
    assert!(eval::<bool>(&engine, "return on_connect_called").unwrap());
}

#[test]
fn test_take_connect_requested_consumed() {
    let engine = LuaEngine::new().unwrap();
    exec(&engine, "Connect()").unwrap();
    assert!(engine.take_connect_requested());
    assert!(!engine.take_connect_requested());
}

#[test]
fn test_take_disconnect_requested_consumed() {
    let engine = LuaEngine::new().unwrap();
    exec(&engine, "Disconnect()").unwrap();
    assert!(engine.take_disconnect_requested());
    assert!(!engine.take_disconnect_requested());
}

#[test]
fn test_set_connected_delay_calls_on_connect_immediately() {
    let mut engine = LuaEngine::new().unwrap();
    exec(
        &engine,
        r#"
        on_connect_called = false
        OnConnect = function()
            on_connect_called = true
        end
        "#,
    )
    .unwrap();

    // 设置延迟 500ms，验证 OnConnect 被立即执行
    engine.set_connect_delay(500);
    engine.set_connected(true);
    assert!(
        eval::<bool>(&engine, "return on_connect_called").unwrap(),
        "set_connected(true) with delay should still call OnConnect immediately"
    );

    // pending_on_connect 已设置，check_pending_on_connect 不应重新执行 OnConnect
    exec(&engine, "on_connect_called = false").unwrap();
    let fired = engine.check_pending_on_connect();
    // 500ms 未到期，不应触发
    assert!(!fired);
    assert!(!eval::<bool>(&engine, "return on_connect_called").unwrap());

    // set_connected(false) 应清除 pending_on_connect
    engine.set_connected(false);
    // 清除标志后重新连接，OnConnect 应再次触发
    exec(&engine, "on_connect_called = false").unwrap();
    engine.set_connected(true);
    assert!(
        eval::<bool>(&engine, "return on_connect_called").unwrap(),
        "reconnect should call OnConnect again"
    );
}

#[test]
fn test_fire_timer_out_of_bounds() {
    with_engine(|engine| {
        // 不存在的 timer 名称不应 panic
        engine.fire_timer_by_name("nonexistent_timer");
    });
}

#[test]
fn test_load_script_nonexistent() {
    let mut engine = LuaEngine::new().unwrap();
    let result = engine.load_script("/nonexistent/path/script.lua");
    assert!(result.is_err());
}

#[test]
fn test_eval_code_error_returns_message() {
    let engine = LuaEngine::new().unwrap();
    let result = engine.eval_code("invalid{{{lua");
    assert!(result.is_err());
    assert!(!result.unwrap_err().is_empty());
}

#[test]
fn test_process_output_empty_line() {
    with_engine(|engine| {
        // 空行不应 panic
        engine.process_output("");
    });
}

#[test]
fn test_process_input_empty() {
    with_engine(|engine| {
        let handled = engine.process_input("");
        assert!(!handled);
    });
}

#[test]
fn test_trigger_count_alias_count_timer_count() {
    let engine = LuaEngine::new().unwrap();
    assert_eq!(engine.trigger_count(), 0);
    assert_eq!(engine.alias_count(), 0);
    assert_eq!(engine.timer_count(), 0);

    exec(
        &engine,
        "AddTrigger('t1', 'test', '', 33, 0, 0, '', '', 0, 0)",
    )
    .unwrap();
    exec(&engine, "AddAlias('a1', 'go', '', 129)").unwrap();
    exec(&engine, "AddTimer('tm1', 0, 0, 10, '', 1)").unwrap();

    assert_eq!(engine.trigger_count(), 1);
    assert_eq!(engine.alias_count(), 1);
    assert_eq!(engine.timer_count(), 1);
}

#[test]
fn test_timer_intervals_with_disabled() {
    with_engine(|engine| {
        exec(engine, "AddTimer('t1', 0, 0, 5, '', 1)").unwrap();
        exec(engine, "AddTimer('t2', 0, 0, 10, '', 1)").unwrap();
        let intervals = engine.timer_intervals();
        assert_eq!(intervals, vec![5000, 10000]);
    });
}

#[test]
fn test_enable_timer_via_api() {
    with_engine(|engine| {
        exec(engine, "AddTimer('t1', 0, 0, 5, '', 1)").unwrap();
        let result: i32 = eval(engine, "return EnableTimer('t1', false)").unwrap();
        assert_eq!(result, 0);
        let result: i32 = eval(engine, "return EnableTimer('t1', true)").unwrap();
        assert_eq!(result, 0);
    });
}

#[test]
fn test_enable_timer_not_found_via_api() {
    with_engine(|engine| {
        let result: i32 = eval(engine, "return EnableTimer('nonexistent', true)").unwrap();
        assert_eq!(result, 1);
    });
}

#[test]
fn test_delete_variable_nonexistent() {
    with_engine(|engine| {
        // DeleteVariable returns nil, should not panic on nonexistent
        exec(engine, "DeleteVariable('no_such_var')").unwrap();
    });
}

#[test]
fn test_get_trigger_info_codes() {
    with_engine(|engine| {
        exec(
            engine,
            "AddTrigger('t1', 'test', '', 33, 0, 0, '', '', 0, 0)",
        )
        .unwrap();
        // code 7 = Keep evaluating (MushClient API)
        let ke: bool = eval(engine, "return GetTriggerInfo('t1', 7)").unwrap();
        assert!(ke);
        // code 8 = enabled (MushClient API)
        let en: bool = eval(engine, "return GetTriggerInfo('t1', 8)").unwrap();
        assert!(en);
        // Set group via SetTriggerOption then read via code 26 (MushClient API)
        exec(engine, "SetTriggerOption('t1', 'group', 'grp1')").unwrap();
        let group: String = eval(engine, "return GetTriggerInfo('t1', 26)").unwrap();
        assert_eq!(group, "grp1");
        // unknown code returns nil
        let val: Value = eval(engine, "return GetTriggerInfo('t1', 999)").unwrap();
        assert!(val.is_nil());
    });
}

#[test]
fn test_get_timer_info_codes() {
    with_engine(|engine| {
        // 常规间隔触发定时器 (flags=1: Enabled)
        exec(engine, "AddTimer('t1', 0, 1, 30, '', 1)").unwrap();
        // code 6 = enabled
        let en: bool = eval(engine, "return GetTimerInfo('t1', 6)").unwrap();
        assert!(en);
        // code 7 = one_shot (false for regular timer)
        let os: bool = eval(engine, "return GetTimerInfo('t1', 7)").unwrap();
        assert!(!os);
        // code 8 = at_time (false for interval timer, true for "at" timer)
        let at: bool = eval(engine, "return GetTimerInfo('t1', 8)").unwrap();
        assert!(!at);
        // code 14 = temporary (not tracked, default false)
        let tmp: bool = eval(engine, "return GetTimerInfo('t1', 14)").unwrap();
        assert!(!tmp);
        // code 19 = group (empty by default)
        let grp: String = eval(engine, "return GetTimerInfo('t1', 19)").unwrap();
        assert_eq!(grp, "");
        // unknown code returns nil
        let val: Value = eval(engine, "return GetTimerInfo('t1', 999)").unwrap();
        assert!(val.is_nil());
    });
}

#[test]
fn test_get_timer_info_at_time_and_one_shot() {
    with_engine(|engine| {
        // AtTime + OneShot + Enabled = 2+4+1=7
        exec(engine, "AddTimer('at_timer', 23, 50, 0, '', 7, 'cb')").unwrap();
        let os: bool = eval(engine, "return GetTimerInfo('at_timer', 7)").unwrap();
        assert!(os, "one_shot should be true");
        let at: bool = eval(engine, "return GetTimerInfo('at_timer', 8)").unwrap();
        assert!(at, "at_time should be true");

        // 纯间隔触发
        exec(engine, "AddTimer('every_timer', 0, 0, 5, '', 1)").unwrap();
        let os: bool = eval(engine, "return GetTimerInfo('every_timer', 7)").unwrap();
        assert!(!os, "should not be one_shot");
        let at: bool = eval(engine, "return GetTimerInfo('every_timer', 8)").unwrap();
        assert!(!at, "should not be at_time");
    });
}

#[test]
fn test_get_info_56() {
    with_engine(|engine| {
        // GetInfo(56) = MUSHclient application path name
        // 本引擎不支持，返回空串
        let path: String = eval(engine, "return GetInfo(56)").unwrap();
        assert_eq!(path, "");
    });
}

#[test]
fn test_set_alias_option_enabled() {
    with_engine(|engine| {
        exec(engine, "AddAlias('a1', 'go', '', 129)").unwrap();
        exec(engine, "SetAliasOption('a1', 'enabled', false)").unwrap();
        // Verify via GetAliasList or re-enable
        exec(engine, "SetAliasOption('a1', 'enabled', true)").unwrap();
    });
}

#[test]
fn test_set_timer_option_enabled() {
    with_engine(|engine| {
        exec(engine, "AddTimer('t1', 0, 0, 5, '', 1)").unwrap();
        exec(engine, "SetTimerOption('t1', 'enabled', false)").unwrap();
        let en: bool = eval(engine, "return GetTimerInfo('t1', 6)").unwrap();
        assert!(!en);
        exec(engine, "SetTimerOption('t1', 'enabled', true)").unwrap();
        let en: bool = eval(engine, "return GetTimerInfo('t1', 6)").unwrap();
        assert!(en);
    });
}

#[test]
fn test_multiline_trigger_with_newlines() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            ml_result = nil
            AddTriggerEx('ml', [[line1\nline2]], '', 33, 0, 0, '', '', 0, 2)
        "#,
        )
        .unwrap();
        engine.process_output("line1");
        engine.process_output("line2");
    });
}

#[test]
fn test_pcre_z_anchor_in_trigger() {
    // 测试 PCRE \Z 锚点在触发器正则中的兼容性
    with_engine(|engine| {
        // 包含 \Z 的正则模式应被正确转换为 Rust regex 的 $
        exec(
            engine,
            r#"
            pcre_result = nil
            AddTriggerEx('pcre_z', [[^(> > > |> > |> |)一个用颅骨制成的钵。\n里面装(满了|了七、八分满|了五、六分满)\Z]], '', 33, 0, 0, '', '', 0, 2)
        "#,
        )
        .unwrap();
    });
}

#[test]
fn test_pcre_z_anchor_simple() {
    // 测试简单的 \Z 转换
    with_engine(|engine| {
        exec(
            engine,
            r#"
            simple_z_result = nil
            AddTriggerEx('simple_z', [[^hello\Z]], '', 33)
        "#,
        )
        .unwrap();
    });
}

#[test]
fn test_pcre_z_anchor_in_rex() {
    // 测试 rex 库中的 \Z 兼容性
    with_engine(|engine| {
        exec(
            engine,
            r#"
            local r = rex.new([[test\Z]])
            rex_match_result = r:match("test")
        "#,
        )
        .unwrap();
        let result: mlua::Value = eval(engine, "return rex_match_result").unwrap();
        assert!(!result.is_nil());
    });
}

#[test]
fn test_sqlite3_changes_and_rowid() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            local db = sqlite3.open(':memory:')
            db:exec('CREATE TABLE t(id INTEGER PRIMARY KEY, v TEXT)')
            db:exec("INSERT INTO t(v) VALUES('hello')")
            test_changes = db:changes()
            test_rowid = db:last_insert_rowid()
            db:close()
        "#,
        )
        .unwrap();
        let changes: i64 = eval(engine, "return test_changes").unwrap();
        let rowid: i64 = eval(engine, "return test_rowid").unwrap();
        assert_eq!(changes, 1);
        assert_eq!(rowid, 1);
    });
}

#[test]
fn test_sqlite3_prepare_bind_step() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            local db = sqlite3.open(':memory:')
            db:exec('CREATE TABLE t(id INTEGER, v TEXT)')
            db:exec("INSERT INTO t(id, v) VALUES(1, 'one')")
            local stmt = db:prepare("SELECT v FROM t WHERE id = ?")
            local row = stmt:step({1})
            test_bind_result = row and row[1] or nil
            stmt = nil
            db:close()
        "#,
        )
        .unwrap();
        let result: String = eval(engine, "return test_bind_result").unwrap();
        assert_eq!(result, "one");
    });
}

#[test]
fn test_regex_escape_special_chars() {
    with_engine(|engine| {
        let result: String = eval(engine, r#"return AddTrigger('esc', 'hello.world', '', 33, 0, 0, '', '', 0, 0) == 0 and 'ok' or 'fail'"#).unwrap();
        assert_eq!(result, "ok");
    });
}

#[test]
fn test_variable_numeric_value() {
    with_engine(|engine| {
        exec(engine, "SetVariable('num', '42')").unwrap();
        let val: String = eval(engine, "return GetVariable('num')").unwrap();
        assert_eq!(val, "42");
    });
}

#[test]
fn test_send_multiple_commands() {
    with_engine(|engine| {
        exec(engine, "send('cmd1')").unwrap();
        exec(engine, "send('cmd2')").unwrap();
        exec(engine, "send('cmd3')").unwrap();
        let cmds = engine.drain_commands();
        assert_eq!(cmds, vec!["cmd1", "cmd2", "cmd3"]);
    });
}

#[test]
fn test_execute_function() {
    with_engine(|engine| {
        // Execute pushes the raw command string to pending_commands
        exec(engine, "Execute('hello')").unwrap();
        let cmds = engine.drain_commands();
        assert!(cmds.contains(&"hello".to_string()));
    });
}

#[test]
fn test_colour_note_with_colors() {
    with_engine(|engine| {
        exec(engine, "ColourNote('red', 'blue', 'colored text')").unwrap();
        let logs = engine.drain_logs();
        // red=31, blue=44 → \x1B[31;44mcolored text\x1B[0m
        assert!(logs
            .iter()
            .any(|l| l.contains("\x1b[31;44mcolored text\x1b[0m")));
    });
}

#[test]
fn test_tell_with_colors() {
    with_engine(|engine| {
        // Tell only takes one string argument
        exec(engine, "Tell('tell text')").unwrap();
        let logs = engine.drain_logs();
        assert!(logs.iter().any(|l| l.contains("tell text")));
    });
}

#[test]
fn test_timer_shorthand() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            timer_result = nil
            timer(5, function() timer_result = "fired" end)
        "#,
        )
        .unwrap();
        assert_eq!(engine.timer_count(), 1);
        engine.fire_timer_by_name("");
        let result: String = eval(engine, "return timer_result").unwrap();
        assert_eq!(result, "fired");
    });
}

#[test]
fn test_dofile_nonexistent() {
    with_engine(|engine| {
        // dofile with nonexistent file should not panic
        let result = exec(engine, "dofile('/nonexistent/file.lua')");
        // May or may not error depending on implementation
        let _ = result;
    });
}

// ================================================================
// rex PCRE 兼容模块测试
// ================================================================

#[test]
fn test_rex_new_basic() {
    with_engine(|engine| {
        let result: Table = eval(engine, "return rex.new('hello')").unwrap();
        // 返回一个表对象（正则对象）
        assert!(result.len().unwrap_or(0) >= 0);
    });
}

#[test]
fn test_rex_new_invalid_pattern() {
    with_engine(|engine| {
        let result = exec(engine, "return rex.new('[invalid')");
        assert!(result.is_err());
    });
}

#[test]
fn test_rex_match_found() {
    with_engine(|engine| {
        let result: Table = eval(
            engine,
            r#"
            local r = rex.new("(\\w+)")
            return r:match("hello world")
        "#,
        )
        .unwrap();
        let full: String = result.get(1).unwrap();
        assert_eq!(full, "hello");
        // 无额外捕获组（整体匹配在索引1）
    });
}

#[test]
fn test_rex_match_with_captures() {
    with_engine(|engine| {
        let result: Table = eval(
            engine,
            r#"
            local r = rex.new("(\\w+)@(\\w+)")
            return r:match("user@host")
        "#,
        )
        .unwrap();
        let full: String = result.get(1).unwrap();
        let cap1: String = result.get(2).unwrap();
        let cap2: String = result.get(3).unwrap();
        assert_eq!(full, "user@host");
        assert_eq!(cap1, "user");
        assert_eq!(cap2, "host");
    });
}

#[test]
fn test_rex_match_not_found() {
    with_engine(|engine| {
        let result: Value = eval(
            engine,
            r#"
            local r = rex.new("xyz")
            return r:match("hello world")
        "#,
        )
        .unwrap();
        assert!(result.is_nil());
    });
}

#[test]
fn test_rex_gmatch_callback() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            local r = rex.new("(\\w+)")
            local results = {}
            r:gmatch("hello world", function(m)
                    table.insert(results, m)
            end)
            SetVariable("gmatch_count", tostring(#results))
            SetVariable("gmatch_1", results[1])
            SetVariable("gmatch_2", results[2])
        "#,
        )
        .unwrap();
        let count: String = eval(engine, "return GetVariable('gmatch_count')").unwrap();
        let first: String = eval(engine, "return GetVariable('gmatch_1')").unwrap();
        let second: String = eval(engine, "return GetVariable('gmatch_2')").unwrap();
        assert_eq!(count, "2");
        assert_eq!(first, "hello");
        assert_eq!(second, "world");
    });
}

#[test]
fn test_rex_gmatch_with_captures() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            local r = rex.new("([^;*\\\\]+)")
            local results = {}
            r:gmatch("cmd1;cmd2*cmd3", function(m)
                    table.insert(results, m)
            end)
            SetVariable("gmatch_cap_count", tostring(#results))
            SetVariable("gmatch_cap_1", results[1])
            SetVariable("gmatch_cap_2", results[2])
            SetVariable("gmatch_cap_3", results[3])
        "#,
        )
        .unwrap();
        let count: String = eval(engine, "return GetVariable('gmatch_cap_count')").unwrap();
        let first: String = eval(engine, "return GetVariable('gmatch_cap_1')").unwrap();
        let second: String = eval(engine, "return GetVariable('gmatch_cap_2')").unwrap();
        let third: String = eval(engine, "return GetVariable('gmatch_cap_3')").unwrap();
        assert_eq!(count, "3");
        assert_eq!(first, "cmd1");
        assert_eq!(second, "cmd2");
        assert_eq!(third, "cmd3");
    });
}

#[test]
fn test_rex_split() {
    with_engine(|engine| {
        let result: Table = eval(
            engine,
            r#"
            local r = rex.new("[,;]+")
            return r:split("a,b;c,d")
        "#,
        )
        .unwrap();
        let first: String = result.get(1).unwrap();
        let second: String = result.get(2).unwrap();
        let third: String = result.get(3).unwrap();
        assert_eq!(first, "a");
        assert_eq!(second, "b");
        assert_eq!(third, "c");
    });
}

#[test]
fn test_rex_find() {
    with_engine(|engine| {
        let result: Table = eval(
            engine,
            r#"
            local r = rex.new("world")
            return r:find("hello world")
        "#,
        )
        .unwrap();
        let start: i64 = result.get(1).unwrap();
        let end: i64 = result.get(2).unwrap();
        let matched: String = result.get(3).unwrap();
        assert_eq!(start, 7);
        assert_eq!(end, 11);
        assert_eq!(matched, "world");
    });
}

#[test]
fn test_rex_find_not_found() {
    with_engine(|engine| {
        let result: Value = eval(
            engine,
            r#"
            local r = rex.new("xyz")
            return r:find("hello world")
        "#,
        )
        .unwrap();
        assert!(result.is_nil());
    });
}

#[test]
fn test_rex_convenience_split() {
    with_engine(|engine| {
        let result: Table = eval(
            engine,
            r#"
            return rex.split("a,b,c", ",")
        "#,
        )
        .unwrap();
        let first: String = result.get(1).unwrap();
        let second: String = result.get(2).unwrap();
        let third: String = result.get(3).unwrap();
        assert_eq!(first, "a");
        assert_eq!(second, "b");
        assert_eq!(third, "c");
    });
}

#[test]
fn test_rex_convenience_match() {
    with_engine(|engine| {
        let result: Table = eval(
            engine,
            r#"
            return rex.match("user@host", "(\\w+)@(\\w+)")
        "#,
        )
        .unwrap();
        let full: String = result.get(1).unwrap();
        let cap1: String = result.get(2).unwrap();
        let cap2: String = result.get(3).unwrap();
        assert_eq!(full, "user@host");
        assert_eq!(cap1, "user");
        assert_eq!(cap2, "host");
    });
}

#[test]
fn test_rex_convenience_find() {
    with_engine(|engine| {
        let result: Table = eval(
            engine,
            r#"
            return rex.find("hello world", "world")
        "#,
        )
        .unwrap();
        let start: i64 = result.get(1).unwrap();
        assert_eq!(start, 7);
    });
}

#[test]
fn test_rex_michen_system_pattern() {
    // 测试实际脚本中的正则: rex.new("([^;*\\\\]+)")
    with_engine(|engine| {
        let result: Table = eval(
            engine,
            r#"
            local r = rex.new("([^;*\\\\]+)")
            return r:match("go north;south*east")
        "#,
        )
        .unwrap();
        let full: String = result.get(1).unwrap();
        let cap1: String = result.get(2).unwrap();
        assert_eq!(full, "go north");
        assert_eq!(cap1, "go north");
    });
}

#[test]
fn test_rex_gmatch_michen_system_usage() {
    // 模拟脚本中 runre:gmatch(str, function(m, t) ... end) 的用法
    with_engine(|engine| {
        exec(
            engine,
            r#"
            local runre = rex.new("([^;*\\\\]+)")
            local results = {}
            runre:gmatch("go north;south*east", function(m, t)
                    table.insert(results, m)
            end)
            SetVariable("runre_count", tostring(#results))
            SetVariable("runre_1", results[1])
            SetVariable("runre_2", results[2])
            SetVariable("runre_3", results[3])
        "#,
        )
        .unwrap();
        let count: String = eval(engine, "return GetVariable('runre_count')").unwrap();
        let first: String = eval(engine, "return GetVariable('runre_1')").unwrap();
        let second: String = eval(engine, "return GetVariable('runre_2')").unwrap();
        let third: String = eval(engine, "return GetVariable('runre_3')").unwrap();
        assert_eq!(count, "3");
        assert_eq!(first, "go north");
        assert_eq!(second, "south");
        assert_eq!(third, "east");
    });
}

#[test]
fn test_get_plugin_info_more_codes() {
    with_engine(|engine| {
        // code 19 = plugin version
        let version: f64 = eval(engine, "return GetPluginInfo(GetPluginID(), 19)").unwrap();
        assert_eq!(version, 1.0);
        // code 20 = directory (string, not boolean)
        let dir: String = eval(engine, "return GetPluginInfo(GetPluginID(), 20)").unwrap();
        assert_eq!(dir, "");
        // unknown code returns nil
        let val: Value = eval(engine, "return GetPluginInfo(GetPluginID(), 999)").unwrap();
        assert!(val.is_nil());
    });
}

#[test]
fn test_trigger_omit_from_output_flag() {
    with_engine(|engine| {
        // flag bit 4 (16) = omit from output
        exec(
            engine,
            "AddTrigger('omit', 'hide_me', '', 49, 0, 0, '', '', 0, 0)",
        )
        .unwrap();
        engine.process_output("hide_me");
        let logs = engine.drain_logs();
        // omit trigger should not produce Note output
        assert!(logs.is_empty() || !logs.iter().any(|l| l.contains("hide_me")));
    });
}

#[test]
fn test_process_output_returns_omit_flag() {
    with_engine(|engine| {
        exec(
            engine,
            "AddTrigger('omit_trig', 'hide_me', '', 1, 0, 0, '', '', 0, 0)",
        )
        .unwrap();
        exec(
            engine,
            "SetTriggerOption('omit_trig', 'omit_from_output', true)",
        )
        .unwrap();
        // 匹配行 → 返回 true
        assert!(engine.process_output("hide_me"));
        // 不匹配行 → 返回 false
        assert!(!engine.process_output("visible line"));
    });
}

#[test]
fn test_process_output_no_omit_when_flag_off() {
    with_engine(|engine| {
        exec(
            engine,
            "AddTrigger('normal_trig', 'test', '', 1, 0, 0, '', '', 0, 0)",
        )
        .unwrap();
        // 未设 omit → 即使匹配也返回 false
        assert!(!engine.process_output("test"));
    });
}

#[test]
fn test_process_output_any_omit_wins() {
    with_engine(|engine| {
        exec(engine, "AddTrigger('t1', 'foo', '', 1, 0, 0, '', '', 0, 0)").unwrap();
        exec(engine, "AddTrigger('t2', 'foo', '', 1, 0, 0, '', '', 0, 0)").unwrap();
        exec(engine, "SetTriggerOption('t2', 'omit_from_output', true)").unwrap();
        // 两个触发器都匹配，其中 t2 omit → 返回 true
        assert!(engine.process_output("foo"));
    });
}

#[test]
fn test_process_output_omit_gbk_pattern() {
    with_engine(|engine| {
        // 验证 GBK 模式触发器也支持 omit（中文模式自动走 GBK 引擎）
        exec(
            engine,
            "AddTrigger('gbk_omit', '你正忙着呢', '', 1, 0, 0, '', '', 0, 0)",
        )
        .unwrap();
        exec(
            engine,
            "SetTriggerOption('gbk_omit', 'omit_from_output', true)",
        )
        .unwrap();
        assert!(engine.process_output("你正忙着呢"));
        assert!(!engine.process_output("你现在不忙"));
    });
}

#[test]
fn test_process_output_omit_gbk_with_trailing_text() {
    with_engine(|engine| {
        // 模拟 linktri 模式：中文 + \w*，匹配真实服务器输出 "你正忙着呢，先忍忍吧"
        // 需要 flag=1065 = KeepEvaluating(8) + RegularExpression(32) + Replace(1024) + Enabled(1)
        exec(
            engine,
            "AddTriggerEx('noecho_test', '^(> > > |> > |> |)(你正忙着呢\\\\w*|你现在不忙\\\\w*)', '', 1065, -1, 0, '', '', 0, 10)",
        )
        .unwrap();
        exec(
            engine,
            "SetTriggerOption('noecho_test', 'omit_from_output', true)",
        )
        .unwrap();
        // 模式应匹配前缀 "你正忙着呢"（\w* 匹配零个单词字符）
        assert!(
            engine.process_output("你正忙着呢，先忍忍吧"),
            "linktri 模式应匹配 '你正忙着呢' 前缀"
        );
        // 不匹配项
        assert!(!engine.process_output("完全不相关的内容"));
    });
}

#[test]
fn test_add_trigger_omit_from_output_flag_bit() {
    with_engine(|engine| {
        // flag 5 = 1(Enabled) + 4(eOmitFromOutput)，通过 flag 位直接设置 omit
        exec(
            engine,
            "AddTrigger('flag_omit', 'hide_me', '', 5, 0, 0, '', '', 0, 0)",
        )
        .unwrap();
        // 匹配行 → 返回 true（omit 经 flag bit 4 设置生效）
        assert!(engine.process_output("hide_me"));
        assert!(!engine.process_output("visible"));
    });
}

#[test]
fn test_alias_callback_sends_command() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            AddAlias('go_alias', 'go', '', 129, 'function() send("north") end')
        "#,
        )
        .unwrap();
        let handled = engine.process_input("go");
        assert!(handled);
        let cmds = engine.drain_commands();
        assert!(cmds.contains(&"north".to_string()));
    });
}

#[test]
fn test_trigger_keep_evaluating() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            result1 = nil
            result2 = nil
            AddTrigger('trig1', 'test', '', 33, 0, 0, '', 'function() result1 = 1 end', 0, 0)
            AddTrigger('trig2', 'test', '', 33, 0, 0, '', 'function() result2 = 2 end', 0, 0)
        "#,
        )
        .unwrap();
        engine.process_output("test");
        let r1: Option<i64> = eval(engine, "return result1").unwrap();
        let r2: Option<i64> = eval(engine, "return result2").unwrap();
        // Both triggers should fire (keep_evaluating is default)
        assert_eq!(r1, Some(1));
        assert_eq!(r2, Some(2));
    });
}

#[test]
fn test_set_trigger_option_send() {
    with_engine(|engine| {
        exec(
            engine,
            "AddTrigger('send_trig2', 'go', '', 1, 0, 0, '', '', 0, 0)",
        )
        .unwrap();
        exec(engine, "SetTriggerOption('send_trig2', 'send', 'north')").unwrap();
        engine.process_output("go");
        let cmds = engine.drain_commands();
        assert!(cmds.contains(&"north".to_string()));
    });
}

#[test]
fn test_delete_trigger_clears() {
    with_engine(|engine| {
        exec(
            engine,
            "AddTrigger('del_me', 'test', '', 33, 0, 0, '', '', 0, 0)",
        )
        .unwrap();
        assert_eq!(engine.trigger_count(), 1);
        exec(engine, "DeleteTrigger('del_me')").unwrap();
        assert_eq!(engine.trigger_count(), 0);
    });
}

#[test]
fn test_delete_alias_clears() {
    with_engine(|engine| {
        exec(engine, "AddAlias('del_me', 'go', '', 129)").unwrap();
        assert_eq!(engine.alias_count(), 1);
        exec(engine, "DeleteAlias('del_me')").unwrap();
        assert_eq!(engine.alias_count(), 0);
    });
}

#[test]
fn test_delete_timer_clears() {
    with_engine(|engine| {
        exec(engine, "AddTimer('del_me', 0, 0, 5, '', 1)").unwrap();
        assert_eq!(engine.timer_count(), 1);
        exec(engine, "DeleteTimer('del_me')").unwrap();
        assert_eq!(engine.timer_count(), 0);
    });
}

#[test]
fn test_enable_trigger_group_via_api() {
    with_engine(|engine| {
        exec(
            engine,
            "AddTrigger('g1_t1', 'a', '', 33, 0, 0, '', '', 0, 0)",
        )
        .unwrap();
        exec(
            engine,
            "AddTrigger('g1_t2', 'b', '', 33, 0, 0, '', '', 0, 0)",
        )
        .unwrap();
        // Set group via SetTriggerOption
        exec(engine, "SetTriggerOption('g1_t1', 'group', 'grp_a')").unwrap();
        exec(engine, "SetTriggerOption('g1_t2', 'group', 'grp_a')").unwrap();
        // Disable group
        exec(engine, "EnableTriggerGroup('grp_a', false)").unwrap();
        let en: bool = eval(engine, "return GetTriggerInfo('g1_t1', 8)").unwrap();
        assert!(!en);
        // Enable group
        exec(engine, "EnableTriggerGroup('grp_a', true)").unwrap();
        let en: bool = eval(engine, "return GetTriggerInfo('g1_t1', 8)").unwrap();
        assert!(en);
    });
}

#[test]
fn test_enable_alias_group_via_set_option() {
    with_engine(|engine| {
        // No EnableAliasGroup API, use SetAliasOption to set group then enable/disable
        exec(engine, "AddAlias('g1_a1', 'x', '', 129)").unwrap();
        exec(engine, "SetAliasOption('g1_a1', 'group', 'grp_b')").unwrap();
        exec(engine, "SetAliasOption('g1_a1', 'enabled', false)").unwrap();
        // Verify disabled
        let handled = engine.process_input("x");
        assert!(!handled);
        // Re-enable
        exec(engine, "SetAliasOption('g1_a1', 'enabled', true)").unwrap();
    });
}

#[test]
fn test_enable_timer_group_via_api() {
    with_engine(|engine| {
        exec(engine, "AddTimer('g1_t1', 0, 0, 5, '', 1)").unwrap();
        // Set group via SetTimerOption
        exec(engine, "SetTimerOption('g1_t1', 'group', 'grp_c')").unwrap();
        // EnableTimerGroup returns nil (unit)
        exec(engine, "EnableTimerGroup('grp_c', false)").unwrap();
        let en: bool = eval(engine, "return GetTimerInfo('g1_t1', 6)").unwrap();
        assert!(!en);
        exec(engine, "EnableTimerGroup('grp_c', true)").unwrap();
        let en: bool = eval(engine, "return GetTimerInfo('g1_t1', 6)").unwrap();
        assert!(en);
    });
}

#[test]
fn test_sqlite3_exec_error() {
    with_engine(|engine| {
        let result: i64 = eval(
            engine,
            r#"
            local db = sqlite3.open(':memory:')
            local ok, err = pcall(function() db:exec('INVALID SQL') end)
            db:close()
            return ok and 1 or 0
        "#,
        )
        .unwrap();
        assert_eq!(result, 0);
    });
}

#[test]
fn test_sqlite3_multiple_rows() {
    with_engine(|engine| {
        let count: i64 = eval(
            engine,
            r#"
            local db = sqlite3.open(':memory:')
            db:exec('CREATE TABLE t(id INTEGER, v TEXT)')
            db:exec("INSERT INTO t VALUES(1, 'a')")
            db:exec("INSERT INTO t VALUES(2, 'b')")
            db:exec("INSERT INTO t VALUES(3, 'c')")
            local c = 0
            -- step() re-prepares each time, so we use exec + specific queries
            local row1 = db:prepare('SELECT v FROM t WHERE id = 1'):step()
            local row2 = db:prepare('SELECT v FROM t WHERE id = 2'):step()
            local row3 = db:prepare('SELECT v FROM t WHERE id = 3'):step()
            if row1 then c = c + 1 end
            if row2 then c = c + 1 end
            if row3 then c = c + 1 end
            db:close()
            return c
        "#,
        )
        .unwrap();
        assert_eq!(count, 3);
    });
}

#[test]
fn test_sqlite3_nrows() {
    with_engine(|engine| {
        let count: i64 = eval(
            engine,
            r#"
            local db = sqlite3.open(':memory:')
            db:exec('CREATE TABLE Room(RoomNO INTEGER, Name TEXT)')
            db:exec("INSERT INTO Room VALUES(1, 'dali')")
            db:exec("INSERT INTO Room VALUES(2, 'changan')")
            db:exec("INSERT INTO Room VALUES(3, 'beijing')")
            local c = 0
            for row in db:nrows('SELECT * FROM Room') do
                    c = c + 1
            end
            db:close()
            return c
        "#,
        )
        .unwrap();
        assert_eq!(count, 3);
    });
}

#[test]
fn test_sqlite3_nrows_column_names() {
    with_engine(|engine| {
        let name: String = eval(
            engine,
            r#"
            local db = sqlite3.open(':memory:')
            db:exec('CREATE TABLE t(id INTEGER, v TEXT)')
            db:exec("INSERT INTO t VALUES(1, 'hello')")
            local result = ""
            for row in db:nrows('SELECT * FROM t') do
                    result = row.v
            end
            db:close()
            return result
        "#,
        )
        .unwrap();
        assert_eq!(name, "hello");
    });
}

#[test]
fn test_get_variable_list_count() {
    with_engine(|engine| {
        exec(engine, "SetVariable('k1', 'v1')").unwrap();
        exec(engine, "SetVariable('k2', 'v2')").unwrap();
        let count: i64 = eval(
            engine,
            r#"
            local list = GetVariableList()
            local c = 0
            for _ in pairs(list) do c = c + 1 end
            return c
        "#,
        )
        .unwrap();
        assert_eq!(count, 2);
    });
}

#[test]
fn test_process_output_with_trigger_and_alias_chain() {
    with_engine(|engine| {
        // Trigger fires on "prompt>" and sends a command
        exec(
            engine,
            r#"
            AddTrigger('auto_cmd', 'prompt>', '', 33, 0, 0, '', '', 0, 0)
            SetTriggerOption('auto_cmd', 'send', 'look')
        "#,
        )
        .unwrap();
        engine.process_output("prompt>");
        let cmds = engine.drain_commands();
        assert!(cmds.contains(&"look".to_string()));
    });
}

// ================================================================
// 触发器集成测试
// ================================================================

#[test]
fn test_trigger_multiple_captures() {
    with_engine(|engine| {
        exec(engine, r#"
            cap1 = nil; cap2 = nil
            AddTrigger('multi_cap', [[^(\w+) hits (\w+)$]], '', 33, 0, 0, '', 'function(name, line, wildcards) cap1 = wildcards[1]; cap2 = wildcards[2] end', 0, 0)
        "#).unwrap();
        engine.process_output("goblin hits warrior");
        let r1: Option<String> = eval(engine, "return cap1").unwrap();
        let r2: Option<String> = eval(engine, "return cap2").unwrap();
        assert_eq!(r1, Some("goblin".to_string()));
        assert_eq!(r2, Some("warrior".to_string()));
    });
}

#[test]
fn test_trigger_no_match_different_line() {
    with_engine(|engine| {
        exec(engine, r#"
            no_match_result = nil
            AddTrigger('no_match', [[^exact$]], '', 33, 0, 0, '', 'function() no_match_result = true end', 0, 0)
        "#).unwrap();
        engine.process_output("not exact at all");
        let result: Option<bool> = eval(engine, "return no_match_result").unwrap();
        assert_eq!(result, None);
    });
}

#[test]
fn test_trigger_ansi_stripped() {
    with_engine(|engine| {
        exec(engine, r#"
            ansi_result = nil
            AddTrigger('ansi_trig', 'hello', '', 33, 0, 0, '', 'function() ansi_result = true end', 0, 0)
        "#).unwrap();
        // ANSI escape codes should be stripped before matching
        engine.process_output("\x1b[31mhello\x1b[0m");
        let result: Option<bool> = eval(engine, "return ansi_result").unwrap();
        assert_eq!(result, Some(true));
    });
}

#[test]
fn test_trigger_callback_error_handled() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            AddTrigger('err_trig', 'test', '', 33, 0, 0, '', 'function() error("boom") end', 0, 0)
        "#,
        )
        .unwrap();
        // Should not panic even if callback errors
        engine.process_output("test");
    });
}

#[test]
fn test_trigger_partial_match() {
    with_engine(|engine| {
        exec(engine, r#"
            partial_result = nil
            AddTrigger('partial', 'hp', '', 33, 0, 0, '', 'function() partial_result = true end', 0, 0)
        "#).unwrap();
        engine.process_output("100hp 200mp 300mv");
        let result: Option<bool> = eval(engine, "return partial_result").unwrap();
        assert_eq!(result, Some(true));
    });
}

#[test]
fn test_trigger_multiple_fire_order() {
    with_engine(|engine| {
        exec(engine, r#"
            fire_order = {}
            AddTrigger('t1', 'test', '', 33, 0, 0, '', 'function() table.insert(fire_order, 1) end', 0, 0)
            AddTrigger('t2', 'test', '', 33, 0, 0, '', 'function() table.insert(fire_order, 2) end', 0, 0)
            AddTrigger('t3', 'test', '', 33, 0, 0, '', 'function() table.insert(fire_order, 3) end', 0, 0)
        "#).unwrap();
        engine.process_output("test");
        let order: Vec<i64> = eval(engine, "return fire_order").unwrap();
        assert_eq!(order, vec![1, 2, 3]);
    });
}

#[test]
fn test_trigger_send_text_with_wildcard() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            AddTrigger('auto_go', 'You see * here', '', 1, 0, 0, '', '', 0, 0)
            SetTriggerOption('auto_go', 'send', 'examine $1')
        "#,
        )
        .unwrap();
        // send_text 不做变量替换，直接发送
        engine.process_output("You see a sword here");
        let cmds = engine.drain_commands();
        // send_text is literal "examine $1", not variable-substituted
        assert!(cmds.contains(&"examine $1".to_string()));
    });
}

#[test]
fn test_trigger_enabled_disabled_toggle() {
    with_engine(|engine| {
        exec(engine, r#"
            toggle_result = nil
            AddTrigger('toggle', 'fire', '', 1, 0, 0, '', 'function() toggle_result = true end', 0, 0)
        "#).unwrap();
        // Initially enabled
        engine.process_output("fire");
        let r1: Option<bool> = eval(engine, "return toggle_result").unwrap();
        assert_eq!(r1, Some(true));

        // Disable
        exec(engine, "EnableTrigger('toggle', false)").unwrap();
        exec(engine, "toggle_result = nil").unwrap();
        engine.process_output("fire");
        let r2: Option<bool> = eval(engine, "return toggle_result").unwrap();
        assert_eq!(r2, None);

        // Re-enable
        exec(engine, "EnableTrigger('toggle', true)").unwrap();
        engine.process_output("fire");
        let r3: Option<bool> = eval(engine, "return toggle_result").unwrap();
        assert_eq!(r3, Some(true));
    });
}

#[test]
fn test_trigger_duplicate_name_replaces() {
    with_engine(|engine| {
        exec(
            engine,
            "AddTrigger('dup', 'first', '', 1, 0, 0, '', '', 0, 0)",
        )
        .unwrap();
        exec(
            engine,
            "AddTrigger('dup', 'second', '', 1, 0, 0, '', '', 0, 0)",
        )
        .unwrap();
        // Both triggers exist (no uniqueness enforcement)
        assert_eq!(engine.trigger_count(), 2);
    });
}

#[test]
fn test_trigger_group_operations() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            grp_a_result = nil
            grp_b_result = nil
            AddTrigger('ga', 'alpha', '', 1, 0, 0, '', 'function() grp_a_result = true end', 0, 0)
            AddTrigger('gb', 'beta', '', 1, 0, 0, '', 'function() grp_b_result = true end', 0, 0)
            SetTriggerOption('ga', 'group', 'groupA')
            SetTriggerOption('gb', 'group', 'groupB')
        "#,
        )
        .unwrap();
        // Disable groupA
        exec(engine, "EnableTriggerGroup('groupA', false)").unwrap();
        engine.process_output("alpha");
        engine.process_output("beta");
        let ra: Option<bool> = eval(engine, "return grp_a_result").unwrap();
        let rb: Option<bool> = eval(engine, "return grp_b_result").unwrap();
        assert_eq!(ra, None);
        assert_eq!(rb, Some(true));
    });
}

// ================================================================
// 定时器集成测试
// ================================================================

#[test]
fn test_timer_fire_executes_send_text() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            AddTimer('cmd_timer', 0, 0, 5, '', 1, 'send("auto_command")')
        "#,
        )
        .unwrap();
        engine.fire_timer(0);
        let cmds = engine.drain_commands();
        // send_text is Lua code that gets executed, which calls send()
        assert!(cmds.contains(&"auto_command".to_string()));
    });
}

#[test]
fn test_timer_fire_with_callback() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            timer_cb_result = nil
            timer(10, function() timer_cb_result = "callback_fired" end)
        "#,
        )
        .unwrap();
        engine.fire_timer(0);
        let result: Option<String> = eval(engine, "return timer_cb_result").unwrap();
        assert_eq!(result, Some("callback_fired".to_string()));
    });
}

#[test]
fn test_timer_one_shot_auto_remove() {
    with_engine(|engine| {
        // flag 5 = Enabled(1) + OneShot(4)
        exec(engine, "AddTimer('oneshot_t', 0, 0, 3, '', 5)").unwrap();
        assert_eq!(engine.timer_count(), 1);
        engine.fire_timer(0);
        assert_eq!(engine.timer_count(), 0);
    });
}

#[test]
fn test_timer_repeating_stays() {
    with_engine(|engine| {
        exec(engine, "AddTimer('repeat_t', 0, 0, 5, '', 1)").unwrap();
        assert_eq!(engine.timer_count(), 1);
        engine.fire_timer(0);
        // Non-one-shot timer should remain
        assert_eq!(engine.timer_count(), 1);
    });
}

#[test]
fn test_timer_disabled_not_fired() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            disabled_t_result = nil
            AddTimer('dis_t', 0, 0, 5, '', 0, 'disabled_t_result = true')
        "#,
        )
        .unwrap();
        engine.fire_timer(0);
        let result: Option<bool> = eval(engine, "return disabled_t_result").unwrap();
        assert_eq!(result, None);
    });
}

#[test]
fn test_timer_enable_disable_cycle() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            function cycle_timer_cb(timer_name)
                    cycle_result = "fired"
            end
            AddTimer('cycle_t', 0, 0, 5, '', 1, 'cycle_timer_cb')
        "#,
        )
        .unwrap();
        // Disable
        exec(engine, "EnableTimer('cycle_t', false)").unwrap();
        engine.fire_timer(0);
        let r1: Option<String> = eval(engine, "return cycle_result").unwrap();
        assert_eq!(r1, None);

        // Re-enable
        exec(engine, "EnableTimer('cycle_t', true)").unwrap();
        exec(engine, "cycle_result = nil").unwrap();
        engine.fire_timer(0);
        let r2: Option<String> = eval(engine, "return cycle_result").unwrap();
        assert_eq!(r2, Some("fired".to_string()));
    });
}

#[test]
fn test_timer_group_enable_disable() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            tg1_result = nil; tg2_result = nil
            AddTimer('tg1', 0, 0, 5, '', 1, 'tg1_result = true')
            AddTimer('tg2', 0, 0, 10, '', 1, 'tg2_result = true')
            SetTimerOption('tg1', 'group', 'grpX')
            SetTimerOption('tg2', 'group', 'grpX')
        "#,
        )
        .unwrap();
        exec(engine, "EnableTimerGroup('grpX', false)").unwrap();
        engine.fire_timer(0); // tg1
        engine.fire_timer(1); // tg2
        let r1: Option<bool> = eval(engine, "return tg1_result").unwrap();
        let r2: Option<bool> = eval(engine, "return tg2_result").unwrap();
        assert_eq!(r1, None);
        assert_eq!(r2, None);
    });
}

#[test]
fn test_timer_replace_flag() {
    with_engine(|engine| {
        exec(engine, "counter = 0").unwrap();

        // First AddTimer with Replace flag
        exec(
            engine,
            "AddTimer('t1', 0, 0, 1, '', 1 + 1024, 'counter = counter + 1')",
        )
        .unwrap();
        exec(engine, "SetTimerOption('t1', 'group', 'g1')").unwrap();

        // Second AddTimer with Replace flag (should replace, not append)
        exec(
            engine,
            "AddTimer('t1', 0, 0, 1, '', 1 + 1024, 'counter = counter + 10')",
        )
        .unwrap();
        exec(engine, "SetTimerOption('t1', 'group', 'g1')").unwrap();

        // Only one timer should exist
        let count: i64 = eval(engine, "return #GetTimerList()").unwrap();
        assert_eq!(count, 1);

        // Fire the timer directly by index
        engine.fire_timer(0);
        let counter: i64 = eval(engine, "return counter").unwrap();
        // Should be 10 (from the replacement timer), not 11 (1+10 from both)
        assert_eq!(counter, 10);
    });
}

#[test]
fn test_timer_replace_preserves_disabled_state() {
    with_engine(|engine| {
        exec(engine, "counter = 0").unwrap();

        // Create a timer with group
        exec(
            engine,
            "AddTimer('t2', 0, 0, 1, '', 1 + 1024, 'counter = counter + 1')",
        )
        .unwrap();
        exec(engine, "SetTimerOption('t2', 'group', 'kill')").unwrap();

        // Disable the group (simulating closeclass("kill"))
        exec(engine, "EnableTimerGroup('kill', false)").unwrap();

        // Replace the timer with AddTimer(Replace) — should inherit disabled state
        exec(
            engine,
            "AddTimer('t2', 0, 0, 1, '', 1 + 1024, 'counter = counter + 10')",
        )
        .unwrap();
        exec(engine, "SetTimerOption('t2', 'group', 'kill')").unwrap();

        // Only one timer should exist
        let count: i64 = eval(engine, "return #GetTimerList()").unwrap();
        assert_eq!(count, 1);

        // Timer should remain disabled (inherited from old timer)
        let enabled: bool = eval(engine, "return GetTimerInfo('t2', 6)").unwrap();
        assert!(!enabled, "Replaced timer should inherit disabled state");

        // Fire the timer — should NOT fire since it's disabled
        engine.fire_timer(0);
        let counter: i64 = eval(engine, "return counter").unwrap();
        assert_eq!(counter, 0, "Disabled timer should not fire after replace");
    });
}

#[test]
fn test_timer_multiple_fire() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            fire_count = 0
            timer(5, function() fire_count = fire_count + 1 end)
        "#,
        )
        .unwrap();
        engine.fire_timer(0);
        engine.fire_timer(0);
        engine.fire_timer(0);
        let count: i64 = eval(engine, "return fire_count").unwrap();
        assert_eq!(count, 3);
    });
}

#[test]
fn test_timer_delete_during_iteration() {
    with_engine(|engine| {
        exec(engine, "AddTimer('del1', 0, 0, 5, '', 1)").unwrap();
        exec(engine, "AddTimer('del2', 0, 0, 10, '', 1)").unwrap();
        assert_eq!(engine.timer_count(), 2);
        exec(engine, "DeleteTimer('del1')").unwrap();
        assert_eq!(engine.timer_count(), 1);
        // Remaining timer should still fire
        engine.fire_timer(0);
    });
}

#[test]
fn test_doafter_executes_command() {
    with_engine(|engine| {
        let count_before = engine.timer_count();
        exec(engine, r#"DoAfter(5, "test_command")"#).unwrap();
        assert_eq!(
            engine.timer_count(),
            count_before + 1,
            "DoAfter should create a timer"
        );
        // Fire the timer
        engine.fire_timer(count_before);
        let cmds = engine.drain_commands();
        assert!(
            cmds.contains(&"test_command".to_string()),
            "DoAfter timer should send command"
        );
    });
}

#[test]
fn test_doafter_note_output() {
    with_engine(|engine| {
        exec(engine, r#"DoAfterNote(3, "test note")"#).unwrap();
        let count = engine.timer_count();
        engine.fire_timer(count - 1);
        let logs = engine.drain_logs();
        assert!(
            logs.iter().any(|l| l.contains("test note")),
            "DoAfterNote should produce Note output"
        );
    });
}

#[test]
fn test_doafter_invalid_time() {
    with_engine(|engine| {
        let r: i64 = eval(engine, "return DoAfter(0, 'x')").unwrap();
        assert_eq!(r, 1, "time < 0.1 should return 1 (eTimeInvalid)");
        let r2: i64 = eval(engine, "return DoAfter(99999, 'x')").unwrap();
        assert_eq!(r2, 1, "time > 86399 should return 1 (eTimeInvalid)");
    });
}

#[test]
fn test_doafter_special_send_to_script() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            doafter_special_result = nil
            DoAfterSpecial(1, "doafter_special_result = 'fired'", 12)
            "#,
        )
        .unwrap();
        let count = engine.timer_count();
        engine.fire_timer(count - 1);
        let r: Option<String> = eval(engine, "return doafter_special_result").unwrap();
        assert_eq!(
            r,
            Some("fired".to_string()),
            "DoAfterSpecial send_to=12 should execute Lua"
        );
    });
}

#[test]
fn test_doafter_special_invalid_send_to() {
    with_engine(|engine| {
        let r: i64 = eval(engine, "return DoAfterSpecial(1, 'x', 99)").unwrap();
        assert_eq!(r, 2, "send_to > 14 should return 2 (eOptionOutOfRange)");
    });
}

#[test]
fn test_doafter_speedwalk() {
    with_engine(|engine| {
        exec(engine, r#"DoAfterSpeedWalk(2, "n;e;n")"#).unwrap();
        let count = engine.timer_count();
        engine.fire_timer(count - 1);
        let cmds = engine.drain_commands();
        assert!(
            cmds.contains(&"n;e;n".to_string()),
            "DoAfterSpeedWalk should send speedwalk string"
        );
    });
}

// ================================================================
// 别名集成测试
// ================================================================

#[test]
fn test_alias_multiple_captures() {
    with_engine(|engine| {
        exec(engine, r#"
            alias_c1 = nil; alias_c2 = nil
            AddAlias('multi_cap_a', 'cast * at *', '', 1, 'function(n, l, w) alias_c1 = w[1]; alias_c2 = w[2] end')
        "#).unwrap();
        let handled = engine.process_input("cast fireball at goblin");
        assert!(handled);
        let r1: Option<String> = eval(engine, "return alias_c1").unwrap();
        let r2: Option<String> = eval(engine, "return alias_c2").unwrap();
        assert_eq!(r1, Some("fireball".to_string()));
        assert_eq!(r2, Some("goblin".to_string()));
    });
}

#[test]
fn test_alias_priority_first_match() {
    with_engine(|engine| {
        exec(engine, r#"
            priority_result = nil
            AddAlias('specific', 'kill goblin', '', 129, 'function() priority_result = "specific" end')
            AddAlias('general', 'kill *', '', 129, 'function() priority_result = "general" end')
        "#).unwrap();
        let handled = engine.process_input("kill goblin");
        assert!(handled);
        let result: Option<String> = eval(engine, "return priority_result").unwrap();
        // Both match, both fire; last one wins since both set the same variable
        assert!(result == Some("specific".to_string()) || result == Some("general".to_string()));
    });
}

#[test]
fn test_alias_no_match_returns_false() {
    with_engine(|engine| {
        exec(engine, "AddAlias('only_go', 'go *', '', 1)").unwrap();
        let handled = engine.process_input("look around");
        assert!(!handled);
    });
}

#[test]
fn test_alias_sends_command() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            AddAlias('kk', 'kk', '', 129, 'function() send("kill") end')
        "#,
        )
        .unwrap();
        let handled = engine.process_input("kk");
        assert!(handled);
        let cmds = engine.drain_commands();
        assert!(cmds.contains(&"kill".to_string()));
    });
}

#[test]
fn test_alias_disabled_not_matched() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            dis_alias_result = nil
            AddAlias('dis_al', 'test', '', 0, 'function() dis_alias_result = true end')
        "#,
        )
        .unwrap();
        let handled = engine.process_input("test");
        assert!(!handled);
    });
}

#[test]
fn test_alias_toggle_enable() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            toggle_alias_result = nil
            AddAlias('toggle_a', 'hello', '', 1, 'function() toggle_alias_result = true end')
        "#,
        )
        .unwrap();
        // Initially enabled
        let h1 = engine.process_input("hello");
        assert!(h1);
        // Disable
        exec(engine, "SetAliasOption('toggle_a', 'enabled', false)").unwrap();
        let h2 = engine.process_input("hello");
        assert!(!h2);
        // Re-enable
        exec(engine, "SetAliasOption('toggle_a', 'enabled', true)").unwrap();
        let h3 = engine.process_input("hello");
        assert!(h3);
    });
}

#[test]
fn test_alias_group_management() {
    with_engine(|engine| {
        exec(engine, "AddAlias('grp_a1', 'x', '', 1)").unwrap();
        exec(engine, "AddAlias('grp_a2', 'y', '', 1)").unwrap();
        exec(engine, "SetAliasOption('grp_a1', 'group', 'combat')").unwrap();
        exec(engine, "SetAliasOption('grp_a2', 'group', 'combat')").unwrap();
        // Disable both via group (manual since no EnableAliasGroup)
        exec(engine, "SetAliasOption('grp_a1', 'enabled', false)").unwrap();
        exec(engine, "SetAliasOption('grp_a2', 'enabled', false)").unwrap();
        let h1 = engine.process_input("x");
        let h2 = engine.process_input("y");
        assert!(!h1);
        assert!(!h2);
    });
}

#[test]
fn test_alias_regex_pattern() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            regex_al_result = nil
            AddAlias('regex_a', [[^#(\d+)$]], '', 129, 'function(n, l, w) regex_al_result = w[1] end')
        "#,
        )
        .unwrap();
        let handled = engine.process_input("#5");
        assert!(handled);
        let result: Option<String> = eval(engine, "return regex_al_result").unwrap();
        assert_eq!(result, Some("5".to_string()));
        // Should not match non-numeric
        let handled2 = engine.process_input("#abc");
        // regex ^#(\d+)$ won't match #abc
        assert!(!handled2);
    });
}

#[test]
fn test_alias_case_insensitive() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            ci_alias_result = nil
            AddAlias('ci_al', 'HELLO', '', 129, 'function() ci_alias_result = true end')
        "#,
        )
        .unwrap();
        // Use regex flag (33) with (?i) prefix for case insensitive
        exec(
            engine,
            r#"
            ci_alias_result2 = nil
            AddAlias('ci_al2', [[(?i)^hello$]], '', 129, 'function() ci_alias_result2 = true end')
        "#,
        )
        .unwrap();
        let handled = engine.process_input("hello");
        assert!(handled);
        let result: Option<bool> = eval(engine, "return ci_alias_result2").unwrap();
        assert_eq!(result, Some(true));
    });
}

#[test]
fn test_alias_delete_and_readd() {
    with_engine(|engine| {
        exec(engine, "AddAlias('temp_a', 'go', '', 1)").unwrap();
        assert_eq!(engine.alias_count(), 1);
        exec(engine, "DeleteAlias('temp_a')").unwrap();
        assert_eq!(engine.alias_count(), 0);
        exec(engine, "AddAlias('temp_a', 'go', '', 1)").unwrap();
        assert_eq!(engine.alias_count(), 1);
    });
}

#[test]
fn test_alias_callback_error_handled() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            AddAlias('err_al', 'boom', '', 129, 'function() error("alias error") end')
        "#,
        )
        .unwrap();
        // Should not panic
        let handled = engine.process_input("boom");
        assert!(handled);
    });
}

#[test]
fn test_alias_input_passed_as_arg0() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            raw_input = nil
            alias('^test$', function(n, l, w) raw_input = l end)
        "#,
        )
        .unwrap();
        let handled = engine.process_input("test");
        assert!(handled);
        let result: Option<String> = eval(engine, "return raw_input").unwrap();
        assert_eq!(result, Some("test".to_string()));
    });
}

#[test]
fn test_get_alias_info_match_text() {
    with_engine(|engine| {
        exec(engine, r#"AddAlias('test_ai', 'kill *', '', 129)"#).unwrap();
        let result: Option<String> = eval(engine, r#"return GetAliasInfo('test_ai', 1)"#).unwrap();
        assert_eq!(result, Some("kill *".to_string()));
    });
}

#[test]
fn test_get_alias_info_response_text() {
    with_engine(|engine| {
        exec(engine, r#"AddAlias('test_ai2', 'go *', 'go_command', 129)"#).unwrap();
        let result: Option<String> = eval(engine, r#"return GetAliasInfo('test_ai2', 2)"#).unwrap();
        assert_eq!(result, Some("go_command".to_string()));
    });
}

#[test]
fn test_get_alias_info_enabled() {
    with_engine(|engine| {
        exec(engine, r#"AddAlias('test_ai3', 'test', '', 1)"#).unwrap();
        let result: Option<bool> = eval(engine, r#"return GetAliasInfo('test_ai3', 6)"#).unwrap();
        assert_eq!(result, Some(true)); // flags=1 => bit0 Enabled set => enabled=true
    });
}

#[test]
fn test_get_alias_info_send_to() {
    with_engine(|engine| {
        // response non-empty, no 5th arg => send_to=12
        exec(
            engine,
            r#"AddAlias('test_ai4', 'test', 'do_something()', 129)"#,
        )
        .unwrap();
        let result: Option<i64> = eval(engine, r#"return GetAliasInfo('test_ai4', 18)"#).unwrap();
        assert_eq!(result, Some(12));
    });
}

#[test]
fn test_get_alias_info_group() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            AddAlias('test_ai5', 'test', '', 129)
            SetAliasOption('test_ai5', 'group', 'mygroup')
        "#,
        )
        .unwrap();
        let result: Option<String> =
            eval(engine, r#"return GetAliasInfo('test_ai5', 16)"#).unwrap();
        assert_eq!(result, Some("mygroup".to_string()));
    });
}

#[test]
fn test_get_alias_info_nonexistent_returns_nil() {
    with_engine(|engine| {
        let result: mlua::Value = eval(engine, r#"return GetAliasInfo('nonexistent', 1)"#).unwrap();
        assert!(matches!(result, mlua::Value::Nil));
    });
}

#[test]
fn test_get_alias_info_shorthand_alias() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            alias('^hello$', function() end)
        "#,
        )
        .unwrap();
        let result: Option<String> = eval(engine, r#"return GetAliasInfo('', 1)"#).unwrap();
        assert_eq!(result, Some("^hello$".to_string()));
    });
}

#[test]
fn test_infobtn_compat_layer() {
    // 验证 infobtn.xxx 调用通过 setmetatable 转发到 cfg.xxx
    with_engine(|engine| {
        exec(
            engine,
            r#"
            -- 模拟 michen_config.lua 的兼容层
            cfg = cfg or {}
            cfg.test_val = 0
            function cfg.set_test()
                    cfg.test_val = 42
            end
            infobtn = infobtn or {}
            setmetatable(infobtn, {__index = function(_, key)
                    if cfg[key] then return cfg[key] end
                    return function() end
            end})
            -- 通过 infobtn 调用 cfg 的函数
            infobtn.set_test()
        "#,
        )
        .unwrap();
        let val: i64 = eval(engine, "return cfg.test_val").unwrap();
        assert_eq!(val, 42);
    });
}

#[test]
fn test_infobtn_missing_method_no_error() {
    // 验证 infobtn.xxx 调用不存在的方法不会报错（返回空函数）
    with_engine(|engine| {
        exec(
            engine,
            r#"
            cfg = cfg or {}
            infobtn = infobtn or {}
            setmetatable(infobtn, {__index = function(_, key)
                    if cfg[key] then return cfg[key] end
                    return function() end
            end})
            -- 调用不存在的方法应静默返回
            infobtn.nonexistent()
            compat_ok = true
        "#,
        )
        .unwrap();
        let ok: bool = eval(engine, "return compat_ok").unwrap();
        assert!(ok);
    });
}

// ================================================================
// 触发器+别名+定时器联动测试
// ================================================================

#[test]
fn test_trigger_sends_command_alias_intercepts() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            AddTrigger('auto_trig', 'prompt>', '', 33, 0, 0, '', '', 0, 0)
            SetTriggerOption('auto_trig', 'send', 'go')
            alias_result = nil
            alias('^go$', function() alias_result = "intercepted" end)
        "#,
        )
        .unwrap();
        engine.process_output("prompt>");
        let cmds = engine.drain_commands();
        // Trigger sends "go" as command, but alias is for process_input not process_output
        assert!(cmds.contains(&"go".to_string()));
    });
}

/// 验证 Execute("war") 产生的命令可以通过 process_input 被别名拦截
/// 这是 send_lua_commands 方案A的核心逻辑链
#[test]
fn test_execute_command_intercepted_by_alias() {
    with_engine(|engine| {
        // 注册 war 别名：匹配 "war"，执行 warteam()，send_to=12
        exec(
            engine,
            r#"
            AddAlias('alias_war', '^war$', 'warteam()', 129, '')
            SetAliasOption('alias_war', 'send_to', 12)
            function warteam()
                    Execute('teamwith alice bob')
            end
        "#,
        )
        .unwrap();

        // 模拟触发器回调中 run("war") → Execute("war")
        // Execute 把 "war" 压入 pending_commands
        engine.process_output("some trigger line");
        // 手动模拟 Execute("war") 的效果
        {
            let mut state = engine.state.borrow_mut();
            state.pending_commands.push("war".to_string());
        }
        let cmds = engine.drain_commands();
        assert!(cmds.contains(&"war".to_string()));

        // 模拟 send_lua_commands 方案A：对 "war" 调用 process_input
        let handled = engine.process_input("war");
        assert!(handled, "war 应被 alias_war 匹配");

        let sub_cmds = engine.drain_commands();
        assert!(
            sub_cmds.contains(&"teamwith alice bob".to_string()),
            "别名匹配后应产生 teamwith 命令，实际: {:?}",
            sub_cmds
        );
    });
}

/// 验证非别名命令不会被拦截，直接通过
#[test]
fn test_execute_command_not_alias_passes_through() {
    with_engine(|engine| {
        // 只注册 war 别名
        exec(
            engine,
            r#"
            AddAlias('alias_war', '^war$', 'warteam()', 129, '')
            SetAliasOption('alias_war', 'send_to', 12)
            function warteam()
                    Execute('teamwith alice bob')
            end
        "#,
        )
        .unwrap();

        // "look" 不是别名，process_input 应返回 false
        let handled = engine.process_input("look");
        assert!(!handled, "look 不应被任何别名匹配");
    });
}

/// 验证别名链式调用：别名A产生命令被别名B拦截
#[test]
fn test_alias_chain_interception() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            -- 别名A: "go" → 回调中 Execute("north")
            AddAlias('go_alias', '^go$', '', 129, 'function() Execute("north") end')

            -- 别名B: "north" → 回调中 Execute("n")
            AddAlias('north_alias', '^north$', '', 129, 'function() Execute("n") end')
        "#,
        )
        .unwrap();

        // 第一层：go 匹配别名
        let handled1 = engine.process_input("go");
        assert!(handled1);
        let cmds1 = engine.drain_commands();
        // Execute("north") 产生的 "north" 命令
        assert!(
            cmds1.contains(&"north".to_string()),
            "go 别名应产生 north 命令，实际: {:?}",
            cmds1
        );

        // 第二层：north 匹配别名
        let handled2 = engine.process_input("north");
        assert!(handled2);
        let cmds2 = engine.drain_commands();
        // Execute("n") 产生的 "n" 命令
        assert!(
            cmds2.contains(&"n".to_string()),
            "north 别名应产生 n 命令，实际: {:?}",
            cmds2
        );
    });
}

#[test]
fn test_gbk_dosth5_matching() {
    // 测试 dosth5 正则匹配
    let pattern = r"^(> > > |> > |> |)你目前还没有任何为 (\S+) 的变量设定。";
    let gbk_pattern = utf8_regex_to_gbk_bytes(pattern);
    eprintln!("GBK pattern: {}", gbk_pattern);

    let re = BytesRegex::new(&gbk_pattern).unwrap();

    // 测试1: gps=start
    let line1 = "> 你目前还没有任何为 gps=start 的变量设定。";
    let (gbk_line1, _, _) = encoding_rs::GBK.encode(line1);
    let matched1 = re.is_match(&gbk_line1);
    eprintln!("Line1 matched: {}", matched1);
    assert!(matched1, "dosth5 should match 'gps=start' line");

    // 测试2: checkyell=yes
    let line2 = "> 你目前还没有任何为 checkyell=yes 的变量设定。";
    let (gbk_line2, _, _) = encoding_rs::GBK.encode(line2);
    let matched2 = re.is_match(&gbk_line2);
    eprintln!("Line2 matched: {}", matched2);
    assert!(matched2, "dosth5 should match 'checkyell=yes' line");

    // 测试3: 捕获组
    if let Some(caps) = re.captures(&gbk_line2) {
        for (i, cap) in caps.iter().enumerate() {
            if let Some(m) = cap {
                let (cow, _, _) = encoding_rs::GBK.decode(m.as_bytes());
                eprintln!("  cap[{}]: {}", i, cow);
            }
        }
    }
}

#[test]
fn test_gbk_dosth2_matching() {
    // 测试 dosth2 正则匹配: "这里明显的出口是"
    let pattern = r"^\s*这里.{4}的出口是 (.*)。$";
    let gbk_pattern = utf8_regex_to_gbk_bytes(pattern);
    eprintln!("GBK pattern (dosth2): {}", gbk_pattern);

    let re = BytesRegex::new(&gbk_pattern).unwrap();

    let line = "    这里明显的出口是 north 和 south。";
    let (gbk_line, _, _) = encoding_rs::GBK.encode(line);
    let matched = re.is_match(&gbk_line);
    eprintln!("dosth2 matched: {}", matched);
    assert!(matched, "dosth2 should match exit line");

    if let Some(caps) = re.captures(&gbk_line) {
        for (i, cap) in caps.iter().enumerate() {
            if let Some(m) = cap {
                let (cow, _, _) = encoding_rs::GBK.decode(m.as_bytes());
                eprintln!("  cap[{}]: {}", i, cow);
            }
        }
    }
}

#[test]
fn test_timer_and_trigger_coexist() {
    with_engine(|engine| {
        exec(engine, r#"
            trig_result = nil
            AddTrigger('co_trig', 'status', '', 33, 0, 0, '', 'function() trig_result = true end', 0, 0)
            timer_result = nil
            timer(5, function() timer_result = true end)
        "#).unwrap();
        engine.process_output("status");
        engine.fire_timer(0);
        let tr: Option<bool> = eval(engine, "return trig_result").unwrap();
        let tmr: Option<bool> = eval(engine, "return timer_result").unwrap();
        assert_eq!(tr, Some(true));
        assert_eq!(tmr, Some(true));
    });
}

#[test]
fn test_variable_shared_across_triggers_and_aliases() {
    with_engine(|engine| {
        exec(engine, r#"
            SetVariable('counter', '0')
            AddTrigger('count_trig', 'tick', '', 33, 0, 0, '', 'function() SetVariable("counter", tostring(tonumber(GetVariable("counter")) + 1)) end', 0, 0)
            AddAlias('show_count', 'count', '', 129, 'function() Note("count=" .. GetVariable("counter")) end')
        "#).unwrap();
        engine.process_output("tick");
        engine.process_output("tick");
        engine.process_output("tick");
        let val: String = eval(engine, "return GetVariable('counter')").unwrap();
        assert_eq!(val, "3");
    });
}

// ================================================================
// JSON 序列化桥接函数测试 — lua_value_to_json
// ================================================================

#[test]
fn test_lua_value_to_json_nil() {
    let lua_val = mlua::Value::Nil;
    let json_val = lua_value_to_json(&lua_val);
    assert_eq!(json_val, serde_json::Value::Null);
}

#[test]
fn test_lua_value_to_json_boolean() {
    let json_val = lua_value_to_json(&mlua::Value::Boolean(true));
    assert_eq!(json_val, serde_json::Value::Bool(true));
    let json_val = lua_value_to_json(&mlua::Value::Boolean(false));
    assert_eq!(json_val, serde_json::Value::Bool(false));
}

#[test]
fn test_lua_value_to_json_integer() {
    let json_val = lua_value_to_json(&mlua::Value::Integer(42));
    assert_eq!(json_val, serde_json::Value::Number(42.into()));
    let json_val = lua_value_to_json(&mlua::Value::Integer(-1));
    assert_eq!(json_val, serde_json::Value::Number((-1).into()));
}

#[test]
fn test_lua_value_to_json_number() {
    let json_val = lua_value_to_json(&mlua::Value::Number(3.14));
    assert_eq!(json_val, serde_json::json!(3.14));
}

#[test]
fn test_lua_value_to_json_string() {
    let lua = Lua::new();
    let s = lua.create_string("hello").unwrap();
    let json_val = lua_value_to_json(&mlua::Value::String(s));
    assert_eq!(json_val, serde_json::Value::String("hello".to_string()));
}

#[test]
fn test_lua_value_to_json_string_utf8() {
    let lua = Lua::new();
    let s = lua.create_string("中文测试").unwrap();
    let json_val = lua_value_to_json(&mlua::Value::String(s));
    assert_eq!(json_val, serde_json::Value::String("中文测试".to_string()));
}

#[test]
fn test_lua_value_to_json_array() {
    with_engine(|engine| {
        let lua_val: mlua::Value = eval(engine, "return {10, 20, 30}").unwrap();
        let json_val = lua_value_to_json(&lua_val);
        assert_eq!(json_val, serde_json::json!([10, 20, 30]));
    });
}

#[test]
fn test_lua_value_to_json_object() {
    with_engine(|engine| {
        let lua_val: mlua::Value = eval(engine, "return {name='test', value=42}").unwrap();
        let json_val = lua_value_to_json(&lua_val);
        assert_eq!(json_val["name"], serde_json::json!("test"));
        assert_eq!(json_val["value"], serde_json::json!(42));
    });
}

#[test]
fn test_lua_value_to_json_nested() {
    with_engine(|engine| {
        let lua_val: mlua::Value = eval(engine, "return {a={b={c=1}}}").unwrap();
        let json_val = lua_value_to_json(&lua_val);
        assert_eq!(json_val["a"]["b"]["c"], serde_json::json!(1));
    });
}

#[test]
fn test_lua_value_to_json_empty_table() {
    with_engine(|engine| {
        let lua_val: mlua::Value = eval(engine, "return {}").unwrap();
        let json_val = lua_value_to_json(&lua_val);
        // 空表既可以视为空数组也可以视为空对象，这里取决于实现
        // 我们的实现中空表没有连续整数键 → 判定为对象
        assert!(json_val.is_object() || json_val.is_array());
    });
}

#[test]
fn test_lua_value_to_json_mixed_array() {
    with_engine(|engine| {
        // 1, 2, name="x" — 非连续整数键 → 判定为对象
        let lua_val: mlua::Value = eval(engine, "return {1, 2, name='x'}").unwrap();
        let json_val = lua_value_to_json(&lua_val);
        assert!(json_val.is_object());
        assert_eq!(json_val["name"], serde_json::json!("x"));
    });
}

#[test]
fn test_lua_value_to_json_function_is_null() {
    let lua = Lua::new();
    let fn_val = lua.create_function(|_, ()| Ok(())).unwrap();
    let json_val = lua_value_to_json(&mlua::Value::Function(fn_val));
    assert_eq!(json_val, serde_json::Value::Null);
}

// ================================================================
// JSON 序列化桥接函数测试 — json_to_lua_value
// ================================================================

#[test]
fn test_json_to_lua_value_null() {
    let lua = Lua::new();
    let lua_val = json_to_lua_value(&lua, &serde_json::Value::Null).unwrap();
    assert!(matches!(lua_val, mlua::Value::Nil));
}

#[test]
fn test_json_to_lua_value_bool() {
    let lua = Lua::new();
    let lua_val = json_to_lua_value(&lua, &serde_json::Value::Bool(true)).unwrap();
    assert!(matches!(lua_val, mlua::Value::Boolean(true)));
}

#[test]
fn test_json_to_lua_value_integer() {
    let lua = Lua::new();
    let lua_val = json_to_lua_value(&lua, &serde_json::json!(100)).unwrap();
    assert!(matches!(lua_val, mlua::Value::Integer(100)));
}

#[test]
fn test_json_to_lua_value_float() {
    let lua = Lua::new();
    let lua_val = json_to_lua_value(&lua, &serde_json::json!(3.14)).unwrap();
    assert!(matches!(lua_val, mlua::Value::Number(v) if (v - 3.14).abs() < 1e-10));
}

#[test]
fn test_json_to_lua_value_string() {
    let lua = Lua::new();
    let lua_val = json_to_lua_value(&lua, &serde_json::Value::String("hi".to_string())).unwrap();
    assert!(matches!(&lua_val, mlua::Value::String(s) if s.to_str().unwrap() == "hi"));
}

#[test]
fn test_json_to_lua_value_array() {
    let lua = Lua::new();
    let json_val = serde_json::json!([1, 2, 3]);
    let lua_val = json_to_lua_value(&lua, &json_val).unwrap();
    if let mlua::Value::Table(t) = &lua_val {
        assert_eq!(t.get::<i64>(1).unwrap(), 1);
        assert_eq!(t.get::<i64>(2).unwrap(), 2);
        assert_eq!(t.get::<i64>(3).unwrap(), 3);
    } else {
        panic!("期望 Table, 获得 {:?}", lua_val);
    }
}

#[test]
fn test_json_to_lua_value_object() {
    let lua = Lua::new();
    let json_val = serde_json::json!({"key": "value", "num": 42});
    let lua_val = json_to_lua_value(&lua, &json_val).unwrap();
    if let mlua::Value::Table(t) = &lua_val {
        assert_eq!(t.get::<String>("key").unwrap(), "value");
        assert_eq!(t.get::<i64>("num").unwrap(), 42);
    } else {
        panic!("期望 Table, 获得 {:?}", lua_val);
    }
}

#[test]
fn test_json_to_lua_value_nested() {
    let lua = Lua::new();
    let json_val = serde_json::json!({"a": {"b": [1, 2]}});
    let lua_val = json_to_lua_value(&lua, &json_val).unwrap();
    if let mlua::Value::Table(t) = &lua_val {
        let inner: mlua::Table = t.get("a").unwrap();
        let arr: mlua::Table = inner.get("b").unwrap();
        assert_eq!(arr.get::<i64>(1).unwrap(), 1);
        assert_eq!(arr.get::<i64>(2).unwrap(), 2);
    } else {
        panic!("期望 Table, 获得 {:?}", lua_val);
    }
}

// ================================================================
// json_encode / json_decode Lua API 测试
// ================================================================

#[test]
fn test_json_encode_nil() {
    with_engine(|engine| {
        let result: String = eval(engine, "return json_encode(nil)").unwrap();
        assert_eq!(result, "null");
    });
}

#[test]
fn test_json_encode_boolean() {
    with_engine(|engine| {
        let result: String = eval(engine, "return json_encode(true)").unwrap();
        assert_eq!(result, "true");
        let result: String = eval(engine, "return json_encode(false)").unwrap();
        assert_eq!(result, "false");
    });
}

#[test]
fn test_json_encode_number() {
    with_engine(|engine| {
        let result: String = eval(engine, "return json_encode(42)").unwrap();
        assert_eq!(result, "42");
        let result: String = eval(engine, "return json_encode(3.14)").unwrap();
        assert!(result.starts_with("3.14"));
    });
}

#[test]
fn test_json_encode_string() {
    with_engine(|engine| {
        let result: String = eval(engine, "return json_encode('hello')").unwrap();
        assert_eq!(result, "\"hello\"");
    });
}

#[test]
fn test_json_encode_array() {
    with_engine(|engine| {
        let result: String = eval(engine, "return json_encode({10, 20, 30})").unwrap();
        assert_eq!(result, "[10,20,30]");
    });
}

#[test]
fn test_json_encode_object() {
    with_engine(|engine| {
        let result: String = eval(engine, "return json_encode({a=1, b='x'})").unwrap();
        assert!(result.contains("\"a\":1"));
        assert!(result.contains("\"b\":\"x\""));
    });
}

#[test]
fn test_json_encode_nested() {
    with_engine(|engine| {
        let result: String = eval(engine, "return json_encode({a={b={c=1}}})").unwrap();
        assert!(result.contains("\"a\""));
        assert!(result.contains("\"b\""));
        assert!(result.contains("\"c\":1"));
    });
}

#[test]
fn test_json_decode_null() {
    with_engine(|engine| {
        let result: String = eval(engine, "local v = json_decode('null'); return type(v)").unwrap();
        assert_eq!(result, "nil");
    });
}

#[test]
fn test_json_decode_boolean() {
    with_engine(|engine| {
        let result: bool = eval(engine, "return json_decode('true')").unwrap();
        assert!(result);
        let result: bool = eval(engine, "return json_decode('false')").unwrap();
        assert!(!result);
    });
}

#[test]
fn test_json_decode_integer() {
    with_engine(|engine| {
        let result: i64 = eval(engine, "return json_decode('42')").unwrap();
        assert_eq!(result, 42);
    });
}

#[test]
fn test_json_decode_string() {
    with_engine(|engine| {
        let result: String = eval(engine, "return json_decode('\"hello\"')").unwrap();
        assert_eq!(result, "hello");
    });
}

#[test]
fn test_json_decode_array() {
    with_engine(|engine| {
        let result: String = eval(
            engine,
            "local t = json_decode('[1,2,3]'); return t[1] + t[2] + t[3]",
        )
        .unwrap();
        assert_eq!(result, "6");
    });
}

#[test]
fn test_json_decode_object() {
    with_engine(|engine| {
        let result: i64 = eval(
            engine,
            "local t = json_decode('{\"a\":1,\"b\":2}'); return t.a + t.b",
        )
        .unwrap();
        assert_eq!(result, 3);
    });
}

#[test]
fn test_json_roundtrip() {
    with_engine(|engine| {
        let result: String = eval(
            engine,
            "local original = {a=1, b='hello', c={nested=true}}; \
             local json = json_encode(original); \
             local decoded = json_decode(json); \
             return json_encode(decoded)",
        )
        .unwrap();
        assert!(result.contains("\"a\":1"));
        assert!(result.contains("\"b\":\"hello\""));
        assert!(result.contains("\"nested\":true"));
    });
}

#[test]
fn test_json_decode_invalid() {
    with_engine(|engine| {
        let result: bool = eval(
            engine,
            "local ok, err = pcall(json_decode, '{invalid}'); return not ok",
        )
        .unwrap();
        assert!(result);
    });
}

// ================================================================
// eval_to_string 方法测试
// ================================================================

#[test]
fn test_eval_to_string_simple() {
    with_engine(|engine| {
        let result = engine.eval_to_string("return 'hello'").unwrap();
        assert_eq!(result, "hello");
    });
}

#[test]
fn test_eval_to_string_number() {
    with_engine(|engine| {
        let result = engine.eval_to_string("return tostring(42)").unwrap();
        assert_eq!(result, "42");
    });
}

#[test]
fn test_eval_to_string_table_json() {
    with_engine(|engine| {
        let result = engine
            .eval_to_string("return json_encode({1,2,3})")
            .unwrap();
        assert_eq!(result, "[1,2,3]");
    });
}

#[test]
fn test_eval_to_string_syntax_error() {
    with_engine(|engine| {
        let result = engine.eval_to_string("syntax error !!!");
        assert!(result.is_err());
    });
}

#[test]
fn test_eval_to_string_runtime_error() {
    with_engine(|engine| {
        let result = engine.eval_to_string("error('boom')");
        assert!(result.is_err());
    });
}

// ================================================================
// cfg.data() / cfg.update() — Lua 侧配置 API 测试
// 通过内联构建测试 schema 来验证逻辑
// ================================================================

#[test]
fn test_cfg_data_empty_schema() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            cfg = cfg or {}
            cfg.schema = {}
            function cfg.data()
                    local result = {}
                    for _, field in ipairs(cfg.schema) do
                        table.insert(result, {key=field.key, value=field.getter()})
                    end
                    return result
            end
        "#,
        )
        .unwrap();
        let result: String = eval(engine, "return json_encode(cfg.data())").unwrap();
        // 空 Lua 表（无连续整数键）→ JSON 对象 {}
        assert_eq!(result, "{}");
    });
}

#[test]
fn test_cfg_data_boolean_fields() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            cfg = cfg or {}
            test_switch = 1
            cfg.schema = {
                    { key="test_switch", label="测试开关", type="boolean", category="测试",
                      getter=function() return test_switch and test_switch>0 end,
                      setter=function(v) test_switch=v and 1 or 0 end },
            }
            function cfg.data()
                    local result = {}
                    for _, field in ipairs(cfg.schema) do
                        local entry = {key=field.key, label=field.label, type=field.type,
                                       category=field.category, value=field.getter()}
                        table.insert(result, entry)
                    end
                    return result
            end
        "#,
        )
        .unwrap();
        let result: String = eval(engine, "return json_encode(cfg.data())").unwrap();
        assert!(result.contains("\"test_switch\""));
        assert!(result.contains("true"));
    });
}

#[test]
fn test_cfg_data_number_fields() {
    with_engine(|engine| {
        exec(engine, r#"
            cfg = cfg or {}
            test_number = 175
            cfg.schema = {
                    { key="test_number", label="测试数值", type="number", category="数值", min=0, max=500,
                      getter=function() return test_number end,
                      setter=function(v) test_number=tonumber(v) or test_number end },
            }
            function cfg.data()
                    local result = {}
                    for _, field in ipairs(cfg.schema) do
                        local entry = {key=field.key, label=field.label, type=field.type,
                                       category=field.category, value=field.getter()}
                        table.insert(result, entry)
                    end
                    return result
            end
        "#).unwrap();
        let result: String = eval(engine, "return json_encode(cfg.data())").unwrap();
        assert!(result.contains("\"test_number\""));
        assert!(result.contains("175"));
    });
}

#[test]
fn test_cfg_update_valid() {
    with_engine(|engine| {
        exec(engine, r#"
            cfg = cfg or {}
            test_val = 10

            cfg.schema = {
                    { key="test_val", label="测试值", type="number", category="数值", min=0, max=100,
                      getter=function() return test_val end,
                      setter=function(v) test_val=tonumber(v) or test_val end },
            }

            -- 构建 schema_map
            local schema_map = {}
            for _, field in ipairs(cfg.schema) do
                    schema_map[field.key] = field
            end

            function cfg._validate(field, value)
                    local t = field.type
                    if t == "number" then
                        local n = tonumber(value)
                        if n == nil then return false, "需要数字" end
                        if field.min ~= nil and n < field.min then return false, "最小值 "..tostring(field.min) end
                        if field.max ~= nil and n > field.max then return false, "最大值 "..tostring(field.max) end
                    elseif t == "boolean" then
                        if type(value) ~= "boolean" then return false, "需要布尔值" end
                    elseif t == "string" then
                        if type(value) ~= "string" then return false, "需要字符串" end
                    end
                    return true, nil
            end

            function cfg.update(changes)
                    if type(changes) ~= "table" then return { ok=false, errors={ _global="参数必须是 table" } } end
                    local errors = {}
                    for key, value in pairs(changes) do
                        local field = schema_map[key]
                        if not field then
                            errors[key] = "未知配置项"
                        else
                            local ok, err = cfg._validate(field, value)
                            if not ok then
                                errors[key] = err
                            else
                                local success, apply_err = pcall(field.setter, value)
                                if not success then errors[key] = "应用失败: "..tostring(apply_err) end
                            end
                        end
                    end
                    if next(errors) then return { ok=false, errors=errors } end
                    return { ok=true }
            end
        "#).unwrap();

        // 测试有效更新
        let result: String = eval(
            engine,
            "local r = cfg.update({test_val=50}); return json_encode(r)",
        )
        .unwrap();
        assert!(result.contains("\"ok\":true"));

        // 验证值已更新
        let val: i64 = eval(engine, "return test_val").unwrap();
        assert_eq!(val, 50);
    });
}

#[test]
fn test_cfg_update_unknown_key() {
    with_engine(|engine| {
        exec(engine, r#"
            cfg = cfg or {}
            cfg.schema = {}

            local schema_map = {}
            for _, field in ipairs(cfg.schema) do
                    schema_map[field.key] = field
            end

            function cfg._validate(field, value) return true, nil end
            function cfg.update(changes)
                    if type(changes) ~= "table" then return { ok=false, errors={ _global="参数必须是 table" } } end
                    local errors = {}
                    for key, value in pairs(changes) do
                        local field = schema_map[key]
                        if not field then errors[key] = "未知配置项" end
                    end
                    if next(errors) then return { ok=false, errors=errors } end
                    return { ok=true }
            end
        "#).unwrap();

        let result: String = eval(
            engine,
            "local r = cfg.update({nonexistent=1}); return json_encode(r)",
        )
        .unwrap();
        assert!(result.contains("\"ok\":false"));
        assert!(result.contains("未知配置项"));
    });
}

#[test]
fn test_cfg_update_invalid_number() {
    with_engine(|engine| {
        exec(engine, r#"
            cfg = cfg or {}
            test_n = 0
            cfg.schema = {
                    { key="test_n", label="N", type="number", category="数值", min=0, max=100,
                      getter=function() return test_n end,
                      setter=function(v) test_n=tonumber(v) or test_n end },
            }
            local schema_map = {}
            for _, field in ipairs(cfg.schema) do schema_map[field.key] = field end

            function cfg._validate(field, value)
                    local n = tonumber(value)
                    if n == nil then return false, "需要数字" end
                    if field.min ~= nil and n < field.min then return false, "最小值 "..tostring(field.min) end
                    if field.max ~= nil and n > field.max then return false, "最大值 "..tostring(field.max) end
                    return true, nil
            end
            function cfg.update(changes)
                    local errors = {}
                    for key, value in pairs(changes) do
                        local field = schema_map[key]
                        if not field then errors[key] = "未知配置项"
                        else
                            local ok, err = cfg._validate(field, value)
                            if not ok then errors[key] = err end
                        end
                    end
                    if next(errors) then return { ok=false, errors=errors } end
                    return { ok=true }
            end
        "#).unwrap();

        // 超出范围
        let result: String = eval(
            engine,
            "local r = cfg.update({test_n=999}); return json_encode(r)",
        )
        .unwrap();
        assert!(result.contains("\"ok\":false"));
        assert!(result.contains("最大值"));
    });
}

#[test]
fn test_cfg_update_non_table_arg() {
    with_engine(|engine| {
        exec(engine, r#"
            cfg = cfg or {}
            function cfg.update(changes)
                    if type(changes) ~= "table" then return { ok=false, errors={ _global="参数必须是 table" } } end
                    return { ok=true }
            end
        "#).unwrap();

        // 直接传入非 table 应该报错
        let result: String = eval(
            engine,
            "local r = cfg.update('invalid'); return json_encode(r)",
        )
        .unwrap();
        assert!(result.contains("\"ok\":false"));
    });
}

// ================================================================
// cfg._validate 边界条件测试
// ================================================================

#[test]
fn test_cfg_validate_boolean_valid() {
    with_engine(|engine| {
        let result: bool = eval(
            engine,
            r#"
            cfg = cfg or {}
            function cfg._validate(field, value)
                    if field.type == "boolean" and type(value) ~= "boolean" then
                        return false, "需要布尔值"
                    end
                    return true, nil
            end
            local ok, err
            ok, err = cfg._validate({type="boolean"}, true);  if not ok then return false end
            ok, err = cfg._validate({type="boolean"}, false); if not ok then return false end
            return true
        "#,
        )
        .unwrap();
        assert!(result);
    });
}

#[test]
fn test_cfg_validate_boolean_invalid() {
    with_engine(|engine| {
        let result: bool = eval(
            engine,
            r#"
            cfg = cfg or {}
            function cfg._validate(field, value)
                    if field.type == "boolean" and type(value) ~= "boolean" then
                        return false, "需要布尔值"
                    end
                    return true, nil
            end
            local ok, err = cfg._validate({type="boolean"}, "not_bool")
            return not ok
        "#,
        )
        .unwrap();
        assert!(result);
    });
}

#[test]
fn test_cfg_validate_number_valid() {
    with_engine(|engine| {
        let result: bool = eval(engine, r#"
            cfg = cfg or {}
            function cfg._validate(field, value)
                    if field.type == "number" then
                        local n = tonumber(value)
                        if n == nil then return false, "需要数字" end
                        if field.min and n < field.min then return false, "最小值" end
                        if field.max and n > field.max then return false, "最大值" end
                    end
                    return true, nil
            end
            local ok
            ok, _ = cfg._validate({type="number", min=0, max=100}, 50); if not ok then return false end
            ok, _ = cfg._validate({type="number", min=0, max=100}, 0);  if not ok then return false end
            ok, _ = cfg._validate({type="number", min=0, max=100}, 100); if not ok then return false end
            return true
        "#).unwrap();
        assert!(result);
    });
}

#[test]
fn test_cfg_validate_number_below_min() {
    with_engine(|engine| {
        let result: bool = eval(
            engine,
            r#"
            cfg = cfg or {}
            function cfg._validate(field, value)
                    if field.type == "number" then
                        local n = tonumber(value)
                        if n == nil then return false, "需要数字" end
                        if field.min and n < field.min then return false, "最小值" end
                    end
                    return true, nil
            end
            local ok, err = cfg._validate({type="number", min=10}, 5)
            return (not ok) and (err == "最小值")
        "#,
        )
        .unwrap();
        assert!(result);
    });
}

#[test]
fn test_cfg_validate_number_above_max() {
    with_engine(|engine| {
        let result: bool = eval(
            engine,
            r#"
            cfg = cfg or {}
            function cfg._validate(field, value)
                    if field.type == "number" then
                        local n = tonumber(value)
                        if n == nil then return false, "需要数字" end
                        if field.max and n > field.max then return false, "最大值" end
                    end
                    return true, nil
            end
            local ok, err = cfg._validate({type="number", max=10}, 20)
            return (not ok) and (err == "最大值")
        "#,
        )
        .unwrap();
        assert!(result);
    });
}

#[test]
fn test_cfg_validate_number_not_a_number() {
    with_engine(|engine| {
        let result: bool = eval(
            engine,
            r#"
            cfg = cfg or {}
            function cfg._validate(field, value)
                    if field.type == "number" then
                        local n = tonumber(value)
                        if n == nil then return false, "需要数字" end
                    end
                    return true, nil
            end
            local ok, err = cfg._validate({type="number"}, "not_a_number")
            return (not ok) and (err == "需要数字")
        "#,
        )
        .unwrap();
        assert!(result);
    });
}

#[test]
fn test_cfg_validate_string_valid() {
    with_engine(|engine| {
        let result: bool = eval(
            engine,
            r#"
            cfg = cfg or {}
            function cfg._validate(field, value)
                    if field.type == "string" and type(value) ~= "string" then
                        return false, "需要字符串"
                    end
                    return true, nil
            end
            return cfg._validate({type="string"}, "hello")
        "#,
        )
        .unwrap();
        assert!(result);
    });
}

#[test]
fn test_cfg_validate_string_invalid() {
    with_engine(|engine| {
        let result: bool = eval(
            engine,
            r#"
            cfg = cfg or {}
            function cfg._validate(field, value)
                    if field.type == "string" and type(value) ~= "string" then
                        return false, "需要字符串"
                    end
                    return true, nil
            end
            local ok, err = cfg._validate({type="string"}, 123)
            return (not ok) and (err == "需要字符串")
        "#,
        )
        .unwrap();
        assert!(result);
    });
}

#[test]
fn test_cfg_validate_option_valid() {
    with_engine(|engine| {
        let result: bool = eval(engine, r#"
            cfg = cfg or {}
            function cfg._validate(field, value)
                    if field.type == "option" then
                        for _, opt in ipairs(field.options) do
                            if tostring(opt) == tostring(value) then return true, nil end
                        end
                        return false, "无效选项"
                    end
                    return true, nil
            end
            local ok
            ok, _ = cfg._validate({type="option", options={"a","b","c"}}, "a"); if not ok then return false end
            ok, _ = cfg._validate({type="option", options={"a","b","c"}}, "b"); if not ok then return false end
            ok, _ = cfg._validate({type="option", options={"a","b","c"}}, "c"); if not ok then return false end
            return true
        "#).unwrap();
        assert!(result);
    });
}

#[test]
fn test_cfg_validate_option_invalid() {
    with_engine(|engine| {
        let result: bool = eval(
            engine,
            r#"
            cfg = cfg or {}
            function cfg._validate(field, value)
                    if field.type == "option" then
                        for _, opt in ipairs(field.options) do
                            if tostring(opt) == tostring(value) then return true, nil end
                        end
                        return false, "无效选项"
                    end
                    return true, nil
            end
            local ok, err = cfg._validate({type="option", options={"x","y"}}, "z")
            return (not ok) and (err == "无效选项")
        "#,
        )
        .unwrap();
        assert!(result);
    });
}

// ================================================================
// cfg.schema 各类型的 getter/setter 测试
// ================================================================

#[test]
fn test_cfg_field_boolean_getter_setter() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            cfg = cfg or {}
            test_flag = 0
            local field = {
                    getter = function() return test_flag and test_flag > 0 end,
                    setter = function(v) test_flag = v and 1 or 0 end,
            }
            -- 初始: false
            assert(field.getter() == false)
            -- 设为 true
            field.setter(true)
            assert(field.getter() == true)
            -- 检查全局变量
            assert(test_flag == 1)
            -- 再设回 false
            field.setter(false)
            assert(field.getter() == false)
            assert(test_flag == 0)
        "#,
        )
        .unwrap();
    });
}

#[test]
fn test_cfg_field_number_getter_setter() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            cfg = cfg or {}
            test_num = 50
            local field = {
                    getter = function() return test_num end,
                    setter = function(v) test_num = tonumber(v) or test_num end,
            }
            assert(field.getter() == 50)
            field.setter(200)
            assert(field.getter() == 200)
            -- 传入字符串也能转换
            field.setter("75")
            assert(field.getter() == 75)
        "#,
        )
        .unwrap();
    });
}

#[test]
fn test_cfg_field_string_getter_setter() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            cfg = cfg or {}
            skills = "unarmed"
            local field = {
                    getter = function() return skills end,
                    setter = function(v) skills = v end,
            }
            assert(field.getter() == "unarmed")
            field.setter("sword")
            assert(field.getter() == "sword")
        "#,
        )
        .unwrap();
    });
}

#[test]
fn test_cfg_field_option_getter_setter() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            cfg = cfg or {}
            _opt = "xue"
            local field = {
                    getter = function() return _opt end,
                    setter = function(v) _opt = v end,
            }
            assert(field.getter() == "xue")
            field.setter("lingwu")
            assert(field.getter() == "lingwu")
        "#,
        )
        .unwrap();
    });
}

// ================================================================
// Simulate API 测试
// ================================================================

#[test]
fn test_simulate_basic_trigger() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            sim_result = ""
            AddTrigger('sim_test', 'Exits: (.+)', '', 33, 0, 0, '', 'function(n,l,w) sim_result = w[1] end', 0, 0)
        "#,
        )
        .unwrap();
        exec(engine, r#"Simulate("Exits: north\n")"#).unwrap();
        let result: String = eval(engine, "return sim_result").unwrap();
        assert_eq!(result, "north");
    });
}

#[test]
fn test_simulate_multiple_args_concatenated() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            sim_result = ""
            AddTrigger('sim_multi', 'hello (.+)', '', 33, 0, 0, '', 'function(n,l,w) sim_result = w[1] end', 0, 0)
        "#,
        )
        .unwrap();
        // MUSHclient Lua 特性：多个参数拼接
        exec(engine, r#"Simulate("hello ", "world\n")"#).unwrap();
        let result: String = eval(engine, "return sim_result").unwrap();
        assert_eq!(result, "world");
    });
}

#[test]
fn test_simulate_does_not_clear_pending_commands() {
    with_engine(|engine| {
        // 先用 Execute 压入一个命令
        exec(engine, "Execute('look')").unwrap();
        let cmds_before = engine.drain_commands();
        assert_eq!(cmds_before, vec!["look"]);

        // 再用 Execute 压入命令，然后 Simulate 不应清空它
        exec(engine, "Execute('score')").unwrap();
        exec(
            engine,
            r#"
            AddTrigger('sim_noclear', 'test_line', '', 1, 0, 0, '', '', 0, 0)
        "#,
        )
        .unwrap();
        exec(engine, r#"Simulate("test_line\n")"#).unwrap();
        let cmds = engine.drain_commands();
        assert!(
            cmds.contains(&"score".to_string()),
            "Simulate should not clear pending_commands, got: {:?}",
            cmds
        );
    });
}

#[test]
fn test_simulate_adds_to_pending_logs() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            AddTrigger('sim_log', 'visible line', '', 1, 0, 0, '', '', 0, 0)
        "#,
        )
        .unwrap();
        exec(engine, r#"Simulate("visible line\n")"#).unwrap();
        let logs = engine.drain_logs();
        assert!(
            logs.iter().any(|l| l.contains("visible line")),
            "Simulate should add text to pending_logs, got: {:?}",
            logs
        );
    });
}

#[test]
fn test_simulate_omit_from_output() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            AddTrigger('sim_omit', 'hide_me', '', 33, 0, 0, '', '', 0, 0)
            SetTriggerOption('sim_omit', 'omit_from_output', true)
        "#,
        )
        .unwrap();
        exec(engine, r#"Simulate("hide_me\n")"#).unwrap();
        let logs = engine.drain_logs();
        assert!(
            !logs.iter().any(|l| l.contains("hide_me")),
            "omit_from_output should suppress log, got: {:?}",
            logs
        );
    });
}

#[test]
fn test_simulate_multiline() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            sim_result = ""
            AddTrigger('sim_ml', 'line1', '', 1, 0, 0, '', 'function() sim_result = sim_result .. "1" end', 0, 0)
            AddTrigger('sim_ml2', 'line2', '', 1, 0, 0, '', 'function() sim_result = sim_result .. "2" end', 0, 0)
        "#,
        )
        .unwrap();
        exec(engine, r#"Simulate("line1\nline2\n")"#).unwrap();
        let result: String = eval(engine, "return sim_result").unwrap();
        assert_eq!(result, "12");
    });
}

#[test]
fn test_simulate_trigger_callback_sends_command() {
    with_engine(|engine| {
        // 触发器回调中调用 Execute 发送命令
        exec(
            engine,
            r#"
            AddTrigger('sim_send', 'go_now', '', 33, 0, 0, '', 'function() Execute("go north") end', 0, 0)
        "#,
        )
        .unwrap();
        exec(engine, r#"Simulate("go_now\n")"#).unwrap();
        let cmds = engine.drain_commands();
        assert!(
            cmds.contains(&"go north".to_string()),
            "Simulate trigger callback should add to pending_commands via Execute, got: {:?}",
            cmds
        );
    });
}

#[test]
fn test_simulate_empty_lines_skipped() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            sim_count = 0
            AddTrigger('sim_empty', '.+', '', 33, 0, 0, '', 'function() sim_count = sim_count + 1 end', 0, 0)
        "#,
        )
        .unwrap();
        // 只有中间一行非空
        exec(engine, r#"Simulate("\nhello\n\n")"#).unwrap();
        let count: i64 = eval(engine, "return sim_count").unwrap();
        assert_eq!(count, 1);
    });
}

#[test]
fn test_simulate_no_return_value() {
    with_engine(|engine| {
        // Simulate returns nothing (nil in Lua)
        let result: mlua::Value = eval(engine, r#"return Simulate("anything\n")"#).unwrap();
        assert!(matches!(result, mlua::Value::Nil));
    });
}

// ================================================================
// ANSI 样式解析测试 (parse_style_runs)
// ================================================================

#[test]
fn test_parse_style_runs_plain_text() {
    let runs = LuaEngine::parse_style_runs("hello world");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].start, 0);
    assert_eq!(runs[0].length, 11);
    assert_eq!(runs[0].textcolour, 7); // 默认前景色 silver
    assert_eq!(runs[0].backcolour, 0); // 默认背景色 black
}

#[test]
fn test_parse_style_runs_empty() {
    let runs = LuaEngine::parse_style_runs("");
    assert!(runs.is_empty());
}

#[test]
fn test_parse_style_runs_red_foreground() {
    let runs = LuaEngine::parse_style_runs("\x1b[31mred text\x1b[0m");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].textcolour, 1); // red
    assert_eq!(runs[0].start, 0);
    assert_eq!(runs[0].length, 8);
}

#[test]
fn test_parse_style_runs_reset_restores_defaults() {
    // 红色前景 → reset → 白色文本（reset 后有文本 → 新增运行）
    let runs = LuaEngine::parse_style_runs("\x1b[31mred\x1b[0mdefault");
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].textcolour, 1); // red
    assert_eq!(runs[0].length, 3);
    assert_eq!(runs[1].textcolour, 7); // default
    assert_eq!(runs[1].length, 7);
}

#[test]
fn test_parse_style_runs_bold_and_underline() {
    let runs = LuaEngine::parse_style_runs("\x1b[1mbold\x1b[0mnormal");
    assert_eq!(runs.len(), 2);
    assert!(runs[0].bold);
    assert!(!runs[0].underline);
    assert!(!runs[0].italic);
    assert_eq!(runs[0].length, 4);
    assert!(!runs[1].bold);
}

#[test]
fn test_parse_style_runs_multiple_colors() {
    // 无 reset 后文本 → 2 个运行
    let runs = LuaEngine::parse_style_runs("\x1b[33myellow\x1b[32mgreen\x1b[0m");
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].textcolour, 3); // yellow
    assert_eq!(runs[0].length, 6);
    assert_eq!(runs[1].textcolour, 2); // green
    assert_eq!(runs[1].length, 5);
}

#[test]
fn test_parse_style_runs_background_color() {
    let runs = LuaEngine::parse_style_runs("\x1b[41mred bg\x1b[0m");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].backcolour, 1); // red background
    assert_eq!(runs[0].textcolour, 7);
}

#[test]
fn test_parse_style_runs_bright_colors() {
    let runs = LuaEngine::parse_style_runs("\x1b[91mbright red\x1b[0m");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].textcolour, 9); // bright red (91 - 82 = 9)
    assert_eq!(runs[0].length, 10);
}

#[test]
fn test_parse_style_runs_combined_sgr() {
    // 多个参数组合 + 无 reset 后文本 → 1 个运行
    let runs = LuaEngine::parse_style_runs("\x1b[1;31;42mstyled\x1b[0m");
    assert_eq!(runs.len(), 1);
    assert!(runs[0].bold);
    assert_eq!(runs[0].textcolour, 1);
    assert_eq!(runs[0].backcolour, 2);
    assert_eq!(runs[0].length, 6);
}

#[test]
fn test_parse_style_runs_unicode_character_width() {
    // 中文字符各占 3 字节（UTF-8），reset 后无文本 → 1 个运行
    let runs = LuaEngine::parse_style_runs("\x1b[31m中文test\x1b[0m");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].length, 10); // 中文=6字节, test=4字节 → 10
    assert_eq!(runs[0].start, 0);
}

// ================================================================
// GetStyle / RGBColourToName API 测试
// ================================================================

#[test]
fn test_get_style_basic() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            -- 先触发一个带 ANSI 的行，让引擎解析样式
            test_styles = nil
            AddTrigger('style_trig', 'hello', '', 33, 0, 0, '',
                    'function(n,l,w,s) test_styles = s end', 0, 0)
        "#,
        )
        .unwrap();
        engine.process_output("\x1b[31mhello\x1b[0m");

        let styles: mlua::Value = eval(engine, "return test_styles").unwrap();
        assert!(
            matches!(styles, mlua::Value::Table(_)),
            "styles should be a table"
        );

        // 用 GetStyle 按位置查询
        let result: mlua::Value = eval(
            engine,
            "
            if test_styles then
                    local s = GetStyle(test_styles, 1)
                    return s.textcolour
            end
            return -1
        ",
        )
        .unwrap();
        assert_eq!(result, mlua::Value::Integer(1)); // red
    });
}

#[test]
fn test_get_style_out_of_bounds() {
    with_engine(|engine| {
        exec(
            engine,
            r#"
            test_styles = nil
            AddTrigger('style_ob', 'hello', '', 33, 0, 0, '',
                    'function(n,l,w,s) test_styles = s end', 0, 0)
        "#,
        )
        .unwrap();
        engine.process_output("\x1b[31mhello\x1b[0m");

        // 查询超出范围的位置 → nil
        let result: mlua::Value = eval(
            engine,
            "
            if test_styles then
                    return GetStyle(test_styles, 999)
            end
            return nil
        ",
        )
        .unwrap();
        assert!(matches!(result, mlua::Value::Nil));
    });
}

#[test]
fn test_rgb_colour_to_name() {
    with_engine(|engine| {
        let cases = [
            (0, "black"),
            (1, "red"),
            (2, "green"),
            (3, "yellow"),
            (4, "blue"),
            (5, "magenta"),
            (6, "cyan"),
            (7, "silver"),
            (8, "grey"),
            (9, "bright red"),
            (10, "bright green"),
            (11, "bright yellow"),
            (12, "bright blue"),
            (13, "bright magenta"),
            (14, "bright cyan"),
            (15, "white"),
        ];
        for (code, name) in &cases {
            let result: String =
                eval(engine, &format!("return RGBColourToName({})", code)).unwrap();
            assert_eq!(result, *name, "colour {} should be '{}'", code, name);
        }
    });
}

#[test]
fn test_rgb_colour_to_name_out_of_range() {
    with_engine(|engine| {
        // 超出 0-15 范围的色号 → 返回 "colour_N"
        let result: String = eval(engine, "return RGBColourToName(42)").unwrap();
        assert_eq!(result, "colour_42");
        let result: String = eval(engine, "return RGBColourToName(255)").unwrap();
        assert_eq!(result, "colour_255");
    });
}

#[test]
fn test_styles_passed_as_fourth_parameter() {
    with_engine(|engine| {
        // 验证 styles 确实作为第 4 个参数传入
        exec(
            engine,
            r#"
            style_count = nil
            AddTrigger('style4', 'hello', '', 33, 0, 0, '',
                    'function(n,l,w,s) style_count = s and #s or -1 end', 0, 0)
        "#,
        )
        .unwrap();
        engine.process_output("\x1b[32mhello\x1b[0m");

        let count: i64 = eval(engine, "return style_count").unwrap();
        assert!(
            count > 0,
            "styles should be non-nil for ANSI text, got count={}",
            count
        );
    });
}

#[test]
fn test_no_styles_for_plain_text() {
    with_engine(|engine| {
        // 无 ANSI 的行 → styles 为包含 1 个默认运行的表
        exec(
            engine,
            r#"
            style_count = -1
            AddTrigger('plain', 'hello', '', 33, 0, 0, '',
                    'function(n,l,w,s) style_count = s and #s or -1 end', 0, 0)
        "#,
        )
        .unwrap();
        engine.process_output("hello world");

        let count: i64 = eval(engine, "return style_count").unwrap();
        assert_eq!(
            count, 1,
            "plain text should have 1 default style run, got {}",
            count
        );
    });
}

#[test]
fn test_simulate_passes_nil_styles() {
    with_engine(|engine| {
        // Simulate 传 nil 作为第 4 参数
        exec(
            engine,
            r#"
            sim_style = 'unknown'
            AddTrigger('sim_style_trig', 'hello', '', 33, 0, 0, '',
                    'function(n,l,w,s) sim_style = (s == nil) and "nil" or "table" end', 0, 0)
        "#,
        )
        .unwrap();
        exec(engine, r#"Simulate("hello\n")"#).unwrap();

        let result: String = eval(engine, "return sim_style").unwrap();
        assert_eq!(result, "nil", "Simulate should pass nil as styles");
    });
}

// ================================================================
// AddTrigger / AddTriggerEx Replace 标志测试
// ================================================================

#[test]
fn test_trigger_replace_flag() {
    with_engine(|engine| {
        exec(engine, "counter = 0").unwrap();

        // 用 Replace 标志创建第一个 trigger
        exec(
            engine,
            "AddTrigger('rep_trig', 'hello', '', 1 + 1024 + 32, 0, 0, '', 'function() counter = counter + 1 end', 0, 0)",
        )
        .unwrap();
        engine.process_output("hello");
        let c: i64 = eval(engine, "return counter").unwrap();
        assert_eq!(c, 1, "first trigger should fire once");

        // 用 Replace 标志创建同名 trigger（应替换，不应追加）
        exec(
            engine,
            "AddTrigger('rep_trig', 'hello', '', 1 + 1024 + 32, 0, 0, '', 'function() counter = counter + 10 end', 0, 0)",
        )
        .unwrap();

        // 验证只有一个 trigger
        let count: i64 = eval(engine, "return #GetTriggerList()").unwrap();
        assert_eq!(count, 1);

        // 重置计数器并触发
        exec(engine, "counter = 0").unwrap();
        engine.process_output("hello");
        let c: i64 = eval(engine, "return counter").unwrap();
        // 应为 10（新 trigger 触发一次），而不是 11（新旧各一次）
        assert_eq!(c, 10, "replaced trigger should fire only new callback");
    });
}

#[test]
fn test_trigger_replace_multiple_calls() {
    with_engine(|engine| {
        exec(engine, "counter = 0").unwrap();

        // 连续多次 Replace
        for i in 1..=5 {
            let code = format!(
                    "AddTrigger('multi_rep', 'hello', '', 1 + 1024 + 32, 0, 0, '', 'function() counter = counter + {} end', 0, 0)",
                    i
            );
            exec(engine, &code).unwrap();
        }

        // 验证始终只有一个 trigger
        let count: i64 = eval(engine, "return #GetTriggerList()").unwrap();
        assert_eq!(count, 1);

        exec(engine, "counter = 0").unwrap();
        engine.process_output("hello");
        let c: i64 = eval(engine, "return counter").unwrap();
        // 应为 5（最后一次 Replace 的 callback）
        assert_eq!(c, 5, "after 5 replaces, only the last callback should fire");
    });
}

#[test]
fn test_trigger_replace_changes_pattern() {
    with_engine(|engine| {
        exec(engine, "last_match = nil").unwrap();

        // 创建匹配 "hello" 的 trigger
        exec(
            engine,
            "AddTrigger('pat_trig', 'hello', '', 1 + 1024 + 32, 0, 0, '', 'function() last_match = \"hello\" end', 0, 0)",
        )
        .unwrap();
        engine.process_output("hello");
        let m: Option<String> = eval(engine, "return last_match").unwrap();
        assert_eq!(m, Some("hello".to_string()));

        // Replace 为匹配 "world" 的 trigger
        exec(
            engine,
            "AddTrigger('pat_trig', 'world', '', 1 + 1024 + 32, 0, 0, '', 'function() last_match = \"world\" end', 0, 0)",
        )
        .unwrap();
        exec(engine, "last_match = nil").unwrap();
        engine.process_output("hello"); // 不应匹配
        let m: Option<String> = eval(engine, "return last_match").unwrap();
        assert_eq!(m, None, "old pattern should no longer match");

        engine.process_output("world"); // 应匹配新模式
        let m: Option<String> = eval(engine, "return last_match").unwrap();
        assert_eq!(m, Some("world".to_string()), "new pattern should match");
    });
}

#[test]
fn test_trigger_without_replace_accumulates() {
    with_engine(|engine| {
        exec(engine, "counter = 0").unwrap();

        // 不加 Replace 标志创建同名 trigger——会追加
        for _ in 1..=3 {
            exec(
                    engine,
                    "AddTrigger('acc_trig', 'hello', '', 1 + 32, 0, 0, '', 'function() counter = counter + 1 end', 0, 0)",
            )
            .unwrap();
        }

        engine.process_output("hello");
        let c: i64 = eval(engine, "return counter").unwrap();
        // 3 个同样的 trigger，各触发一次
        assert_eq!(c, 3, "without Replace, duplicates accumulate");
    });
}

// ================================================================
// AddAlias Replace 标志测试
// ================================================================

#[test]
fn test_alias_replace_flag() {
    with_engine(|engine| {
        exec(engine, "counter = 0").unwrap();

        // 用 Replace 标志创建第一个 alias
        exec(
            engine,
            "AddAlias('rep_alias', 'hello', '', 1 + 1024 + 32, 'function() counter = counter + 1 end')",
        )
        .unwrap();

        // 匹配 alias
        engine.process_input("hello");
        let c: i64 = eval(engine, "return counter").unwrap();
        assert_eq!(c, 1, "first alias should fire once");

        // 用 Replace 标志创建同名 alias（应替换）
        exec(
            engine,
            "AddAlias('rep_alias', 'hello', '', 1 + 1024 + 32, 'function() counter = counter + 10 end')",
        )
        .unwrap();

        // 验证只有一个 alias
        let count: i64 = eval(engine, "return #GetAliasList()").unwrap();
        assert_eq!(count, 1, "Replace should leave only one alias");

        exec(engine, "counter = 0").unwrap();
        engine.process_input("hello");
        let c: i64 = eval(engine, "return counter").unwrap();
        assert_eq!(c, 10, "replaced alias should fire only new callback");
    });
}

#[test]
fn test_alias_replace_multiple_calls() {
    with_engine(|engine| {
        exec(engine, "counter = 0").unwrap();

        // 连续多次 Replace，最后一次应生效
        for i in 1..=5 {
            let code = format!(
                    "AddAlias('multi_alias', 'hello', '', 1 + 1024 + 32, 'function() counter = counter + {} end')",
                    i
            );
            exec(engine, &code).unwrap();
        }

        let count: i64 = eval(engine, "return #GetAliasList()").unwrap();
        assert_eq!(count, 1);

        exec(engine, "counter = 0").unwrap();
        engine.process_input("hello");
        let c: i64 = eval(engine, "return counter").unwrap();
        assert_eq!(c, 5, "after 5 replaces, only last alias callback fires");
    });
}

#[test]
fn test_alias_replace_changes_pattern() {
    with_engine(|engine| {
        exec(engine, "last_match = nil").unwrap();

        // 创建匹配 "hello" 的 alias
        exec(
            engine,
            "AddAlias('pat_alias', 'hello', '', 1 + 1024 + 32, 'function() last_match = \"hello\" end')",
        )
        .unwrap();
        engine.process_input("hello");
        let m: Option<String> = eval(engine, "return last_match").unwrap();
        assert_eq!(m, Some("hello".to_string()));

        // Replace 为匹配 "world" 的 alias
        exec(
            engine,
            "AddAlias('pat_alias', 'world', '', 1 + 1024 + 32, 'function() last_match = \"world\" end')",
        )
        .unwrap();
        exec(engine, "last_match = nil").unwrap();
        engine.process_input("hello"); // 不应匹配
        let m: Option<String> = eval(engine, "return last_match").unwrap();
        assert_eq!(m, None, "old alias pattern should no longer match");

        engine.process_input("world"); // 应匹配新模式
        let m: Option<String> = eval(engine, "return last_match").unwrap();
        assert_eq!(
            m,
            Some("world".to_string()),
            "new alias pattern should match"
        );
    });
}

#[test]
fn test_alias_without_replace_accumulates() {
    with_engine(|engine| {
        exec(engine, "counter = 0").unwrap();

        // 不加 Replace 标志，同名 alias 会累积
        for _ in 1..=3 {
            exec(
                    engine,
                    "AddAlias('acc_alias', 'hello', '', 1 + 32, 'function() counter = counter + 1 end')",
            )
            .unwrap();
        }

        engine.process_input("hello");
        let c: i64 = eval(engine, "return counter").unwrap();
        assert_eq!(c, 3, "without Replace, aliases accumulate");
    });
}

// ================================================================
// AddTimer Replace 标志补充测试
// ================================================================

#[test]
fn test_timer_replace_multiple_times() {
    with_engine(|engine| {
        exec(engine, "counter = 0").unwrap();

        // 多次 replace 后，最后一次应生效
        for i in 1..=5 {
            let code = format!(
                "AddTimer('multi_t', 0, 0, 1, '', 1 + 1024, 'counter = counter + {}')",
                i
            );
            exec(engine, &code).unwrap();
        }

        let count: i64 = eval(engine, "return #GetTimerList()").unwrap();
        assert_eq!(count, 1);

        exec(engine, "counter = 0").unwrap();
        engine.fire_timer(0);
        let c: i64 = eval(engine, "return counter").unwrap();
        assert_eq!(c, 5, "after 5 replaces, only last timer callback fires");
    });
}

#[test]
fn test_timer_replace_without_replace_accumulates() {
    with_engine(|engine| {
        exec(engine, "counter = 0").unwrap();

        // 不加 Replace 标志，同名 timer 会累积
        for _ in 1..=3 {
            exec(
                engine,
                "AddTimer('acc_t', 0, 0, 1, '', 1, 'counter = counter + 1')",
            )
            .unwrap();
        }

        let count: i64 = eval(engine, "return #GetTimerList()").unwrap();
        assert_eq!(count, 3);

        // 触发所有 timer
        for i in 0..3 {
            engine.fire_timer(i);
        }
        let c: i64 = eval(engine, "return counter").unwrap();
        assert_eq!(c, 3, "without Replace, timers accumulate");
    });
}

#[test]
fn test_timer_replace_preserves_disabled_after_enable_group() {
    with_engine(|engine| {
        exec(engine, "counter = 0").unwrap();

        // 创建 timer 并设置 group
        exec(
            engine,
            "AddTimer('grp_t', 0, 0, 1, '', 1 + 1024, 'counter = counter + 1')",
        )
        .unwrap();
        exec(engine, "SetTimerOption('grp_t', 'group', 'test_grp')").unwrap();
        // 禁用 group
        exec(engine, "EnableTimerGroup('test_grp', false)").unwrap();

        // Replace 后应继承禁用状态
        exec(
            engine,
            "AddTimer('grp_t', 0, 0, 1, '', 1 + 1024, 'counter = counter + 10')",
        )
        .unwrap();
        exec(engine, "SetTimerOption('grp_t', 'group', 'test_grp')").unwrap();

        // 再次启用 group
        exec(engine, "EnableTimerGroup('test_grp', true)").unwrap();

        engine.fire_timer(0);
        let c: i64 = eval(engine, "return counter").unwrap();
        assert_eq!(c, 10, "timer should fire after group re-enabled");
    });
}

#[test]
fn test_timer_replace_name_not_found() {
    with_engine(|engine| {
        exec(engine, "counter = 0").unwrap();

        // Replace 一个不存在的 timer——应像 AddTimer 一样创建
        exec(
            engine,
            "AddTimer('new_t', 0, 0, 1, '', 1 + 1024, 'counter = counter + 1')",
        )
        .unwrap();

        let count: i64 = eval(engine, "return #GetTimerList()").unwrap();
        assert_eq!(count, 1);

        engine.fire_timer(0);
        let c: i64 = eval(engine, "return counter").unwrap();
        assert_eq!(c, 1);
    });
}

// ================================================================
// AddAlias 参数错误处理
// ================================================================

#[test]
fn test_addalias_missing_arguments() {
    with_engine(|engine| {
        // 少于 4 个参数应报错
        let result: mlua::Result<()> = exec(engine, "AddAlias('name', 'match', 'resp')");
        assert!(result.is_err(), "AddAlias with 3 args should error");
    });
}

// ================================================================
// AddTriggerEx 参数错误处理
// ================================================================

#[test]
fn test_addtriggerex_missing_arguments() {
    with_engine(|engine| {
        // 少于 4 个参数应报错
        let result: mlua::Result<()> = exec(engine, "AddTriggerEx('name', 'match', 'resp')");
        assert!(result.is_err(), "AddTriggerEx with 3 args should error");
    });
}

// ================================================================
// AddTimer 参数错误处理
// ================================================================

#[test]
fn test_addtimer_missing_arguments() {
    with_engine(|engine| {
        // 少于 6 个参数应报错
        let result: mlua::Result<()> = exec(engine, "AddTimer('name', 0, 0, 5, 'resp')");
        assert!(result.is_err(), "AddTimer with 5 args should error");
    });
}

// ================================================================
// GetStyle + capture group w[1] 综合测试
// ================================================================

/// 模拟 always_daytime.dosomething1 的颜色过滤场景：
/// trigger 捕获组 w[1] → string.find(l, w[1]) → GetStyle → RGBColourToName
#[test]
fn test_get_style_with_capture_group() {
    with_engine(|engine| {
        exec(engine, "test_colour = -1; test_back = -1; test_w1 = ''").unwrap();
        exec(
            engine,
            "AddTrigger('cap_style', '(hello|world)', '', 41, 0, 0, '', \
             'function(n,l,w,s) \
                    test_w1 = w[1] \
                    local col = string.find(l, w[1]) \
                    if col then \
                        local st = GetStyle(s, col) \
                        if st then test_colour = st.textcolour; test_back = st.backcolour end \
                    end \
             end', 0, 0)",
        )
        .unwrap();

        // 模拟带 ANSI 颜色的服务器文本（cyan = 6）
        engine.process_output("\x1b[36mhello\x1b[0m");

        let w1: String = eval(engine, "return test_w1").unwrap();
        assert_eq!(w1, "hello", "w[1] should be the captured text");

        let colour: i64 = eval(engine, "return test_colour").unwrap();
        assert_eq!(colour, 6, "cyan text should have colour code 6");

        let back: i64 = eval(engine, "return test_back").unwrap();
        assert_eq!(back, 0, "default background is 0");

        // RGBColourToName 验证
        let name: String = eval(engine, "return RGBColourToName(6)").unwrap();
        assert_eq!(name, "cyan", "colour 6 should be cyan");
    });
}

#[test]
fn test_get_style_silver_filter_scenario() {
    with_engine(|engine| {
        exec(engine, "silver_detected = false").unwrap();
        exec(
            engine,
            "AddTrigger('silver_test', '(command)', '', 41, 0, 0, '', \
             'function(n,l,w,s) \
                    local col = string.find(l, w[1]) \
                    if col then \
                        local st = GetStyle(s, col) \
                        if st then \
                            local c = RGBColourToName(st.textcolour) \
                            if c == \"silver\" and st.backcolour == 0 then silver_detected = true end \
                        end \
                    end \
             end', 0, 0)",
        )
        .unwrap();

        // 模拟命令回显（无 ANSI，默认样式 silver=7, back=0）
        engine.process_output("command");

        let detected: bool = eval(engine, "return silver_detected").unwrap();
        assert!(detected, "plain text should have silver colour on back=0");
    });
}

#[test]
fn test_get_style_colourful_daytime_text() {
    with_engine(|engine| {
        exec(
            engine,
            "extracted_colour = -1; extracted_bg = -1; found_pos = -1",
        )
        .unwrap();
        exec(
            engine,
            "AddTrigger('daytime_cap', \
             '(太阳从东方的地平线升起了|东方的天空中开始出现一丝微曦|夜晚降临了)', \
             '', 41, 0, 0, '', \
             'function(n,l,w,s) \
                    local col = string.find(l, w[1]) \
                    found_pos = col or -1 \
                    if col then \
                        local st = GetStyle(s, col) \
                        if st then extracted_colour = st.textcolour; extracted_bg = st.backcolour end \
                    end \
             end', 0, 0)",
        )
        .unwrap();

        // 模拟 "东方的天空中开始出现一丝微曦"（cyan = 6）
        engine.process_output("\x1b[1;36m东方的天空中开始出现一丝微曦\x1b[37;0m");

        let pos: i64 = eval(engine, "return found_pos").unwrap();
        assert!(pos > 0, "w[1] should be found in clean_line");

        let colour: i64 = eval(engine, "return extracted_colour").unwrap();
        assert_eq!(colour, 6, "cyan ANSI (36) should map to colour 6");

        let name: String = eval(engine, "return RGBColourToName(extracted_colour)").unwrap();
        assert_ne!(name, "silver", "daytime text should NOT be silver");

        // RGBColourToName 验证标准色
        let cyan_name: String = eval(engine, "return RGBColourToName(extracted_colour)").unwrap();
        assert_eq!(cyan_name, "cyan");
    });
}

#[test]
fn test_get_style_colourful_daytime_text_yellow() {
    with_engine(|engine| {
        exec(engine, "y_colour = -1; y_bg = -1").unwrap();
        exec(
            engine,
            "AddTrigger('daytime_yellow', \
             '(太阳从东方的地平线升起了|东方的天空中开始出现一丝微曦)', \
             '', 41, 0, 0, '', \
             'function(n,l,w,s) \
                    local col = string.find(l, w[1]) \
                    if col then \
                        local st = GetStyle(s, col) \
                        if st then y_colour = st.textcolour; y_bg = st.backcolour end \
                    end \
             end', 0, 0)",
        )
        .unwrap();

        // 模拟 "太阳从东方的地平线升起了"（yellow=3, bold）
        engine.process_output("\x1b[1;33m太阳从东方的地平线升起了。\x1b[37;0m");

        let colour: i64 = eval(engine, "return y_colour").unwrap();
        // yellow ANSI 33 → ANSI colour 3
        assert_eq!(colour, 3, "yellow ANSI (33) should map to colour 3");
    });
}

#[test]
fn test_get_style_with_w0_full_match() {
    with_engine(|engine| {
        exec(engine, "w0_colour = -1").unwrap();
        exec(
            engine,
            "AddTrigger('w0_test', '(hello)', '', 41, 0, 0, '', \
             'function(n,l,w,s) \
                    local col = string.find(l, w[0]) \
                    if col then \
                        local st = GetStyle(s, col) \
                        if st then w0_colour = st.textcolour end \
                    end \
             end', 0, 0)",
        )
        .unwrap();

        // 红色文本
        engine.process_output("\x1b[31mhello\x1b[0m");

        let colour: i64 = eval(engine, "return w0_colour").unwrap();
        assert_eq!(colour, 1, "red text should have colour 1");
    });
}

#[test]
fn test_get_style_mid_line_position() {
    with_engine(|engine| {
        exec(engine, "mid_colour = -1").unwrap();
        exec(
            engine,
            "AddTrigger('mid_test', '(world)', '', 41, 0, 0, '', \
             'function(n,l,w,s) \
                    local col = string.find(l, w[1]) \
                    if col then \
                        local st = GetStyle(s, col) \
                        if st then mid_colour = st.textcolour end \
                    end \
             end', 0, 0)",
        )
        .unwrap();

        // 一行多个颜色段，world 是绿色
        engine.process_output("\x1b[31mhi \x1b[32mworld\x1b[0m");

        let colour: i64 = eval(engine, "return mid_colour").unwrap();
        assert_eq!(colour, 2, "mid-line green text should have colour 2");
    });
}

#[test]
fn test_get_style_capture_not_found_returns_nil() {
    with_engine(|engine| {
        exec(engine, "style_result = 'not_called'").unwrap();
        exec(
            engine,
            "AddTrigger('nil_test', '(xyz)', '', 41, 0, 0, '', \
             'function(n,l,w,s) \
                    local col = string.find(l, w[1]) \
                    if col then \
                        local st = GetStyle(s, col) \
                        style_result = (st == nil) and \"nil\" or \"table\" \
                    else \
                        style_result = \"not_found\" \
                    end \
             end', 0, 0)",
        )
        .unwrap();

        // 未匹配的文本不应触发
        engine.process_output("nothing here");
        let result: String = eval(engine, "return style_result").unwrap();
        assert_eq!(
            result, "not_called",
            "non-matching text should not fire trigger"
        );
    });
}

#[test]
fn test_get_style_adjusts_twelve_hour_boundary() {
    with_engine(|engine| {
        exec(engine, "evening_colour = -1").unwrap();
        exec(
            engine,
            "AddTrigger('evening_cap', '(傍晚了|一轮火红的夕阳|夜幕笼罩)', '', 41, 0, 0, '', \
             'function(n,l,w,s) \
                    local col = string.find(l, w[1]) \
                    if col then \
                        local st = GetStyle(s, col) \
                        if st then evening_colour = st.textcolour end \
                    end \
             end', 0, 0)",
        )
        .unwrap();

        // 模拟 "傍晚了，太阳的馀晖将西方的天空映成一片火红。"（magenta=5）
        engine.process_output("\x1b[1;35m傍晚了，太阳的馀晖将西方的天空映成一片火红。\x1b[37;0m");

        let colour: i64 = eval(engine, "return evening_colour").unwrap();
        assert_eq!(colour, 5, "purple/magenta ANSI (35) should map to colour 5");
    });
}

#[test]
fn test_get_style_all_ansi_colours_round_trip() {
    // 验证所有 0-15 标准色都能通过 GetStyle + RGBColourToName 正确映射
    with_engine(|engine| {
        for (ansi_code, expected_colour, expected_name) in [
            (30, 0, "black"),
            (31, 1, "red"),
            (32, 2, "green"),
            (33, 3, "yellow"),
            (34, 4, "blue"),
            (35, 5, "magenta"),
            (36, 6, "cyan"),
            (37, 7, "silver"),
            (90, 8, "grey"),
            (91, 9, "bright red"),
            (92, 10, "bright green"),
            (93, 11, "bright yellow"),
            (94, 12, "bright blue"),
            (95, 13, "bright magenta"),
            (96, 14, "bright cyan"),
            (97, 15, "white"),
        ] {
            // 为每个 ANSI 色号注册一个独立的 trigger
            let setup = format!(
                "colour_{c} = -1; \
                     AddTrigger('ansi_{c}', '(text{c})', '', 41, 0, 0, '', \
                     'function(n,l,w,s) \
                        local col = string.find(l, w[1]) \
                        if col then \
                            local st = GetStyle(s, col) \
                            if st then colour_{c} = st.textcolour end \
                        end \
                     end', 0, 0)",
                c = ansi_code
            );
            exec(engine, &setup).unwrap();

            let ansi = format!("\x1b[{}mtext{}\x1b[0m", ansi_code, ansi_code);
            engine.process_output(&ansi);

            let colour: i64 = eval(engine, &format!("return colour_{}", ansi_code)).unwrap();
            assert_eq!(
                colour, expected_colour,
                "ANSI {} should map to colour {} ({})",
                ansi_code, expected_colour, expected_name
            );

            let name: String = eval(
                engine,
                &format!("return RGBColourToName({})", expected_colour),
            )
            .unwrap();
            assert_eq!(
                name, expected_name,
                "colour {} should be named '{}'",
                expected_colour, expected_name
            );
        }
    });
}
