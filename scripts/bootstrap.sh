#!/bin/bash
# RustLuaMud 初始化脚本
# 在 ~/RustLuaMud/ 下创建数据目录结构（profiles/ scripts/ logs/）
#
# 用法：
#   方式一（从仓库外初始化）：
#     bash <(curl -Ls https://raw.githubusercontent.com/llps112/RustLuaMud/main/scripts/bootstrap.sh)
#
#   方式二（从仓库内初始化）：
#     bash scripts/bootstrap.sh

set -e

DATA_DIR="$HOME/RustLuaMud"

echo "==> 创建数据目录: $DATA_DIR"
mkdir -p "$DATA_DIR"/{profiles,scripts,logs}

# 创建示例角色配置文件
EXAMPLE_TOML="$DATA_DIR/profiles/example.toml"
if [ ! -f "$EXAMPLE_TOML" ]; then
    echo "==> 创建示例角色配置: $EXAMPLE_TOML"
    cat > "$EXAMPLE_TOML" << 'TOML'
# 角色连接配置
# 文件名即为角色标识，建议用角色名命名
#
# 运行时新增此文件后，可在客户端内用以下命令加载（无需重启）：
#   /profile list              — 列出可用角色
#   /profile load <角色名>     — 加载并连接

# 连接信息
name = "角色名"
host = "ln.xkxmud.com"
port = 5555
encoding = "gbk"

# Lua 脚本路径（相对于程序运行目录）
script = "scripts/example.lua"

# 连接行为
auto_connect = true
auto_reconnect = true
reconnect_delay_secs = 5

# 登录凭证（启动时自动注入 Lua 变量 char_name / char_password）
# 留空则不注入，需手动输入或通过 Lua 脚本设置
username = "your_character_name"
password = "your_password"

# SOCKS5 代理（可选，不设置则直连）
socks5_enable = false
socks5_host = "127.0.0.1"
socks5_port = 1080
socks5_username = ""
socks5_password = ""

# 实时渲染开关（可选，默认 false，true 时忽略 render_interval 直接实时渲染）
realtime = true
# 渲染间隔（毫秒，0=实时渲染，默认 1000=1秒刷新一次）
render_interval = 1000

# 日志文件保留数量（可选，默认 24，即保留最近 24 个小时的日志文件）
# log_rotation_count = 24
TOML
fi

# 创建示例脚本
EXAMPLE_LUA="$DATA_DIR/scripts/example.lua"
if [ ! -f "$EXAMPLE_LUA" ]; then
    echo "==> 创建示例脚本: $EXAMPLE_LUA"
    cat > "$EXAMPLE_LUA" << 'LUA'
-- RustLuaMud Lua 脚本示例
-- 适用于侠客行 MUD (ln.xkxmud.com:5555)
--
-- 正则语法：Rust regex（与 PCRE 大部分兼容）
-- 回调参数：
--   trigger(pattern, callback)  → callback(matches)
--   alias(pattern, callback)    → callback(matches)  matches[0]=原始输入
--   timer(interval, callback)   → callback()

-- 自动回答 BIG5 编码询问
trigger("Are you using BIG5 code\\?", function()
    send("No")
    Note("已自动回答 BIG5 询问")
end)

-- 自动登录后执行命令
trigger("^欢迎来到侠客行", function()
    Note("已进入游戏")
    send("look")
end)

-- 被攻击时自动反击
trigger("^(.+) 向你攻击！", function(matches)
    Note("被攻击: " .. matches[1])
    send("fight " .. matches[1])
end)

-- 断线重连后自动登录
trigger("^请输入你的名字", function()
    send(get("char_name"))
end)
trigger("^请输入你的密码", function()
    send(get("char_password"))
end)

-- 方向别名
alias("^lh$", function() send("look"); send("hp") end)
alias("^gs$", function() send("go south") end)
alias("^gn$", function() send("go north") end)
alias("^gw$", function() send("go west") end)
alias("^ge$", function() send("go east") end)
alias("^gu$", function() send("go up") end)
alias("^gd$", function() send("go down") end)

-- 设置角色名和密码（用于自动登录）
alias("^setname (.+)$", function(matches)
    set("char_name", matches[1])
    Note("角色名已设置: " .. matches[1])
end)
alias("^setpwd (.+)$", function(matches)
    set("char_password", matches[1])
    Note("密码已设置")
end)

-- 每 60 秒自动查看状态
timer(60, function()
    send("hp")
end)

Note("脚本已加载: example.lua")
LUA
fi

echo ""
echo "=========================================="
echo "  RustLuaMud 数据目录已就绪"
echo "=========================================="
echo ""
echo "  目录位置: $DATA_DIR"
echo ""
echo "  目录结构:"
echo "    $DATA_DIR/"
echo "    ├── profiles/"
echo "    │   ├── example.toml   ← 示例配置（自动跳过，不会加载）"
echo "    │   └── mychar.toml    ← 在此创建你的角色配置"
echo "    ├── scripts/"
echo "    │   └── example.lua    ← 示例脚本"
echo "    └── logs/              ← 日志文件自动生成于此"
echo ""
echo "  下一步："
echo "    1. 将 RustLuaMud 二进制放入 $DATA_DIR"
echo "    2. 在 profiles/ 下创建角色 TOML 文件"
echo "    3. 放入你的 Lua 脚本到 scripts/"
echo "    4. 运行：cd $DATA_DIR && ./RustLuaMud"
echo ""
