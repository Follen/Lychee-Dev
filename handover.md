# Rust 迁移：当前交接

更新时间：2026-10-21。此文件只保留当前状态；架构和合同见 [DESIGN](DESIGN.md)、[业务覆盖](docs/architecture/agent-task-business-coverage.md)、[领域迁移](docs/architecture/domain-navigation-migration.md)、[整体实现清单](docs/architecture/reconstruction-whole-plan-ledger.md)。原始失败和历史报告保持原身份，不重标为当前通过。

## 目标、来源与执行边界

- 沿用 `rust-refactor` / [草稿 PR #14](https://github.com/Follen/Lychee-Dev/pull/14)。已接受 [GitHub 交接](docs/handoff/2026-10-21/HANDOFF.md)和[统一验证队列](docs/handoff/2026-10-21/VALIDATION-QUEUE.md)，交接实现基线 `cfdefd46a11ec8dce470d296b424a3eaa1372b8b`；原交接包的 NOT_RUN 是冻结时事实，后续结果以本页为准。
- 顺序：Source/Data 业务质量不低于 `f811fc7f75fc206158cecb01dd0eb5d39e88b35f` → 完整实现和 correctness → 同任务 Rust 可控耗时 ≤ 固定 Go 的30% → 最终五循环。用户在2026-10-21指定后续基线为 Go v3.2.4 peeled `29bdbdaff5f064ec212ff2064380b1ccac652eda`，并明确保留Rust优化；[增量范围](docs/toolkit/rust-go-3.2.4-increment.md)主要为插件战斗态修复。旧3.2.3报告保留原身份。
- 当前产品为3.5.0 beta配置。当前Skill anchor `bd8196aa68ba1570652bb9540a01bbcdb4628638`、25文件tree `4eace74fd90ebc4cc12b4628bb58c6f74262ccb5`；只更新三个Go324入口声明，旧75cd及全部历史manifest/报告保留原身份。当前安装路径拒绝旧包冒充新树。Agent是正式Data入口，migration-oracle仅作旧业务对照，Release不启用。七产品、双Forever身份、Mailbox v1、未知输入不重放、精确数字/coverage/CAP合同不变。
- 用户要求全部隔离测试，不写入现有 `~/.agents`。Skill/npm安装仅在项目隔离目录；测试用户HOME/USERPROFILE/CODEX_HOME、AppData、temp和npm配置/cache均由当前owner持有。编译器及离线依赖缓存保持既有位置，不重新安装工具链。`runCargo`新增显式isolateUser选项，后续native队列启用；已核现有delivery/CLI Skill写入用例均显式临时target，未走真实用户发现。
- 本机主代理独占 native 队列和此文件，开发与固定提交测试副本分开；其余代理只在分配文件内工作。保留 main 的独立任务改动及 `.worktrees/rust-refactor`。无游戏、合并或发布授权。
- 全部 Rust 调用经官方 owned runner，600秒/组、默认stack、locked/offline/j1、no one-shot，默认20GiB构建cache/5GiB scratch。Data产品容量另为十进制5GB。用户已授权本任务后续同一固定副本target/debug/incremental的自动清理：无编译进程/占用、逐项核对清单，只删可重建incremental，保留其他cache/EXE/PDB/依赖/报告且上限不变。最近一批2,433,793,455字节/6,397文件已清理，10,287个保留cache文件及受保护报告前后hash一致，双lease已释放；当前cache16,918,930,915字节。两次先前获批清理及所有拒绝报告保留原记录。

## 已实现与实际验证

- 当前固定测试副本为 `284a576e5cfa9af1ce47f192221802779faa35e4`，tree `d1678976a5e3a477eacd5cc74d38fabb57943639`。全部9 crate默认及migration-oracle两套native check通过；后者首次因cache准入未启动，清理后以独立新stage通过，未覆盖旧失败。真实用户目录隔离已在owner记录核验。开发分支已保存bd8196的当前基线/Skill说明，本批其余Go324增量正在保存；这不是新树原生验证通过。
- CORE-FIXTURE-01：853原例839通过/14失败；14个迁移后锁序、CAS、recency/durable peak旧夹具已修，受影响130项在 `1764e19c8c` 逐名全部通过，产品未改。
- b6cf Core其余6个integration目标65项全部通过，完整native退出和资源回收已核。CLI当前207 active均有实际通过记录：bin184及supervisor6在b6cf通过；另外17个integration跨b6cf/f5dc通过，原5失败在f5dc修复后闭合；3 ignored单列。
- DATA-RUNTIME-01 / DATA-CORRELATION-01：执行流schema、eager布尔错误、公有ID流内缺失证明、安全等值关联键已修并推送；Engine165项在 `8819608054` 全部通过，新增多键MAX和CAST短路回归已通过。
- Data library当前1530项，1529 active、1 ignored。先前1528 active逐名对账均有真实pass，来源为1764/bc77/8819/f29c/e0c6；f5dc新增真实Session恢复回归通过，并重验受影响196项（Agent Session23、engine result4、engine source24、local_query145）全部通过。这是跨版本覆盖，不是当前整套单次通过。原600秒timeout及所有旧失败保留，nested child和输出混排只作独立核定。
- DATA-REMOTE-FIXTURE-01：最后4个remote夹具已修并实跑通过；准确使用durable peak、共享catalog序列化，取消后两库不存在且readonly返回record_missing。取消身份、未发布pin/CAP和私有容量边界保留。
- CLI-SOCKET-01：accepted socket恢复blocking后，原4项Windows真实supervisor测试全部通过。EOF半关闭、stdout背压、整epoch取消、私有入口拒绝已实跑。
- CAP-TRANSPORT-01：Data实际Session query/capture和Navigate自动CAP在stdout flush后observer失败的2项新回归通过，核唯一CAP、原body/manifest和query handle；全部Agent Session消费者22项及12条Navigate业务路径同组通过。b6cf CLI bin184项、supervisor6项全部通过，含真实BrokenPipe保留committed CAP、父进程终态统一失效标记及排队deadline。精确non-yield CPU poll及真实private receipt断线仍无实测。
- Data integration当前284 active均有实际通过记录：e0原275通过/9失败；b6cf通过DB2六项及hotfix一项，f5dc通过content全13项及Agent native parity2项。selected缺失body重建在正式Agent和migration-oracle两条路径均修复并实跑；坏body仍明确拒绝，alias/CAP/归属不变。Data全部ignored合计25，不计业务pass。
- Windows library188 active跨f5dc/284a均有通过记录；唯一主文件pin失败修后，284a destination八项及Core四个实际调用回归全部通过。两项suspended-child ignored在284a用本次编译helper显式通过，真实正常/取消/准备失败证明及全部进程、reader、句柄回收已核。11个integration目标中10个非空目标52项全部通过，另一个harness=false helper无业务case；integration2个ignored由其parent调用，单列不重复计数。
- Source完整38目标新库存为836 active、45 ignored、1 helper。library749 active在284a按完整名单分组全部通过，7 ignored单列。integration的47 active已通过，diff_prune四例在fixture Git worktree add因隔离TEMP长路径失败，未进入prune断言；36项oracle留专用入口。已为单次fixture Git add启用core.longpaths并推送d3e30db270，保留实际长路径、全部断言及隔离；原生复跑待做。
- Source两项native quota ignored已实际通过并回收子进程；临时汇总器误把Windows OsString当string导致外层报告失败，独立只读派生报告已核原结果且未改原报告。观察到接近预算的分配拒绝，不据此声称瞬时硬上限或唯一拒绝原因。外部Lua工具九例六过三败：goto旧断言误用公有Go扫描器，测试已改到native Lua5.1结构；两个refs用例需迁至真实Release CLI，仍待实跑。原有passed六例不重复。
- Emmy adapter全部19目标197 active在284a实际通过，0 ignored；binding-contracts integration五项实际通过，library按真实列表合法0-case，不计业务pass。Source专用参考端36项实际通过并生成4,137行完整参考，候选端在Cargo启动前被cache准入拦住；原失败报告和参考保留，准备清理已授权incremental后只续候选，不重新生成参考。
- Node原399项393通过/3失败/3权限skip；旧Skill差异清单和metadata布局3失败修后对应43项全部通过。后继Source exporter/corpus工具5套84通过/1权限skip；本批真实Skill入口及用户环境隔离的4套Node68项全部通过。重叠套件不累加为独立覆盖，mock/静态检查不代替Windows业务。

## 当前修正与负责人

| ID | 状态与边界 | 负责人 / 下一步 |
| --- | --- | --- |
| GO324-BASELINE-01 | 新固定Go324，移入addon两文件和combat夹具，保留Rust优化/3.5.0。formal工具118项Node、legacy消费者9项和旧322冻结输入4项通过；CLI当前静态门通过、41项Node通过；paired470通过/4权限skip；Skill新身份121通过/1权限skip。历史清单不重写，真实Lua/bridge/npm/Go构建/paired均待执行 | 三worker已交回，主代理已核本批差异，统一保存；native待固定副本切换 |
| SOURCE-EXTERNAL-FIXTURE-01 | 真实luac九例中goto结构断言已修；refs两个测试改用Release CLI并保留未调用Caller负对照。Bulk通过正式selected EntryRequest作假设执行，核130操作/两批/128原调用/实际观察及CAP，incoming负例保留空观察 | 两worker已交回，主代理已核生产入口语义；实际复跑待匹配Release |
| CLI-TERMINAL-01 / CLI-FIXTURE-01 | b6cf bin184及supervisor6全通过，原19个bin旧合同/种子问题及BrokenPipe终态已闭合 | 主代理已实跑 |
| DATA-INTEGRATION-01 / DATA-HOTFIX-FIXTURE-01 | 原9失败均已修复并实跑通过，整个284 active跨版本均有通过记录 | 主代理已实跑；旧报告不改写 |
| DATA-LOCAL-ORDER-01 | local_query及正式Agent sql_reader延后一次配置provenance发布；f5dc受影响196 library、content13及Agent parity2全通过，含真实Session缺体恢复/等长坏体拒绝 | 已保存、推送、验证 |
| CLI-PUBLIC-FIXTURE-01 | prune当前71目录、normal exit分组help/clap结构、source inspect空值文案夹具已更新；对应10项在f5dc全通过，原退出码/无home副作用/完整输出保留 | 已保存、推送、验证 |
| DATA-NUMERIC-01 | Arrow NullArray结果补ScalarValue::Null，原scalar边界及CLI numeric3在f5dc通过，涵盖public page/JSON/CSV/CAP；精确整数和负零保留 | 已保存、推送、验证 |
| WINDOWS-PIN-01 | InspectPinned增加FILE_READ_DATA并保留共享读写；284a destination八项全通过，涵盖已有/创建、写入、持有期间拒绝rename/delete和关闭后允许；Core四个主库pin实际调用回归也全部通过 | 已保存、推送、验证 |
| SOURCE-PRUNE-FIXTURE-01 | diff_prune四例因fixture Git worktree add长管理路径失败；仅该调用加core.longpaths=true，不写仓库配置、不修改生产Git或prune逻辑 | core_evidence已交回，主代理已核四行diff；六例实际复跑待做 |
| BUILD-RUNNER-01 | Release、Source full-corpus/fresh场景统一600秒，fresh exporter默认no-oneShot和原20/5GiB；isolateUser令用户目录归owner scratch，本批补齐buildRust透传、Release CLI及三exporter准备显式启用。原prepare身份规则保留；19项对应Node/mock通过。实际Release/exporter未跑 | core_metadata/core_evidence已交回，主代理已核 |
| SOURCE-INSTALLED-01 | npm smoke在隔离prefix执行真实global布局安装；--installed运行固定25文件Skill launcher，使用npm生成的prefix入口。旧local安装报告仍直接进包并标not_run，staged模式保留；未造shim/改Skill。Node及隔离回归通过，实际npm/Source业务未跑 | 主代理整合完成 |
| ACCEPTANCE-ISOLATION-01 | formal Lua/bridge、五个协议prototype及Source fixed oracle入口补齐owner用户环境；Source外层600秒、Cargo no-oneShot。Live工具Node77通过；Source runner/wrapper27通过，历史checker4失败另列 | core_evidence/data_engine已交回，主代理已核15文件；实际专用入口待跑 |
| SOURCE-HISTORY-01 | historical checker的原ac8 runner/inputs/完整harness统一从contractRoot认证，测试从原Git blobs取冻结输入并验证篡改拒绝。55项Node通过、1 symlink权限skip；原pin/owner/失败/4137行/两差异不变 | data_engine已交回，主代理已核diff；这是历史checker修复，不是fresh Source通过 |
| PAIRED-ISOLATION-01 | Agent paired的准备Cargo、controller及调用环境使用已有owner目录；Source paired的outer/准备/Git和样本分别使用已有outer/pair目录。业务home、20对、固定输入、测量及限额不变；Agent15与Source41项Node通过 | data_engine/core_evidence已交回，主代理已核九文件；真实paired仍未执行 |
| COVERAGE-01 | 9 crate共150个lib/bin/test声明；原漏26目标中adapter19和binding2已完成，delivery5目标待执行。binding lib合法0-case、两个harness=false helper不计业务pass；ignored单列 | 主代理 |

## 下一步

1. 本批Go324增量、当前身份和fixture修正保存到同一分支。固定副本仍284a，adapter197和binding5已通过；Source参考端完成，候选端因cache未启动。core_evidence准备原参考续跑；data_engine准备同目录incremental清单，主代理按既有授权核对后清理及执行。Windows suspended和Source quota各两例已实际通过；六项外部Lua工具已通过，三项失败按上表修复。
2. 当前native队列结束后切换已保存修正，复跑diff_prune六例及受影响goto；完成Go324 formal Lua和Live/Delivery。284a两配置check、Windows pin八项及Core四项均已通过，无新影响的CLI/Data/Core/Source库已通过组不重复运行。
3. 完成Source、Live和补漏26目标；Source十个native_oracle目标仅通过专用参考生成器，五个protocol目标用各自wrapper。为其余refs和formal bridge准备当前生产Release及Lua admission，再补这些依赖Release的实际用例。真实数据/权限ignored单列。自有窗口/子进程测试不等于真实游戏测试。
4. 实际npm安装匹配25文件Skill，执行no-home发现、installed Source及Agent Data。三项exporter/importer显式准备，不在测量内隐式构建；旧SQL私有metadata fixture不能拿来测新Agent。
5. 再执行现有owned paired协议：完整任务、20对、不扣helper成本，CSV仅correctness；历史单pair、诊断profile和旧产物均不是当前性能验收。当前无≤0.30或全产品矩阵通过结论。
6. 最后五循环依序为架构/业务/Skill、覆盖有效性与冗余、死代码、联合全仓、Rust原生度。需要真实配合的项目始终留在最后，不从历史计划推导操作授权。

## 11. 最后执行：需要实际配合的测试清单

**本节必须始终是handover的最后一节。** 只有前述可独立完成的实现、离线差分、自动Windows测试、封存候选和可自动性能检查就绪后再约实际配合。当前全部 `not_run`；以下“结果”故意留空待真实执行填写。每次先固定candidate commit/exe/package/addon hash、产品/build、所选安装/PID/创建时间/HWND及授权操作；缺客户端或用户未就绪留缺口，不缩范围。普通Windows API单元/自有子进程/合成窗口CI不在本节。

| ID | 配合/前提 | 步骤 | 预期证据 | 结果（实测后填） |
| --- | --- | --- | --- | --- |
| H01 | 用户指定真实安装和角色；七产品逐一可用 | Retail、Classic、Classic Era、Titan、Forever Asia Beta、Forever CN Beta、Anniversary独立identify/connect/execute/report/cleanup/disconnect | 同candidate各产品身份、完整精确receipt和清理；两beta不按同build合并；不能以Source/Data成功代替Live | |
| H02 | 选定未安装或旧200槽真实客户端，用户可手动reload/重启 | 首装/升级后doctor；无可验证INPUT时手动reload；未发现500槽时重启；原CON resume | 未盲发输入、原目标/CON不变，旧200仅升级识别，500发现/启用/加载事实分别记录 | |
| H03 | 用户允许安全编辑框/聊天焦点和键位冲突场景 | 测两实际绑定、已有绑定保留、聊天/普通EditBox透传、secure/protected拒绝、UIParent隐藏、最小化和全部键盘VK按住/释放、500ms静默中再次活动、活动与已开始chord重叠、仅释放本attempt按键 | 不改文字/光标/焦点，不抢前台，不Esc/Enter；恢复handler，500ms后新鲜INPUT、原deadline不延长、submitted/uncertain不重放及精确receipt各自证据 | |
| H04 | 同安装双实例、跨build并行及混合多实例就绪 | 分别连接/执行/关闭，交错共享槽发布和选定实例reload/doctor --fix；至少Retail双实例，其他产品依声明范围补齐 | 独立身份/owner/fence/CON，短锁预约不串线，其他实例不被授权/升级结果冒认 | |
| H05 | 用户允许真实容量与reload/relogin验证 | 连续运行触发500槽完整操作容量边界，先日志后reload/重绑同请求；reload/relogin前后验证 | 旧槽不复用、预算不延长、目标固定、exact报告与清理，运行时退役明确 | |
| H06 | 用户同意可控故障与恢复窗口 | bind无回执关闭、submitted/uncertain中断、发布替换、release无回执5秒cleanup reload、退出/重开CLI、opaque与observation区别 | 无未知业务重发；原request/ticket/期限/attempt保留；报告可用和cleanup分离，退役后才runtime_destroyed | |
| H07 | 用户同意在选定真实角色设置受管槽禁用并恢复 | 按角色GUID启用、复查并保存；doctor只读核slot_activation；有新鲜INPUT时按原CON reload，模拟多次replacement与可控保存失败 | 不LoadAddOn、不消费槽、不动foreign元数据；slotReloadRequired未清不发槽动作，reload自身可执行；原deadline/交易/unknown不重放，失败不报ready；记录恢复步骤 | |
| H08 | 同机同数据Go/Rust正式候选与用户允许的只读/有界Live采样 | 按B06独立cold/warm交替重复，真实游戏等待分段；保持同业务/输入/校验 | 原始计时/输出等价、excluded阶段与总wall时间，未达0.30照实失败，不影响前面可自动工作 | |
| H09 | 若既有“十分钟空格”人工活动仍由用户确认需要 | 先确认当时任务目的和安全场景，再由用户实际配合；不把历史计划当持续输入授权 | 人工行为与工具输入分开标记，明确时长/目标/结果，不冒充CLI输入或协议通过 | |
