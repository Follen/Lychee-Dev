# Lychee Dev 插件桥可靠性调查总结

日期：2026-09-27。证据读取截止约 14:52（Asia/Shanghai）。

增补：15:39（Asia/Shanghai）再次只读核对主线程调用及已保存回执；以下增补不覆盖原调查时间点。主线程仍在进行，运行中状态按该时点记录，不代为恢复或清理。

## 15:39 增补：最新失败不是同一种故障

### 嵌套滚动检查：探针前置状态失败，桥接报告正常

操作 `OP-0446963643f1276aeaa4e71068e035e9`：15:28 左右 load 返回 loaded，run 返回 verified，但报告正文为 `probeStatus=failed`、`panel absent`。15:29:32 finish 已确认 complete=true、cleanup=complete、display.state=cleared；本次只读 status 再次确认报告和清理完成。

源探针第 19 行直接断言 `LycheeTalentFrame` 存在，没有建立检查所需的页面状态。当前 `internal/live/bootstrap_probe.go` 明确在加载队列时提交一次初始 reload，prepare 分发到 `Reentry.LoadQueue`；`addon/Bridge/Reentry.lua` 的提交路径调用 `C_UI.Reload`。因此不能假定加载前手动打开的临时编辑面板在运行探针时仍存在。主线程也明确承认了这一前置条件错误。

这是**探针设计与加载生命周期不匹配**，不是命令未输入、报告编码失败或没有回执。证据：`nested-scroll-load.json`、`nested-scroll-result.json`、`nested-scroll-finish.json`；报告正文 `CAP-1d2af72264ef5bf85f20a144ac7db5067d9d11bc0eb9f2db1f97176e730b1f58`，清屏 `CAP-0099620a45fda17ae9e5b3a075702db2fad5018d36d99261a071b32c2569ba75`，文件位于同机 `D:/Code/wow/addons/Lychee Talent/docs/validation/`。

修正后的 `OP-2d606478a26fed0aa04da628d9a09c0c` 使用不可变探针 `PRB-a29e1b252eb2666d6a4e63477307d7d754061ed2eac9a7613bc44cccc3f849b3`：主动打开主面板、进入我的方案、打开关联方案编辑与图标选择器后采集几何。15:39 只读核对得到 report=verified、probeStatus=completed、result.passed=true，正文 capture 为 `CAP-9a888f4cf3b6be8b8705c835ec1fec8f864c55b0a955349a993ee0d50ac34dd2`。当时 cleanup=pending、complete=false，finish 文件尚为空，不能将进行中的清理判成失败或完成。

该修正探针只采集 frame 层级、坐标、wheel 标志及 inner/outer 偏移，并未实际滚动后断言隔离效果；`passed=true` 只表明它走到了采集结束。直接调用 OnClick 也不等同于真实输入命中和事件分派。它证明脚本诊断路径有效，不能据此宣布嵌套滚动业务已通过真机回归。其依赖现存关联方案也应在通用探针中声明或替换为明确授权的临时夹具。

### 更早一轮激活：bootstrap 失败且没有 operation ID

主线程保存的 `inline-icon-live-attempt-2026-09-27.md` 记录：预检查 `OP-af85091eeb2c43d487628b04ff71293e` 报告已核验并清理；随后 live reload 在连接阶段返回 `live.identity_unreadable`，没有 operation ID 或 runtime capture。同窗口一次 live reset 在 dispatch 超时，也没有可核验 reset。该记录引用的 WGC 图像显示设置/按键捕获界面与聊天 `/dev bridg...` 残文，最终残文清除未确认。

本增补读取的是既有文字与图像引用，没有重新抓图或逐像素复核。证据足以说明该次身份握手/恢复未完成，并记录了不适合继续盲输的 UI 状态；不足以还原具体哪个玩家按键、发送字符或焦点变化首先造成失败。不能认定“缺 Esc 就是唯一根因”，也不能把没有 operation ID 理解成肯定没有发出输入。

新通道必须覆盖首次 identify/connect/reset，而不只是已有 operation 的 run。应在第一次可能发送输入前创建可恢复的 bootstrap attempt 记录，保留目标身份、阶段、发送进度、nonce、回执与不确定性；它不同于探针 operation，不能伪造一个探针来 finish。恢复失败时必须明确区分未发送、部分/未知发送、已接收，以及接收器是否已释放。

### 对总方案新增的要求

用户随后指出探针应充分使用 `/run` 同等 Lua 能力，判断成立：本次 panel absent 应首先修正场景准备与回归写法，而不是归因于 CLI 没有鼠标模拟。普通自有 UI 可以由探针打开、切页、展开并调用实际滚动路径，然后断言内外偏移和条目变化。最新修正探针虽完成场景准备，却只采集几何，仍缺少滚动动作及其结果断言。总方案已新增“前置检查 → 准备场景 → 执行动作 → 断言效果 → 清理与报告”规范。

此结论不撤销已确认的桥接问题，也不把程序调用回调等同于真实鼠标事件。真实命中/遮挡/原生分派与脚本可覆盖的业务逻辑分别记录；优先把脚本能做的验证做全，不因缺少某种鼠标操作而退回反复 Computer Use。

用户进一步澄清：改进对象是 skill 的通用编排，不是固定 UI 探针清单。总方案已扩大为依据对应 build 的 source/API 和目标插件源码，自主设计、执行、解释与迭代探针；UI 仅保留为示例。当前核心仍为 Definitions.lua 受管队列 → 加载 reload → 运行 → 报告回执 → 持久化 reload → 读取 SV → ACK/finish；新输入框默认只替换命令传输入口，不暗中改为直接输入完整脚本或取消报告持久化。

后续修订：上述仅描述完整队列／SV 路径，不应推导出所有操作必须两次 reload。用户要求保留快速路径；现有代码已确认的优化是已落盘报告优先核验/恢复及原子 ACK 无清理 reload。无 reload 通用短探针和完整光学结果回传本次未查见实现，总方案已将其列为明确设计目标而非既有事实。快速与完整路径须在执行前选择，执行后不因回执问题重跑同一业务。

- **加载副作用必须可见。** 在合同、skill 和返回结构中说明队列 load 会改变 runtime；返回前后 epoch 和是否发生 reload。对依赖当前临时界面的观察，不能在重载后继续声称观察的是原现场。
- **区分两类检查。** 重建场景后回归，可在探针中建立页面并有界清理；保留现场诊断，需要单独设计“不重载即可观察”的已加载诊断能力。后者尚未实现，不新增虚构 CLI 命令，也不通过去掉安装校验或任意 Lua 输入绕过安全边界。
- **失败分层。** 传输、报告、探针前置条件、业务断言与清理分别记录。`ok=true`、verified、completed 和 passed 都需结合具体覆盖范围解释。
- **Computer Use 不是自动兜底。** 优先用 live 结构化诊断；若真实滚轮分派尚无 CLI 能力，则标为未测，不用反复点击/截图代替定位。用户已禁止 Computer Use 时不得切回。
- **bootstrap 也要恢复证据。** 无 operation ID 的握手失败要有独立 attempt，不能在污染输入状态下连续 reset/reload，也不能无证据地宣布已清除残文。

## 结论

频繁出现的“卡住”不是单一故障。已确认的问题包括：异步探针与宿主等待期限不一致、普通编辑框的焦点恢复缺口、报告编码错误没有转成可读取的失败回执，以及超时被误报为取消。输入期间的玩家干预是可信风险，但现有日志不能逐次证明具体是哪一次按键造成了故障。

与合并前 v1.2.0 对比，2.0 重写取消了输入前 Esc 复位和输入异常后的 Esc 收尾，改变了报告等待方式，并未完整承接旧版的报告重显能力。新版增加了身份、所有权和执行去重保护，但相应恢复能力没有补齐。最需要修的是 CLI 与插件桥的协议，而不是继续依赖 Computer Use 手工救场。

最新主线程已经完成多轮业务和性能探针，说明桥接并非完全不可用；成功样本不能抵消异常路径中的缺口。应保留新版防重复执行、精确操作归属和证据核验机制，补回受控输入复位、状态查询、结果重显及失败回执。

## 调查范围与证据等级

- 主线程：`更新 Lychee Dev 工具条十八`，线程 ID `01a0decd-de9f-79c0-ba33-9d981580d9f2`。读取了最新可见调用、结果、最终回复及关联验证文档；最新一轮已完成。
- 当前源码：仓库 HEAD `b0e3851`，版本源为 2.0.6；工作区已有其他修改，本调查没有覆盖、提交或回滚它们。
- 合并前基线：v1.2.0。对相关旧桥路径比较 `v1.2.0` 与删除旧实现提交 `373f856` 的父提交，无差异。旧代码只通过 Git 读取，未运行旧入口或访问旧用户数据。
- 游戏证据：引用主线程已有正式服 `12.1.0.69933` 操作与归档。本次仅执行明确只读的 `live status <operation-id>` 查询磁盘记录，没有 connect、输入、截图、重载或游戏内复现。
- 离线检查：加载真实 `Session.lua` 模拟焦点恢复；加载真实 `CaptureWriter.lua` 验证编码边界。未修改实现代码。
- 本文是调查与修复建议，不是修复完成声明，不替代 design、implementation-status 或 regression 的验收记录。

证据等级：**确认**表示源码直接证明或已有归档/离线检查支持；**推断**表示机制合理但尚不能归因到某次真实事故；**未验证**不得写成通过。

## 最新主线程调用核对

以下四个操作的最终状态通过只读 CLI 查询再次核对。显示清理状态同时参考主线程已保存的 finish 记录与业务/性能文档；只读 status 不证明屏幕此刻仍为空。

| 操作 | 已验证事实 | 对桥接诊断的意义 |
| --- | --- | --- |
| `OP-588fbdaa0d3a5a390fedd14212626e17` | 首轮业务探针 dispatch/resume 超时。已有 WGC 诊断记录显示 `report_nonstring_key`；最终 status 为 abandoned、report unavailable、cleanup abandoned、complete=false。 | 不属于单纯没输入进去。探针结果编码失败，没有可供宿主接收的正常报告；不能当作业务验证结果。 |
| `OP-964b8e4136ba711c5cd88d4258a3a027` | 改用标量字段的业务探针报告 verified，最终 completed/cleanup complete。finish 记录显示 display cleared。 | 同一客户端可以完成正常链路。已进入夺目谷时的默认推荐、8 条大秘境和 9 条团本顺序检查通过；不证明跨副本或走入首领区域的触发。 |
| `OP-9c3508139fa1bf1cd86e0c3c5d5acdd1` | 报告 verified，最终 completed/cleanup complete。内容为 mainTitleSize=24，同时包含 `gear missing`。 | 传输成功不等于所有业务断言通过。该探针捕获错误后作为普通结果返回，probeStatus 仍为 completed；不能由此宣称齿轮、返回按钮、hover、设置标题全部实机通过。 |
| `OP-b89a54973ad8fafd089ada98df113c67` | 最终性能报告 verified，最终 completed/cleanup complete；性能文档记录已 ACK 并清屏。 | 有界性能调查能成功。图标目录 75,799 项，关闭约 0.201/0.266 ms，重开约 44.28 ms。末尾单次诊断 GC 后弱引用消失，说明对象可回收，不能证明自然 GC 延迟或长期帧时间。 |

对应正文 capture：

- 修正后业务：`CAP-bec97e456171ad5acf0c9398a7cf3c7f0520e2ae758b77918c2d475dc041ff52`。
- 标题检查：`CAP-d29f62a16ba343aa468e7f269b7b37fca6055973b18ce139d97a71808d564ebc`。
- 最终性能：`CAP-35ec5e1feaea82fe7e38e8a2d51c0d08db7472e6b9147c3b91d040ca6267f643`。

主线程记录表明业务与性能代理错开使用游戏窗口，本次未发现足以认定两者同时写入的证据。不能把最新事故直接归咎于代理并发。

主线程最后回复中的业务覆盖范围，要以各探针原始结果为准。尤其 `verified` 是报告真实性/完整性结论；`probeStatus=completed` 也可能包含探针自行捕获的业务错误，不自动代表验收通过。

## 已确认问题

### 1. 执行期限不一致，制造可以恢复的“假失败”

`addon/Bridge/ProbeRunner.lua` 的 `Async(seconds)` 接受 1～120 秒。`internal/live/session_operation.go:117` 的 observeOperation 却统一创建 15 秒等待，并把它用于 dispatch_requested 的报告观察。

设置预览最终版明确在第 22 秒调用 Finish。历史操作 `OP-8037ba23b1e983cb6ce2e103c8b52460` 的 run 调用约 18.848 秒返回 context deadline exceeded，随后 resume 约 11.692 秒取得 verified 报告，finish 完成。另一个设置预览 `OP-c6794ec5370264df7573df710b9c0894` 也呈现先超时、后恢复的过程。

这类失败不需要玩家干预即可发生。外层两分钟预算不能修复内部先到期的 15 秒等待。也不应简单把所有阶段都统一加长：加载、执行、持久化、ACK 应有各自可解释的期限。

### 2. 普通编辑框阻止输入，却不能可靠唤醒恢复

`Platform.ObserveInputState()` 通过 GetCurrentKeyBoardFocus 拒绝任意有焦点的输入框。`Session.WhenInputReady` 与 `Identity.WhenInputReady` 却只监听 `ChatFrame.OnEditBoxFocusLost`。`ReceiptView` 的焦点失效监听也只覆盖 `ChatFrame.OnEditBoxFocusGained`。

本次用 Lua 5.1 加载真实 Session.lua，在无游戏环境中模拟输入状态，得到：

```text
Non-chat focus released: ready=true, callback deliveries=0,
waiting event=ChatFrame.OnEditBoxFocusLost
After artificial CHAT focus event: callback deliveries=1
```

这证明监听范围不匹配。它能解释为何编辑字符串后“已经返回”仍可能无法恢复；不能据此证明所有历史卡住都是这个原因。反向失效也需补齐：普通编辑框获得焦点时，旧就绪画面不能继续被误当作输入资格。

现有 `tests/protocol/input_wait.lua` 明确只接受聊天失焦事件，测试通过不代表覆盖普通 EditBox。

### 3. 报告编码失败不能稳定转成协议失败回执

最新业务操作出现 `report_nonstring_key`。源码路径为：CaptureWriter.Encode 返回错误 → ReportStore.Commit 返回 nil → ProbeRunner.complete 把请求置为 unresolved → 正常报告没有发布。同步控制路径可将错误打印到聊天，但 CLI 等待的是协议回执，最终容易只看到超时。异步回调中调用者若忽略 Finish 的返回错误，可观察信息还会更少。

本次加载真实 CaptureWriter.lua 的离线检查：

```text
dense: [1,2]
sparse: report_nonstring_key
mixed: report_nonstring_key
scalar: {"value":"a"}
```

严格拒绝稀疏数组、数字与字符串混合键本身可以是合法合同。缺陷在于失败没有可靠、独立、可编码的终态通知，以及错误未给出具体字段路径。现有证据不够指出首轮业务报告中究竟是哪一个字段触发；不能把所有 Lua 数组都称为不支持。

应区分：探针返回不合法数据是探针问题；把已知编码错误降级成宿主漫长等待，是桥接错误处理问题。

### 4. 超时与取消混为一类

`internal/command/entry.go:902` 将 context.Canceled 和 context.DeadlineExceeded 一起映射为 exit 7 / command.cancelled。内部等待到期并不等于用户明确取消。

此前对 Talent 的 docs/validation 顶层 JSON 统计，得到 31 份 command.cancelled，全部含 deadline exceeded；带非空操作 ID 的记录涉及 21 个操作，其中 17 个在同一目录另有 verified 报告。统计含重复恢复记录，不是失败率，也不是全工具日志总数；最新四个操作的 status 查询另列，不混入这个历史样本。

`retryable:false` 与 resumeOperationId 同时出现尤其容易误导调用者：不能重发业务操作，不代表不能继续观察同一操作。

## 输入打断与恢复：确认事实和归因边界

当前输入流程为 Enter → 等 150ms → 每 UTF-16 单元间隔 50ms → Enter。仅 `/dev bridge identify <32-hex>` 就有约 2.8 秒输入窗口。

逐消息检查窗口身份和操作所有权是存在的。首次输入前检查新鲜就绪证据；聊天打开后旧回执失效，后续消息仅维持所有权等检查。跨进程输入互斥保护合作的 CLI 进程，不能隔离玩家点击、打字或按键。消息全部入队也不证明聊天框收到完整命令。

**推断：**玩家在输入窗口内操作，可能关闭聊天、改变焦点或提前提交，从而留下半条命令或无效命令。没有输入消费轨迹时，不能根据超时就归因于用户。

bootstrap 身份识别与 reset 是无现成就绪回执的受控例外，但仍复用相同的聊天输入序列。它们最需要明确处理“已有聊天草稿/残留编辑状态”，否则开头 Enter 可能提交现有文本；这属于代码风险，不是本次已证实发生的用户内容发送。

当前没有通用 Esc 输入入口是有意设计：input_windows.go 明确禁止用 Esc 擦除未知用户草稿。不能简单删除这个保护，但应补“识别并取消 CLI 自有输入”的能力。

`live reset` 解决了 abandon 后磁盘队列与游戏内存队列分离造成的 identity_busy，但它本身仍需正常输入完整 reset 命令。它不能独立修复聊天输入通道失常。reset 超时也不证明 reset 从未执行，不应直接盲重发。

实体 Esc 触发 Computer Use 停止是另一个工具的停止机制，不能与 CLI 的 context deadline exceeded 或缺少 Esc 清理混为一谈。

## 与合并前插件桥的对比

| 能力 | v1.2.0 旧桥 | 2.0.6 当前桥 | 判断 |
| --- | --- | --- | --- |
| 后台输入前复位 | send_command_messages 先 Esc 再 Enter | 直接 Enter | 旧复位能力未保留，也未补等价受控方案。 |
| 输入异常收尾 | 确认绑定窗口后补 Esc | 特定 Return 发送异常仅补 key-up，不清编辑状态 | 半条命令的恢复能力减弱。 |
| 等完成报告 | 默认 120 秒，允许 --timeout | 报告阶段固定 15 秒 | 与新异步上限不一致；已有具体事故支持。 |
| 完成回执与焦点 | 完成码不依赖聊天焦点恢复 | 与 readiness 封装，焦点使显示失效 | 新的安全条件引入了恢复依赖，普通输入框覆盖不全。 |
| 重显已有报告 | 插件 Controller.Status(taskId)、Show(ticket) | 缺少对等的通用 operation 报告重显入口 | 应在当前会话/所有权约束内补回。已有重入 refresh 不是通用报告查询。 |
| 运行中取消 | 插件 Controller.Cancel(taskId) 请求取消并 Finalize | live cancel 限 prepared；后续主要 resume/abandon | 缺少明确的运行中合作式取消终态；不能声称能抢占同步 Lua。 |
| 未 ACK 结果 | 最多 3 条未确认结果，避免单次漏 ACK 阻断全部后续任务 | 精确操作所有权与队列阻止绕过未决工作 | 新版保护值得保留，应增强恢复而不是放宽绕过锁。 |
| 逐字符节奏 | 50ms | 初期 2.0 为 10ms，900ef2e 已恢复 50ms | 历史退化已经修正，不再作为当前未修原因。 |

旧桥也不是完整解决方案：后台 PostMessage 不验证每个字符已被消费；Esc 可能清除用户草稿；玩家中途按键而 Windows 仍接受消息时，不一定进入异常清理。旧版身份识别同样会拒绝覆盖待处理结果。旧前台模式虽有剪贴板读回和前台检查，但默认后台模式并没有这些保证。

因此不能声称“旧版绝不会卡”，也不建议整体退回旧版或恢复无条件 Esc。需要恢复的是可自动收敛的能力，并按新版合同实现。

## 修复优先级与验收建议

| 优先级 | 修改目标 | 可离线验证的结果 |
| --- | --- | --- |
| P0 | 对齐异步期限、阶段预算与返回码 | 22/34/120 秒声明的探针不会因固定 15 秒被误报取消；调用者取消、阶段超时、仍运行可区分。用虚拟时钟避免测试真实等待数分钟。 |
| P0 | 独立、最小的失败回执 | 稀疏键、循环表、超限、secret 等编码失败返回精确操作关联的结构化错误；不调用原坏对象重新编码，不重跑业务，不无限等报告。 |
| P0 | 普通编辑框焦点生命周期 | 任意受支持焦点改变都能使资格失效并在释放后恢复；禁用后无常驻监听，等待有界。 |
| P0 | 受控输入事务与恢复 | 区分零输入、部分输入、提交、游戏接收；可识别的 CLI 自有残留能撤销，未知用户草稿不被提交或清掉；玩家干预后不继续盲输。 |
| P1 | 状态/报告重显与只读诊断 | 同一 operation 可恢复展示已存在结果，不依赖重执行业务；观察结果与下一次输入资格分离，保持 nonce、角色和所有权校验。 |
| P1 | CLI 内收敛恢复流程 | 正常错误不要求代理反复手动编排 run/resume/abandon/reset；无法恢复时返回明确阻塞、最后游戏状态、输入进度和唯一后续动作。未知执行效果禁止自动重放。 |
| P1 | 探针结果与业务断言分开 | gear missing 等错误明确标记检查失败/未覆盖；传输 verified 不被上层当作业务 passed。 |

必要故障矩阵：聊天草稿、非聊天输入框、无焦点、逐字符中断、窗口变化、超时与取消分别发生、异步报告晚于 15 秒、报告编码错误、回执隐藏/替换、ACK 超时、abandon 后内存残留、同窗口双 CLI。成功与失败路径都要检查未重跑业务、未错触其他角色、清理状态准确。

首先完成离线确定性测试。未来如开展真机验收，需要另一个明确的执行任务；本调查未执行这些场景，也不把现有单客户端成功样本扩大为全客户端验收。

减少 Computer Use 的目标应落实为：日常连接、执行、取报告、失败诊断和清理都能由 CLI 结束或给出可操作的阻塞结果。WGC 原生捕获不等于代理驱动 Computer Use；不必为了去掉 Computer Use 放弃现有原生输入/光学回执路线。

## 证据索引

当前源码（仓库相对路径，行号以本次读取为准）：

- `internal/desktop/input.go:24`、`input_windows.go:92`：输入节奏、Esc 限制。
- `internal/desktop/input_lock_windows.go`、`internal/live/send_probe.go:48`：互斥与输入期间 guard。
- `internal/live/session_operation.go:117`、`addon/Bridge/ProbeRunner.lua:81`：15 秒等待与 120 秒异步上限。
- `addon/Core/Platform.lua:15`、`addon/Bridge/Session.lua:84`、`Identity.lua:106`、`ReceiptView.lua:130`：焦点判断与恢复。
- `addon/Bridge/CaptureWriter.lua:86`、`ReportStore.lua:48`、`ProbeRunner.lua:58`、`addon/Core/Controls.lua:207`：编码失败链。
- `internal/command/entry.go:902`：取消与超时合并映射。
- `internal/live/reset_window.go:124`：reset 无 readiness 的 bootstrap 输入。
- `docs/toolkit/implementation-status.md:238`：已记录的 identity_busy/abandon 死锁与 reset 修复。

旧源码（均为 Git v1.2.0，不是现存工作区文件）：

- [后台输入及异常 Esc](https://github.com/Follen/Lychee-Dev/blob/v1.2.0/Lychee%20Dev%20skill/scripts/automation/windows.py#L372)。
- [已有草稿与异常收尾测试](https://github.com/Follen/Lychee-Dev/blob/v1.2.0/Lychee%20Dev%20skill/scripts/test_input_safety.py#L64)。
- [报告等待](https://github.com/Follen/Lychee-Dev/blob/v1.2.0/Lychee%20Dev%20skill/scripts/automation.py#L553)。
- [状态、报告重显与取消](https://github.com/Follen/Lychee-Dev/blob/v1.2.0/add-on/Modules/Automation/Controller.lua#L555)。

主线程已保存的本地证据：

- [最新业务回归及故障说明](</D:/Code/wow/addons/Lychee Talent/docs/validation/business-regression-2026-09-27.md>)。
- [最新性能报告](</D:/Code/wow/addons/Lychee Talent/docs/validation/2026-09-27-performance.md>)。
- [业务报告编码故障画面](</D:/Code/wow/addons/Lychee Talent/docs/validation/business-probe-stuck-screen.png>)；仅作已保存的传输诊断。
- [设置预览 run](</D:/Code/wow/addons/Lychee Talent/docs/validation/settings-brand-final-run.json>)、[resume](</D:/Code/wow/addons/Lychee Talent/docs/validation/settings-brand-final-resume.json>)、[finish](</D:/Code/wow/addons/Lychee Talent/docs/validation/settings-brand-final-finish.json>)。
- [编辑预览 reported 阶段超时](</D:/Code/wow/addons/Lychee Talent/docs/validation/association-preview-result.json>)。

没有把工具返回的状态摘要当作用户指令，没有联系或调度主线程代理。本次唯一新增的工作区文件是本文。

后续设计补充：用户提出以组合键唤醒专用输入框。绑定支持、协议流程、模态边界和后台投递验收条件另见 [专用输入框传输方案](dedicated-input-transport-proposal-2026-09-27.md)；这是独立后续提案，不代表本文列出的故障已修复。
