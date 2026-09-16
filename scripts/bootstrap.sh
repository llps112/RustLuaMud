#!/bin/bash
# RustLuaMud 一键初始化脚本
# 在 ~/RustLuaMud/ 下创建数据目录，下载预编译二进制，生成示例配置
#
# 用法：
#   bash <(curl -Ls https://raw.githubusercontent.com/llps112/RustLuaMud/main/scripts/bootstrap.sh)
#   bash <(curl -Ls ...) --nightly              # 下载 nightly 版
#   bash <(curl -Ls ...) --gitee                # 从 Gitee 镜像下载
#   bash <(curl -Ls ...) --nightly --gitee      # 从 Gitee 下载 nightly
#
#   或从仓库内执行：
#   bash scripts/bootstrap.sh
#   bash scripts/bootstrap.sh --nightly

set -e

# --- 参数解析 ---
RELEASE_CHANNEL="stable"
USE_GITEE=false
for arg in "$@"; do
    case "$arg" in
        --nightly) RELEASE_CHANNEL="nightly" ;;
        --gitee)   USE_GITEE=true ;;
    esac
done

DATA_DIR="$HOME/RustLuaMud"

# 自动检测架构
detect_arch() {
    local machine=$(uname -m)
    case "$machine" in
        x86_64|amd64)   echo "linux-x86_64" ;;
        i686|i386)      echo "linux-i686" ;;
        aarch64|arm64)  echo "linux-aarch64" ;;
        *)
            echo "!! 不支持的架构: $machine"
            echo "   支持的架构: x86_64, i686, aarch64"
            exit 1
            ;;
    esac
}

ARCH=$(detect_arch)

# 根据 channel 和镜像源决定下载 URL
if [ "$USE_GITEE" = true ]; then
    if [ "$RELEASE_CHANNEL" = "nightly" ]; then
        BINARY_URL="https://gitee.com/bai-yifei180/RustLuaMud/releases/download/nightly/RustLuaMud-${ARCH}.tar.gz"
        CHANNEL_LABEL="nightly (Gitee)"
    else
        LATEST_TAG=$(curl -sS "https://gitee.com/api/v5/repos/bai-yifei180/RustLuaMud/releases?per_page=100" | python3 -c "
import sys, json
releases = json.load(sys.stdin)
# 排除 nightly，按语义版本号排序取最高版本
tags = [r['tag_name'] for r in releases if r['tag_name'] != 'nightly']
sorted_tags = sorted(tags, key=lambda t: [int(x) for x in t.lstrip('v').split('.')])
print(sorted_tags[-1])
" 2>/dev/null || echo "")
        if [ -n "$LATEST_TAG" ]; then
            BINARY_URL="https://gitee.com/bai-yifei180/RustLuaMud/releases/download/${LATEST_TAG}/RustLuaMud-${ARCH}.tar.gz"
            CHANNEL_LABEL="stable (Gitee)"
        else
            # 降级到 nightly
            BINARY_URL="https://gitee.com/bai-yifei180/RustLuaMud/releases/download/nightly/RustLuaMud-${ARCH}.tar.gz"
            CHANNEL_LABEL="nightly (Gitee fallback)"
        fi
    fi
else
    if [ "$RELEASE_CHANNEL" = "nightly" ]; then
        BINARY_URL="https://github.com/llps112/RustLuaMud/releases/download/nightly/RustLuaMud-${ARCH}.tar.gz"
        CHANNEL_LABEL="nightly"
    else
        BINARY_URL="https://github.com/llps112/RustLuaMud/releases/latest/download/RustLuaMud-${ARCH}.tar.gz"
        CHANNEL_LABEL="stable"
    fi
fi

echo "=========================================="
echo "  RustLuaMud 一键初始化"
echo "  版本: $CHANNEL_LABEL"
echo "=========================================="
echo ""

# ---- 1. 创建数据目录 ----
if [ -f "$DATA_DIR" ]; then
    echo "==> 删除同名文件: $DATA_DIR（与目录名冲突）"
    rm -f "$DATA_DIR"
fi

echo "==> 创建数据目录: $DATA_DIR"
mkdir -p "$DATA_DIR"/{profiles,scripts,logs}

# ---- 2. 下载并解压二进制 ----
echo "==> 下载 $CHANNEL_LABEL 版二进制..."
echo "    地址: $BINARY_URL"
TMP_TAR=$(mktemp)
if ! curl -fsSL --http1.1 --retry 3 -o "$TMP_TAR" "$BINARY_URL"; then
    echo "!! 下载失败，请检查网络连接或确认 Release 是否存在"
    rm -f "$TMP_TAR"
    exit 1
fi
# 先删除旧二进制（运行中可删，Linux 仅 unlink 目录项，inode 保持打开）
rm -f "$DATA_DIR/RustLuaMud"
tar xzf "$TMP_TAR" -C "$DATA_DIR"
rm -f "$TMP_TAR"
echo "    ✓ 解压完成"

# ---- 3. 创建示例角色配置 ----
EXAMPLE_TOML="$DATA_DIR/profiles/example.toml"
if [ ! -f "$EXAMPLE_TOML" ]; then
    echo "==> 创建示例配置: $EXAMPLE_TOML"
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
# 留空则不注入，需手动输入或通过 Lua 脚本 setname/setpwd 设置
#
# 支持环境变量占位符，避免密码明文写在本文件里（防配置文件外流泄露）：
#   password = "${MUD_MYCHAR_PWD}"      # 整值形如 ${变量名} 时启动时从环境变量读取
#   Linux 写入 ~/.bashrc：export MUD_MYCHAR_PWD="真实密码"
#   Windows 设置一次永久生效：setx MUD_MYCHAR_PWD "真实密码"（新开终端窗口后生效）
#   环境变量未设置时该项按空处理并在启动时告警，不会把占位符当密码发送
#   密码本体恰好长 ${XXX} 样子时，用 $ 转义：password = "$${LITERAL}" 代表 ${LITERAL}
#
# 集中管理多个号的密码：推荐把变量写进 profiles/.env 文件（免去 setx/export），
# 用法详见同目录的 .env.example，复制为 .env 后填写即可
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
log_rotation_count = 24

# 命令发送速率限制（令牌桶 + 滑动窗口，可选）
# 限速由 Rust 侧统一保证，Lua 脚本只负责入队
#
# 服务端限速机制（LPC cmd.c）：
#   - cnt 计数器，每条命令 +1
#   - 每 2 秒 drain 40（clear_cmd_count: cnt -= 40）
#   - cnt > 60 → 雷劈/unconscious/踢出
#   - cnt > 20 → 小惩罚（扣气）
#   等效令牌桶：容量 60，每 2 秒补充 40
#
# 安全不等式（两条必须同时成立，否则长时间挂机仍会雷劈）：
#   1) cmds_per_sec ≤ 20                —— 长期速率不得超过 drain 速率
#   2) burst_size + 2×cmds_per_sec ≤ 60 —— 单次突发 + 随后 2 秒匀速的峰值
#   例：burst=15、cmds_per_sec=20 → 15 + 40 = 55 ≤ 60，留 5 条余量
#   注意：cmd_interval_ms 不构成长期速率上限，它只约束非突发模式的相邻间隔，
#   富余令牌会累积到 burst_size 再以突发形式花掉，长期速率由 cmds_per_sec 决定。
#   上述不等式在配置解析时自动校验，不满足时启动与 /profile load 都会告警。
#
# cmd_interval_ms: 突发用完后的最小发送间隔（毫秒，默认 50，范围 20~200）
#   50ms = 20条/秒 = 40条/2秒（drain周期内）
cmd_interval_ms = 50
#
# burst_size: 突发上限（默认 10，需满足 burst_size + 2×cmds_per_sec ≤ 60）
#   连线/空闲后允许连续发送的命令数（0ms 间隔），用完后进入匀速模式
burst_size = 15
#
# cmds_per_sec: 每秒令牌补充速率（默认 20）
#   控制长期平均发送速率，应与服务端 drain 速率匹配（40/2秒=20/秒）
#   切勿调高：超过 20 时 cnt 会逐周期净增，window_limit = 60 挡不住这种长期超速
cmds_per_sec = 20
#
# window_limit: 滑动窗口内允许的最大命令数（默认 60，范围 1~1000）
#   对应服务端雷劈阈值 3*CMDS_PER_TICK，不依赖与服务端 tick 对齐
#   封顶的是突发密度（半开区间），不封顶长期速率（长期速率由 cmds_per_sec 决定）
#   设为 60 时仍需上面两条不等式成立才安全；若想无条件兜底，可设为 40
#   （= 服务端每周期 drain 量），此时即使令牌桶参数配错，cnt 也恒 ≤ 40，
#   代价是突发吞吐下降
window_limit = 60
#
# window_duration_ms: 滑动窗口时长（毫秒，默认 2000，范围 2000~10000）
#   对应服务端 clear_cmd_count 的 2 秒 drain 周期，一般无需修改
#   不得低于 2000：短于 drain 周期时兜底会失效，运行时会被上调到 2000 并告警
window_duration_ms = 2000
TOML
fi

# ---- 3b. 创建凭据文件示例 ----
ENV_EXAMPLE="$DATA_DIR/profiles/.env.example"
if [ ! -f "$ENV_EXAMPLE" ]; then
    echo "==> 创建凭据示例: $ENV_EXAMPLE"
    cat > "$ENV_EXAMPLE" << 'ENVEOF'
# ============================================================
# RustLuaMud 凭据文件示例（.env）
# ============================================================
#
# 【作用】
#   把密码集中放在这一个文件里，角色配置（*.toml）中只写占位符
#   "${变量名}"。这样拷走/分享 profiles 目录下的 toml 时，
#   不会把密码一起带出去。
#
# 【使用步骤】
#   1. 把本文件复制为同目录下的 .env：
#        cp .env.example .env
#   2. 编辑 .env，按 变量名=密码 的格式一行写一个：
#        MUD_GBDOOR_PWD=我的真实密码
#   3. 在角色配置 gbdoor.toml 中引用：
#        password = "${MUD_GBDOOR_PWD}"
#   4. 启动客户端，登录时自动从 .env 取密码。
#
# 【格式规则】
#   - 每行一条：变量名=值，等号两边不要有空格（有也会被自动去除）
#   - 变量名只能以字母或下划线开头，由字母、数字、下划线组成
#   - 以 # 开头的行是注释，空行会被忽略
#   - 值里有空格时用引号包起来：MUD_PWD="my pass word"（引号会被自动去掉）
#   - 密码本身含 # 号无需转义，但整行若以 # 开头会被当作注释
#
# 【注意事项】
#   - 请以 UTF-8 编码保存（ANSI/GBK 等本地编码保存会导致整个文件被
#     拒绝加载，启动时会打印警告）
#   - .env 已被 .gitignore 排除，不会被提交到仓库；也不要把它
#     发给别人或上传网盘
#   - 系统环境变量（如已 export 过）优先于 .env：同名变量
#     已存在时 .env 里的值不会覆盖它
#   - 修改 .env 后需要重启客户端才生效
#   - 变量在 .env 里缺失或名字写错时，启动会打印警告，该角色
#     密码按空处理（停在登录界面手动输入），不会把占位符文本
#     当密码发给服务器
#
# ============================================================
# 下面是示例条目，复制为 .env 后改成你自己的：
# ============================================================

# gbdoor 角色的登录密码
MUD_GBDOOR_PWD=replace_with_real_password

# 第二个角色（变量名随意取，只要和 toml 里 ${...} 中的名字一致）
MUD_FKAKMA_PWD=replace_with_real_password

# 也可以给"用户名"建占位符（同样支持 username = "${...}" 写法）
# MUD_GBDOOR_USER=gbdoor

# SOCKS5 代理密码同理
# MUD_SOCKS5_PWD=proxy_password
ENVEOF
fi

# ---- 4. 创建示例脚本 ----
EXAMPLE_LUA="$DATA_DIR/scripts/example.lua"
if [ ! -f "$EXAMPLE_LUA" ]; then
    echo "==> 创建示例脚本: $EXAMPLE_LUA"
    cat > "$EXAMPLE_LUA" << 'LUA'
-- RustLuaMud 示例脚本
trigger("Are you using BIG5 code\\?", function()
    send("No")
    Note("已回答 BIG5 询问")
end)
trigger("^欢迎来到侠客行", function()
    Note("已进入游戏")
    send("look")
end)
trigger("^请输入你的名字", function()
    send(get("char_name"))
end)
trigger("^请输入你的密码", function()
    send(get("char_password"))
end)

alias("^lh$", function() send("look"); send("hp") end)
alias("^gs$", function() send("go south") end)
alias("^gn$", function() send("go north") end)
alias("^gw$", function() send("go west") end)
alias("^ge$", function() send("go east") end)
alias("^setname (.+)$", function(m)
    set("char_name", m[1]); Note("角色名已设置: " .. m[1])
end)
alias("^setpwd (.+)$", function(m)
    set("char_password", m[1]); Note("密码已设置")
end)

timer(60, function() send("hp") end)
Note("脚本已加载: example.lua")
LUA
fi

# ---- 5. 完成 ----
echo ""
echo "=========================================="
echo "  ✓ RustLuaMud 已就绪"
echo "=========================================="
echo ""
echo "  目录结构:"
echo "    $DATA_DIR/"
echo "    ├── RustLuaMud          ← 主程序"
echo "    ├── profiles/"
echo "    │   ├── example.toml    ← 示例配置"
echo "    │   ├── .env.example    ← 凭据模板（复制为 .env 可让密码留在 toml 之外）"
echo "    │   └── mychar.toml     ← 在此创建你的角色配置"
echo "    ├── scripts/"
echo "    │   └── example.lua     ← 示例脚本"
echo "    └── logs/               ← 日志文件自动生成"
echo ""
echo "  首次使用："
echo "    1. 编辑角色配置："
echo "       cp $DATA_DIR/profiles/example.toml $DATA_DIR/profiles/mychar.toml"
echo "       vim $DATA_DIR/profiles/mychar.toml"
echo "    2. 放入你的 Lua 脚本到 $DATA_DIR/scripts/"
echo "    3. 启动："
echo "       cd $DATA_DIR && ./RustLuaMud"
echo ""
echo "  可选（推荐）：不想把密码明文写进 toml 时"
echo "       cd $DATA_DIR/profiles && cp .env.example .env && vim .env"
echo "       在 .env 里写 MUD_MYCHAR_PWD=真实密码，再在 toml 里写："
echo "       password = \"\${MUD_MYCHAR_PWD}\""
echo ""
