# Rust 迁移交接：唯一当前事实来源

更新：2026-10-15 15:39 UTC。分支 `rust-refactor`，持续交付到[草稿 PR #14](https://github.com/Follen/Lychee-Dev/pull/14)。最新已核远端批次：`bc85a4c22734f57e2fa6395165b9214afa34ddb7`。实现进行中，尚未完成全量验收。

## 1. 当前决定与不可漂移基线

- 唯一验收目标：以固定 **Go 3.2.2** tag 源码为当前行为基线，实现 Rust 宿主并保留 **EmmyLua 直接分析库/native-worker/session** 替代 LuaLS。除此之外，全部业务、同一 addon、CLI、Skill、完整 Mailbox 协议、错误恢复、诊断和输入合同对齐正式版本。
- 当前 Go tag `v3.2.2`：tag object `97890b267a620a166889db0934e57a826d72a872`，peeled commit **`0bec60e4406f5d5dc155ee25cef44f9a710e37e8`**；2026-10-15 精确远端对象与本地 fetch 已核，检查时 main 同 commit。Release API 当时返回 404，不能声称发行二进制/资产已验证。业务增量 `969fe480fdb38eb038401044c3327b04ccaaee81`，详见[3.2.2 增量矩阵](docs/toolkit/rust-go-3.2.2-increment.md)。
- 前 v3.2.1 tag object `637584b8b976966c3863c1889e9ff86fe55493d4` / commit **`10b561db5706702cd42e047eaeed4ccb5e825351`** 和 v3.2.0 tag object `93849f43f34d8ff1d6f4539c5a3496ab941ef229` / commit **`3057a9666ab078270b63c7f06de748069b0d1f4b`** 的源码、fixture、二进制、测试/失败证据均保留原身份，不能重标为3.2.2通过。未完成的[3.2.1 累积增量](docs/toolkit/rust-go-3.2.1-increment.md)仍须完成。
- 2026-10-15 用户最新版本决定：未发布 Rust 候选源码/包版本 **3.5.0**，发行配置渠道 **beta**（版本与 npm dist-tag 分开，不自动改成 `3.5.0-beta.1`）。Go 行为 oracle 仍为上述 3.2.2；只获准同步源码和配置，未执行发布、tag 或 npm 渠道变更。版本同步已完成源码集成及本地包合同检查；精确 b16a 候选已通过 Windows all-target 编译和 CLI 构建，后续提交及完整产品验证仍按各自证据记录。
- 2026-10-15 13:39 用户明确要求“全量替换”：最终验收必须移除全部 Emmy 上游源码补丁，使用未修改的固定上游公开 API，自有 crate 承接完整 XML/Lua 增强、类型/诊断/来源/取消与资源合同。此前“尽可能减少、必要时保留最小 hook”的最终豁免已被此要求取代；已有补丁仅属过渡实现，不能作为交付完成。逐 production hook→实际消费者→自有替代→差分/全仓证据列迁移表并分批实施，不另写整个 LuaLS、不削功能或假造 checker coverage。现有原版5/5小样及 public rendering27+27/450trace证据只覆盖各自范围，不能宣称全量零补丁已完成。
- Rust 调查起点：`5443314c536e925801d3f0360110db946181d1b3`。它是复用基础，不是完成声明。按模块差分决定保留/替换/补齐，不从零丢弃已有成果，不沿用不符目标的旧协议。
- unsafe约束：自有业务/跨平台Rust代码默认禁止unsafe，用适当crate级forbid/deny守住；确有必要的Windows FFI/内存边界必须最小化安全封装、逐处SAFETY理由和指针/生命周期/线程约束与测试，独立审查，不整模块无限豁免，不为性能目标放开unsafe。第三方依赖另列审计，不能宣称整个依赖图无unsafe。
- 内部设计应体现Rust特点，不逐行翻译Go：合理所有权/借用减少复制，enum/类型状态、Result、RAII和有界安全并发按实际收益使用，避免过度抽象/炫技。对外合同不变，unsafe边界不放松，性能仍实测；纳入独立架构验收。
- Skill尽量复用固定Go3.2.2版本；仅Emmy引擎、安装和真实诊断等必要差异修改，不因换语言重造工作流。
- 性能：可控耗时减少至少 70%，即 **Rust ≤ Go 30%**，排除下载与不可控外部等待；同机同数据、输出与校验等价，cold/warm 分开独立重复，逐路径记录。未测不能保证达标。
- 所有需要用户、实际游戏或特定本机互动配合的测试放在本文最后；离线与可自动执行的 Windows CI 先做。Windows 专属测试不自动等于游戏测试。
- 首批交付仅文档；随后已获准全量实施本计划。仍不操作游戏、不发布、不合 main，不修改 Go PR #13 或其他项目。只保留本文件，不创建 `handoff` 文件或第二份交接事实来源。
- 交付约定：为Rust分支创建独立草稿PR；之后每批获准代码改动及时推送并更新同一草稿PR，不等全部验证完才提交，也不每批新建PR。先查已有PR避免重复。后续实现授权不等于游戏操作、发布或合并授权。

[DESIGN.md](DESIGN.md)解释目标结构，[todo.md](todo.md)给执行顺序，两者从本文件派生；更新状态与证据首先改这里。旧 `docs/toolkit/` 和历史 README/UI 设计是原受测范围的记录，不得覆盖本节。历史“Go3.1.2替换”“Rust≤Go50%”“重新设计四槽exact-COMMIT”与旧输入合同已被本次目标替代；历史测试通过仍只证明原始候选。

## 2. 当前实现与未闭环事项

| 模块 | 已实现/已接线 | 当前缺口 |
| --- | --- | --- |
| CLI / 版本 / 打包 | 3.5.0 beta 源码配置；当前 Go describe 合同；正式 addon ZIP；默认 Live 路由 | 最终包安装/更新/卸载全链、参数/输出完整差分；内部 Invocation 空值处理不等于实际 argv 空值已等价 |
| Live / Delivery | Go Mailbox v1 默认 CON、RIP 定位/输入/持久恢复；321键盘静默与322槽启用/reload；500槽发布/维护 | 精确最终树自动回归与真实 Lua Driver 桥；多实例/真实客户端最后验收；未知结果绝不重放 |
| Source | 完整固定 Retail 静态导入/查询；Emmy direct worker/session；Graph move、一次 XML catalog、90秒 research 原预算、调用者身份候选已落地 | 完整语义与常驻会话仍未通过；新优化全仓时长/内存待实测；零上游补丁迁移尚未完成 |
| Data | 本地 DB2/CASC/SQL/Hotfix；可取消 HTTP、remote catalog/target/encoded Session；direct remote target 已接实际 CLI；decoded alias 与共享 body 的 retained 责任转交已组合；Core 原控制贯通 blob/pin/capture | named target、navigation/JSONL/streamed assets；SQL/DB2默认provider及外层Metadata已接，内部Store/Fragments预算错误首因、自动pin/effective旧等待与实际使用仍待闭合；archive计量、gzip边界、合法raw-URI差异仍有缺口 |
| Skill | 固定 Go 工作流及实际 CLI 验收入口 | Source长语义阻塞后的完整任务复测；Source/Skill/Data各自独立真实使用循环未收敛 |
| 清理 / 质量 | 逐批独立源码审与回归持续执行，旧证据保留 | 21个旧 addon 额外文件仍有真实动态消费者；不能只删文件让静态门槛变绿。最终四阶段审计尚未开始收敛 |

### Emmy 全量替换当前顺序

当前补丁31路径：26源文件、2 manifest/lock、3测试。六组真实职责为共享摄取、context递归推断、object/member身份、call/return/generic checker、诊断range/selection、session/coverage。全部必须外移后恢复原版来源，不能以包装crate冒充零补丁。

1. 已完成 public declaration rendering 的局部替代，历史27+27/450 trace证明只属于其冻结输入。
2. DeclarationModel摄取后继已独审，三方 A旧构造/B新构造/C完全原版公开API的prepare-lock已完成，150registry+5local每侧身份核验一致，实际验证已启动：A31通过，B编译与projection11/declaration11通过；随后B callback列表进程exit0但namespace回收观察遇EACCES，无法确认完整树回收，owner中断且symbols lease保留。C及近8MiB资源组未运行；后继本地Cargo暂停，准备独立Windows三方证据链，不能追认旧owner。公开parse/index合并了原阶段边界回调（原实现也无逐token回调）、增加文本持有是待测取舍，不能宣称等价或减内存。
3. owned session/coverage ledger、selected-decision与DerivedNumber/EnvironmentCallable/EnvironmentReturn、call/return query编排三片已分别独审并累计组合12个源文件；21新增Rust仍NOT_RUN，已接准确测试目标。递归后端尚未完成，未切生产、未删除现有hook，不能以新增自有代码冒充零补丁完成。旧callability首候选存在enable/coverage顺序差异，未集成。全部43个registry入口按真实消费者核对，不能删除nil/unknown/denied-call能力换取无补丁。
4. 其余推断、identity、cardinality与checker随后按真实消费者逐片替换。最后核未修改上游完整文件身份、产品锁、全Retail和全部增强回归。

## 3. 最近已验证证据与限制

- **8e2dc296**：15文件上传后逐SHA/长度读回一致。新增direct remote CLI route及Core optional blob R4已独审；7项CLI和20项Core Rust测试仍NOT_RUN。R3目录walk未贯通控制的finding由R4五个native callsite修复，原R3证据保留。
- **同组合本地静态**：干净Git基底加精确15文件执行实际static job的14条Node命令；221测试中218通过、3项Windows launcher在Linux跳过。前13命令成功；最后真实parity报告仍退出1，保留21旧addon额外文件差异。未执行Rust或游戏。
- **b16a2d87**：Windows run `38740832295` 已结束失败。精确 all-target 编译、sealed CLI build、真实 describe、静态 full Retail 成功；final artifact `11981206048` 的168 owner/2655 sealed logs逐hash核验。真实native refs/context/API/session及IPC bootstrap在约27–31ms即被`delivery.invalid_release`拒绝：Rust delivery reader仍硬编码next，与获准3.5.0 beta包绑定冲突，分析器没有开始，本轮没有Graph/XML优化后的完整语义内存/耗时样本。现候选共享既有精确version→channel函数，公开manifest读取与update的5组合回归已写，Rust仍NOT_RUN。normal control和Mailbox的三项fixture偏差已独审修正，原失败保留。66c2/311250/8e2dc后续新增不在b16编译证明内。
- **本地GNU typecheck**：fixed b16首轮600秒冷编译超时并完整回收；暖轮因git archive漏掉export-ignore的4份真实profile而exit101。已按完整Git blobs重建5244文件闭包并独审，173处include无缺失；第三轮在原600秒上限内实际exit0（Cargo 5m10s），完整前后305官方包/源码/工具/config复核、16封存日志核hash及进程回收通过；owner `bdfa539a-0c87-4ca6-8abb-bd20c81fc2b4`，receipt SHA256 `b34ef64e2402887d6e8ec6e75042fe9a7edd60fc3aa267cd540aae8b50ac2362`。GNU检查不等于MSVC或Windows运行。
- **b16正式Lua与静态内存**：固定Go322正式Lua的220项observations实际通过，输入未变且全部句柄回收；不等于真实游戏通过。静态Retail source-process采样working set冷索引675.54MiB/暖索引224.04MiB/查询362.52MiB，private bytes783.52/329.69/539.76MiB；与未启动的全语义路径严格分开。
- **479e57a6**：31文件已推并逐SHA/长度读回一致，含R13c2/R13d2、beta准入及三项fixture修正。run `38754201940` 当前all-target/CLI/静态Retail成功；真实Driver对正式Lua peer步骤失败，尚待封存日志定位；Emmy独立job成功，static仅最终21旧addon差异失败。完整产品/native结果仍待终态。
- **9ba0bd1 SQL组合**：R17.1实际`--cdn --snapshot`静态/effective/meta SQL及EXPLAIN/CSV走scoped remote provider，持有admission至capture并借用Session共享body；六项真实consumer测试仍NOT_RUN，现有remote测试组会匹配。local同受64MiB executor reservation与bindings提前的错误优先级调整。Go外层Metadata生命周期、自动pin/effective旧等待仍未完成，不宣称完整SQL准入闭合。
- **b0fe353 definitions组合**：R3把原invocation与独立Budget贯通metadata等待、可取消HTTP、解析及发布；实际已读网络先记账、consumer留存后准入、原30秒request上限/4MiB+1前缀和Go逻辑计量保持。R1/R2已发现的取消fixture竞态与解析错误被stop覆盖均已独审修正；24新增Rust未运行。现有definitions::和remote::组覆盖，无重复新job；实际SQL/DB2调用已在当前R18.3组合改接；Core隐藏publication Directory checked-close与既有gzip优先级缺口仍保留。
- **bc85a4c DB2组合**：R18.3接实际record/page/schema/search/FK远程provider，outer Metadata→pin/lease顺序逐Go命令保持，owner持至capture。独审修正两项：外层/读pin直接保原invocation+Budget回调；资源cleanup-only后统一stop，避免把同一stop当close event。15新增/11继承Rust未运行。Store/Fragments内部仍有Budget child导致错误首因改变，已明确确认且另片修复，不宣称完整端到端取消分类完成。
- **最新完整旧语义内存**：ce6 sampled source-process group working set约2356.48MiB、private bytes2489.39MiB；retained cold失败，warm/steady未测。62d约2355.94MiB，79ef约2350.31MiB；均未含后续Graph/XML改动，不能称有下降。100ms采样有缺口，是观测下界，不是RSS/内核绝对峰或稳定占用。
- **Source历史失败**：62d cold refs在约182秒runner成功但不满足Go90秒总预算；context超时、retained worker EOF仍是实质缺口。ce6全部长语义路径失败。新90秒、phase、Graph/XML修改不能在新全仓结果出来前宣称修复。
- **执行/写入边界**：被明确拒绝的重复gzip验证未重跑；PR14标题/正文刷新两次拒绝后保持暂停。代码、文档与其它已授权验证继续，不改rules、不换路绕过。未发布、未合main。

所有旧批次、owner/hash、失败与修复来源见[历史证据记录](docs/toolkit/rust-migration-evidence-2026-10-15.md)。旧状态按原树/时间理解，不覆盖本文现状。

## 4. 接下来按可交付批次执行

1. 跟进479e57当前Windows批：真实release读取/Source入口、完整Retail语义、常驻退出/IPC及同口径内存，不放宽90秒合同。持久化已审R17.1 SQL组合并继续默认DB2等消费者。
2. 本地symbols因Emmy owner未确认回收而保留，不接管/新channel绕行；当前479e57 GNU完整输入已准备但未执行。独立Windows三方方案先审再运行；未来Linux观察修正不追认历史owner。
3. decoded alias与body owner已组合，接续真实DB2/SQL/static/effective/meta/JSONL外层admission到capture。EXPLAIN/schema-only不能扫描内容，callback不得重放；大资产保持流式。
4. named target按固定Go配置-only分页lookup、pin-ref冲突和输出合同补齐，不复用旧show造成额外检查/错误。direct remote已接线但真实Rust/online仍待跑。
5. 继续全部Emmy hook实际外移、真实Source可读性、Skill/Data使用质量及旧消费者替代；完成后依次执行下列四阶段审计。
6. 每批及时普通push保留工作。只有有意义的验证批使用 `[rust-native]`；fast成功/长组skip不等于产品通过。只维护此handover，历史细节移至证据文档而不持续堆叠当前页。

## 5. 验收与循环审计

完整[F01–F13功能、P01–P14协议、B01–B07性能和Go来源合同](docs/toolkit/rust-migration-acceptance.md)保留；[321增量](docs/toolkit/rust-go-3.2.1-increment.md)、[322增量](docs/toolkit/rust-go-3.2.2-increment.md)是累积要求。代码已实现、静态已审、某树历史通过、当前实际通过必须分开。

- 固定Retail数据为Gethe/wow-ui-source `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b`：4036 Lua/XML/TOC，32,269,051字节。每次核身份后全仓分析/代表查询/源码定位/refs/context/flow/cold-warm/session/释放，不以小fixture代替。
- Source、Skill、Data分别由独立使用者执行真实CLI任务，记录输入、完整输出、正确性/可读性/来源/错误和复测。实现者不自评通过，无法运行时记blocked。
- 全量实现后的四阶段**顺序循环**：①架构/全业务/Skill/全部代码；②有效覆盖与无独立价值冗余；③真实消费者确认后的死代码；④前三维度联合全仓。每阶段独审→修复→实际验证→独立复审至无新可行动finding，再进下一阶段。每批源码审不等于最终收敛。
- Go322测试映射为差分oracle，320/321原证据保留；尤其未知结果不重放、事务、取消、空map、槽迁移与错误恢复。记录固定树、覆盖分母、平台排除与停止条件，不保证数学零bug。
- 性能必须同机同数据/结果与校验等价，独立cold/warm逐路径重复；可控耗时Rust≤Go30%，外部等待另列。当前仍未达成验收，不能把缓存预热或删校验当提速。

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
