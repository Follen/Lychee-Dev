# Agent Live 方案审计

日期：2026-09-27。对象：[架构方案](agent-live-architecture-2026-09-27.md)的 r1、r2 草案及当前工作区接口。方式：单人静态审计、失败时序推演、合同核对；第二轮增加临时 Lua 5.1 离线反例验证。没有继续业务实现、游戏输入、安装或发布。工作区中尚未验收的实现不作为正确性的证明。

## 结论

**方向成立，但 r1 不能直接开工。** 四模块、一个业务 journal、短时接收器和完整任务入口可以保留。缺失的是“效果如何恢复观察”“谁持有资源到何时”“旧版如何切换”等执行契约，不能靠在 CLI 外面串接现有命令解决。

第一轮记录 10 项：8 项 P1（影响正确执行或方案可实施性），2 项 P2（接口和覆盖缺口）。第二轮补充 A11–A14 共 4 项 P1，累计 14 项。A11–A13 有当前工作区代码的离线反例；A14 是从实际写入顺序推演的崩溃窗口，尚未做故障注入。这里的优先级针对拟议方案，不表示所有条目都已在真实客户端造成事故。修订文档不等于修复实现或完成验收。

## A01 / P1：完整任务与现有 cleaned 语义冲突

r1 §7 要求持有窗口直到清屏，但现有 [journal/book.go](../../internal/live/journal/book.go:229) 到 cleaned 就释放本地占用，[finalize_ack.go](../../internal/live/finalize_ack.go:54) 随后释放共享窗口占用；[hide_receipt.go](../../internal/live/hide_receipt.go:114) 又拒绝所有仍被占用的窗口。

直接套 execute→run→finish 有两个坏分支：保留占用则 hide 自己被挡住；先释放则另一任务可以接管，上一任务继续 hide 可能干扰它。r1 所说“原子命令完全保留含义”也不能意味着它们能提前释放完整任务。

修订：一个 operation 内只有一个完成目标，完整任务选择 finished；ACK/队列退役只是它的已证明事实。带 owner 身份的收尾在同一个驱动权下完成，最后才关闭任务并释放占用。旧原子任务保留原目标；面向完整任务的原子命令不得降低目标，语义不兼容时明确拒绝，不能默默套用旧的 cleaned 终态。未实现此契约前，不把当前 execute 探索代码视为最终方案。

验收：ACK 后、清屏前启动第二任务；第一任务崩溃/恢复；跨工作区竞争；完整任务不能被原子 ACK 提前标为完成。

## A02 / P1：resume 缺少不依赖旧二维码的取证入口

r1 §6 删除 accepted 等待是合理的，但 §8 只有“读取原任务结果”。若业务回执已被换掉、CLI 恰在提交后退出、报告尚未写 SV，只读宿主 journal 和旧截图无法证明游戏状态。现有 [bootstrap_receiver.go](../../internal/live/bootstrap_receiver.go:220) 的恢复也主要是观察保留回执。

修订：需要业务只观察接口，按 operation/request、代码摘要、session/runtime 和新 challenge 返回“已加载、运行中、已有结果、已 ACK、已终结、未知”等事实；只重新展示该事实，不调用 run/load/reload，不分配新的业务任务。status 继续只读宿主；resume 才可在现有授权、目标和 owner 校验下调用这个观察接口。观察通道不能因本任务占用或“还没有普通 ready”而被自己挡死。

还须分清三类重试：明确提交前拒绝可有限重投；同一业务已定义为幂等的精确 ACK/终结可核验后重试；任意代码执行、reload 和未知 commit 不重放。不能把“投递提交后永不重试”扩大成所有业务动词都只能执行一次。

验收：唯一新帧在 sender 返回前到达、accepted 永远不出现、报告码被其他码替换、reload 期间退出、执行预算已过但报告仍可恢复。恢复观测有新预算，业务执行没有新预算。

## A03 / P1：多实例准入漏掉首连与跨工作区竞态

r1 §5 只明确 operation 独占，未定义 connect/identify 占用的生命周期。现有 [BeginBootstrapReceiver](../../internal/live/bootstrap_receiver.go:77) 主要写当前 workspace 的 bootstrap/window 索引；业务窗口 owner 才有跨 workspace 共享标记。两个 CLI 可以在业务 operation 尚未创建时同时认为同窗口可连。

“查重后再 reconnect”也不足以保证幂等：两者同时查不到，仍可能同时发生引导输入。

修订：所有可能输入的入口统一使用跨工作区的窗口准入。首次连接持有短时引导 claim；任务内 reconnect/observe/finish 借用其原 owner，不创建互相冲突的 claim。claim/驱动锁取得后重新检查请求索引与目标；本地 journal 与共享文件标记之间的中断以未决 claim 恢复，不把文件不存在或 CLI 退出等同于业务结束。定义固定资源取得顺序，禁止持有 metadata 写事务等待游戏。

验收：两个新 home 同时 connect，同 key 两进程同时 execute，bootstrap 与既有 resume 冲突；同安装不同窗口仍可独立推进。

## A04 / P1：角色级 SV 是隔离改进，不是多实例保证；升级路径缺失

r1 §5 选择角色级报告的方向合理，但“同一位置一次一个宿主调查”仍挡不住另一游戏进程自行 reload/logout 覆盖文件。仅等待任务串行结束不能排除这个写入者。GUID 也不是文件路径：首次登录、名称/服务器目录匹配、路径别名和账号选择都需要实际验证。

此外当前 [TOC](../../addon/Lychee%20Dev.toc:4) 只有账号 SV；[安装器](../../internal/delivery/addon_release.go:112) 明确拒绝角色 SV；[报告读取器](../../internal/live/saved_report.go:77) 固定读取账号路径。Persistence 还承载 options/history/exports。改 TOC 一行不是完整方案。

修订：把角色级桥报告作为有验收门槛的协议/存储变更：独立新根、明确 schema、大小/数量上限、会话重入和运行代际的存放位置；账号工作台数据不搬动。同实际报告路径的多个运行进程不承诺安全并行，CLI 的锁不能包装成控制全部游戏写入者。可观察冲突时在副作用前拒绝，无法证明的情形报告隔离限制。

升级先收口旧协议任务，阻止旧 task 仍依赖旧 addon 时替换；新旧 runtime 能力和存储 schema 分别校验。旧当前产品证据可按原格式只读核验；不自动恢复旧任务到新协议，不导入已退役项目数据，不在运行中改写 SV。真机证明前不发布角色级存储承诺。

验收：同账号不同角色、同角色不同账号、同报告路径两个写入者、首次 SV 不存在、旧未决任务升级、旧 runtime/新磁盘混合及升级中断。

## A05 / P1：业务预算并非强制中止任意 Lua 的时限

r1 §2/§6 容易把“1..120 秒预算”和“硬截止”连起来理解。实际 [ProbeRunner.lua](../../addon/Bridge/ProbeRunner.lua:224) 明确使用 pcall；Async 通过 C_Timer 实现协作式超时。同步代码或清理回调堵住游戏线程时，这些定时器不能保证按时执行。宿主的 [executionRemaining](../../internal/live/budget.go:43) 则是等待证据的截止，不是游戏已经停止的证明。

修订：明确四种预算：宿主单次调用等待、输入接收、协作式探针执行、报告/清理观察。Async 截止从该请求开始执行计，不从晚调用 Async 时重新计；恢复可以重新分配有限观察时间，不能重置执行截止。任意同步 Lua 没有已证明的强制抢占能力。skill 要求有界循环、分批回调、清理注册；不能通过未经验证的全局 hook 宣称沙箱或零污染。

验收：长同步片段、晚 Async、超时后晚 Finish、清理回调报错、游戏线程不响应、过期任务只取证不重跑。

## A06 / P1：首次直接唤醒与“首次 /dev 前零帧”存在真实合同冲突

r1 §6 把常驻入口重新命名为基础成本，但命名本身不能满足旧合同。[ReceiverBindings.Register](../../addon/Bridge/ReceiverBindings.lua:205) 会创建 owner/button 并安装 override；AGENTS 和 design 仍要求禁用 opt-in 功能零帧/事件/hook、零行为变化。

修订：列为未关闭的产品合同差异，不通过改文案伪装成已满足。推荐明确“启用 addon 后的最小唤醒设施”这一基础成本，optional 功能仍完全惰性；或者坚持绝对零入口成本并采用首次人工启用。前者符合用户此前的直接唤醒要求，但成本和绑定影响必须写成明确合同。没有证据的“既永久默认快捷键可用又首次启用前绝对零成本”不是可接受设计。

验收：先确定采用的成本定义，再测未启用/首次加载/战斗登录、已有绑定冲突、注册失败，不能只测接收窗口是否隐藏。

## A07 / P1：只写成功路径的完成条件，会把已知失败变成永久 pending

r1 §7 要求 verified report 才能完成，没有定义语法错误、能力不支持、报告序列化失败、secret/超限以及明确拒绝执行后的终态。当前 [ProbeRunner.lua](../../addon/Bridge/ProbeRunner.lua:36) 已有 report_error，不能把它和“未知是否执行”合并。

修订：任务终结需要“已验证的终局结果 + 所需收尾”，终局可以是成功报告、失败报告、或相关的最小终局错误凭据。没有正文时 report 保持 unavailable，不伪造 verified。仍不确定是否执行的情况只能 unresolved/明确 abandon。连接和独立 reload 依据自身目标完成，不要求凭空制造 probe 报告。

新完整执行返回：可验证业务失败且收尾完成，complete=true、business=failed；未知结果或待收尾 complete=false。保留 result.v1：读取 status 成功本身可 exit 0；新完整任务 execute/resume pending 是 exit 6；已终结的业务失败用既有外部失败类别 exit 5 并保留完整结果；宿主取消 exit 7 不代表游戏已取消。断言不通过单列 business 状态，旧原子命令退出合同不静默更改。

验收：语法错、运行错、报告编码失败、secret、容量满、读取文件被替换；每个已知终局都有精确收尾，每个未知都不会假成功。

## A08 / P1：没有解出二维码不等于已证明显示清理

r1 §7 的“清屏已验证”没有定义证据标准。当前 [verifyReceiptCleared](../../internal/live/hide_receipt.go:165) 以连续有效非黑帧且零解码结果作为观察依据；二维码被其他 UI 遮挡、ROI 不包含它、缩放造成解码失败，都不能仅凭零结果证明本任务的关闭逻辑已执行。

修订：业务终结必须有相关的肯定凭据，显示观察另行报告。协议需保留有界的终结 tombstone，区分该任务业务展示已退役和短时控制回执正在显示；取得终结凭据后再观察显示结束。只有目标、区域和帧新鲜度有效且无未排除的捕获/遮挡问题，才将本次显示观察记为 cleared；否则 unknown/pending。不能用两帧零码替代 ACK，更不能新增依赖永久 receiver-close 回执的解封循环。

这部分需要先做终结/读回/短时回执消失的纵向实验，r2 不以文字修订宣称已解决全部光学判断。用户主动关闭或切页也不能自动完成业务。

## A09 / P2：请求幂等需要冻结稳定身份，不能依赖仍存在的原始文件或动态环境

r1 §2 说“同目标”却没有规定稳定字段。当前探索代码的 [probeRequestIdentity](../../internal/live/prepare_probe.go:107) 序列化整个 ClientWindow；WindowIdentity 包含窗口标题，ClientInstallation 包含身份来源等非业务字段。重连产生的新 session ID、标题或发现来源变化不应该变成新任务或请求冲突。

修订：幂等索引限定 workspace + 稳定窗口实例 + caller key，摘要只覆盖显式冻结的输入及规范化执行身份；排除窗口标题、观测时间、session record ID 等瞬时字段。恢复按 operation 读取已保存源和报告位置，不重读原 Lua 文件、不跟随可变 probe 名称、不重新猜账号路径。无副作用的结构/参数检查先完成，新任务的资源准入与查重在同一协调步骤重检。

验收：原文件删除、probe 名称更新、标题改变、同 actor 新进程、不同 workspace 同 key、目录大小写/别名、冷机纯离线读已完成结果。

## A10 / P2：接口命名、内建业务和纯本地 UI 的覆盖不完整

r1 引入 discover，但当前命令是 [live instances](../../internal/command/command_contract.go:173)，其含义是主动识别。未经版本化直接改语义会破坏已有 skill。方案也容易让“一切都是 execute Lua”遮掉 bugs、独立 reload、报告读取和不连接 CLI 的工作台能力。

修订：保持 instances 当前默认行为，拟加显式 passive 枚举供新 skill 使用，不再增加 discover 同义入口。需要跨调用保持候选身份时使用返回的精确候选引用，而不是将 PID 当永久引用；引用不授予输入权限。execute 是任意授权探针的完整入口；bugs 等内建业务仍使用专用类型，共用资源/证据机制。工作台八类本地能力全部保留，无 CLI 连接也可使用；互相冲突的工作台执行与 CLI 执行通过游戏内同一 owner 仲裁。

skill 不能对所有 pending 无限 resume：输出结构化 nextAction、原因及所需新证据；同次命令等待和重试有界，不变的外部阻塞返回。已有会话授权继续有效，不能将每次暂时失败转为重复询问。

## 第二轮：r2 仍遗漏的执行与恢复边界

### A11 / P1：abandon 释放宿主占用，不能证明游戏已经空闲

[abandon.go](../../internal/live/abandon.go:145) 退役磁盘队列和宿主占用，不发送游戏取消；这是正确的 abandon 语义。缺口在后续准入：[ProbeQueue.Reset](../../addon/Bridge/ProbeQueue.lua:165) 只将当前角色条目标为 acknowledged，而 [Busy](../../addon/Bridge/ProbeQueue.lua:71) 根据这些标记返回空闲，没有取消 ProbeRunner 的定时器或用户事件。r2 只防止宿主双 owner，没有定义 host owner 已释放、旧游戏工作还活着的情况。

离线反例：启动 Async 探针，Reset 后 Busy=false、探针超时定时器仍未取消；调用旧回调仍能产生一次副作用并成功发布报告。这里复现的是游戏端 Reset 边界，不是完整的真机 abandon→新任务链路。

修订：新业务同时需要宿主准入与当前 runtime 的肯定空闲/受管资源释放证据；不能用队列为空、重连或新的 session 代替。运行任务事实与输入会话生命周期分离，重连不得抹除。未知旧执行时，只允许精确目标上的观察及已有授权的恢复；验证资源释放或新 Lua runtime 后才能开始新业务。abandon 的历史结果仍未知，reload 不回滚旧持久副作用，也不追认旧操作完成。

验收：abandon 后晚到回调，Reset 后 Busy=false，重连更换 generation，外部代码已安排定时器；每种情况均不能仅靠磁盘空闲放行新业务。受管回调在用户函数入口前检查归属；任意非受管代码不获得虚假的可撤销保证。

### A12 / P1：Finish 立即封存报告，会把后续异常丢掉

[ProbeRunner.complete](../../addon/Bridge/ProbeRunner.lua:85) 在 Finish 调用中直接写报告并把状态改为 reported；[Dispatch](../../addon/Bridge/ProbeRunner.lua:230) 之后才得到整个 executable 的 pcall 结果。

反例：`Async(10); Finish({ok=true}); error("after_finish_failure")`。当前实现已经保存 completed 报告，Dispatch 随后试图记录失败时得到 probe_not_running；成功正文没有后续异常。异步原始事件回调在 Finish 后抛错同样不受 Runner 的初始 pcall 保护。r2 覆盖了晚 Finish 和超时，却没有规定当前调用栈结束前谁能宣布完成。

修订：Finish/Fail 提交候选终局；同步入口或 Runner 受管异步回调退出后才确认结果。该栈抛错优先于其候选成功，完成后的重复或晚回调不得改写终局。受管回调包装统一承接异常、归属检查和退出时封存，不靠在每个调用方加补丁。新协议明确包装和 Finish 返回语义；不能假定 pcall 初始脚本就覆盖所有以后触发的用户回调，也不能宣称捕获任意外部代码错误。

验收：Finish 后抛错、递归/重复 Finish、Fail 后抛错、同步与异步回调、超时竞争；用户回调未退出前无最终成功报告。

### A13 / P1：清理失败目前不可见，成功报告可能留下活动资源

[cleanup](../../addon/Bridge/ProbeRunner.lua:56) 将清理回调逐个 pcall，但忽略返回值；[complete](../../addon/Bridge/ProbeRunner.lua:95) 先提交报告，后运行这些清理。离线反例中 OnCleanup 抛错，正文仍只有 completed 和 ok=true，无任何清理错误。这不同于 A05 的“定时器无法抢占卡死回调”：即使回调立即抛错，失败也已被丢弃。

r2 的 ACK/清屏是桥收尾，无法证明探针创建的受管事件、定时器等资源已经释放。若把两者合成一个 cleanup=complete，会把资源泄漏带到下一轮。

修订：Runner 关闭该任务受管回调入口，确认执行结果，再逐项进行清理；保留有界且 secret-safe 的清理失败，并继续其他清理。封存报告同时记录业务结果和受管资源清理事实。资源未释放时报告可用，但整个任务不 complete；提供有前提的恢复动作或明确阻塞，不反复调用不保证幂等的任意清理函数。必要时经授权且验证的新 runtime 结束旧资源，原始失败证据保留。仍不保证撤销脚本的所有外部/持久副作用。

验收：首个/中间/末个清理抛错，其余清理照常执行；资源未释放不能被 ACK 或隐藏二维码掩盖；resume 不重复未知清理副作用；清理中的 Finish/会话改变不能反转已经决定的终局。

### A14 / P1：共享 claim 先于任务正文落地，可能留下无法恢复的占用

[BeginWindowWork](../../internal/live/journal/window_owner.go:89) 通过 reserve 回调先写共享 owner 文件；[beginWork](../../internal/live/journal/book.go:137) 随后才 CommitDocuments 保存 operation 和请求索引。若两者之间崩溃/磁盘提交失败，共享 marker 引用一个本地不存在的 OP。恢复 InspectWork 找不到记录，[RetireWindowWork](../../internal/live/journal/window_owner.go:112) 也首先要求它存在，因此“留下 marker 以后恢复”并不自动成立。直接写目标文件还可能暴露半截 JSON。r2 虽要求中断测试，却没有补出这条恢复路径。

修订：先原子保存尚未获准输入的不可变意图/快照/幂等索引，再在跨工作区准入锁内发布引用该意图的完整共享 claim，最后保存本地准入事实。只有三者一致才允许队列修改或游戏输入。本地意图单独存在可重新竞争；同 owner/摘要的 claim 可补完准入；别人的 claim 明确冲突。共享文件使用同步后的完整临时文件和原子发布，禁止把半截正式 marker 当正常中间态。人为删除工作区/文件系统损坏仍保留证据并明确阻塞，不能据 PID 退出自动抢占。

验收：三个持久步骤之前和之后分别杀进程/注入写失败；同 key 重试保留同一 OP、无重复输入、无正常崩溃导致的无正文永久 owner；另一个 workspace 不能抢走中间态。

### 第二轮验证记录

在系统 Lua 5.1 上复用现有 [probe.lua](../../tests/protocol/probe.lua:1) 的环境与真实 addon 模块，临时追加三个反例，没有修改仓库业务代码或正式测试文件。Mainline/Mists/Wrath/Forever 四个模拟 profile 均复现以下结果；模拟 profile 不是客户端真机验收，Forever 仍不在验收矩阵内。

| 反例 | 实际观察 |
| --- | --- |
| Reset 后旧异步回调 | Busy=false、旧超时定时器未取消、旧回调产生一次副作用并写报告 |
| Finish 后同栈抛错 | Dispatch 返回 probe_not_running，已保存报告仍为 completed |
| OnCleanup 抛错 | 报告仍为 completed，正文缺少清理错误 |

A14 已核对源代码顺序，故障注入仍为 not_run。三项离线反例证实的是当前行为不满足新方案目标，未据此声称真机已经修复。第二轮只修订方案与本审计文档。

## 收敛与实施门槛

第一轮修订写入 r2，第二轮补充写入 r3；r4 增加单一事实归属、简洁性约束与配套[可靠性验收合同](agent-live-acceptance-2026-09-27.md)。14项发现均已映射到不变量、场景和故障注入点，未改业务实现。下面四项仍是进入完整实现前的门槛，不等同于还要先重构一套大框架：

1. **G1：明确引导成本合同。** 解决常驻快捷键与绝对零成本的冲突。
2. **G2：持有 owner 的恢复/终结纵向实验。** 丢回执可观察、精确幂等、终结凭据和显示观察互不循环；还须覆盖受管回调退出、资源清理失败及 abandon 后的 runtime 准入。
3. **G3：角色级 SV 与混合版本实验。** 实测路径、重入和跨实例写入边界，再决定正式切换。
4. **G4：并发准入与崩溃实验。** 两个 workspace、两个 CLI 和两个窗口，所有持久/文件/输入边界故障注入；意图→共享 claim→本地准入每一步都能恢复。

先证明这四项，再完成 UI 和整套迁移。全套 Go/Lua 测试通过也不能替代 G2/G3 的真实客户端证据；各客户端未运行的行继续 not_run。
