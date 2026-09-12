# 固定 Lua 文件投递与 SV 回收方案

**收尾状态：用户已转向 Lychee 新的验证，明确搁置聊天日志路线。本研究停止，不再执行或等待 LDLOG 探针，不再观察日志/游戏，也不实施替代信号方案。下文为保留的设计与历史证据；完成信号和自动输入仍未验证。用户决定搁置不等于实测证明所有 chatlog 路径不可能。**

建议继续这个方向：**固定 TOC 登记的任务文件 → reload 加载但不执行 → 短命令显式运行 → 完成后正常 reload 保存 SV → 外部读取确切结果**。这能绕开巨大 EditBox 粘贴，保留任意临时 Lua 脚本和参数化探针，也不需要维护进程内存偏移。

两次 reload 都由外部协调脚本决定并发送。确认加载后，外部将本轮短运行命令放入剪贴板并读回校验，定向正确窗口执行回车/粘贴/发送；随后增量读取游戏落盘的完成/错误标记，满足条件才发送第二次 reload。插件只登记、执行、提供状态和内存结果，**不自行触发 reload**。这是用户指定的目标流程，自动输入可靠性与实时信号尚未验收。

这是方案研究。插件接口、部署器、自动按键及自动往返尚未实现。本轮没有访问游戏进程、修改游戏安装/SV/Skill、发送按键、开启日志或启动采样。内存路线已放下，不创建内存 Analyses 目录。独立 Lychee Performance Test 的修复/实测不受本任务干扰。

## 已知事实与边界

| 情形 | 证据与结论 |
| --- | --- |
| 本次新增 Lychee Performance Test 插件 | 原任务转述用户实测：未退出正式服客户端，安装后 `/reload` 配合 `C_AddOns.LoadAddOn` 进入 running，并完成两份独立 SV 报告。本任务未重新操作或复测。不能仅因 PID 早于安装就要求重启。 |
| 已有 Lua / 固定 TOC 预登记文件内容修改 | 最适合每轮投递的候选；使用正常 Lua 加载，不发明 readfile。仍须对新入口做一次“文件版本 A → B，reload 后加载回执改变”的实机验收。 |
| 新增 Lua 文件或 TOC 条目 | 不等同于只修改原文件。初始化桥接时一次加入入口，之后每轮不改 TOC。客户端重新发现条目的行为需分别验证。 |
| 新增插件发现 | 本次正式服有成功实证，不推广为所有 build、任意安装状态的保证。 |
| 修改媒体文件 | 不由 Lua/reload 实证保证缓存更新；稳定引用并单独验证，必要时版本化文件名。 |
| 数据输入 | 参数/小型夹具编码进任务数据；大数据由外部预处理并封装为任务需要的数据。游戏不读取任意主机路径。 |

版本源码证据见 [sources.json](sources.json)，均由 wowdoc 读取 `wow-ui-source`，Commit 固定，matchedTag 为 null：

- Retail `8ea15b61e45c0ed4eba01439c90757f86eb78d34`，`Interface/AddOns/Blizzard_DebugTools/Blizzard_DebugTools.toc:5-7`：`LoadOnDemand: 1`，登记 `Blizzard_DebugTools_Bootstrap.lua [Bootstrap]`。它证明 TOC 登记 Lua 的实例，不证明 native 文件发现/缓存策略。
- Retail 基线 `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b`，`Interface/AddOns/Blizzard_APIDocumentationGenerated/AddOnsDocumentation.lua:347-361`：`C_AddOns.LoadAddOn(name: uiAddon)` 返回 loaded/value。
- Classic `1028c1e687f721ba9d3af14d1b12a5745e4227c7`、Titan `825d29d3662b372f0bead725ee6abd339e4a77b5`，同路径 `:326-339` 有相同参数/返回接口。**接口存在不认证三端热更新行为**。这些 UI 源码也不是 native loader 实现。

## 最小改动（待实施）

| 位置 | 责任 |
| --- | --- |
| `add-on/Core/DevTasks.lua` | 任务登记、校验、显式运行/取消/状态、结果发布；沿用 combat gate 和 AddExport，不恢复性能模块。公开能力经 `/dev task ...` 路由调用。 |
| `add-on/Tasks/Current.lua` | 三个 TOC 一次登记同一路径，排在任务接口定义之后；发布包默认是空占位文件。开发者明确启用任务模式后由外部原子替换此文件。 |
| `Core/Bootstrap.lua`、三端 TOC | 保留裸 `/dev` 行为，添加少量任务子命令；三个 TOC 的共享顺序一致。所有下述命令名均为拟议接口，当前不能使用。 |
| `Lychee Dev skill/scripts/task.py` | 单个辅助 CLI 提供 prepare/status/collect/clear，保存外部任务记录、生成封包、校验文件、解析 SV。先输出短操作步骤；经单独验证的窗口适配器以后接入同一个协调器。 |
| Skill 及打包检查 | 增加该工作流；打包必须确保 Current.lua 是空占位，拒绝把活动任务/敏感夹具带进发布 ZIP。当前 Package.ps1 会原样打包 TOC 引用文件，不能直接沿用而忽略这一点。 |

任务入口文件只调用登记函数，源码作为**转义后的字符串数据**传入。禁止把用户代码直接拼成顶层 Lua，也不使用易与载荷冲突的裸长括号边界。加载阶段不编译/调用任务，不因登录、reload、ADDON_LOADED 自动执行。SV 只在允许的初始化时机读取；文件加载阶段只登记内存描述。默认关闭，无新 frame/event/hook/ticker；空文件随发布，开发载荷只在明确启用后投递。

封包字段保持少量且明确：协议版本、随机 taskId/请求 nonce、外部安装配对 ID、预期 product/build/桥接版本及角色 GUID、创建/过期时间、源码字节数与校验、source 或 probeId/params。外部记录 PID+进程创建时间+路径及账号 SV 路径；**Lua 不假装知道 PID、安装目录或账号路径**，只核对游戏可见身份。当前未实机核查这些拟议身份接口，实施时按三端基线补证据。

载荷校验可以采用外部 SHA-256 归档加游戏端长度/Adler-32 重算并回显；Adler-32 只检查意外损坏，不是认证或抗恶意碰撞。taskId/nonce 必须独立随机。若要求游戏端密码学摘要，需另选并验证实现，不能把回显宿主提供的 SHA-256 当作游戏端重算结果。

## 一轮操作与确认

1. 外部选定目标、锁定实际安装目录与目标 SV 写入域，将源码及清单归档。生成临时文件，做 Lua 5.1 语法/编码与大小检查，在同一目录内完整替换 Current.lua，回读核对摘要；不向 SV 写入任务。当前任务未收尾时拒绝覆盖。
2. 向正确窗口请求第一次 reload。拟议 `status` 返回 **loaded** 回执：taskId、载荷摘要、当前角色/build、桥接版本和本次 UI 加载 token。外部必须读到对应回执，不能以窗口回来了或等待数秒代替。旧版本/缺文件/损坏/目标不符/过期均不运行。
3. 外部先持久化 attempted 记录，再发送拟议 `/dev task run <taskId> <loadToken>`。插件重新核对身份、摘要、状态、过期和战斗限制，立即消费本次执行许可，再进入 running。运行命令不会隐式 reload；重复命令返回已有状态。UI token 在每次 UI 加载后变化，旧命令失效。
4. 同步脚本显式运行并捕获返回/错误；异步脚本使用任务 context 的 complete/fail 和 cleanup 回调，返回函数本身不代表异步完成。登记拥有的事件/资源，终态只接受一次，迟到回调忽略。参数化探针与任意临时脚本用同一协议，复杂脚本需要适配 context，不能承诺自动推断任意代码何时结束。
5. 结果先经 `ns.AddExport` 成功写入内存中的 `LycheeDevDB.exports.records[Ticket]`，再显示终态小信号，包含 taskId、loadToken、status、Ticket、schema、结果字节数/摘要。信号不是报告正文。发布失败给出 export_failed，不冒充成功或可回收。
6. 外部确认确切终态且该轮获准保存，才对同一目标请求第二次 reload。异步任务运行中不因固定超时自动 reload。磁盘出现完整可解析的记录，并匹配任务/会话/客户端/schema/终态/载荷和结果完整性后，才称 **collected**。仅文件 mtime 改变、出现 Ticket、收到 done 都不够。读到半写文件可有界重读，不执行未知 Lua SV。
7. 外部记录 collected，恢复固定入口为空占位并释放锁。临时 source 归档保留在外部工作目录；中断时需要人工处理的状态明确保留，不能偷偷重跑。

当前可靠窗口输入尚未验证，已有原生按键无响应记录。适配器必须核对 PID/创建时间/窗口归属、焦点/状态并读取回执；不能广播按键、忙循环重试或用后台发送成功返回值代替游戏接收。今天可以减少到两次 reload 加一条短运行命令，仍由用户明确操作；不能声称一键无人值守。加载回执、运行回执和终态若使用聊天日志，也都依赖下节验证。

超时只令外部流程停止等待并标记 unknown/needs-attention，不等于游戏任务已停止。取消依赖 context 清理和合作式任务；Lua 同步死循环、native 卡住不能被普通取消命令抢占。取消/错误也必须完成结果发布才允许自动保存。SV 终态账本有界，外部持久化已尝试/已消费记录提供跨崩溃防重试；发生崩溃即使 SV 没保存，也不自动重新运行，不承诺跨崩溃 exactly-once。

同一个 AddOns 目录的多个客户端都会看到同一任务文件；先只支持该目录串行执行。不同账号/角色用目标身份拒绝误运行，但 Lua 无法可靠区分两个相同角色实例；这种情况拒绝自动模式。共享同一 SV 文件的多客户端也会互相覆盖输出，必须串行或使用独立安装/WTF 域。主机任务锁约束协作脚本，不把它当作能阻止游戏用户手动操作的锁。

## 优先完成信号候选：原生聊天日志

当前结论是**尚未证明可用**。第一次只读检查时 `D:/Game/World of Warcraft/_retail_/Logs/WoWChatLog.txt` 不存在；随后用户核对日志期间，该文件已出现，长度 0，mtime 为 2026-09-12 07:48:16.296455 UTC。最新状态是**文件已创建但仍空**，不是不存在。本任务未开关日志、未产生探针；旧报告标记不在空文件内不能否定本地信号可行性。原任务已收到一条新的唯一标记命令，待用户本地执行后观察，未声称探针已运行。见 [history-files.json](history-files.json)，未输出私人聊天。

固定 Retail Commit `8ea15b61e45c0ed4eba01439c90757f86eb78d34` 的源码链（原始片段见 [chat-evidence.json](chat-evidence.json)）：

- `Blizzard_ChatFrameBase/Shared/SlashCommands.lua:844-853`：LoggingChat 查询/开关；开启分支调用 `LoggingChat(true)`，用 AddMessage 显示提示。
- `Blizzard_APIDocumentationGenerated/ChatInfoDocumentation.lua:363-369`：`C_ChatInfo.IsLoggingChat()` 返回 enabled bool。没有精确命中 `C_ChatInfo.LogChatMessage`，不使用这个虚构入口。
- `Blizzard_PrintHandler/Blizzard_PrintHandler.lua:93-95` → `:84-91` → `:66-71`：print 经 print_inner、LOCAL_PrintHandler，默认将 printMsg 交给 DEFAULT_CHAT_FRAME:AddMessage。
- `Blizzard_ChatFrameBase/Shared/ChatFrame.lua:35-41` → `Blizzard_SharedXML/ScrollingMessageFrame.lua:806-808` → `:10-17`：AddMessage 转交安全包装，再 PushFront 到 historyBuffer、滚动并 MarkDisplayDirty。观察者还可收到通知。这条源码链没有直接磁盘写入。
- 收到聊天事件走另一入口：`Blizzard_ChatFrameBase/Mainline/ChatFrameOverrides.lua:276-304` 处理 CHAT_MSG 前缀事件及过滤器。显示历史、接收事件和 native 日志不是同一证据。

这些源码只支持本地显示链，不说明 native 日志是否旁路捕获本地 print/AddMessage，更没有 flush 时延保证。**只有开启日志且本地 marker 在未 reload/未关闭日志前进入磁盘，才可考虑此信号。**若必须关闭日志或 reload 才出现标记，就不能用它触发第二次 reload。当前不能假定它满足条件。

其他历史入口的有界核查：

| 类型 | 本轮实证与限制 |
| --- | --- |
| 原生聊天框显示历史 | 上述 historyBuffer 是显示数据结构；未在已查源码链找到其 native 实时写盘接口，不等于已经证明不存在任何客户端持久化机制。 |
| 聊天编辑输入历史 | 已装 EllesmereUIChat.lua:4092/4195 指向 EditBox 的 AddHistoryLine（Alt+Up/Down）。这是输入过的命令历史，不是任务执行结果，也不能把其中含有 done 的命令当完成。未证明实时文件入口。 |
| chat-cache.txt / chat-frontend-cache.txt | 实际找到。chat-cache.txt 中只检查指令结构及旧测试前缀：WINDOW/SIZE/COLOR/DOCKED/POSITION/DIMENSIONS、消息类别/频道等是窗口配置结构，不能因 MESSAGES 关键字就说它保存显示正文。9 份 txt 的已知旧标记计数均 0；frontend 只核对元信息，内容语义未验证，不按名字推断。 |
| 已装 EllesmereUIChat 9.1.8 | TOC:9-10 明确声明 EllesmereUIChatDB 与角色级 EllesmereUIChatScrollDB。SessionHistory.lua:27、129-135、305-316 使用 `_G[SV_NAME].sessionLog`；:364-391 仅捕获带白名单聊天事件的显示尾部，不假定包含本地 print；:585-594 在 PLAYER_LOGOUT/PLAYER_LEAVING_WORLD 做快照。是普通 SV 的内存修改/跨 reload 历史机制，没有发现立即写文件接口，不能当作第二次 reload 之前的已落盘信号。目录存在也不证明本轮已加载/启用该功能。 |

以上检查没有定位一个已证实的“本地显示正文无需 reload 即落盘”通道，仍尊重用户对此行为的记忆，先用当前 WoWChatLog 的新标记实验判断实际行为。无需继续大范围搜索或向其他玩家发送消息。

最小剩余实测见 [chatlog-probe-plan.md](chatlog-probe-plan.md)，待独立性能测试结束和明确安排后执行，不对其他玩家发测试消息。若不支持本地消息，本候选判定不满足需求；不擅自改用公会/队伍/世界/私聊或伪造聊天事件。

如果验证通过，外部只增量读取准备前的文件尾之后的数据，匹配精确协议/nonce/阶段/终态，不搜索历史 done 作新结果。识别文件替换/缩短/轮转，丢弃半行并有限缓存，按新文件身份重新定位；无法辨认来源、重复/冲突终态、多客户端共用日志均暂停。默认不开日志；按本轮授权开启并保存原状态，结束恢复，日志开关不是每轮随意 toggle。日志缓冲超时返回未知，不虚报完成。日志只触发保存尝试，最终成功仍以确切 SV 记录为准。

## 文件放置与本轮离线验证

建议在目标插件项目使用普通忽略目录 `analyze/dev-tasks/<taskId>/` 保存 source、输入夹具、manifest、状态、收回报告；这不是内存 Analyses。游戏目录只放固定 Current.lua 与经过明确安装的媒体。媒体单独放 Media 的开发资源子目录并做发布排除；不把任意 CSV/JSON 路径交给 Lua 假装能 readfile。常用探针由辅助脚本生成同一种封包，源代码不会经过 EditBox。

[offline-check.py](offline-check.py) 在临时目录生成固定 Task.lua，用本机 Lua 5.1.5 模拟 TOC 向 chunk 传递 addonName/ns，登记函数仅保存描述；没有实现或冒充 WoW 加载器。运行命令：

```powershell
python add-on/docs/validation/2026-09-12-file-delivery/offline-check.py
```

[offline-results.json](offline-results.json) 的三组检查通过：468817 字节合成源码封包为 468921 字节，加载/编译无副作用，显式调用返回 49；全部 256 字节值及引号/反斜线/CRLF 精确还原；无效 Lua 可以登记但显式编译失败。校验能发现本实验的损坏，不证明强认证。合成大脚本主要是注释填充，不代表原先大型调查代码的性能、语义或游戏执行安全。本实验不验证客户端热加载、任务状态机、SV 写盘、窗口输入或日志 flush。

原计划的后续验证现已取消。本轮保留固定文件设计、Lua 5.1 离线封装实验和版本源码证据；不宣称完整自动往返已实现，无实时任务运行。
