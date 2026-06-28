#!/usr/bin/env lua
-- run 函数优化验证测试

-- Mock 依赖
local mock_execute_log = {}
local mock_allcmd = {}
local mock_cmd = {nums = 0, setnums = 20, settime = 0.5}
local mock_cmd_flag = true

-- Mock 函数
function IsConnected() return true end
function Connect() end
function openclass(name) end
function isopen(name) return false end

-- Mock rex 模块
local rex = {}
function rex.new(pattern)
    local self = {}
    function self:gmatch(str, callback)
        -- 简单实现：按分号分割
        for cmd in str:gmatch("[^;]+") do
            callback(cmd, 0)
        end
    end
    return self
end

-- Mock wait 模块
local wait = {}
function wait.make(fn)
    -- 记录函数但不立即执行，避免递归
    _G.last_wait_fn = fn
end
function wait.time(seconds)
    -- 记录等待时间用于验证
    _G.last_wait_time = seconds
end

-- Mock Execute
function Execute(cmd)
    table.insert(mock_execute_log, cmd)
end

-- Mock Split
function Split(str, sep)
    local result = {}
    for s in str:gmatch("[^" .. sep .. "]+") do
        table.insert(result, s)
    end
    return result
end

-- Mock alias
alias = {}
function alias.xxpfm(arg)
    table.insert(mock_execute_log, "xxpfm:" .. tostring(arg))
end

-- Mock aliasStepNum
aliasStepNum = {
    ["walk"] = 2,
    ["run"] = 3
}

-- Mock findstring
function findstring(str, pattern)
    return string.find(str, pattern) ~= nil
end

-- 初始化全局变量
runre = rex.new("([^;]+)")
allcmd = mock_allcmd
cmd = mock_cmd
cmd_flag = mock_cmd_flag

-- 加载优化后的 run 函数
run = function(str)
    local _cmd, _tb
    if not IsConnected() then Connect(); return else openclass("cmdcount") end
    if str == nil then 
        if #allcmd > 0 then
            str = "" 
            if isopen("gps_start") and allcmd[1] ~= "halt" and not isopen("kill") then
                table.insert(allcmd, "halt") 
            end
        else
            return
        end
    end
    runre:gmatch(str, function(m, t)
        if m ~= "" then 
            table.insert(allcmd, 1, m) 
        else
            print("放弃插入空白命令")
        end
    end)
    if #allcmd < 1 then return end
    if cmd.nums >= cmd.setnums and #allcmd > 0 then
        cmd_flag = false
        wait.make(function()
            wait.time(cmd.settime or 0.7)
            cmd_flag = true
            run("")
        end)
        return
    end
    while cmd.nums <= cmd.setnums and #allcmd > 0 do
        _cmd = table.remove(allcmd)
        if _cmd == nil then break end
        local _t = string.gsub(_cmd, "-", "")
        if string.find(_cmd, "xxpfm") then
            _tb = Split(_cmd, " ")
            pcall(alias.xxpfm, _tb[2])
        elseif aliasStepNum[_t] ~= nil then 
            Execute("/alias." .. _t .. "()")
            cmd.nums = cmd.nums + aliasStepNum[_t]
        else 
            Execute(_cmd)
            cmd.nums = cmd.nums + 1
        end
    end
    run()
end

-- 测试用例
print("=== run 函数优化验证测试 ===\n")

-- 测试 1: 基本命令执行
print("测试 1: 基本命令执行")
mock_execute_log = {}
mock_allcmd = {}
mock_cmd.nums = 0
allcmd = mock_allcmd
cmd = mock_cmd
run("look;score")
assert(#mock_execute_log == 2, "应该执行 2 个命令")
assert(mock_execute_log[1] == "look", "第一个命令应该是 look")
assert(mock_execute_log[2] == "score", "第二个命令应该是 score")
print("✓ 通过\n")

-- 测试 2: xxpfm 特殊处理
print("测试 2: xxpfm 特殊处理")
mock_execute_log = {}
mock_allcmd = {}
mock_cmd.nums = 0
allcmd = mock_allcmd
cmd = mock_cmd
run("xxpfm test_arg")
assert(#mock_execute_log == 1, "应该执行 1 个命令")
assert(mock_execute_log[1] == "xxpfm:test_arg", "应该调用 alias.xxpfm")
print("✓ 通过\n")

-- 测试 3: aliasStepNum 映射
print("测试 3: aliasStepNum 映射")
mock_execute_log = {}
mock_allcmd = {}
mock_cmd.nums = 0
allcmd = mock_allcmd
cmd = mock_cmd
run("walk")
assert(#mock_execute_log == 1, "应该执行 1 个命令")
assert(mock_execute_log[1] == "/alias.walk()", "应该调用 alias 函数")
assert(cmd.nums == 2, "cmd.nums 应该增加 2")
print("✓ 通过\n")

-- 测试 4: 限流逻辑
print("测试 4: 限流逻辑")
mock_execute_log = {}
mock_allcmd = {}
mock_cmd.nums = 20  -- 达到限制
mock_cmd.settime = 0.5
allcmd = mock_allcmd
cmd = mock_cmd
_G.last_wait_time = nil
_G.last_wait_fn = nil
run("look")
assert(_G.last_wait_fn ~= nil, "应该创建等待函数")
_G.last_wait_fn()  -- 手动执行等待函数
assert(_G.last_wait_time == 0.5, "应该使用 cmd.settime 配置的等待时间")
print("✓ 通过\n")

-- 测试 5: 默认等待时间
print("测试 5: 默认等待时间（cmd.settime 为 nil）")
mock_execute_log = {}
mock_allcmd = {}
mock_cmd.nums = 20
mock_cmd.settime = nil
allcmd = mock_allcmd
cmd = mock_cmd
_G.last_wait_time = nil
_G.last_wait_fn = nil
run("look")
assert(_G.last_wait_fn ~= nil, "应该创建等待函数")
_G.last_wait_fn()  -- 手动执行等待函数
assert(_G.last_wait_time == 0.7, "应该使用默认等待时间 0.7")
print("✓ 通过\n")

-- 测试 6: 空命令队列处理
print("测试 6: 空命令队列处理")
mock_execute_log = {}
mock_allcmd = {}
mock_cmd.nums = 0
allcmd = mock_allcmd
cmd = mock_cmd
run(nil)  -- str 为 nil 且队列为空
assert(#mock_execute_log == 0, "不应该执行任何命令")
print("✓ 通过\n")

-- 测试 7: 验证没有全局变量 n 污染
print("测试 7: 验证没有全局变量 n 污染")
_G.n = "original_value"
mock_allcmd = {}
mock_cmd.nums = 0
allcmd = mock_allcmd
cmd = mock_cmd
run("look")
assert(_G.n == "original_value", "全局变量 n 不应该被修改")
print("✓ 通过\n")

print("=== 所有测试通过 ===")
