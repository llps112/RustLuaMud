-- 测试脚本：验证 findstring 和 findstrlist 函数优化后的行为

-- 从 michen_system.lua 中提取的优化后的函数
function findstring(str,...)
    if str==nil then return false end
    -- 优化：添加短路返回，找到第一个匹配立即返回
    for _, v in ipairs{...} do
        if string.find(str, v) then
            return true
        end
    end
    return false
end

function findstrlist(str,list)
    if str==nil then return false end
    if list==nil then return false end
    -- 优化：添加短路返回，移除多余的 nil 检查
    for _, v in pairs(list) do
        if string.find(str, v) then
            return true
        end
    end
    return false
end

-- 测试框架
local test_count = 0
local pass_count = 0

local function test(name, actual, expected)
    test_count = test_count + 1
    if actual == expected then
        pass_count = pass_count + 1
        print("[PASS] " .. name)
    else
        print("[FAIL] " .. name)
        print("  Expected: " .. tostring(expected))
        print("  Actual: " .. tostring(actual))
    end
end

print("=== 开始测试 findstring ===")

-- findstring 测试用例
test("findstring: 单参数匹配", findstring("abcde", "a"), true)
test("findstring: 单参数不匹配", findstring("abcde", "x"), false)
test("findstring: 多参数第一个匹配", findstring("abcde", "a", "b", "c"), true)
test("findstring: 多参数中间匹配", findstring("abcde", "x", "c", "y"), true)
test("findstring: 多参数都不匹配", findstring("abcde", "x", "y", "z"), false)
test("findstring: 正则表达式匹配", findstring("abc123", "%d+"), true)
test("findstring: 正则表达式不匹配", findstring("abc", "%d+"), false)
test("findstring: str 为 nil", findstring(nil, "a"), false)
test("findstring: 无额外参数", findstring("abcde"), false)
test("findstring: 空字符串匹配", findstring("", "a"), false)
test("findstring: 空字符串匹配空字符串", findstring("", ""), true)

print("\n=== 开始测试 findstrlist ===")

-- findstrlist 测试用例
test("findstrlist: 列表第一个匹配", findstrlist("abcde", {"a", "b", "c"}), true)
test("findstrlist: 列表中间匹配", findstrlist("abcde", {"x", "c", "y"}), true)
test("findstrlist: 列表都不匹配", findstrlist("abcde", {"x", "y", "z"}), false)
test("findstrlist: 正则表达式匹配", findstrlist("abc123", {"%d+"}), true)
test("findstrlist: 正则表达式不匹配", findstrlist("abc", {"%d+"}), false)
test("findstrlist: str 为 nil", findstrlist(nil, {"a"}), false)
test("findstrlist: list 为 nil", findstrlist("abcde", nil), false)
test("findstrlist: 空列表", findstrlist("abcde", {}), false)
test("findstrlist: 空字符串匹配", findstrlist("", {"a"}), false)

print("\n=== 测试结果 ===")
print(string.format("总计: %d, 通过: %d, 失败: %d", test_count, pass_count, test_count - pass_count))

if pass_count == test_count then
    print("\n✓ 所有测试通过！")
    os.exit(0)
else
    print("\n✗ 部分测试失败！")
    os.exit(1)
end
