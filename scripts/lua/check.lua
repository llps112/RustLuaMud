-- [MUSHCLIENT_API] 本文件为MUSHclient配套工具库,依赖MUSHclient内建全局变量:

-- [MUSHCLIENT_API] error_code - 错误代码表 (行22)

-- [MUSHCLIENT_API] error_desc - 错误描述表 (行23)

--

-- check.lua

--

-- ----------------------------------------------------------

-- return-code checker for MUSHclient functions that return error codes

-- ----------------------------------------------------------

--

--[[



Call for those MUSHclient functions that return a result code (like eOK).

Not all functions return such a code.



eg.



require "check

  check (SetVariable ("abc", "def"))  --> works ok

  check (SetVariable ("abc-", "def")) --> The name of this object is invalid



--]]



function check (result)

  if result ~= error_code.eOK then

    error (error_desc [result] or 

           string.format ("Unknown error code: %i", result), 

           2) -- error level - whoever called this function

  end -- if

end -- function check 



return check