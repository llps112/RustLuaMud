luapath=string.match(GetInfo(35),"^.*\\")

include=function(str)
	dofile(luapath..str)
end
loadmod=function(str)
	local ok, err = pcall(function() include("class\\"..str) end)
	if ok then
		ColourNote("green", "black", str.." 模块加载成功")
	else
		ColourNote("red", "black", str.." 模块加载失败: "..tostring(err))
	end
end

me={}
me.charid=char_name
me.pwd=char_password
include("config_"..me.charid..".lua")

version="2.0.0"


if GetTriggerList() ~= nil then
	for k,v in pairs(GetTriggerList()) do
		DeleteTrigger(v)
	end
end
if GetAliasList() ~= nil then
	for k,v in pairs(GetAliasList()) do
		DeleteAlias(v)
	end
end
if GetTimerList() ~= nil then
	for k,v in pairs(GetTimerList()) do
		DeleteTimer(v)
	end
end

loadlua_list={
	"michen_var.lua",
	"michen_system.lua",
	"michen_alias.lua",
	"perform.lua",
	"always.lua",
	"check.lua",
	"common.lua",
	"gps_lib.lua",
	"gps.lua",
	"godie.lua",
	"kill.lua",
	"skills.lua",
	"xinfa.lua",
	"fj.lua",
	"ftb.lua",
	"chongmai.lua",
	"qzwd.lua",
	--"war.lua",           -- 回退时取消注释，注释下面两行
	"war_members.lua",
	"war_refactor.lua",
	"dummy.lua",
	--"pk.lua",
	"michen_yb.lua",
	"michen_mp_gb.lua",
	"michen_mp_dl.lua",
	"michen_mp_hs.lua",
	"michen_mp_qz.lua",
	"michen_mp_wd.lua",
	"michen_mp_sl.lua",
	"michen_mp_xs.lua",
	"michen_mp_em.lua",
	"michen_mp_gm.lua",
	"michen_mp_bt.lua",
	"michen_mp_xx.lua",
	"michen_mp_th.lua",
	"michen_mp_mj.lua",
	"basic_skills.lua",
	"michen_config.lua",
	"Entrance_table.lua",
  }

for i=1,table.getn(loadlua_list) do
	loadmod(loadlua_list[i])
end

-- 使用 UTF-8 安全的 ctonum 覆盖脚本中的 GBK 版本
-- 原有 ctonum 按 GBK 字节操作（每汉字2字节），但 MUD 输出已解码为 UTF-8（每汉字3字节）
-- 此处提供 UTF-8 安全的实现，按字符遍历解析中文数字
local function ctonum_utf8(str)
    local _nums = {
        ["一"] = 1, ["二"] = 2, ["三"] = 3, ["四"] = 4, ["五"] = 5,
        ["六"] = 6, ["七"] = 7, ["八"] = 8, ["九"] = 9
    }

    -- 按 UTF-8 字符边界分割字符串
    local chars = {}
    local i = 1
    local len = #str
    while i <= len do
        local byte = string.byte(str, i)
        if byte == nil then break end
        local char_len
        if byte < 0x80 then
            char_len = 1
        elseif byte < 0xC0 then
            -- continuation byte，跳过
            i = i + 1
            goto continue
        elseif byte < 0xE0 then
            char_len = 2
        elseif byte < 0xF0 then
            char_len = 3
        else
            char_len = 4
        end
        if i + char_len - 1 <= len then
            table.insert(chars, string.sub(str, i, i + char_len - 1))
        end
        i = i + char_len
        ::continue::
    end

    local char_count = #chars
    if char_count == 0 then
        return 0
    end

    local result = 0
    local unit = 1
    local wan = 1

    for i = char_count, 1, -1 do
        local ch = chars[i]
        if ch == "十" then
            unit = 10 * wan
            if i == 1 then
                result = result + unit
            elseif _nums[chars[i - 1]] == nil then
                result = result + unit
            end
        elseif ch == "百" then
            unit = 100 * wan
        elseif ch == "千" then
            unit = 1000 * wan
        elseif ch == "万" then
            unit = 10000 * wan
            wan = 10000
        else
            if _nums[ch] ~= nil then
                result = result + _nums[ch] * unit
            end
        end
    end

    return tonumber(result)
end

_G.ctonum = ctonum_utf8

-- 防发呆保护：防止 accessing 因 flood 丢包永久卡在 1
-- 场景：大量 flood 导致服务端"您目前的权限是："被丢弃，
-- login.dosomething1 无法将 accessing 置 0，防发呆永久失效
-- 修复：记录 accessing 置 1 的时间，超过 120 秒超时则自动复位
local _orig_atconnect = alias.atconnect
alias.atconnect = function()
    accessing_time = os.time()
    _orig_atconnect()
end
local _orig_timer60 = always_watch.timer60
always_watch.timer60 = function()
    -- 超时保护：accessing 卡住超过 240 秒（正常登录只需数秒），自动复位
    if accessing > 0 and accessing_time ~= nil and os.time() - accessing_time > 240 then
        print("[系统] 登录确认超时，自动恢复防发呆模式")
        accessing = 0
    end
    _orig_timer60()
end

-- OnConnect() 抽象接口：由 Rust 端 set_connected(true) 调用
-- Lua 脚本覆盖此函数实现自定义连接初始化逻辑
OnConnect = function()
    if alias and alias.atconnect then
        alias.atconnect()
    end
end

alias.initialize_variable()
notconnect=0
if IsConnected() then run("score;hp;cha;jifa") end
ColourNote("red","blue","欢迎使用XKX RBT FOR RustLuaMud VER."..version)
ColourNote("red","blue","本RBT由Shana@zj按照Michen版MUSHclient RBT大宝剑分支完整重写而成")
ColourNote("red","blue","目前实现了所有门派MP+FJ+YB+WAR+LW+XUE+CHONGMAI+XINFA+FTB功能")
ColourNote("red","blue","欢迎测试和提报BUG。")
--if not IsLogOpen() then OpenLog("",false) end
always_watch.timer_log()

-- 设置数据库文本编码为 GBK（xkxMAP.db 中房间名称等字段为 GBK 编码）
local _orig_connectDB = xkxGPS.connectDB
xkxGPS.connectDB = function()
	_orig_connectDB()
	if conn ~= nil then
		conn:set_gbk(true)
	end
end

DB_Import()		--调用gps_lib.lua中的函数，把数据库导入到table中使用

-- 读取保存的配置文件

Execute("/set_"..setting.."()")
print("载入"..setting.."模块成功")

if setting_resetidle>0 then resetidle=1 else resetidle=0 end

if per.roomno==1061 then
	per.roomno=1061
	per.npcid="zei"
	per.way="nu;sd"
end
if per.roomno==609 then
	per.roomno=609
	per.npcid="robber"
	per.way="w;e"
end
if per.roomno==1392 then
	per.roomno=1392
	per.npcid="robber"
	per.way="nd;su"
end
if per.roomno==936 then
	per.roomno=936
	per.npcid="robber"
	per.way="n;s"
end
if per.roomno==1369 then
	per.roomno=1369
	per.npcid="robber"
	per.way="s;n"
end
if per.roomno==1328 then
	per.roomno=1328
	per.npcid="robber"
	per.way="n;s"
end

if wantxuelit==1 then havegoldxuelit=1;xuelit=1 end
if wantxuelit==2 then havegoldxuelit=0;xuelit=1 end
if wantxuelit==3 then havegoldxuelit=0;xuelit=0 end

-----------------------------------------------------------------------------------
weapon_now=""

