# Rust 迁移交接：唯一当前事实来源

更新：2026-10-17 03:29 UTC。分支 `rust-refactor`，持续交付到[草稿 PR #14](https://github.com/Follen/Lychee-Dev/pull/14)。最新已核远端批次：`bbee33587a8bbfe31a1ea192be645884f0e67b97`。正常4036事实与84个固定边界输入已有等价实证；retained冷refs与跨CLI暖context均已通过；2d7 runner/a338产品完整full14项已通过，身份变更与恢复、同owner均成立；partial155保持。公开project选择新增12项含真实packaged已通过。性能与全量验收未完成。

## 1. 当前决定与不可漂移基线

- 2026-10-17用户明确“更优秀的肯定是要留下来的”：经源码与真实消费者证明更准确/完整的Rust结果保留，单列兼容差异并正式回归；历史raw差异和精确当前合同检查并列。既有两条必要object call_result保留，未核差异不得自动称增强，也不机械删正确结果只求Go逐字一致。

- 唯一验收目标：以固定 **Go 3.2.3** tag 源码为当前行为基线，实现 Rust 宿主并保留 **EmmyLua 直接分析库/native-worker/session** 替代 LuaLS。除此之外，全部业务、同一 addon、CLI、Skill、完整 Mailbox 协议、错误恢复、诊断和输入合同对齐正式版本。
- 当前 Go tag `v3.2.3`：tag object `7a24532cf0fe294faeb96e8224797b6fe69386d5`，peeled commit **`3766f9cacb52e93a803a626187e2a1d54a445468`**；2026-10-15远端tag/main与本地精确对象已核。322→323为2提交、41文件；业务在该正式提交，另一个提交为322发布文档。新增自适应Data内存/准入容量、紧凑记录、IN选择及Data Skill，详见[323增量矩阵](docs/toolkit/rust-go-3.2.3-increment.md)。新增验收PENDING；未验证发行二进制/资产。
- 历史 Go tag `v3.2.2`：tag object `97890b267a620a166889db0934e57a826d72a872`，peeled commit **`0bec60e4406f5d5dc155ee25cef44f9a710e37e8`**；2026-10-15 精确远端对象与本地 fetch 已核，检查时 main 同 commit。Release API 当时返回 404，不能声称发行二进制/资产已验证。业务增量 `969fe480fdb38eb038401044c3327b04ccaaee81`，详见[3.2.2 增量矩阵](docs/toolkit/rust-go-3.2.2-increment.md)。
- 前 v3.2.1 tag object `637584b8b976966c3863c1889e9ff86fe55493d4` / commit **`10b561db5706702cd42e047eaeed4ccb5e825351`** 和 v3.2.0 tag object `93849f43f34d8ff1d6f4539c5a3496ab941ef229` / commit **`3057a9666ab078270b63c7f06de748069b0d1f4b`** 的源码、fixture、二进制、测试/失败证据均保留原身份，不能重标为3.2.2通过。未完成的[3.2.1 累积增量](docs/toolkit/rust-go-3.2.1-increment.md)仍须完成。
- 2026-10-15 用户最新版本决定：未发布 Rust 候选源码/包版本 **3.5.0**，发行配置渠道 **beta**（版本与 npm dist-tag 分开，不自动改成 `3.5.0-beta.1`）。Go 行为 oracle 现为上述 3.2.3；只获准同步源码和配置，未执行发布、tag 或 npm 渠道变更。版本同步已完成源码集成及本地包合同检查；精确 b16a 候选已通过 Windows all-target 编译和 CLI 构建，后续提交及完整产品验证仍按各自证据记录。
- 2026-10-15 13:39 用户明确要求“全量替换”：最终验收必须移除全部 Emmy 上游源码补丁，使用未修改的固定上游公开 API，自有 crate 承接完整 XML/Lua 增强、类型/诊断/来源/取消与资源合同。此前“尽可能减少、必要时保留最小 hook”的最终豁免已被此要求取代；已有补丁仅属过渡实现，不能作为交付完成。逐 production hook→实际消费者→自有替代→差分/全仓证据列迁移表并分批实施，不另写整个 LuaLS、不削功能或假造 checker coverage。现有原版5/5小样及 public rendering27+27/450trace证据只覆盖各自范围，不能宣称全量零补丁已完成。
- Rust 调查起点：`5443314c536e925801d3f0360110db946181d1b3`。它是复用基础，不是完成声明。按模块差分决定保留/替换/补齐，不从零丢弃已有成果，不沿用不符目标的旧协议。
- unsafe约束：自有业务/跨平台Rust代码默认禁止unsafe，用适当crate级forbid/deny守住；确有必要的Windows FFI/内存边界必须最小化安全封装、逐处SAFETY理由和指针/生命周期/线程约束与测试，独立审查，不整模块无限豁免，不为性能目标放开unsafe。第三方依赖另列审计，不能宣称整个依赖图无unsafe。
- 用户2026-10-16进一步明确所有模块按Rust原生方式设计：第一阶段逐一审Core/Source/Data/Live/CLI/Skill交付边界，给出所有权/借用与clone成本、typed状态、紧凑结构、流式/堆任务栈、取消/错误/析构及有益并发的具体证据。保留已有好实现，不机械翻译Go、不盲目全重写或为语言名义造泛用框架；外部合同、unsafe边界和实测性能要求不变。当前实现和本机测试持续进行。
- Skill尽量复用固定Go3.2.3版本；仅Emmy引擎、安装和真实诊断等必要差异修改，不因换语言重造工作流。
- 性能：可控耗时减少至少 70%，即 **Rust ≤ Go 30%**，排除下载与不可控外部等待；同机同数据、输出与校验等价，cold/warm 分开独立重复，逐路径记录。未测不能保证达标。
- 所有需要用户、实际游戏或特定本机互动配合的测试放在本文最后；离线与可自动执行的 Windows CI 先做。Windows 专属测试不自动等于游戏测试。
- 首批交付仅文档；随后已获准全量实施本计划。仍不操作游戏、不发布、不合 main，不修改 Go PR #13 或其他项目。只保留本文件，不创建 `handoff` 文件或第二份交接事实来源。
- 交付约定：为Rust分支创建独立草稿PR；之后每批获准代码改动及时推送并更新同一草稿PR，不等全部验证完才提交，也不每批新建PR。先查已有PR避免重复。后续实现授权不等于游戏操作、发布或合并授权。

[DESIGN.md](DESIGN.md)解释目标结构，[todo.md](todo.md)给执行顺序，两者从本文件派生；更新状态与证据首先改这里。旧 `docs/toolkit/` 和历史 README/UI 设计是原受测范围的记录，不得覆盖本节。历史“Go3.1.2替换”“Rust≤Go50%”“重新设计四槽exact-COMMIT”与旧输入合同已被本次目标替代；历史测试通过仍只证明原始候选。

- 2026-10-15 18:24 用户调整实施节奏：先按完整端到端功能实现并接入默认生产消费者，再集中补测试和统一验证；取消自行增加的逐小片冻结/独审/CI先行门禁。实现过程中仍及时分批提交推送同一草稿PR，真实安全边界和文件单写协调保留；已有CI旁路诊断，不等待每轮才继续编码。完整验收要求保持，最终审计按2026-10-16后续明确新增的第五项执行。

## 2. 当前实现与有限剩余项

- **Data 默认链已接入**：本地/CDN、Prepared/cache、SQL、导航、asset、Hotfix、自适应内存与checked收尾已有实现。统一单堆SQL保持默认栈、原深度、逐行provider与真实预算lease；正式relational299及原6执行崩溃回归通过。local_query76/0/1、全Data库1063/0/1已有证据，原集成预算失败经7db27/0/0闭合。当前剩余为新Data配对入口真实执行、代表性SQL/读写/解码性能和最终独立审计。
- **Emmy 已使用原版848文件，语义验证未完成**：`3336d82`已保存 initializer/provider 重复查询、call quality 多余查询和完整键集合被未知值污染的修复。原固定参考与当前候选的全量类型/provider/coverage差分保留；ac8实际Adapter140项、context56项已通过；完整历史严格差分参考36通过、候选35通过1失败，4137行中仅object成功投影的两条正向call_result证据有差异。独立当前合同检查已在原ac8真实sealed产物通过（4135相同行+2精确当前receipt），原始严格失败、全部类型/provider/coverage原文和Cargo退出结果保留；不能将新增检查冒充原历史比较通过。
- **HTTP 后继已保存并取得定向实证**：`6051063`使用固定 native-tls/schannel 三源38加4删的最小适配，将SNI名与证书校验名分离，保原验证与tokio所有权。完整来源/许可/逆补丁和打包已接，Node20/20通过；固定fa封存Data EXE实际remote329项与私有根TLS五项Go1.27.1对照已全部通过，多尾点SNI一致且证书拒绝保留。后续ac8正式Cargo remote329及固定Go五loopback也已通过；不能外推系统根/撤销全部策略。Emmy零补丁不等于整个依赖树零补丁。
- **Source 正确性与性能仍未验收**：5bbb完整配对中暖query20对等价且Rust/Go比0.202388，只有该组达到≤0.30；describe两组约1.13、cold query约5.00未达标，cold index因完整公共事实不同而失败。Metadata前置与SQL预算缺口已实际闭合。当前完整修复统一Go公共Facts字段/顺序、两个不同validate入口及完整类别gap缓存；native XML/Emmy语义增强保留在独立真实消费者，不用summary扣数掩盖差异。e843已通过Rust编译及4036文档逐字段/顺序0差；7f畸形输入52项已全字段差分通过，冷性能仍不达标。旧52与新增32 XML边界已完整差分通过；不将有限输入集冒称所有非法文本已穷尽。完整Retail refs/context/batch维持原90秒，旧dev仍deadline失败；优化408普通native refs/context/API已实际通过；2d7 runner/a338产品完整full14项已通过，包含身份变更/恢复和同owner；e843真实600秒lifetime已退出0通过。完整B01–B07、内存与五阶段审计均未验收。
- **正式 Lua 已明确切到 Go323**：`4092344`保存113原blob认证、精确Rust3.5 overlay与220/9正式入口，旧320/321/322原清单保留。Node正式63及包/版本115通过；本机已安装官方固定MSVC14.51与SDK，Community三工具字节/版本/签名均匹配原Enterprise固定身份；Community精确路径已实际验证。固定7c07的Go323正式Lua220/0/0与Rust桥9/0/0现已真实通过，全部owner回收；只是该自动化范围，不含实机或性能。

### 当前全仓架构实施批

e734已保存四个真实消费者边界，详细设计见 [DESIGN](DESIGN.md#whole-repository-rust-native-implementation-plan)：Core 借用型 pin 请求已接默认 Data provider；Data typed stream 已接 CLI、逐行执行和统一 wire renderer；Live 三态结果已迁移 connection/activation/doctor/CLI，清理合并消费原结果而不 clone；Source 集中原 Lua51 配置、八类诊断顺序和准入策略。Live 新增12项、Data/CLI新增13项、Core/Data新增10项、Source配置新增3项回归已接正式组；[7a实际13组](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6224181570)622通过1个Core阶段fixture失败0忽略，all-target通过，包含Live348、CLI69、adapter57及Source新26项；不重复累计子进程与52行输入。Node合同/身份45项实际通过。后续跨模块静态消费者曾60通过1失败，原因是旧322整体文件身份未纳入三态迁移；精确52编辑可逆认证补齐后62/62通过，原失败保留，旧manifest/Go输入字节未动。保留原预算、错误次序、输出和已提交事实；本批不代表最终五阶段审计已完成。

Source 冷链同时实施有界 scoped 多线程纯解析，按序提交并共同占用原 retained 额度；不把所有模块强行并发。Core/Data 现有所有权与单堆 SQL 保留，Live 输入状态仍串行。成熟库按实际能力、成本、许可与升级验证选用，不为架构名义另造空 crate 或通用调度框架。

## 3. 当前实际验证与证据

逐批结果、原失败及迁移前完整条目保留在[10月16日验证历史](docs/toolkit/rust-migration-evidence-2026-10-16.md)与[前期历史](docs/toolkit/rust-migration-evidence-2026-10-15.md)。不同版本、重叠过滤组不累计为当前唯一总数。

- **SQL/Data/Core**：ac8正式SQL299/0/0及原6个默认栈执行崩溃通过；local_query76/0/1，Data库1063/0/1。36集成原256通过1预算断言失败20忽略，后继[7db定向27/0/0](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6220137512)关闭该预算缺口。Core492/0/0含ABA；024真实request_tests8/0/0关闭7a定时fixture。0e1 typed Data默认consumer66/0/0；f088新增14纹理解码并行回归全部通过，性能未验收。
- **Emmy/TLS/正式Lua**：原版Emmy848身份不变。历史严格4137行仍有2条正向receipt差异；[当前合同检查](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6220362319)4135相同+2精确通过，历史raw失败保留。正式TLS329与Go1.27.1五loopback通过；Go323正式Lua220和Rust桥9通过，不能外推游戏或系统TLS全策略。
- **全仓架构消费者**：[7a的13组](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6224181570)622通过1个Core阶段fixture失败0忽略，后继024闭合该项。Live348、CLI69、adapter57及Source新26并行回归均在原组实际命中；0e1、f088后继各保原身份。专项通过不等于最终五阶段审计完成。
- **按需原文与retained结构回归**：8ee Git需求流17项真实Windows通过，原fallback fixture在[6ce完整facts_cache19/0/0](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6226082277)闭合，保4文档body扫描/cursor/禁镜像warm。76d all-target、native72及research76通过2原忽略，包含18个共享分析/流式摘要/索引/XML所有权新回归；匹配优化CLI构建成功。该树完整retained原失败保留；后继闭合见下一项。
- **2d7 runner/a338产品完整full14/0**：[本机完整结果](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6228204406)全部14项实际通过、exit0；变体pin真实准入、changed/restored inputIdentity和同owner均成立，owner e30c7e06-acf4-401f-a937-24e7134dce4e封存回收、active0、无forced/deadline。产品仍是精确a338，runner为2d7，不能归后继性能/项目选择源码。仍semantic.partial/155gaps，公开project参数修复已在30d另行实测；原a338与2c2的13/1保留在历史。
- 978/de903/02b/f40等逐批原失败、修复与当时NOT_RUN状态已完整移入[历史](docs/toolkit/rust-migration-evidence-2026-10-16.md#2026-10-17-retained-history-before-full-pass)；当前以2d7/a338的14/0及a338 native99为准，旧失败不改写。

- **正常Facts与有限畸形输入**：[76d完整4036文档v9](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6226409878)逐字段/顺序0差。后继[e8d新增XML32与旧52](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6226448382)全字段/顺序全部等价，Rust15/0/0、Node18/0/0；关闭该明确输入集差异，不称非法Lua/XML已穷尽。Facts v10/index v11及[显式v10读侧](tests/rust-performance-paired/read-fact-delta.README.md)已保存，[e8d独立新home完整4036 v10](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6226520186)逐字段/顺序0差，声明65320/关系227786/loads4169/headers1888/diagnostics0；报告SHA `722f3ea2ca4dfb2b3ba6b2aef4020d0a8bbc39cadd6fc50cf608b3c547d4a202`，匹配优化构建成功，未重跑retained/20对性能。
- **寿命与性能**：e843真实600047ms正常寿命退出0通过，原失败保留。76d单次等价诊断cold index Go8626ms/Rust9617ms、cold query9199/10107ms、warm query642/130ms；不是20对验收。旧[5bbb完整20对](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6221187576)仅warm query比0.202388达≤0.30，B01约1.13/cold query约5.00失败。新的完整B01–B07配对与内存验收仍未完成，单次改善不能覆盖旧统计失败。

### 实际155条semantic gaps的范围

已认证a338冷refs与暖context原始stdout，155对象逐项相同：load_file_missing17＋toc_load_target_unavailable17（同17个Classic TOC行的两层记录），load_condition_unavailable2＋toc_load_condition_unavailable2（同2行）；xml_template_unavailable65、xml_mixin_source_unavailable49、xml_parent_key_receiver_unavailable1、xml_native_or_parent_binding_unavailable2。两个scope都为alternative_manifest_hypothetical_after_load，不是客户端实际加载观察；template65仅21种位置/模板组合，mixin49仅21种位置/名称组合。不能把155说成155独立bug或全属动态Lua。最后两条真实来自StatusTrackingBar.xml:3的parentArray，当前实现直接记gap，属于可确认绑定覆盖缺口；其他项须逐消费者/可达范围核因，不能自动称全Go等价或全Rust回归。完整请求/响应通过不代表semantic.complete=true。

### 冷索引发布与流水线（正确性通过，单次性能未达标）

固定76d原始profile实核：总9.409秒，facts发布5.060秒/4036次；worker解析累计6.403秒与主线程重叠，不相加。当前实现仅在原资源边界内复用严格持有的locks目录，每次仍验证祖先、按原资源hash取得跨进程kernel锁并checked close；首次并发准备的多余目录owner明确关闭。收到最早已完成纯parse后，仍对Facts/原文字节/Ready槽计费，只释放已经销毁的parser scratch，允许下一份已读取/认证输入先进入空闲worker，再按原顺序提交上一份。原256MiB/槽上限、首错与线程回收保持。增加锁、暂存、写、seal、rename及两类close的分段profile，关闭profile时不读时钟。30d本机alltargets通过，正式pipeline28/0/0、profile2/0/0、facts_cache21/0/0、Core sourcefacts13/0/0、blobwait6/0/0均已通过；不得重复累加新增与继承测试。[30d匹配优化实测](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6228621264)全4036字段/顺序0差；单次Go8457.3107ms/Rust9551.7776ms，比1.12941075未达≤0.30。Facts发布4.931秒中锁1.594、stage1.496、rename1.314秒；write仅277ms、seal3ms。worker等待383ms，累计解析不与总墙钟相加。20对仍未执行，无已证加速结论。

### Data 专用配对（前4路径等价，清理失败阻断后继）

沿现有paired身份/owner/Windows原计时新增[Data入口与完整命令](tests/rust-performance-paired/data.README.md)，固定Go323完整优化包与匹配Rust优化artifact。独立受控preparer串行生成三个原fixture：微型SQL只核正确性；17MiB/三BLTE块真实内容读取；1031×519五种BLP存储模式分别默认PNG/WebP导出。逐样本验证完整行/顺序、源blob、全部像素、capture身份与有序依赖、原预算及真实冷热缓存工作。默认cold/warm各一对仅观察；显式acceptance内置单对preflight，再每性能路径20冷/20暖，SQL不冒性能样本。Node19/19真实通过，独审修正实际encodingIO布尔字段、完整byte-work及Go323 capture依赖三个问题；[30d实际结果](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6228602282)：三个fixture真实准备成功；SQL、17MiB BLTE、palette PNG/WebP共4路径冷暖8观察全部等价，16CLI均exit0/reaped。第4pair成功封存后scratch cleanup报EPERM，未到retired，原目录/owner/证据保留；后8路径与20对NOT_RUN。后继runner已改为已限长且完整认证的Buffer解码、关闭decoder缓存并finally销毁pipeline；尚未证明历史EPERM持有者。新增--resume-unattempted严格核原产品/输入/完整封存日志与前4完整等价路径，只用新owner执行剩8路径，不导入旧计时、不与20对模式混合。独立runnerRoot允许复用30d产品；Node26/26及独审通过。[d86 runner/30d产品实际续跑](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6228983589)前置52/52、剩8路径16冷暖观察/32CLI全等价且全部新owner清理回收成功。原4路径未重跑，旧owner不改不删；两批单pair不能拼成20对验收。BC1 WebP warm内存无完整样本，peak=null，CPU/I/O未测。完整新20对仍待执行。bbee修复独立runner在acceptance模式误定位旧工具；后继把每pair末尾输入完整性终态纳入汇总，保输出等价事实但末核失败不得判通过。SQL唯一pair、第20pair及中间pair故障注入与成功控制组Node29/29通过，Windows正式20对待新runner。准备、构建和测试仍由唯一Windows队列安排，不新增清缓存/改配额/下载路径。

### 公开 Source project 选择修复（新增12项实测通过）

refs/context省略snapshot时复用现有受控project/最近父级锁选择，显式snapshot仍优先。context把认证后的同一pin移交relations，避免cursor认证后再次读取可变project锁，保原control和绝对deadline。固定Go命令先解析数值、后project、再research selector的顺序已核；本片保context数值首错和project错误优先于selector。新增12 Rust＝2移交/控制单元＋9公开消费者回归＋1需要匹配包的真实CLI static/native显式ignored项。30d本机两单元2/0/0、完整refs_context14/0/3实际通过；[匹配30d release/package后真实CLI测试1/1通过](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6228578918)，新增12项逐名全通过；其余原ignored保持原边界。云端Node默认3通过1明确Go源码复核skip，指定持有固定Go323对象的checkout后4/4真实通过；核心3项不要求本机Rust仓有旧Go对象。普通产品consumer/cursor切pin与恢复、immutable pin认证已有上述Windows组证据；真实packaged static/native已在本轮命中。此修复独立于已通过的runner显式variant snapshot路线。

## 4. 进入最终五阶段前的有限收口

1. SQL结构性栈修复与预算回归已有实证，保完整实现供最终独立审计；当前集中闭合retained owner回复处理/回收和真实冷链全路径性能；已通过的52+32边界与600秒正常寿命保留原身份，不扩展新数据库或通用框架。
2. 在同草稿PR保存完整修复批，集中跑实际生产链与原全量比较；外层验证必须有界失败并让后续独立组执行，不提高产品deadline。
3. 验证Source短预算/暖态回收与完整Retail输出、诊断、性能/内存；未完成组清楚列NOT_RUN，不以unit或静态Retail替代。
4. 完成上述实现闭环后进入下节五阶段循环。继续以完整功能为单位实施与集中验证，不恢复逐几十行独审门禁。

## 5. 验收与循环审计

完整[F01–F13功能、P01–P14协议、B01–B07性能和Go来源合同](docs/toolkit/rust-migration-acceptance.md)保留；[321增量](docs/toolkit/rust-go-3.2.1-increment.md)、[322增量](docs/toolkit/rust-go-3.2.2-increment.md)与[323增量](docs/toolkit/rust-go-3.2.3-increment.md)是累积要求。代码已实现、静态已审、某树历史通过、当前实际通过必须分开。

- 固定Retail数据为Gethe/wow-ui-source `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b`：4036 Lua/XML/TOC，32,269,051字节。每次核身份后全仓分析/代表查询/源码定位/refs/context/flow/cold-warm/session/释放，不以小fixture代替。
- Source、Skill、Data分别由独立使用者执行真实CLI任务，记录输入、完整输出、正确性/可读性/来源/错误和复测。实现者不自评通过，无法运行时记blocked。
- 全量实现后的五阶段**顺序循环**：①架构/全业务/Skill/全部代码；②有效覆盖与无独立价值冗余；③真实消费者确认后的死代码；④前三维度联合全仓；⑤全项目Rust原生度。第五项是用户2026-10-16新增要求，核crate职责与依赖、所有权/借用、typed状态、clone与分配、流式、CPU/在途任务字节与背压、错误取消/RAII及unsafe边界，要求具体结构与实测证据，不以语法风格或无收益重写替代。历史四轮要求/报告保留原身份。每阶段独审→修复→实际验证→独立复审至无新可行动finding，再进下一阶段。每批源码审不等于最终收敛。
- Go323测试映射为当前差分oracle，Go322映射仍保留，320/321原证据保留；尤其未知结果不重放、事务、取消、空map、槽迁移与错误恢复。记录固定树、覆盖分母、平台排除与停止条件，不保证数学零bug。
- 性能必须同机同数据/结果与校验等价，独立cold/warm逐路径重复；可控耗时Rust≤Go30%，外部等待另列。当前仍未达成验收，不能把缓存预热或删校验当提速。

## 6. 本机接力：当前执行与下一批

本机测试通道已正常恢复，原unknown历史保留，无需再次apply或重启。仅云端实现，用户指定本机线程负责固定SHA测试并将报告写回同一PR。普通push不自动切换其正在测试的树；不另排重复长CI。

当前2d7独立runner/a338产品full14/0已安全终态，身份变更和恢复闭环；all-target/native99与匹配优化构建保持a338原身份。下一统一源码批先alltargets及受影响Source/Core/Data组，再匹配优化产物做profile和Data配对；不得将旧EXE当新性能产物。其余XML84/4036及quota/idle/lifetime保原身份不机械重跑。

- 所有Rust检查沿仓库受控 `tools/rust-cargo.mjs`、固定工具链和原owner/cache合同，先all-target编译，再受影响组；默认线程栈不加大。完整SQL299及7db预算组已过，不机械重跑历史失败树。
- 正式Lua与9桥使用[现有入口](tools/rust-formal-lua-bridge.mjs)，每次新输出目录；原runtime与CLI封存产物必须通过全部身份验证，不能覆盖旧失败目录。当前正式Go323220/9已有实证。
- 性能沿[专用配对入口](tests/rust-performance-paired/README.md)，独立优化构建、原20cold/20warm及完整输出等价。单次profile只诊断；索引/query180秒，refs/context/batch90秒，整棵retained分析树2GiB不变。不可扣除未测成本来宣称达标。
- 完整Retail/native沿[已封存候选入口](tests/rust-source-native-acceptance/README.md#current-workflow-and-separately-sealed-runner)，runner与产品各自认证，记录所有依赖未执行、退出码、精确子进程回收和内存采样缺口。最终五阶段循环尚未开始。

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
