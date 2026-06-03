-- RustLuaMud Lua 脚本示例
-- 适用于侠客行 MUD (ln.xkxmud.com:5555)

-- ========== 触发器 ==========

-- 自动回答 BIG5 编码询问
trigger("Are you using BIG5 code%?", function()
    send("No")
    log("自动回答 BIG5 询问")
end)

-- 自动登录后执行命令
trigger("^欢迎来到侠客行", function()
    log("已进入游戏")
    send("look")
end)

-- 被攻击时自动反击
trigger("^(.+) 向你攻击！", function(matches)
    log("被攻击: " .. matches[1])
    send("fight " .. matches[1])
end)

-- 经验获取提示
trigger("^你获得了 (.+) 点经验", function(matches)
    log("获得经验: " .. matches[1])
end)

-- 断线重连后自动登录
trigger("^请输入你的名字", function()
    local name = get("char_name")
    if name ~= "" then
        send(name)
        log("自动输入角色名: " .. name)
    end
end)

trigger("^请输入你的密码", function()
    local pwd = get("char_password")
    if pwd ~= "" then
        send(pwd)
        log("自动输入密码")
    end
end)

-- ========== 别名 ==========

-- lh = look + hp
alias("^lh$", function()
    send("look")
    send("hp")
end)

-- gs/gn/gw/ge = go south/north/west/east
alias("^gs$", function() send("go south") end)
alias("^gn$", function() send("go north") end)
alias("^gw$", function() send("go west") end)
alias("^ge$", function() send("go east") end)
alias("^gu$", function() send("go up") end)
alias("^gd$", function() send("go down") end)

-- sk = 查看技能
alias("^sk$", function() send("skills") end)
alias("^sc$", function() send("score") end)

-- 设置角色名和密码（用于自动登录）
alias("^setname (.+)$", function(matches)
    set("char_name", matches[1])
    log("角色名已设置: " .. matches[1])
end)

alias("^setpwd (.+)$", function(matches)
    set("char_password", matches[1])
    log("密码已设置")
end)

-- ========== 定时器 ==========

-- 每 60 秒自动查看状态
timer(60, function()
    send("hp")
end)

-- ========== 初始化 ==========

log("脚本已加载: example.lua")
log("可用别名: lh, gs, gn, gw, ge, gu, gd, sk, sc")
log("设置自动登录: setname <名字>, setpwd <密码>")
