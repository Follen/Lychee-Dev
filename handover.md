# Rust 迁移交接：唯一当前事实来源

更新：2026-10-16 16:46 UTC。分支 `rust-refactor`，持续交付到[草稿 PR #14](https://github.com/Follen/Lychee-Dev/pull/14)。最新已核远端批次：`c508b10ef0e651fc8721d251feef5d25e41a0454`。实现进行中，尚未完成全量验收。

## 1. 当前决定与不可漂移基线

- 唯一验收目标：以固定 **Go 3.2.3** tag 源码为当前行为基线，实现 Rust 宿主并保留 **EmmyLua 直接分析库/native-worker/session** 替代 LuaLS。除此之外，全部业务、同一 addon、CLI、Skill、完整 Mailbox 协议、错误恢复、诊断和输入合同对齐正式版本。
- 当前 Go tag `v3.2.3`：tag object `7a24532cf0fe294faeb96e8224797b6fe69386d5`，peeled commit **`3766f9cacb52e93a803a626187e2a1d54a445468`**；2026-10-15远端tag/main与本地精确对象已核。322→323为2提交、41文件；业务在该正式提交，另一个提交为322发布文档。新增自适应Data内存/准入容量、紧凑记录、IN选择及Data Skill，详见[323增量矩阵](docs/toolkit/rust-go-3.2.3-increment.md)。新增验收PENDING；未验证发行二进制/资产。
- 历史 Go tag `v3.2.2`：tag object `97890b267a620a166889db0934e57a826d72a872`，peeled commit **`0bec60e4406f5d5dc155ee25cef44f9a710e37e8`**；2026-10-15 精确远端对象与本地 fetch 已核，检查时 main 同 commit。Release API 当时返回 404，不能声称发行二进制/资产已验证。业务增量 `969fe480fdb38eb038401044c3327b04ccaaee81`，详见[3.2.2 增量矩阵](docs/toolkit/rust-go-3.2.2-increment.md)。
- 前 v3.2.1 tag object `637584b8b976966c3863c1889e9ff86fe55493d4` / commit **`10b561db5706702cd42e047eaeed4ccb5e825351`** 和 v3.2.0 tag object `93849f43f34d8ff1d6f4539c5a3496ab941ef229` / commit **`3057a9666ab078270b63c7f06de748069b0d1f4b`** 的源码、fixture、二进制、测试/失败证据均保留原身份，不能重标为3.2.2通过。未完成的[3.2.1 累积增量](docs/toolkit/rust-go-3.2.1-increment.md)仍须完成。
- 2026-10-15 用户最新版本决定：未发布 Rust 候选源码/包版本 **3.5.0**，发行配置渠道 **beta**（版本与 npm dist-tag 分开，不自动改成 `3.5.0-beta.1`）。Go 行为 oracle 现为上述 3.2.3；只获准同步源码和配置，未执行发布、tag 或 npm 渠道变更。版本同步已完成源码集成及本地包合同检查；精确 b16a 候选已通过 Windows all-target 编译和 CLI 构建，后续提交及完整产品验证仍按各自证据记录。
- 2026-10-15 13:39 用户明确要求“全量替换”：最终验收必须移除全部 Emmy 上游源码补丁，使用未修改的固定上游公开 API，自有 crate 承接完整 XML/Lua 增强、类型/诊断/来源/取消与资源合同。此前“尽可能减少、必要时保留最小 hook”的最终豁免已被此要求取代；已有补丁仅属过渡实现，不能作为交付完成。逐 production hook→实际消费者→自有替代→差分/全仓证据列迁移表并分批实施，不另写整个 LuaLS、不削功能或假造 checker coverage。现有原版5/5小样及 public rendering27+27/450trace证据只覆盖各自范围，不能宣称全量零补丁已完成。
- Rust 调查起点：`5443314c536e925801d3f0360110db946181d1b3`。它是复用基础，不是完成声明。按模块差分决定保留/替换/补齐，不从零丢弃已有成果，不沿用不符目标的旧协议。
- unsafe约束：自有业务/跨平台Rust代码默认禁止unsafe，用适当crate级forbid/deny守住；确有必要的Windows FFI/内存边界必须最小化安全封装、逐处SAFETY理由和指针/生命周期/线程约束与测试，独立审查，不整模块无限豁免，不为性能目标放开unsafe。第三方依赖另列审计，不能宣称整个依赖图无unsafe。
- 内部设计应体现Rust特点，不逐行翻译Go：合理所有权/借用减少复制，enum/类型状态、Result、RAII和有界安全并发按实际收益使用，避免过度抽象/炫技。对外合同不变，unsafe边界不放松，性能仍实测；纳入独立架构验收。
- Skill尽量复用固定Go3.2.3版本；仅Emmy引擎、安装和真实诊断等必要差异修改，不因换语言重造工作流。
- 性能：可控耗时减少至少 70%，即 **Rust ≤ Go 30%**，排除下载与不可控外部等待；同机同数据、输出与校验等价，cold/warm 分开独立重复，逐路径记录。未测不能保证达标。
- 所有需要用户、实际游戏或特定本机互动配合的测试放在本文最后；离线与可自动执行的 Windows CI 先做。Windows 专属测试不自动等于游戏测试。
- 首批交付仅文档；随后已获准全量实施本计划。仍不操作游戏、不发布、不合 main，不修改 Go PR #13 或其他项目。只保留本文件，不创建 `handoff` 文件或第二份交接事实来源。
- 交付约定：为Rust分支创建独立草稿PR；之后每批获准代码改动及时推送并更新同一草稿PR，不等全部验证完才提交，也不每批新建PR。先查已有PR避免重复。后续实现授权不等于游戏操作、发布或合并授权。

[DESIGN.md](DESIGN.md)解释目标结构，[todo.md](todo.md)给执行顺序，两者从本文件派生；更新状态与证据首先改这里。旧 `docs/toolkit/` 和历史 README/UI 设计是原受测范围的记录，不得覆盖本节。历史“Go3.1.2替换”“Rust≤Go50%”“重新设计四槽exact-COMMIT”与旧输入合同已被本次目标替代；历史测试通过仍只证明原始候选。

- 2026-10-15 18:24 用户调整实施节奏：先按完整端到端功能实现并接入默认生产消费者，再集中补测试和统一验证；取消自行增加的逐小片冻结/独审/CI先行门禁。实现过程中仍及时分批提交推送同一草稿PR，真实安全边界和文件单写协调保留；已有CI旁路诊断，不等待每轮才继续编码。最终四阶段循环审计与完整验收要求不变。

## 2. 当前实现与有限剩余项

- **Data 默认链已接入**：本地/CDN、Prepared/cache、SQL、导航、asset、Hotfix、自适应内存与 checked 收尾均已有实现。当前重点是 SQL 默认线程栈安全：原合法 scalar 深度63在 parse 实际溢出；本批25文件完整候选已实现堆任务栈解析/降低、bind/eval与迭代树 clone/equality/drop、flat JOIN及visitors，新增18项默认栈深度/混合嵌套/错误析构回归。最新fa587f已通过all-target与原三个parse边界测试，但同步 provider callback 的原生重入已在默认栈实际导致6项执行/错误回收溢出。本轮30源已完整实现统一单堆query/CTE/derived/UNION/JOIN与bind/eval任务驱动，子查询通过请求返回主循环而不嵌套poll；provider单次open逐行cursor保留当前行与预算lease。原右侧JOIN物化策略保留，未新增全表缓冲、抬栈或降低深度。迭代Drop/Debug涵盖Weak与并发最后引用；独立结构与固定Go审查已完成，净增22测试，fed首次生产lib的5处子模块可见域错误已在ac8最窄修复；ac8生产lib与workspace all-target编译真实通过，原6个执行崩溃逐项通过，完整正式Cargo relational组299/0/0通过。原预算、深度与默认线程栈不变；专项独立架构/provider/固定Go差分深审完成、无确认新缺陷，local_query76/0/1与全Data库1063/0/1通过；36集成256通过1预算断言失败20忽略，7db补强定向27/0/0已通过，关闭该预算测试缺口，原ac8失败保留。
- **Emmy 已使用原版848文件，语义验证未完成**：`3336d82`已保存 initializer/provider 重复查询、call quality 多余查询和完整键集合被未知值污染的修复。原固定参考与当前候选的全量类型/provider/coverage差分保留；ac8实际Adapter140项、context56项已通过；完整历史严格差分参考36通过、候选35通过1失败，4137行中仅object成功投影的两条正向call_result证据有差异。独立当前合同检查已实现、Node82/82通过，待读取原sealed实证执行；原始严格失败、全部类型/provider/coverage原文和Cargo退出结果保留；不能将新增检查冒充原历史比较通过。
- **HTTP 后继已保存并取得定向实证**：`6051063`使用固定 native-tls/schannel 三源38加4删的最小适配，将SNI名与证书校验名分离，保原验证与tokio所有权。完整来源/许可/逆补丁和打包已接，Node20/20通过；固定fa封存Data EXE实际remote329项与私有根TLS五项Go1.27.1对照已全部通过，多尾点SNI一致且证书拒绝保留。后续ac8正式Cargo remote329及固定Go五loopback也已通过；不能外推系统根/撤销全部策略。Emmy零补丁不等于整个依赖树零补丁。
- **Source 正确性与性能仍未验收**：短预算隔离/暖态回收已有实际测试；完整Retail仍在原90秒内未完成。最新 dev 候选准备约72秒，API虽写完回复，最终 capture 仍耗尽总预算。不得增加deadline、减少输入或校验来通过。ac8优化release实际构建成功，describe冷热各20对输出全部等价，但Rust/Go中位数比为1.127/1.126，未达到≤0.30；静态Retail四组在真实init/sync后因Rust metadata缺deleted_generations而无计时样本。默认Metadata删除generation真实ABA修复已实现、Rust待验，启动成本分段诊断正在实现；CLI已实现同进程不可变commands目录一次解析和只读引用，3项Rust回归待跑、收益未测；不能放宽基准校验掩盖。完整 B01–B07仍未通过。
- **正式 Lua 已明确切到 Go323**：`4092344`保存113原blob认证、精确Rust3.5 overlay与220/9正式入口，旧320/321/322原清单保留。Node正式63及包/版本115通过；本机已安装官方固定MSVC14.51与SDK，Community三工具字节/版本/签名均匹配原Enterprise固定身份；Community精确路径已实际验证。固定7c07的Go323正式Lua220/0/0与Rust桥9/0/0现已真实通过，全部owner回收；只是该自动化范围，不含实机或性能。

## 3. 最新实际验证与证据

- **默认Metadata generation真实缺口已修、待Rust复验**：原HistoricalWriter已有固定Go323 tombstone，但默认writable open缺表、创建从1重置、删除不留generation；target remove→resolve与capture重新持久化可让旧CAS误匹配。本批统一两writable入口原样DDL、五创建路径读tombstone+1、四删除路径同事务保generation；readonly不迁移，原幂等/CAS/取消与rollback保留。新增7存储测试、2真实消费者回归及Source取消COMMIT回滚断言；Node6/6执行固定Go/Rust真实SQL通过，Rust仍未编译/执行。不能恢复旧Rust从未保存的删除历史，未猜测重建旧数据。原B02 seed失败保留，修后须沿原认证重跑。

- **7db预算回归已闭合**：[三个正式Cargo组27通过/0失败/0忽略](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6220137512)：temporary3、stack15、relational_parity9，包含新增5项。原低cap确在future首次poll前拒绝；另有限校准真实进入scalar/concat后验证准入与退款。scalar63/mixed128六种shared错误、取消和物理拒绝退出均保foreign113，释放foreign后0；累计logical账保留。原预算、默认栈不变，三个owner均sealed/retired/reaped；两组受审辅助清理forced=true如实保留。ac8原失败不改写，不与旧全Data重复相加。

- **ac8完整Emmy与Source后继**：[原始严格差分](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6219800482)参考36通过、候选35通过1失败；4137行/38段，仅object_carrier的ordinal18/22新增两条准确成功call_result receipt，其余4135行相同。原raw allTraceFieldsMatch=false仍保留，不能忽略coverage泛化通过。[Source自动化补测](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6219841437)：Adapter140、context56、persistence7、query_store6、batch14全部通过；refs/context与trace storage合计9通过3忽略，缺真实fixture的忽略项保留。这些不是完整Retail语义或性能验收。
- **首次真实优化版配对性能未达标**：[ac8 release与B01/B02原始报告](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6220085568)。release构建8分10秒成功；describe cold/warm各20对、完整输出等价，Rust中位46.1928/46.0976ms，Go40.97075/40.9547ms，比值1.127458/1.125575，明确不满足≤0.30。B02四组均0样本：两侧init/sync成功后，seed认证发现Rust仅documents表而固定Go另有deleted_generations；先核生产删除/CAS/generation合同，不能为了测试随意加表或放宽认证。52项性能前置已真实Windows通过；prepare成功不等于性能通过。owner完整回收，无deadline/forced；当前启动成本调查与存储合同修复并行。

- **ac8统一SQL真实闭环进展**：[完整Cargo relational299通过/0失败/0忽略](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6219373341)，原[6个执行栈崩溃定向全部通过](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6219348934)，与299组重叠不加总。生产lib/workspace all-target编译已通过；默认栈与63/64、128/129、原预算/断言保持，未设RUST_MIN_STACK/RUSTFLAGS。最深scalar63+UNION保127活跃query states、527304B future、native poll nesting1，退出/取消/错误后query/future/poll余额0；原299组深层样例使用shared=None；后继7db已额外通过真实shared Budget错误/取消/物理拒绝退款回归，证据分列如下。527304B只是活跃future内联状态，不是总堆峰值；marker span0不能冒充完整栈字节测量。完整组owner已sealed/retired/handlesReaped，历史fa崩溃和fed编译失败保留。local_query完整组76/0/1通过（1项要求真实加密CASC与认证key fixture），8集成42通过1失败，独立SQL专项深审完成、无确认新缺陷，未开始或完成最终四阶段审计。

- [89f61b9 run38880882451](https://github.com/Follen/Lychee-Dev/actions/runs/38880882451)已终态：static/context成功、product失败。final产物211 owners、8194 sealed日志逐SHA匹配，ZIP `1534ce9c1aca3718ec62d2b53b69f46e9f66884f44731c25c8e8b92e49d03c17`。all-target/CLI、Core、slot、正式旧Lua与桥、HTTP及IPC等已执行组通过；adapter、固定oracle、keys、local SQL、relational、Skill、fullRetail失败，不报全绿。
- **ac8完整Data与正式TLS**：[三互斥父测试组](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6219604224)与[正式TLS](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6219621835)去重为Data lib1063通过/0失败/1忽略；36集成256通过/1失败/20忽略。原真实CASC/DBD/key/客户端与独立性能fixture缺口保留，不能计通过，也不与前述SQL子集累加。正式remote329/0/0及选定Rust owner的Go1.27.1五个私有根loopback对照0差异，完整sealed/回收；只证明该SNI/证书范围，不外推系统根/撤销。完整Emmy严格raw后继仅剩两条成功对象投影receipt差异，原失败保留；Source其它组见下条。
- **ac8后继真实失败仍未闭合**：[local_query与8集成原始报告](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6219458827)。relational_parity的共享准入用例先正确返回resource_budget，再在错误后retained范围断言失败；原日志未打印具体子项/实际值，不能断言越预算分配，已核driver Future约4152B在第一次poll前超过旧4096/2200测试限额，释放后0合法。本批保该早拒回归，另外用有限空输入校准和严格work/retained差分证明确实抵达scalar/concat准入，未改生产预算。新增3个深层shared错误/取消/物理准入测试及guard观察单测，旧foreign reservation和累计Go账保留；共5新增测试及原测试修正已在7db实际运行通过，关闭本项测试合同缺口。另Cargo101使既有成功root专用辅助清理条件不成立，尚未进入vctip身份审核；Observe的reviewedAuxiliary默认false不能当身份不匹配证据。随后forced但最终完整回收，与真实断言分别记录；普通lease已正常释放。
- **89具体失败**：adapter lib50通过4失败，object member6通过1失败；member缺键Nil/Unknown真差异已在3336修，其他四项测试合同已据固定函数参数/诊断/metatable行为修正并新增独立原版对照，待复测。relational在 scalar63/parse发生 `0xc00000fd`，未超deadline且完整回收；另外8个SQL集成target共43项通过。keys旧fixture与local SQL scratch准入已在1f9修，随后本机定向1+1+15实际全部通过。
- **完整Retail**：4049文件/4036文档及1091 XML/4018映射认证通过；普通refs/context/API和retained冷启动均原deadline失败，后续依赖暖态无前置而未测。API最终 `source.refs.commit_capture` 耗尽预算，不能把worker产出当CLI成功。retained同身份child观测private峰1,995,444,224字节，低于2GiB，未证明OOM；全Source多进程集合private峰2,495,275,008字节且有采样缺口。Skill静态流程通过，唯一原生refs/context同样deadline。此为dev失败记录，无70%速度或内存验收结论。
- **本机ec503**：process47、native_session28、trace storage5通过，父启动故障peer1项单列ignored。固定oracle reference36/36、candidate29通过7失败，双方4137行/38段，完整回收；大边界trace输出修复已实证，语义差异仍保留。公开差异见[PR证据](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6213988386)，后续0d同输入已缩至下述3例12行差异，ec503原结果保留。
- **本机4092344历史阶段**：Root/keys定向17项及CLI dev构建已通过，当时正式Lua被固定compiler路径阻断，随后7c已通过；优化release构建在旧600秒准备期限命中且完整回收、无artifact；本批仅buildRust release/symbols准备期限固定30分钟，dev/test/check/fetch及产品原90秒保持，Node8/8通过。优化配对实际测量尚未运行。后续普通push不打断固定SHA测试、不自动排重复长CI；本机只测试，云端负责实现修复。
- **本机0d0da9f编译失败**：[原始记录](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6214850678)为lowering局部Option类型不明确、两处AttachedToken没有Default、KeyIndex构造漏预算lease四点。本批最小修复明确Program类型、使用原empty token构造并接回真实allocation lease；后续fa all-target已通过；当时失败事实保留。Community辅助进程此前未被精确路径清单认可而强制收尾，所有句柄已回收，与四个编译错误分别记录。该旧批后续项当时未运行；后继TLS/正式Lua结果按各自提交单列。
- **本机7c后继**：四个生产编译点已通过，独立SQL集成43项和48个Node/Windows前置通过；libtest仍因compound::branches的成功值不实现Debug而不能使用unwrap_err。本批改为显式match，原取消原因/7次回调断言不动，后续fa默认栈执行结果见下项。Community身份已实际生效。Emmy原版36通过/候选33通过3失败，剩12行仅新增call_result coverage，原provider重复及Nil/Unknown差异消失；完整raw比较仍失败。独立审确认denied gate未进入投影，本批通用修正只对有效Bound且实际尝试投影生成result receipt，拒绝/无效permit保原gate+Unknown；两object正向结果完成证据保留。实际重跑前不预测通过，不忽略字段；仅剩object差异也需精确当前合同并列证据，历史raw verdict不重标。
- **正式Go323自动Lua闭环**：固定`7c07f2406f1866abc533c421c8629ccf119c282a`的[220项Lua](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6215206258)与[9项真实Driver桥](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6215231080)全部通过、零失败/跳过，CLI/解释器构建exit0，Community身份按原hash/签名验证，所有owner sealed/retired/handlesReaped。旧7c libtest编译失败和0d oracle raw差异仍保留。本机该批已安全结束；dev产物不作性能证明。
- **本机fa587f实际执行仍失败**：[完整组](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6215304345)all-target与原3个parse边界入口已通过，relational声明284项、207通过后scalar63-late-error栈溢出，后76未执行；[追加独立证据](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6215417969)使10个stack_safety用例共4通过、6个执行/错误清理栈溢出。CTE126、长左链销毁、flat4095通过不代表混合callback链安全。随后逐名称补充诊断共观察284项中的278通过、6项执行栈溢出，另68项subquery补充通过；这些封存EXE诊断不重标完整Cargo组成功。全部已完成的失败均保原默认栈与深度。另owner `9f053173-8472-4860-be36-19a455a32e62`只有request而无native receipt，ordinary lease保留、结果/回收未知；不得按dead PID清锁或伪造reaped。独立已封存EXE补测有自己的真实owner，不追认旧owner。
- **fa封存EXE的TLS实证**：[remote329及Go五项](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6215452005)零失败/跳过，双多尾点SNI现相等、错误名和原证书校验拒绝保留，两个独立owner均完整回收。EXE SHA `5093d4a541a2b44fc4e7fbcfcabf28b7a7e8ead5e32210b5f29cc589b9274458`绑定同fa源码；这是保全EXE补充运行，非官方Cargo阶段，旧unknown owner未改。[Go323性能包准备](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6215390982)成功：clean固定commit、Go1.27.1、VCS未修改、优化构建和完整LuaLS私有包验证通过，EXE SHA `9f9a39ecaff18622b1dc9295bcd5ea671cb04dd4ba6961ad4a295dc53f1913f6`。Rust优化构建和20对实际测量仍未执行。
- **9395282完整SQL/进程所有权已保存**：30源的堆驱动、真实Physical/Overlay游标与最深错误/取消清理已接生产；修正固定Go bare关联列Equal/IN首错及JOIN ON临时量逐候选释放，保全部历史失败和原10栈样例。30源rustfmt/diff通过，22净新增测试NOT_RUN。另原子JOB_LIST随CreateProcess创建归属、启动/根身份在Resume前持久化，消除未来create/assign间隙；仅未来执行生效，旧unknown不追认。对应Node32项在云端16通过/16Windows跳过；[集中Windows run38907640603](https://github.com/Follen/Lychee-Dev/actions/runs/38907640603)已启动，实际产品/context前置失败：新增PowerShell sidecar数组逗号/加法优先级使合法路径被拒，Cargo fetch未启动，SQL编译/业务均NOT_RUN。本轮仅两处拼接加括号，原路径校验不松；补实际PowerShell数组/校验块测试。失败产物56owners/60sealed日志全hash通过，原unknown保留。
- **32c80d9已保存**：通用gate/result阶段修正及7c正式Lua通过记录，Rust语义复验尚未执行。原0d严格raw失败保留，两个object正向receipt仍在；不得预先豁免全coverage或报告完整oracle通过。
- **bed恢复准备已实际完成**：[本机记录](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6215888111)正式恢复测试17/17通过（含Windows真实C#与8项持柄锁/rename检查），prepare成功，原lease/owner/request字节不变。fence `cc31783a-9ed6-47a8-a150-e656b88bd90e`，prepared SHA `f4ef4175d09553b39fea44ebf85866cbba6c438e10dab3ed12214a66e3e9dd1b`。用户当前不方便重启，因此暂不重启/apply，旧owner仍unknown。原本机线程正在检查是否能凭当前执行域、进程创建身份与资源占用证据安全恢复通道；没有结果前不清lease、不追认历史回收。云端完整实现与必要的集中GitHub CI继续。恢复工具4文件字节保持不变。后续3d本机测试发现旧rename尾部名字损坏，当前禁止执行旧boot apply；改用本轮修正的独立readmit原子归档路径，具体失败与验证入口见§6。
- 历史run、原失败和前版交接正文完整保留在[历史证据](docs/toolkit/rust-migration-evidence-2026-10-15.md)。四阶段最终循环审计尚未开始，实机清单仍在本文最后。

## 4. 进入最终四阶段前的有限收口

1. 结构性修复SQL输入驱动的parse/retrieval/lowering/execute/drop递归与错误释放；不能以Box缩小帧、抬线程栈或降低原深度代替闭合。修复当前编译、测试输出/收尾与已确认SQL/Data/Core失败，保真实错误、预算、部分结果和提交事实；完成HTTP剩余Go既有边界，不扩成新数据库或通用网络栈。
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

## 6. 本机线程接力验证入口（2026-10-16）

**本机已恢复正常测试，无需再次apply或重启。** 固定fed的[新工具25/25真实Windows通过与正式再准入](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6216965340)已完成；generation `329c17de-52c7-4cd0-bfe7-383cabb2ff3e`，apply/inspect均 `readmission_commit_observed`。原lease精确归档，原prepared/owner/request/scratch保留，旧历史仍unknown，不伪造reaped。旧boot rename缺陷仍保留且禁止旧apply；新readmit的UTF-16名称、同句柄no-replace和Job模块无关哈希已实际验证。

随后[首条Data lib check](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6217011842)真正进入SQL编译并报5处E0603：joins/selection子模块的prepare/Cursor/open使用pub(super)，兄弟execution_select不能访问。ac8仅将所需接口限于共同relational模块可见，后续生产lib/workspace all-target与完整relational299组均真实通过。该失败Cargo101、无超时或强制终止，owner已sealed/retired/完整回收，ordinary lease正常释放。不要重复恢复或重复跑同一编译失败的全部组。

本批Metadata先受控执行core lib metadata::全组，再两个精确消费者removed_target_reference_recreated_by_resolution_rejects_stale_generation与removed_capture_republished_by_persistence_rejects_stale_generation，随后完整core lib；全部通过后按原paired认证重跑B02，不修改旧seed。

下一低成本步骤为[独立当前Emmy合同检查](tests/rust-source-engine/fixed-emmy-oracle/CURRENT-CONTRACT.md)：使用三个原ac8封存owner，写入全新独立报告目录，不重跑Cargo或改原报告。新Node82/82已过，原实证读取尚未执行。CLI目录缓存优化另需原contract::tests及真实describe完整输出回归；实测收益待确认。

本机固定ac8的全Data/TLS、完整Emmy严格差分、Source补测与首轮优化配对已结束，结果如上。固定7db已完成resource::temporary_tests::、relational::stack_safety_tests::及relational_parity共27/0/0；不重复已过的同树全SQL组。原fa6个执行栈溢出与ac8预算断言失败均保留。

用户指定现有本机线程接力，双方只经同一草稿PR同步源码与结果。先拉取本节标题上方已确认commit，保存自己的未提交修改，读AGENTS与本文件；不要把另一环境的历史lease复制或清除。89f61b9集中CI已终态；本机fed已正式恢复通道，旧fa执行栈溢出保留，新SQL编译与集中回归已完成，后继修复按受影响范围验证，不逐小片排长CI。

1. 在本机明确记录OS/架构、Rust1.99.0、Node24、Windows SDK/编译器和可用空间。Windows正式产品命令沿[当前workflow](.github/workflows/rust-parity.yml)的准备步骤：认证原版Emmy、受控`rust-cargo.mjs --owner local-fetch -- fetch --locked`，随后`--owner local-check --one-shot -- check --locked --offline --workspace --all-targets -j 1`。Linux只能报告其真实支持的子集，不假称Windows测试通过；不用裸Cargo绕现有owner/cache控制。
2. 本机每批固定协调明确指定的完整SHA，不随普通push自动换树；fa为历史失败；939完整执行架构已由ac8与7db实际验证，后续只按新改动跑受影响范围。先受控all-target编译，再`node tools/rust-cargo.mjs --owner local-sql-stack-tests --one-shot -- test --locked --offline -p lycheedev-data --lib relational:: -j 1 -- --test-threads=1 --nocapture`，包括原三个崩溃、原10个执行栈样例及当前新增所有权回归；单线程用于定位，保持默认线程栈，不设置更大RUST_MIN_STACK。之后跑原joins/CTE/derived/recursive及资源集成组、完整adapter组与`node tests/rust-source-engine/fixed-emmy-oracle/controlled-run.mjs`。保原63/64、128/129边界、全部输入/分段trace、完整panic/退出码及实际回收，不能只改期望。
3. TLS新组按[专用README](tests/rust-parity/tls-name-oracle/README.md)先运行一次真实remote组，再以其精确owner执行Go对照；Go1.27.1与私有测试CA准备沿同workflow，不安装系统信任。随后依workflow检查Core修复与其他失败组，不能跳过尚未跑的组来报全绿。
4. 正式Lua重试不得删除旧失败输出。先沿`tools/rust-formal-lua.mjs build <新的runtime目录>`和`run <新目录/build-report.json> <已封存CLI目录/artifact.json>`实际执行220项；9桥可用`node tools/rust-formal-lua-bridge.mjs <新runtime目录/build-report.json> <已封存CLI目录/artifact.json> <新的bridge输出目录>`，三个路径须均在repo内且输出不存在，原身份验证完整保留。无参数仍为原CI固定路径。
5. Source真实短预算/暖态回收及完整Retail使用现有sealed CLI、package与native acceptance入口及固定Gethe输入；性能另用[专用优化候选配对入口](tests/rust-performance-paired/README.md)（52项Windows前置已过；ac8 B01实测未达标、B02尚无样本），不拿dev单次结果冒充70%达标。报告commit、命令、通过/失败/跳过、证据位置与结果摘要回同PR；不操作游戏、不发布。最终四阶段审计仍未开始。

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
