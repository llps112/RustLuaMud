# Phase 1 测试方法文档

## 1. 测试环境准备

### 1.1 编译

```bash
cd /home/baiyf/RustLuaMud
cargo build --release
```

二进制文件位于 `target/release/rust-lua-mud`

### 1.2 本地模拟 MUD 服务器

由于需要真实 TCP 连接测试，在本地启动一个简易 echo 服务器模拟 MUD：

```bash
# 安装 socat（如果没有）
sudo apt install socat

# 启动一个简单的 echo 服务器，监听 4000 端口
# 收到任何数据后回显 + 发送欢迎信息
socat TCP-LISTEN:4000,reuseaddr,fork EXEC:"/bin/bash -c 'echo 欢迎来到MUD世界!; cat'"
```

或者用 Python 写一个更像 MUD 的模拟服务器：

```python
# fake_mud_server.py
import socketserver
import time
import random

messages = [
    "你看到一条蜿蜒的小路通向远方。",
    "一只野兔从草丛中窜出。",
    "你获得了 50 经验值。",
    "一位商人向你打招呼。",
    "你的内力恢复了。",
    "天空飘来一朵乌云。",
]

class MudHandler(socketserver.StreamRequestHandler):
    def handle(self):
        self.wfile.write(b"\r\n欢迎来到测试MUD世界!\r\n")
        self.wfile.write(b"请输入你的名字: \r\n")
        try:
            while True:
                line = self.rfile.readline()
                if not line:
                    break
                cmd = line.decode('utf-8', errors='ignore').strip()
                self.wfile.write(f"\r\n你输入了: {cmd}\r\n".encode('utf-8'))
                # 随机发送一些场景描述
                msg = random.choice(messages)
                self.wfile.write(f"\r\n{msg}\r\n".encode('utf-8'))
                self.wfile.write(b"> ")
                self.wfile.flush()
        except:
            pass

class ThreadedServer(socketserver.ThreadingTCPServer):
    allow_reuse_address = True

if __name__ == "__main__":
    server = ThreadedServer(("0.0.0.0", 4000), MudHandler)
    print("Fake MUD server listening on port 4000...")
    server.serve_forever()
```

```bash
python3 fake_mud_server.py
```

### 1.3 配置文件

修改 `configs/default.toml` 指向本地测试服务器：

```toml
[general]
scroll_buffer = 5000
log_dir = "logs"
log_rotation_size_mb = 10
log_rotation_count = 5

[[connections]]
name = "测试角色"
host = "127.0.0.1"
port = 4000
auto_connect = true
auto_reconnect = true
reconnect_delay_secs = 5
```

---

## 2. 功能测试用例

### 测试 1: 启动与配置加载

**步骤：**
1. 确保 `configs/default.toml` 存在且配置正确
2. 运行 `./target/release/rust-lua-mud`

**预期结果：**
- 终端切换到 TUI 模式（备用屏幕缓冲区）
- 状态栏显示：`[1]测试角色 ● 1/1 RustLuaMud`
- 输出区显示：`[系统] 连接 1 (测试角色) 已建立`
- 底部输入行显示绿色 `> ` 提示符和闪烁光标

**异常测试：**
- 删除 `configs/default.toml`，启动应显示警告但使用默认配置（无连接）
- 配置文件中 host 错误，启动应显示连接失败信息

### 测试 2: TCP 连接与数据接收

**步骤：**
1. 启动 fake_mud_server
2. 启动 rust-lua-mud
3. 观察是否收到欢迎信息

**预期结果：**
- 输出区显示 MUD 服务器发送的欢迎文本
- ANSI 转义序列被正确剥离（不显示乱码）
- 多行文本逐行显示

### 测试 3: 命令发送

**步骤：**
1. 在输入行输入 `hello`
2. 按 Enter

**预期结果：**
- 输出区回显 `> hello`
- 服务器收到命令并回复
- 回复内容显示在输出区
- 输入行清空，光标回到行首

### 测试 4: 输入行编辑

**步骤：**
1. 输入 `look`，然后按左箭头将光标移到 `l` 和 `o` 之间
2. 输入 `x`，观察变为 `lxook`
3. 按 Backspace，观察变为 `look`
4. 按 Home，光标到行首
5. 按 End，光标到行尾
6. 按 Delete（光标在行首时），删除 `l`

**预期结果：**
- 左右箭头正确移动光标
- 字符插入在光标位置
- Backspace 删除光标前字符
- Delete 删除光标后字符
- Home/End 正确跳转

### 测试 5: 命令历史

**步骤：**
1. 依次输入并发送 `look`、`hp`、`go north`
2. 按上箭头，应显示 `go north`
3. 再按上箭头，应显示 `hp`
4. 再按上箭头，应显示 `look`
5. 按下箭头，应显示 `hp`
6. 再按向下箭头，应显示 `go north`
7. 再按向下箭头，输入行清空

**预期结果：**
- 上下箭头正确浏览历史
- 选中历史条目后可直接发送或编辑

### 测试 6: 断线检测

**步骤：**
1. 连接正常后，关闭 fake_mud_server（Ctrl+C）
2. 观察客户端行为

**预期结果：**
- 输出区显示 `[系统] 连接 1 (测试角色) 已断开`
- 状态栏图标变为 `○`
- 显示 `[系统] 5 秒后尝试重连 测试角色...`

### 测试 7: 自动重连

**步骤：**
1. 断线后，重新启动 fake_mud_server
2. 等待自动重连

**预期结果：**
- 客户端自动重新连接
- 状态栏恢复为 `●`
- 收到新的欢迎信息

### 测试 8: 退出

**步骤：**
1. 按 Ctrl+C 或 Ctrl+D

**预期结果：**
- 程序立即退出
- 终端恢复正常模式（回到命令行）
- 无残留的 raw mode 或备用屏幕缓冲区

### 测试 9: 终端大小变化

**步骤：**
1. 调整终端窗口大小
2. 观察输出区和输入行是否正确重绘

**预期结果：**
- 状态栏、输出区、输入行正确适应新尺寸
- 无显示错乱

---

## 3. 稳定性测试

### 3.1 长时间运行测试

**步骤：**
1. 连接到真实 MUD 服务器或 fake_mud_server
2. 让客户端保持连接 4 小时以上
3. 定期检查内存占用

```bash
# 另一个终端监控内存
watch -n 60 'ps aux | grep rust-lua-mud | grep -v grep'
```

**预期结果：**
- RSS 内存占用稳定，不持续增长
- 连接保持正常
- 无 panic 或崩溃

### 3.2 高频数据测试

**步骤：**
1. 修改 fake_mud_server，高频发送数据（每秒 10-20 行）
2. 观察客户端是否流畅

```python
# 在 fake_mud_server 的 handle 方法中加入
import threading
def spam():
    while True:
        self.wfile.write(f"\r\n[{time.strftime('%H:%M:%S')}] 场景更新...\r\n".encode())
        self.wfile.flush()
        time.sleep(0.1)
threading.Thread(target=spam, daemon=True).start()
```

**预期结果：**
- 输出区持续更新，无明显卡顿
- 输入行仍可正常输入和发送命令
- 内存不持续增长

### 3.3 断线重连循环测试

**步骤：**
1. 编写脚本反复关闭和启动 fake_mud_server
2. 观察客户端重连行为

```bash
# 循环脚本
for i in $(seq 1 20); do
    echo "=== 第 $i 轮 ==="
    timeout 10 python3 fake_mud_server.py &
    sleep 15
    kill %1 2>/dev/null
    sleep 10
done
```

**预期结果：**
- 每次断线都能检测到
- 每次服务器恢复后都能重连成功
- 无 panic、无死锁、无内存泄漏

---

## 4. 性能基准

在 J1800 + 2GB 环境上的目标指标：

| 指标 | 目标值 | 测量方法 |
|------|--------|----------|
| 启动时间 | < 2 秒 | `time ./target/release/rust-lua-mud` |
| 单连接内存占用 | < 10MB | `ps aux` 查看 RSS |
| CPU 空闲占用 | < 1% | `top` 查看，连接空闲时 |
| CPU 活跃占用 | < 5% | `top` 查看，高频数据时 |
| 输入延迟 | < 50ms | 主观感受，按键到字符显示 |

---

## 5. 已知限制（Phase 1）

- ANSI 颜色被剥离而非渲染（Phase 2 完善）
- 仅前台连接渲染，后台连接数据暂不记录日志（Phase 4 完善）
- 自动重连逻辑尚未实现定时器触发（Phase 4 完善）
- 无 Lua 脚本支持（Phase 3 完善）
- 无滚动回看功能（PageUp/PageDown）
