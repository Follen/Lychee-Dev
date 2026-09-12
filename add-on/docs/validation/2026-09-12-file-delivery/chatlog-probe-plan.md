# 本地聊天日志信号：待执行的最小验证

**已搁置，仅作历史设计保留。用户已决定转向 Lychee 新的验证，不再请求执行下述命令、不等待探针、不观察日志或游戏。以下步骤不再是当前行动项；本任务没有执行探针。**

本文件只是审阅方案，未运行、未部署、未开日志。先等 Lychee Performance Test 完成并安排独占测试窗口。只使用本地 print/AddMessage，不调用 SendChatMessage、SendAddonMessage、任何频道/私聊或伪造收到的聊天事件。

## 用户当前已建立日志时：最短一步

文件目前为 0 字节，mtime UTC 07:48:16。下面命令已经交给原任务，但本任务未执行。让用户在**普通聊天输入框**输入这一条（170 ASCII 字节），随后不 reload、不关闭日志：

```text
/run if not C_ChatInfo.IsLoggingChat() then print("LDLOG: logging OFF"); return end print("LDLOG-7f4c9e83-b261-P"); DEFAULT_CHAT_FRAME:AddMessage("LDLOG-7f4c9e83-b261-A")
```

显示 logging OFF 表示没有有效探针，不能据此评价落盘；需另行明确开启再测。这条命令不改开关，因此不需要恢复开关。外部从执行前的 EOF（当前为 0）观察最多 30 秒，只输出精确 `LDLOG-7f4c9e83-b261-P` / `LDLOG-7f4c9e83-b261-A` 匹配及文件元信息；分别记录本地可见和磁盘首次可见，不转存其他聊天正文。30 秒是这次验证的观察上限，不是生产任务完成的固定等待。

## 如需主动开启日志：完整可恢复步骤

1. 重新确认目标客户端/窗口/安装目录。主机生成新的随机 token（下面示例 `LDLOG-20260912-7f4c9e83` 只用于审阅，真正执行时换新值）。记录已知 Logs/WoWChatLog.txt 的存在性、文件身份、长度和末尾偏移，禁止将旧标记当新标记。
2. 在普通游戏 `/run` 或已确认的本地执行入口运行以下开始逻辑。若使用 Lychee Dev Run，其自定义 print 会被捕获到结果面板，故明确调用 `_G.print`，以测试原生默认打印链。界面必须直接确认两个 marker 可见及 IsLoggingChat=true；本地显示确认不算落盘确认。

```lua
assert(not _G.LycheeChatLogProbe, "probe already active")
local state = { token = "LDLOG-20260912-7f4c9e83", previous = C_ChatInfo.IsLoggingChat() }
_G.LycheeChatLogProbe = state
local ok, err = pcall(function()
    LoggingChat(true)
    assert(C_ChatInfo.IsLoggingChat(), "logging did not enable")
    _G.print(state.token .. " method=print state=done")
    DEFAULT_CHAT_FRAME:AddMessage(state.token .. " method=addmessage state=done")
end)
if not ok then
    LoggingChat(state.previous)
    _G.LycheeChatLogProbe = nil
    error(err)
end
```

3. 外部每 250 ms 有界检查文件增量，最多 30 秒；只保留带精确 token 的完整行及时间、文件元信息，不输出/保存无关聊天正文。记录发出时间、UI 可见时间、首个磁盘完整行时间，标明跨主机/游戏时间不能混用。日志内容可能含时间前缀，匹配完整 marker 字段而非任意 done 子串。此轮不 reload，不切换日志；必须测到开启期间本地两条消息是否落盘。
4. 不论命中、错误或超时，执行独立恢复片段；确认开关恢复原值。若原状态本来开启就保持开启，不能简单 `/chatlog` 再 toggle 一次。对无法送达的恢复命令明确请求手动恢复，不声称已清理；不要自动重试游戏输入。

```lua
local state = _G.LycheeChatLogProbe
assert(state and state.token == "LDLOG-20260912-7f4c9e83", "probe identity mismatch")
LoggingChat(state.previous)
assert(C_ChatInfo.IsLoggingChat() == state.previous, "restore did not apply")
_G.LycheeChatLogProbe = nil
```

5. 恢复后再读取一次新增匹配行，单独标为“仅关闭后可见”，不要计作无 flush 操作时成功。不删除日志或历史。如期间 UI 重载/客户端退出导致 Lua 恢复状态丢失，用外部记录的原值手动恢复；本轮同时标记受干扰。

判定：两种显示方式分别记录 `visible`、`loggingEnabled`、`onDiskBeforeRestore`、`latencyMs`。日志不开/文件不生长/观察超时只说明该条件下未验证；启用确认且 UI 有 marker，但直到恢复才落盘，不满足自动第二次 reload 的前置条件。只有稳定在不 reload、不关闭日志的情况下出现当前 marker，才进一步验证 loaded/running/failed/cancelled 阶段及多次任务。一次成功仍不保证所有 build、flush 延迟或多客户端来源隔离。

此方案只验证小信号通道；没有向磁盘写测试任务、没有操作 SV。若日志通道成立，再验证 done 后第二次 reload 的定向输入和确切 SV schema/任务/完整性匹配，不能把本探针命中称为完整往返成功。
