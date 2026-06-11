# MUSHclient GetInfo 官方文档

> 来源：http://www.mushclient.com/scripts/function.php?name=GetInfo
> 保存日期：2026-06-11

Gets details about the current world.

**Prototype**: `VARIANT GetInfo(long InfoType);`

## GetInfo code 映射

### 字符串 - 配置项
| code | 说明 |
|------|------|
| 1 | Server name (IP address) |
| 2 | World name |
| 3 | Character name |
| 4 | Send to world - file preamble |
| 5 | Send to world - file postamble |
| 6 | Send to world - line preamble |
| 7 | Send to world - line postamble |
| 8 | World notes |
| 9 | Sound on new activity |
| 10 | Scripts editor |
| 11 | Log file preamble |
| 12 | Log file postamble |
| 13 | Log file line preamble - player input |
| 14 | Log file line preamble - notes |
| 15 | Log file line preamble - MUD output |
| 16 | Log file line postamble - player input |
| 17 | Log file line postamble - notes |
| 18 | Log file line postamble - MUD output |
| 19 | Speed Walk Filler |
| 20 | Output window font name |
| 21 | Speed walk prefix |
| 22 | Text sent on connecting |
| 23 | Input font name |
| 24 | Paste to world - file preamble |
| 25 | Paste to world - file postamble |
| 26 | Paste to world - line preamble |
| 27 | Paste to world - line postamble |
| 28 | Scripting language |
| 29 | Script function for world open |
| 30 | Script function for world close |
| 31 | Script function for world connect |
| 32 | Script function for world disconnect |
| 33 | Script function for world get focus |
| 34 | Script function for world lose focus |
| 35 | Script file name |
| 36 | Scripting prefix |
| 37 | Auto-say string |
| 38 | Auto-say override |
| 39 | Tab-completion defaults |
| 40 | Auto-log file name |
| 41 | Recall window - line preamble |
| 42 | Terminal ID (telnet negotiation) |
| 43 | Mapping failure message |
| 44 | Script function for MXP starting up |
| 45 | Script function for MXP closing down |
| 46 | Script function for MXP error |
| 47 | Script function for MXP tag open |
| 48 | Script function for MXP tag close |
| 49 | Script function for MXP variable set |
| 50 | Sound to play for beeps |

### 字符串 - 运行时计算
| code | 说明 |
|------|------|
| 51 | Current log file name |
| 52 | Last "immediate" script expression |
| 53 | Current status line message |
| 54 | World file pathname |
| 55 | World title |
| 56 | MUSHclient application path name |
| 57 | World files default path (directory) |
| 58 | Log files default path (directory) |
| 59 | Script files default path (directory) |
| 60 | Plugin files default path (directory) |
| 61 | World TCP/IP address (after DNS lookup) |
| 62 | Proxy server TCP/IP address |
| 63 | Host name (name of this PC) |
| 64 | Current directory |
| 65 | Script function for world save |
| 66 | MUSHclient application directory |
| 67 | World file directory |
| 68 | MUSHclient startup (initial) directory |
| 69 | Translation file |
| 70 | Locale |
| 71 | Font used for fixed-pitch dialogs |
| 72 | MUSHclient version (eg. "4.11") |
| 73 | MUSHclient compilation date/time |
| 74 | Default sounds file directory |
| 75 | Last telnet subnegotiation string received |
| 76 | Special font pathname |
| 77 | Windows version debug string |
| 78 | Foreground image name |
| 79 | Background image name |
| 80 | libpng version |
| 81 | libpng header |
| 82 | Preferences database pathname |
| 83 | SQLite3 database version |
| 84 | File-browsing directory |
| 85 | Plugins state file directory |
| 86 | Word under mouse on mouse menu click |
| 87 | Last command sent to the MUD |

### 布尔值 - 运行时计算
| code | 说明 |
|------|------|
| 101 | 'No Echo' flag |
| 102 | Debug incoming packets |
| 103 | Decompressing |
| 104 | MXP active |
| 105 | Pueblo active |
| 106 | Disconnected flag (true if not connected) |
| 107 | Currently-connecting flag |
| 108 | OK-to-disconnect flag |
| 109 | Trace flag |
| 110 | Script file changed |
| 111 | 'World file is modified' flag |
| 112 | Automapper active flag |
| 113 | 'World is active' flag |
| 114 | 'Output window paused' flag |
| 115 | Localization active |
| 118 | Variables have changed |
| 119 | Script engine is active |
| 120 | Scroll bar is visible for output window |
| 121 | High-resolution timer is available |
| 122 | Is the SQLite3 library thread-safe? |
| 123 | Are we currently processing a "Simulate" function call? |
| 124 | Is the current line from the MUD being omitted from output? |
| 125 | Is the client in full-screen mode? |

### 数字 (long) - 运行时计算
| code | 说明 |
|------|------|
| 201 | Total lines received |
| 202 | Lines received but not yet seen |
| 203 | Total lines sent |
| 204 | Packets received |
| 205 | Packets sent |
| 206 | Total uncompressed bytes received |
| 207 | Total compressed bytes received |
| 208 | MCCP protocol in use (0=none, 1 or 2) |
| 209 | MXP error count |
| 210 | MXP tags received |
| 211 | MXP entities received |
| 212 | Output font height |
| 213 | Output font width |
| 214 | Input font height |
| 215 | Input font width |
| 216 | Total bytes received |
| 217 | Total bytes sent |
| 218 | Count of variables |
| 219 | Count of triggers |
| 220 | Count of timers |
| 221 | Count of aliases |
| 222 | Count of queued commands |
| 223 | Count of mapper items |
| 224 | Count of lines in output window |
| 225 | Count of custom MXP elements |
| 226 | Count of custom MXP entities |
| 227 | Connect phase (0~8) |
| 228 | World TCP/IP address (as number) |
| 229 | Proxy server TCP/IP address |
| 230 | Script execution depth |
| 231 | Log file size |
| 232 | High-performance counter output (seconds) (double) |
| 233 | Time spent executing trigger matching (double) |
| 234 | Time spent executing alias matching (double) |
| 235 | Number of world windows open |
| 236~238 | Command window selection info |
| 239 | Source of current scripted action |
| 240~241 | Character width/height in output window |
| 242 | Count of lines with bad UTF-8 sequences |
| 243 | Font size of fixed pitch font |
| 244~247 | Match counts |
| 248 | Count of timers that fired |
| 249~264 | Window dimensions |
| 265~268 | Windows version info |
| 269~279 | Image/colour/text rectangle info |
| 280~281 | Output window client dimensions |
| 282~294 | More UI info |
| 295~300 | More runtime info |

### 日期 - 运行时计算
| code | 说明 |
|------|------|
| 301 | Time connected |
| 302 | Time log file was last flushed to disk |
| 303 | When script file was last modified |
| 304 | The current date/time |
| 305 | When client started executing |
| 306 | When this world was created/opened |

### 更多数字
| code | 说明 |
|------|------|
| 310 | Newlines received from the MUD (lines terminated by a newline) |
