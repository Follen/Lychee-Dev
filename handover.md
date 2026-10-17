# Rust 迁移交接：唯一当前事实来源

更新：2026-10-17 13:01 UTC。分支 `rust-refactor`，持续交付到[草稿 PR #14](https://github.com/Follen/Lychee-Dev/pull/14)。最新已核远端批次：`b3d24460c6586926314a0ed9e76e7e624ec6c014`。正常4036事实与84个固定边界输入已有等价实证；retained冷refs与跨CLI暖context均已通过；2d7 runner/a338产品完整full14项已通过，身份变更与恢复、同owner均成立；partial155保持。公开project选择新增12项含真实packaged已通过。性能与全量验收未完成。

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

### 4a1 三模块优化前置与两项测试修订

4a1 已保存 Source XML/TOC typed parse、Data 请求内 Metadata owner 复用、Live owned projection。[正式前置结果](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6231331899)及[原证据](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6231332319)：all-targets通过，Source facts39/index68/lua19、Core evidence14/capture9/open19、Data export5/local83加1原忽略/admission14加1失败、Live358加1失败，总628通过2失败1忽略；新41为40通过1失败，迁移3/改名3均过，12个owner已回收。Source后续补验2个测试的84条完整Go差分通过，最初仅观察运行不重标。ab4只修下述两个fixture，本机实际remote admission 15/15、Live主组359/359均通过，原两失败逐名通过，嵌套子进程不重复累计；两个owner已封存回收，[ab4正式终态](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6231683736)已出。唯一匹配release及assembly成功，EXE 44541952B / SHA256 `3b124671486c476e55fabf8358d737ebe541005111585837f6ce03a609f95e47`；不是发布或npm smoke。旧7c Lua解释器报告绑定旧owned-process两文件身份，当前正式verifyInterpreter明确要求当前SUPPORT及原sealed build receipt一致；已在新目录用固定原tar和当前正式build入口重建并认证、回收成功，旧报告/清单未改；正式Lua9桥已在新runtime/匹配候选通过9/0/0，所有真实case root退出0并回收；未操作客户端。

两处失败已定位并仅修测试，修订已由上述ab4正式组实测通过：Data 原50ms在初次SQLite schema持久提交前开始，失败于open_metadata而未到目标acquire；现先用原control建立真实owner，再启动同50ms执行实际被占用的admission，保probe零调用、原owner仍持锁、原control正常及释放后成功准入断言。Live新测试从ResultVerified进入resume需要业务release，而所用recovery Peer只支持bind/unbind；现从真实Complete走disconnect/unbind，仍注入native checked-close失败，严格保完整raw report、持久关闭/无事务、一次打开关闭及owner claim已释放。生产实现、Peer防线、预算与deadline均未变；原4a1失败保留，当前通过仅归属ab4对应正式组。

逐批结果、原失败及迁移前完整条目保留在[10月16日验证历史](docs/toolkit/rust-migration-evidence-2026-10-16.md)与[前期历史](docs/toolkit/rust-migration-evidence-2026-10-15.md)。不同版本、重叠过滤组不累计为当前唯一总数。

- **SQL/Data/Core**：ac8正式SQL299/0/0及原6个默认栈执行崩溃通过；local_query76/0/1，Data库1063/0/1。36集成原256通过1预算断言失败20忽略，后继[7db定向27/0/0](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6220137512)关闭该预算缺口。Core492/0/0含ABA；024真实request_tests8/0/0关闭7a定时fixture。0e1 typed Data默认consumer66/0/0；f088新增14纹理解码并行回归全部通过，性能未验收。
- **Emmy/TLS/正式Lua**：原版Emmy848身份不变。历史严格4137行仍有2条正向receipt差异；[当前合同检查](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6220362319)4135相同+2精确通过，历史raw失败保留。正式TLS329与Go1.27.1五loopback通过；Go323正式Lua220和Rust桥9通过，不能外推游戏或系统TLS全策略。
- **全仓架构消费者**：[7a的13组](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6224181570)622通过1个Core阶段fixture失败0忽略，后继024闭合该项。Live348、CLI69、adapter57及Source新26并行回归均在原组实际命中；0e1、f088后继各保原身份。专项通过不等于最终五阶段审计完成。
- **按需原文与retained结构回归**：8ee Git需求流17项真实Windows通过，原fallback fixture在[6ce完整facts_cache19/0/0](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6226082277)闭合，保4文档body扫描/cursor/禁镜像warm。76d all-target、native72及research76通过2原忽略，包含18个共享分析/流式摘要/索引/XML所有权新回归；匹配优化CLI构建成功。该树完整retained原失败保留；后继闭合见下一项。
- **2d7 runner/a338产品完整full14/0**：[本机完整结果](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6228204406)全部14项实际通过、exit0；变体pin真实准入、changed/restored inputIdentity和同owner均成立，owner e30c7e06-acf4-401f-a937-24e7134dce4e封存回收、active0、无forced/deadline。产品仍是精确a338，runner为2d7，不能归后继性能/项目选择源码。仍semantic.partial/155gaps，公开project参数修复已在30d另行实测；原a338与2c2的13/1保留在历史。
- 978/de903/02b/f40等逐批原失败、修复与当时NOT_RUN状态已完整移入[历史](docs/toolkit/rust-migration-evidence-2026-10-16.md#2026-10-17-retained-history-before-full-pass)；当前以2d7/a338的14/0及a338 native99为准，旧失败不改写。

- **正常Facts与有限畸形输入**：[76d完整4036文档v9](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6226409878)逐字段/顺序0差。后继[e8d新增XML32与旧52](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6226448382)全字段/顺序全部等价，Rust15/0/0、Node18/0/0；关闭该明确输入集差异，不称非法Lua/XML已穷尽。Facts v10/index v11及[显式v10读侧](tests/rust-performance-paired/read-fact-delta.README.md)已保存，[e8d独立新home完整4036 v10](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6226520186)逐字段/顺序0差，声明65320/关系227786/loads4169/headers1888/diagnostics0；报告SHA `722f3ea2ca4dfb2b3ba6b2aef4020d0a8bbc39cadd6fc50cf608b3c547d4a202`，匹配优化构建成功，未重跑retained/20对性能。
- **寿命与性能**：e843真实600047ms正常寿命退出0通过，原失败保留。76d单次等价诊断cold index Go8626ms/Rust9617ms、cold query9199/10107ms、warm query642/130ms；不是20对验收。旧[5bbb完整20对](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6221187576)仅warm query比0.202388达≤0.30，B01约1.13/cold query约5.00失败。新的完整B01–B07配对与内存验收仍未完成，单次改善不能覆盖旧统计失败。

### ab4 三模块优化实测边界

[完整4036 v10及同run Source分账](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6231623390)字段/顺序0差。单次native Rust9918.747ms/Go10654.6351ms，比0.9309326，未达≤0.30，也不是20对验收。bbfe对应7.905/8.546秒比0.92505；两侧跨run都变慢，不能把Rust绝对增时直接归因某一改动，也不能宣称typed解析已提速。新增1473次worker与减少1473次串行解析对应；串行仍2095.311ms/685次、worker等待1003.759ms/3374、abandon仍23次，剩余重路径需按实际格式/任务成本继续归因。

同run非重叠主线程账（毫秒）：metadata0.349、index锁0.673、git tree109.393、facts读346.910、串行parse2095.311、facts序列化514.373、facts发布4038.019、original24.041、symbols146.331、worker等待1003.759、Git/流水线未细分377.847、index序列化645.354、index发布102.806、manifest7.893、profile内其它252.989、profile外252.699，合9918.747。未细分余量不是CPU或物理磁盘标签；worker累计parse和发布子桶均不再相加。Facts发布的7个锁字段全部0，stage1782.109ms、rename1714.018ms仍为嵌套子段。

同批Data正式诊断48测量CLI加15准备、24个cold/warm行完整结果/校验等价，全部测量exit0并回收，12pair全部retired；24份Rust11字段profile和48份startup有效。仅单pair观察，不能替代旧22组20对性能失败。内存46次successful、2次Go cold样本有gap，不补样。Capture仍每次durable事务，连接owner复用收益尚须按同run完整分账核实；不得用仅6–17ms的capture范围推断已达标。原4a1两失败、旧runtime身份失败、历史严格差分及EPERM均保留。

### 下一步：现有双Rust产物相邻诊断

已新增[专用入口与操作说明](tests/rust-performance-paired/README.md)：固定bbfe/ab4两份原sealed优化产物、各自完整匹配源码见证，原artifact路径/记录不改；独立导出18个runner依赖。固定3对AB/BA/AB，每次独立application-cold home，原180秒产品/210秒调用/默认栈不变；两调用紧邻，完成后验证4036完整Facts所有字段/顺序及完整31phase，再封存/清理。失败不替样、未知回收停后继。外层固定60分钟，计入准备/身份/对照/收尾。Node定向47/47（0skip）已由作者及协调者各实际运行；Windows实际前置在bbfe源码见证digest不匹配处停止，6次产品调用均NOT_RUN，3个owner已回收（[原阻塞报告](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6232021715)，[证据索引](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6232021948)）。bbfe EXE/artifact匹配，clean见证1061文件逐Git blob匹配，但计算digest为cc35ac5c386a45010f90f1f518720800c5a0c35f721b6e73f76616a281f2194e，原artifact要求76bea4b27798356c9f5194f56c97c3a1292b214166b84c875dfc0bb75dfc7b34；必须核原构建输入清单/算法，不重标产物。ab4身份通过。本机Node导出遗漏先46/1、补齐后47/47，原失败保留。三对仍不是20对验收，不重建产品。另扩大既有runner测试时两项因云端固定Go对象缺文件失败，未更改原测试/未冒全套通过。generated完整guard/字段算法及Data私有流式解压由后继生产批保存，不混入b150 runner-only身份。

### Source 有界 Facts cohort pack 后继

当前实现已贯通默认index构建与XML/TOC等懒读取：按原eligible顺序每≤64文档形成确定cohort，每pack≤原32MiB，magic/定长header/完整v11 envelope逐条写入原私有stage，不聚合第二份pack body。Document持固定groupKey/offset/length/hash locator，按文档Vec实际capacity与cohort显式计入原256MiB准入；原worker cap/槽数不增。仅确认Published且writer/reader checked-close完成后最终Index可提交，Unknown保持错误不重放。范围读取验证header/key/长度/SHA与完整envelope；坏/缺缓存回认证Git、原parser和per-file修复。跨过pack byte cap先提交前缀，剩余走原per-file；原单record上限不扩大。

这是可重建缓存内部checkpoint调整，未完成组可重算，不损失源文件/业务capture。eligible成员及排序固定时，单内容变更最多连带63同行重算；插删/重命名可移动后续分组，最坏重算后续整个corpus，明确保留这项成本。正式回归含128原组命中、单改、前端插入129/删除127重建和旧cachebackfill；性能必须同时观察cold与跨snapshot，不只展示cold。

正式facts reader新增显式`v11-pack1`及本run真实index blob路径/SHA，禁止扫描目录猜旧pack；历史v6–v11读者与旧报告保留。无全局locator/GC/新Windows接口，祖先身份验证未跳过。完整独立静态审阅发现的一处Document测试字段遗漏已修，未剩确认生产缺陷；[b3d实际前置](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6233354114)all-targets通过，Corestream10/0/0、Sourcepack11/0/0、retainedadmission1/0/0、旧cache26/0/0、Node64/0/0；22个新命名入口逐名核，其中1个惰性子进程helper不重复算独立断言。旧104严格全同，boundary11仍5相同/6精确endLine增强，10owner回收。[匹配release/assembly](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6233470665)已实际成功并回收，EXE SHA `bc8f67991a98d607ec7a0ca08293e8c34abe4ef2d1122c8b344ddd01a29c3e64`。[本机随后确认](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6233513933)同run冷index Go9199.0487ms/Rust5873.878ms，完整4036字段/顺序0差；单次比约0.6385，未达≤0.30且不是20对验收。暖Rust509.5789ms已执行，但临时编排错误套用cold的git_tree校验，完整暖等价尚未完成。另一取证失败是临时脚本将82,228,560字节完整index直接交16MiB单文件seal，触disk.log_budget；产品进程已回收，原scratch封存未终态，不能写成产品失败或已清理。正在补正式流式分片与冷暖/变更模式验证，保原文件、SHA和已完成调用，不提高上限、不重跑已完成产品。单改/插删仍NOT_RUN；Data v3独立实际终态见下一节。已补[正式证据与续验入口](tests/rust-performance-paired/SOURCE-PACK-EVIDENCE.md)：原82MB按64KiB流式读取、8MiB分片及整件SHA复验，新custody owner仅封存自己的文件工作，旧receipt/大文件不改；正确cold/warm/changed模式不修改历史bbfe/ab4冷判据。Node15/15由作者和协调者各实际通过，23文件隔离导出另15/15通过。当前帮助程序本机NOT_RUN；它不重新证明原launch身份，不把cachedIndex冒充完整Facts等价，也不自动重跑/改变样本。

### 固定历史双Rust witness后继

[历史恢复实际报告](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6232226662)已逐字认证7个公开原始文件，独立bbfe/ab4 witness的完整sourceDigest均匹配原artifact，但Git七项未暂存M触旧clean条件，6个产品调用仍NOT_RUN，原失败owner封存回收。现最窄专用入口将该已证历史输入合同显式化：固定HEAD/artifact/EXE/原完整digest、七路径逐size/SHA、其它Git状态必须clean；通用verifyCandidate仍严格clean，不改Gitconfig/index/旧artifact，不接受任意dirty。报告保真实dirty与artifact-bound-witness标记。32项Node通过；5d4本机alltargets/Core19/Data10/resource37/Node62已通过，部分Data组覆盖重叠不重复累计，见[实际前置](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6233193157)。历史witness认证及双Rust3pair已实际完成，见[主报告](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6233251030)与[完整终态](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6233277808)。三对bbfe/ab4原native分别8188.601/8465.1212、8100.1353/8144.7837、8080.0792/8211.0037ms，完整4036及31phase均通过；中位耗时比1.013687，逐对ratio中位1.016203，ab4三对均未更快，XML/TOC并行批没有观察到端到端收益。6份内存均有采样缺口，整机27–80%busy不证明CPU单因。13owner回收、37证据段齐，旧b150失败及6NOT_RUN保持。此为两版历史Rust3对诊断，非20对Go验收，也不代表后续generated或pack候选性能。正式固定命令和反例范围见[专用说明](tests/rust-performance-paired/SOURCE-CANDIDATE-WITNESS.md)，不能用它放行其它产品或重标旧失败。

### Data publisher v3观测后继

已完成原publisher十个固定子段观测，保原11业务段与v2十detail，默认关闭不增加clock。没有删除读/哈希/事务/校验/收尾；source→spool、spool→shard、finalverify各自为inclusive；成功路径winner-open在返回File时结束，随后才进入finalverify，两者互斥（先前文档误写嵌套，现按固定b3d源码及assert_not_nested测试更正）。业务11段与publisher子段仍不能跨层相加，错误分支close保各自inclusive边界。原48份v1/v2 stderr已由新reader实际读取且不改对象；Node30/30、七Rust文件rustfmt解析及独立静态核通过，新3项Rust已在5d4真实前置逐名通过，Corestaged19、Data phase10/resource37均通过（覆盖重叠不重复累计），[b3d v3真实主报告](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6233589545)和[终态](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6233633639)：12路径24冷暖行全部等价，48CLI exit0/计时有效/回收，12pair及全部14owner回收；24份profile含264业务段、240 detail、240 publisher，原0保留。内存46成功2gap（Go palette PNG warm、Rust BGRA8 WebP warm），不补样；单pair不是20对验收。24证据段692文件/545唯一blob共17,153,184B逐SHA核，48native原时间、441日志引用和48内存块认证，24行互斥账加回各自原T。cold BLTE原T228.0321ms；同row publisher103.289ms=source→spool35.585（CKey20.956、inflate1.050、read0.067、块MD5 0.206、其余13.306）+spool→shard16.959+finalverify10.709+seal30.570+其它9.466。warm原T125.5419ms、两次cacheverify合34.888ms，无decoder/publisher。seal四次合并值只是待核上界，不代表全部可删；不把嵌套子项与父项重复相加，也不由跨版差值宣称流式收益。完整字段和计时限制见[Data正式入口](tests/rust-performance-paired/data.README.md#publisher-diagnostic-scopes-v3)。未引入OpenSSL/CNG或更换MD5依赖，现阶段不凭21ms哈希子段贸然增加原生依赖面。

### Data 私有spool typed read-ready后继

已实现独立PrivateSpool，限制为同命令临时原handle，不能publish/replace/Deref/取inner；完成写入后只读与checked discard，不请求私有sync。最终shard stage原sync_all、三次内容读/哈希/EOF检查、业务capture独立durable事务与原控制/预算保持。显式内部差异：仅原私有sync会观察到的错误不再请求，实际write/read/seek/finalsync/close错误保全，不能宣称所有错误时序完全相同。无新FFI/unsafe/全body分配，无跨命令恢复/后台推迟成本。

v4把原stage_seal拆为private_spool_read_ready/final_stage_seal，保11业务段/10detail及v1–v3严格reader；已逐字复读历史96份profile（48v1、24v2、24v3）。[e253真实前置](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6233947994)alltargets通过，Windows staged28/0/0、Core staged24/0/0、Data phase10/0/0与resource37/0/0、compile-fail doc1/0/0单列；12新增逐名各一次（Win5/Core5/Data2），Node31/0/0、7owner回收，组间覆盖重叠不累加。独立静态复核无确认缺陷，11Rust解析亦过。[匹配release/assembly](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6234058245)已成功并回收，EXE 44,704,256B、SHA `ad2d17fe4a1431ee3cadfd18235ceb8df6ab35d55dc339be02993c091cff3aa2`；[v4主报告](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6234095130)及[终态](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6234117922)48CLI/24冷暖行全等价、计时/profile有效、14owner回收。48内存均sampled_successful_run，仍为采样下界而非完整峰保证。26段812路径/626唯一blob共17,662,793B认证，24行互斥账加回原T。cold BLTE T201.3286ms、warm123.2666ms；同row publisher83.255ms=三pass61.959ms（35.668/15.822/10.469）+私有read-ready0.002ms/2次+最终durable seal11.711ms/2次+其它9.583ms。CKey21.002ms在source pass内，不再次加。warm两cacheverify35.395ms，无publisher。图像三capture冷32.320–40.024ms/暖30.893–33.397ms；24行没有≤0.30。相较b3d单次cold228.0321ms有端到端下降观察，但条件未控且只有一对，不能宣称因果节省或20对达标。本批按[正式Data说明](tests/rust-performance-paired/data.README.md#private-spool-and-final-seal-diagnostic-scopes-v4)验证和一次匹配构建已完成；原30.570ms四次seal历史合计不重标，完整elapsed/内存目标仍未验收。

### 8cab 同run性能诊断终态

[Source31段](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6232776993)和[批主报告](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6232891216)：完整4036字段/顺序零差，Rust8015.3771ms、Go10113.785ms。git_blobs6886.849ms内互斥为read324.377、serial parse1189.303、serialize471.953、publish3649.523、original26.774、symbols134.061、worker wait721.033、未细分369.825ms。外层已测合7560.288ms，profile7781.565ms另留221.277ms未埋点，native与profile相差233.8121ms仅是测量边界差。worker累计5095.419ms重叠，不相加。3983次worker尝试减23 abandoned=3960采用，加76串行=4036；609小generated均进入worker，其成功数仅能界定586–609，不能从aggregate推断23个失败所属模式。发布stage1740.808＋rename1426.979ms为最大已定位部分，locks全0、seal3.210ms。

[Data v2终态](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6232888659)：12路径48CLI、24cold/warm行全部等价，12pairs及outer回收，24×11业务段和240嵌套detail、48startup均有效；内存Go20成功4gap，Rust15成功6gap3未测，保原缺口。43段包1533引用/770唯一blob已逐SHA核验，24行互斥账均回原总T。cold BLTE总221.3877ms，prepare51.074＋payload98.426ms；跨Root与payload的publish98.910ms包含read0.069、encoded MD5 0.199、Z refill1.000、whole CKey21.673和其余75.969ms，不能把该publish全算payload独占。warm总123.8675ms，cache verify34.879ms跨两次，payload39.358ms。Z refill包含控制/工作/EOF检查，不是纯CPU；warm无decode。24单次ratio最小0.4474，全部未≤0.30；不得用跨run差值或不完整内存样本宣称流式收益。原正式20对性能失败仍保留。

### 公共 Facts 缓存身份后继

8cab纠正括号声明/参数、操作符诊断及跨行位置后，仍沿用旧index/facts身份，旧合法缓存可能继续返回9a旧结果。此真实一致性缺口已补最窄后继：index fingerprint v12，facts key/layout/envelope v11-go323-generated-origins；公开source-v1、generator-v6和完整index container v6形状不变。query/readiness/session/research/relations消费者继续派生统一fingerprint。旧文件不改写、不重标。

新增2个Rust真实写盘迁移回归，按9a封存原行重构合法旧缓存与旧whole-index，验证拒绝旧身份、完整重建、当前envelope/warm命中、query manifest及readiness迁移；不冒称这些重构文件是原捕获文件。正式read-fact-delta新增显式v11，保历史v6–v10与默认v6；新reader改变新runner输入身份，旧runner/receipt保持原字节。[afbeb实际验证](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6232978472)：all-targets通过，完整facts_cache26/0/0、两项新迁移逐名通过，Node56/56，六owner回收；[原证据](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6232979009)保留。此后继不打断正在使用8cab产物的Data诊断，后续新产物和reader须分别核准身份。

### 8cab 完整前置与真实位置增强

[8cab本机实际前置与完整6项位置差异](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6232595033)：all-targets通过，Source lib603通过、0失败、4原忽略；旧52＋XML32＋generated20合104个固定Go输入严格逐字段通过，Node7/7。新增boundary11真实观察中5项全部相同，6项仅endLine不同：Go为0，Rust保留实际table body结束行，且匹配预先声明的完整候选快照；其他字段及错误顺序相同。依据用户保留经证实更准确结果的要求，将这6项记录为真实位置增强，原Go观察和完整差异继续保留，不重标为11项严格相等。四owner已回收。证据索引尚待原测试线程生成，当前主报告包含完整位置差异。

原9a四失败及旧参考原文均保留；8cab关闭对应正常括号声明/参数、诊断首错与混合队列测试缺口。4个ignored不计通过。唯一匹配release与assembly随后实际成功，两owner回收，EXE SHA256为`c73a5cdac405d62593e82ab0fbea515c81be94bc9d16d441ad6956ccfd5bf4b4`。4036完整字段/顺序差分随后实际0差。单次同run Rust8015.3771ms、Go10113.785ms，约0.79252，仍未达到≤0.30；串行parse1189.303ms/76、facts发布3649.523ms/4036、worker等待721.033ms/3983。完整31段原报告链接待补，不能将这三项当完整总时间分账，也不能由跨run串行时间下降单独宣称优化收益。Data v2仍在原队列中，尚无新结果。

### 9a 真实 Source 后继与差异分类

[9a实际终态](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6232309345) all-targets通过，Source lib597通过、4失败、4原忽略；新增19为18通过1失败，256扫描/取消回归通过。旧52＋XML32完整Go差分通过；新20与优化前Rust生成算法参考全部相同，但固定Go严格对照只有18项相同，2差异保留于[完整报告](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6232293175)。parenthesized表entry/参数被旧Rust忽略，是合法观察遗漏；unsupported输入两端均清空Facts，但Rust通用诊断遗漏Go token/column。它们不能自动称更优增强，也不能修改输入或规范化报告求通过。后继需保旧参考原文与实测差异，准确修正常语义。

其余3失败是混合队列测试仍将generated当作Sequential填槽；新生产路径已允许它进入worker。全eligible的4-worker队列不应同时要求峰值>4。后继保全部40份正常四模式输入及完整raw/Facts对照，另用明确≤64KiB非UTF8输入验证真实Sequential pending、错误顺序、满8槽和Ready持槽；>64KiB生产分支会drain后同步执行，不能替代该测试。没有改生产预算/并发上限。两owner已回收，release/assembly/新性能仍未启动。

跨行括号声明的起点与声明体结束位置另外核证：候选保最外透明括号的起始位置和实际table body的结束位置。固定Go的tableEnds行号队列可能在括号跨行时丢失真实endLine，目前这是源码推导，尚无新增跨行真实观察；后继需独立固定Go报告确认并明确记录准确位置增强，不能冒充原20个单行输入已通过，也不为重现Go队列缺陷制造另一套位置模型。原20输入、旧参考和历史失败报告保留。 当前后继已实现括号entry/参数及源码叶序binary //诊断，保19项旧生成算法完整相等并对第20项保存旧行、另核已封存Go正确合同。新增独立boundary11含6跨行位置与5首错输入，候选完整Facts快照/worker/serial对照已写；实际固定Go观察与Rust执行待本机。Node身份7/7、4个Rust文件rustfmt解析、94个format模板静态检查和diffcheck通过，不等于产品测试通过。

### caa 首轮结果与最小编译修复

[caa实际前置](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6232136327)：workspace all-targets因 `lua_tree/generated_tests.rs` 的fixture `format!` 含未转义右花括号编译失败，Cargo101，owner回收，无timeout；rustfmt只解析语法，先前静态检查不等于宏展开编译通过。Source19新项、扫描回归、旧84及新Go20候选严格对照均未执行；Go20真实观察报告已生成封存，后继直接复用，不重跑或换输入。此处最小后继只修fixture构造，所有原比较/断言不变。

独立Data BLTE19/19、profile7/7、Core staged17/17实际通过，新增Data/Core19逐名命中；Node5/5加29/29、0skip。它们不被Source编译失败抹去，也不代表全workspace通过。release、assembly、4036及新性能未运行，旧性能失败保留。

### 下一生产批：generated 借用解析与 Data 私有增量 Z

Data 本地/CDN 的已认证 publisher 已接入增量 Z：保存已验证的 encoded chunk，以固定64KiB缓冲解压到未发布私有spool，删除 decoded-sized Vec。原 encoded MD5、最终decoded大小/Adler、整份CKey/source EOF在发布前校验；非Z/嵌套/加密及公开 eager Decode/unpack 保持原路径。原每块保守 retained 准入时点和持有期保持，因此不能声称账面峰值已下降；有效输入的完整bytes/hash/work计量须实际对照。

私有spool明确采用首个已观察错误：sink失败立即停止，不继续inflate坏尾或下块来重建旧 eager 双故障顺序。故“未观察坏尾＋早期写失败”和紧work预算组合可与旧内部错误先后不同，公共坏块零输出合同不变；没有发布部分spool，原取消与checked cleanup仍保留真实错误。详见[设计边界](DESIGN.md#private-streaming-decode-and-observable-errors)。新增19 Rust覆盖正常buffer/chunk边界、真实publisher、完整认证、预算/取消、sink/cleanup及历史fallback，现已在caa独立三个完整组逐名实际通过；发布时9个Rust文件rustfmt仅解析、独立静态复审及Node29/29记录仍保原边界，性能未运行。

Data opt-in profile v2保留原11段和总边界，另列10个嵌套detail，严格兼容原v1报告；默认关闭无额外clock。下一匹配产物沿原正式入口执行，旧928正式样本与48次诊断不改。Source generated借用字段视图、显式文档任务栈及256扫描工作步观察粒度已完成，完整guard允许原64KiB内的generated纯解析进入同一有界worker。发布时新增19 Rust及2个原测试重命名/扩展尚未运行；随后9a实际新增19项为18通过1失败，具体完整组与两项Go差异见上节。8个Rust文件发布前仅rustfmt解析通过，新fixture Node3项加旧XML身份2项实际5/5通过。临时容量推导、默认栈深树、分配/取消出口及[22个正式测试全名与Go命令](tests/rust-parity/source-facts-generated/README.md)已记录；其20个新增固定Go输入保持原文，旧Rust参考与新算法等价、固定Go实测是两条独立证据，疑似旧差异不预先豁免或换fixture。

### 实际155条semantic gaps的范围

已认证a338冷refs与暖context原始stdout，155对象逐项相同：load_file_missing17＋toc_load_target_unavailable17（同17个Classic TOC行的两层记录），load_condition_unavailable2＋toc_load_condition_unavailable2（同2行）；xml_template_unavailable65、xml_mixin_source_unavailable49、xml_parent_key_receiver_unavailable1、xml_native_or_parent_binding_unavailable2。两个scope都为alternative_manifest_hypothetical_after_load，不是客户端实际加载观察；template65仅21种位置/模板组合，mixin49仅21种位置/名称组合。不能把155说成155独立bug或全属动态Lua。最后两条真实来自StatusTrackingBar.xml:3的parentArray，当前实现直接记gap，属于可确认绑定覆盖缺口；其他项须逐消费者/可达范围核因，不能自动称全Go等价或全Rust回归。完整请求/响应通过不代表semantic.complete=true。

### 冷索引发布与流水线（正确性通过，单次性能未达标）

固定76d原始profile实核：总9.409秒，facts发布5.060秒/4036次；worker解析累计6.403秒与主线程重叠，不相加。当前实现仅在原资源边界内复用严格持有的locks目录，每次仍验证祖先、按原资源hash取得跨进程kernel锁并checked close；首次并发准备的多余目录owner明确关闭。收到最早已完成纯parse后，仍对Facts/原文字节/Ready槽计费，只释放已经销毁的parser scratch，允许下一份已读取/认证输入先进入空闲worker，再按原顺序提交上一份。原256MiB/槽上限、首错与线程回收保持。增加锁、暂存、写、seal、rename及两类close的分段profile，关闭profile时不读时钟。30d本机alltargets通过，正式pipeline28/0/0、profile2/0/0、facts_cache21/0/0、Core sourcefacts13/0/0、blobwait6/0/0均已通过；不得重复累加新增与继承测试。[30d匹配优化实测](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6228621264)全4036字段/顺序0差；单次Go8457.3107ms/Rust9551.7776ms，比1.12941075未达≤0.30。Facts发布4.931秒中锁1.594、stage1.496、rename1.314秒；write仅277ms、seal3ms。worker等待383ms，累计解析不与总墙钟相加。20对仍未执行，无已证加速结论。 后继仅增加五个锁内观测段verify/open/kernel attempt/contention backoff/failure close，旧26段保序；失败close与成功LockClose分开，无竞争不计backoff，所有子段嵌套不能加总。保原open flags/逐次祖先验证/取消/真实句柄与checked close，关闭profile不增加clock。d090本机alltargets与四个完整组57/0/0通过，八个新Rust回归逐名称各命中一次，owner封存回收；此前工具Node6/6保持原身份。[d090匹配优化构建/assembly和完整4036差分](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6229841252)已通过。单次cold Go9564.4355ms/Rust10636.1542ms，比1.1120524782未达标；锁open1577.532ms/4036、verify179.033ms/4036、kernel14.614ms/4036、backoff0次、failureclose0次。成功lockclose86.270ms另列，子段不与parent相加。明确本样本无锁争用退避，open时间仍含系统调用/路径处理，不能当物理磁盘时间。后继held-directory relative OPEN_IF窄候选保旧逐doc缓存/checkpoint和完整控制/checkedclose，7新增Rust待实际验证；内容键仅37/4036重复不优先，分段缓存文档仅候选未实施。

### Data 专用配对（正确性与回收通过，22组性能全部未达标）

[5bc runner/30d产品完整正式结果](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6229610505)实际执行928测量CLI＋15准备调用，232 pair owners与464冷暖行全部本轮新建；22行性能预检、440行正式性能、2行微型SQL正确性分开。11条性能路径各20冷/20暖，全部完整内容/像素/capture/缓存/预算断言等价；928 CLI均exit0且回收，232pair均retired。outer `5cd2078b-11f3-46be-9827-02d96767bb6b` exit1/failed仅因22组全部未达Rust/Go≤0.30，最终无forced/deadline、active0。不能将正确性通过称性能验收通过，也不外推完整B04或SQL吞吐。

正式中位比cold/warm：17MiB三BLTE块1.029283/0.798972；palette PNG1.134704/0.999569、WebP0.680457/0.555488；BGRA8 PNG1.077913/0.948885、WebP0.731375/0.576258；BC1 PNG1.067895/0.983206、WebP0.717355/0.578054；BC2 PNG1.081733/0.903294、WebP0.717622/0.594889；BC3 PNG1.104392/0.924568、WebP0.730342/0.582489。保持原CreateProcess→root/tree回收时钟与全部慢样本，无下载/外部等待扣减。暖Rust约112–125ms包含完整进程生命周期，不能直接归因为启动成本；本轮未启startupDiagnostics，旧数据无法事后恢复该细分。

13条内存缺测（Rust12、Go1，均正式暖样本）原样null、未补样；其余915条仅有间隙的owned Job采样下界，CPU/I/O未测。产品EXE `cb5599490c23d27bbe48e43f4baa482680d67051a858a12b872bbce8cf2cf186` 未重建；Node55/55。report SHA256 `1d5c0f49a61228afea5f61e02f8c9221a48321250d0a7aa3f95d31ae29aa1f0b`；[97段完整证据索引](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6229628728)保全部原样本。30d原第4pair EPERM现场/receipt不变，d86剩8路径续跑和本次新acceptance不追认旧owner已清理。原单pair历史保留在[验证历史](docs/toolkit/rust-migration-evidence-2026-10-16.md#2026-10-17-data-observations-before-complete-acceptance)。完整97段已还原9748路径/7021blob并逐SHA认证，928样本/24组统计复算一致，9455封存日志引用全部核过。root退出被观察后到tree/drain中位Go0.463ms/Rust0.669ms，不是约100ms整体耗时的主因；启动与产品阶段尚待分清。现新增独占--startup-diagnostics沿原owner计时，12路径各1冷/1暖共48测量CLI＋15准备，完整等价/回收不变，不与acceptance/resume混用，缺proof单列诊断失败而保产品结果。Node29/29通过；[9de runner/30d产品Windows诊断](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6229910794)48测量CLI＋15准备、24冷暖行全部等价，48startup proof有效，outer和12pair全部回收。两产品和8个非conditions输入与旧样本相同，第九conditions为新诊断副本。Rust preResume20.2591–35.1257ms，Resume至reap66.1001–180.9937ms，后者含调度/loader/runtime/业务/清理，不是纯产品CPU；rootCPU为15.625ms量化。46内存sampled_successful、2有gap，无完全缺测，不能外推完整RSS。原23段666文件/498blob、635封存日志已独立SHA验证。共同Data业务phase观测已实现、实际复验仍待；保48单pair与旧928正式样本各自身份。既有20对总时钟/判据不改，不能事后减掉启动或把诊断当性能验收。

### 当前 Source/Data 窄优化与真实所有权修复（首次编译失败，后继待复验）

Source仅将受控Facts锁的open替换为held Directory相对OPEN_IF，精确保read/write、share read/write/delete、非截断和原leaf策略；File先转交Core再原取消检查/checked close。其它caller仍旧OpenOptions，逐文档缓存/恢复/错误合并与原等待不变，无新extern/unsafe。7新增Rust覆盖真实争用、取消关闭、失败/目录映射、非截断、共享rename及held-parent/decoy路径；静态23项、rustfmt/diff过；d69实际Core storage55/Facts14通过，Windows destination28通过1失败。失败是held父目录下Win32全路径rename第73行error32，不能据此认定叶子share差异；本后继仅修fixture，增加旧OpenOptions同失败对照、DELETE-access peer及只关闭父目录后两叶子原句柄完整rename/readback，生产flags不变。466实际该旧rename修正已通过；父目录与叶子共享对照真实命中。改善范围只是原1.578秒open中的完整路径处理，不能承诺省去整段。

Data两个真实Root消费者接收同一已认证materialization owner返回的BlobRef，保原checkedclose后才交付，删除再次publish/hash/reopen。新增默认关闭且不读时钟的11段query-owned profile；capture/metadata为真实inclusive，不假分SQL连接内部，沿现Budget共享且不改变计量。另修三个真实VerifiedBlob退出缺口：部分批次/交付前stop按获取顺序关闭所有已持有blob；回调接管后晚期source/reader错误关闭已交付file；本地asset inspect capture成功或失败均checkedclose，保已提交身份与原主错/全部cleanup原因。故障注入发生在真实close之后，不声称模拟原生OS关句柄失败。

d69本机首次all-targets实际失败：两个新增测试文件共4处通过PinnedSet的Deref移动`.data`触发E0507；本后继只改为移动真实owned字段`.spec.data`，不clone、不改生产或断言。原进程已安全回收，Source/Core独立组仍在测试，优化构建与产品测量未启动。[4c2实际复验](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6230316571)alltargets通过，Data profile8/regression12/adaptive17/export4共41/0/0，16新增逐名全部通过。

Data新增16个唯一Rust测试（profile8、真实consumer8）已在4c2实际运行通过；后继同字节不重复累加。原组过滤为phase_profile、local_query::regression_tests、local_query::adaptive_tests、assets::export::tests，需alltargets先真实编译；Source同时跑Core storage原锁/等待/checkedclose与cache::source_facts::、Windows filesystem::destination::原全组。Node34/34、14Rust解析、6Node语法及scoped diff已过，不替代业务实测。

[现Data入口](tests/rust-performance-paired/data.README.md)新增独占--data-phase-profile，需本批匹配优化产品：12路径各1冷/1暖、48测量CLI＋15准备，原startup proof/完整结果等价/总时钟保留；只Rust测量进程开启env，严格接受单条完整profile，默认与Go仍空stderr，错误/未知stderr不吞。它不能与acceptance/resume/startup-only组合，不把子段相加或扣除后宣称达标。统一一次新构建后同时验证Source4036及Data阶段，本机已重连，父线程统一安排后继编译及当前独立组，不擅自重复运行。

### Facts 无独立锁文件原子发布（bbfe 正确性通过，性能测量进行中）

用户要求核清只读输入与缓存锁的实际必要性。输入与cache.read原本无该锁；旧锁仅包已解析完整bytes的发布，不防重复解析。现Facts-only改为私有排他CREATE→完整写/seal→同目录atomic replace，Originals、整index、metadata锁保持。公开Core接口本就允许同key不同完整body，保last-writer及原NotPublished等价fallback，不新增首赢家/不同body拒绝或正常成功路径额外读取。

64个stage槽共同容纳活跃writer与未知残留，仅已获owner可关闭/清理；占用跳过、不按PID/年龄回收。Win32真实sharing32与AlreadyExists作为未获owner的占用，其他权限错误仍返回。保32MiB、原deadline、完整错误/checkedclose与Published/Unknown事实，逐文档顺序checkpoint不改。旧读句柄可持旧完整快照。此为同步文件I/O的无独立锁文件发布，不称理论lock-free；不改缓存格式/GC/耐久合同，也不删除历史锁文件。

[466本机完整前置](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6230454742)alltargets通过，六组165通过1失败：Facts23/storage55/Sourcecache21/pipeline28/profile4通过，Windows34通过1失败。新增15断言中14过1败，真实双进程helper命中。唯一失败为前writer已rename但原stage句柄未close，第二replace返回Win32 sharing32；旧rename fixture已过。优化构建/4036/性能均未跑，不能称去锁方案已闭合。

本后继保stage原share READ与严格父目录，不能放宽DELETE-sharing让私有stage被迁到别的父目录。仅exact同步native STATUS_SHARING_VIOLATION转typed SharingBlocked且保持NotPublished；Facts-only保同sealed句柄在原remaining内最多25ms一等再试，每次仍原parentverify/控制/计费。普通rename与Originals错误合同不变，任意io error32、已提交后伪32、其它不确定结果仍Unknown终态不重试。原窗口test改为两次真Blocked→原writer真实关闭→同owner发布；新增6项覆盖真实pending/取消/原deadline、普通API、非cache拒绝及Unknown不重放。[bbfe 实际前置](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6230696383)all-targets通过，六组172/172，新6及修订writer窗口逐名通过，7个owner封存回收；Node22/22和静态/解析通过。[匹配优化构建与assembly](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6230789929)已成功，EXE SHA-256 `f25d5fbe4e43eb0ae2f28e310f28e4de51ab6be701d2919f94729132c76a39e6`；完整4036 Facts v10字段和顺序零差异。[同次Source冷profile](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6230905817)原完整native墙钟Rust7905.3876ms、Go8545.909ms，ratio0.925049，单次等价但未达≤0.30，不能替代20对验收。Facts发布3168.351ms，其中stage1462.106、rename1251.122、write283.678、seal3.023、close158.258、其余10.164；7个锁字段均为0。串行解析2005.918ms/2158份，主线程worker等待565.354ms；worker累计不加入端到端总和。Data48个诊断样本已实际完成，完整11段同row归因正在核验；原466失败保留。

### 当前 Source 多格式纯解析候选（实现完成，待集中验证）

普通Lua、XML、TOC使用同一typed纯解析任务，原文字节移动而非复制，worker无cache/目录/可变index；协调端仍按原顺序认证cache、处理首错、序列化及发布。XML与inline Lua共用同文档4MiB Facts和32MiB保守scratch准入，保64KiB输入、2MiB线程栈、原256MiB共享额度及逻辑核选择；generated Lua仍走完整原串行路径。预算不足或取消丢弃整个投机结果，在原消费位置走原完整解析，不把partial当成功。固定属性结构仍校验每个属性，保重复最后胜出和未知属性错误顺序。

新增21个唯一回归覆盖混合并发/顺序、84固定Lua/XML边界全字段、共享scratch与cache优先；原3个guard测试仅移到facts::worker模块，3个pipeline测试改名不重复计数。当前仅rustfmt解析、diff和unsafe扫描通过，Cargo/Windows/性能NOT_RUN。目标是同次bbfe串行解析2.006秒的实际关键路径，不能预先把它全部算作可省时间。

### 当前 Data request-owned Metadata 候选（实现完成，待集中验证）

bbfe真实48诊断样本/40段封存证据已核：图片冷prepare42.292–47.727ms，三类capture合计30.909–36.743ms；暖capture29.898–34.752ms，texture+pixel/hash+encode仅4.351–9.588ms。17MiB BLTE payload冷95.022ms/暖36.639ms包含完整认证与缓存发布，不等同纯解压。全段原时钟保留，单pair诊断不是20对验收。证据见[全批报告](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6230863009)与[索引](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6230863476)。

本候选在image三capture、inspect、SQL/CSV同步工作中借用OpenWorkspace已持有的Metadata；每次仍独立schema/依赖验证及BEGIN IMMEDIATE/COMMIT，不跨codec/输出持事务。显式架构变化是每capture物理SQLite close合并到原scope最终close：晚close失败继续保全部已提交capture IDs与outputPublished，外部artifact时点不移动，成功CLI Outcome只在最终checkedclose后返回。原outer SQLite连接本就在Windows请求全程持有，固定bundled Win32 VFS无FILE_SHARE_DELETE，正常生产协议不能同时替换整DB；合法行事务仍可并发，引用依赖每次重新认证。此说明不声称防任意裸写或提供Unix同等共享保证。

新增9项回归覆盖三阶段事务失败/取消、post-commit事实、跨owner拒绝、删除依赖后重验和PNG/WebP最终真实close后的错误注入。13文件rustfmt语法与diff检查通过；Cargo/新测试与收益NOT_RUN。不改变BLTE双stage同目录发布链、原完整hash及phase字段，不新增全局连接池。

### 当前 Live owned 投影候选（实现完成，待集中验证）

snapshot改显式持久元数据投影，去整State JSON/base64中间副本；小INPUT仍用原自定义codec，保legacy绑定/host时间/inputAttempt清理。CodeBytes每个非法UTF-8字节的旧替换语义与原始report字节保留，artifact去重只持首次副本。BODY及终态report Vec转移所有权，drive去整State clone并释放已取得lease前的旧快照；reload/repair在backend打开失败前仍需的副本不机械删除。原backend checked-close→journal close→条件retirement顺序不变。

新增11项离线回归，以独立旧snapshot/present实现比较完整raw、artifact、事件/checksum，并核Vec指针/容量、verified report与cleanup失败并存及预算失败关闭顺序。当前仅12文件rustfmt语法、独立静态复核和diff通过，Cargo/离线执行/收益NOT_RUN。日志解析缓存只保留设计：每次仍须完整认证当前字节，变化走原错误路径，不能用mtime/长度略过同长篡改；本批未引入常驻日志缓存或改变durability。

### 公开 Source project 选择修复（新增12项实测通过）

refs/context省略snapshot时复用现有受控project/最近父级锁选择，显式snapshot仍优先。context把认证后的同一pin移交relations，避免cursor认证后再次读取可变project锁，保原control和绝对deadline。固定Go命令先解析数值、后project、再research selector的顺序已核；本片保context数值首错和project错误优先于selector。新增12 Rust＝2移交/控制单元＋9公开消费者回归＋1需要匹配包的真实CLI static/native显式ignored项。30d本机两单元2/0/0、完整refs_context14/0/3实际通过；[匹配30d release/package后真实CLI测试1/1通过](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6228578918)，新增12项逐名全通过；其余原ignored保持原边界。云端Node默认3通过1明确Go源码复核skip，指定持有固定Go323对象的checkout后4/4真实通过；核心3项不要求本机Rust仓有旧Go对象。普通产品consumer/cursor切pin与恢复、immutable pin认证已有上述Windows组证据；真实packaged static/native已在本轮命中。此修复独立于已通过的runner显式variant snapshot路线。

## 4. 进入最终五阶段前的有限收口

当前优化以 Source、Data、Live 同次请求的完整端到端墙钟为主：互斥已测区间加明确余量，嵌套与并行累计单列，不扣除成本或转移到首次查询。逐项核算法、数据结构、分配复制、存储与调度。Source 当前优先已实施发布去锁的真实总收益及格式受控纯解析；Data 等匹配11段业务观测选择热点；Live 先复用正式离线 service/Lua bridge 检查 journal 重扫与 payload/snapshot 复制，游戏项仍留本文末尾。

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

当前bbfe匹配优化产物、4036 Facts差分与Source/Data单次诊断已安全终态；2d7独立runner/a338产品full14/0继续保原身份，不重复跑旧retained故障。下一统一Source/Data/Live源码批先alltargets，再Source facts::（含迁入guard及84边界）、index::、lua_tree::，Core evidence::budgeted/metadata::capture_write/metadata::controlled_open，Data assets::export/local_query/remote::admission，Live go_mailbox_v1完整离线组。新增测试按精确名称去重，未运行保持NOT_RUN。前置过后一次匹配release/assembly，运行4036全字段顺序差分、同输入Source/Data阶段诊断及正式9桥；不得用旧EXE归新源码收益。全部本机任务仍由唯一父协调队列发送。

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
