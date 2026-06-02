-- RustLuaMud Lua 脚本示例

-- 注册触发器：匹配服务器输出
trigger("^你获得了 (.+) 经验值$", function(matches)
    log("获得经验: " .. matches[1])
end)

trigger("^(.+) 向你攻击！", function(matches)
    log("被攻击: " .. matches[1])
    send("fight " .. matches[1])
end)

-- 注册别名：简化输入
alias("^lh$", function()
    send("look")
    send("hp")
end)

alias("^gs$", function()
    send("go south")
end)

-- 定时器：每 30 秒检查一次状态
timer(30, function()
    send("hp")
end)

-- 初始化
log("脚本已加载: example.lua")
