> Historical reference for the Rust migration (2026-10-14): current decisions, baseline and acceptance status are in [handover.md](../../handover.md). This document preserves its original scope and evidence; conflicting requirements, protocol designs, performance targets and release instructions are superseded. Historical test results are not current-candidate passes.

# Native Live 协议候选：单一 Mailbox 与统一推进（2026-10-08）

状态：**设计候选，未定稿；不得据此开展大范围替换。** 本文不是旧 EXECUTE/SETTLE 的改名，也不是实现通过记录。消息、参数和槽位布局是供反例验证的完整候选；标为阻断的假设尚未解决。静态阅读、模型、真实 Lua 5.1、Windows 原型、最终游戏验收分别记录，不互相替代。

需求来源：`rust-refactor-requirements-2026-10-08.md`；独立审查登记在 `rust-native-protocol-audit-2026-10-08.md`。旧协议和现有代码仅用于提取业务能力及失败轨迹。当前约定取消色块、光学心跳、WGC 及其回退，不能再以它们补足任何证明。

2026-10-12 最新定位修订：Mailbox 参考 Go `v3.1.0`，运行时指令特征匹配/RIP 解析动态 Lua 根，再沿 state → globals → Mailbox 读取；停止私有堆扫描。身份、认证、本次观察、exact COMMIT、持久结果及统一预算继续适用。见 [总方案 §7](rust-refactor-plan-2026-10-08.md#7-mailbox-运行时指令定位与-lua-路径读取)；旧原型证据只证明原受测范围，不代表新入口通过。

## 一页核心

**游戏只有一个私有当前任务，主机只有一个推进器，公开只有一个不可变状态快照及至多一份结果正文。** 游戏私有状态决定能否接受；主机只能根据验证过的 Mailbox 事实推进。主机持久化 intent 在先，可执行文件发布在后；文件一旦可能被游戏看见，就不能再声称“肯定未发送”。

正常任务：

1. 主机读取两次真正推进的、属于目标实例的有效快照，确认输入条件满足；同一快照重读不续期。
2. 主机将任务完整内容和槽位占用原子记入 intent，发布一个 `RUN`，向固定窗口投递一次受限唤醒。游戏再次检查身份、私有状态、输入条件、时限和内容承诺后，接受并执行一次。
3. 游戏发布终态及不可变正文。主机完整读取、验证、落盘并重新读回校验。终态与主机持久化是两个不同事实。
4. 主机发布一个 `RECEIVED`，声明已保存的 exact task/result digest。游戏先完成执行资源清理，再释放正文，最后发布完成清理的快照。下一项任务随后使用同一个流程。

默认一次任务两个可执行消息；没有独立 accepted ACK、finish ACK、历史 ACK 或状态查询消息。快照同时表达当前任务、最后命令结论及输入条件。取消和关闭各自是同一命令格式的另一种操作。恢复只是从已持久化事实重新运行同一推进规则，禁止第二套重放输入引擎。

最后一个确认不能直接删除：游戏不能通过读取自己内存知道另一进程是否已把结果持久保存。省略它，要么可能丢失唯一结果，要么结果永远保留。默认自动清理和单次命令的完成语义也不能依赖“下一项任务会顺便确认”。以后可验证可选合并，但不得先删除独立收尾能力来追求少一次往返。

候选规模：每安装 500 个 LoD；8 条互不交叉的 lane，每条 62 个，余 4 个不参加本版本分配；每实例最多一个任务和一个未确认正文。8 是候选支持上限，不是已验收事实。只在有活动 CLI、输入观察租约或执行任务时进行有界观察；禁用态保持既有批准的 bootstrap 预算，无周期工作。

**不能提前承诺的事项：** (B1) 内存读取与 OS 投递之间发生暂停时，当前没有“绝不投递键”的原子证明；(B2) 所有客户端的 runtime 唯一性、时钟暂停语义与 Lua 可用原语须证明；(B3) 公开 Mailbox 完整伪造不能只靠 checksum，认证候选须在真实 Lua 验证成本与密钥边界；(B4) 任意同步 Lua 或任意外部清理回调不具备可证明的抢占/幂等性。上述事项是阻断项，不是暗中放宽的要求。

## 1. 简化选择与删除反例

| 结构 | 唯一职责 | 删除或合并后的最短反例 |
| --- | --- | --- |
| 私有 `currentTask` | 唯一接受、取消、完成与释放权威 | 改公开表即可假装没有活动任务并接受第二项 |
| 不可变 `STATE` 字符串 | 同一时刻的输入条件、身份、任务和命令结论 | 多张表分别读到新身份、旧就绪、另一任务结果 |
| 至多一份 `BODY` 字符串 | 保留完整结果，避免每次刷新输入状态复制大正文 | 把大正文每次写进状态导致周期复制；不保留则主机崩溃丢唯一结果 |
| 单一命令格式 `COMMAND` | 对所有有副作用操作统一验证、去重及消费槽位 | 多条控制路径各自解释 owner、期限与恢复 |
| 主机 session journal | intent、输入不确定性、持久结果的权威 | 已发布文件遇迟到键执行，主机却没有任务记录 |
| 安装 allocation ledger | 共享安装下 lane 独占与代际约束 | 两个 CLI 各自认为自己拥有同一文件名 |
| 结果内容对象 | exact 正文及校验；journal 仅引用它 | 把结果复制进多份状态，崩溃后不知道哪份权威 |
| `RECEIVED` | 将主机已保存事实传到游戏 | 游戏过早删除结果，或永远不能结束最后一项 |
| 活动期 publication cadence | 仅凭 Mailbox 证明游戏最近仍在推进 | 静态 ready 记录在暂停后被反复认作新鲜 |

不新增单独的 route book、slot manifest、recovery journal、收妥队列、公开 INPUT 表、公开 HEAD 状态机或光学许可。内容文件的 hash 可由 journal 重建；allocation ledger 只保存活跃/隔离 lane 及其世代，不复制任务正文或完整历史。

认证方案比较：

| 候选 | 主机能否抵挡完整公开表伪造 | 复杂度与结论 |
| --- | --- | --- |
| 校验和 + 私有执行门 | 否；可伪造 ready、结果、清理完成 | 不符合要求，淘汰 |
| 私有安装密钥认证 `STATE` 和命令，结果用认证状态内的强摘要绑定 | 在第 2 节的限定威胁模型内有机会 | 首选原型；纯 Lua 成本、密钥初始化、重放尚待独立验证 |
| 从私有 Lua 对象另读可信影子状态 | 可能，但另建了状态通道 | 与 Mailbox 唯一读取通道相冲突，淘汰 |
| 继续光学/WGC 辅助 | 不能满足本次范围 | 已明确取消，不作为备选 |

这里选择认证是因为已有具体伪造反例，不是预先指定密码学复杂度。原型若找出更简单且通过同样反例的真实性机制，应替换首选方案并重新审查。

## 2. 参与者与信任边界

参与者：Rust CLI/推进器、每安装文件协调器（进程内库加 OS 文件锁，不常驻服务）、Windows 进程/窗口/输入适配器、游戏私有 Lua 协议实例、公开 Mailbox、业务适配器。定位器只交付经运行时指令/RIP 验证的根绑定、Lua 路径读取字节及覆盖事实，从不交付输入许可。

可信前提：OS 对进程 creation identity 和文件锁的实现、受测可执行文件及 Lua 运行时、经受管安装验证的私有协议代码、批准的业务适配器。公开 Mailbox 中的所有路径、字符串、字段和 checksum 都不可信，必须独立验证。攻击方可任意修改公开 Mailbox，包括观察真实历史记录后复制、重排、重编码和伪造全部公开字段；协议不得因此授予输入、执行、取消、结果确认或 lane 释放。

首选认证候选：安装时由 Rust OS 随机源生成 256-bit 安装密钥，写入单独的受管私有 Lua 文件与主机安装记录；不得使用分发包内的全局固定密钥。Lua 加载后仅在私有 closure 持有；不向公开表、日志、结果或任意参数签名接口暴露。唯一签名函数签协议构造的固定类型状态，不为其他 addon 提供任意内容签名。`STATE` 和 `COMMAND` 使用 domain-separated HMAC-SHA256；`BODY` 摘要为 SHA-256，摘要由状态 MAC 覆盖。先限定协议 v1 的一个算法，不做运行时协商。

该边界不抵抗能任意改私有 closure、替换受管文件、调用 OS 写目标任意内存或窃取安装密钥的攻击者。**不能将“修改公开表的 addon”自动扩张为这些能力从而排除用户明确要求的测试。** 需要原型证明普通公开表篡改不会得到私有签名接口。Lua 调试/反射 API 能否破坏此边界是 B3 的一部分；若实际 API 无法隔离，认证方案尚未成立。

主机可证明：指定进程/窗口身份、读取的 exact bytes、MAC/摘要/记录格式验证结果、观察到的 publication 推进、自己保存的字节、OS 输入调用的已知/未知边界。主机不能由静默推导游戏失败，不能由投递成功推导接受，不能由进程退出推导任务未执行。

游戏可证明：私有 currentTask、已接受的 exact COMMAND、自己执行和清理的事实、当前安全输入条件。游戏不能证明主机物理磁盘永不损坏；`RECEIVED` 是有权限主机按协议作出的持久化声明。主机必须在每次发布该声明前重新验证结果文件。

## 3. 身份与绑定

| 身份 | 定义/权威 | 变化后的处理 |
| --- | --- | --- |
| client/build/protocol | 受管客户端声明、验证过的 Mailbox build/接口/协议及 runtime recipe/layout profile | build/协议/布局变化重新定位；同 build 不共享地址，根或 layout 身份不授予输入权限 |
| installation | 规范路径 + 卷/目录文件身份 + 安装 generation +受管清单 hash | 升级先处理未决；不靠路径相同继承身份 |
| process | OS 只读进程句柄 + PID + OS creation time；映像路径核验目标身份 | PID 相同但 creation 不同是新实例；不分析 EXE 内容来定位 |
| window | HWND + owning PID/creation +当前目标窗口证据 | 销毁/重建立即撤销输入许可，重新观察 |
| actor | client flavor/region + realm + character GUID；secret/unavailable 明示 | 角色变化不续接旧任务 |
| runtime | 当前私有 Lua 实例的 incarnation +成功绑定的主机随机 runtime token | reload/relogin 后不得沿用旧任务权限 |
| owner/session | 主机随机 owner/session 256-bit token；受安装锁和私有绑定约束 | owner 变更必须关闭/隔离旧 binding，不盗取未决 |
| task | session 内单调 task number + caller key +完整任务 digest | 相同 key/内容可只读取历史；不同内容冲突 |
| command | binding 内单调 command number +完整 message digest | exact duplicate 只读返回结论；同编号异内容拒绝 |

所有授权字段都包含在 MAC/digest 内。原始消息字节使用规范编码；nonce 是固定长度小写十六进制，不接受浮点数、重复字段、别名拼写或未知字段。没有“只比较 key 和 code，忽略 budget/policy”的去重。

Runtime 不能只用 SavedVariables epoch、`math.random()` 或秒时钟宣称唯一。绑定分两层：启动记录仅用于无业务观察；主机在获得安装 lane 排他权后，用新的 OS 随机 token 绑定选定进程，游戏回显并私有保存。跨实例发现相同未绑定 incarnation 或任何绑定歧义时拒绝，不能靠“概率很低”宣告通过。**B2 仍要求证明 reload 后旧 BIND 不会被新 runtime 接受为原 binding；候选不得把主机 token 本身当作该证明。** 原型必须故意令 PRNG、时钟、SV 和内存地址复用，审查启动 incarnation/nonce 的可实现来源。此项未闭环前不能启用业务。

不可变指令 recipe/layout 描述可供实例复用；module base/root binding、state/对象路径、publication baseline、binding 和窗口按 process identity 独立验证，不共享地址或授权。每次从当前根沿命名 Lua 路径读取并前后复核，不缓存 heap 地址，不做私有堆扫描回退。

## 4. Wire 格式与单一公开快照

协议候选名 `lycheedev.native.mailbox.v1`，与旧协议版本独立；新 host 不猜测旧记录的字段含义。正文和状态均发布为新建不可变 Lua 字符串；通过替换根表字段发布，绝不原地修改已发布字符串。

`STATE` 逻辑字段（编码原型需输出单独逐字段 schema）：

```text
version, installGeneration, clientBuild
binding {processToken, runtimeIncarnation, runtimeToken, actor,
         owner, session, lane, laneGeneration} | unbound
publication {revision:u64, gameClockMicros:u64, leaseEndMicros:u64}
input {ready:bool, combat:bool|unknown, focus:clear|blocked|unknown,
       loading:bool|unknown, reason:enum, permitRevision:u64}
connection = unbound|bound|closed
lastCommand {number, digest, outcome=accepted|rejected, reason}
task = none | {number, keyDigest, requestDigest, phase=running|terminal,
               outcome?, resultDigest?, resultBytes?, cleanup=pending|blocked|complete}
slots {loadedBitmap:u64, nextOrdinary:u8, mode=normal|fused|exhausted}
mac
```

`BODY` 包含版本、binding digest、task number、request digest、结果编码长度、结果正文。`STATE.resultDigest` 承诺 **完整 BODY 字节**；任何头或字段错配都不得返回业务结果。先发布 BODY，再发布引用其摘要的 terminal STATE；只有后者是提交点。读 BODY 后重读并认证 STATE 的 task/result commitment；publication revision 可正常增加，但 commitment 必须保持相同。

`COMMAND` 统一字段：版本、完整 binding、command number、command digest、目标 exact slot、操作、操作字段、input permit revision、绝对游戏时钟有效期、MAC。`BIND` 使用经审计的未绑定 challenge；其他操作必须匹配已绑定私有身份。完整消息 digest 覆盖全部字段（digest/MAC 自身以定义好的空位处理），不能排除任务预算、代码、policy、取消目标或结果摘要。

操作只有：

| 操作 | 必要字段 | 私有前置条件 | 接受后的作用 |
| --- | --- | --- | --- |
| `BIND` | 新 binding、启动 challenge、lane generation | unbound；独占 lane；有效启动许可 | 绑定当前实例；无业务执行 |
| `RUN` | task、caller key、code/content digest、budget、policy、业务参数 | bound；task=none；可用槽和预算足够；游戏输入 gate 通过 | 先私有登记 exact task，再调用业务一次 |
| `RECEIVED` | exact task/request/result digests | terminal；主机声明 exact result durable | 执行有界清理；清理全成功后释放 BODY，task=none |
| `CANCEL` | exact running task/request digest、原因 | bound；同一任务仍 running；支持协作取消 | 请求取消；取消与完成争用唯一 terminal 提交点 |
| `CLOSE` | binding、可选下一步 reload 意图 | task=none；无清理义务 | 私有禁止后续 RUN，发布 closed；lane 仍待证明可回收 |

拒绝是已消费命令的结论，不是业务失败。可认证的拒绝记录包含 exact command digest 和原因；损坏 MAC/格式时只提供有界诊断，不伪造对应 command accepted。拒绝不会重复执行、释放结果或窃取 owner。已加载 LoD 不重新执行；不要把“重复命令幂等”误写成“可以无限重发物理键”。

## 5. 输入与新鲜度：必须正面处理的边界

主机输入 gate 按顺序检查：受管安装 byte-for-byte；目标 process/window；从当前私有内存重读的完整认证 STATE；client/build/protocol 与 binding/actor；当前协议要求的本次观察关联及固定 freshness 窗口；not loading、not combat、focus clear、ready；持久 intent 与 exact 文件 publication；输入调用前再做目标窗口身份校验。任一步未知、失败或过期都不投递业务按键。指令命中、根/路径绑定和历史记录本身不授权；本次观察关联采用后续 v1/exact-COMMIT 原型的修订规则，不恢复已经被反例推翻的“双递增即 fresh”谓词。

新鲜度候选窗口：活动期间状态每 250 ms 更新一次，观察两次 revision/gameClock 真正递增，最后一次 publication 的可证明年龄不超过 750 ms；数字是原型参数，须由真实客户端时钟分辨率与性能决定。不能用“此次读取的 Instant::now()”反复刷新同一 revision 的时间。主机从第一个新 revision 建立本地 observation baseline；进程/主机暂停、时钟回退、revision 回绕/重置、lease 到期都撤销它，重新获取两个样本。

游戏每次入口重新读取本地实际 combat/focus/loading/actor 条件，再检查 COMMAND 所引用的 permit 和 game-clock expiry。secret value 在比较/转字符串前识别；无法安全判断即 unknown/拒绝。采样字段只用于主机筛选，不能代替入口检查。一个 permit 只适用于 exact command digest，不能被另一命令沿用。

**B1：两次真实样本也不能消除 read→input 的 TOCTOU。** 可能顺序为：读完 ready → 游戏暂停 → OS 投递 → 游戏恢复。本文不声称解决了“任何暂停期间绝不投递任何键”的字面要求。候选能提供的是有界新鲜观察、固定目标、入口复核，以及过期 payload 不执行业务；如果游戏时钟暂停时也停止，单用 game-clock expiry 还不够，须由 B2 证明另一过期依据。不能通过测试未命中这个竞态就记为通过。需独立审查给出可实现的严格机制，或由主代理向用户明确确认语义；设计者无权自行降低要求。

文件 publication 本身是执行暴露边界：之前遗留的物理唤醒可能消费刚发布的新文件。为此 **intent 必须在发布前持久化**，而不能等当前 PostMessage 前才记录；命令本身必须包含全部 gate 和期限。主机不声称某个 exact 键是任务接受的唯一因果，只有游戏认证的 accepted/terminal 是事实。缺少回执时不得再次投递同一不确定业务键。

Mailbox-only 没有给主机主动通知的通道，因此活动 CLI 使用总时限和总读取预算约束的轮询，100 ms 起步、上限 500 ms，空闲退出；这不是常驻 polling gate。若用户将项目“无 polling gate”理解为连活动期 Mailbox 读取也禁止，则该约束与唯一内存读取通道需显式协调，不能隐瞒。

## 6. 首次激活、观察与连接

禁用态：保留已批准的一 loader、一 binding owner、三 binding buttons；不创建采样 frame、业务 frame、事件、hook、timer 或 OnUpdate。可发布一次认证的被动启动记录，但静态记录不授予业务输入。密钥计算只发生在装载时；成本须测量。

`ObservePulse` 是不加载任何 LoD、不执行业务、不取消、不释放结果的受限只读唤醒。它只启动最长 15 秒的输入状态观察租约，使用已 opt-in 的采样 frame；每 CLI 操作最多两次，有总等待上限。租约到期且无任务后移除 OnUpdate/事件，而非只 Hide。允许由认证被动记录选定目标后发送；没有合法 Mailbox 时不能把 ObservePulse 当通用绕过。连接期间新建的 8 个 lane binding buttons 仅在 opt-in 后创建，不计入禁用 bootstrap；按键冲突及不同客户端能力必须验收。

首次完全没有 Mailbox：单独的安装激活流程，只在 exact clean managed installation、固定 PID/creation/窗口和本次 install transaction 下使用一次固定激活动作，不接受 Lua/业务参数、不消费 RUN 槽。先记录 activation intent，再发送；unknown 不自动重放。激活后必须通过正常 Mailbox 定位、认证、新鲜度及 BIND，不能继承 activation 的输入资格。

具体无 Mailbox 激活动作仍需在原型/最终客户端选择：现有固定 Esc×3 /reload 序列是历史参考，不因沿用而自动安全；无 combat/focus 读数时不能声称它已满足正常业务 gate。`addon install` 的明确授权可覆盖限定激活，但普通 live read failure 不授权将自己改称 first-install。正常 reload、升级、Mailbox 损坏、未知任务各有独立前置条件，不回退到该入口。

连接：读取被动记录 → 受限观察 → 两个 fresh STATE → 安装锁内分配 lane 并先持久 BIND intent → 发布 lane 的 slot 0 → 定向按该 lane 的 binding key 一次 → 读取绑定回显 → ready。不得为寻找当前目标依次加载其他 lane 的 bootstrap；每次只加载指定 lane 名。

## 7. 状态转换与对外事实

游戏私有任务只有 `none / running / terminal` 三个阶段。`cleanup` 是 terminal 任务的收尾事实；connection 与 slot mode 正交，但只允许下表可达组合，不用任意布尔组合制造隐含状态。

| 当前事实 | 事件/条件 | 后继 | 禁止推导 |
| --- | --- | --- | --- |
| unbound/none | 合法 BIND | bound/none | BIND 不表示业务接受 |
| bound/none | 合法 RUN、登记完成 | bound/running | OS 投递不等于该转换 |
| running | 第一次合法完成/失败/协作取消提交 | terminal、BODY retained、cleanup=pending | 第二次 callback 不覆盖第一个结果 |
| running | 主机等待超时/CLI 退出/未确认输入 | 无游戏状态推导 | 不变成 failed/cancelled |
| terminal/pending | 合法 RECEIVED，清理成功 | none；BODY 释放；清理完成结论 | 未保存结果时不得发该命令 |
| terminal/pending | 清理失败 | terminal/blocked；BODY 保留 | 不能写 cleared/settled 或清 active |
| terminal/blocked | 有限允许的幂等清理重试 | terminal/blocked 或 none | 不重新执行业务，不续任务预算 |
| bound/none | CLOSE | closed/none | closed 不证明所有旧消费者已销毁 |
| 任意 | runtime/actor 消失 | 主机保存最后已证事实 +当前未知 | 退出不是业务成功/失败/未执行 |

对外结果沿用产品 envelope/退出码合同，但区分以下字段：业务 `outcome=success|failure|cancelled|unknown`、报告 `unavailable|verified|durable`、清理 `pending|blocked|complete|runtime_destroyed|abandoned`、操作 `complete`、下一步。`runtime_destroyed` 表示旧执行资源随已证 runtime 销毁，不谎称原清理回调成功。`abandoned` 不表示游戏已取消、已确认或未执行。

主机提交阶段记录 `prepared / exposed / observed`，它们描述证据边界，不复制游戏 running/terminal 枚举。输入调用的 `not_called / partial_or_unknown / returned` 是 journal 中一次输入事实；`returned` 不能授权成功。只有暴露前且已证明无可执行文件可达才允许本地取消记为未发送。

## 8. 正常、取消与收尾时序

```text
Host journal       Shared files        Game private            Mailbox
RUN intent durable                     none
                   exact RUN publish   (may be consumed by a late wake)
input attempt                          gate + register task
                                       execute once             running
                                       commit BODY, then STATE  terminal + BODY
read complete BODY; verify; atomic persist; readback digest
RECEIVED intent durable
                   exact RECEIVED      verify task/result
input attempt                          cleanup all handles
                                       only on success: free    none + exact completion
record completion; release task reservation (not lane generation)
```

```text
CANCEL vs finish:
  both target the same private task object and terminal commit guard;
  whichever commits terminal first wins;
  cancel accepted only means cancellation requested until a cancelled result is committed;
  late finish/cancel callback returns the existing terminal fact and cannot replace it.
```

同步 Lua 的取消不能在解释器不让出时 magically 执行；业务预算与协作异步必须由现有安全执行器及明确支持方式实现。若依赖 debug hook 抢占，必须证明真实客户端 API 可用且不改保护行为；否则只给出 cooperative 语义并将未满足的公开业务等价列为缺口。不能用 host timeout 写伪 cancelled。

清理分两类：工具拥有的 frame/event/hook/timer/lease 用幂等 handle 操作，记录每个 handle 的完成位，最多三次、固定总期限，不重置 task deadline；业务提供的 opaque cleanup callback 若未声明并验证幂等，异常后不能盲调。失败保留 BODY、task identity 和恢复所需句柄，禁止下一 RUN；给出 blocked 及可执行后续（受控 runtime 替换或显式 abandon）。新增控制文件不得通过“旧 RECEIVED 已经失败”重执行业务。

ROOT 已复现旧实现 A03：第一次清理失败后 BODY 被删、CanSettle=false，第二次收妥不再调 cleanup。证据 `.tmp/rust-supervision/protocol-a03-cleanup-counterexample.json`（及同前缀 inputs/script）。这是新候选必测的负例：**清理失败不得擦除唯一正文或恢复义务；fail-once 第二次应只重试允许的清理，业务执行计数仍为一。** 该复现是反例证据，不是新协议通过。

## 9. 持久化与中断恢复

主机 journal 是 append/atomic-replace 的单一任务权威，result 为内容寻址不可变对象；每条记录有 schema、sequence、内容 hash 和前项绑定。实现可采用一种原子格式，不要求两套并存。必须先证明 Windows flush/replace/目录恢复的实际保证；进程崩溃与整机断电分开验收。不得把当前 `atomic_write` 的名字当断电保证。

恢复读取固定安装、session、binding 和 exact intent，不重新选窗口，不重置 deadline，不自动抢 owner。缺失 journal 与存在可运行文件冲突时隔离该 lane，不能猜测删除后重用。只由权威 journal 重建衍生文件；不同字节同 ID 一律 conflict。

| 中断点 | 恢复推进 |
| --- | --- |
| intent 前 | 没有授权任务；可重新准备 |
| intent 后、证明文件从未发布 | 可以完成原 publication，但须重新取得输入许可；期限不续 |
| 文件可能发布，未记 input returned | 已暴露/未知；只读观察，不能靠“OS 调用可能没发生”重发 |
| accepted/running 后 | 观察原 task；必要时一次新的 exact CANCEL，结果仍由游戏提交 |
| BODY 有而 terminal STATE 无 | 不承认完成，保留未知 |
| verified BODY、result 未 durable | 重读并保存；不发 RECEIVED |
| result durable、journal 引用未更新 | 校验内容对象及 intent，恢复引用；冲突即隔离 |
| RECEIVED 已暴露、回执缺失 | 读取原状态；不能凭正文消失写 complete |
| cleanup fail-once/fail-always | 按第 8 节有限清理路径；正文及义务保留 |
| 游戏退出/reload/actor 变化 | 保存已有 durable 结果；未取得结果仍 unknown；新 binding 不续旧任务 |
| owner 进程退出 | 释放 OS 锁不等于游戏 owner 已结束；同 session resume 可接管 driver，新的 owner 不可抢任务 |

历史观察 policy 保留业务等价：允许在**已证明新的合格 runtime** 中创建最多三次观察尝试，必须新 task/attempt identity，保留原 attempt 的 unknown，所有尝试共享原始总截止；opaque policy 不自动重跑未知操作。协议不是保证 exactly-once 跨 runtime 的系统。历史相同 key/完整参数读回不发送输入；相同 key/不同参数必须冲突。连接 closed 后新 key 不得误返回上一项历史结果。

取消本地尚未暴露意图可终止；暴露后 abandon 只是主机明确放弃继续推进，保留证据与隔离占用，不假称游戏取消。只有经证明旧消费者不可达后才可回收 lane。

## 10. 500 LoD：固定 lane、收尾预算与换代

候选分配：8 × 62 = 496，另外 4 个 LoD 在本协议版本保持 inert、不分配、不加载。全部 500 个仍由安装清单管理；余数不是隐藏的第九实例或无限恢复池。

每条 lane：`0=BIND`；`1..58=ordinary`；`59=reserved finalization`；`60=reserved CLOSE/rotation`；`61=reserved reconcile`。ordinary 承载 RUN/RECEIVED/CANCEL，不能由命令类型各自计算相邻槽。每条 command 记录 exact slot；游戏 loaded bitmap、journal reservation 和认证 consumed receipt 对照推进，不再要求 RECEIVED=RUN+1。

RUN admission 要求普通区至少四个未消费槽，以及 reserved finalization/close 仍可用；普通成功任务消费两个，所以无故障可完成 28 个任务后换代（剩余两普通槽保留用于已接受任务的控制，不接新任务）。取消/拒绝/多次清理会更早触发换代。这个上限必须由可执行模型推出并边界测试，不能沿用旧 47 次结果作为新协议容量证明。

普通唤醒只尝试私有 next slot。空、损坏、foreign 或不合当前私有状态的 payload 最多消费一个槽，随后 `mode=fused`；不继续扫后面的槽。reserved reconcile 只恢复精确 cursor/已消费事实，不执行新任务，不清理或释放结果；每 runtime 最多一次。它解决单次可观察损耗，不承诺无限重复故障仍有收尾能力。

reserved finalization 只允许当前 terminal 的 exact RECEIVED，reserved CLOSE 只允许 task=none 的 CLOSE。任何 reserve 已损坏/消费且无剩余合法路径时，状态为 blocked/exhausted，保留结果和义务；新任务拒绝。不能因“为了恢复”加载 foreign lane 或让旧 runtime 使用下一 generation 文件。

同安装两个进程由相同 allocation ledger 分配不同 lane；不同安装各自 500。host 持有短 OS 安装锁完成 claim/发布，锁内不等待游戏；每 lane 有一个有效 driver lease（OS handle identity，不以 wall-clock timeout 自动夺权）。task 进行期间其他 lane 可以推进，但任何写入必须限定 exact 文件集合；测试逐字节证明 foreign lane 不变。

换代顺序：停止新 RUN → 当前结果 durable → 清理完成 → CLOSE 私有确认 → 记录 retirement intent → 通过受控 reload/已证进程退出使旧 Lua 消费者不可达 → 取得新 incarnation 与新 binding → generation 单调增加后重用同 lane。重新看到一个不同公开 runtime 字段不构成销毁证明。OS process creation 对应的进程确已退出可证明其旧 Lua 不再执行，但不能证明未知任务结果。相同进程 reload 的销毁边界属于 B2/最终实机假设。

ledger 热数据只保留最多 8 条 active/quarantined lane、每条当前 generation 和必要未决引用。已结束的完整证据进入有配额的历史文件；不在热表累计无限 retired tombstones。单调 generation 及旧消费者不可达证明共同阻止旧 owner 复活，不能仅靠删除 tombstone。9 个实例请求应返回明确 capacity/backpressure，而不是借槽或杀已有任务；隔离 lane 也计入 8 个上限。

## 11. 明确资源边界

以下是原型硬上限候选；必须与冻结 Go 业务合同核对，不得以缩小合法工作负载满足性能。若旧业务允许更大，则更改数值并重新跑相同故障边界，不能直接删业务。

| 资源 | 候选上限及耗尽行为 |
| --- | --- |
| 活跃实例/lane | 每安装 8，包括隔离；第 9 个背压 |
| 活跃任务/结果 | 每实例 1；未收妥前不接下一项 |
| code | 256 KiB UTF-8 source；拒绝 Lua bytecode；超界在发布前拒绝 |
| result BODY | 512 KiB；超限发布明确 oversize 结果/截断元数据，不静默截断 |
| STATE | 8 KiB，规范字段和枚举；未知字段拒绝 |
| 日志预览 | 32 KiB，显式 total/truncated；完整业务结果另算 |
| 历史 | 最近 100 条，每条预览 32 KiB；未决/唯一结果不按年龄淘汰 |
| Mailbox 读取 | 代码区特征匹配/RIP/根绑定及 Lua 路径读取有调用、候选、字节、重定位次数和总截止上限；正文按已验证终态准入；总预算含 OS、定位和 anchor/root/path 全部重读；具体起点见总方案 §7.2 |
| task 时间 | 1..120 s，由首次 intent/私有接受规则固定；resume 不重置；跨 attempt 共享总期限 |
| 单 CLI 等待 | 独立有界 wait deadline，到期 pending/unknown，不改业务事实 |
| cleanup | 最多 3 次幂等步骤重试，固定总期限；opaque callback 不隐式重跑 |
| observe lease | 每次最长 15 s、每操作最多 2 次；任务活跃期另有固定 task lease；无无限 keepalive |
| malformed/empty 消耗 | 普通加载一次后熔断；reserved reconcile 最多一次 |
| command 去重 | 当前 task 相关命令 +最后结论 +lane已消费摘要，随本 runtime 有界；长期历史由主机负责 |
| 主机磁盘 | 安装/会话/证据总字节配额；写入前 admission；不足返回明确环境错误；不自动删未知/未决/唯一证据 |

u64 在 Lua 5.1 number 不能精确表达全域，因此 wire 整数用固定宽度十六进制字符串或经过审计的 hi/lo 运算，禁止 silently round 到 double。revision/generation 临近表示上限时不回绕，停止新任务并按受控换代处理。

## 12. 旧版本升级与现有实现处置

安装更新必须检测所有旧 schema 未决事务；无法安全迁移的旧任务继续由已封存旧实现只读恢复/收尾，或由用户明确 abandon，不能复制几个字段当新任务。新协议 addon、Rust CLI、500 个 LoD 和 Skill 组成同一受管 generation；中断恢复保留旧清单和事务，不能混装后继续输入。

需要替换但目前尚未修改的旧实现包括：`windows_channel.rs` 的 `Signal/Tracker/CaptureStream`、`optical()` 与 `wait_event()`；`NativeInputState.lua` 通过 `InputSignal` 创建/更新采样；TOC 的 StartupBeacon/InputSignal；NativeAdapter 的相关激活/隐藏路径；NativeProtocol 的失败清理即删 BODY；`routes.rs` 相邻 settle 和 retired 无限累计。新方案最终验收将扫描依赖与产物确认无 WGC/optical 残余或回退入口。

现有 Rust 单元、47 次 fixture、历史 Go 实机及 runtime `.text` 归档都不证明新协议通过。当前路线须实际证明 Go v3.1.0 对应的运行时特征/RIP/根绑定和 Lua 布局/路径读取、认证及跨实例隔离；磁盘静态定稿不作为门槛。旧私有堆扫描停止作为产品路线；所有历史研究和失败保留原范围。

## 13. 不变量、故障矩阵与证据准入

| 不变量 | Rust/模型必须可执行 | 真实 Lua 5.1 必须执行 | 最终游戏专属 |
| --- | --- | --- | --- |
| I01 无有效认证/新鲜 STATE 不授权输入 | 完整伪造、旧重放、停止 revision、时钟回退 | 私有签名边界、permit过期 | 时钟暂停/加载行为，B1/B2 |
| I02 公开表不能授予执行/取消/释放 | 独立篡改器改所有授权字段 | 任意公开表修改后 RUN/CANCEL/RECEIVED | 真实 protected/secret API |
| I03 同任务内容最多接受一次 | 重复、乱序、same-ID-different-digest | 重复回调、完整参数变更 | LoD 一次加载行为 |
| I04 暴露前 intent durable | publication各切点崩溃、遗留唤醒 | publish后旧键消费新文件仍匹配intent | Windows真实键延迟 |
| I05 无 durable result 不释放正文 | 保存/读回/损坏/冲突各切点 | BODY→STATE及RECEIVED每步异常 | 最终候选Windows持久化 |
| I06 清理失败保留义务且可有限恢复 | A03轨迹贯穿真实推进器 | fail-once、fail-always、opaque failure | 真实frame/event释放 |
| I07 未知不推导成功失败、不盲重放 | input前后相同记录、timeout/exit/missing | 恢复不重新执行业务 | OS partial与窗口变化 |
| I08 task/result/owner/runtime隔离 | 相同PID新creation、相同PRNG/SV、ABA | 双实例故意碰撞、旧owner与旧BIND | 同安装同build双实例及混合build |
| I09 槽位可收尾且foreign字节不变 | 两实例最后4槽、reserve损坏、9实例 | fuse/reconcile/非相邻RECEIVED | 8实例上限与实际热键 |
| I10 资源与历史可持续有界 | 100/101历史、1024+换代、字节配额 | frame/hooks/timers计数、deadline不重置 | CPU/内存前后同业务测量 |
| I11 取消与完成仅一个终态 | 两事件全部交错 | 同步/异步、晚回调 | 实际执行器抢占边界 |
| I12 activation不成为旁路 | firstinstall/upgrade/readfail/pending分流 | activation无业务LoD/释放动作 | 无Mailbox真实激活 |

所有故障类至少注入：owner/claim 持久化、文件临时写与原子替换、input intent/每次按下释放/OS返回、LoadAddOn/验证/接受、执行与终态提交、结果文件 flush/replace/journal、RECEIVED/清理/正文释放/回执、CLOSE/reload/新binding/回收、历史淘汰和磁盘满。每个 cut 保留首次失败、修复和相同事件轨迹复测；不能让模型把 authentication/atomicity/runtime uniqueness 作为无条件 true 从而循环证明。

证据分层：M=状态模型；L=真实 Lua 5.1 + Rust 推进器同轨迹；W=Windows 真实文件/进程/输入原型；G=同一最终候选游戏验收。所有行当前均 **not_run**；旧反例复现仅是设计输入。G 覆盖 Retail、Classic、Titan、Forever Asia Beta、Forever CN Beta，每类单实例/同安装双实例、跨 build 并行、多 build 各多实例混合、产品上限压力。缺失客户端保持未验收，不能缩减范围。

## 14. 定稿前阻断项与审查入口

| ID | 尚未证明的事项 | 下一步及对应审计 |
| --- | --- | --- |
| B1 | 任意暂停点都不投递业务键的字面要求与异步OS输入间隙 | 审查可实现机制/明确语义；A02，不由设计者自行放宽 |
| B2 | 不可重放的startup/runtime身份、相同进程reload销毁证明、游戏时钟暂停/回退 | 故意碰撞模型、Lua原语核对、最终各客户端；A07/A09 |
| B3 | 仅公开表可写威胁下私有MAC密钥边界、纯Lua算法与采样CPU成本 | 真实Lua独立伪造器/公开签名oracle检查/固定负载benchmark；A01/A10 |
| B4 | 同步任务超时/取消与非幂等cleanup的业务等价 | 冻结Go能力逐项对照，实际执行器原型；A03/A12 |
| B5 | 500槽选8lane布局、直接lane绑定与末尾预留的完整活性 | 枚举全部合法/损坏消息、旧键、换代切点；A04/A07/A08 |
| B6 | 首次无Mailbox激活动作的授权、固定目标和安全边界 | 独立限定入口原型与最终实机；A11 |
| B7 | schema规范字节、MAC覆盖、持久化保证、完整读取预算 | 逐字段wire schema与真实Rust/Lua互操作/故障矩阵；A05/A06/A10 |

独立审查者应先审一页核心是否已有更简单且满足同样反例的结构，再审 B1..B7。每个问题保留反例、决定、复核与受测文档 hash。只有正确性、隔离、恢复、有界资源问题关闭，且模型及真实 Lua 联合关键矩阵通过，才能将本文改为最终协议；最终游戏专属假设继续单列未验收。协议实质变化重开对应审查和回归。
