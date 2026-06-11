# MUSHclient API 官方索引

> 来源：http://www.mushclient.com/scripts/function.php
> 保存日期：2026-06-11

| Function | Description | Version |
|---|---|---|
| [Accelerator](#) | Add or modify an accelerator key | 3.53 |
| [AcceleratorList](#) | List defined accelerators | 3.53 |
| [AcceleratorTo](#) | Add or modify an accelerator key - with "Send To" parameter | 4.27 |
| [Activate](#) | Activates the world window | |
| [ActivateClient](#) | Activates the main MUSHclient window | 3.48 |
| [ActivateNotepad](#) | Activates a notepad window | |
| [AddAlias](#) | Adds an alias | |
| [AddFont](#) | Adds a custom font for use by MUSHclient | 4.32 |
| [AddMapperComment](#) | Adds a comment to the auto-map sequence | |
| [AddSpellCheckWord](#) | Adds a word to the user spell check dictionary | 3.74 |
| [AddTimer](#) | Adds a timer | |
| [AddToMapper](#) | Adds a mapping direction to the auto-map sequence | |
| [AddTrigger](#) | Adds a trigger | |
| [AddTriggerEx](#) | Adds a trigger - extended arguments | 3.18 |
| [AdjustColour](#) | Adjust an RGB colour | 3.41 |
| [ANSI](#) | Generates an ANSI colour sequence | 3.37 |
| [AnsiNote](#) | Make a note in the output window from text with ANSI colour codes imbedded | 3.37 |
| [AppendToNotepad](#) | Appends text to a notepad window | |
| [ArrayClear](#) | Clears an array | 3.46 |
| [ArrayCount](#) | Returns the number of arrays | 3.46 |
| [ArrayCreate](#) | Creates an array | 3.46 |
| [ArrayDelete](#) | Deletes an array | 3.46 |
| [ArrayDeleteKey](#) | Deletes a key/value pair from an array | 3.46 |
| [ArrayExists](#) | Tests to see if the specified array exists | 3.46 |
| [ArrayExport](#) | Exports values from an array into a single string | 3.46 |
| [ArrayExportKeys](#) | Exports keys from an array into a single string | 3.46 |
| [ArrayGet](#) | Gets the value of an array item | 3.46 |
| [ArrayGetFirstKey](#) | Gets the key of the first element in the array (if any) | 3.46 |
| [ArrayGetLastKey](#) | Gets the key of the last element in the array (if any) | 3.46 |
| [ArrayImport](#) | Imports values into an array from a single string | 3.46 |
| [ArrayKeyExists](#) | Tests to see if the specified array key exists | 3.46 |
| [ArrayListAll](#) | Gets the list of arrays | 3.46 |
| [ArrayListKeys](#) | Gets the list of all the keys in an array | 3.46 |
| [ArrayListValues](#) | Gets the list of all the values in an array | 3.46 |
| [ArraySet](#) | Sets the value of an array item | 3.46 |
| [ArraySize](#) | Returns the number of elements in a specified array | 3.46 |
| [Base64Decode](#) | Takes a base-64 encoded string and decodes it | 3.22 |
| [Base64Encode](#) | Encodes a string using base-64 encoding | 3.22 |
| [BlendPixel](#) | Blends a single pixel with another, using a specified blending mode | 4.36 |
| [BoldColour](#) | Gets/sets the RGB colour for one of the 8 ANSI bold colours | |
| [Bookmark](#) | Sets of clears a bookmark on the nominated line | 4.83 |
| [BroadcastPlugin](#) | Broadcasts a message to all installed plugins | 3.67 |
| [CallPlugin](#) | Calls a routine in a plugin | 3.23 |
| [ChangeDir](#) | Changes the MUSHclient working directory | 3.80 |
| [CloseLog](#) | Closes the log file | |
| [CloseNotepad](#) | Closes a notepad window | 3.29 |
| [ColourNameToRGB](#) | Converts a named colour to a RGB colour code | 3.22 |
| [ColourNote](#) | Sends a message to the output window in specified colours | 3.23 |
| [ColourTell](#) | Sends a message to the output window in specified colours - not terminated by a newline | 3.23 |
| [Connect](#) | Connects the world to the MUD server | |
| [CreateGUID](#) | Creates a GUID - Global Unique Identifier | 3.23 |
| [CustomColourBackground](#) | Sets the RGB value for the background of a custom colour | |
| [CustomColourText](#) | Sets the RGB value for the text of a custom colour | |
| [DatabaseChanges](#) | Returns a count of the changes to the database by the most recent SQL statement | 4.40 |
| [DatabaseClose](#) | Closes an SQLite database | 4.40 |
| [DatabaseColumnName](#) | Find the name of a specified column returned by an SQL statement | 4.40 |
| [DatabaseColumnNames](#) | Return a table of all the columns returned by an SQL statement | 4.40 |
| [DatabaseColumns](#) | Find how many columns will be returned by an SQL statement | 4.40 |
| [DatabaseColumnText](#) | Returns the contents of an SQL column, as text | 4.40 |
| [DatabaseColumnType](#) | Returns the type of data in an SQL column | 4.40 |
| [DatabaseColumnValue](#) | Returns the contents of an SQL column, as text, float, integer, or null | 4.40 |
| [DatabaseColumnValues](#) | Returns the contents of all the SQL columns after a step | 4.40 |
| [DatabaseError](#) | Returns an English string describing the most recent SQL error | 4.40 |
| [DatabaseExec](#) | Executes SQL code against an SQLite database | 4.40 |
| [DatabaseFinalize](#) | Finalizes (wraps up) a previously-prepared SQL statement | 4.40 |
| [DatabaseGetField](#) | Returns a single field from an SQL database | 4.65 |
| [DatabaseInfo](#) | Returns information about a database | 4.40 |
| [DatabaseLastInsertRowid](#) | Returns the most recently automatically allocated database key | 4.40 |
| [DatabaseList](#) | Lists all databases | 4.40 |
| [DatabaseOpen](#) | Opens an SQLite database | 4.40 |
| [DatabasePrepare](#) | Prepares an SQL statement for execution | 4.40 |
| [DatabaseReset](#) | Resets a previously-prepared SQL statement to the start | 4.40 |
| [DatabaseStep](#) | Executes a previously-prepared SQL statement | 4.40 |
| [DatabaseTotalChanges](#) | Returns a count of the total changes to the database | 4.40 |
| [Debug](#) | Displays debugging information about the world | |
| [DeleteAlias](#) | Deletes an alias | |
| [DeleteAliasGroup](#) | Deletes a group of aliases | 3.29 |
| [DeleteAllMapItems](#) | Deletes the all items from the auto-mapper sequence | |
| [DeleteCommandHistory](#) | Deletes command history list | |
| [DeleteGroup](#) | Deletes a group of triggers, aliases and timers | 3.29 |
| [DeleteLastMapItem](#) | Deletes the most recently-added item from the auto-mapper sequence | |
| [DeleteLines](#) | Clears some recent lines from the output window | 3.76 |
| [DeleteOutput](#) | Clears all output from the output window | |
| [DeleteTemporaryAliases](#) | Deletes all temporary aliases | 3.18 |
| [DeleteTemporaryTimers](#) | Deletes all temporary timers | 3.18 |
| [DeleteTemporaryTriggers](#) | Deletes all temporary triggers | 3.18 |
| [DeleteTimer](#) | Deletes a timer | |
| [DeleteTimerGroup](#) | Deletes a group of timers | 3.29 |
| [DeleteTrigger](#) | Deletes a trigger | |
| [DeleteTriggerGroup](#) | Deletes a group of triggers | 3.29 |
| [DeleteVariable](#) | Deletes a variable | |
| [DiscardQueue](#) | Discards the speed walk queue | |
| [Disconnect](#) | Disconnects the world from the MUD server | |
| [DoAfter](#) | Adds a one-shot, temporary timer - simplified interface | 3.18 |
| [DoAfterNote](#) | Adds a one-shot, temporary note timer - simplified interface | 3.18 |
| [DoAfterSpecial](#) | Adds a one-shot, temporary, timer to carry out some special action | 3.35 |
| [DoAfterSpeedWalk](#) | Adds a one-shot, temporary speedwalk timer - simplified interface | 3.18 |
| [DoCommand](#) | Queues a MUSHclient menu command | 3.39 |
| [EchoInput](#) | A flag to indicate whether we are echoing command input to the output window | |
| [EditDistance](#) | Returns the Levenshtein Edit Distance between two words | 3.82 |
| [EnableAlias](#) | Enables or disables an alias | |
| [EnableAliasGroup](#) | Enables/disables a group of aliases | 3.27 |
| [EnableGroup](#) | Enables/disables a group of triggers, aliases and timers | 3.27 |
| [EnableMapping](#) | Enables or disables the auto-mapper | 3.47 |
| [EnablePlugin](#) | Enables or disables the specified plugin | |
| [EnableTimer](#) | Enables or disables an timer | |
| [EnableTimerGroup](#) | Enables/disables a group of timers | 3.27 |
| [EnableTrigger](#) | Enables or disables a trigger | |
| [EnableTriggerGroup](#) | Enables/disables a group of triggers | 3.27 |
| [ErrorDesc](#) | Converts a MUSHclient script error code into an human-readable description | 3.68 |
| [EvaluateSpeedwalk](#) | Evaluates a speed walk string | |
| [Execute](#) | Executes a command as if you had typed it into the command window | 3.35 |
| [ExportXML](#) | Exports a world item in XML format | 3.41 |
| [FilterPixel](#) | Performs a filtering operation on one pixel | 4.36 |
| [FixupEscapeSequences](#) | Converts "escape sequences" like \\t to their equivalent codes | |
| [FixupHTML](#) | Fixes up text for writing as HTML | |
| [FlashIcon](#) | Flashes the MUSHclient icon on the Windows taskbar | 4.41 |
| [FlushLog](#) | Flushes the log file to disk | 3.82 |
| [GenerateName](#) | Generates a random character name | |
| [GetAlias](#) | Gets details about an alias | |
| [GetAliasInfo](#) | Gets details about an alias | |
| [GetAliasList](#) | Gets the list of aliases | |
| [GetAliasOption](#) | Gets the value of a named alias option | 3.29 |
| [GetAliasWildcard](#) | Returns the contents of the specified wildcard for the named alias | 3.48 |
| [GetAlphaOption](#) | Gets the value of a named alpha option | |
| [GetBackgroundImage](#) | Gets background image info | 4.37 |
| [GetBackgroundImageInfo](#) | Gets background image info | 4.36 |
| [GetBetaOption](#) | Gets the value of a named beta option | |
| [GetClipboard](#) | Gets the contents of the Windows clipboard | 3.22 |
| [GetCommand](#) | Gets the current command from the command window | |
| [GetCommandHistory](#) | Gets the most recent command from the command history | |
| [GetConnectDuration](#) | Gets the duration of the MUD connection | 4.70 |
| [GetCurrentRoom](#) | Gets the current room number | 4.37 |
| [GetEntity](#) | Gets the text for a named MXP entity | |
| [GetEscape](#) | Returns the escape character in effect for the command window | |
| [GetFont](#) | Gets font information | 3.22 |
| [GetFontList](#) | Gets a list of available fonts | 3.23 |
| [GetForegroundImage](#) | Gets foreground image info | 4.37 |
| [GetForegroundImageInfo](#) | Gets foreground image info | 4.36 |
| [GetGammaOption](#) | Gets the value of a named gamma option | |
| [GetGlobalOption](#) | Gets the value of a named global option | |
| [GetImageInfo](#) | Gets image dimension info | 4.36 |
| [GetImmediate](#) | Returns the "immediate" script | |
| [GetInfo](#) | Gets information about the current world | |
| [GetLinesInBuffer](#) | Gets the number of lines in the output buffer | 4.66 |
| [GetLogFile](#) | Gets information about the log file | |
| [GetMapInfo](#) | Gets the auto-mapper attributes | 4.37 |
| [GetMember](#) | Gets the group number for a group name (MUSHreader) | |
| [GetMemberInfo](#) | Gets info about a MUSHreader member | |
| [GetMemberName](#) | Gets the group name for a group number (MUSHreader) | |
| [GetMenu](#) | Gets the context menu on a notepad | 3.66 |
| [GetModuleInfo](#) | Gets information about a loaded module | 4.71 |
| [GetModuleName](#) | Gets the file name for a loaded lua module | 4.71 |
| [GetNotepad](#) | Gets the title and contents of a notepad window | 3.29 |
| [GetNotepadList](#) | Gets the list of notepad windows | 3.29 |
| [GetOption](#) | Gets the value of a named world option | |
| [GetPluginInfo](#) | Gets details about a specified plugin | 3.23 |
| [GetPluginList](#) | Lists all plugins | 3.38 |
| [GetPluginOption](#) | Gets the value of a named plugin option | |
| [GetPluginTriggerInfo](#) | Gets details about a named trigger for a specified plugin | |
| [GetPluginVariable](#) | Gets a plugin variable | 3.52 |
| [GetPopupMenuItem](#) | Gets info about the specified popup menu item | 4.21 |
| [GetQueue](#) | Gets the contents of the speed walk queue | |
| [GetRGBColour](#) | Gets the RGB colour for one of the 8 standard colours | 3.22 |
| [GetScriptTime](#) | Gets the time that scripts used | 4.65 |
| [GetStyleInfo](#) | Gets information about a style run in the output buffer | 3.64 |
| [GetSystemIcon](#) | Gets icon information | 4.54 |
| [GetSystemVariable](#) | Gets the value of a named system variable | |
| [GetTempVariable](#) | Gets a temporary variable | 4.60 |
| [GetTimer](#) | Gets details about a timer | |
| [GetTimerInfo](#) | Gets details about a timer | |
| [GetTimerList](#) | Gets the list of timers | |
| [GetTimerOption](#) | Gets the value of a named timer option | |
| [GetTimerVariable](#) | Gets a timer variable | 4.60 |
| [GetTrigger](#) | Gets details about a named trigger | |
| [GetTriggerInfo](#) | Gets details about a named trigger | |
| [GetTriggerList](#) | Gets the list of triggers | |
| [GetTriggerOption](#) | Gets the value of a named trigger option | |
| [GetTriggerWildcard](#) | Returns the contents of the specified wildcard for the named trigger | |
| [GetVariable](#) | Gets a variable | |
| [GetWorld](#) | Gets the world window handle and more | 3.39 |
| [GetWorldId](#) | Gets the unique World ID | 3.67 |
| [GoNext](#) | Forces the action to go to the next command in the command queue | |
| [Help](#) | Shows help for a MUSHclient function | |
| [HelpAbout](#) | Shows the About box | |
| [HelpCommands](#) | Shows the commands help | |
| [HelpKeys](#) | Shows the keys help | |
| [HelpMacros](#) | Shows the macros help | |
| [HelpOptions](#) | Shows the options help | |
| [HelpPlugins](#) | Shows the plugins help | |
| [HelpShortcuts](#) | Shows the shortcuts help | |
| [HelpTopics](#) | Shows the help topics that are available | |
| [HelpTrouble](#) | Shows the trouble shooting help | |
| [Hotlink](#) | Processes a hot link action | |
| [HTML](#) | Processes HTML for display in the output window | |
| [Hyperlink](#) | Processes a hyper link action | |
| [ImageCache](#) | Caches an image | 4.36 |
| [ImageCacheList](#) | Lists the cached images | 4.36 |
| [ImageInfo](#) | Gets info about a file | 4.36 |
| [ImportXML](#) | Imports a world item from XML | 3.41 |
| [Info](#) | Displays information about the world | |
| [InfoBox](#) | Displays an information box | |
| [Input](#) | Gets the current command from the command window | |
| [IsAlias](#) | Tests to see if an alias exists | |
| [IsConnected](#) | Tests to see if the world is connected to the MUD | |
| [IsDown](#) | | |
| [IsPluginInstalled](#) | Tests to see if a plugin is installed | 3.67 |
| [IsTimer](#) | Tests to see if a timer exists | |
| [IsTrigger](#) | Tests to see if a trigger exists | |
| [IsUTF8](#) | Tests to see if the input is valid UTF-8 | 4.80 |
| [LoadPlugin](#) | Loads a plugin | 3.67 |
| [LogFile](#) | Starts logging to a log file | |
| [LogFileClose](#) | Stops logging | 3.82 |
| [LogFileFlush](#) | Flushes the log file to disk | 3.82 |
| [LogFileOpen](#) | Opens a log file | 3.82 |
| [LogFileWrite](#) | Writes to the log file | 3.82 |
| [MapperAddRoom](#) | Adds a room to the automapper database | 4.37 |
| [MapperDeleteRoom](#) | Deletes a room from the automapper database | 4.37 |
| [MapperGetRoom](#) | Gets info about an automapper room | 4.37 |
| [MapperGetRoomExits](#) | Gets exit info for an automapper room | 4.37 |
| [MapperSetRoom](#) | Sets the info for an automapper room | 4.37 |
| [Message](#) | Displays a message in a message box | |
| [MoveFontEdge](#) | Moves the font edge | 4.36 |
| [MoveFontEdgeEx](#) | Moves the font edge (extended) | 4.36 |
| [MoveMainWindow](#) | Moves the main window | 4.39 |
| [MoveWorldWindow](#) | Moves the world window | 4.39 |
| [MultiNode](#) | Sends text over multiple lines | |
| [Name](#) | Returns the world name | |
| [Note](#) | Sends a message to the output window | |
| [NoteBackgroundColour](#) | Sets the background colour for subsequent Note/ColourNote calls | 3.79 |
| [NoteColour](#) | Sets the colour for subsequent Note/ColourNote calls | 3.79 |
| [NoteFormat](#) | Sets the formatting for subsequent Note/ColourNote calls | 3.79 |
| [NoteStyle](#) | Allows UTF-8 text output from within a plugin | 4.77 |
| [OpenLog](#) | Starts logging to a log file | |
| [Paste](#) | Pastes the clipboard contents to the MUD (as input) | 3.22 |
| [PlaySound](#) | Plays a sound file | |
| [PluginSupports](#) | Queries what a plugin supports | |
| [RaiseMainWindow](#) | Raises the main window | |
| [RaiseWorld](#) | Raises the world window | |
| [ReadApplicationLog](#) | Reads a line from the application log | 3.74 |
| [Redraw](#) | Forces a redraw of the output window | |
| [ReloadPlugin](#) | Reloads a plugin | 3.52 |
| [RemoveAllFonts](#) | Removes all custom fonts | 4.32 |
| [RemoveFont](#) | Removes a custom font | 4.32 |
| [RemoveImage](#) | Removes an image from the image cache | 4.36 |
| [RemoveMenu](#) | Removes a previously-added menu item | |
| [Repeat](#) | Executes the last command again | |
| [Replace](#) | Replaces text in the output window | |
| [ResetAlias](#) | Resets alias matching counts | 4.70 |
| [ResetNoteColour](#) | Resets the colour used in the output window to the default | 3.79 |
| [ResetTimer](#) | Resets a named timer | |
| [ResetTimers](#) | Resets all timers | |
| [ResetTrigger](#) | Resets trigger matching counts | 4.70 |
| [RestorePlugin](#) | Restores the state of a plugin | 4.28 |
| [SavePluginState](#) | Save the state of a plugin | 4.28 |
| [SaveState](#) | Saves the world file | |
| [ScreenReader](#) | Announces text to a screen reader | 4.13 |
| [Select](#) | Selects text in the output window | 4.09 |
| [Send](#) | Sends text to the MUD | |
| [SendImmediate](#) | Sends text to the MUD immediately | 3.18 |
| [SendNoEcho](#) | Sends a command to the MUD without it being echoed | 3.57 |
| [SendPkt](#) | Sends a raw data packet to the MUD | 3.35 |
| [SendToScript](#) | Sends text directly to the scripting engine for evaluation | |
| [SetBackgroundImage](#) | Sets a background image | 4.36 |
| [SetBackgroundImageEx](#) | Sets a background image (extended) | 4.36 |
| [SetClipboard](#) | Sets the contents of the Windows clipboard | 3.22 |
| [SetCommand](#) | Sets the current command in the command window | |
| [SetEntity](#) | Sets the text for a named MXP entity | |
| [SetForegroundImage](#) | Sets a foreground image | 4.36 |
| [SetForegroundImageEx](#) | Sets a foreground image (extended) | 4.36 |
| [SetFont](#) | Sets a font used in the output window | |
| [SetHelp](#) | Provides context-sensitive help for this world | |
| [SetImmediate](#) | Sets the "immediate" script | |
| [SetMember](#) | Sets the group number for a group name | |
| [SetMenu](#) | Adds a menu item to a menu | |
| [SetNotepad](#) | Sets the contents of a notepad window | 3.29 |
| [SetOption](#) | Sets the value of a named world option | |
| [SetPluginOption](#) | Sets the value of a named plugin option | |
| [SetPopupMenuItem](#) | Sets the properties of a popup menu item | 4.21 |
| [SetStatus](#) | Sets the status line | |
| [SetTimerOption](#) | Sets the value of a named timer option | |
| [SetTriggerOption](#) | Sets the value of a named trigger option | |
| [SetVariable](#) | Sets a variable | |
| [SetWorld](#) | Simulates drawing of text in the output window | |
| [ShowPlugin](#) | Shows the plugin window for a specific plugin | 3.44 |
| [ShowTable](#) | Displays a table in a notepad | |
| [ShowToolbar](#) | Shows or hides a toolbar | |
| [Simulate](#) | Simulates the user pressing keys | |
| [Sound](#) | Plays a sound file | |
| [Speedwalk](#) | Sends a speedwalk | |
| [StartLog](#) | Start logging | |
| [StopLog](#) | Stop logging | |
| [Teleport](#) | Changes the MUD the world is talking to | 3.68 |
| [TestAlias](#) | Tests an alias against some text and returns matching wildcards | 4.70 |
| [TestTrigger](#) | Tests a trigger against some text and returns matching wildcards | 4.70 |
| [TextRectangle](#) | Defines the text rectangle in the output window | |
| [Trace](#) | Sets tracing on/off | |
| [TraceOut](#) | Sends output to the trace file | |
| [Translate](#) | Translates a speedwalk string into a list of directions | |
| [Trigger](#) | Tests specified text against all triggers | |
| [UDP](#) | Sends a UDP message to the specified address | |
| [UnloadPlugin](#) | Unloads a plugin | 3.67 |
| [Until](#) | Pauses the command queue for a specified time | |
| [URL](#) | Opens a URL in a web browser | |
| [Wait](#) | Makes the script engine wait for a specified period | |
| [Walk](#) | Walks in the specified direction, adding to the speed walk queue | |
| [Warning](#) | Displays a warning in a message box | |
| [WorldAddress](#) | Returns the IP address of the world | |
| [WorldColour](#) | Sends a message to the output window with colour based on a custom colour number | |
| [WorldName](#) | Returns the world name | |
| [WriteLog](#) | Writes a line to the log file | |
| [Yellow](#) | Sends a message to the output window in yellow | |
