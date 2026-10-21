# Rust 迁移交接：唯一当前事实来源

## 当前接手与验证恢复（2026-10-21）

- 后续开发与统一验证已由本机主代理接手；沿用 `rust-refactor` / [草稿 PR #14](https://github.com/Follen/Lychee-Dev/pull/14)。[GitHub 交接入口](docs/handoff/2026-10-21/HANDOFF.md)来自 `0dfcebd25238239887f4dfb5c209a5bf4c07c392`，9 个文件及清单中的 8 项内容哈希已核验。产品及原测试副本继续固定 `cfdefd46a11ec8dce470d296b424a3eaa1372b8b`；保留其他任务的 main 改动和原测试证据。
- 当前文件负责人为主代理，范围为交接维护脚本、配套哈希/说明和本交接。MAINT-ENCODING-01 已修：PowerShell 的 JSON 为 UTF-8 BOM、重定向 stderr 为 UTF-16LE BOM；仅在内存解码，保留原始字节、全部身份/大小/哈希检查和清理范围。两次原生失败均发生在创建 blocked 记录与删除之前；修复后实际封存通过。
- 已验证并封存 112 manifest / 2135 引用及原六份配额拒绝文件；只清理获批的 `target/debug/incremental`，移除 6,819,246,069 字节。2534 项受保护文件和 8663 个其他缓存文件前后哈希一致，最终 receipt 为 `retired`、双 lease 已释放；独立检查无相关进程/lease，缓存为 12,899,139,776 字节，20 GiB 上限未改。原 wrapper exit 1、Cargo 未启动且无实际 owner/receipt 的历史事实保持。
- 本批产品业务用例实际执行仍为 0；既有两种 check、CI、Data/CLI 名单及 Core all-list 不等于业务通过。下一步在同一固定测试副本新建续跑记录，补 Core ignored-list，再按已列出的目标逐组执行 correctness；不覆盖原拒绝记录、不派发旧队列、不提前运行性能或游戏验收。

## 1. 当前目标和执行顺序

当前执行已批准的 [完整架构计划](DESIGN.md#authorized-architecture-implementation-plan--2026-10-19)，不是开始新一轮正式审计。顺序是：**Source/Data 业务质量不低于固定 f811 → 全部实现门禁 → 同任务 Rust 可控耗时 ≤ Go 的 30% → 五项循环审计**。必要开发独审、差分和回归不是最终五循环完成证据。

- 质量基线 `f811fc7f75fc206158cecb01dd0eb5d39e88b35f`：实际 check 与71项针对性测试通过，16新增逐名一次；不是全仓通过。准确性、完整性、排序/可读性、来源和已提交证据不能退步，已认可的 Rust 语义优势保留。
- Go 业务/性能参考固定 v3.2.3 peeled commit `3766f9cacb52e93a803a626187e2a1d54a445468`。既有增强单独有任务级 oracle，不机械回退正确结果以求原始字节全等。
- 产品仍为3.5.0 beta配置；无发布、合并或游戏操作授权。Skill 七产品定义和双 Forever 分离、Mailbox v1、未知结果不重放、精确数字/partial/证据合同不变。Emmy使用固定未修改上游；自有业务Rust不新增unsafe，必要Windows边界独立审查。
- 当前仓库只在 `rust-refactor` / [草稿PR #14](https://github.com/Follen/Lychee-Dev/pull/14) 分批保存。用户本机只负责固定SHA测试及必要协调，由唯一父队列派发；运行中的身份不会随新head改变。

## 首组合静态与身份检查（native 尚未执行）

本次新 Skill 固定身份引用实际产品 commit `3ae03c6bd7f13260629e2f92218f8d9e262b1ebb` 的25文件 tree `e1c9f992da62a048d24c62843636230783ff6928`，仅 SKILL.md、commands.md、data-investigation.md 相对6187改变；6187及0002历史身份和已封报告保留，不能重标为当前产品。新增固定清单验证、原始6187回放、同长篡改及伪SHA负例。

Node：Skill全套110通过；CLI八组107通过、1非Windows跳过；Data四组69通过；Agent合同及静态Skill组合33通过（与前组有重叠，不累加）。rustfmt仅80个变更Rust文件语法解析通过；没有运行Cargo或原生测试。这些只说明静态合同/证据工具回归，不代表新Agent业务、硬超时或资源验收通过。首Windows队列必须固定最终发布SHA，先all-targets及Release编译，再真实private Data metadata fixture、Session/Engine、supervisor进程和迁移回归；不开始性能或盲评。

## Agent 组合修正 r2：仍待实际编译与验收

首片发现DF单次native poll可长时间不yield，query内协作取消及Job内存限额不足以保证deadline。修正采用caller-owned supervisor与单个可复用Engine子进程：已有suspended guarded spawn先关联Job；私有入口核实际父进程PID/creation/同binary并建立自身Job；专用loopback channel以临时BCrypt token及held child PID认证完成receipt，只在公共输出flush后发送。父进程独立监测排队与输出背压期限，必要时终止原Job并检查reap/job-empty/pipe回收；不新增unsafe或游戏通信。输入EOF是半关闭，先排空已提交任务；显式close同样排空；取消outstanding请求会终止整个session，全部旧句柄失效，不自动重启或重放。此处仍须Windows嵌套Job、硬取消/非yield执行、背压和异常清理实际验证，不能因代码存在称漏洞已验收关闭。

当前首slice仅公开typed plan任务；原`data sql`尚未迁移，统一消费者主线仍未完成。Query provenance改为持有原预算guard的OwnedProvenance；DatasetInfo/ResultPage也直接拥有guard到serialization/flush，禁止to_value克隆后提前退款。核心共享lifetime释放由独立作者修正。历史Skill/CLI custody与新增身份分开，旧证据不重标。

当前组合还包含quota-backed共享blob publisher、显式recency冷淘汰和严格closed-body恢复，以及definition/listfile/Wago/decoded等Data可重建metadata迁入独立cache-v1数据库。它们只证明已接代码，未证明全Data 5GB业务闭合；durable stage/spool、CAP完整归属与全部旧消费者退役仍在后继主线。Cargo.lock仅新增Data与CLI测试fixture到既有固定rusqlite的两条依赖边，不漂移包版本；正式DataFusion/Arrow pool已默认开启。

新增agent_native_parity以生产DataCacheMetadata播种同一认证CASC/DBD/WDC，比较typed排序/宽整数/数组与迁移oracle并测试release。其库级协作取消与正式transport的整session硬取消明确不同。旧Node/Skill/performance归档保持原schema；新性能/盲试前必须另封新metadata exporter/loader/seed适配，不可将旧toolkit.sqlite定义种子误用于新产品。首次Windows队列先编译及真实业务定向验证，性能后置；尚无本组合Rust/Release/Windows运行通过结论。

## 1a. 全架构恢复核对（2026-10-20；非完成声明）

依据原 `c44c8d735ebb03a0ec76381578a59db52b9b557f:DESIGN.md` 六项主线，对照真实 `eeeb0a14c6c91cfc5cc135a6a2fc6b45d6669908` blob，而非本地陈旧 HEAD：

- Home/配置：`core/workspace/data_resources.rs`、`data_disk/**` 已有正式配置和准入；旧writer全口径、去重/冷淘汰/保护闭环仍须完成，不能称全Data 5GB已完成。
- Data：`generation/**` 已提供不可变Arrow派生与实际SQL消费；`local_query.rs` 仍走旧Program，`engine.rs`及`relational/engine_compat.rs`仍仅test+feature资格。正式统一执行核、任务API、跨调用同进程session、owner-backed结果句柄在此基线未实现；CAP与CASC query session不能冒充它们。
- Source：`index/persistence_wire.rs` 与二进制Facts/Index消费者已实装；已有多场景完整证据保留，不无谓重写或重新标为未做。新增完整质量/性能验收与已执行历史场景分开。
- Live：`connection`的私有SessionOwner与原journal/platform归属已实装；Mailbox v1与未知输入不重放不变。本轮Data worker不改变Live协议、不增加Live线程或操作游戏。
- CLI/Skill：已有真实describe/help与精简指南及分批试用证据；新的Agent任务/handle发现和正确调用路径仍需正式接线，不能只新增library类型或文档示例。
- 集成：Source/Data完整业务质量、全实现、性能及最终五循环均未闭。当前先完成新架构与业务结果对齐或更优，暂停旧SQL零碎优化优先级；历史失败/测量全部保留。

当前云端实施文件边界：统一内核作者负责engine及数值桥；磁盘作者负责quota/generation retirement与既有writer；Agent集成作者负责session/CLI/Data导出及依赖、Skill和权威文档。Agent结果页在序列化结束前必须持有原结果与预算guard。Windows Job能力与逻辑账本分开报告，不将Node静态或源码审阅当作Rust/Windows运行通过。

## 2. 当前实现批次与验证状态

| 范围 | 已保存/已实现 | 当前证据和剩余门禁 |
| --- | --- | --- |
| Core/Data/Source基础 | 8c9：配置、Data派生目录/准入、真实Arrow cache prepare、Source Postcard Facts | 070f实际check通过；原copy/发布receipt路径/generation取消四个修复目标均通过。19e check与67项定向回归通过，070f全部23失败逐项闭合；完整实现门禁仍未闭 |
| Live资源归属 | 5c60：私有 SessionOwner，native先于journal关闭与claim退役 | 8bfe connection61/durable36通过，六新增逐名一次；无新线程/daemon，真实客户端未验 |
| 编译修复 | 6ea恢复原SYM SHA writer；2b400显式Data结果类型；66d空slice断言类型 | 8c9、6ea、2b400、66d原编译失败保留。8bfe实际check通过，config14通过；disk26为24通过2失败（copy注入阶段不确定、Windows路径文本）。其余11组续跑392通过/1失败/1 ignore；新增失败为generation取消测试期待operation.cancelled、实际command.cancelled，后续断言未执行，单独修正和复验 |
| Skill精简 | 308efa 指南与 ac48 逐调用试用入口；固定86f封存CLI | 实际8臂旧/新独立盲评为6通过、2 partial，两版评分相同：未知参数纠错、版本差异与真实Source inspect、超能力写入拒绝均完成；原任务04走点查缺key，未覆盖SQL空partial/CAP，保持partial。另开2臂明确SQL补验均通过：空rows、complete=false/partial=true且不能断言不存在，CAP保留、fixture不变。合计分批事实不重标原8臂，不宣称新版整体更优。Windows入口Node107通过/1权限skip及脚本首验另计；历史MODEL_ONLY不计真实盲评，Live/实机仍待 |
| Source正式二进制证据 | 740a：已独审10文件工具片，完整Facts exporter与六场景适配 | 七Node套件382/382通过；新Rust exporter/Windows/性能待验证。历史v11读取与新v13显式分开 |
| DataFusion适配资格 | 独立可选资格片已保存并静态独审，固定54.0.0/Arrow58.3.0 | 278默认/feature check实际通过，14数值/计划回归全部通过；默认依赖图不引入DF，真实provider/任务资源适配仍待，生产执行器未切换，不能宣称性能通过 |
| Data磁盘后继 | 共享blob准入、精确decoded spool与CAP独立引用保护已整合28代码路径 | 070f的56新增为55通过、1个uppercase历史CAP删除失败；该失败在19e实际通过，新增两项归属/签名回归亦通过；只覆盖已接入归属，历史blob/CAP/SQLite等全口径和冷缓存淘汰仍未闭，不宣称5GB已全闭合 |
| Source主Index二进制 | 已整合24文件r2与8文件正式Node适配，采用共享typed模型/Postcard，公共JSON不变 | 070f的25 active新增逐名一次通过；Source全lib699通过/21旧fixture失败/6 ignored；19e validate32全过并逐项闭合这21失败，inspect15与flow4原通过，正式luac真实ignored用例另计1通过。七Node357通过/0失败/4权限skip；完整语料导出容量与性能仍待 |

已完成的DBCD四项修复保留：WDC5 palette物理顺序、多词metadata、官方BUILD预设、非SQL实际读取限制传播。c571与9004分批合计113 distinct通过、3原ignore；旧红例及四个admission测试失败保留，不能将小fixture称作真实>256MiB表验证。产品定义未改变，完整产品矩阵仍待。

Source 后继借用 Symbol 投影片已实现并完成源码独审：summary 重建与完整查询复用同一四来源映射，完整 Index 字节、SYM 身份及查询 owned 结果保持；summary 不再保留全仓第二份 owned Symbol 字符串。44a 实际 all-targets check 通过，index 155通过/1 exporter忽略、inspect15与flow4通过，合174通过/0失败/1忽略，10项新增各执行一次；44a匹配产物六场景完整Facts/summary/custody及finalIntegrity均通过；性能见第4节。精确完整组合树的云端九套 Node 382/382 通过；首轮因副本缺少历史 Git oracle 为380通过/2失败，补齐既有只读历史后完成复验。此片不改变 warm 读取；ID/hash工作移入 IndexSerialize，不能把 Symbols 桶下降算作全部收益，44a已比较匹配产物总wall，正式20对仍待。128MiB输出容量与原owned预算保持，单个临时ID与输出buffer同时存活；没有RSS改善实测。

普通 static SQL 的已存在精确 generation 消费片已保存于 e964，并与 b47 一起完成两项实际 check；完整固定队列270 matched为265通过、2失败、3原忽略，63新增为61通过、2失败。两个失败在测试目标断言前：inventory用例在持有objects保护句柄时重命名直接子目录被Windows共享冲突拒绝；SQL关闭故障用例未引用保留词 First，先被语法拒绝。最小测试修正保留原所有生命周期/CAP断言，当前修后尚未实跑，原失败不重标。消费片复用原 IPC 校验，保留行/数组/精确数字语义，认证后每 cursor 最多保留一批；短 quota 后转移同一受控文件和祖先句柄，所有 reader 显式关闭后才结束源与请求。producer 私有 schema 表示上限已知不适用时，明确走原 WDC；其余仅在完整认证 manifest 与当前来源匹配且对象原 open 确证 NotFound 时，明确记为未命中并走原 WDC，不重建 generation，常规 CAP 保持原流程；损坏、权限、预算、取消和关闭失败不回退。普通 SQL 不自动物化，Effective/meta 保原路径；这不是 DataFusion 默认切换或全 Data 5GB 闭合。

Core 库存目录复用与本地 decoded 缺体重建已保存于 b47；实际两check通过，相关固定队列结果及两个待复验测试修正见上：前者只复用本次 quota 已持有的精确 Directory，保留每次成功的普通新 body 流式发布的四次全库存扫描和四处 sync、逐文件认证与所有实际 owner 的 checked close；13项新回归覆盖真实构造/关闭观察点、未知子目录、嵌套发布后新库存和错误聚合。目录计数回归已实际执行，其中上述目录rename用例失败；其余通过不能替代该后续断言，仍无此片匹配wall收益。后者仅在已认证本地 alias 指向原生缺体时使用既有精确重建，EOF 再核 alias 身份，保留原 pair/CAP/keep；同长坏体等仍硬拒。本地9项回归已实际通过，包含公开 asset 的 Root/selected 重建与损坏拒绝，以及 EOF 身份、预算、取消和上游错误；注入错误不等于真实 OS 故障实测。两片不改变产品定义或 SQL 的缺 generation 回退合同。六源码文件的最终组合树已重跑既有三套 Node：52通过、0失败、11非Windows平台skip；这是验证工具检查；Rust以之后b47实际逐名队列为准。

新显式 `cache prune --data-derived` 片已静态独审，Rust尚未编译/运行：仅退役当前typed generation与无剩余引用的受管IPC，默认普通prune保持；未知/损坏manifest在首删前拒绝，原handle完整认证和祖先保护、active pin、共享引用及CAP/keep独立保留。每组仅同时持body和当前manifest删除句柄；部分删除/取消/close失败返回实际完成事实，规范orphan保留计量并可重试回收。target是该范围剩余逻辑字节，max-objects是进入退役尝试的独特对象组（纯busy不计），确认删除body数另报；无last-used/LRU，按SHA稳定选择。23项新Rust回归未跑。自动容量触发、共享decoded归属释放及全Data5GB仍开放。

Source warm诊断仅增加profile v2的4个互斥子段：实际read、SHA、typed decode、finish_file；都包含在cached_index父桶，open/扩容等仍属父残余，finish_file含终止检查与错误合并，不能称纯OS close。原业务与错误顺序不改，历史v1/31项继续严格读取；每块计时有自身开销。作者与独审各九套Node422通过，Rust两新增/两适配及真实v2计数尚未执行。

CLI 静态 oracle 修复（2026-10-20）：历史适配器单测固定读取并认证 c508b10 的原始 contract.rs，不再把当前产品变化误判为单测自身损坏；实际历史 3.2.2 runner 仍退出1，原始差异和未验证提取保留。新的独立固定 Go3.2.3 门禁认证85项完整命令、116项参数arity、7个完整产品profile和55份addon，仅接受已独立规定的精确CLI扩展/措辞及两个完整release字段文件差异；raw exact仍FAIL，approved static为PASS。最终f0d产品字节输入不变，云端八套Node为106通过/0失败/1非Windows跳过（107项），四代fixture认证通过；这不是Rust/Windows/runtime或性能验收。CI保留旧报告并阻断旧提取错误，新3.2.3步骤作为独立阻断门禁；当前固定f0d本机测试队列不变。说明及复验命令见[3.2.3静态门禁](tests/rust-parity/README-3.2.3.md)。

退役 uppercase 最小产品修复（2026-10-20）：固定 f0d 的实际 native 队列出现历史 uppercase source BlobRef 被拒；根因是 retirement::blob 误用 lowercase-only selection::valid_digest。本批仅该路径改为精确64字节 ASCII hex（大小写均可），保正bytes、严格serde形状、optional合同；identity/derived hash与definition耦合不放宽。两份产品/测试文件已经独立源码审查、rustfmt解析及patch检查；云端缺少固定Emmy源码，编译/测试NOT_RUN，不能称失败已闭合。新增1项严格边界测试并扩展原uppercase测试，静态generation 41→42、retirement 5→6；须以固定新SHA实际libtest名单为准，顺序复验default/DF all-targets、generation全组，再完成原固定队列剩余项；旧失败和原已通过结果保留，不合并冒充新版本全过。此前CLI oracle-only提交5fbbb7e完整保留。

键盘历史源码custody后继（2026-10-20）：5fbbb7e的正式Windows CLI八套107/107通过、当前Go3.2.3静态门禁通过；同run整体失败仅为键盘retainedProduction仍把后续所有权优化当作未变的历史文件（addon组28通过/1失败），原失败保留。本批仅修复两份Node验证源码：原始map与increment字节不改，分别认证原历史Git对象和固定三路径后继版本；observations以完整字节逆变换证明仅move替代clone，durable/journal保精确固定源码hash，并独立检查控制、checksum、sync/close、限额、回放及借用边界与负向mutation。五套addon相关云端32/32通过，CLI回归106通过/0失败/1平台skip；不是Rust执行、Windows hook或游戏验收。产品源码未变，7bd的Data大小写修复与固定本机验证队列保持，新提交CI待实际结果。

## 2a. 2026-10-20 容量准入与 generation 性能工具后继

先区分已经实测的身份：`7bd4216c28b45761f27239e1d74536af71b11a9b` 的 default/DF 两项 all-targets check 和 generation 42项已实际通过，原 uppercase 失败按该版本闭合；`384f130383cff29b86fcb26cfcf426e8c3d3ce90` 的[CI](https://github.com/Follen/Lychee-Dev/actions/runs/39349042670)通过，CLI107/107、addon32/32，native/performance步骤跳过。这些结果不属于本次新产品；原失败保留，当前7bd release/性能队列不切换、不重派。

本批12份Core/Data源码/测试加入typed cache-prepare miss的容量触发派生退役：同一quota owner贯穿盘点、严格manifest投影、精确退役、重盘点、最终峰值准入及发布。保留active pins、原handle SHA/EOF与祖先保护、共享引用组、checked close、CAP/keep和源数据；未知manifest首删前拒绝，未知文件保留计量。已超额、单次峰值或受保护占用使目标不可能时不删；部分回收后拒绝/取消及后续producer或清理失败保留实际完成事实与一次转移的输出预算。不声称LRU，普通显式prune合同不变。

12项新Core admission与3项真实Data prepare/error-path测试均为NOT_RUN；独立源码审查、12文件rustfmt与patch字节验证通过。最终云端Windows-target check因libsqlite3-sys找不到MSVC `lib.exe` 在Core/Data类型检查前阻断，不能称编译通过。新版本须由既有父队列后续固定SHA执行default/DF all-targets、data_disk全组、generation全组、remote::admission与CLI data_cache_prune_public，并逐名确认15新增及原忽略；本批不自行派发。

独立7文件generation SQL性能工具调用真实prepare/普通SQL，以完整16,384行、原CAP/manifest/IPC、精确数值及进程证据核验；一次诊断严格一对七次CLI，正式门禁为另一次preflight加20对，不能混用。最终组合树重跑generation-contract、generation-evidence、data-runner、data-csv-diagnostics、data-resume与runner六套Node：254通过、0失败、0跳过，包含最终wrapper parse-under-catch字节，wrapper语法检查亦通过；这些不代替真实Windows wrapper执行，native及性能均NOT_RUN。初次测试产品必须固定7bd（sourceTree `cbad32ff06cac1fbcd01cbba5dd6dc5a8028a70e4a8741cf2a44a1266cdeeadd`），runner另固定本次提交。该验证器明确要求prepare的automaticEvictionEnabled=false，因此不适用于本批新增自动退役产品；不得放松断言或冒称最新产品性能已验。以后支持新策略须显式合同、独审及回归后另行准入。

自动策略仅覆盖typed写入准入。私有state/data SQLite主库/journal/residual已有计量，不能误称全部SQLite未接入；历史未登记/共享Data blob、历史共享toolkit数据库、shared decoded归属释放、legacy staging/spill/export、typed准入前共享发布的自动回收及完整全Data5GB验收仍开放。固定Go323、≤0.30门禁、原资源限制和最终五循环顺序不变。

## 2b. 2026-10-20 Source warm decode 窄优化候选

固定7bd匹配产物的六场景完整Facts、Index、summary、custody及finalIntegrity全部通过。Go/Rust毫秒分别为cold 8395.3783/3507.9365、warm 577.8278/288.8792、change 2798.1872/823.718、insert 2784.275/800.2367、delete 2738.0123/814.2435、rename 2738.8422/811.0872。四增量单pair比率低于0.30，cold/warm仍未达线；不是20对正式验收。该次warm profile v2实际typed decode为121642µs、read 12065µs、SHA 20687µs，父cached_index为168301µs；子段属于父桶，不能相加为额外成本，计时开销亦不能当收益。

本次基于7786只修改Source decoder及其测试：每个序列保留一个上次成功验证路径的借用slice，仅完整不可变UTF-8字节相等时复用路径验证；symbol kind接受集合与计数复用同一分类。原完整SHA、逐行解码、控制检查、所有字段/顺序、错误和分配预算保持。无全局缓存或新增owned路径。四项新回归覆盖完整字段/顺序与混合kind、首个及变化坏路径、重复路径下kind/count拒绝、full/summary深层取消。独立源码审查通过；官方云端入口返回owned.windows_required，Rust编译和这四项执行均NOT_RUN。最终组合树九套Node422/422通过、0失败/skip；这不能替代Rust执行或证明优化收益。

后续由既有唯一父队列固定最终SHA执行Source all-targets及lib，逐名确认上述四项与原ignored库存；再以匹配产物完整六场景核Facts/Index/summary/custody/finalIntegrity及warm总wall/typed decode，补低重复/唯一路径语料后才可声称一般收益。新候选性能NOT_MEASURED；不增加嵌套计时。7786容量15新增仍NOT_RUN；当前Data性能观察仍固定7786 runner / 7bd产品，严格automaticEvictionEnabled=false，不切换或重派进行中的队列。无原始本机日志入库，PR保持draft。

## 2c. 2026-10-20 generation prepare 公共timeout合同修复

实际固定7786 runner / 7bd产品的一对观察在5/7次CLI后失败：prepare产品exit0，但公共render_with_context额外输出timeoutSeconds，旧工具期望遗漏该字段并拒绝。其余两次Rust SQL和finalIntegrity未执行，没有完整有效任务比率，所有任务性能均未验收；不得将已退出/回收owner或已完成样本算通过。原失败及封存证据保留，7bd产品和既有artifact不变。

本次仅两份工具/测试源码把完整context合同改为精确numeric timeoutSeconds=300；snapshot/coverage及未知字段拒绝保持，未忽略字段或放宽超时。该值来自固定7bd CLI renderer与默认prepare超时，当前harness没有覆盖参数。11项新增覆盖真实公共形状及缺失、错误类型/数值、未知字段和缺snapshot/coverage。独审通过；在05660完整Source后继树上重跑六套Node265/265、0失败/0跳过（含focused160），只证明工具合同测试。源码复查未发现其他具体形状不匹配，但未取得本机raw stdout，因此不是原始输出重放。

该工具修复的native重试和性能仍NOT_RUN。保持正在运行的05660固定native组合回归，不切换、不重派；后续仅由既有父队列准入新runner配原7bd产品、原认证fixture和全新owner/evidence的一对--observation。先完成七次样本和全部完整性核验，再讨论独立20对正式门禁，本次不运行20对。05660 Source优化、7786容量实现与各自待验证事实完整保留。

## 2d. 2026-10-20 当前实测收口与 generation 阶段诊断

本节更新2a–2c的待验时间点，原失败和原版结果不改写。05660固定组合default/DF两项check已通过，定向native合计948通过、6原ignored；容量15项与Source4项新增共19项逐名执行一次。不是全仓或性能验收。原05660匹配release/Source完整六场景仍在既有唯一父队列中，不切换、不重复派发。历史7bd Source四增量单pair低于0.30、cold/warm失败和20对未验收保持。

精确timeout修复后的7bd Data generation一对观察已完整通过七次CLI及完整性核验，完整结果/CAP/IPC验证有效；任务总耗时仍约为Go的3–4倍，远未达到≤0.30。原7786五样本后失败保留，不以重试重标。该历史模式继续固定7bd与automaticEvictionEnabled=false；没有20对正式验收。

本批15份opt-in插桩文件与7份显式新产品观察工具文件共同整合：Data v5增加18个SQL/generation scope，CLI独立记录main entry、dispatch、render。关闭时不读clock、不锁stderr，普通性能环境大小写无关剥离两profile opt-in。scope有包含与重叠关系，不可相加、相减推出独占成本；CLI之外残差不是精确startup。这里只增加诊断，不是优化或性能收益。

新v2 --profile-observation仅允许一对诊断，明确automaticEvictionEnabled=true，绑定本次最终发布产品commit、干净sourceTreeSHA256与匹配optimized binarySHA256；候选tree或旧7bd binary不能代替。完整16,384×5有序精确数值、七CLI样本、四双profile、全部CAP/IPC、timeoutSeconds=300、原预算/取消/错误/owner退役/finalIntegrity不减。instrumented模式不能作为20对验收，旧false历史合同不变。

两个源码片均独审通过；最终整合全部19份paired Node测试855/855通过、0失败/0跳过；20份修改源码语法检查通过（10 Rust语法解析、10 Node --check），diff检查通过。6项新增Rust实际编译/执行仍NOT_RUN：云端官方入口因缺固定Emmy upstream在Cargo之前停止，不以Node或语法通过代替。后续先由既有父队列完成本次固定发布SHA的all-targets、Data phase/generation/local_query回归及CLI三个profile测试，逐名确认六新增和resource v5断言，再构建匹配产物执行新v2一对完整诊断；不得抢占原05660 Source队列。无本机原始日志入库，无merge/release或性能门禁完成声明。

## 2e. 2026-10-20 Source 实际结果与 test-only decode/control 诊断

本节更新前面05660 Source待验时间点，不改写历史结果。固定05660匹配产物六场景完整Facts、Index、summary、custody和finalIntegrity全部通过。Go/Rust毫秒：cold 8519.7729/3532.1597（0.41458）、warm 565.9414/227.6897（0.40232）、change 2770.111/882.5193（0.31859）；仅insert/delete/rename三个增量单pair低于0.30，cold/warm/change未达，20对正式验收仍未完成。warm typed decode 106632µs，对比7bd的121642µs只是不同run的诊断，环境/缓存未受控，不能据此归因相邻路径优化或宣布性能验收。

本次三个Source文件只增加cfg(test)诊断接线、已有exporter fixture认证复用及测试模块，不改变非test产品语义。先用真实完整Counted decoder做一次不计时控制调用计数；关闭计数后交替顺序测三轮相同完整decoder、真实OperationControl::check循环及有deadline维护检查的空循环。保全部字段/控制/认证，不设no-op decoder；固定总180s合作deadline、每pass/loop最多6400万检查，仍需外部owner超时/回收。读取/认证、完整summary/gaps的有界hash证明及析构不计入decode桶；测试TLS与关闭计数分支仍存在，隔离循环的代码/缓存环境不同，禁止相减当codec归因或外推cold/生产收益。

诊断只接受已有Source benchmark认证的一记录source-index-export-input.v1 manifest及固定index.bin；保128MiB输入和独立128MiB证明上限，不开放任意文件入口。前后复核原custody、manifest/body完整身份，最终复核前先释放首份完整buffer。三个active回归覆盖计数作用域/真实取消与deadline、完整decoder及错误退栈、固定fixture/身份/坏体拒绝；唯一benchmark默认ignored，仅由父队列在认证优化release或symbols测试产物、精确源码/工具链/flags/EXE记录下运行。debugAssertions=false本身不是优化证明；要求exact一次pass、三轮、countingEnabledDuringTiming=false及finalIntegrityVerified=true，其他原ignored单列。

独立最终源码审查通过，最终组合九套Node422/422通过、0失败/skip；官方云端入口owned.windows_required在编译前阻断，新三个active回归和ignored诊断均NOT_RUN，不是优化或性能验收。当前fca9固定native质量队列保持，不切换或重派；本批后续由唯一父队列固定新SHA进行Source all-targets/lib和独立优化诊断。Data插桩及v2产品观察合同保持原样，不借用旧版本结果，无本机原始日志入库。

## 2f. 2026-10-20 Scalar inline 与 generation 启动归因后继

固定 fca9 匹配产品的一对完整诊断已通过完整有序16,384×5结果、CAP/manifest/IPC、generation复用和finalIntegrity。Go/Rust完整毫秒为fresh 144.0619/425.7497、first 141.7286/339.5167、hit 119.5951/334.484，Rust prepare 271.7316；不是性能验收，均不能称达到≤0.30。一次hit诊断的IPC约0.422ms、source prepare 51.33ms、projection 26.97ms、CAP serialize 31.482ms、render 18.361ms；嵌套桶重叠不可相加或相减归因，不能把完整外部时钟余量直接当启动耗时。

本次以8e817e为精确基线，整合已独审的两份Data Rust与五份generation工具文件。Scalar值栈首个值inline，第二个同时存活值才分配原四槽Vec；generic Task栈、四槽预算及退栈refund、原控制检查和解释步骤不变。五投影加一ORDER叶子、16,384行推导减少98,304次小Vec backing分配，这是源码推导而非实测wall/RSS。首次allocator失败点随分配消失，spill先分配再取原owner；真实boxed future可能变大，原size_of_val准入、默认栈/深度和所有原限额必须原样通过，不得放宽或宣称全查询计数逐字相同。

新profile-observation要求七份实际native measured receipt（三Go、四Rust）均有startupDiagnostics及独立复核的startupBreakdown；helper、历史数据和普通/acceptance模式不启用。原CreateProcess至reap/drain的完整时钟不改、不扣除。rootExitTicks是owner延迟观察而非精确OS退出；CPU取原已回收root handle，不代表后代或observer CPU。旧fca收据不补造归因，新结果必须绑定本批最终SHA和匹配优化产物。

最终组合19份paired Node加owner套件共901项：890通过、0失败、11个Windows native平台skip；四份Node语法与两份Rust语法解析及diff检查通过。六个新增Rust仍NOT_RUN，前期官方入口在Cargo前因固定upstream缺失阻断；语法/Node不代表编译、native或收益。保留正在运行的8e Source诊断，不切换、不重派。随后唯一父队列固定本批SHA依序all-targets、完整relational（含原execution边界/default-stack/depth/future测量）、generation/local_query输出投影回归，六新增逐名一次、原ignored单列；通过后才以新匹配优化产物做完整七样本profile观察。全仓/业务/性能门禁和正式五循环仍未闭，无原始本机日志入库、merge或release。

## 2g. 2026-10-20 d952 实测与有界 ASCII identifier 快路

更新前节待验状态但不改写历史：固定d952原生首批335项与补充61项Rust通过；Node307通过/1项symlink权限skip。七次CLI调用完整正确性及最终完整性通过，仍未达到性能目标：Rust hit339.3229ms、Go113.6338ms，startup preResume分别24.0017ms与17.7223ms。inline未显示明确实际提速，不能以源码分配推导替代端到端收益。

本批仅一个产品源码文件hotfix/remote/model.rs：双方长度均≤64字节且均ASCII时用eq_ignore_ascii_case；其他输入走原Unicode17逐字符映射。64只是快路资格，不限制标识符长度；长串保原短路遍历，Kelvin/long-s跨ASCII、Turkish-I排除、非扩张语义原样保留。不改绑定/scope、work/control/准入、认证、输出或所有权。独审通过，最终源码SHA256为613b56133294db097ef3ffbbdbca33d081830dc7cf34920339fd894b993c7f6c；精确d952基线逆变换逐字相同。

四个新增Rust测试覆盖全部ASCII对、63/64/65边界与长串、跨ASCII Unicode及全部有效scalar映射。云端提取精确生产函数/测试和真实Unicode表独立重编译4/4通过，不是全crate或Windows原生测试。候选Node330/330通过；最终发布组合19份paired加owner套件890通过/0失败/11平台skip，源码语法及diff通过。全Cargo仍因缺固定Emmy源码在编译前blocked。15交替对孤立ID/F0/F1/F2/F3查找微基准中位20.451711→2.558425ms，仅纯函数查找，不是SQL实际收益或≤0.30验收；不将重叠profile桶相加/相减归因。

后续唯一父队列固定本批SHA，先列实际libtest名单，按互不重复的完整relational、hotfix::remote、local_query、generation、unicode_checks调用方集合执行，四新增各一次、原ignored单列。统一--locked --offline -j 1与--test-threads=1，原600s/default-stack与所有预算不变，禁止one-shot模式。通过后构建匹配优化产品做同fixture完整七调用及startup观察，全部SQL/CAP/IPC/manifest/authentication/复用/最终完整性检查保持；原d952结果不改标，新Windows质量及性能待测。无原始本机日志入库，无merge/release。

## 2h. 2026-10-20 ea7 实测与共享数字 provenance 内层

固定ea7上一批原生585通过/0失败/1原ignored；完整七调用及全部正确性通过。hit Rust301.2569ms、Go112.4970ms，比d952的339.3229ms低，但两次单轮不能归因为ASCII快路，也未达≤0.30门禁。原始分批身份与profile时钟保留。

本次四文件Core/Data优化保留外层Indexed，首个非空float行模板至多8个位置时才共享不可变内层SharedIndexed；若首个模板更宽，则整次调用保持原owned路径。宽度/行数无新限制，差异前缀重建最多8项。两种内层共用原最大索引检查、遍历、shape错误及两遍serializer；外层错误顺序不改。所有逐行check、逐cell work、逐float retained（含0 charge）与外层准入位置不变；实际NumberLayout size/alignment与原spare-node覆盖Arc owner为阻断门禁，失败不得改阈值/计费。

精确ea7树的四文件已独审。候选Node330通过；最终组合19份paired加owner套件890通过/0失败/11平台skip，四源码rustfmt与diff通过。隔离提取实际producer/serializer与7项原新增测试，加3项独立差分，debug/release各10通过（同10项不是20distinct）；9,374故障ordinal、178个零retained点、4,096 float-mask对及4,096混合宽度案例一致。Budget/ToolkitError是明确披露的scaffold，不是产品原生计费/取消并发验证；完整Cargo仍缺固定Emmy源码而编译前blocked，新Windows待测。

60交替对孤立构造微基准为12.648→9.101ms、析构2.284→0.257ms；输入16,384×5且4 float，采用实际BTreeMap/Arc但synthetic Budget。仅孤立构造/析构，不是SQL、序列化、全进程wall、Windows、RSS或≤0.30证明，不能把历史inclusive projection桶当可回收收益。

后续唯一父队列固定本批SHA，先实际名单核对互不重叠Core public_json、Data完整local_query/relational/generation/phase_profile及CLI data_phase_profile集合，7新增各一次、旧ignored单列。全部locked/offline/-j1与test-threads=1，原600s/default-stack及size/allowance/深度/所有预算门禁不变，禁止one-shot。质量通过后才构建匹配优化产物，完整七调用、精确数值/CAP/IPC/manifest/认证/不重发布/最终完整性及owner退役全部保持；对照ea7计数，不相加/相减嵌套桶，不外推RSS。无原始本机日志入库、merge/release。

## 3. Home、容量和所有权边界

Data默认容量为**5,000,000,000字节（十进制5GB）**，配置支持明确单位；与测试scratch 5/6GiB不同。最终统一核算Data下载/共享blob归属、物化/索引/目录、staging/spill/导出峰值和证据占用，按内容去重防双计。活跃lease和证据引用保护；旧已超额或配置缩小先拒新增并说明，不删除证据。

CAP删除会识别同body的独立Data保留权：仅删除CAP引用并返回仍共享，不承诺物理释放。判定在原blob lease内认证固定Data记录和body类型/长度，不倒序获取quota或重复全读hash；这是保留权判断，同长度坏内容仍留存，真实消费继续完整认证。损坏记录、孤立完成记录或已登记body缺失/类型长度不符均硬拒，不静默减账。Data引用的冷缓存释放与完整淘汰生命周期仍须后继闭合。

当前基础只覆盖已接入的Data写入；私有state/data SQLite主库、journal/residual和已登记共享body已计量。历史未登记blob、历史共享toolkit SQLite/sidecar、归属释放及剩余临时/导出路径尚未闭合。不能把派生对象helper当全Data硬上限。只缓存实际查询过的表，临时查询可不落盘；不默认长期保留raw、IPC、Parquet三份。尚未实现并验收的淘汰策略不称已生效。

Source主Index采用明确v18身份与LYCIDX01帧，query metadata同步版本化。完整文件SHA认证后，共享typed字段校验供完整与摘要消费；摘要不构造全量owned Symbol。128MiB限制新缓存实际文件字节，构造分配另行有界准入，不再为旧JSON内部表示额外全量计数。因新版本及时取消可先于尚未读取的损坏尾返回；已经确认的语义/预算错误保持，不把坏缓存作为健康结果。正式诊断保完整公共Index与Facts，两个exporter输出共用原128MiB证据额度，超过明确失败，不能截断比较。资产比较保f811已认可的大资产前置稳定分区和组内完整顺序/重复。

Source新格式旧cache失效后重建，不维护生产旧格式迁移。新格式完整认证、边界/预算/取消和损坏拒绝仍必须保留；坏新缓存不能通过旧格式fallback隐藏。公共JSON和query/inspect业务输出保留，私有wire变化单独版本化。

## 4. 性能与已知证据范围

最新44a六场景Go/Rust毫秒：cold8761.3351/3835.3861；warm574.6557/249.7914；change3024.2660/877.4871；insert2991.4838/850.2395；delete3012.4679/849.8195；rename2926.3632/860.6207。质量与最终完整性全部通过；四个增量单pair比率0.2821–0.2941达到0.30线，cold0.4378/warm0.4347未达，20对正式验收仍未完成。同次warm cached174.316ms；cold visit3470.405ms、串行53份777.555ms、worker等待1959.438ms。增量FactsRead250.545–257.277ms；Symbols缩小伴随ID计算转入序列化，不能只按单桶宣称收益。内嵌/并行phase不相加作独占成本。

上一轮Source诊断固定86f2bbb：一次匹配release/装包后六场景完整Facts/Index、summary、custody与最终完整性校验全部通过（finalIntegrityVerified=true）。同run Go/Rust毫秒：cold7801.86/3885.1199；warm587.1695/255.1399；change2965.3629/982.6756；insert2910.4461/984.6361；delete2848.9826/997.6467；rename2849.0348/1158.9573。六项比率仍均大于0.30，仅一轮诊断，不是20对正式门禁。21个owner、40个native调用均正常退出并回收；2次forced均已认证辅助进程，无未知辅助或超时，进程/lease归零。当前仅非敏感汇总，原始本机记录不公开。

历史2d23695：匹配release、91文件装包及exporter认证通过。同run Go/Rust毫秒：cold10687.5959/5095.8654；warm667.0608/267.0469；change3958.0999/1344.9300；insert3962.2321/1357.1795；delete3871.5698/1225.4815；rename3704.7494/1240.8674。六场景逐项完整Facts/Index、summary与custody比较通过；原最终runner因owner receipt的1MiB上限拒绝合法约5.2MB封存记录而失败，原finalIntegrityVerified=false保持。666修正验证器后的独立只读复核已exit0，结果为sealed_complete_scenarios_reverified，认证52,494文件；没有新增产品、build或exporter调用，也未改旧记录或六组wall。666 Windows Node505项为498通过、0失败、7个symlink权限skip；两个owner均回收，无forced或timeout。该新复核闭合封存证据完整性，不把原失败改成功。六比率均大于0.30，只一轮诊断；内存采样缺口不证明RSS峰值改善，不能把异负载绝对耗时变化全归优化。

同次冷路径visit为4587.845ms，内含工作与等待；串行解析76次累计1278.543ms，worker等待2202.707ms，23任务放弃。增量Facts读取/认证/解码321.305–363.620ms、Symbols构造114.992–138.115ms；warm完整缓存读/SHA/typed校验188.943ms。各阶段嵌套、worker解析并行累计，不相加或差减成伪独占。FactsRead次数包括组目录、probe、记录与close，不能称重复磁盘读取次数；摘要路径已借用typed字段，不再构造完整owned Symbols。下一窄片已实现并静态独审：Lua递归临时预留使用当前子树跨度，保原共享额度和真实超限回退；Facts编解码移除仅复刻旧JSON私有缓存大小的全遍历，沿现有Wire编码同时核规范decoded ownership，实际binary32MiB和decode256MiB额度保持。完整输出/证据预算仍独立，0bca已实际check通过，Source lib730通过/6忽略、inspect15、flow4及正式luac5.1.5精确用例1通过，合750通过/0失败/6忽略；13个新增或调整名称及1个既有细化各执行一次。86f新性能结果见上；同次冷串行53份累计765.792ms、worker等待1921.308ms、abandoned=0，旧2d为76份/23弃用。现FactsRead增量243.809–265.784ms、Symbols88.793–110.011ms；warm cached172.061ms、Index读取54,464,537B。本轮abandoned=0，历史2d为23；该差异不独立证明因果，跨负载绝对秒数不能全部归因此片；解析并行累计和嵌套phase仍不可相加。下一步按真实准入原因分析剩余串行与聚合表示，不先假定全部大文档或线程数不足。

Data CSV9693工具对旧63d与a860的单pair：cold401.4790/389.9062ms，warm349.0380/324.7937ms；完整CSV/sidecar与8CAP核验。该对比是旧Rust/新Rust，不是Go性能门禁；retainedBytes减少46,988,672B与decodeWork减少81,920是逻辑计量，不是RSS。其余Data、Live正式性能与全产品矩阵仍未验收，不把历史结果归新产物。

86f 的现成 Data 诊断已完成：24对/48次 CLI 全部结果等价，40项完整图像像素与128个CAP核验通过，Node63通过；没有调用 cache prepare 或 DF。Go/Rust 毫秒：BLTE cold197.14/335.78、warm152.78/127.19；五种PNG cold Rust270.64–301.38 对 Go152.36–154.45，warm Rust122.16–130.90 对 Go121.40–130.36；五种WebP cold Rust269.39–303.87 对 Go225.74–245.49，warm Rust124.75–149.96 对 Go195.62–213.67。所有性能路径比率仍大于0.30，单轮诊断不作20对验收；tiny SQL只作正确性。48份内存采样中6份有缺口，其余亦不保证完整RSS峰值。同次冷图像 prepare 约91–109ms、payload约66–86ms，decoded发布包含两个不同body约117–144ms；纹理解码约0.8–1.8ms、编码约1.5–6.8ms。源码确认每次成功的普通新 body 流式发布包含两个quota生命周期、各在前后盘点，共四次库存；每次重开本次已经持有的固定目录，是后继复用片的依据。各计时嵌套，未计量部分不能直接称磁盘耗时；4处durable sync全部保留，实际收益需同任务匹配产物复测。

历史coverage十二integration实际130通过/1权限ignore，报告是独立对象分母，不能平均成全项目覆盖率。f86 companion真实四CLI产生profile后超原raw128MiB总预算，merge/export/witness未执行；不加预算，覆盖工具扩展暂停，先完成业务与性能门禁。真正OS压力、游戏及尚未覆盖的Skill实际任务仍有缺证；本轮四类离线真实CLI盲评与SQL补验见第2节。

## 5. 当前阻断与下一批

当前后继以第2a节为准：7bd的42项及两check、384f的CI均已通过；本批自动容量退役的新产品仍待固定SHA原生验证，generation性能先测独立固定7bd产品。下面保留前批未复验时的历史队列记录，不重派或重标。

b47两个check通过，270 matched为265通过、2失败、3原ignore；实际执行267项，63新增为61通过、2失败，正式名单无缺漏/重复。Core目录替换fixture及SQL保留词fixture的最小测试修正已独立源码复核，尚待固定版本实际复验。未到达的后续断言不以静态推理当通过；本地decoded9项及其余实际通过组保持有效。

下一候选包含这两处测试修正、显式Data derived退役与Source warm子段计时。新功能Rust尚未编译/运行；源代码检查和Node工具测试均不能代替Windows验证。当前无自动5GB全闭合、默认DF切换或全部性能验收结论。

最终25源码组合的15套Node共538项：527通过、0失败、11平台skip；额外历史source-project-selection封存映射失败在base与独立退役候选均复现，未计入这15套、最终组合未重跑，仍单列失败，不称全仓通过或Rust验证通过。

历史编译、测试和runner失败保留于[前一固定交接](https://github.com/Follen/Lychee-Dev/blob/b47ede61f0530fc954d754f49a1f16452f297603/handover.md)及对应Git历史，不重复列成当前待跑队列。组合验证必须使用精确待提交完整字节树；旧隔离base的通过不替代最终组合结果。Windows正式新测试名单以实际libtest inventory为准，忽略与权限skip单列。

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
