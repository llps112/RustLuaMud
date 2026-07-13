#!/bin/bash
# RustLuaMud 一键初始化脚本
# 在 ~/RustLuaMud/ 下创建数据目录，下载预编译二进制，生成示例配置
#
# 用法：
#   bash <(curl -Ls https://raw.githubusercontent.com/llps112/RustLuaMud/main/scripts/bootstrap.sh)
#   bash <(curl -Ls ...) --nightly    # 下载 nightly 版
#
#   或从仓库内执行：
#   bash scripts/bootstrap.sh
#   bash scripts/bootstrap.sh --nightly

set -e

# --- 参数解析 ---
RELEASE_CHANNEL="stable"
if [ "$1" = "--nightly" ]; then
    RELEASE_CHANNEL="nightly"
fi

DATA_DIR="$HOME/RustLuaMud"
ARCH="linux-x86_64"

# 根据 channel 决定下载 URL
if [ "$RELEASE_CHANNEL" = "nightly" ]; then
    BINARY_URL="https://github.com/llps112/RustLuaMud/releases/download/nightly/RustLuaMud-${ARCH}.tar.gz"
    CHANNEL_LABEL="nightly"
else
    BINARY_URL="https://github.com/llps112/RustLuaMud/releases/latest/download/RustLuaMud-${ARCH}.tar.gz"
    CHANNEL_LABEL="stable"
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
echo "    $BINARY_URL"
curl -L "$BINARY_URL" 2>/dev/null | tar xz -C "$DATA_DIR"
echo "    ✓ 解压完成"

# ---- 3. 创建示例角色配置 ----
EXAMPLE_TOML="$DATA_DIR/profiles/example.toml"
if [ ! -f "$EXAMPLE_TOML" ]; then
    echo "==> 创建示例配置: $EXAMPLE_TOML"
    cat > "$EXAMPLE_TOML" << 'TOML'
# 角色连接配置
# 文件名即为角色标识，建议用角色名命名

# 连接信息
name = "角色名"
host = "ln.xkxmud.com"
port = 5555
encoding = "gbk"

# Lua 脚本路径（相对于程序运行目录）
script = "scripts/your_script.lua"

# 连接行为
auto_connect = true
auto_reconnect = true
reconnect_delay_secs = 5

# 登录凭证
username = "your_character_name"
password = "your_password"

# SOCKS5 代理（可选）
socks5_enable = false
socks5_host = "127.0.0.1"
socks5_port = 1080
socks5_username = ""
socks5_password = ""

# 实时渲染（可选）
realtime = true
render_interval = 1000
# log_rotation_count = 24
TOML
fi

# ---- 4. 完成 ----
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
echo "    │   └── mychar.toml     ← 在此创建你的角色配置"
echo "    ├── scripts/            ← 放入你的 Lua 脚本"
echo "    └── logs/               ← 日志文件自动生成"
echo ""
echo "  首次使用："
echo "    1. 编辑角色配置："
echo "       cp $DATA_DIR/profiles/example.toml $DATA_DIR/profiles/mychar.toml"
echo "       vim $DATA_DIR/profiles/mychar.toml"
echo "    2. 将游戏脚本放入 $DATA_DIR/scripts/ 目录"
echo "    3. 启动："
echo "       cd $DATA_DIR && ./RustLuaMud"
echo ""
