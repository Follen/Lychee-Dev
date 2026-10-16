# Rust 迁移交接：唯一当前事实来源

更新：2026-10-16 20:33 UTC。分支 `rust-refactor`，持续交付到[草稿 PR #14](https://github.com/Follen/Lychee-Dev/pull/14)。最新已核远端批次：`0e1d7e5bb4e9caea232e959a1be06b8bd2de93a0`；当前本机已完成913优化产物的正常4036/profile及retained复验；旧优化产品e843结果保持原身份，runner身份独立认证。实现进行中，尚未完成全量验收。

## 1. 当前决定与不可漂移基线

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

- 2026-10-15 18:24 用户调整实施节奏：先按完整端到端功能实现并接入默认生产消费者，再集中补测试和统一验证；取消自行增加的逐小片冻结/独审/CI先行门禁。实现过程中仍及时分批提交推送同一草稿PR，真实安全边界和文件单写协调保留；已有CI旁路诊断，不等待每轮才继续编码。最终四阶段循环审计与完整验收要求不变。

## 2. 当前实现与有限剩余项

- **Data 默认链已接入**：本地/CDN、Prepared/cache、SQL、导航、asset、Hotfix、自适应内存与 checked 收尾均已有实现。当前重点是 SQL 默认线程栈安全：原合法 scalar 深度63在 parse 实际溢出；本批25文件完整候选已实现堆任务栈解析/降低、bind/eval与迭代树 clone/equality/drop、flat JOIN及visitors，新增18项默认栈深度/混合嵌套/错误析构回归。最新fa587f已通过all-target与原三个parse边界测试，但同步 provider callback 的原生重入已在默认栈实际导致6项执行/错误回收溢出。本轮30源已完整实现统一单堆query/CTE/derived/UNION/JOIN与bind/eval任务驱动，子查询通过请求返回主循环而不嵌套poll；provider单次open逐行cursor保留当前行与预算lease。原右侧JOIN物化策略保留，未新增全表缓冲、抬栈或降低深度。迭代Drop/Debug涵盖Weak与并发最后引用；独立结构与固定Go审查已完成，净增22测试，fed首次生产lib的5处子模块可见域错误已在ac8最窄修复；ac8生产lib与workspace all-target编译真实通过，原6个执行崩溃逐项通过，完整正式Cargo relational组299/0/0通过。原预算、深度与默认线程栈不变；专项独立架构/provider/固定Go差分深审完成、无确认新缺陷，local_query76/0/1与全Data库1063/0/1通过；36集成256通过1预算断言失败20忽略，7db补强定向27/0/0已通过，关闭该预算测试缺口，原ac8失败保留。
- **Emmy 已使用原版848文件，语义验证未完成**：`3336d82`已保存 initializer/provider 重复查询、call quality 多余查询和完整键集合被未知值污染的修复。原固定参考与当前候选的全量类型/provider/coverage差分保留；ac8实际Adapter140项、context56项已通过；完整历史严格差分参考36通过、候选35通过1失败，4137行中仅object成功投影的两条正向call_result证据有差异。独立当前合同检查已在原ac8真实sealed产物通过（4135相同行+2精确当前receipt），原始严格失败、全部类型/provider/coverage原文和Cargo退出结果保留；不能将新增检查冒充原历史比较通过。
- **HTTP 后继已保存并取得定向实证**：`6051063`使用固定 native-tls/schannel 三源38加4删的最小适配，将SNI名与证书校验名分离，保原验证与tokio所有权。完整来源/许可/逆补丁和打包已接，Node20/20通过；固定fa封存Data EXE实际remote329项与私有根TLS五项Go1.27.1对照已全部通过，多尾点SNI一致且证书拒绝保留。后续ac8正式Cargo remote329及固定Go五loopback也已通过；不能外推系统根/撤销全部策略。Emmy零补丁不等于整个依赖树零补丁。
- **Source 正确性与性能仍未验收**：5bbb完整配对中暖query20对等价且Rust/Go比0.202388，只有该组达到≤0.30；describe两组约1.13、cold query约5.00未达标，cold index因完整公共事实不同而失败。Metadata前置与SQL预算缺口已实际闭合。当前完整修复统一Go公共Facts字段/顺序、两个不同validate入口及完整类别gap缓存；native XML/Emmy语义增强保留在独立真实消费者，不用summary扣数掩盖差异。e843已通过Rust编译及4036文档逐字段/顺序0差；7f畸形输入52项已全字段差分通过，冷性能仍不达标。非法Lua诊断文本与全部非法XML错误文本尚未完整对照，是明确剩余边界。完整Retail refs/context/batch维持原90秒，旧dev仍deadline失败；优化408普通native refs/context/API已实际通过；retained首次请求EOF仍未闭合；e843真实600秒lifetime已退出0通过。完整B01–B07、内存与四阶段审计均未验收。
- **正式 Lua 已明确切到 Go323**：`4092344`保存113原blob认证、精确Rust3.5 overlay与220/9正式入口，旧320/321/322原清单保留。Node正式63及包/版本115通过；本机已安装官方固定MSVC14.51与SDK，Community三工具字节/版本/签名均匹配原Enterprise固定身份；Community精确路径已实际验证。固定7c07的Go323正式Lua220/0/0与Rust桥9/0/0现已真实通过，全部owner回收；只是该自动化范围，不含实机或性能。

### 当前全仓架构实施批

e734已保存四个真实消费者边界，详细设计见 [DESIGN](DESIGN.md#whole-repository-rust-native-implementation-plan)：Core 借用型 pin 请求已接默认 Data provider；Data typed stream 已接 CLI、逐行执行和统一 wire renderer；Live 三态结果已迁移 connection/activation/doctor/CLI，清理合并消费原结果而不 clone；Source 集中原 Lua51 配置、八类诊断顺序和准入策略。Live 新增12项、Data/CLI新增13项、Core/Data新增10项、Source配置新增3项回归，全部本架构批新 Rust 尚未执行；Node合同/身份45项实际通过。后续跨模块静态消费者曾60通过1失败，原因是旧322整体文件身份未纳入三态迁移；精确52编辑可逆认证补齐后62/62通过，原失败保留，旧manifest/Go输入字节未动。保留原预算、错误次序、输出和已提交事实；本批不代表最终四阶段审计已完成。

Source 冷链同时实施有界 scoped 多线程纯解析，按序提交并共同占用原 retained 额度；不把所有模块强行并发。Core/Data 现有所有权与单堆 SQL 保留，Live 输入状态仍串行。成熟库按实际能力、成本、许可与升级验证选用，不为架构名义另造空 crate 或通用调度框架。

## 3. 当前实际验证与证据

完整逐批结果、原失败和工具恢复记录保留在[10月16日验证历史](docs/toolkit/rust-migration-evidence-2026-10-16.md)与[前期历史](docs/toolkit/rust-migration-evidence-2026-10-15.md)。以下只列当前结论；不同版本结果不能相互替代。

- **缓存与紧凑表示已通过定向回归**：[f6 53通过3忽略](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6223486548)包含全部17新增；[913 111通过3忽略](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6223576658)包含native58与6项紧凑表示回归，52完整字段仍52/52，all-target实际通过。忽略项保留原边界。913后续优化实测见下：正常Facts等价，但retained仍失败；不能据定向测试宣称内存验收通过。
- **SQL/Data/Core**：ac8默认栈正式relational299通过，6个原执行溢出逐项通过；local_query76通过1原fixture忽略，Data库1063通过1忽略。36集成256通过1预算断言失败20忽略，后续7db定向27/0/0关闭该断言缺口；[证据](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6220137512)。[Core492/0/0](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6220479350)含真实ABA/取消回归，不能把重叠子组重复累计。
- **Emmy/HTTP/Lua**：原版Emmy848身份不变；历史严格比较4137行仍有2条必要成功receipt差异，独立当前合同检查4135相同+2精确通过，原raw失败保留，[当前合同证据](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6220362319)。正式TLS329及固定Go五loopback通过，不外推全部系统根策略。Go323正式Lua220与Rust桥9已实际通过，原证据见历史链接。
- **完整正常Source公共事实已等价**：[e843真实4036文档全字段/顺序0差](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6222922847)，声明65320、关系227786、loads4169、headers1888、diagnostics0。报告SHA `bd73dcb164b11cd30f60e967805aaedcc8434214c695f092546d4ba2981d4d39`。这不覆盖畸形输入或native语义完整性。
- **7f52边界已全部差分通过**：[真实Go报告复用/52全字段比较](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6223216537)全部一致，相关47个唯一测试通过；首次本机相对路径错误改用实际绝对路径后仅补跑该一项，原失败仍留。Go报告SHA `849e3b0ab76e2252e10f183bb84c281ba96452cb0f32bc0f430e8ab9680730dc`、52输入和全部字段/顺序不改，事实v9/index指纹v10失效旧错误cache。goto/label属于固定gopher公共合同，native Lua5.1名字处理独立。这只覆盖该52输入，不冒所有非法Lua/XML已穷尽。

- **retained与寿命待完整实测**：[ed6定向53/0/0](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6222429490)含5个scope旧算法差分及8个寿命单元；不能据此宣称内存收益。408普通native refs/context/API通过，但retained首请求EOF、后继依赖未跑、600秒寿命退出5失败。[e843完整native复验](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6222986269)仍8通过5依赖失败：retained首请求11.738秒EOF，最后query_build_scope；精确child67904接近2GiB但没有退出码/limit通知，不能断言OOM。原scope去副本没有闭合此项，继续结构性驻留诊断；[e843真实600秒寿命](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6223130356)600047ms退出0通过，关闭旧正常到期误报缺口。retained首请求仍独立失败。
- **性能整体未达标**：[5bbb完整配对](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6221187576)只有warm query20对达到Rust/Go=0.202388；B01冷热约1.13、cold query约5.00失败。e843单次等价cold index Go9.554s/Rust45.104s，非20对验收；phase44.898s中Facts发布15.613s、原文发布21.874s、解析5.424s，Git inclusive阶段重叠不能相加。正实现可重建缓存发布优化，保正式捕获与最终index durability，收益未测。原内存采样有缺口，完整内存验收未完成。

- **冷index发布优化已实现，定向Rust验证通过**：Facts和4036份原文分别进入固定路径的可重建cache，单次index复用已准入目录、同key原deadline锁、最多64个不确定残留stage、同目录原子可见与checked close；这些缓存不请求逐文件crash durability。暖读仍核全文SHA/长度/UTF8，缺失或损坏按已认证index内固定repository/product/commit/parser及Git树边/对象身份恢复，不能采用移动分支。正式inspect/loadEvidence/diff/closure BlobRef及4个asset、最终index/manifest保持原durable发布。三个实际CLI search入口补传原inv.control，避免恢复时换新预算。新增Core9/Source6/Windows2回归，Node读侧14/14通过；17项新增Rust已在f6实际通过；新优化产物完整4036与新性能仍待跑。原e843慢样本保留，不预报降幅。

- **retained结构性内存候选已实现，收益待测**：native raw Emmy观察改为共享path/原SemanticId的紧凑typed记录，序列化保持原字段/字节顺序；实际research回复不整体物化该表。全工作区managed membership改为FileId/语法种类分组的精确u64范围，每成员8字节、冻结上下文Arc共享，保持Name/Index/Call区别。新增6项Rust逐字输出、原集合、取消/输出预算测试；94项相关Node与语法解析通过；6新增Rust已在913实际通过，完整2GiB复验仍未完成。e843112原始内存行SHA已核，增长主要在analysis child；采样只有外层cold标签，不能据此归因某个内部phase或断言OOM。原4个冻结参考文件未改，上游生产仍原版848。
- **实际并行冷链候选已实现，待验证**：最多4个scoped纯Lua解析worker、8个有序槽，仅≤64KiB普通UTF8 Lua可提前计算；小XML/TOC只持有原文、按原主线程完整流程处理。cache读取/序列化/发布与所有公开错误按序，批尾drain后才启动下一Git进程；真实Git Vec capacity、原文、结果/scratch及worker栈共同受原256MiB retained余量准入。超额或未支持输入走原完整串行路径，无新增公共拒绝；第三方tree-sitter C分配不是Rust账目/RSS硬上限。26新Rust及真实默认2MiB worker栈的混合输入/跨4097对象batch回归尚未执行。并行profile只累计已消费worker任务时长，重叠不能相加；完整4036等价、取消与新20对性能仍待测。

- **913优化复验已有终态**：[4036 v9完整字段/顺序0差](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6223829481)，单次等价cold index Rust18.796s/Go9.653s；其中Facts发布5.494s、原文发布5.242s、解析5.572s，总phase18.57s。相较e843单次45.104s是跨版诊断，不能当20对统计或≤0.30验收。[retained仍失败](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6223875817)：首请求11.95s EOF/exit5，最后query_execute_operations，private2142556160/kernel peak2144694272；无child退出/limit通知，不能断言OOM，外层已回收。紧凑表示没有闭合实际retained缺口，继续定位真实驻留与认证退出事实。
- **typed组合复审发现并修正继承的字段解析与首错差异**：固定Go323 StreamDataRows先验证filter字段，再投影字段；原Rust和新增旧顺序测试相反，且投影字段错误地trim了Go保留的空格，UInt过滤错误接受Go ParseUint拒绝的正号，尾逗号空字段错误多拼了冒号。当前生产按Go原样split与顺序修复，保原真实table准入阶段，新增双故障无yield/无capture与无符号正号过滤零行manifest回归，合法filter/坏projection、原空格和超额projection先后也覆盖。语法/静态检查通过，Rust尚未运行；四边界独立组合复审已完成，修订静态复核未见确认剩余问题；新增Rust仍待跑，不冒称最终第一阶段已完成。

- **retained进一步去副本与真实退出诊断已实现，尚未实测**：研究关系查询将每site BTreeMap/Vec改为稳定排序的单平面借用索引，保同site观察顺序；完整relations/analysis JSON直接move组装，保原wire字节。现有guarded Child/Job句柄记录退出码、终止前/后观察及历史commit峰值，私有认证Response由调用CLI在diagnostic stderr呈现；不改公开结果/error、不加owner同步落盘、不猜limit触发。读侧仅对精确owner/child创建身份与原请求deadline匹配的完整行关联旧phase，缺失仍unavailable。新增7Rust未跑；75Node读侧/runner/IPC实际通过，收益和retained失败是否闭合待新优化产物。

- **7a Core typed入口首轮17通过1失败，阶段fixture已修待复验**：[原完整失败](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6224048336)来自两条入口各用80ms墙钟，在retry前或真实SQLite BUSY后停止，合法错误树不同。当前只新增cfg(test)线程局部BUSY计数，以首次真实BUSY触发同一独立stop，逐层核完整原SQLite/补充原因/cleanup及原control deadline不变；不改生产错误选择、不扁平化断言，原失败保留。

## 4. 进入最终四阶段前的有限收口

1. SQL结构性栈修复与预算回归已有实证，保完整实现供最终独立审计；当前集中闭合Source52边界、retained回收/寿命和真实冷链发布性能，不扩展新数据库或通用框架。
2. 在同草稿PR保存完整修复批，集中跑实际生产链与原全量比较；外层验证必须有界失败并让后续独立组执行，不提高产品deadline。
3. 验证Source短预算/暖态回收与完整Retail输出、诊断、性能/内存；未完成组清楚列NOT_RUN，不以unit或静态Retail替代。
4. 完成上述实现闭环后进入下节四阶段循环。继续以完整功能为单位实施与集中验证，不恢复逐几十行独审门禁。

## 5. 验收与循环审计

完整[F01–F13功能、P01–P14协议、B01–B07性能和Go来源合同](docs/toolkit/rust-migration-acceptance.md)保留；[321增量](docs/toolkit/rust-go-3.2.1-increment.md)、[322增量](docs/toolkit/rust-go-3.2.2-increment.md)与[323增量](docs/toolkit/rust-go-3.2.3-increment.md)是累积要求。代码已实现、静态已审、某树历史通过、当前实际通过必须分开。

- 固定Retail数据为Gethe/wow-ui-source `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b`：4036 Lua/XML/TOC，32,269,051字节。每次核身份后全仓分析/代表查询/源码定位/refs/context/flow/cold-warm/session/释放，不以小fixture代替。
- Source、Skill、Data分别由独立使用者执行真实CLI任务，记录输入、完整输出、正确性/可读性/来源/错误和复测。实现者不自评通过，无法运行时记blocked。
- 全量实现后的四阶段**顺序循环**：①架构/全业务/Skill/全部代码；②有效覆盖与无独立价值冗余；③真实消费者确认后的死代码；④前三维度联合全仓。每阶段独审→修复→实际验证→独立复审至无新可行动finding，再进下一阶段。每批源码审不等于最终收敛。
- Go323测试映射为当前差分oracle，Go322映射仍保留，320/321原证据保留；尤其未知结果不重放、事务、取消、空map、槽迁移与错误恢复。记录固定树、覆盖分母、平台排除与停止条件，不保证数学零bug。
- 性能必须同机同数据/结果与校验等价，独立cold/warm逐路径重复；可控耗时Rust≤Go30%，外部等待另列。当前仍未达成验收，不能把缓存预热或删校验当提速。

## 6. 本机接力：当前执行与下一批

本机测试通道已正常恢复，原unknown历史保留，无需再次apply或重启。仅云端实现，用户指定本机线程负责固定SHA测试并将报告写回同一PR。普通push不自动切换其正在测试的树；不另排重复长CI。

当前 **e8434bfbdb3e26c19c3c2c84c4be3624a63a3499** 优化EXE SHA `a413febf012185fa1c3c124ce89c4eac7266565fa497221ab3400149c7784d39` 已结束正常4036 Facts、单次profile、完整native（retained仍失败）和真实600秒寿命（通过）。后继7f52完整差分也已通过；新的缓存/紧凑内存候选必须新编译与认证，不能冒用e843优化产物。当前913已通过all-target及定向111项，优化4036/profile与retained已完成。后继全仓架构和并行冷链以7a为已保存起点，按协调指定SHA切换，不覆盖原产物。

- 后继全仓架构批定向：Core `selection::project_controlled::tests`；Data `stream::tests` 与默认provider的 `default_db2`/`default_stream`；Live `go_mailbox_v1`；CLI `formal_live`、`native_stream_writer` 与原 raw-number tests；Source `engine_config::tests` 和 adapter `--lib`。所有新增测试当前未执行。
- 所有Rust检查沿仓库受控 `tools/rust-cargo.mjs`、固定工具链和原owner/cache合同，先all-target编译，再受影响组；默认线程栈不加大。完整SQL299及7db预算组已过，不机械重跑历史失败树。
- 正式Lua与9桥使用[现有入口](tools/rust-formal-lua-bridge.mjs)，每次新输出目录；原runtime与CLI封存产物必须通过全部身份验证，不能覆盖旧失败目录。当前正式Go323220/9已有实证。
- 性能沿[专用配对入口](tests/rust-performance-paired/README.md)，独立优化构建、原20cold/20warm及完整输出等价。单次profile只诊断；索引/query180秒，refs/context/batch90秒，整棵retained分析树2GiB不变。不可扣除未测成本来宣称达标。
- 完整Retail/native沿[已封存候选入口](tests/rust-source-native-acceptance/README.md#current-workflow-and-separately-sealed-runner)，runner与产品各自认证，记录所有依赖未执行、退出码、精确子进程回收和内存采样缺口。最终四阶段循环尚未开始。

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
