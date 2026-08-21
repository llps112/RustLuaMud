// 内置命令解析（纯逻辑，无 IO 依赖）
// 从 app.rs 拆分而来：BuiltinCommand 枚举与 parse_* 系列函数

/// 内置命令解析结果
#[derive(Debug, PartialEq)]
pub(crate) enum BuiltinCommand {
    /// /connect <名称> <主机> <端口>
    Connect {
        name: String,
        host: String,
        port: u16,
    },
    /// /disconnect [编号]
    Disconnect { id: Option<usize> },
    /// /reconnect [编号] — 断开并重新连接
    Reconnect { id: Option<usize> },
    /// /close [编号]
    Close { id: Option<usize> },
    /// /list
    List,
    /// /load <脚本路径>
    Load { path: String },
    /// /load reload
    LoadReload,
    /// /lua <代码>
    Lua { code: String },
    /// /set <选项> <值>
    Set { option: String, value: String },
    /// /switch <角色名或编号>
    Switch { target: String },
    /// /profile load <角色名> | /profile list
    Profile { sub: ProfileSubcommand },
    /// /all <命令> — 发送命令到所有连接
    All { cmd: String },
    /// 未知命令
    Unknown,
}

/// /profile 子命令
#[derive(Debug, PartialEq)]
pub(crate) enum ProfileSubcommand {
    /// /profile load <角色名> — 从 profiles/ 加载角色配置并连接
    Load { name: String },
    /// /profile list — 列出 profiles/ 下可用角色
    List,
}

/// 解析内置命令（纯逻辑，无 IO 依赖）
pub(crate) fn parse_builtin_command(cmd: &str) -> BuiltinCommand {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        return BuiltinCommand::Unknown;
    }

    match parts[0] {
        "/connect" => {
            if let Some((host, port)) = parse_connect_args(&parts) {
                let name = parts[1].to_string();
                BuiltinCommand::Connect { name, host, port }
            } else {
                BuiltinCommand::Unknown
            }
        }
        "/disconnect" => {
            let id = parts.get(1).and_then(|s| s.parse::<usize>().ok());
            BuiltinCommand::Disconnect { id }
        }
        "/reconnect" => {
            let id = parts.get(1).and_then(|s| s.parse::<usize>().ok());
            BuiltinCommand::Reconnect { id }
        }
        "/close" => {
            let id = parts.get(1).and_then(|s| s.parse::<usize>().ok());
            BuiltinCommand::Close { id }
        }
        "/list" => BuiltinCommand::List,
        "/reload" => BuiltinCommand::LoadReload,
        "/load" => {
            if parts.len() < 2 {
                return BuiltinCommand::Unknown;
            }
            if parts[1] == "reload" {
                BuiltinCommand::LoadReload
            } else {
                BuiltinCommand::Load {
                    path: parts[1].to_string(),
                }
            }
        }
        "/lua" => {
            let code = cmd.strip_prefix("/lua ").unwrap_or("").to_string();
            if code.is_empty() {
                BuiltinCommand::Unknown
            } else {
                BuiltinCommand::Lua { code }
            }
        }
        "/set" => {
            if parts.len() < 3 {
                BuiltinCommand::Unknown
            } else {
                BuiltinCommand::Set {
                    option: parts[1].to_string(),
                    value: parts[2].to_string(),
                }
            }
        }
        "/switch" | "/sw" => {
            if parts.len() < 2 {
                BuiltinCommand::Unknown
            } else {
                BuiltinCommand::Switch {
                    target: parts[1].to_string(),
                }
            }
        }
        "/all" => {
            let rest = cmd.strip_prefix("/all ").unwrap_or("").to_string();
            if rest.is_empty() {
                BuiltinCommand::Unknown
            } else {
                BuiltinCommand::All { cmd: rest }
            }
        }
        "/profile" => {
            if parts.len() < 2 {
                BuiltinCommand::Unknown
            } else {
                match parts[1] {
                    "load" => {
                        if parts.len() < 3 {
                            BuiltinCommand::Unknown
                        } else {
                            BuiltinCommand::Profile {
                                sub: ProfileSubcommand::Load {
                                    name: parts[2].to_string(),
                                },
                            }
                        }
                    }
                    "list" => BuiltinCommand::Profile {
                        sub: ProfileSubcommand::List,
                    },
                    _ => BuiltinCommand::Unknown,
                }
            }
        }
        _ => BuiltinCommand::Unknown,
    }
}

/// 解析分号分隔的命令，支持转义（\; 表示字面量分号）
pub(crate) fn split_commands(cmd: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut chars = cmd.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            // 转义字符：检查下一个字符
            if let Some(&next) = chars.peek() {
                if next == ';' {
                    // \; 表示字面量分号
                    current.push(';');
                    chars.next();
                } else {
                    // 其他情况保留反斜杠
                    current.push('\\');
                }
            } else {
                // 字符串末尾的反斜杠
                current.push('\\');
            }
        } else if c == ';' {
            // 分号：结束当前命令
            let trimmed = current.trim();
            if !trimmed.is_empty() {
                result.push(trimmed.to_string());
            }
            current.clear();
        } else {
            current.push(c);
        }
    }

    // 处理最后一个命令
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        result.push(trimmed.to_string());
    }

    result
}
/// 解析 /connect 命令参数，返回 (host, port)
pub(crate) fn parse_connect_args(parts: &[&str]) -> Option<(String, u16)> {
    if parts.len() < 3 {
        return None;
    }
    let (host, port) = if parts[2].contains(':') && parts.len() == 3 {
        let hp: Vec<&str> = parts[2].splitn(2, ':').collect();
        (hp[0], hp[1].parse::<u16>().unwrap_or(5555))
    } else {
        let p = if parts.len() > 3 {
            parts[3].parse::<u16>().ok()
        } else {
            None
        };
        (parts[2], p.unwrap_or(5555))
    };
    Some((host.to_string(), port))
}
/// 格式化 Lua 错误信息，将含路径的文本分行
pub(crate) fn format_lua_error(err: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for line in err.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("stack traceback:") {
            lines.push("stack traceback:".to_string());
        } else if !trimmed.is_empty() {
            lines.push(trimmed.to_string());
        }
    }
    if lines.is_empty() {
        lines.push(err.to_string());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_connect_args_host_port() {
        let parts: Vec<&str> = "/connect test mud.example.com 4000"
            .split_whitespace()
            .collect();
        let result = parse_connect_args(&parts);
        assert_eq!(result, Some(("mud.example.com".to_string(), 4000)));
    }

    #[test]
    fn test_parse_connect_args_host_colon_port() {
        let parts: Vec<&str> = "/connect test mud.example.com:4000"
            .split_whitespace()
            .collect();
        let result = parse_connect_args(&parts);
        assert_eq!(result, Some(("mud.example.com".to_string(), 4000)));
    }

    #[test]
    fn test_parse_connect_args_default_port() {
        let parts: Vec<&str> = "/connect test mud.example.com".split_whitespace().collect();
        let result = parse_connect_args(&parts);
        assert_eq!(result, Some(("mud.example.com".to_string(), 5555)));
    }

    #[test]
    fn test_parse_connect_args_invalid_port() {
        let parts: Vec<&str> = "/connect test mud.example.com abc"
            .split_whitespace()
            .collect();
        let result = parse_connect_args(&parts);
        assert_eq!(result, Some(("mud.example.com".to_string(), 5555)));
    }

    #[test]
    fn test_parse_connect_args_too_few() {
        let parts: Vec<&str> = "/connect test".split_whitespace().collect();
        let result = parse_connect_args(&parts);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_connect_args_empty() {
        let parts: Vec<&str> = "/connect".split_whitespace().collect();
        let result = parse_connect_args(&parts);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_connect_args_host_colon_invalid_port() {
        let parts: Vec<&str> = "/connect test host:abc".split_whitespace().collect();
        let result = parse_connect_args(&parts);
        assert_eq!(result, Some(("host".to_string(), 5555)));
    }

    #[test]
    fn test_parse_connect_args_host_colon_port_with_extra() {
        // host:port format with extra arg should use separate format
        let parts: Vec<&str> = "/connect test host:4000 extra".split_whitespace().collect();
        let result = parse_connect_args(&parts);
        // When len != 3 and contains ':', falls to else branch
        assert_eq!(result, Some(("host:4000".to_string(), 5555)));
    }
    // === parse_builtin_command 测试 ===

    #[test]
    fn test_parse_builtin_connect() {
        let cmd = parse_builtin_command("/connect test mud.example.com 4000");
        assert_eq!(
            cmd,
            BuiltinCommand::Connect {
                name: "test".to_string(),
                host: "mud.example.com".to_string(),
                port: 4000
            }
        );
    }

    #[test]
    fn test_parse_builtin_connect_colon_port() {
        let cmd = parse_builtin_command("/connect mymud host:5555");
        assert_eq!(
            cmd,
            BuiltinCommand::Connect {
                name: "mymud".to_string(),
                host: "host".to_string(),
                port: 5555
            }
        );
    }

    #[test]
    fn test_parse_builtin_connect_invalid() {
        let cmd = parse_builtin_command("/connect");
        assert_eq!(cmd, BuiltinCommand::Unknown);

        let cmd = parse_builtin_command("/connect test");
        assert_eq!(cmd, BuiltinCommand::Unknown);
    }

    #[test]
    fn test_parse_builtin_disconnect_with_id() {
        let cmd = parse_builtin_command("/disconnect 2");
        assert_eq!(cmd, BuiltinCommand::Disconnect { id: Some(2) });
    }

    #[test]
    fn test_parse_builtin_disconnect_no_id() {
        let cmd = parse_builtin_command("/disconnect");
        assert_eq!(cmd, BuiltinCommand::Disconnect { id: None });
    }

    #[test]
    fn test_parse_builtin_disconnect_invalid_id() {
        let cmd = parse_builtin_command("/disconnect abc");
        assert_eq!(cmd, BuiltinCommand::Disconnect { id: None });
    }

    #[test]
    fn test_parse_builtin_reconnect_with_id() {
        let cmd = parse_builtin_command("/reconnect 2");
        assert_eq!(cmd, BuiltinCommand::Reconnect { id: Some(2) });
    }

    #[test]
    fn test_parse_builtin_reconnect_no_id() {
        let cmd = parse_builtin_command("/reconnect");
        assert_eq!(cmd, BuiltinCommand::Reconnect { id: None });
    }

    #[test]
    fn test_parse_builtin_reconnect_invalid_id() {
        let cmd = parse_builtin_command("/reconnect abc");
        assert_eq!(cmd, BuiltinCommand::Reconnect { id: None });
    }

    #[test]
    fn test_parse_builtin_close_with_id() {
        let cmd = parse_builtin_command("/close 3");
        assert_eq!(cmd, BuiltinCommand::Close { id: Some(3) });
    }

    #[test]
    fn test_parse_builtin_close_no_id() {
        let cmd = parse_builtin_command("/close");
        assert_eq!(cmd, BuiltinCommand::Close { id: None });
    }

    #[test]
    fn test_parse_builtin_list() {
        let cmd = parse_builtin_command("/list");
        assert_eq!(cmd, BuiltinCommand::List);
    }

    #[test]
    fn test_parse_builtin_load() {
        let cmd = parse_builtin_command("/load /path/to/script.lua");
        assert_eq!(
            cmd,
            BuiltinCommand::Load {
                path: "/path/to/script.lua".to_string()
            }
        );
    }

    #[test]
    fn test_parse_builtin_load_reload() {
        let cmd = parse_builtin_command("/load reload");
        assert_eq!(cmd, BuiltinCommand::LoadReload);
    }

    #[test]
    fn test_parse_builtin_load_no_path() {
        let cmd = parse_builtin_command("/load");
        assert_eq!(cmd, BuiltinCommand::Unknown);
    }

    #[test]
    fn test_parse_builtin_lua() {
        let cmd = parse_builtin_command("/lua print('hello')");
        assert_eq!(
            cmd,
            BuiltinCommand::Lua {
                code: "print('hello')".to_string()
            }
        );
    }

    #[test]
    fn test_parse_builtin_lua_no_code() {
        let cmd = parse_builtin_command("/lua");
        assert_eq!(cmd, BuiltinCommand::Unknown);
    }

    #[test]
    fn test_parse_builtin_set_keep_command_on() {
        let cmd = parse_builtin_command("/set keep_command on");
        assert_eq!(
            cmd,
            BuiltinCommand::Set {
                option: "keep_command".to_string(),
                value: "on".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_builtin_set_keep_command_off() {
        let cmd = parse_builtin_command("/set keep_command off");
        assert_eq!(
            cmd,
            BuiltinCommand::Set {
                option: "keep_command".to_string(),
                value: "off".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_builtin_set_too_few_args() {
        let cmd = parse_builtin_command("/set");
        assert_eq!(cmd, BuiltinCommand::Unknown);
    }

    #[test]
    fn test_parse_builtin_unknown() {
        assert_eq!(parse_builtin_command("/unknown"), BuiltinCommand::Unknown);
        assert_eq!(parse_builtin_command(""), BuiltinCommand::Unknown);
        assert_eq!(parse_builtin_command("hello"), BuiltinCommand::Unknown);
    }

    #[test]
    fn test_parse_builtin_set_realtime_on() {
        let cmd = parse_builtin_command("/set realtime on");
        assert_eq!(
            cmd,
            BuiltinCommand::Set {
                option: "realtime".to_string(),
                value: "on".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_builtin_set_realtime_off() {
        let cmd = parse_builtin_command("/set realtime off");
        assert_eq!(
            cmd,
            BuiltinCommand::Set {
                option: "realtime".to_string(),
                value: "off".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_builtin_set_render_interval() {
        let cmd = parse_builtin_command("/set render_interval 500");
        assert_eq!(
            cmd,
            BuiltinCommand::Set {
                option: "render_interval".to_string(),
                value: "500".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_builtin_profile_load() {
        let result = parse_builtin_command("/profile load mychar");
        assert_eq!(
            result,
            BuiltinCommand::Profile {
                sub: ProfileSubcommand::Load {
                    name: "mychar".to_string()
                }
            }
        );
    }

    #[test]
    fn test_parse_builtin_profile_list() {
        let result = parse_builtin_command("/profile list");
        assert_eq!(
            result,
            BuiltinCommand::Profile {
                sub: ProfileSubcommand::List
            }
        );
    }

    #[test]
    fn test_parse_builtin_profile_no_subcommand() {
        assert_eq!(parse_builtin_command("/profile"), BuiltinCommand::Unknown);
    }

    #[test]
    fn test_parse_builtin_profile_unknown_subcommand() {
        assert_eq!(
            parse_builtin_command("/profile foo bar"),
            BuiltinCommand::Unknown
        );
    }

    #[test]
    fn test_parse_builtin_profile_load_no_name() {
        assert_eq!(
            parse_builtin_command("/profile load"),
            BuiltinCommand::Unknown
        );
    }

    #[test]
    fn test_parse_builtin_set_partial_arg() {
        // 只有 option 没有 value
        assert_eq!(
            parse_builtin_command("/set keep_command"),
            BuiltinCommand::Unknown
        );
    }
    #[test]
    fn test_split_commands_basic() {
        let result = split_commands("east;east;look");
        assert_eq!(result, vec!["east", "east", "look"]);
    }

    #[test]
    fn test_split_commands_single() {
        let result = split_commands("look");
        assert_eq!(result, vec!["look"]);
    }

    #[test]
    fn test_split_commands_empty() {
        let result = split_commands("");
        assert_eq!(result, Vec::<String>::new());
    }

    #[test]
    fn test_split_commands_escape_semicolon() {
        let result = split_commands("say hello\\;world");
        assert_eq!(result, vec!["say hello;world"]);
    }

    #[test]
    fn test_split_commands_mixed_escape() {
        let result = split_commands("east;say hi\\;there;west");
        assert_eq!(result, vec!["east", "say hi;there", "west"]);
    }

    #[test]
    fn test_split_commands_empty_parts() {
        let result = split_commands("east;;west");
        assert_eq!(result, vec!["east", "west"]);
    }

    #[test]
    fn test_split_commands_whitespace() {
        let result = split_commands("  east  ;  west  ");
        assert_eq!(result, vec!["east", "west"]);
    }

    #[test]
    fn test_split_commands_trailing_semicolon() {
        let result = split_commands("east;");
        assert_eq!(result, vec!["east"]);
    }

    #[test]
    fn test_split_commands_leading_semicolon() {
        let result = split_commands(";east");
        assert_eq!(result, vec!["east"]);
    }

    #[test]
    fn test_split_commands_backslash_not_before_semicolon() {
        let result = split_commands("east\\;west");
        assert_eq!(result, vec!["east;west"]);
    }

    #[test]
    fn test_split_commands_trailing_backslash() {
        let result = split_commands("east\\");
        assert_eq!(result, vec!["east\\"]);
    }

    #[test]
    fn test_split_commands_backslash_at_end() {
        let result = split_commands("say test\\");
        assert_eq!(result, vec!["say test\\"]);
    }

    #[test]
    fn test_format_lua_error_basic() {
        let result = format_lua_error("error: syntax error");
        assert_eq!(result, vec!["error: syntax error"]);
    }

    #[test]
    fn test_format_lua_error_stack_traceback() {
        let err = "stack traceback:\n\t[string \"line\"]:1: in main chunk";
        let result = format_lua_error(err);
        assert_eq!(
            result,
            vec!["stack traceback:", "[string \"line\"]:1: in main chunk"]
        );
    }

    #[test]
    fn test_format_lua_error_empty_lines() {
        let err = "line1\n\n\nline2";
        let result = format_lua_error(err);
        assert_eq!(result, vec!["line1", "line2"]);
    }

    #[test]
    fn test_format_lua_error_all_whitespace() {
        let result = format_lua_error("   \n  \n  ");
        assert_eq!(result, vec!["   \n  \n  "]);
    }

    #[test]
    fn test_format_lua_error_empty_string() {
        let result = format_lua_error("");
        assert_eq!(result, vec![""]);
    }

    #[test]
    fn test_format_lua_error_single_line() {
        let result = format_lua_error("just one line");
        assert_eq!(result, vec!["just one line"]);
    }
    #[test]
    fn test_parse_builtin_switch_by_name() {
        let result = parse_builtin_command("/switch char2");
        assert_eq!(
            result,
            BuiltinCommand::Switch {
                target: "char2".to_string()
            }
        );
    }

    #[test]
    fn test_parse_builtin_switch_alias() {
        let result = parse_builtin_command("/sw char2");
        assert_eq!(
            result,
            BuiltinCommand::Switch {
                target: "char2".to_string()
            }
        );
    }

    #[test]
    fn test_parse_builtin_switch_no_target() {
        let result = parse_builtin_command("/switch");
        assert_eq!(result, BuiltinCommand::Unknown);
    }

    #[test]
    fn test_parse_builtin_switch_alias_no_target() {
        let result = parse_builtin_command("/sw");
        assert_eq!(result, BuiltinCommand::Unknown);
    }

    #[test]
    fn test_parse_builtin_switch_by_number() {
        let result = parse_builtin_command("/switch 3");
        assert_eq!(
            result,
            BuiltinCommand::Switch {
                target: "3".to_string()
            }
        );
    }

    #[test]
    fn test_parse_builtin_connect_host_port_separate() {
        let result = parse_builtin_command("/connect char2 mud.example.com 6666");
        assert_eq!(
            result,
            BuiltinCommand::Connect {
                name: "char2".to_string(),
                host: "mud.example.com".to_string(),
                port: 6666,
            }
        );
    }
}
