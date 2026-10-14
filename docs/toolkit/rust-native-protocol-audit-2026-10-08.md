> Historical reference for the Rust migration (2026-10-14): current decisions, baseline and acceptance status are in [handover.md](../../handover.md). This document preserves its original scope and evidence; conflicting requirements, protocol designs, performance targets and release instructions are superseded. Historical test results are not current-candidate passes.

# Native 协议独立审计（2026-10-08）

状态：**完整协议未通过；已完成冻结 v0 与 v1/P0 独立审查，P0 仅固定可信 binding 内的 nonce 抗历史观察重放有限通过。** 旧 A03 已在真实 Lua 5.1 复现，v0 新鲜度与 v1 APPLY 因果缺口另有模型反例；均不表示新协议通过。本文不是协议设计，也不批准现有 EXECUTE/SETTLE 为最终方案。

2026-10-12 执行状态核对：P3 lifecycle revision 已实际运行，不能继续记为 prepared/not_run。封存结果 `.tmp/rust-supervision/native-revised-lifecycle-a3d04951fa2844c716b653e8e5010cac/result.json` 明确 `failed_maximum_code_full_lifecycle`、`finalProtocolPassed=false`：原 scoped positive matrix 通过，两/四槽 S02/S03/S04 修订断言运行，完整 MAX_CODE 两边均 `observation_expired`。外层 receipt `386ba33b-439e-4496-a0ea-b28c82199f83` 的 Cargo exit101 与 VCTIP 未获 reviewed auxiliary 导致的 forced retirement 是两项分别保留的失败，不能相互覆盖。当前继续缺 P1、clock/B1、500 allocator、OS carrier、恢复活性及最终独立审计/实机证据，详见 [原型当前状态](../../protocol/native-lifecycle-revision/README.md)。

## 审查范围与证据身份

2026-10-12 最新定位修订：Mailbox 参考 Go `v3.1.0`，运行时指令特征匹配/RIP 解析动态 Lua 根，再沿 state → globals → Mailbox 读取；停止私有堆扫描。身份、认证、本次观察、exact COMMIT、持久结果及统一预算继续适用。见 [总方案 §7](rust-refactor-plan-2026-10-08.md#7-mailbox-运行时指令定位与-lua-路径读取)；旧原型证据只证明原受测范围，不代表新入口通过。

- 审查者：独立新上下文 GPT-6 Astra / high；未参与被审查协议设计。
- 要求：本工作树 `AGENTS.md`、`rust-refactor-requirements-2026-10-08.md` 第二、三、四、八节。
- 源码基准：工作树 HEAD `9d3d3598b8bac7efabf0b937df30508075eaa841` 加审查时已有未提交文件；HEAD 不能独自标识这些文件。交付测试报告前须封存实际文件哈希。
- 被审查材料：历史 `live-mailbox-protocol-2026-10-01.md`；当前 `NativeProtocol.lua`、`NativeRuntime.lua`、`NativeAdapter.lua`、`NativeInputState.lua`；Rust `mailbox.rs`、`transport.rs`、`windows_channel.rs`、`lifecycle.rs`、`routes.rs`、`slots.rs`、`replacement.rs`、`journal.rs`。
- 以下“静态确认”仅指可从代码路径推出的行为；没有把代码阅读记作模型检查、真实 Lua 测试、Windows 测试或游戏验收通过。所有实机项目仍为 `not_run`。
- 修改权限仅限本文；未修改生产代码、Cargo、共享 fixture；未扫描/删除构建缓存，未输入游戏、发布或提交。

## 初始结论

Mailbox 单通道不能仅通过删除光学调用实现。新协议首先要证明读取真实性和新鲜度的边界，再证明输入、业务接受、结果保存与清理之间的状态转换。当前私有 Lua 请求表能抵挡一部分公开表篡改，但不能使 Rust 自动获得真实输入条件、真实回执或真实结果。公开校验和、公开 runtime、公开 challenge 都不是独立的真实性来源。

对任意时刻的进程暂停，存在不可消除的观察/动作间隙：主机读完有效快照后，客户端可在下一条主机输入前暂停。协议应精确界定能证明的有限新鲜窗口、游戏回调再次核验和迟到输入语义；若承诺暂停时绝不会投递任何键，必须提供额外可实现的同步机制，不能用“两次读取”代替证明。此项不能由审查者自行放宽用户要求。

## 问题与反例登记

每项含可重放事件序列、影响、所需证据与复核状态。`open` 表示尚未关闭；“候选需证明”不表示已确认发生于所有真实客户端。

### A01 — 公开 Mailbox 伪造可误导主机输入与确认（阻断）

反例：取得公开 INPUT 的全部字段 → 在同一当前公开路径发布 `inputBlocked=false`、递增 sequence/sampleMillis，按公开格式重算校验和 → 主机读取通过 → 输入。另一路：业务未完成时伪造匹配 owner/epoch/request 的 reported HEAD/BODY → 主机持久化伪造结果；结果已持久时伪造 settled + 两个 cleanup 标志为真 → 主机释放磁盘 reservation。

依据：`mailbox.rs` 的 `decode`/`Reader::lookup` 验证格式、身份、长度和 Adler32；`transport.rs::receipt/observe` 据公开状态推进；`routes.rs::settle_request` 依赖调用者验证回执。私有 Lua 会拒绝不匹配的实际业务或收妥，但主机已被错误授权或已报告错误事实。

影响：违反公开篡改不得错误授予输入资格；可产生假结果、假清理。需要明确攻击能力和真实性来源。若秘密仍在公开 Mailbox 中、认证只签部分字段、初始化信任来自同一可伪造路径，反例仍成立。

所需证明：独立篡改器修改 INPUT/HEAD/BODY/owner/runtime/路径及完整重编码；Rust 推进器所有依赖事实不得推进/投递输入。真实性机制如采用密钥，须验证来源、私有保存、跨 runtime 绑定、初始化与升级，以及签名覆盖/重放。外部任意内存写或任意 Lua 执行的排除范围必须明确，不能排除用户明确要求的公开 Mailbox 篡改。

复核：`open`；当前缺少主机真实性机制，尚无新草案。

### A02 — 重读同一记录被当作新鲜；暂停与键入有 TOCTOU（阻断）

反例一：发布 ready 记录后暂停游戏；循环重读同一记录，`WindowsChannel::snapshot` 每次令 `observed=(snapshot,Instant::now())`。删除 optical 后，1500 ms 检查可反复通过而记录从未推进。反例二：两个真实递增样本通过后暂停游戏，主机随即投递键；后续恢复时该键可能迟到。

依据：`windows_channel.rs:371`、发送 guard 的 elapsed 检查；当前 `optical()/wait_event()` 仍依赖 WGC。候选不能继承被删除通道提供的条件。

影响：输入新鲜度承诺不成立；迟到键可能消费错误下一槽。

所需证明：可控单调时钟和 publication 时钟；相同字符串/地址重读不延长期限；挂起发生在每个 read/guard/input cut 的故障注入；计数回绕、时钟回退、host resume 不重置期限。Windows 输入队列延迟与游戏再次核验分开验证。

复核：`open`；暂停间隙的严格语义须由设计者明确，不能提前标记通过。

### A03 — 收妥时清理失败后没有重试路径（阻断）

反例：执行完成 → 主机持久化结果 → `a.cleanup` 返回 false/抛错 → Lua 仍删除 BODY、清 `active`、写 settled(displayCleared=false) → 重试 settle 只返回旧 false，不再调用 cleanup；通过正常适配器甚至因 `CanSettle=false` 无法加载控制槽 → Rust 停在 Settled(cleanup_complete=false)，既不能关闭也不能开始下一项。

依据：`NativeProtocol.lua:161-170`；`NativeRuntime.lua::load`、`lifecycle.rs::advance` 对 Settled 仅 Observe。

影响：可用结果仍存在但清理义务无法自动完成；资源释放失败也有同型问题。

所需证明：真实 Lua 清理适配器 fail-once/fail-always；Rust/Lua 联合执行恢复；成功重试不重复业务且不丢唯一结果；永久失败有明确有限终态/人工后续，不谎称 cleanup complete。

复核：`open`，静态确认；随后 ROOT 的真实 Lua 5.1 反例已经复现，证据见末尾“实际反例证据”。新协议修复尚未实现、未验证。

### A04 — Lua 恢复槽路径与 Rust 路由结算不一致（阻断）

反例：execute 槽 E 接受 → 额外唤醒消耗空槽 E+1 并熔断 → bootstrap recover → settle 槽 E+2 被 Lua 接受，nextSlot=E+3 → Rust `settle_request` 强制 settle=execute+1，报 `live.route_settle_cursor`，占用永久保留。

依据：`NativeProtocol.lua` 的 cursor 前移/恢复；`slots.rs::late_wakes_fuse_once_and_recover_settle` 表达这种局部恢复；`routes.rs:215-216` 仅承认相邻结算。孤立单元测试不证明生产层可达。

影响：正常与恢复未复用一致的路径；预留恢复槽并不能保证实际收尾。

所需证明：真实 Lua + Rust 持久路由同一次事件轨迹，覆盖无槽损耗/一个损耗/控制损耗/恢复预算耗尽；对每步校验 cursor、reservation 与所有实例槽内容。

复核：`open`，静态确认；新协议可使用完全不同结构，但须关闭同类反例。

### A05 — 输入 intent 后未发送与发送不确定之间的活性（阻断验收项）

反例：持久化 InputExecute intent → 在 OS 调用前崩溃 → 恢复后只观察，游戏永远无回执。与“OS 已投递、游戏暂停”在主机记录上不可区分。普通超时或缺失记录不能把任一路径证明为未发送。

依据：`lifecycle.rs::advance` 对 pending 输入只 Observe 是安全的，但不足以证明恢复活性；当前取消仅允许尚未发布的 Prepared。

影响：禁止盲重放与“自动恢复总能完成”不能同时无条件成立。必须保留未知，给出有限等待、显式弃置/替换及结果丢失语义；取消操作也须定义接受/完成竞争。

所需证明：每个 pre-input/post-input 崩溃 cut 使用相同恢复推进器；未知不变成 false/success；显式终止不声称游戏取消；cancel-vs-complete、迟到执行、close/reopen、owner 变更的线性化规则及资源处理。

复核：`open`；当前安全观察机制可保留为参考，不认可其已满足完整取消和活性。

### A06 — 结果提交、保存与释放的跨进程顺序（阻断验收项）

反例族：BODY 已写/HEAD 未写时 Lua 中断；HEAD 可见/主机未写结果时退出；结果文件写完/journal 未写时 CLI 中断；SETTLE 文件发布后结果文件损坏；释放 BODY 后 settled 回执未发布时 runtime 消失；旧结果路径同 ID 不同正文。

依据：`NativeProtocol.lua::finish` 先 BODY 再 reported；`journal.rs::persist_result/verify_result` 有内容冲突检查；`lifecycle.rs` 先保存再发布 settle，是有用机制但不是全切点证明。

影响：需要分别表达游戏完成、结果取得、主机持久、资源释放、正文释放、历史保留。任何阶段缺证均不可推导业务失败或收妥成功。

所需证明：封存 Windows 原子替换/刷新语义与崩溃模型（进程退出和断电不是同一保证）；每个切点恢复；读回 exact result 后才允许收妥；结果丢失后不得继续释放；重复正文相同幂等、不同冲突；清理失败不擦除恢复所需状态。

复核：`open`，待故障矩阵；未断言现有 atomic_write 已有断电缺陷。

### A07 — 共享安装容量回收及迟到 LoD（阻断）

反例：A 保留尚未处理的输入，B/host 将 A lane 认作可回收并覆写 → A 恢复后 LoadAddOn 读到新 payload；或 A 的 bootstrap 探索已加载 B 的槽，B 后续归属变化后同 runtime 不能再次执行此 LoD。仅“文件已空”“记录过期”“PID 不存在”“读到另一个 runtime”不充分说明所有旧消费者不可达。

影响：槽被错误消费、结果串实例、无可用收尾槽。当前 20+8×60=500 是一个候选，不是用户指定的最终划分。最大支持实例数、每实例预留与换代均需证明。

所需证明：至少两同 build 同安装实例和混合 build 模型；九个实例对八 lane 边界（若继续选八）；每实例最后四槽；旧窗口/旧 PID generation/旧 runtime 迟到键；换代在分配、发布、关闭、reload、新样本、回收每个 cut 崩溃；所有 foreign lane 字节不变。真实 LoD 一次加载语义最终在五类客户端验收。

复核：`open`，候选需证明；未声称真实新 runtime 已销毁旧 Lua 状态这一前提本身错误，问题是其证明来源、共享消费者范围及回收切点。

### A08 — 历史上限变为不可恢复的永久饱和（阻断）

反例：重复 runtime 替换累计 retired 项；`routes.rs::open` 在条目数 >1024 时拒绝，而 retirement 插入未见预先上限/安全淘汰路径。第 1025 次写出后下一次连恢复也无法打开账本；JSON 512 KiB 上限可能更早触发。

影响：有“拒绝上限”不等于有可持续的有界生命周期。若改成淘汰，必须证明淘汰不允许旧 owner/runtime 复活。

所需证明：按条目与字节双边界做上限前/等于/超过测试；可恢复 backpressure；安全淘汰或有界代际摘要的 no-resurrection 模型；日志、结果文件、history、取消记录、重试总量也有边界与保留规则。

复核：`open`，静态确认当前 retired 表缺少闭环。

### A09 — runtime 身份唯一性与共享 SavedVariables（高风险待证明）

反例：两个实例读取相同持久 epoch、同一时钟秒及相同 PRNG 序列，生成相同 runtime；共享安装/account 的较旧 SavedVariables 写回覆盖新 epoch。进程身份可避免部分 host 混淆，但 payload 若仅按相同 runtime/actor/build 识别目标会丢失一层隔离。

依据：`NativeAdapter.lua:58-65` 使用持久 memoryEpoch + floor(clock) + math.random；未在该构造中证明跨实例熵或全局原子计数。

所需证明：身份层分别绑定 exe/build、安装 file identity/generation、PID+creation、HWND generation、角色、runtime、owner、会话、任务；以相同 PRNG/时钟/SV 双实例 fixture 故意碰撞；碰撞时拒绝而非串路。不能把概率独立性当作已测事实。

复核：`open`，条件性反例；实际 WoW PRNG 初始化/共享 SV 写回行为另列实机项目。

### A10 — 撕裂、路径 ABA、布局及完整内容绑定（阻断验收项）

反例族：table/node 在各读取 cut 更换再恢复；同地址字符串回收重用；BODY 在 guard 间改变为校验和碰撞内容；错误 build Lua layout 恰有可解析 tags；HEAD/BODY 来自不同请求；同 seq/digest/ticket 但 code/budget/expiry 不同。

依据：`mailbox.rs` 有 512 次/1 MiB budget 和路径 guard，保护方向正确，但 Adler32 只检测意外错误。`NativeProtocol.lua` 的 prior 去重只比较部分 envelope（清理后不再保留 code）；全消息内容承诺尚未证明。

所需证明：独立 Memory 实现逐 read 注入 mutation/short read/layout mismatch/secret bits/collision 链/整数溢出；Lua immutable-string 前提与 live GC 读取错误边界；所有业务相关字段绑定同一不可变请求身份；same-ID-different-content 一致拒绝且不消耗无限资源。

复核：`open`；不要求通过“扫描更多内存”修复，静态定位成功不得授予输入资格。

### A11 — 首次激活不能成为正常校验旁路（阻断验收项）

反例：无 Mailbox 或旧版 addon 时正常命令自动改称 activation，执行 Esc/Enter/reload；已有未知业务时走 activation 覆盖 runtime；过期 activation record 被反复用于发键。

依据：当前 activation_only 依赖被动 Mailbox，因此不覆盖完全没有 Mailbox 的首次安装情形；协议必须单独定义这一边界，不能从现有函数名推断满足需求。

所需证明：首次安装状态、旧版升级状态、Mailbox 暂缺、读取失败、reload 中和已有 pending 各自分流；固定受限动作可证明不加载业务槽；一次尝试持久化、不重放未知输入；支持无 Mailbox 的方式必须有单独授权与受管目标证据。

复核：`open`，新草案待审。

### A12 — 预算、禁用态与业务等价（阻断验收项）

反例族：CLI 重启重设截止；Lua 异步任务在宿主恢复后重获预算；无终止的同步 Lua 阻止 sampler/cancel；读取失败时无间隔忙循环；每次观察新建事件/计时器；停止仅 Hide 未解除回调；历史/日志持续增长。

影响：必须定义业务预算、命令等待、清理预算和历史 retention 各自边界。保留“未知”不能无限占用活动资源；减少往返也不能删掉取消、异步或大结果能力。

所需证明：真实 Lua 5.1 执行器超时、异步完成、重复回调、cleanup 失败；disabled/enabled/reload/upgrade hooks/events/timers 数量；持久 deadline 不续期；资源计数按实例上限压力；Source 等业务不属于本审计通过范围。

复核：`open`。

## 必须覆盖的故障切点矩阵

| 切点 | 注入 | 恢复不变量 |
| --- | --- | --- |
| owner/route intent 前后 | 断进程、记录缺失、同安装第二 host | 至多一个有效 driver；不盗取未决 owner |
| payload 临时写/替换/manifest 完成 | 短写、旧字节、新字节、日志缺失 | 输入仅在 exact publication 可证明时发生 |
| 输入 intent / 每个按下与释放 / 返回 | 0、partial、unknown、完整返回后断进程 | 不重放不确定业务；释放修复不触发新业务 |
| Lua LoadAddOn/校验/私有注册 | 空槽、foreign、迟到、同 ID 异内容 | 业务前私有接受；有界消耗与可收尾 |
| 业务开始/异步等待/取消/完成 | 取消与完成同时、重复 callback、超时 | 业务事实与主机等待超时分离 |
| BODY/HEAD 发布 | 每步异常、reload/角色变化 | 只承认同一任务完整提交；未知保留 |
| 结果文件刷新/重命名/journal | 崩溃、损坏、同名冲突 | 无持久结果不授予释放 |
| 收妥发布/输入/清理/BODY 释放/终态 | 每步崩溃、清理 fail-once | 重复控制不重复业务；清理义务可继续 |
| close/新 owner/reload/换代 | 旧 owner 请求、旧键、旧回执 | 新会话不能继承未证实成功 |
| 历史淘汰/容量上限 | 最后一项、满额、溢出、恢复 | 不淘汰未决证据；不复活旧消费者 |

## 自动化与实机证据分层

1. 状态模型：枚举关键 cut、重复/乱序、至少两实例及一个 foreign owner；断言 no-false-success、no-blind-replay、no-foreign-slot、no-release-before-durable、bounded-resource、no-resurrection。模型动作必须映射生产边界，不能把待证明的认证/原子性直接作为 true 输入。
2. Rust 推进器 + 真实 Lua 5.1：同一事件轨迹穿过实际序列化、验证、持久化与私有状态；独立篡改方不调用被测实现的“正确消息构造器”掩盖字段缺口。
3. Windows 离线原型：真实持久化、进程 identity、窗口重建/重用、输入 partial/release 及过程暂停；自建窗口证据不得称为游戏证据。
4. 最终五类游戏客户端：真实布局读取、新鲜度时钟、受限激活、protected/secret/combat/focus、LoD/reload/relogin、多实例混合及实例上限。自动审计未闭环前不得运行最终游戏验收。

## 新草案复核记录

首批登记时新草案尚未交付。后续冻结 v0 的独立复核见下节；以上均未由新协议自动验证关闭。文档修改、模型通过、真实 Lua 通过、Windows 原型通过和实机通过分别登记，不互相替代。协议有实质变更时，重新开放受影响问题与回归项。

## 简洁性审计要求（用户后续强调“很简洁，健壮”）

简洁性也是正式审计对象。设计者应为每种消息、状态、持久记录与后台活动指出一个不能由现有机制承担的职责，以及删除它会产生的最短反例。审查者不会因为名称不同而接受同一事实的重复保存，也不会为了少几个状态把“未发送”和“发送未知”合并成可重试。

| 候选复杂度 | 必須回答的问题 | 删除或合并前的证明 |
| --- | --- | --- |
| identity/input/receipt 多份重复字段 | 哪些必须独立发布，哪些是同一不可变状态的视图？ | 原子快照、身份绑定和结果读取不被削弱 |
| session journal、安装 claims、route book、slot manifest | 哪份是每个事实的唯一权威，跨文件更新如何恢复？ | 崩溃切点后可由明确权威重建，不猜测另一个副本 |
| 正常推进、recover、reload fallback、activation 特判 | 是否存在多个独立输入/重试引擎？ | 一个推进规则处理正常和恢复；特殊入口不能绕过未知与授权 |
| execute/settle/close/后续任务可能合并 | 分开消息实际保护什么？ | 去重、已保存结果、关闭和下一任务独立结果均可表达 |
| fused/controlBlocked/recoveryUsed 等并行标志 | 可达组合是否受约束，是否有永远不能收尾的组合？ | 显式可达状态图，故障后有界且无复活/误释放 |
| 常驻采样、逐事件刷新、额外探测循环 | 每个周期任务为何必要，是否重复提供同一活性事实？ | Mailbox 新鲜度和输入再次校验仍满足；禁用态零工作 |

这些是审查问题，不是指定最终结构或预先批准合并。所有简化提案都需在相同反例和核心不变量下验证；增加第二套恢复机制来掩盖第一套缺陷不算关闭问题。

## 冻结 v0 独立审查结论

受审文件：`rust-native-protocol-proposal-2026-10-08.md`，SHA-256 `9e4032b2700f8a562f481519ccffff3e88c66a66b7a96d16b9ba52ec4eb26892`。审查者完整读取并核验此 hash。以下结论仅绑定该版本，不自动适用于设计者后续修改。

**决策：保持设计候选；允许最小反例/机制原型，禁止据此大范围替换。** 单一不可变状态、私有唯一当前任务、至多一份正文、一个恢复推进器、以结果持久化为收妥前置，均是有理由保留的方向。但 v0 还不是完整可实现的协议：认证新鲜度、permit 绑定、槽位控制入口、换代原子顺序、有限历史去重等关键规则没有闭合。作者把事项列为 B1–B7 是诚实登记，不是关闭证据。

### V01 — 正确 MAC 加两次递增仍不能证明当前状态（阻断；关联 A01/A02）

最短轨迹：

1. 同一 binding 曾正常发布签名正确的 ready STATE `S41=(revision 41, gameClock 10.25s)` 与 `S42=(42,10.50s)`；公开篡改者保存字节，不获得签名密钥。
2. 主机开启新的观察流程（如 CLI 重启/新 observe lease），没有独立可信的当前游戏时钟映射；即便 journal 保存了 revision 40 的高水位，下面两个旧样本仍都超过它。
3. 游戏当前 input blocked。普通公开表篡改者在游戏仍能执行 addon 回调时，依次以 250 ms 间隔把 `S41`、`S42` 放回公开 STATE；其余字段、MAC、binding、leaseEnd 全部保留。
4. 主机见到两次真实签名、递增时钟/序号、本地间隔小于 750 ms。v0 没有已定义规则可区分这些旧字节与刚发布的相同字节；若准入新鲜历史，便也准入此旧历史。

这不依赖签名伪造，也不依赖游戏私有执行 gate 被突破。即使游戏拒绝业务，主机已错误授予业务输入资格。签名只能证明某时刻的真实性；从首次接收时间推导 publication 年龄存在循环。单加“更多样本”“高水位”“更短 750 ms”仍可被预先保留且尚未观察的历史序列击穿。

ROOT 已执行二历史抽象模型，见证据节。**范围限制：** 模型设定整个目标暂停时仍可逐条向 reader 呈现历史。普通恶意 addon 在完全暂停的游戏中也不能运行，因此这一暂停版模型不能直接冒充真实 addon 攻击。它证明该准入谓词没有独立新鲜信息；上面“游戏运行但实际 blocked”的轨迹才是不增加任意 OS 内存写能力的直接原型目标。若要实际测试暂停期间逐条改公开表，必须先声明且论证这个额外攻击能力。

关闭条件：给出独立于攻击者延迟/重放的可验证年龄或本次观察关联，并说明初始化、断线、重启、暂停和时钟回退；真实 Lua 公开表重放与 Rust 实际 gate 必须拒绝。可比较可信时钟关系和本次不可重放观察机制，**不预设增加一条握手是唯一方案**。当前 `open`。

### V02 — B1 的严格送键承诺在现有异步原语下不可证明（阻断；关联 A02）

在“只读内存观察，再调用独立 OS 输入 API”的前提下，令两个执行历史直到最后一次内存读取都完全相同；其中一个随后暂停游戏，另一个继续运行。主机在送键前没有新信息，必然作出相同动作。若正常历史需要发送，它也会向暂停历史发送。多读一次只是把竞态移动到最后一次读取之后。这个逻辑反例不需要篡改公开表。

因此须分开审查：①主机不依据已知缺失/失败/过期/blocked 的状态发送；②游戏私有入口不在实际不合格条件下接受；③**暂停期间绝不向 OS 投递任何业务键**。前两项是不同的可验证性质，不能措辞上合并成第三项。750 ms 有界观测也不是第三项的实现。

“恢复后输入过期”同样不能仅依赖可能随暂停停止的游戏时钟；输入滞留还可能消费新 payload，必须与 V03/文件暴露规则一起验证。若严格第三项确为用户要求，当前候选机制不满足，需设计者给出原子协调原语及其权威，或由主代理明确向用户取得精确语义决定。审查者不自行把它改成概率足够低或短暂停可接受。

关闭条件：形式化授权/投递/游戏接受三个线性化点与反例处理；Windows 暂停逐 cut 原型验证实际边界。客户端时钟、键投递行为留作实机专属，不能用单次未触发竞态关闭。当前 `open`。

供主代理向用户解释的**语义候选，尚未获得用户同意，也不是已实现承诺**：主机仅在每次业务按键前已验证属于本次观察、年龄不超过明确Δ、身份和输入条件合格的认证Mailbox事实时投递；任何已观测的缺失、失败、过期、加载或阻塞立即禁止新按键。按键可以在最后检查后因不可观察的暂停留在OS队列，投递不构成接受；游戏处理时必须再次以私有当前条件、exact请求身份及未过期许可决定，旧/不匹配请求不得执行。无回执始终unknown且不盲重放。这里的Δ必须从独立新鲜机制证明，不能仍由V01的首次接收时间计算；跨暂停的过期依据也须有实际原语。这个定义能形成分别可测的主机gate和游戏acceptance性质，**明确不承诺暂停期间绝不投递OS键**。若用户不接受该差别，则当前异步原语不足，不能继续声称满足严格要求。

### V03 — permit 在命令之前发布，exact digest 绑定尚无转换（阻断；关联 A05/A10）

v0 STATE 先发布 `permitRevision=P`，此时命令 C 尚不存在；COMMAND 的 digest 又覆盖 P、操作、内容和期限。§5 却承诺“一个 permit 只适用于 exact command digest”。反例：先用 P 构造合法 C1，尚未消费时又构造同 P 的合法 C2（不同任务、操作或预算）；当前 schema 没有说明私有状态何时、根据何种事实把 P 绑定给 C1 而非 C2。

“首次验证时原子消费 P”可能是一种可审方案，但它只能证明首个消费者唯一，不证明此前主机资格已专属于 C1。若这就是目标，须准确写出该规则以及 next command、迟到旧键、拒绝、取消、清理重试对 P 的消耗；不要把公开 revision 描述为已存在的 exact-digest 授权。若与 command number 的私有高水位完全同义，独立 permit 状态可能可以删除。

关闭条件：明确单一消费线性化点和最少私有字段；Lua 轨迹 `P→C1/C2`、先错误 MAC/错误槽/拒绝后合法命令、迟到旧 P、同一命令重复全部可执行。当前 `open`。

### V04 — reserved reconcile 是未定义的消息与未实现的路由（阻断；关联 A04/A07）

§4 称操作“只有” BIND/RUN/RECEIVED/CANCEL/CLOSE；§10 又引入 reserved reconcile，可改变 fused/cursor，但没有 COMMAND 操作值、字段、前置条件和回执。它在语义上是第六种状态变更，即便称为恢复也不能从完整消息表中消失。

此外，普通唤醒只加载私有 next slot，八个新按钮被描述为 lane BIND 入口。反例：next=k，空槽后 fused；主机在 slot61 发布 reconcile，但游戏在读该文件前如何知道应加载61？terminal 时要直接选59、none时要选60也有同一问题。主机磁盘 ledger 对 Lua 不可读；模型若直接调用 `Receive(61)`，会跳过真正待证明的入口。

关闭条件：给出真实可调用 button/handler→exact slot 的确定映射，以及同一个键在不同私有 phase 的行为；从实际 Lua entry 经模拟一次性 LoD loader 执行，不直接调用 Receive 绕过路由。挑战 reconcile 是否只是已签名 slot 事实的重复确认：若游戏已经知道 cursor、主机也能认证它，额外消息究竟改变哪个必须私有授权的事实？允许删除它或合并进统一 exact 控制，但需用空槽/控制失败/收尾轨迹证明；不可仅再加一套入口。当前 `open`。

### V05 — 清理完成证明与重试命令身份不完整（阻断验收项；关联 A03/A06）

新候选规定 cleanup 失败保留 BODY/currentTask，静态上修正旧 A03 的方向。但最后 schema 的 `lastCommand.outcome` 只有 accepted/rejected，task=none 后不再有 task/result/cleanup 字段，时序却写“none + exact completion”。必须精确定义：是保留一个完成承诺，还是通过 **同一 binding + exact RECEIVED digest + 私有规定仅清理成功才能到达 none** 推导完成。后者可以足够简洁，不能据此声称一定需要新增 ACK/终态对象；但规则须处理其他控制覆盖 lastCommand、重启和缺失状态。

重试反例：第一次 RECEIVED 被 accepted，但 cleanup fail-once；重复完全相同的 COMMAND 按§3只能只读返回旧结论，不能再次 cleanup；如果新 command number 才触发重试，要明确它如何引用原 task/result、共享原 cleanup 预算，并在槽预算内准入。opaque callback 已发生副作用后抛错不得伪称幂等。

关闭条件：执行旧 A03 对应的真实 Lua 新接收端轨迹，业务=1、fail-once cleanup允许重试、唯一BODY在成功前始终存在；duplicate command 与新 retry command 的行为可分辨；回执遗漏后只读确认可达。当前 `open`，旧 A03 实际复现不等于此项通过。

### V06 — generation 更新与新绑定存在循环（阻断；关联 A07/A09）

§10 顺序为“取得新 incarnation 与新 binding → generation 单调增加后重用 lane”，但§3/4的新 binding/BIND 已包含 laneGeneration。反例：旧 generation=g，BIND(g+1) 发布必须先获授权写入被回收 lane；若先 bind(g)，随后ledger升g+1，则私有 binding 与主机账本不同；若未退休先写g+1，又违反旧消费者不可达前不得覆写。

关闭条件：只选一个完整换代事务顺序，列出 durable reservation、消费者销毁证明、新 generation 发布、BIND 接受各切点；任一崩溃后唯一权威确定，不靠“下一次看情况修正”。仅有目标 runtime 不同不够；启动身份旧 BIND 重放、相同 PRNG/SV/地址故意碰撞必须同时通过。当前 `open`。

### V07 — 固定八 lane 的安全理由不自动证明简单或可持续（高风险验收项）

500 槽只启用496、单实例也仅用62，每28正常任务需 CLOSE/reload/重新定位/观察/BIND。这是明确的固定分配取舍：它减少分配交错，却增加用户可见 reload、runtime 替换次数与未决恢复频率。新设计不能只报告“两条业务消息”而忽略每28项的换代路径。现有47次历史fixture不具比较效力。

所需决定：8的业务来源、固定lane相对更少/更宽lane或其他复用方法的必要性；**不要求采用动态分配**，其协调复杂度也可能更高。用单实例及8实例，任务27/28/29/56/57、取消与cleanup重试、一个空槽、最后reserve受损衡量总消息、总reload、等待、输入数量、峰值资源与可完成比例。正常吞吐目标必须包括换代。反复故障允许 bounded blocked，不得宣称任何故障都自动收尾。

`loadedBitmap + nextOrdinary + mode` 也需最小性证明：如果所有普通槽严格顺序消费，普通 bitmap 可由高水位推导，至多独立保存三个 reserve 位；若确有孔洞才保留完整 bitmap。每轮换代是否必须显式 CLOSE，要比较旧消费者确证销毁能否承担同一职责，同时保持业务关闭命令和未决保护。

当前 `open`；布局在没有可执行加载和换代模型前不定稿。

### V08 — 有界历史与无限期 caller-key 去重不能同时含糊承诺（阻断验收项）

反例：完成101项后，第一项已不在最近100条；再次提交第一项 key，相同或不同内容。§9承诺前者只读结果、后者冲突，§11却仅给最近100条与有配额历史。若最早key索引/结果也被删，就不能区别“从未用过”和“已忘记”；若永远保留，磁盘配额最终背压，不能称无条件持续。

关闭条件：明确去重作用域与保留期、历史删除后的行为（如明确 history unavailable/拒绝，不猜新任务）、永久任务高水位或权威索引的有界性。命令旧编号若已丢结论，可以拒绝stale而不能承诺读回不存在的历史。测试100/101、配额满、显式历史清理、关闭重开、同key变参。当前 `open`。

### V09 — 启动/观察/认证仍需真实可用原语（阻断；关联 A01/A09/A11）

私有HMAC安装密钥在限定的“仅公开表可改”模型中是合理候选，不是已证明隔离。必须核验 WoW 可用的反射/环境/全局hook能力是否能取得key或形成签名oracle；每安装生成文件怎样进入clean managed receipt，升级如何保全/轮换未决binding，也未定义。BIND启动challenge必须不可在reload后被旧文件重放，主机随机token本身不能替代它。

每安装共享key意味着该安装的各实例处于相同MAC信任域；一个有效MAC本身不证明来自选定Windows进程。Lua若不能独立取得PID/creation/EXE/layout，`processToken`等仅是主机传入并由Lua回显的绑定值，不能描述成游戏核验了Windows事实。字段分层应标出“主机本地已验证 / 私有可直接验证 / 仅在可靠启动challenge下回显”，没有事实来源的字段可以留在host ledger或降为描述，不靠增加九层身份字符串宣称隔离。跨实例旧BIND、相同启动challenge和重启旧文件必须用实际输入/时钟原语验证；MAC域分离不能替代这些证明。

ObservePulse 可作为不消费业务槽的有界观察，但其按键入口必须在冷启动时可达：静态被动状态不足以通过普通fresh gate，因此它是显式的非业务例外，须限制只建立观察租约，重复/迟到不得转为RUN。首次完全无Mailbox的具体动作未选定，不能记为实现完成。游戏时钟与OS时钟关系、activation/combat/focus/binding能力，仍是最终客户端证据。

关闭条件：密钥/签名oracle与startup replay原型；禁用/opt-in/pulse到期资源计数；不同客户端可用原语来自固定源码并最终实测。当前 `open`。

### V10 — 显式事实与派生字段应避免组合爆炸（简洁性验收项）

`ready + combat + focus + loading + reason` 允许不一致组合，如ready=true且combat=true；若这些全部进入有效schema，就产生额外安全路径。用户要求Mailbox表达角色、runtime、输入就绪、战斗、焦点和阻塞原因，因此不能直接删到只剩一个优先reason而丢失并发事实。可审的简化是固定事实枚举/位集为唯一权威，`ready`和主阻塞reason按明确优先级派生；或在wire保留字段但规范唯一合法组合，接收端拒绝不一致。

同理 `cleanup=complete` 若成功即task=none，则terminal/complete是不可达编码；`connection=closed`与task非none、normal/exhausted与next数值矛盾也须拒绝。不需要机械增加更多阶段；需要一张完整可达组合表及穷举负例。当前 `open`。

### 正常两条消息及可删结构的独立判断

在“每次独立run必须保存结果并完成清理，无后续任务也能结束”的目标下，RUN 与独立收妥信号各自必要：游戏不能从自己的内存证明主机已保存。可以不新增 accepted ACK、finish ACK、清理 ACK；由同一个认证STATE表达对应事实即可。若以后提供批量/保留会话语义，可以研究将上一任务收妥携带在下一RUN中，但最后一项仍需独立收妥，且上一清理失败必须阻止下一执行。这是可选优化，不能作为当前删除RECEIVED的依据。

以下简化候选不预先增加握手，需各自回归证明：

- 若command number消耗已覆盖permit唯一性，删掉独立permit状态/术语，只保留必要的新鲜观察关联；须通过V03。
- 若ordinary严格前进，用cursor加reserve位替代重复bitmap/next；须通过V04/V07，不能丢真实已加载孔洞。
- 若fused后的私有cursor已可被STATE完整认证，不另创reconcile事务；合并或删除必须从真实按键入口完成旧空槽反例。
- 派生ready/reason与不可达cleanup complete，不保存多个权威；仍完整表达用户要求的combat/focus/loading事实。
- 首次RUN是否可携带binding claim、换代是否可把CLOSE与明确的runtime retirement合并，可在连接命令及未决保护仍独立可用的前提下比较；不假定新消息更安全。
- journal、allocation ledger、result三类权威分别有不同职责，暂不主张为了文件少而强并；但同一事实不再进入第二套route/slot/recovery账本。绝不把未知态删成“可重试”。

## 实际反例证据（独立核对范围）

### E-A03：旧协议清理失败（真实 Lua 5.1）

ROOT执行 `.tmp/rust-supervision/protocol-a03-cleanup-counterexample.lua`；结果 `protocol-a03-cleanup-counterexample.json`；输入清单 `protocol-a03-cleanup-counterexample-inputs.json`。清单记录解释器 `C:/Program Files (x86)/Lua/5.1/lua.exe`，SHA-256 `a45f0f8376d3059a8bc79a4d6d07536cc1d9dec429852cfbc1f899ee96d6cd88`，exit=0。

脚本SHA-256 `70bf912a992f0c3d3f10b24ac1ae174c65d98c0547bce55c3c1ceec6d920f490`；结果SHA-256 `fd54a2887c0d1d3388955824ea42f0547e9c88145e9215a8697816bbaf6d5eb1`。独立审查者已读脚本/结果并逐一核验清单的 CaptureWriter、NativeWire、NativeProtocol 和脚本四项哈希均匹配；未重复执行，未声称亲自运行。

结果：execute=1；第一次cleanup=false后bodyPresent=false、CanSettle=false、cleanupCalls=1；第二次直接Receive收妥后仍cleanupCalls=1。直接Receive比生产入口更宽松，仍不能重试；实际CanSettle已为false。此证据证明旧私有协议A03，而非Windows输入链或新协议。

### E-V01：v0新鲜度二历史（抽象模型）

ROOT执行 `.tmp/rust-supervision/protocol-v0-freshness-counterexample.mjs`；结果同前缀`.json`；Node v24.18.1；脚本SHA-256 `92f9413f40e4d99621d0d68f2e0131d82433cb50c585560f60f7d9df7fb59039`，绑定本节冻结proposal hash。审查者已读完整脚本/结果；未重复执行。

模型确认：两段host观测字节与相对时刻完全相同，所有HMAC有效，攻击方仅复制旧公开字节；两条递增样本 + 750 ms候选谓词对正常和历史重放都准入。exit0代表**反例断言成立**。模型没有生产接收端、真实游戏进程或OS输入；未证明普通addon能在完全暂停进程中逐条更新公开表。新机制仍未验证。

ROOT随后保留原件并执行更贴近声明威胁的 `.tmp/rust-supervision/protocol-v0-freshness-blocked-counterexample.mjs`（exit0），结果同前缀`.json`。脚本SHA-256 `90b56cd83a3757eef01e1f3c894f674abae16ecc462dfe2ab25d3b4bf8df92f0` 已由审查者核验；绑定同一proposal hash。新模型保持目标运行，私有combat=true，publicWriterCanRun=true；重放旧ready仍被host准入，未给攻击方私钥。**V01以这个活动公开写入威胁作为原型目标**；旧暂停模型只保留其抽象不可区分范围。新模型仍不是生产exploit、真实Lua公开表攻击或游戏验收，这些均待测。

## 最小原型与主代理决策建议

优先顺序应按能否推翻核心机制安排；不先实现完整产品：

1. **P0：观测/输入模型。** 复用E-V01字节但补全binding/lease字段；两个真实Lua发布器历史，一份被篡改公开表而私有gate保持blocked。Rust候选gate读同轨迹；校验拒绝重放，不把“mac valid/fresh=true”作为模型公理。另加最后一次读取后暂停的二历史，输出可证明范围而非伪造通过。先解决V01/V02/V03，否则继续做大量消息实现没有价值。
2. **P1：私有认证与冷启动机制。** 真实Lua5.1与Rust固定向量互操作，独立篡改/反射/签名oracle测试；不止比较两个使用相同编码错误的实现。故意固定PRNG/SV/时钟/地址，重放旧BIND，要求拒绝。度量8KiB STATE每250ms的CPU/分配，不能靠弱化MAC采样达标。
3. **P2：实际入口与500槽小模型。** 模拟LoadAddOn一次性语义，从真实button handler进入，普通/三个reserve均须可达；不得直接Receive(slot)替代。两实例完整62槽、9实例准入、任务28→29换代及每个崩溃cut；同轨迹Rust ledger＋Lua私有state一致，foreign文件不变。新generation先后顺序由模型给出counterexample或明确不变量，不靠文案。
4. **P3：结果/取消/清理最小接收端。** 先移植E-A03轨迹到新原型，加入cancel/finish交错、BODY→STATE切点、RECEIVED重复/新编号重试、lastCommand覆盖与exact completion恢复。持久化真实Windows文件故障注入随后接入，同一推进器无恢复副本。
5. **最终客户端专属项。** 上述闭环后，按五类客户端核验Lua原语、时钟/暂停、加载/战斗/焦点/secret、真实按键和LoD/reload/实例上限；当前全部`not_run`。原型通过不替代候选游戏验收。

主代理现在可批准上述原型工作，不能批准协议定稿或大范围替换。若B1坚持严格“任何暂停点绝不投递OS键”，须先取得可实现机制或由用户明确决定语义；除此之外，各缺口可继续独立完善，不需要用户批准一般实现细节。设计者下一版必须带逐项处理决定和可执行证据，不能只把V01–V10加进另一张阻断表。

## v1 / P0 独立复核增量

受审文件：`rust-native-protocol-proposal-v1-2026-10-08.md`，SHA-256 `8c4712835a67738198b7efb6d77826df6e93cb90ca3a2530bec124d7af3c6ca8`，已完整读取并核验。此轮只复核新证据和 STAGE/APPLY，不重复宣称旧 open 已关闭。**B1 用户语义问题尚无回答，未批准，不以等待时间当同意。**

### P0 结论：有限通过可以保留

审查者核对了 `protocol/native-v1-prototype/gate.rs`、`observation.lua`、`p0_fixture.lua`、`apply_counterexample.lua`、实际 Rust integration wrapper，以及下列证据：

- 编译运行封存：`.tmp/rust-supervision/native-v1-rust-7cdbc26a-c4ca-401b-bc12-b23c5ed8cc7f/result.json`。
- Rust/Lua逐条输入输出与11类断言：`.tmp/rust-supervision/native-v1-p0-4dc9d3330d6762d30b65948831cf83c0/`。
- Cargo输出与受控receipt：`.tmp/rust-disk/evidence/b6a5a95a-a7ea-4c18-84b8-20e3c9d5d369/`。实际输出为1个测试通过、0失败，执行1.10s；编译0.77s。11类断言不是11个独立测试。
- 独立Node核对真实Lua签名、Lua按键回调/一次性加载器反例：`.tmp/rust-supervision/native-v1-lua-398360af-cbb3-4b86-a6e5-28d584c736da/`。

独立哈希核对：额外9项源码/runner输入的 before/after/current 均一致；封存EXE `e69eb3085dd6efe4f7ee4ad71f7ca56ba92511d83fae5ecff6f201051a728198`、PDB `7b7c90d8dd82ae09542f26219e9a51345fd5c37c33afc3b246091fb0d43bc608` 均匹配。审查者没有重跑这批Cargo或更改其源码。

**可支持的精确结论：** 给定可信固定 binding/runtime/session、不可预知且本次新生成的nonce、可信的生成前主机单调时间，以及真正私有的签名器，真实Lua已生成的旧ready记录不能冒充这次nonce的观察。当前blocked会拒绝，当前ready可在750ms预算内通过。MAC、字段、尺寸、32读次数及同一个Attempt对象的一次消费负例有实际联合断言。nonce→ready单样本11.3913ms只描述这个短消息fixture。

机制理由成立：旧记录在nonce生成前不含它；认证回应含nonce及完整command digest，因此产生时间不得早于nonce生成；读完时的年龄上界可由本次总耗时给出。这补入了v0缺失的信息，且没有假设“MAC valid/fresh=true”外部布尔值。此结论依赖nonce新颖性/签名边界和时间来源，不解决真实runtime启动绑定。

**不支持的外推：** P0没有产品Mailbox读取、OS送键、APPLY、持久journal、重启、真实WoW密钥隔离或完整500槽生命周期。`Attempt`可clone，single_use只证明同一内存对象；不能称为跨driver/崩溃的一次性许可。测试给qualify传入已记录的读完成Instant，未测试读取后耗时工作再送键的到期；产品仍须在实际输入guard检查当前时间。真实STAGE对大COMMAND进行强摘要/MAC的成本也没包含：纯Lua fallback的8KiB HMAC×10约1秒提示256KiB代码的整个nonce→验证→回答是否能在750ms完成必须实测，不能只用短STATE通过宣布正常负载可用。

本轮决定：**保留P0“同一可信binding内本次nonce抗旧观察重放”的有限通过；不将A01/A02/A09或V01的完整产品验收改为通过。** 删除独立permit和无必要的周期HMAC采样仍是合理简化方向。

### S01 — accepted 不是主机物理 APPLY 已排空的证明（仍阻断）

v1说只有认证证据证明此前所有host APPLY已消费，才能发布下一pending；该条件本身正确，但目前没有获得该事实的机制。真实accepted、apply callback计数、OS调用returned或本地“没有unknown”标志均不标识哪个物理输入产生了接受。把错误推断先写入durable journal，只会持久保存错误结论。

最短因果歧义：A已staged；主机的`APPLY_H(A)`已经排队；另一物理`APPLY_E`先执行A；Mailbox诚实发布accepted(A)。此时主机的H仍可能在队列。主机看到accepted(A)不能清除H的未确认投递义务。没有公开表伪造，也不需要私有状态被改。

**严守FIFO的进一步反例：** 不能仅把新STAGE任意调到早于旧H，冒充Windows消息顺序。审查者新增模型固定队列为 `[APPLY_E, STAGE_old, APPLY_H(A)]`，全部FIFO投递：

1. A取前一任务的RECEIVED，原结果已经durable，A的STAGE已完成，host对A也已观察合格；队列中另一次APPLY和重复STAGE早于host的H。
2. E先处理A；私有清理完成，task=none，发布真实accepted(A)。H仍排队。
3. host错误地以accepted(A)当作H已消费，满足其本地`unresolvedApply=0`条件，持久准备并发布下一RUN-B文件。
4. 队首旧STAGE读取这个新文件，pending=B；host还没有看到B的本次观察，甚至不必已经发送新的STAGE。
5. 后面的H到达。它不携带A的身份，通用APPLY执行B。

文件publication先于新STAGE，故旧STAGE能利用这一窗口；FIFO本身不关闭反例。若某实现能证明所有额外STAGE被拒绝且之后再无迟到消费者，须交付那个事实，不能用“正常只有一个driver”排除重复物理回调。

### E-S01：本轮独立执行的FIFO反例

脚本 `.tmp/rust-supervision/astra-v1-apply-accepted-not-drained.mjs`，SHA-256 `067653ee65982932c21ae76b5f562016930daa89110f9aba33d8b83e159fcef9`；结果同前缀`.json`；Node v24.18.1，exit0，绑定上述v1 proposal hash。脚本直接调用模拟button dispatcher和once-loader，队列严格shift、不重排，无子进程、Windows API或游戏输入。

实际断言：accepted(A)真实但H未消费；旧STAGE加载未来B；H执行B，hostSawBObservationBeforeExecution=false；业务B执行1次、A清理1次。保留未确认H则阻止新publication；如果“实际提交”确实携带exact command身份，A对pending B会被拒绝——后一个断言仅证明所需区分信息的作用，不证明物理载荷入口已经存在。

此证据比任意Lua调用次序更强，但仍是**FIFO回调模型**。Windows keyboard/posted消息队列拓扑、按键重复与真实WoW调度未验证；模型没有宣称该精确时序已在游戏发生。旧真实Lua `apply_counterexample.lua` 的反例继续保留，其scope也不提升为OS证据。

### STAGE/APPLY 是否能简洁闭合：需要一个真实的新事实

当前“STAGE准备＋通用无载荷APPLY执行当前pending”不能靠新增状态名闭合。所需事实至少是以下之一：**实际提交绑定exact命令且其授权只在本次gate之后暴露**，或**存在带真实因果归属的队列消费/失效证明**。仅nonce观察机制没有提供其中任一项。

| 可评估方向 | 必须新增并证明的事实 | 正常输入/往返/状态成本 | 500槽及恢复成本 |
| --- | --- | --- | --- |
| 物理提交携带exact attempt/binding/command | 游戏从这次实际输入读到完整命令身份；旧A不能变成B；部分/串接/迟到字节不组成错误授权 | 仍可保留每逻辑命令一次STAGE观察＋一次提交/结果等待；APPLY从单chord变为有界token输入及提交，增加输入字节与focus/partial/release处理。exact校验复用现有command身份，无需再创permit | 可继续每逻辑命令1个LoD，即RUN/RECEIVED共2槽；载荷入口及实际WoW API未实现、未验收 |
| 每请求不可变COMMIT/入口 | COMMIT含exact command，**只能在本次fresh gate之后发布**；旧按键只触发读取已被授权的exact文件，不能提前授权未来pending | 在一种最小安排中保留STAGE/APPLY两次按键与两次观察阶段，但APPLY实际加载有内容的COMMIT，不能执行可变current指针；增加一种有明确作用的不可变授权记录 | 若每逻辑命令多1个LoD，则每任务由2槽变4槽；普通区58槽的28任务上限将约减半，须重新算收尾预留。空COMMIT提前加载可能永久消费槽，仍需有界阻断/恢复，不能隐瞒 |
| 有因果标识的输入fence/drain | 被认证的游戏事实能指认H已消费，或OS原语证明旧输入永久不可达；且覆盖外部重复STAGE/APPLY | 若需独立barrier，至少增加对应输入/往返及一个明确消费事实；如原语可携带标签，已接近第一方向。计数、延时、accepted不是此事实 | 不能仅以reload/relogin宣称队列清空，同HWND/线程可能仍有旧输入；真正窗口/线程generation失效或排空能力须实证。常规槽成本取决于实际barrier而非假设 |

“每请求不可变入口”若只是**反复改同一个热键的回调去指向新请求**，并不携带输入身份：旧键到达时仍调用新回调，S01照常成立。为每请求使用不同物理键/地址且永不复用能形成区分，但会消耗有限输入命名空间；再次使用前仍须证明旧键不可达，不能把问题移到500槽换代。

将COMMIT放入文件的方向不要求证明某一次host按键是唯一触发源；它改为证明**gate之后才存在可执行的exact授权**。旧键触发已授权消息可按这个明示因果合同接受，但仍受私有当前条件和期限约束，仍不能放宽未回答的B1。相反，若COMMIT早在STAGE前随文件发布、或只公开一个可变“允许当前pending”布尔位，则没有新增所需事实。

保守策略“无法证明H消费就永不发布新STAGE”安全但可能在第一条命令后永久停住；它可作为未知终态，不能宣称可持续正常流程。清空journal、等超时、reload、增加accepted计数或再发一次泛用APPLY都不构成解决。

### 本轮交接决策

1. 接受P0有限证据；保留nonce观察，不重跑已经有封存身份的11类断言，不宣称完整协议通过。
2. 通用APPLY继续禁止接入产品。下一最小原型只比较一个exact提交载体与当前方案，先通过E-S01及原Lua旧APPLY反例；不要先写完完整driver再补因果证明。
3. 新原型必须从真实候选入口得到exact身份或因果标识，并注入所有partial/迟到/重复cut；不能在模型输入直接给`hostApplyDrained=true`。若走不可变COMMIT，加入“gate前任何旧键均不可执行业务，gate后仅exact消息可执行”的publication切点断言，并重新核算500槽。
4. 大COMMAND/最大BODY的强校验成本纳入实际nonce窗口，真实客户端与B1继续未验收/未批准。仅改变快照字数或状态名称不能解决S01。

本轮只修改审计报告和独立审计反例；没有native/Cargo改动、缓存扫描清理、部署或游戏输入。审查完成后交还设计者收敛，不用新增代理叠加设计。
