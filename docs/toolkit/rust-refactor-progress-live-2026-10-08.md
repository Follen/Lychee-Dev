# Rust Live / Windows 实施进度

工作树：`.worktrees/rust-refactor`，冻结 Go 基线 `v3.1.2 / 9d3d3598b8bac7efabf0b937df30508075eaa841`。
本页是实施证据与边界记录，不是完成声明或发行合同。没有提交、推送、发布或部署真实 addon；没有向游戏发送输入，也没有启停用户每 10 分钟 PostMessage 空格任务。

## 2026-10-12 定位设计修订

2026-10-12 最新定位修订：Mailbox 参考 Go `v3.1.0`，运行时指令特征匹配/RIP 解析动态 Lua 根，再沿 state → globals → Mailbox 读取；停止私有堆扫描。身份、认证、本次观察、exact COMMIT、持久结果及统一预算继续适用。见 [总方案 §7](rust-refactor-plan-2026-10-08.md#7-mailbox-运行时指令定位与-lua-路径读取)；旧原型证据只证明原受测范围，不代表新入口通过。 当前 Rust 磁盘 locator/exact-hash layout 门须替换为 RuntimeModule/RootBinding；历史记录保持原受测范围。

## 当前交付

| 模块 | 已实现及实际验证 | 尚未完成 |
| --- | --- | --- |
| Windows process | 只读 OpenProcess、进程创建身份、OS 主模块枚举、同 read-locked File 的 SHA256/原件、不可构造/不可修改的 ModulePlan、MEM_PRIVATE heap 与 MEM_IMAGE/MEM_MAPPED segment 前后检查、线程约束 mutex、RAII handles | 更多映射竞态故障注入、同文件实际身份/PE runtime 完整 section guards 的集成 |
| Windows input | 仅 PostMessage、每消息 identity/guard、modifier 释放、零/部分/完整发送证据 | 真游戏输入与绑定 profile 配对、新鲜度/光学联检实际适配 |
| Windows capture | 原生 WGC free-threaded pool、D3D11 staging/Map/Unmap、bounded channel、64 MiB frame budget、启动失败 Close、每 stream apartment 与进程 MTA retention | 实际 WGC capture/resource stress 为 `not_run`；不能用编译成功代替帧证据 |
| Native lifecycle | 激活→打开→EXECUTE→正文持久→SETTLE→关闭的一个推进器；intent 先 checkpoint；未知发送只观察；已发送后等待回执，不重复推进；runtime 变化保留未知结果 | 生产 NativeTransport、cancel/abandon/reload 完整入口、所有旧 OP/BTP 业务映射、结果/显示/资源清理事实的正式适配 |
| Journal | 独立结果文件、原子持久、SETTLE 前 digest 回读、同 PID/creation 跨 home guard、安装级共享 durable claims；无输入 activation 的 preclaim crash 可恢复；Closed 后精确退役 | 其余全部 crash 边界、reparse/hardlink 负例与公共 CLI 对接 |
| Mailbox | 按当前 Lua root/globals/namespace/field/key 读取、collision links、secret/type/length、路径回读、HEAD/BODY 的 runtime/nonce/ticket/length/checksum、512 reads / 1 MiB 总预算 | 生产已定稿 root binding、实际 build layout 准入；目前完整 lookup 自动证据为合成内存 |
| Static locator | 独立 Rust PE parser、完整声明 section 竞争、masked pattern、RIP operand constraints、typed output、独立 source.value/hash 依据、内部 FinalizedMethod、目标直扫无需 source runtime replay、正式 parameter→method→reader binding、layout exact-hash scope | 实际游戏 root 的磁盘规则定稿未完成；支持子集是 pointer_slot/module_rva 的 RIP extraction；field_displacement 与通用 wowdump 全格式不支持并明确拒绝 |
| Lua v4 | 独立 NativeWire/NativeProtocol/NativeRuntime 候选，私有去重与 detached immutable public records，EXECUTE+SETTLE，独立 phase gate，route/fuse/control recovery | 没有切换生产 TOC；真实 inventory、500 LoD 与 Bootstrap key adapter、新输入发布、AutomationHistory/ActivityView 的业务等价集成尚未完成 |

`lycheedev-live::commands()` 当前注册 probe put/list/show/remove，以及 instances/connect/execute/status/session/resume/disconnect/reload，共12个实际handler；正式 `host::dispatch_with` 通过 CLI 注入 DeliveryAdapter/WindowsServices。reload当前只准入已Closed生命周期，Idle/未决业务的旧效果仍未完成，capabilities公开该边界。生产 RootRule catalog 仍为空，对未定稿游戏 root 返回 `live.locator_not_finalized`，不发布槽文件或发送输入。其余旧业务尚未完成，不能把存在 handler 当业务等价验收。`capabilities()` 是组装入口事实对象，不是新增 CLI 命令。

## 500 槽离线决策

固定总容量：20 bootstrap + 8 lanes × 60 = 500；最大接纳实例数 8。
每 lane 的 bootstrap bind/recovery 是 `2*lane+1` / `2*lane+2`（16 个）；17..20 是共享 spare。其他实例不能预约这对专属控制位置。
每个已绑定 runtime 只加载自己的 route，绝不扫 foreign lanes；OS process identity、runtime、actor、sessionEpoch、owner/fence 属于不同校验层。

普通任务使用两槽。接纳前保留四个连续 route 槽，以及末尾关闭容量；正常空闲在 28 任务后需要 runtime 换代。500 不等于每实例 500 或 250 任务，旧每实例 47 轮验收必须重新跨换代验证。一次 emergency recovery 消费后不得再接新任务，在无未决结果的空闲点换代。

原型曾允许自由消费全部 bootstrap。这不能证明 fuse 后已有任务仍有收尾容量，已修正，未把旧测试提升为闭环证据。
新的私有 phase gate：running/reported 时普通 Wake 不加载任何 LoD；SETTLE 控制只在 private reported 时加载 own-route next slot，且不能执行代码。早到 submit 在 running 时零加载。一次空/错误 control 后 private controlBlocked 拒绝继续加载；exact bootstrap recovery 清除它，一次 recovery 预算保持不可重置。

真实 Lua adapter 用例覆盖：1000 late Wake、1000 late submit、异步 running→reported、一次空 control、错误 action、后续请求 reported 时旧 nonce、恢复→SETTLE→close、1000 重复 close；跨 lane foreign 加载为零。测试的是有限模型：每 runtime 至多一次受控错误 control 后使用正确 recovery 和已发布 SETTLE。有限池不能对无限、持续、恶意任意键入作无条件容量承诺。第二次故障或本实例 emergency 已消费时保持结果与 pending 清理事实；生产恢复/退役路径仍需完成，不能宣称 closed。

## 当前磁盘源与独立依据

本次重新读取本机当前安装的 wowdump Skill 及 v2 references（不是声称远程最新）：

| 文件 | SHA256 |
| --- | --- |
| `C:/Users/follen/.agents/skills/wowdump/SKILL.md` | `a166f0b124ebf488f872d19b6e40c41f6fe4fe2ed5e1f81c5193012a18be4b6f` |
| `references/v2/analysis.md` | `22e773de6ae787ec1eeb2f67f022c733175fca86a9a80bd251de473c0523ef8d` |
| `references/v2/recipes.md` | `b09b9625abf6e88fcf63d589161d17fba848d676e953d1a38a92bde1f489ff90` |

研究 CLI 实际 `wowdump describe` 为 Rust 2.1.2。它与官方 IDA MCP 仅用于研究，不是产品依赖。
用户后来允许以当前实际 build 为源并进行必要的只读现场定位调查；这不授权提前做人工功能验收。

当前 Retail 原件 `D:/Game/World of Warcraft/_retail_/Wow.exe`，96,334,544 bytes，SHA256 `d41f11de411f6fdb280a7c1c6380ba3d1b17614f03e5aca8fabce2cc715cd7dd`。
OS 确认 PID 42656 / creation `134358635916197740`，module base `0x7ff783b30000`，module size 136,511,488；这个当前文件与已有 12.1.0.69933 来源 hash 相同，不能假装换成了另一个 build。
只读当前 semantic site 的 58 bytes，第一条 RIP 操作数独立导出 source.value `0x79c0c18`，root pointer `2574404108592`；thread tag=8，globals TValue tag=5/secret=0。这些只读研究不是新 Lua v4 功能验收。
记录在 `.tmp/live-static/current-runtime-anchor{,.recording}.json`、`current-root-pointer{,.recording}.json`、`current-root-tag{,.recording}.json`、`current-globals{,.recording}.json`。原先独立分析依据为 `docs/toolkit/research/lua-mailbox/root.anchor.json`，hash `11d48aa512fd890df85066f30b4ffd1a086d30c1b75dcab0eb59ad033d4ceb0e`；已确认旧输出不能参与静态候选筛选。
随后独立 Rust `examples/current_source.rs` 在同 current PID 上实际运行：OS-sealed main module、同 read-locked file hash、完整 58-byte semantic site、root/tag/globals 双读与 process verify，8 次 / 182 bytes 均通过。结果 `.tmp/live-static/current-source-witness.json` 绑定完整 process/module/evidence 身份；source.value 从本次 RIP operands 导出，明示 diskFinalization=false。通用 wowdump memory recording 的 context=null 不被单独当成完整进程身份凭据。以该当前 witness 为独立 source.value 的新尝试在 `.tmp/live-static/current-retail-method.json` / `current-source-disk-finalization.json`，同样完整扫描后 SOURCE_METHOD_NO_MATCH；保留原失败材料。

实际 Rust 磁盘扫描完成 `.text` 64,564,172 bytes 的竞争域，对旧 runtime 58-byte 模板没有候选；原已知位点的 disk bytes 为 `7f716440464616a2471a39eb0bb905dac1f4776a71a1b4b1e8c3f263814feca1845a0c125d96282c1622357620e665d6a2a4a1a484fa89a3134e`，与 runtime 明文不同。失败报告 `.tmp/live-static/current-retail-complete-search.json`、声明规则 `retail-method.json`。这只否证旧 runtime 模板直接用于磁盘，不证明所有静态方法失败。不使用 encrypted anchor + 旧 delta 或 hash→固定 RVA 来绕过。

IDA 专用分析副本 `.tmp/live-static/current-retail.original.exe` 与原件 hash 一致，独立 instance `74286712e95e`。只读查询已发现：RunScript string RVA `0x4f44cc0`、Usage string `0x4f44cd0` 暂无静态 xrefs；loadstring string `0x4ff0280` 有明文 registry entry `0x4560b50`，其 function pointer 指向 `0x3cf3e00`，但该函数磁盘 bytes 也被变换。该 registry/保护变换及静态入口是后续研究线索，不是已定稿根方法。IDA 未修改原 EXE，未把 undefined/BSS 里的 IDA 填充值当磁盘覆盖。

## addon 全插件成本盘点与已验证优化

固定 Blizzard 来源只读：`wow-ui-source` Retail commit `09b9db7948abc9b9648dedaab51eb0cf3ee67b31`，来自当前 source snapshot pin。

| 路径 | Git blob | 本次用途 |
| --- | --- | --- |
| `Interface/AddOns/Blizzard_APIDocumentationGenerated/UITimerDocumentation.lua` | `8df6440842c744520cce54ae13c2086ecadbec03` | After callback 与 NewTimer/Ticker 的返回/secret 声明，不把定时回调当空闲免费 |
| `Interface/AddOns/Blizzard_SharedXMLBase/CallbackRegistry.lua` | `3affcc7a159c9dbf49f301ddc63d94921c7455d5` | 精确 owner 注销、reentrant dispatch 与 deferred mutation；不修改 Blizzard callback tables |
| `Interface/AddOns/Blizzard_SharedXMLBase/FrameUtil.lua` | `de0f6fee48710d0c03429448f4f5c5a412090d52` | 无活动任务时取消 OnUpdate、taint 边界；仅操作 addon 所有 frame |

盘点覆盖 Core/UI/Modules/Bridge：Runtime 是一次 loader；Safety 是 first-workbench-use 的 combat driver；Events/Trace 是固定环（500/300）；Stores history/export 分别有 16 MiB 上限；AutomationHistory 100 条 × 32 KiB preview；Theme reveal 140ms、Widgets 交互动画有停止路径；InputState opt-in sampler、ProbeExecution deadline、InputProtection timer、StartupBeacon 45s、ActivityView collection 20+5s、Receiver 20s 均属于有界活动工作。没有因 CPU 指标删除 capture、secret 检查、combat shutdown、freshness 或清理。

实际修改 `Events.lua` / `Trace.lua` 的隐藏页通知调度：隐藏时不排 After，Events 记 dirty 并在 activate 补刷新，Trace 现有 HandleActivate 从 ring 补刷新；可见通知保持 next-frame coalescing。
`t_refresh_cost.lua` 使用完整 page/widget fixture，优化前 100 hidden bursts 各创建 100 timer callbacks；优化后各 0；visible 100 bursts 仍 1 callback，200 records 保留，trace captureActive=true，activate/render 刷新两次不变。Retail/Classic/Titan/Forever fixture 都通过。disabled 注册前 frame count=0。
真实 CPU/RSS/分配峰值为 `not_run`；此指标是计数证据，不能宣称整插件耗时或内存改善比例。500 inventory 与 publication serialization 在生产 v4 适配后还需独立测量；当前候选 adapter 没有每 wake 的 global 500 sweep，但实际 activation inventory 与 clean-file guards 尚未集成，不能省略后声称提速。
另一个已测候选热点是 `NativeProtocol.finish` 同时持有完整 payload 与已包含它的 immutable BODY；payload 没有消费者，已移除该额外引用。真实 Lua 5.1 `native-v4-cost-test.lua` 的 500,000-byte result，在 collect 后 retained delta 从 1102.03 KiB 降到 613.68 KiB，约减少 488.35 KiB；receipt 535 bytes 不变，整个 BODY 保留。该自动指标包含 Lua string-table 开销，是候选内存保留量而不是游戏 RSS、分配峰值或 50% wall-time 证明。

## 自动验证与性能门槛

- Go build/vet 已在 addon edit batch 后通过；mandatory Lua affected suites `tests/addon`, `tests/protocol`, `tests/slotset` 通过，旧 v3 case 保留，新 v4 是 additional `t_native_v4.lua`，没有修改旧 wire 的生产加载闭包。
- 当前 Rust Live：19 unit + 7 完整合成 Mailbox tests 通过，包括最大 512 KiB BODY、损坏指针 overflow 与不支持 layout 的拒绝。
- 当前 Windows：5 OS self-process/file/mutex/job tests + 3 invisible message-only HWND PostMessage tests 通过，所有输入仅发向测试自己创建的 HWND，无游戏输入。
- `cargo check`、两个 crates 的 fmt 与 strict clippy `-D warnings` 通过。
- 第一次 sandbox MoveFileEx isolated-temp 测试 Access denied；相同隔离测试 require_escalated 后通过。原失败保留，不解释为产品 API 不可用。

新增硬目标：相同 local 工作负载总耗时至少减少 50%，同时保留质量、完整结果和 cleanup，CPU/峰值内存单列。Live 必须公平比较 Go v3 与 Rust v4 的普通/大结果/连续任务/恢复、cold/warm；probe 自带 sleep 与业务时间另记，不用它掩盖协议成本。当前只是普通四槽→两槽的离线计数，尚未达成或验证 wall-time 50% 门槛。

真实多 build、同 build 多实例、实际 WGC、500 LoD/目录缓存、200→500 runtime 激活、连续容量换代、性能阈值与最终人工功能/视觉验收全部 `not_run`。外部每 10 分钟空格和现有其他研究模块属于环境因素，不能把 callback counter 变化直接归因于本 host 按键。

## 正式 host / 文件 / Lua 端到端新增证据

`crates/lycheedev-live/tests/native_e2e.rs` 使用正式 `host::dispatch_with`、DurableStore、NativeTransport、实际 Delivery500模块生成/manifest/hash发布及 Lua5.1 NativeProtocol/Runtime peer，临时受管安装与 workspace。测试自己创建 HWND_MESSAGE，实际 PostMessage仅投向它；wire字节经实际 self-process MEM_PRIVATE读取。Lua publication和光学为显式 fixture：尚未经过生产 WGC、完整 Lua heap / sealed static binding，不能当实际游戏功能通过。

90KiB+Unicode完整BODY先独立文件持久化和SHA验证，再SETTLE；状态/同请求重复为read-only，close和重复close一致。最初44726因迟到Settled覆盖Closing失败，已修复并保留失败；原无Win合测93477通过30.20s，自有HWND/selfmemory2056通过31.37s。

异常批48532实际1/1 passed52.25s：真实Delivery pendingHash发布中断，同immutable请求resume补写并最终核clean/hash；接受完整publication前没有按键；部分输入在真实chord第5次guard失败，保持queued/release证据与Unknown，连续resume不重放；不同home同PID/creation不能抢占；Lua角色变更拒绝进一步发送。根独立稳定批91044通过50.62s，日志`.tmp/rust-supervision/native-e2e-independent-03.log`；此前4241通过29.74s日志02，初次--locked失败01在运行前发生，不当fixture失败或通过。

当前实际Retail PID42656上的未定稿负门禁仅只读OS/file身份：`.tmp/live-static/native-unfinalized-real-pid-refusal.json` exit3/inputSent=false；隔离home只有workspace.json，无CON/journal或slot publication。原研究source witness/disk noMatch不提升为执行准入，`diskFinalization=false`、`executionAdmission=false`。

## 候选 Lua 生产闭包与物理 Loaded 约束

新增 `NativeAdapter.lua`、`NativeInputState.lua`、`Lychee Dev Rust.toc`，原`Lychee Dev.toc`仍加载冻结v3闭包。候选adapter接真实Compat LoadNativeInputSlot(1..500)、ReceiverBindings、InputProtection、ProbeExecution、InputSignal。首次activation一次1500 metadata调用（500×3）；每实际load只重查本槽3项，host仍校验完整受管文件，不省clean守卫。INPUT改wire07/input.v4，身份快照从private state生成，编辑Mailbox不授权执行。

三个固定键不能直接索引8个bind位置，因此首次Wake仅扫奇数1..15，最多8次LoD；找到own runtime的open即停止，even恢复槽保持未加载。foreign bind在本runtime会变为Loaded，即使未调用private Receive，它也不可再次执行。候选明确只允许一次bind搜索；同runtime close后reconnect/换lane返回native_reload_required且零加载，不能重新写Payload并假设LoadAddOn再次执行。正常EXECUTE+SETTLE仍2LoD，首次connect最多8成本单列。生产host的qualified reload准入与route退役/重连仍须完成；此限制不冒称旧连接业务已经等价。

`t_native_adapter.lua`在四client profile通过：disabled零frame/metadata/load；一次500inventory；扫描foreign bind→lane2连接→EXECUTE/SETTLE/close；1000lateWake不吃收尾；LoD Loaded不重执行；同runtime重连拒绝；public identity修改/返回snapshot修改不改变private owner/nextSlot；bridge-off sampler停止。首fixture遗漏ActivityView.Receiving的模拟导致保护不可用，修正fixture后通过；次fixture错误期待close消耗前slot143，改为实际next144后通过。整TOC startup、历史/活动报告完整业务映射、生产动态fuse恢复仍未全验收。

`t_native_candidate_closure.lua`现已通过四profile完整候选TOC disabled启动：一个loader、一个binding owner、三个buttons共5frames；PLAYER_LOGIN结束后无OnEvent/OnUpdate/常驻timer，Mailbox未创建。候选History Observe/ActivityView状态接回：running/reported/released沿用原bounded history preview，完整payload仅临时借给Observe，不增加常驻报告副本；真实游戏历史/UI表现未验收。

## 密封绑定到实际进程读取的新自动层

`cargo test -p lycheedev-live --test sealed_memory --locked --offline -- --nocapture`实际通过（d3ebc3，1/1，0.92s）。测试自己的EXE定义明确x64 RIP accessor，先用compiled symbol RVA及调用返回值建立独立source.value，再由完整磁盘引擎扫描 `.text` 2,845,600bytes唯一candidate。此批原件SHA256为 `127235a3b5c6b34029b15a4a28bcfd1c3b31fec62831dcc2f0c9a099be090d12`，可重建后变化，必须按当前proof核hash；保存 `.tmp/live-static/sealed-self-test.original.exe`、`sealed-self-test-proof.json`（包括method/source证据、完整competition、candidate和OS实例）。History.semantic_proof=false表示引擎不自行替调用者证明函数业务语义；独立test accessor/symbol witness保留在proof中。

opaque FinalizedMethod→typed `/root` Parameter→hash范围Layout→OS mainmodule不可构造ModulePlan→同readlocked file→运行时PE结构/anchor guards→真实MEM_PRIVATE完整syntheticLuaHeap→named globals/LycheeDevInternal/Mailbox lookup已验证。wrong parameter type、wrong layout hash拒绝；namespace变secret后拒绝，恢复后重新解析成功。不是Retail ABI、游戏root、生产WGC或游戏输入验收。当前Retail未定稿门禁仍保留。

最新Live24unit、Windows2unit通过；增加pending cleanup不可close，以及普通deadline exit6/live.deadline_pending与caller取消exit7区分。等待预算不超shared OperationControl deadline；已有不确定输入保留journal，不把超时改写为取消或重放。Windows publication在写槽前先验证本实例route/owner，成功exactpublish后继续核pending/filehash，再允许输入。

当前TLS进一步研究保存 `.tmp/live-static/ida-tls-entry-research.json`：TLS0/entry互补跳转进入IDA已有指令内部，现有linear disassembly/xrefs与精确decode边界不一致，不能把autoanalysis结束当CFG完整。entry可见PEB模块链及FNV名称解析；尚未证明与Lua根或磁盘代码变换的关系。raw JSON保完整64-bit operands，admission=false，未patch/save数据库或改EXE；不把局部未解释路径扩大为所有静态方法失败。

## 激活与qualified reload新增稳定批

disabled基础设施现有一次有界passive identity/INPUT记录（每record编码上限2KiB），属于bootstrap观测预算；它明确activationOnly=true/inventory=0/inputBlocked=true，无500metadata loop或frame/event/timer/OnUpdate。不能沿用早期“Mailbox未创建”的fixture结论当当前证据。真正按Wake是明确Enable而非toggle，先核实际当前login/combat/focus，然后启动protocol/sampler和一次500inventory，零LoD、零业务执行；下次精确OPEN才扫描bind。explicit-off再次Wake恢复观测也不加载槽。disabled .Start不创建engine或业务队列；缺actor的首安装通过原loader的一次PLAYER_LOGIN重试，不新增driver。

正式host先持久Activate意图、固定OS PID/creation/HWND/module/managedbytes；未知/partial激活仅观察不重发。新的runtime/actor出来后才claimroute，不能以空runtime或旧bootstrap runtime占lane。无request的resume不先执行request runtime校验：8853因此错误live.target_changed（51.68s），修复后93454 actual完整候选TOC/安装代码+真实500Loader/Payload+Lua NativeAdapter通过91.20s。该批包括首次Enable后OPEN→90KiB结果持久→SETTLE→close，partialActivation自有HWND queued2后两次resume零重放，外部显式Enable后观察恢复。外部Enable不归因于原未知Host输入。

关闭后reload已接同lifecycle：原CON exact claim持久reactivate，固定字面Esc×3 Enter /reload Enter经PostMessage投定向目标，pendingAction在发送前存盘；zero/partial/unknown不重发。新runtime同actor/window/module、unowned、两个递增sample及clock后签发opaque replacement Proof；scope含原instance/owner/epoch/fence。routebook只退休原lane，保留retired tombstone，foreign lane保持逐字节状态。closed旧runtime再claim拒绝；新runtime才复用释放lane。read-only同reloadrequest重复保持原control事实；另request必须新连接，防止用旧fromRuntime假报第二次新reload。

72979正式E2E通过100.20s；Root90588独立通过99.71s，`.tmp/rust-supervision/native-e2e-independent-04.log`。包含ActualAdapter、实际managed candidateTOC全闭包与500Loader/Payload、private fresh claims、部分未知不重放、close→qualifiedreload→两fresh samples→actual routebookexact退役→同PID/actor重连。LoD/namespace换代由明确Lua reload fixture模拟；光学/WGC未合测，Retail ABI/实际游戏功能仍未证明。Windows input4fixtures通过（4cd459），固定17msgs、partial8msgs、150ms×4/50ms×7等待计划；fixture wait是计数，不是假报游戏实际pacing。最新Live28unit72a842通过；strictalltargets38395e通过。

新增生产selection比较当前Mailbox character/realm/product/fullBuild与bounded本地.flavor/version及可选pinned source/data，--session connect转原生命周期resume。Core client catalog fallback本身仍partial，不将folder/interface算术当新的build证据。真实客户端所有selection矩阵与旧OP/BTP业务映射未全验收。

## 当前代码与metadata只读归档研究

PID42656:134358635916197740、原件d41f11...身份保持。本地wowdump2.1.2仅研究工具，不是产品依赖。第一次.text8192page reads触budget，只读32MiB，有suffix gap，保留`current-retail-runtime-text`。新独立capture `current-retail-runtime-text-full/Wow.exe/manifest.json`实际64,564,172bytes/15,763reads/0gaps/atomic=false；source完整身份/captureId/原件SHA与inputClosure保留。`runtime-loadstring-registry-function.json`来自该runtime来源的离线反汇编，不当磁盘代码。

.text-only xrefs缺函数边界，输出complete=false/all.text gap。单独.pdata采集2,490,744bytes/609reads/0gaps，但different captureId与.text合并被正式工具拒INPUT_CAPTURE_MISMATCH，不改ID伪合并。同一次bounded modulecapture `current-retail-runtime-module/Wow.exe/manifest.json`实际136,492,359bytes/33,327reads/0gaps/atomic=false，仅module、不读heap。xrefs报告`current-root-full-module-runtime-xrefs.json`已有256命中但max-hits截断且92,005个解码/函数覆盖gaps，complete=false；不据此称完整CFG或全竞争唯一。首256 runtime refs与disk同位12bytes比较0exactMatches（`runtime-root-reference-disk-comparison.json`），仅说明该样本有变换，不扩展为所有静态方法失败。

精确磁盘branch解码已保存tls0-{first-branch,real-branch,selected-block}-disk.json：4458 JNE4460、4460 JNE44B2；44B2的正确块计算0xdf8-0xda3=0x55并写word RVA56e2a78，其Lua root/变换语义尚未证明。

单独runtime-text分析包装`current-retail.RUNTIME-TEXT-ANALYSIS-ONLY.exe`/provenance，derivedSHA0b97aa8...，只有.text来自runtime；其他sections/headers是disk scaffolding，BSS/heap/runtime data未知，admission=false。IDA专用instance bc1aed6c5216首次execute等待autoanalysis在300s RPC超时，serverlog只tool_call没有tool_result；不称完成，不重启。实际owned worker链已核到PID44796，8threads/RSS约1.56GiB、累计userCPU约408.8s，说明进程存在，但单次累计数还不能证明当前autoanalysis推进。原disk实例74286712e95e对应worker71960独立；二者未patch/save。后台IDA CPU/IO需作为性能测量外部条件，不能混入Go/Rust公平bench。

## 116s新Native profile与Source共用Windows平台批

Native E2E48492实际terminal0，1/1passed116.28s：新增Idle先close再qualifiedreload；partial EXECUTE显式reload保留original_action/messages/executionUnknown，report unavailable且旧status complete=false；compile error成为verified failed BODY并经过SETTLE，零业务fn调用。可读完整BODY在reload前先单独durable保存与SHA核验，失败阻止发送。30unit通过，最新strictWindows+Live c8ca77通过。Root尚未独立复跑本116s新profile，不把旧99.71s覆盖套用新case。

Windows共用平台新增broker.rs：canonical home+current logon SID确定endpoint，protected logon-only DACL、PIPE_REJECT_REMOTE_CLIENTS、FILE_FLAG_FIRST_PIPE_INSTANCE，accept交接前预建下一instance保无owner空隙。OVERLAPPED+completion event，IO每片<=64KiB；AtomicBool没有kernel事件，所以50ms bounded wait分片检查cancel，明确有idle wakeups，不假报0idle cancel。CancelIoEx后GetOverlappedResult terminal drain才释放buffer。client/server peer PID真实API，processcreation/exe SHA由Source核验。try_clone实际DuplicateHandle；drop单clone不Disconnect破坏另writer，最后handle自然EOF。native109/233 read返回EOF0，connect2/231/5稳定not_found/busy/access_denied分类。23c34f实际EOF233 fixture失败保留，修后c8ca77三owned pipe tests通过（peer/first-instance/timeoutcancel/cloneobserver退出writer仍工作/EOF）。跨实际另一logon、remote、DACL hostile peer与大规模stress未运行。

ChangeWatch原生ReadDirectoryChangesW+completion/stop events，start在arming后返回；name/dir/write/size/create/attrs/security任何notify或loss使session取消，64KiB bounded buffer，changed()同时直接检查kernel通知event；Drop CancelIoEx terminal drain+join，无idle timer/poll。实际temp transient insert/delete后两次membership可能相同但仍取消，fb97b1通过。重解析目录拒绝；watcher多树/overflow/reparse竞态更多negative仍需补。

BrokerJob::own_current_process(2<<30)设置broker+继承children aggregate commit ceiling及kill-on-close，Source先join再显式disarm。dedicated own executable+cmdchild d0bb80配置2GiB并正常disarm后生存；不是RSS/内存压力证明。disarm失败不关闭armed selfjob以免杀报告者，保留到process退出并由explicit方法报错。Process::parent_pid实际ToolHelp inventory+before/after子进程creation verify，给CLI sealed broker启动核actual parent用；当前API compile通过876b90，尚需独立child parent negative fixture。

IDA actual状态随后经官方installed Nexus只读poll_autoanalysis=complete核实，同bc1查询真实成功420datarefs/262相关functions；small函数有positiveSP或opaque flow，不能据pseudo当已证明getter。存ida-runtime-root-research.json。新的runtime fullmodule localxref扩8192只得到374、不截断但仍92,005函数/解码gaps，仍不完整CFG。原件/disk未改，RootCatalog仍空；静态研究继续关联getter/注册/call语义到磁盘变换，不拿旧RVA数值绕过。

## 原业务分阶段与终止恢复新增批次

公共真实 handler 现为18项；新增 probe load/run/ack/finish/cancel/abandon。v4 load 冻结 host request（源、SHA、1..120s budget、request key）后返回 Prepared，无 slot publication/物理键/业务执行；run 仅推进至完整 BODY 独立持久与 SHA 验证，ResultDurable/report verified/cleanup pending，不消费SETTLE。ack/finish要求已有精确durable report，才做SETTLE与resourcesReleased/displayCleared；正常CON长期连接仍保留，任务预约单独释放。旧OP的SavedVariables queue load/reload机制不混入v4 wire，旧未决OP/BTP仍要求其原记录恢复；新路径注册本身不代表完整旧profile参数/effects等价已验收。

cancel只在Prepared、无PublishExecute intent、无input_messages（含0）、无executeExchange时允许。abandon拒绝Prepared/Idle/成功Settled，保留原phase/action/messages/code/evidence及executionUnknown，终态complete=false、cleanup=abandoned；只退休exact安装claim，不发键、不伪报ACK/runtimeunload、route不回收直到qualified replacement。成功终态重复resume/status无native capability调用。

Host历史MEM读取现在先于CON terminal短路，合法MEM-A Settled→同CON MEM-B Abandoned后，resume/ack/finish/status MEM-A返回A原result/cleanup/id而非B。terminal checkpoint先写CON再归档/retire的失败cutpoint有恢复：complete_terminal仅补缺归档/仍pending的exactclaim，不重写已成功receipt/claim。status ownershipReleased实际读claim，不以终态枚举推断。真实Windows claims文件open仅SHARE_READ/noDELETE使MoveFileEx返回ACCESS_DENIED(0x80070005)，终态日志+metadata保留而claim pending；移除读锁后resume退休且journal bytes/成功重试claimmtime不变。

新native_termination四项实际通过95309f（0.63s），32unit通过，strict2f53f4通过。首次同actualPID并行测试lock.busy失败d42b14后仅fixture串行，不放宽生产锁；d821f9 HRESULT assertion只接受sharing20但真实denydelete返回access5，修稳定code/HRESULT断言后通过。Delivery中间sha2 unresolved导致2fa776未执行新测试，owner改为core controlled streamhash后恢复，不隐瞒初始失败。

新增分阶段实际managed文件+Lua5.1+selfHWND/memory E2E92041通过119.63s：load零input、无report ack拒绝、mutable probe selector remove后原冻结code仍run、90KB verified BODY pendingcleanup、重复run只读、ack SETTLE、重复finish只读。Root独立最新完整batch handle57791 terminal0：termination4/0ignored0.65s与native_e2e1/0ignored123.72s，日志 .tmp/rust-supervision/native-termination-independent-02.log 和 native-e2e-independent-05.log，包含历史MEM及terminal cutpoint当前源码。

Root Windows broker独立3passed日志 .tmp/rust-supervision/windows-broker-independent-01.log；watch transient insert/delete+idleDrop独立1passed日志 .tmp/rust-supervision/windows-watch-independent-01.log。Root当前Lua5.1 affected full suite addon8.118s/protocol16.919s/slotset0.337s terminal0，日志 .tmp/rust-supervision/addon-protocol-current-independent-01.log（含native/closure/refresh_cost四profile）；不扩为人工游戏、WGC、hostile logon/remote、跨build真实ABI验收。

## 生产任务预约接入与容量未决

真实installation routebook新增reserve_request/settle_request，命名scope/owner/fence/epoch/currentruntime/actor/physicalcursor检查；预约在PublishExecute前durable，verifiedSETTLE才消费两slot并释放exact请求，foreign lane与pending请求不动。28正常任务后cursor56，需要capacity reload，预留收尾槽仍存在。af1da2真实file/claim unit验证28次SETTLE可继续下一task、重复reserve/settle只读、wrongepoch/foreignscope拒绝、foreign pending lane逐字节不变；6713cc33unit+strictpassed。初次放宽pending ProcessExited使旧unit失败f542c0，已恢复裸PID exit不退休未决任务；仅生产opaque qualified RuntimeReplaced允许旧pending预约退役，保留原lifecycle unknown。

该接口已接WindowsChannel与NativeTransport、actualAdapter file fixture，整体新E2E原handle67484正在运行，未收终态前不计通过。完整28task自动换代、47task旧场景、新LoD双actualPID/多build fixtures仍未实现完，不能将capacity_reload_required当等价完成；绑定/bugs/hide/reset/reloadfallback及profile参数仍余下范围。CON安装claim属于长期连接，taskreservation属于单次request，两者不能混淆。

## 当前static getter/caller与磁盘关系新增证据

同已完成IDA runtime分析instance bc1aed6c5216只读提取全部420datarefs保存 .tmp/live-static/ida-runtime-root-full420.json（IDA覆盖本身不代表全CFG）。所有420同RVA12byte磁盘exact比较为0matches，完整哈希再次核d41f11de411f6fdb280a7c1c6380ba3d1b17614f03e5aca8fabce2cc715cd7dd，.tmp/live-static/ida-full420-disk-corroboration.json。仅说明这些IDA已识别runtime根访问窗口与磁盘不同；没有DIR64规则应用，不证明所有静态方法失败、未知变换不能伪称decrypt。

5个正常有界函数raw instructions+caller保存 .tmp/live-static/ida-runtime-getter-callers.json：28bfa60(68bytes)检查相邻generation并调用3cf21f0再清ref；29c02c0(73)、2cacb70(108)、3a3f700(115)、3a3f780(239)留实际调用边界，不用positiveSP伪反编译作语义。来源仍runtime.text analysis wrapper，其他section为disk scaffolding/runtime data未知。下一调查需把具体getter/caller/Lua注册semantics关联保护变换或独立磁盘入口；RootCatalog仍空，diskFinalization/executionAdmission=false。未重启IDA任务或改/保存原件。

67484随后已原handleterminal0，native_e2e1/0ignored124.11s；包含生产Channel任务预约与actualAdapter routebook/SETTLE/partialunknown qualifiedreload接入，不扩为actualgameWGC/ABI。Root任务预约实际routebookunit独立1passed0ignored0.46s，.tmp/rust-supervision/native-task-reservation-independent-01.log；Instance100/200是model，不是实际OS双实例。foreign比较为canonical typed lane state（serializable JSON），不能把它说成整个routebook原始文件字节未修改。

Windows新实际OS双实例自动fixture `crates/lycheedev-windows/tests/multiinstance.rs`：相同实际testEXE/hash的两个owned子进程，各自真实PID/FILETIME/父PID/mainmodule hash/独立HWND_MESSAGE与MEM_PRIVATE只读buffer，向A定向chord6msgs后B0；foreign HWND配A process和篡改created均0msgs；向B partialguard只发owned CTRLdown/up2msgs且A保持6；退出A后旧identity拒绝、B继续可读/只有2msgs。PostMessage只投自建child windows，不是游戏。0410b4通过0.15s+strictWindows；实际scope只有1个主fixture（另owned_child_entry是子进程入口的harness辅助test，普通运行直接返回），不声称2业务case。proof .tmp/live-static/windows-actual-multiinstance-proof-01.txt保存实际PIDcreated/hash/计数。父子JobGuard attach仍spawn→attach边界，无产品公共测试flags。此层不包含LuaLoaded/正式LiveHost/完整Luaheap/WGC或真实多gamebuild；下一步须把相同owned targets接full native fixture，不拿OS平台结果代替完整多实例验收。

## RuntimeArchive/External 合同与独立原型（未开放生产门禁）

按最新本机wowdump2.1.2 SKILL/analysis.md/recipes.md复核：运行时dump是合法离线static分析输入；recipes.md的显式External方法保留其输入合同，明确target-manifest可选运行时目标。但原生binaryVerification finalized仍要求精确原磁盘内容完整声明范围唯一+独立source.value，不把runtime-derived PE包装称原件或disk finalized。schema已只读导出 .tmp/live-static/wowdump-2.1.2-schema.json，analysis_source.kind允许disk/runtime且atomic固定false，analysis_step.external有显式method输入对象；这是另种分析输入路线，不是导入旧定稿声明绕过。

独立Rust runtime_archive.rs+runtime_archive_location example，不调用wowdump/IDA；Archive读取选定域的manifest/fragment SHA，实际deny-write/delete文件句柄，拒绝路径逃逸、hash改动、覆盖gap/overlap、多section、原件身份/capture PID不一致；精确PE executable section约束后完整扫描。ArchivedMethod/Report私有构造/noDeserialize，不导入flags签能力；source.value独立evidence hash后才核输出，未以旧RVA筛候选。目标独立Archive完整重扫，不读source旧文件；正式named luaStateRoot/PointerSlot→/root、exact原件hash布局绑定分开。

31505原handleterminal0，.tmp/live-static/current-runtime-archive-prototype.json：source/target同fullmodule archive重复扫描64,564,172字节各1候选，输出127667224与独立current-source-witness hash233d5603...一致；module原件d41f11.../manifest4773643a.../domain69523086.../PID42656creation134358635916197740/base保留。Root独立32165改source为fulltext capture rust-root-text-study-full-20261008、target为fullmodule capture rust-root-module-study-20261008（不同capture，同declaredOSinstance），同完整域各1候选，.tmp/rust-supervision/runtime-archive-current-independent-01.json。Source/target均atomic=false，disk_finalization/semantic_proof/execution_admission=false。完整字节竞争只对该不可变archive成立，不是假称原生disk credential、完整CFG或当前进程atomic快照。

7eb390 Live36unit/strictalltargets通过；Root95199 runtime_archive三fixture独立passed0ignored0.09s，.tmp/rust-supervision/runtime-archive-fixture-independent-01.log。含源manifest/fragments删掉后不同declaredPID target重扫仍不读旧source、Sourcevalue冲突/多候选不能制造唯一、hash/gap/overlap/identity/capture/pathescape拒、文件denywrite锁。此不同PID是模型身份，不是实际newOSinstance gate。共享scanner重构后的真实selfOS模块+synthetic完整Luaheap sealed_memory 1passed0ignored0.63s，fe8684随后Go build/vet terminal0；不扩为真实RetailABI/WGC。

下一环节需要OS-inventoried same readlocked originalmodule capability+当前完整域code重读、当前criticalanchor/PEheader guards和source/target过程身份检查，明确nonatomic限制并失败关闭，另需reader真实ABI适用范围与多build布局证据。此前RootCatalog仍空，不让本prototype签RootRule或发真实游戏键。

## Source broker no-inherit 最小平台接口

Source独立诊断在父CLI退出后yield2秒仍未stdout EOF、broker OSidentity/authready，显式retire后才EOF（report rust-source-broker-inheritance-observe-02），只此新诊断支持真实继承因素；保留旧阻塞callback测量的局限。Windows detached::spawn_hidden_no_inherit 为精确absolute exe/Windows UTF16 quoted args/customUnicode env、CreateProcessW bInheritHandles=false+CREATE_NO_WINDOW，不改globalenv或父handleinherit属性。DetachedChild真实PID/FILETIME、try_wait、boundedterminate_and_wait，startup未handoff Drop kill/reap自身；authenticated owner确认后handoff只关闭父handle。进程子树清理由Source在readiness前建立Self BrokerJob，hProcess TerminateProcess不冒称自动killtree。

官方依据：[Rust CommandExt](https://doc.rust-lang.org/std/os/windows/process/trait.CommandExt.html#tymethod.inherit_handles) 当前inherit_handles仍nightly且默认true；[CreateProcessW](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessw) FALSE禁止继承。Windows增加Win32_Globalization仅已有SDKfeature，无新package/lock/globalconfig改变。

actual私有inheritable anonymouspipe+StdIO null的stdCommand对照仍保持pipeopen；nativechild保持alive时PeekNamedPipe立刻ERROR_BROKEN_PIPE，真实parent/created核验、Dropfailedstartup重收、relative/NUL/envkey拒。原60847三个harness tests通过（2业务+1辅助），末clippycollapsible_if失败已修8a1f18strictWindows通过。Root独立只2业务passed0ignored0.03s，.tmp/rust-supervision/windows-noinherit-independent-01.log，child51352 created134359012883358055 parent29820；实际EOFbeforechildexit，不用JS延后callback推断。Source仅原brokerlaunch处接入，公共packaged broker gate仍由Source后续stage09验收，不能以这两个平台fixtures替代。

Broker平台又补四轮actual accept/cloneDrop/EOF/serverDrop仍保持listener firstinstance ownership，87ad9b1passed0ignored与strictWindows通过，排除基本secondaccept重置FIRST/dropclone关闭所有连接的猜测；不替代Source公共生命周期或hostileSID/remote负例。

当前实际module重验新API Archive::validate_current已落：OS-inventoried same readlocked module capability，exactPIDcreated/base/hash/imageSize/PE executable域检查，然后2遍完整domain读取（每logical chunk64KiB，shareddeadline/cancel），均须与不可变archive domainSHA相同，否则failclosed。56745原handleterminal0，仅实际PID42656creation134358635916197740 module/code，2×64,564,172bytes均69523086...，1972logical read_module calls/3410ms，.tmp/live-static/current-runtime-module-revalidation.json。没有heap读取、RootRule签发或键入；atomic=false/execution_admission=false仍明确保留；两次匹配不是冻结/原子snapshot，更不证明reader全部ABI。新actualOS实例、header/criticalanchor的生命周期承载与生产RuntimeRoot注册仍未完成。

Source retirement又需原held Process::wait_exit(deadline,cancel)，已窄实现SYNCHRONIZE kernelwait，原FILETIME核验、50ms cancellation wait tick、终态GetExitCode；不重开PID，不terminate此方法。15d037 detached测试通过（2业务+helper，0.03s）与strictWindows+Livepass：已exit原held handle返回7、startupDrop重收后wait成功、self活process deadline/cancel分别保command.deadline/command.cancelled。Source在authenticatedretire前hold handle，response后wait actualexit再prune，公共Source验收由其owner记录。没有借此移除原idle期限。

## 双实际OS目标接完整Host与LuaLoaded新稳定批

自有target child支持抽入Windows tests/support/owned_target.rs，仅test harness入口；Live formal fixture使用两个真实OS进程各自HWND_MESSAGE/Process FILETIME/OS-inventoried same testEXE SHA/held deny-write moduleFile，选定target的实际MEM_PRIVATE wirecarrier通过read_private读取（Lua VM是明确sidecar，非游戏ABI），snapshot address使用实际carrier地址，同wire保持不可变地址供freshsample守卫。没有产品CLI测试flags，没有改变生产physicalheld保护。

原62624 terminal1/21.07s fixture Struct构造维持RefCell immutableborrow与stage_wire mutableborrow冲突，已缩小borrow寿命。原14243 terminal1/54.72s到A disconnect时GetAsyncKeyState真实physical_key_held，保留failed；没有删除或放宽guard、没有重放该旧lifecycle。新ownedtargets独立批原66464 terminal0：formal_host_two_actual_targets_share_installation_and_isolate_lua_loaded_routes 1passed0ignored61.60s。最终strictWindows+Live3ddcf2通过。

proof .tmp/live-static/native-actual-dual-target-proof.json 保存实际A PID72324creation134359033813852723/HWND907022580、B PID48520creation134359033815774311/HWND493885206、same testEXE SHA2d847e07210fc1b2e3bb76e1f76232c0950421760fad0d9d7f9fa425ac6b5f24。A首次bind加载1，B实际永久Loaded foreign bind1再own bind3加载2；各真实500受管Payload/Loader+Lua NativeAdapter执行90087-byte完整BODY→SHA持久→SETTLE（route cursor各2/taskreservation空）。A close→fixedqualifiedreload17msgs→两fresh private样本→exactroute退休→同actualPID/actor重连；B foreign canonical lane state/Loaded不变，物理messages保持26（自己connect12+EXECUTE6+SETTLE8），A动作不投B。

artifact含A/B target、result、before/after真实routebook，以及持久session的nonce/ticket/exchange slot/payloadSHA/requestdigest/resultSHA/bytes/adler/checkpoint/reload facts。末新增session证据自动输出不变业务；成功tempjournal同源证据已补入当前artifact。Root实际18576正在独立此exactcase，未收终态前不计独立通过。原Windows平台Root independent proof为其自己的actualPID42492/608与testEXE0d15452f...，与本新HosttestEXE/PID分别记录，不套原件/hash。

证明范围：正式Rust Host/NativeTransport/claims/journal/managed Delivery500files、Lua actual NativeAdapter/Loaded模型，physical PostMessage只自有两个actualOS targets、selectedprocess的readonlyMEM_PRIVATE wirecarrier；光学fixture、Lua VM sidecar，不是生产WindowsServices RootCatalogue/WGC/Retail heapABI。完整47task自动capacity换代、多gamebuild/其他故障组合仍原任务未完。binding/bugs/hide/reset/reloadfallback等5未注册path与完整legacyprofile/effects、全部Live50%宏观门槛也未完。

Root双实际Host独立首批18576随后terminal101/49.23s，在live execute遇input.physical_key_held，日志 .tmp/rust-supervision/native-actual-dual-host-independent-01.log。GetAsyncKeyState仅highbit(<0)已核，不是recentpress lowbit误判；这一环境held失败不冒称业务回归通过，也不抹owner66464成功记录。新fixture-only chord_admission只记录精确action的≤4允许keys的highbit、phase、fixedtarget与queued（不采其他按键/非globalkeylogger），before/after/error写 .tmp/live-static/native-chord-admission.jsonl；生产guard、key通道、未知输入不重放均不改。新ownedtargets原87938正在执行，等待原handle终态；未提前算通过。

Source实际broker27220/created134359040219327134观测0xC0000005 ACCESS_VIOLATION，不能把idle/EOF或管道猜测当原因。本lane按diagnosing-bugs只读设施调查发现现成 C:/Program Files (x86)/Windows Kits/10/Debuggers/x64/cdb.exe、windbg.exe、dumpchk.exe/dumpexam.exe，无安装或global WER registry改动；CrashDumps未见lycheedev*.dmp，严格该PID近2h Application1000筛暂无匹配输出。Source提供下次authenticated owned PIDcreated/stageexeHash后可单变量debugger栈取证，只该broker、不触游戏。Broker短4轮unit Root独立1pass .tmp/rust-supervision/windows-sequential-pipe-independent-01.log 不代表fullSource AV修复。当前大payload+cancel/clone pending drain code未见可直接断言unsafe，需真实异常PC/stack；尚未patchAPI因果。

本批最后Go build/vet原10433 terminal0，符合改动批次基础gate；不扩大为新fullLua/游戏验收。

新允许chord诊断fixture原87938随后terminal0：同exact fullHost actualdual1passed0ignored66.84s，before/after相关keys highbit与阶段/queued日志保留。仅该新ownedtargets执行成功，不将Root首次18576failed改写；Root当前独立本诊断snapshot尚未执行。原owner66464/Root18576/新87938三次时间与各target/moduleproof分别留。

Root后续实际双Host独立1733已terminal0：exactcase1passed0ignored70.72s，`.tmp/rust-supervision/native-actual-dual-host-independent-02.log`与`native-actual-dual-host-independent-02-proof.json`。actual A PID69732/creation134359051373256372，B PID73080/creation134359051375504839，同test EXE412d3...；both verified/complete，B physical messages26，foreignLaneUnchanged=true，记录的allowlist keys highbit全部false。第一次18576 held-key失败保留。此是Host+LuaLoaded/self HWND/self MEM_PRIVATE wire/optics fixture，不是Retailheap或真实WGC接受；47task capacity及5未注册旧path等原scope继续。

## 47任务生产自动容量实现批（完整fixture仍进行）

Root此前1733独立actualdual exact1passed0ignored70.72s，.tmp/rust-supervision/native-actual-dual-host-independent-02.log / -02-proof.json：实际A69732/B73080各PIDcreated/HWND，same testEXE412d3...，bothverified/complete、Bphysical26/foreignlaneunchanged；首18576physicalheld失败保留，与owner87938诊断成功分别记录。

新增生产Session.CapacityRotation：下一个id/code/SHA/budget/newEpoch在close/reload之前持久冻结；仅旧Settled且cleanupcomplete/no pendingAction/28任务达额可自动换代，未知/未清理/未证明replacement拒绝。自动Close→fixedReload，private两freshsample签replacement并退休exactoldlane后，仍同CON/actor/PIDcreation/module，fence+1/newEpoch/sequence0，exactexistingclaim重新activate，freshOPEN后才在新runtime prepare并执行冻结next；不重发原旧业务。历史control有界128并保留from/to/samples，旧MEM精确metadata继续独立。route退休tombstone改owner+oldruntime键，允许同CON多次换代，兼容读取旧owner单键对本exactinstance的记录。

4b9bb0 Live38unit与strictpassed：冻结next→close/reload→reopen新runtime/fence后prepare新request且旧InputExecute不重放；unknown/unclean/unproven否决。当前生产RootCatalogue gate未变，人工游戏/WGC/真实RetailABI不扩大。

真实47fixture formal_47_tasks_automatic_capacity_reload_keeps_actual_foreign_target_unchanged 使用两个actualownedOS targets/heldmodule/readonly MEM_PRIVATE wirecarrier、真实500Payload/Loader与Lua NativeAdapter/永久Loaded模型，sameCON47(28+19)、任务29自动换代并检查foreignlane/Loaded/messages不变、重复最后request只读、旧MEM原样。原51861 terminal1/52.94s，未到capacity时InputExecute.before/failed明确VK18(ALT)highbittrue、CTRL/F12false、queued0；保生产heldguard/不重放该旧lifecycle。fixture-only wait在原inv.control.deadline内最多30秒等精确allowedkeys释放，随后重新freshSource，禁止延deadline/globalkeyrelease，真实chord仍保自己的guard，非零/未知不会retry。

下一原37856 terminal1/319.51s实到A seq28/Settled、B Idle，出现业务cutpoint：queue_capacity checkpoint携带next而旧Settled request仍在Session，complete_terminal试图用rotation字段重写旧MEMmetadata而conflict。已修只在无capacity_rotation时归档原request；actual文件回归queued_capacity_journal...1passed0.16s，旧metadata bytes/report不变、下一MEMunavailable/pending。新完整原28884正在运行，先该新回归已pass，完整47终态尚未返回，不能记自动容量已验收完成。

Source只读平台补充 BrokerJob.memory_accounting：官方QueryInformationJobObject ExtendedLimitInformation(9)真实peakJobMemoryUsed/peakProcessMemoryUsed/JobMemoryLimit、BasicAccountingInformation(1)ActiveProcesses/flags，不改2GiB限制；公开SDK无current aggregate接口，aggregate_commit=null/unsupportedPublicSdk，不sumRSS、不用notification历史值伪current。held Process.memory GetProcessMemoryInfo PROCESS_MEMORY_COUNTERS_EX.PrivateUsage私人commit、WorkingSet/PeakWorkingSet/PeakPagefile分列，pre/post原PIDcreated核验。6772b8 actualself test1passed与ownedBrokerJob example peak929792/limit2147483648/active1+正常child/disarm通过，strictWindowspassed；不是实际Source压力结论，Source另行记录。

## 47任务容量批终态与接续只读核验

原28884随后terminal0、1passed、439.33s；此终态由原监督记录接续，未重复运行长fixture。现存 `.tmp/live-static/native-actual-capacity47-proof.json` 已只读复核47个operation，均为同一CON，最后 `Settled/report.state=verified/cleanup=complete/complete=true`，一次换代后runtimeTasks=19。foreign lane的规范JSON前后相等；B保留2次LoD加载、12条消息，A总LoD加载97。旧held-ALT失败与第28任务metadata冲突失败材料仍保留。

该proof的实际自建目标A为PID40236/creation134359071804366510，B为PID42056/creation134359071806996658；当前两个进程及Lua sidecar均未在进程列表发现。保护产物 `target/debug/deps/native_e2e-b75ecc363c735ccc.exe` SHA256 `6db93fa6afc1b3e00bd0d4922e1bd692a14aaeccde8f0c29d693bfee629fc722`，与proof模块hash一致；配对PDB SHA256 `5dabb1e7944ce9cda7fa26590cecdfcafec98d5dde9239bca131fe71537589a8`。另一个中断会话94361在接续代理不可恢复，当前无cargo/rustc/native/Lua匹配进程；该会话自身终态未知，不据进程消失推断测试通过。

此证据范围仍是正式Host/受管500文件、自建OS窗口/只读MEM_PRIVATE wire carrier、Lua5.1永久Loaded模型与光学fixture。没有证明Retail Lua ABI、真实WGC、多gamebuild或本地耗时下降50%，未向游戏发送输入。

## live bugs 兼容入口待验证修改

增加第19项handler `live bugs`，以Go native CON的固定Diagnostics observation生成Lua，默认count=80、上限100、预算5s；请求key必填、最多256bytes且拒绝控制字符。使用同一EXECUTE/结果持久/SETTLE生命周期，provider缺失或Snapshot字段缺失保留业务层unavailable/complete=false，不能混同传输失败。native CON的account不决定SavedVariables范围，与原Go CON处理一致；旧OP/BTP仍不转换日志或冒用新CON。

新增参数边界unit及独立 `formal_bugs_preserves_provider_coverage_and_uses_durable_settle` fixture源码：真实Diagnostics读取fixture provider，覆盖90KB完整stack、Unicode、缺字段、provider缺失/抛错、重复只读、相同key不同count冲突、后续任务之后历史result及disconnect。仅完成格式化，构建因共享磁盘清理暂停；本段修改的编译、strict lint、上述测试与Go build/vet均为 `not_run`。其余bind/hide/reset/reload fallback未被简单alias成native操作。

磁盘受控通道恢复后，owner=live-windows的精确参数unit原调用f16785 terminal0、1passed，证据 `.tmp/rust-disk/evidence/988d1f65-31fb-44e2-93f7-0cf5b85d9eff/receipt.json`。精确bugs E2E原25121 terminal1：编译成功、1failed/33.37s，证据 `.tmp/rust-disk/evidence/03300f5e-7bc7-477d-80c9-fad2e841f452/receipt.json`。失败为fixture Target结构构造期间仍持有 `owned.borrow()` 临时借用，同时 `peer.record()` 经carrier借用同一owned，触发RefCell already borrowed；尚未执行bugs业务，不记为通过。该批sourceTree=`1af774b29119def99540d0de84384181e56a9a906882912e18d0b5ac49e53797`，失败记录保留，后续修复另批验收。

## 新 Native P3 共享生命周期准备态（受编译冻结约束）

接续既有四槽 exact COMMIT、两槽 token 与 Root 独立 age-only 反例，不重复原封存测试。Root 当前独占 Source 原件/符号封存与正常 Retail 查询的编译和重负载窗口；本批只编辑 `protocol/native-lifecycle-prototype/` 与文档，没有修改 crates/Cargo/release/vendor，没有创建 crate wrapper、运行 Cargo/真实 Lua/OS fixture、游戏输入、commit/push/publish。

一份共享私有 `business.lua` 与 canonical `commands.lua` 接 RUN/RECEIVED/CANCEL/CLOSE；同一 `receiver.lua` 的两/四槽传输只通过真实 Lua STAGE/once-loader/final-key callback 进入它。隔离 Rust `driver.rs::advance` 为正常与 restore 共用；准备的 `host.rs` 把同一业务/故障矩阵分别接两/四槽，不由 host 直接 dispatch 业务。实际落盘顺序设计为 BODY fsync→immutable meta→RECEIVED intent fsync→uncertainty witness→exposure→final callback；副作用仅明确 true 才确认，false/nil/throw 与先副作用再 false/nil/throw 均不提前释放 private BODY。

本批隔离源码核对修正 stable completion identity（不再存可变全 state wire）、closed 后 exact duplicate CLOSE 只读观察、restore 缺 input witness 时禁止猜测未发送、过期四槽 pending 淘汰、临时 hardlink 回收及 32 intent 上限。保守 restore quarantine 仍可能阻止真实未发送请求自动继续，是明确的活性缺口，不记为恢复闭环。

详细断言与未写/实机专属项见 `docs/toolkit/rust-native-shared-lifecycle-matrix-2026-10-08.md`。已写场景包括完整 RUN/BODY/restore/RECEIVED/CLOSE、发布/close/clear false-nil-throw 前后边界、同编号只读、新编号 inspect 重试、timeout/cancel unknown 和 producer race、lost witness/result bytes、public tamper、4次上限/原5秒清理deadline、16项history与512KiB BODY边界。**全部仍为 not_run，包括语法/编译/严格检查**；没有以准备源码或老分项通过提升新 P3。

当前 `driver.rs` SHA256 `f3b491931433ea4bcdb0385ebbf06971e28fd436cc98e4f14faaf87759db2184`、`host.rs` `427a0ee9c718d52aa74762fc7937e4335925de2e5eb11829f3ddcb260253ca80`、`receiver.lua` `09443328cfb94eb10618097ae53a8f987e1f249e5502e09a4435a69669ba07c9`。仅源码身份，不是受测产物身份。Root 放行后才新增薄 wrapper 与受控 runner；再对准备的原型进行新上下文独立审计。P1 私钥/reflection/cold binding、真实跨暂停clock/B1、500 shared allocator/最大实例、OS carrier和实际游戏均未验证，协议与总Goal未完成。
