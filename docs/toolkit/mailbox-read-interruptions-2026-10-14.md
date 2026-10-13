# 2026-10-14：Retail 双实例的六次 Mailbox 读取中断

调查对象是 [双实例实测](live-500-slots-3.2.0.md#2026-10-14同一-retail-安装双实例)中保留的六次错误。
初次调查未修改产品代码；随后按用户要求完成 Go 修复与双正式服复测，记录如下。

## 修复与复测完成

2026-10-14，Go 读取层为 path_changed、lua_state_null、string_layout 保留独立错误身份，
同时维持原错误文本和 ErrMailboxUnavailable 分类。调度器把路径变化作为本次调用中的
可重读状态，使用已有 250ms 调度间隔、发现冷却与原持久期限；重载已处于 input_attempted
时，才把后两种错误当作等待新 Lua 运行时。普通阶段的布局错误、协议不兼容、指令配方变化
与进程身份变化仍立即停止。指令锚点在复核阶段变化也明确报告 root_recipe_changed。

发键前复核改走 Native.Find，与普通读取共用包装和 trace；明确 0 条消息的 path_changed
记为可重试 not_sent，submitted/uncertain 的原有不重发规则不变。
trace 增加可选 error 字段，保留本次被内部处理的读取错误；未增加插件采样、光学或输入动作。

五个定向故障入口（普通 INPUT、发键前复核、回执、重载根为空、重载字符串布局）先复现失败，
修复后全部通过，并检查原起止期限、槽发布次数与输入次数。反例确认持续布局/协议/根配方/
进程错误不会被吞成 pending。真实 INPUT 指针替换用例确认旧快照仍被拒绝、下一次读新发布。
`go build ./...`、`go vet ./...` 及以下三个受影响包的完整测试通过：

```powershell
go test ./internal/live/memory ./internal/live/channel ./internal/command -count=1
```

北京时间 01:37–01:38，固定原 Retail PID 30552 / 58940，各执行 4 次业务、重载一次、
再执行 4 次并断开。合计 22 条 CLI 命令全部 exit 0，16 次报告 verified、ok=true、cleanup=complete。
B 的 reload 命令实际捕获一次 INPUT/path_changed，并在同一 CLI 调用内恢复后完成；
验证脚本没有调用外部 resume。两边各 36 次提交输入、恰有 1 次 reload，零 Esc、零重复输入。
最终两 CON 均 closed，槽 pending=0，owner 文件为 0。

- A：`CON-b7858da0a16ae9216e62f8bf6b8ea318`。
- B：`CON-48a8f9905f78aad7723b4140b3ed8d72`。
- 修复后的本地 CLI：`.tmp/live-protocol-v1/mailbox-fix/lycheedev.exe`。
- 结果、逐条回执与脚本：`.tmp/live-protocol-v1/mailbox-fix/smoke.json`、同目录 `smoke.mjs`。
- 被内部恢复的 trace：`retail-dual/b/.lycheedev/live/scans/d64000c96ceefdc84f712e99b3a4a4ee/021.json`，
  路径相对 `.tmp/live-protocol-v1/`；完整新连接日志仍位于 retail-dual/a、b 的原项目内。

Lua 插件字节未变化，无需重新安装；版本仍为 3.1.2，未提交、发布或替换全局 npm。
原 string_layout 的历史对象证据缺口仍保留；修复仅在已提交重载的观察阶段等待该状态，
没有将任意场景的 string_layout 一律忽略。

## 初次调查结论

主要问题在 Go 的读取错误分类和调度：一致性检查正确丢弃了读取期间已变化的快照，
但临时读取失败没有进入当前调用的有界等待流程，直接结束了 CLI 调用。
原 CON、请求与期限仍在，所以外部 `live resume` 能继续完成。
仅在验证脚本增加 resume 可以接通测试，不能视为产品已经修复。

四次 `path_changed` 都处于 INPUT 读取路径。使用相同生产读取代码、增加失败 guard 位置的
只读 Go overlay，在两个实际进程中分别连续采样 8 秒：

| PID | 完整 Lookup 次数 | INPUT 发布变化 | path_changed | 下一次 Lookup 恢复 |
| --- | --- | --- | --- | --- |
| 30552 | 5,012 | 8 | 3 | 3 |
| 58940 | 4,534 | 8 | 3 | 3 |

这六次新捕获的失败全部位于 `Mailbox.input` 值的 guard（第 66/68 个），
字符串指针在第一次读取与末尾复核之间变了，Lua 值的 string tag 保持不变。
原 runtime、根绑定和进程身份保持有效；没有业务调用并发时也能复现，因此共享槽竞争
不是该问题出现的必要条件。不能据此量化双实例对发生率的影响。

## 原六次错误

证据根目录：`.tmp/live-protocol-v1/retail-dual/`。

| 原结果文件 | 阶段 / 读取 | 观察到的事实 |
| --- | --- | --- |
| `0003-a-align-0.json` | reload_input_attempted / identity | F11 已提交；重载后第 29 次内存读取返回 string_layout；原 CON resume 后新 runtime 正常 |
| `0106-a-dual-cycle-060-0.json` | commit_ready / INPUT | path_changed；106 次读取，正常同路径为 108；未发出本次待执行输入 |
| `0110-a-dual-cycle-063-0.json` | prepared / INPUT | path_changed；106 次读取；原请求 resume 完成 |
| `0117-b-dual-cycle-044-0.json` | prepared / INPUT | path_changed；106 次读取；原请求 resume 完成 |
| `0235-b-dual-cycle-105-0.json` | confirm_ready / 发键前再次读取 INPUT | 裸 memory.mailbox_unavailable: path_changed；明确 not_sent / 0 条窗口消息；该直读不经过普通扫描 trace |
| `0252-b-dual-cycle-122-0.json` | reload_input_attempted / identity | 只读了 58 字节锚点和 8 字节根指针，根值为 0；容量重载期间 Lua state 尚不可读，原 CON resume 后正常 |

前三份带 INPUT trace 的 path_changed 比正常路径少最后两次 guard 读取，与本次实际捕获的
input 值 guard 失败位置一致；第四份发生于相同 Lookup 的发键前复核入口。
原日志没有保存 guard 字节，因此这四次的历史指针变化不能逐字节回放；上述定位同时由
原读取种类、读取次数、相同错误入口和新实机捕获支撑。

两次重载错误需区分证据强度：`lua_state_null` 由代码分支与 66 字节读量直接确定。
`string_layout` 位于重载后 identity 解析中，高概率是旧 Lua 对象销毁期间的不完整路径；
原 trace 未保存字符串头、字段名或失败对象，不能确定当时是哪一个对象，也不能声称已排除
该次独立的布局问题。它随后在新 runtime 恢复，未表现为持续的根定位失效。

## 代码链路

1. `addon/Bridge/InputState.lua` 的 sample 每秒生成新的 INPUT 字符串并 rawset 到
   Mailbox.input；`addon/Bridge/SlotRuntime.lua` 在 wake 处理结束时还会主动刷新 INPUT。
2. `internal/live/memory/lua_mailbox.go` 的 lookup 保存命中节点的值指针/tag，
   verify 在全部读取后逐项复核。只要 INPUT 在这几毫秒间发布了新字符串，就返回
   ErrMailboxUnavailable/path_changed 并丢弃本次结果。这项一致性检查应保留。
3. `internal/live/channel/native_windows.go` 的 findPath 将错误包装成
   live.channel_mailbox_unavailable，但没有把可重读状态与配置/协议错误区分。
   `wait.go` 的 Continue 只在 ErrPending 时继续；普通输入检查的错误直接返回。
   重载的 RuntimeCandidate 读取错误也直接返回，未在原重载期限内继续等待。
4. `internal/live/channel/input_windows.go` 的 Input/send 又直接调用 Mailbox.Lookup，
   绕过 findPath 的包装和 trace。读失败且尚未发键时，outcome 虽正确记为 not_sent，
   但 Retryable 只识别 ErrPending / held key，因而仍把裸错误返回上层。
5. `internal/command/live_channel_error.go` 按 live.channel_ 前缀分类：包装后的读取错误
   是 exit 3，裸 memory 错误落到通用 exit 5 / command.failed。两者都保留了
   continuation=continue，却没有表示为正常 pending；这是同一故障出现不同退出码的原因。

## 最小复现及验证边界

所有临时源码与插桩只在 `.tmp/live-protocol-v1/mailbox-diagnosis/`，通过 Go overlay 加载。
常规构建不会包含这些诊断文件。

```powershell
go test -overlay .tmp/live-protocol-v1/mailbox-diagnosis/overlay.json ./internal/live/memory ./internal/live/channel -run TestDualMailboxDiagnostic -count=1 -v
```

结果保存在 `repro.txt`：冻结发布的对照通过；读取中只替换一次 INPUT 指针会返回
path_changed，下一读立即得到新记录；通道收到一次临时读取错误便在 reads=1 时退出，
continuation 仍为 continue，显式续行只重读，不重复已有 publication/input。
这些诊断断言期望内部继续，因此当前代码下故意为红，代表问题已复现，并非修复完成。

只读实机插桩记录为 `live-read.txt`，它仅使用已有生产 OpenLuaMailbox/Lookup 路径，
用同一套进程身份、运行时指令特征与 RIP 根校验，没有写进程或发送游戏输入。
测试断言验证诊断运行完成；观察到的读取碰撞仍按错误计数，不当作产品通过项。

## 调查时提出的修正方向

- 将已确认的快照变化表示为可重读状态，复用原 CON、请求和期限，在 CLI 内有界重读。
  仅重读失败快照，已有 submitted/uncertain 输入仍不重发。
- 重载已经提交后，Lua state 暂空属于等待新运行时；继续原预算下的观察和新绑定验证。
- 普通观察和发键前复核共用错误分类；后者只在证明 0 条消息时按现有 not_sent 路径续行。
- 不把全部 ErrMailboxUnavailable 或全部 string_layout 一律吞成 pending。
  结构不支持、协议不兼容、根验证变化、进程身份变化应保留明确错误；需要暂态判断时记录
  失败字段/阶段，避免再次只能看见一个宽泛错误名。

修复验收应围绕 INPUT 指针更新、重载空窗和发键前复核三个入口，核对有界恢复、
原期限不变与不重复输入；无须重复七产品或整轮 296 次容量测试来证明这一个修复。
