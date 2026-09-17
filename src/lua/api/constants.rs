//! 常量表注册
//!
//! 对应拆分前 `api.rs` 中的「常量表」分节
//! （trigger_flag / alias_flag / timer_flag / custom_colour / sendto /
//! error_code / error_desc）。

use mlua::Result as LuaResult;

use crate::lua::types::LuaEngine;

impl LuaEngine {
    pub(super) fn register_constants_api(&mut self) -> LuaResult<()> {
        let lua = &self.lua;
        let globals = lua.globals();

        // ============================================================
        // 常量表
        // ============================================================

        // trigger_flag
        let trigger_flag = lua.create_table()?;
        trigger_flag.set("Enabled", 1i64)?;
        trigger_flag.set("OmitFromLog", 2i64)?;
        trigger_flag.set("OmitFromOutput", 4i64)?;
        trigger_flag.set("KeepEvaluating", 8i64)?;
        trigger_flag.set("IgnoreCase", 16i64)?;
        trigger_flag.set("RegularExpression", 32i64)?;
        trigger_flag.set("ExpandVariables", 512i64)?;
        trigger_flag.set("Replace", 1024i64)?;
        trigger_flag.set("LowercaseWildcard", 2048i64)?;
        trigger_flag.set("Temporary", 16384i64)?;
        trigger_flag.set("OneShot", 32768i64)?;
        globals.set("trigger_flag", trigger_flag)?;

        // alias_flag — 严格按 MushClient 官方定义
        // https://www.mushclient.com/scripts/function.php?name=AddAlias
        let alias_flag = lua.create_table()?;
        alias_flag.set("Enabled", 1i64)?;
        alias_flag.set("KeepEvaluating", 8i64)?;
        alias_flag.set("IgnoreAliasCase", 32i64)?;
        alias_flag.set("OmitFromLogFile", 64i64)?;
        alias_flag.set("RegularExpression", 128i64)?;
        alias_flag.set("ExpandVariables", 512i64)?;
        alias_flag.set("Replace", 1024i64)?;
        alias_flag.set("AliasSpeedWalk", 2048i64)?;
        alias_flag.set("AliasQueue", 4096i64)?;
        alias_flag.set("AliasMenu", 8192i64)?;
        alias_flag.set("Temporary", 16384i64)?;
        alias_flag.set("OneShot", 32768i64)?;
        globals.set("alias_flag", alias_flag)?;

        // timer_flag — 严格按 MushClient 官方定义
        let timer_flag = lua.create_table()?;
        timer_flag.set("Enabled", 1i64)?;
        timer_flag.set("AtTime", 2i64)?;
        timer_flag.set("OneShot", 4i64)?;
        timer_flag.set("TimerSpeedWalk", 8i64)?;
        timer_flag.set("TimerNote", 16i64)?;
        timer_flag.set("ActiveWhenClosed", 32i64)?;
        timer_flag.set("Replace", 1024i64)?;
        timer_flag.set("Temporary", 16384i64)?;
        globals.set("timer_flag", timer_flag)?;

        // custom_colour — 严格按 MushClient 官方定义 (lua_methods.cpp:7229)
        let custom_colour = lua.create_table()?;
        custom_colour.set("NoChange", -1i64)?;
        custom_colour.set("Custom1", 0i64)?;
        custom_colour.set("Custom2", 1i64)?;
        custom_colour.set("Custom3", 2i64)?;
        custom_colour.set("Custom4", 3i64)?;
        custom_colour.set("Custom5", 4i64)?;
        custom_colour.set("Custom6", 5i64)?;
        custom_colour.set("Custom7", 6i64)?;
        custom_colour.set("Custom8", 7i64)?;
        custom_colour.set("Custom9", 8i64)?;
        custom_colour.set("Custom10", 9i64)?;
        custom_colour.set("Custom11", 10i64)?;
        custom_colour.set("Custom12", 11i64)?;
        custom_colour.set("Custom13", 12i64)?;
        custom_colour.set("Custom14", 13i64)?;
        custom_colour.set("Custom15", 14i64)?;
        custom_colour.set("Custom16", 15i64)?;
        custom_colour.set("CustomOther", 16i64)?;
        globals.set("custom_colour", custom_colour)?;

        // sendto — 严格按 MushClient 官方定义 (lua_methods.cpp:7453)
        let sendto = lua.create_table()?;
        sendto.set("world", 0i64)?;
        sendto.set("command", 1i64)?;
        sendto.set("output", 2i64)?;
        sendto.set("status", 3i64)?;
        sendto.set("notepad", 4i64)?;
        sendto.set("notepadappend", 5i64)?;
        sendto.set("logfile", 6i64)?;
        sendto.set("notepadreplace", 7i64)?;
        sendto.set("commandqueue", 8i64)?;
        sendto.set("variable", 9i64)?;
        sendto.set("execute", 10i64)?;
        sendto.set("speedwalk", 11i64)?;
        sendto.set("script", 12i64)?;
        sendto.set("immediate", 13i64)?;
        sendto.set("scriptafteromit", 14i64)?;
        globals.set("sendto", sendto)?;

        // error_code — 严格按 MushClient 源码 lua_methods.cpp:7288 (error_codes[])
        // 注意：ePluginDoesNotSaveState 和 ePluginCouldNotSaveState 共用 30037（源码如此）
        let error_code = lua.create_table()?;
        error_code.set("eOK", 0i64)?;
        error_code.set("eWorldOpen", 30001i64)?;
        error_code.set("eWorldClosed", 30002i64)?;
        error_code.set("eNoNameSpecified", 30003i64)?;
        error_code.set("eCannotPlaySound", 30004i64)?;
        error_code.set("eTriggerNotFound", 30005i64)?;
        error_code.set("eTriggerAlreadyExists", 30006i64)?;
        error_code.set("eTriggerCannotBeEmpty", 30007i64)?;
        error_code.set("eInvalidObjectLabel", 30008i64)?;
        error_code.set("eScriptNameNotLocated", 30009i64)?;
        error_code.set("eAliasNotFound", 30010i64)?;
        error_code.set("eAliasAlreadyExists", 30011i64)?;
        error_code.set("eAliasCannotBeEmpty", 30012i64)?;
        error_code.set("eCouldNotOpenFile", 30013i64)?;
        error_code.set("eLogFileNotOpen", 30014i64)?;
        error_code.set("eLogFileAlreadyOpen", 30015i64)?;
        error_code.set("eLogFileBadWrite", 30016i64)?;
        error_code.set("eTimerNotFound", 30017i64)?;
        error_code.set("eTimerAlreadyExists", 30018i64)?;
        error_code.set("eVariableNotFound", 30019i64)?;
        error_code.set("eCommandNotEmpty", 30020i64)?;
        error_code.set("eBadRegularExpression", 30021i64)?;
        error_code.set("eTimeInvalid", 30022i64)?;
        error_code.set("eBadMapItem", 30023i64)?;
        error_code.set("eNoMapItems", 30024i64)?;
        error_code.set("eUnknownOption", 30025i64)?;
        error_code.set("eOptionOutOfRange", 30026i64)?;
        error_code.set("eTriggerSequenceOutOfRange", 30027i64)?;
        error_code.set("eTriggerSendToInvalid", 30028i64)?;
        error_code.set("eTriggerLabelNotSpecified", 30029i64)?;
        error_code.set("ePluginFileNotFound", 30030i64)?;
        error_code.set("eProblemsLoadingPlugin", 30031i64)?;
        error_code.set("ePluginCannotSetOption", 30032i64)?;
        error_code.set("ePluginCannotGetOption", 30033i64)?;
        error_code.set("eNoSuchPlugin", 30034i64)?;
        error_code.set("eNotAPlugin", 30035i64)?;
        error_code.set("eNoSuchRoutine", 30036i64)?;
        error_code.set("ePluginDoesNotSaveState", 30037i64)?;
        error_code.set("ePluginCouldNotSaveState", 30037i64)?;
        error_code.set("ePluginDisabled", 30039i64)?;
        error_code.set("eErrorCallingPluginRoutine", 30040i64)?;
        error_code.set("eCommandsNestedTooDeeply", 30041i64)?;
        error_code.set("eCannotCreateChatSocket", 30042i64)?;
        error_code.set("eCannotLookupDomainName", 30043i64)?;
        error_code.set("eNoChatConnections", 30044i64)?;
        error_code.set("eChatPersonNotFound", 30045i64)?;
        error_code.set("eBadParameter", 30046i64)?;
        error_code.set("eChatAlreadyListening", 30047i64)?;
        error_code.set("eChatIDNotFound", 30048i64)?;
        error_code.set("eChatAlreadyConnected", 30049i64)?;
        error_code.set("eClipboardEmpty", 30050i64)?;
        error_code.set("eFileNotFound", 30051i64)?;
        error_code.set("eAlreadyTransferringFile", 30052i64)?;
        error_code.set("eNotTransferringFile", 30053i64)?;
        error_code.set("eNoSuchCommand", 30054i64)?;
        error_code.set("eArrayAlreadyExists", 30055i64)?;
        error_code.set("eArrayDoesNotExist", 30056i64)?;
        error_code.set("eArrayNotEvenNumberOfValues", 30057i64)?;
        error_code.set("eImportedWithDuplicates", 30058i64)?;
        error_code.set("eBadDelimiter", 30059i64)?;
        error_code.set("eSetReplacingExistingValue", 30060i64)?;
        error_code.set("eKeyDoesNotExist", 30061i64)?;
        error_code.set("eCannotImport", 30062i64)?;
        error_code.set("eItemInUse", 30063i64)?;
        error_code.set("eSpellCheckNotActive", 30064i64)?;
        error_code.set("eCannotAddFont", 30065i64)?;
        error_code.set("ePenStyleNotValid", 30066i64)?;
        error_code.set("eUnableToLoadImage", 30067i64)?;
        error_code.set("eImageNotInstalled", 30068i64)?;
        error_code.set("eInvalidNumberOfPoints", 30069i64)?;
        error_code.set("eInvalidPoint", 30070i64)?;
        error_code.set("eHotspotPluginChanged", 30071i64)?;
        error_code.set("eHotspotNotInstalled", 30072i64)?;
        error_code.set("eNoSuchWindow", 30073i64)?;
        error_code.set("eBrushStyleNotValid", 30074i64)?;
        globals.set("error_code", error_code)?;

        // error_desc — 严格按 MushClient 源码 lua_methods.cpp
        // key 为数字（错误码），value 为描述字符串：error_desc[30022] → "Time given to AddTimer is invalid"
        let error_desc = lua.create_table()?;
        error_desc.set(0i64, "No error")?;
        error_desc.set(30001i64, "The world is already open")?;
        error_desc.set(
            30002i64,
            "The world is closed, this action cannot be performed",
        )?;
        error_desc.set(30003i64, "No name has been specified where one is required")?;
        error_desc.set(30004i64, "The sound file could not be played")?;
        error_desc.set(30005i64, "The specified trigger name does not exist")?;
        error_desc.set(30006i64, "Attempt to add a trigger that already exists")?;
        error_desc.set(30007i64, "The trigger \"match\" string cannot be empty")?;
        error_desc.set(30008i64, "The name of this object is invalid")?;
        error_desc.set(30009i64, "Script name is not in the script file")?;
        error_desc.set(30010i64, "The specified alias name does not exist")?;
        error_desc.set(30011i64, "Attempt to add an alias that already exists")?;
        error_desc.set(30012i64, "The alias \"match\" string cannot be empty")?;
        error_desc.set(30013i64, "Unable to open requested file")?;
        error_desc.set(30014i64, "Log file was not open")?;
        error_desc.set(30015i64, "Log file was already open")?;
        error_desc.set(30016i64, "Bad write to log file")?;
        error_desc.set(30017i64, "The specified timer name does not exist")?;
        error_desc.set(30018i64, "Attempt to add a timer that already exists")?;
        error_desc.set(30019i64, "Attempt to delete a variable that does not exist")?;
        error_desc.set(
            30020i64,
            "Attempt to use SetCommand with a non-empty command window",
        )?;
        error_desc.set(30021i64, "Bad regular expression syntax")?;
        error_desc.set(30022i64, "Time given to AddTimer is invalid")?;
        error_desc.set(30023i64, "Direction given to AddToMapper is invalid")?;
        error_desc.set(30024i64, "No items in mapper")?;
        error_desc.set(30025i64, "Option name not found")?;
        error_desc.set(30026i64, "New value for option is out of range")?;
        error_desc.set(30027i64, "Trigger sequence value invalid")?;
        error_desc.set(30028i64, "Where to send trigger text to is invalid")?;
        error_desc.set(
            30029i64,
            "Trigger label not specified/invalid for 'send to variable'",
        )?;
        error_desc.set(30030i64, "File name specified for plugin not found")?;
        error_desc.set(
            30031i64,
            "There was a parsing or other problem loading the plugin",
        )?;
        error_desc.set(30032i64, "Plugin is not allowed to set this option")?;
        error_desc.set(30033i64, "Plugin is not allowed to get this option")?;
        error_desc.set(30034i64, "Requested plugin is not installed")?;
        error_desc.set(30035i64, "Only a plugin can do this")?;
        error_desc.set(
            30036i64,
            "Plugin does not support that subroutine (subroutine not in script)",
        )?;
        error_desc.set(
            30037i64,
            "Plugin could not save state (eg. no state directory)",
        )?;
        error_desc.set(30039i64, "Plugin is currently disabled")?;
        error_desc.set(30040i64, "Could not call plugin routine")?;
        error_desc.set(30041i64, "Calls to \"Execute\" nested too deeply")?;
        error_desc.set(30042i64, "Unable to create socket for chat connection")?;
        error_desc.set(
            30043i64,
            "Unable to do DNS (domain name) lookup for chat connection",
        )?;
        error_desc.set(30044i64, "No chat connections open")?;
        error_desc.set(30045i64, "Requested chat person not connected")?;
        error_desc.set(
            30046i64,
            "General problem with a parameter to a script call",
        )?;
        error_desc.set(30047i64, "Already listening for incoming chats")?;
        error_desc.set(30048i64, "Chat session with that ID not found")?;
        error_desc.set(30049i64, "Already connected to that server/port")?;
        error_desc.set(30050i64, "Cannot get (text from the) clipboard")?;
        error_desc.set(30051i64, "Cannot open the specified file")?;
        error_desc.set(30052i64, "Already transferring a file")?;
        error_desc.set(30053i64, "Not transferring a file")?;
        error_desc.set(30054i64, "There is not a command of that name")?;
        error_desc.set(30055i64, "Attempt to create an array that already exists")?;
        error_desc.set(30056i64, "Attempt to delete an array that does not exist")?;
        error_desc.set(30057i64, "Array does not have an even number of values")?;
        error_desc.set(30058i64, "Imported with duplicates")?;
        error_desc.set(30059i64, "Bad delimiter")?;
        error_desc.set(30060i64, "Set replaced an existing value")?;
        error_desc.set(30061i64, "Key does not exist")?;
        error_desc.set(30062i64, "Cannot import")?;
        error_desc.set(
            30063i64,
            "Cannot delete trigger/alias/timer because it is executing a script",
        )?;
        error_desc.set(30064i64, "Spell checker is not active")?;
        error_desc.set(30065i64, "Cannot create requested font")?;
        error_desc.set(30066i64, "Invalid settings for pen parameter")?;
        error_desc.set(30067i64, "Bitmap image could not be loaded")?;
        error_desc.set(30068i64, "Image has not been loaded into window")?;
        error_desc.set(30069i64, "Number of points supplied is incorrect")?;
        error_desc.set(30070i64, "Point is not numeric")?;
        error_desc.set(30071i64, "Hotspot processing must all be in same plugin")?;
        error_desc.set(30072i64, "Hotspot has not been defined for this window")?;
        error_desc.set(30073i64, "Requested miniwindow does not exist")?;
        error_desc.set(30074i64, "Invalid settings for brush parameter")?;
        globals.set("error_desc", error_desc)?;

        Ok(())
    }
}
