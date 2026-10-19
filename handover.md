# Rust 迁移交接：唯一当前事实来源

更新：2026-10-19 07:50 UTC。当前已验证生产为 `d4271ae430ac2a80087af81c70eea960d547acbb`，分支 `rust-refactor`、草稿 PR #14。下列为非敏感结果摘要；本机原始日志未随文档发布。Stage2与完整性能验收仍未完成。

### CLI companion 首次Windows采集：对象文件限额误用（77df）

固定77df正式Node为146项：145通过/0失败/1个真实file-symlink EPERM跳过；Rust/LLVM预检与Cargo均成功。collector在选择companion时返回coverage.file_budget：实际instrumented CLI为149678592B，超过单对象沿用的134217728B上限。此时raw/merged均0、evidence3160726B；不是raw总额或累计证据耗尽。原exact四次CLI、merge、三次export与main witness均未执行，不能报告覆盖率成功。

三个owner回收、八native均0；一个forced为已审核辅助进程，deadline/lease/活动进程均0。源码定位为artifact选择及通用archive validation沿用了RAW_LIMIT作为对象大小限制；hash/copy本身已是64KiB流式。后继只分离对象证据准入与raw/merged限制，累计evidence256MiB及其metadata保留、raw+merged128MiB与16MiB分片保持；双对象和后续profiles是否装得下仍需实际验证。原失败保留，不改优化级别或裁剪符号。 新companion模式使用显式object类v2分片：单件理论上限由原256MiB总额扣16MiB保留得到，复制前仍按双对象及全部已有/待复制日志累计准入；旧默认单对象与raw/merged的v1≤128MiB不变。真实同次test对象42493440B，两对象合计192172032B，扣已有证据和保留空间后的56325482B仅是当时余额，不保证未来profiles/merged/exports都能容纳。

最终cloud coverage/build/disk Node156通过/0失败/0跳过（143+8+5，本批新增10），包含真实双对象尺寸、超过128MiB的两个对象角色、kind伪装、actual remaining恰好/越界与真实pair加60MiB raw仍拒绝保全的反例。语法/diff检查通过；Windows后继采集尚未运行，原77df失败不改标。

### Stage2 既有覆盖率证据范围复核（2026-10-19）

唯一测试协调线程只读核验a860既有12份integration报告：130通过/0失败/1忽略，逐名无重漏；2400个引用可读且hash一致，12个EXE及归档、各一份profraw/profdata的有序分片完整。scratch已按原合同清理，不需为恢复证据重跑。此处仅记录非敏感汇总。

共同工具条件为Rust1.99.0/LLVM23.1.1、Windows MSVC、test opt0/debug1、crt-static与coverage、incremental0、locked/offline/j1/default stack和原600秒。Source含default/emmy-prototype feature，其余为空；12份环境摘要不同且无完整快照，不据此强行跨报告合并。所有export只映射各自integration EXE，CLI对象映射为0，已确认公开CLI子进程统计缺口。

按原目标顺序，src/tests路径数：Core capture_dependencies、budgeted_capture、capture_query_references均93/1；Data sql_csv_parity、sql_lazy_provider_parity为236/2，relational_parity236/1，sql_explain_parity236/2；Source source_inspect、flow_parity均313/1；Live mailbox_lookup182/1；Delivery slot_maintenance109/1、release_control108/1。src也含内嵌测试/宏，不称纯生产分母；Source名为generated的模块是手写代码，不按名字排除。各映射集合不同，不相加/平均百分比。唯一ignore为linked_static_slot_member_is_rejected_without_following_it，权限边界尚未执行。

下一最窄工具片为同一次成功Cargo调用认证的integration+声明CLI双对象，默认单对象保持；首验保留现有Source inspect raw-admission完整测试的四次CLI调用。工具实现已完成，最终cloud coverage/build/disk三个Node suite实际146通过/0失败/0跳过，其中coverage133（原63+新增70）、build8、disk5。回归包括同流对象选择、默认兼容、双对象累计准入、严格映射及故障保全；非作者审查发现对象变更与lookup污染双故障会短路终态检查，已拆开独立检查并补组合反例。Windows实际双对象采集仍NOT_RUN，Node模拟不证明本机LLVM产物规模适配。原600秒、raw+merged128MiB、累计evidence256MiB和16MiB分片不变，实际超限不删case或自动扩大预算。目标只补该invocation的两个对象映射，不能证明每个子进程profile均完整，也不是全仓覆盖率。Stage2、完整Skill/压力/实机及70%性能目标仍待。

### DBCD 四项确认修复已有红绿回归（2026-10-19）

用户明确要求修复四项，并要求保留现有Skill product定义。独立审计固定DBCD `e732093f8864240fc5884bd1bba6b02f3dfc0d56`、WoWDBDefs `b9a63096cdd99b26967f17f9515377ae62cadfd7`；前三项固定Go323同样存在，不能以Go同错结果作为正确oracle。

- F1：仅WDC5 palette物理区遵从所有scalar Pallet后所有PalletArray，各类字段顺序保留；WDC2/1SLC/3/4、common/bit offset与严格size/index拒绝不变。
- F2：metadata值行数值后保完整多词Name，注释仍独立解析；mapping token/path规则不放宽。原每行logical160保留，安全计量另计实际owned Name/Comment UTF8字节并在分配前准入；这是旧已有字符串少计费的修正，极紧budget可更早拒绝，不声称所有错误时序不变。
- F3：只metadata BUILD支持官方12个精确预置范围，采用官方preset分量匹配；未知值拒绝，原数字版本范围语义保留。TBC/WotLK等不成为产品alias；现Skill产品名称/对应关系、两条Forever路由完全保留。
- F4：统一从请求实际max-bytes生成parser caps（metadata64MiB、rows1M、columns/partitions4096），Prepared保存同一limits贯穿prepare及所有二次open；SDK默认值不改，不引入product专用parser。小fixture的512MiB请求传递不等于已验真实>256MiB文件。

独立测试-only版本 `51d430690d2799deb1a1159f4f7a9169e236a0d3` 已实际通过all-targets；13项为5通过/8失败、无忽略。palette为2通过/1失败，实际数组[900,100]而正确期望[100,200]；metadata为1通过/3失败，enum/flags返回records.definition_identity，Weather返回schema.invalid_definition。请求限额为2通过/4失败：1000001行及4097物理列/总列/partition均在每个测试的首个data db2消费者返回table.invalid_format，期望table.limit_exceeded；后续消费者断言尚未执行，不能把这一轮描述成全消费者覆盖。

四个owner均封存退役/回收，activeProcesses为0、无deadline；三个失败目标的辅助进程在drain后被强制回收且knownAuxiliaryCleanup/reviewedAuxiliary均false，原收尾异常保留，不能标成clean成功。以上仅非敏感结果汇总，未发布本机原始日志。

最终测试修正版本 `9004f3cc700bf479b0388b10f6ce85cb218699fe` 已实际通过all-targets与完整reader limits5项（0失败/0忽略，1193 filtered），四个原失败均已越过真实准入，执行原限额/副本/动态批次/partial/capture/stream断言。两owner退役回收、native均0，两个forced均为已审核辅助进程、无deadline。与c571中已过且未改的108个非reader测试合并，实际去重为113通过/3个原忽略；这是两个准确版本的定向回归，不是单次全库通过。四项确认缺陷现具源码修复、独立审查和真实红绿证据，完整product矩阵与真实>256MiB文件验收仍待。Skill/product定义保持不变。

以下保留修正前的实际失败截面，不重标为成功：

修复版本 `c5718ffa43bf3df575ab808cb4b8aa584545af6b` 已实际通过all-targets，116 matched为109通过/4失败/3忽略；核心36为32通过/4失败，旧13项复现测试全部通过。分组：metadata单元14通过；reader limits1通过/4失败；palette7、metadata公开4、limits公开6全部通过；旧semantic SQL11、WDC16通过/1忽略、catalog7通过/2忽略、stream23、SQL lazy20通过。三个既有fixture/benchmark忽略不算通过。

四项失败均在reader_limits_tests直接prepare/prepare_following阶段返回records.internal（Query accounting used before workspace admission），后续限额/副本/dynamic/partial目标断言未到。根因是新增测试直接构造pending query budget后调用prepare，遗漏真实workspace admission。后继仅改reader_limits_tests：经OpenedScope.admit和同一budget/local session运行原prepare_scoped/二次parser断言，成功后原source校验，local reader及scope checked finish之后才执行独立公开请求或删fixture；不修改生产/手动激活预算/减少断言。该截面后继当时尚待实测；其最终结果见上文9004，不覆盖本次失败历史。11个owner退役回收、无deadline；九个forced均已审核辅助进程回收，无unknown。原51d失败与其异常收尾不被本轮覆盖。本次仅非敏感结果摘要。

15/15修复文件及27份Skill/product定义字节已核；核心清单36项中本任务新增/扩展29、既有7。小fixture未证明真实>256MiB文件能力。Stage2和完整性能验收仍未结束。

### Facts 编码所有权小修已通过定向回归（cb409）

源码确认 `encode_facts` 原先把已经构造的 Facts Value 再经 `json!` 复制成第二棵DOM，且为 canonical SHA额外生成整片Vec。后继候选沿同一Value/canonical writer，把信封先构造为facts:null，再移动原Value填入；原最终完整编码、32MiB判断与错误、profile及发布边界不变。适用冷解析、cache miss与repair/backfill的FactsSerialize，不是增量FactsRead主桶优化；不会把数千Read次数误称重复body读取。现每cohort目录与每record的外SHA/内部canonicalSHA各有独立认证职责，保留。

四项新回归以完整旧producer为byte/error oracle，核五数组、顺序/重复、Unicode/optional/usize/skip字段、8KiB边界和32MiB−1/恰好/+1。非作者独审与syntax检查后，固定 `cb409837eda576b70c42b6b48e841e43df018c2f` 已实际通过all-targets和完整index::141项（0失败/0忽略，570 filtered），四项新增各执行一次。原600秒/default stack不变；两个owner退役回收，native均0，已审核辅助进程回收标志保留，无deadline。本批未release或性能复测，没有新收益结论。

### d427 低耦合投影实际验证与匹配性能（2026-10-19 01:06 UTC）

- all-targets通过；200项定向Rust回归全部通过、0失败/0忽略。Source index137、query_store8、inspect15；Data projection3、output5、sparse1、regression15、CSV8、EXPLAIN5；CLI3。18新增逐名各执行一次，包括真实128MiB+1缓存上限。11测试owner回收，无deadline；既有辅助进程回收标志保留。
- 同一固定版本首次release与assembly均成功，原构建预算未延长；随后六场景12测量及4准备完整Facts/顺序/summary/引用custody与身份均通过。测量10owner退役/回收、20native exit0，无forced或deadline。
- Go/Rust毫秒：冷7730.4355/4036.1422（0.5221），暖559.6647/345.8190（0.6179），单改2740.9258/1127.5592（0.4114），插入2725.3417/1156.8068（0.4245），删除2726.4203/1149.3939（0.4216），重命名2842.2565/1168.1344（0.4110）。六项仍未≤0.30；仅一轮诊断，不是20对验收。相对a860的时间小幅波动不证明因果改善。
- warm cached阶段240.625ms。Counted逐项释放已进入decode，旧full集合在调用方释放，不能直接比较phase推导独占收益。多数内存样本有缺口，未证明RSS改善。此次明确验证了共享typed schema、按需保留及SQL结果序列化的业务等价；架构收益与耗时证据分别记录。
- DBCD审计四项发现仍仅报告/测试准备，未修生产且不计本批。完整Skill仍false、真实OS压力/游戏与Stage2后续覆盖仍待；Emmy保持零补丁。以下旧版本结果和原NOT_RUN截面保留历史身份。

### 最新已验证结果与低耦合优化方向（2026-10-19）

最新已推工具/fixture批为`9693ba0af6ebb8ef5eaff1e3136753680dd2630d`；最新已验证生产仍为`a8604d0dabf6b7ecefa8f15409bda2bf9fbaf8be`。以下为唯一Windows协调线程的非敏感结果汇总，旧失败/时间截面的记录原样保留，不另公开原始本机日志。

- a860匹配Source六场景全部完整Facts/顺序/summary/custody通过。Go/Rust毫秒：冷7167.8766/3944.7180，暖533.6527/351.4062，单改2701.0428/1130.8321，插入2609.6107/1168.7120，删除2617.4956/1130.6978，重命名2592.3705/1155.7738。六比值约0.419–0.659均未≤0.30，仅单组诊断；Go也随运行条件变快，跨run变化不能全部归因优化。10owner/20native成功回收，无deadline。
- a860正式coverage共12个独立integration对象：130测试通过、1项真实权限ignore，全部test/merge/export成功。前五组为Core capture_dependencies、Data sql_csv_parity、Source source_inspect、Live mailbox_lookup、Delivery slot_maintenance；后七为Core budgeted_capture/capture_query_references、Data sql_lazy_provider_parity/relational_parity/sql_explain_parity、Source flow_parity、Delivery release_control。每份分母为该EXE映射的自有crate文件，含关联模块和测试代码，不相加/平均百分比，不称全仓覆盖率。后七47/0/0、14owner/70native回收；前五83通过/1ignore、10owner/50native回收。Stage2尚未结束。
- 9693真实CSV准备与诊断：Node28通过/1文件链接EPERM skip，all-targets通过，ignored numeric exporter明确执行1次通过。独立producer提供同一16,384行输入，旧63d/新a860各冷暖共4次测量及8次CAP验证，CSV/sidecar/双CAP/最终身份均通过。冷401.4790/389.9062ms（新/旧0.9712），暖349.0380/324.7937ms（0.9305）。两组实际retainedBytes减少46,988,672B、decodeWork减少81,920，均为精确准入计量差而非RSS；单pair不是20pair或Go目标验收。6owner全部回收进程，其中5retired、1成功fixture保留；44native成功，无deadline。

用户进一步明确低耦合、高通用性与性能共同衡量。当前新候选复用同一typed Index schema及认证入口，按消费者保留完整集合或逐项完整解码后仅计数；Data直接借用已有QueryResult Serialize，删除同业务重复Value投影。Source新增15项、Data新增3项正式回归；Source/Data最终7文件非作者静态复核与语法解析通过，全部新Rust编译/执行/性能仍未运行，不把上述旧产物结果当候选验证。Source有序投机队列与Data纹理不相交借用分片的调度/退款语义不同，已有OperationControl复用保留，不为统一而制造线程池或泛预算框架。逐项释放会进入暖decode/elapsedMs，而旧集合在调用方释放，因此phase口径发生相应移动，只有完整native wall及有效内存观测才能验证收益。仍保完整raw/SHA/逐Symbol真实分配；完整Skill仍false，OS压力/实机与≥70%目标仍未闭合。

### a860正式定向与coverage最小链通过（2026-10-18 16:25 UTC）

唯一Windows协调线程报告固定`a8604d0dabf6b7ecefa8f15409bda2bf9fbaf8be`全批终态：all-targets成功；Source index122、Data output5、sparse exact1、Data close15、CSV8、CLI3，共154项普通Rust全部通过，Source6/Data5新增逐名各执行一次。Node75通过、1项真实文件symlink创建EPERM跳过，未执行断言不算通过。

正式coverage选定Core测试1通过，LLVM merge/export均成功，3.1.0 schema被接受；raw/binary/merged分别2/6/1片完整认证。该单测试报告只涵盖其instrumented对象内146个项目Rust文件，另外380文件被路径过滤排除，不能据此称全仓分母、全仓覆盖率或Stage2完成。10owner全部回收、18native成功退出，6次forced均为已认证辅助清理，无deadline，结束时lease/进程为空。仅保存非敏感结果汇总，不附受限本机原始日志。

当前a860首次匹配release与正式assembly已由唯一队列实际成功完成并回收；同一产物Source六场景新性能及五个integration coverage报告尚待终态。两处生产优化已经过上述真实正确性回归，但没有新匹配产物性能结论；旧63d结果不能归入a860。Data CSV规模化诊断另准备最窄可选工具，仍须完整CSV与双CAP验证，既有小SQL样本不证明收益。历史coverage三次失败、平台跳过、性能未达0.30、完整Skill/实机压力未验保留。

可选Data CSV诊断已完成独立静审及新增Node29/0/0：固定16,384行真实WDC fixture，独立producer身份与旧63d/新a860产物身份，完整CSV/sidecar/双CAP和精确计量差验证，默认1pair四次测量，不改B04。Rust exporter与实际Windows性能尚未运行；广泛Node wildcard因partial clone潜在懒拉远端被拒，未拆子集绕行。原Data5GiB及fixture256MiB/outer1GiB准入保持，实际空间不足须诚实停报。五个integration coverage目标准备就绪，仍分别报告object映射分母，CLI多object与Delivery分组选择缺口未伪报补齐。

### 当前Source诊断与下一最窄算法片（2026-10-18 15:55 UTC）

唯一Windows协调线程报告：固定5859 runner与封存63d产品的新六场景12次测量全部完成，full Facts/字段顺序/summary及实际引用custody均认证，formal exit0；最终10owner/20native均成功回收，无forced/deadline，结束时lease/进程为空。Go/Rust native毫秒依次为：cold7912.9545/4345.4009，warm592.1216/438.2824，单改3363.6330/1209.7676，插入3073.8941/1198.1737，删除3021.6315/1180.2194，重命名2963.5203/1227.8438。六比值约0.5492/0.7402/0.3597/0.3898/0.3906/0.4143，全部未达0.30，均为单组诊断而非20pair验收；所有内存采样有缺口。只保存非敏感汇总，原始本机日志未另公开。旧高系统commit异常那轮不改标成功。

按已证真实消费者，下一生产片仅去两处无用工作：Source认证Facts时用相同serde_json序列化器写入固定8KiB缓冲SHA writer，替换canonical Vec再hash，保全部字节/错误/control/损坏回退语义；仍保Value DOM与typed decode，不把整个FactsRead当收益。Data CSV分支不再构造从未消费的public number layout，非CSV仍在原capture前构造，CSV双CAP/manifest/精确数值输出及已提交事实保持。CSV实际safety资源used会降低，极紧预算下不再因该无用布局拒绝是预期改进；不宣称资源诊断字节或所有预算失败时点完全相同。

Source新增6项独立旧to_vec oracle、分段/缓冲边界、深unknown/重复/数字、取消过期、loose损坏恢复与默认pack真实消费回归；Data新增5项真实CSV与普通SQL数值/CAP、冲突后已提交CAP、执行后取消与线程局部观察器隔离。新Rust测试仅语法解析，最终非作者静审通过；Windows/编译/吞吐尚待验证；较宽warm摘要Index读取和materialized blob生命周期方案未实施。Stage2有效性/覆盖率仍未结束，本片不代替最终审计。

### Coverage第三次Windows复验：归档及LLVM成功、JSON版本门禁待修（2026-10-18 16:00 UTC）

固定9e38实际Node70/0/1EPERM、选定Rust测试1/0/0；10个进程步骤包含LLVM merge/export均native0。大profile、instrumented binary及merged均按原单片16MiB和总量预算成功分片认证，3owner退役，原scratch按已认证证据安全清理。最终collector拒绝LLVM23实际export版本3.1.0，报coverage.export_schema；没有已接受覆盖率报告。本机没有扩大预算、修改代码或重跑普通Rust。

官方LLVM23.1.1的[JSON exporter](https://github.com/llvm/llvm-project/blob/llvmorg-23.1.1/llvm/tools/llvm-cov/CoverageExporterJson.cpp)明确version3.1.0，summary-only所消费functions/lines/regions的count/covered与旧2.0.1相同；详细MC/DC变化不属于当前采集分母。最窄修复显式支持2.0.1与3.1.0，保严格type/data/files及项目summary计数/范围校验，未知版本仍拒绝；不将依赖totals或MC/DC混作项目分母。测试fixture按官方结构合成，非本机原始export外发。最终非作者独审通过，Linux三suite76/0/0含5新增，覆盖3.1.0完整526文件合成链、未知版本/type/shape/计数及native0后schema失败保全。新Windows最终报告仍待验证，Stage2未闭合。

### Stage2 Windows定向回归与首个coverage失败（2026-10-18 15:16 UTC）

固定`5859d3f89beaa9fbd2560aa8199e4ceaec7d8d36`已完成all-targets check及19个互斥Rust组134/0/2ignored、11个Node文件438/0/6skip，所有普通组wrapper/native成功。Core/Live/Delivery有效性、SQL真实CAP/CSV关闭后恢复、Source实际worker/取消和普通XML持久CAP checker均实跑；不是全workspace或原ignored native XML完整链/真实npm安装验收。六Node均实际file-symlink创建EPERM，两个Rust文件链接测试显式ignored，未执行的断言不计通过。完整分组/逐名跳过见[终态6251526895](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6251526895)。

既有stable单独安装官方llvm-tools组件成功，Rust1.99.0/LLVM23.1.1匹配，rustc/cargo/default配置hash前后不变。但首次正式coverage失败：[6251518367](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6251518367)。直接toolchain Cargo编译exit0，既有owned层因该Cargo不是reviewed启动入口而helperAdmission=owned.unreviewed_cargo_identity，VCTIP留存后按原合同forced收尾；不是编译/断言/deadline失败。test/merge/export未执行，profiles=0/complete=false，不能解释为0%覆盖。本轮23新owner全退役/reaped，27native记录中12forced（11known auxiliary、1该真实失败），无deadline；旧63d healthy-base active receipt/claims继续保留，不补造历史终态。[488文件证据附件索引](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6251534728)首次公开GET404记录保留；[后续交付终态6251553910](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6251553910)确认同一链接随后HTTP200、完整包字节/SHA、ZIP CRC及488文件逐项校验全部通过，没有重新打包或上传。另行追加原始日志公开请求仍待精确批准，由根线程处理。本次源码文档仅结果摘要与既有PR链接，不包含该原始载荷。

coverage后继只修collector入口与测试：沿原reviewed Cargo launcher，并将请求固定到已完整认证的绝对toolchain路径，保原安装工具hash/版本/sysroot、辅助工具字节/签名、原600s及失败回收语义。不改policy或忽略forced；RUSTUP_AUTO_INSTALL=0禁止自动安装，直接Cargo与launcher的verbose身份必须一致。新增7项回归，Linux coverage/build/disk三suite50/0/0，仍只是模拟owned故障/真实文件边界；新修复的实际Windowscoverage仍待验证，普通已通过组不重复；Stage2、性能≤0.30、完整Skill/实机压力仍未闭合。

### Coverage第二次Windows复验：真实测试通过、raw归档受阻（2026-10-18 15:30 UTC）

固定`4bc30967c3f01c778f5c40814592e0f68769abb8`的Node复验49/0/1实际文件symlink创建EPERM；正常reviewed Cargo入口与指定toolchain认证已通过，选定Rust测试实际1/0/0。随后collector把32,916,376B的LLVM原始profile套入16MiB单文件默认上限，报coverage.file_budget；merge/export均未执行，尚无覆盖率数值。两个相关owner已安全回收、build channel释放，原raw保留；这不是Rust测试失败。以上为唯一Windows协调线程结果摘要，未另公开原始路径/argv/身份载荷。

后继collector将完整binary/raw/merged按16MiB以内有序分片流式认证、封存，LLVM仍读取前后校验的完整binary/raw/merged；不把碎片当LLVM输入。build owner仅绑定custody索引，coverage owner保唯一一份payload分片，避免重复大文件。raw总量128MiB与原600s保持，落实raw+merged实际scratch总量128MiB及两owner证据累计256MiB的检查，并为报告保留空间。此前256MiB只是空闲空间预留而非硬累计检查；新检查也只是执行前后观察与失败保留，不是OS写入硬限。超限或身份变化不清理未完整归档scratch，通用disk.seal的16MiB政策不变。最终Linux coverage/build/disk三suite71/0/0（coverage58/build8/disk5，21新增）并独立复审通过，实际32,916,376B raw、超过16MiB的binary/merged、总量边界、短写/截断/篡改、失败后raw变化/新增与receipt增长预算均覆盖；旧16MiB判定mutation在真实尺寸fixture明确失败。正式Windowsmerge/export仍待验证。原5859入口身份失败和本次归档失败分别保留。

### Stage2第三轮有限消费者审查（2026-10-18 10:09 UTC）

基于5ab8012，四位独立审者完成此前未深核的SQL六集成suite/真实事务CAP链、Source九test目标与历史native oracle、完整Data Skill42argv/16CAP消费者、十二打包支持文件。确认六处测试有效性缺口并修复：CSV参数首错、scalar列数首错、XML必需持久CAP/result配对、取消前真实worker触发、安装包native版本匹配，以及SQL最终真实close后的已提交事实保留。均为测试验证不足，未据此宣称产品bug；独立预期/原正常边界保留。

最后一项仅在cfg(test)选择既有post-metadata-close seam，线程局部一次性RAII无跨测试污染；普通SQL/CSV各保close-only、cancel-only、二者共存三模式，真正读取CAP/blob/CSV/manifest、重取资源和同PIN新请求恢复。原手造CAP错误组合器保留并准确改名；生产非test路径仍原scope.finish。所有最终字节独审通过，Linux package Node7/0/0（1新增）；六个受影响Rust文件仅rustfmt语法解析，Rust类型检查/Windows/实际installed/native仍NOT_RUN。共4新增Rust入口、1原组合器改名，无删除正常场景。

撤回一项truncated候选：Core本来禁止complete=true且truncated=true，不能造非法CAP追独立分支。历史npm handoff/console未找到当前调用者，其旧断言和fresh-build适用性留Stage3判断，本轮未重建旧测试系统。完整有限范围和未验边界见[Stage2记录](docs/toolkit/rust-audit-stage2-2026-10-18.md)。此轮修后静态复核无新增confirmed finding；仍需Windows有效性回归/真实instrumentation、特权链接和OS压力能力证据，不能据此结束Stage2或进入Stage3。资源暂停继续，未启动本机任务。

### Stage2正式有限target覆盖率入口（2026-10-18 09:54 UTC）

新增 `tools/rust-coverage.mjs` 与独立Node回归，沿原runCargo ordinary lease持有整个固定工具身份检查→Cargo JSON选定binary→真实测试→LLVM merge/export链；保Rust1.99.0、Windows x64、crt-static、原600s总期限、owner未知不释放、source/Emmy及工具前后身份、有限profile/证据。每次明确一个package与lib/integration target，可选一个exact函数；不安装工具、不自动切toolchain、不生成全仓覆盖率承诺。

独审按最终字节通过；Linux Node三suite43/0/0（coverage30/build8/disk5）。这些是真实文件/owner状态的模拟执行器回归，实际Windows编译器、LLVM组件及instrumented tests仍NOT_RUN。报告只统计选定binary中映射的owned文件lines/functions/regions；映射的ignored测试代码仍在分母，未映射其他target/feature/doctest不代表覆盖；合并子进程profile可能贡献匹配计数，但未添加其他object映射。完整口径与使用方式见[Stage2记录](docs/toolkit/rust-audit-stage2-2026-10-18.md)。本机资源暂停继续，未启动采集，Stage2仍在测试有效性与冗余阶段。

### Stage2第二轮有限断言审查（2026-10-18 09:50 UTC）

独立审者继续核Source/Data/CLI/Skill11完整test/support文件及Core存储/evidence、Live协议/journal/recovery真实消费者，未重复已完成平台库存。确认并修四处测试有效性缺口：Skill EXE反例先恢复并验证launcher避免旧错误遮蔽；Core provider反例不再同时wrong snapshot；Source deadline无CAP检查改走真正batch发布入口并保原1s/索引复用；Live旧500capability的6项预期从生产helper自证改为固定Go合同独立矩阵。只改四个既有测试文件，无新产品bug结论、无删除边界；另省Skill7条纯路径语法反例的无用完整package构造。

两个独审者按最终字节通过，Linux受影响Node两suite71/0/0；三Rust文件仅syntax parse，Rust/Windows NOT_RUN。完整范围、保留的分层义务和有限候选见[Stage2记录](docs/toolkit/rust-audit-stage2-2026-10-18.md)。本机资源暂停不阻云端工作；coverage正式入口另片实现中，固定toolchain/owned channel/有限target，尚无实际覆盖率。Stage2未结束，不进入Stage3。

### Source诊断控制器请求内投影候选（2026-10-18 07:59 UTC）

只读调用链核实：同次compareFacts先readFactDelta，再captureRustCustody→verify、bindComparedCustody→verify，共五次完整index JSON.parse；是顺序重复分配，不是已证五份同时驻留。保留的Facts逐文档处理，长期引用仅document/summary等；没有堆快照，不能据此确定63d异常退出根因。

本片只改runner两个模块与Node测试：首个完整字节SHA/UTF-8/JSON和Facts验证后，用私有WeakMap以真实comparison对象登记请求内投影，仅保documents/summary/bytes/SHA，消费字段与locator副本冻结，不保原root/symbols/Buffer。后四个边界仍认证当前live完整SHA及所有archive parts/catalog/manifest、完整五数组/重复/顺序/record SHA，认证相同才复用；伪造/copy对象不能冒认证，跨home仍受路径和实际source tuple约束。独立verify无context仍完整parse。无新持久缓存/tokenizer/生产改动，产品计时与owner流程不变。

独审通过；Linux Node v24.19.0最终六suite **355/0/0**，44新增（reader12/scenario32），真实compareFacts观察5→1完整parse且独立verify再parse。缺字段/重复last-wins/顺序、source tuple篡改、同路径换bytes、合法另home与越界路径、archive变更和末次复验均有反例。早期新增测试两处期望错误（summary默认字段、原断言文案）保留原失败日志，修期望未改生产逻辑，旧249/42/352等重叠运行不累计。Windows和完整63d场景NOT_RUN，未测峰值/提速，不宣称异常已解决。

外层controller原runOwned未开启memoryObservation，产品子Job样本不覆盖后续比较/归档峰值。既有开关可用于后续独立观察，但当前未实施：须按原policy预留64MiB而非现16MiB，100ms/15000样本上限约25分钟，耗尽标gap；Node PID+FILETIME计数与包含后代的Job总量分开。当前本机仍暂停，OS压力/正式coverage能力待恢复后验证。

### Stage2平台条件完整扫描与第二小批（2026-10-18 07:43 UTC）

[平台清单](docs/toolkit/rust-audit-stage2-platform-tests-2026-10-18.md)按固定05649树检查1069个Rust、342个Node/TS文件的cfg/平台分支/skip/早return，列出74个自有显式ignored（含16宏展开）及1个vendor项。三个Windows覆盖缺口已独审修复：Delivery integration有效managed目录junction与独立需权限文件symlink；Core发现路径同时断言无候选且无issues、显式alias仍正确且外部bytes不变；Node候选收集器真实file-link单列精确拒绝，权限不足仅skip该项。另Data非Windowsfallback测试为支持平台不可达覆盖，明确列出，不当环境待验。Linux受影响Node34/0/0；三修复Rust/Windows NOT_RUN，不宣布Stage2结束或覆盖率。

63d Source新诊断07:33开始，4prep过，cold完整核验Go9.668s/Rust5.465s（ratio0.5653）；warm未闭，后四场景未启。系统commit99%且旁路shell报页面文件不足，[现场报告](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6247196057)。controller在停止指令到达前07:36:08异常退出native3221226505、forced=false/deadline=false；[终态报告](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6247207139)。07:37:34原11条native记录PID均不存活、sampler退出，outer failed/handlesReaped=true；healthy-base854baae1…原receipt仍active且回收字段未完成，不能称所有owner封存。原scratch/custody保留，不当主动取消或产品根因。高commit影响现场，只是未完成诊断，不作性能验收或回归定因。新的本机队列暂停，不自动重跑；不是OS压力验收，既有b4d完整结果与性能未达仍保留。

### Stage2首轮测试有效性候选（2026-10-18 07:31 UTC）

全仓首轮范围矩阵与本有限修复见[Stage2记录](docs/toolkit/rust-audit-stage2-2026-10-18.md)。两名独立审者覆盖Source/Data/Live/CLI/Core存储/Delivery/package/Skill的真实消费者与fixture层次，确认不可达平台负例、错误原因可替代的弱反例、静默跳过和重复无用setup；不是新生产bug结论，也不是完整覆盖率审完。

本批仅测试能力/成本修复：Core Windows junction保原维护锁拒绝/close；Live active junction与单列需权限file-symlink；Delivery159名称不变，4锁例去无用500槽构造，12组合/双public apply/42快照保留并用真实share句柄拒同字节重写，扫描中原control取消使用局部thread-local RAII cfg(test)观察点。Node valid-pack负例精确断因，Windows目录junction实际执行，file-link权限只skip该单项。独审通过；Rust/Windows NOT_RUN。Linux原三suite288/0/0，canonical期望修订后仅受影响runner39/0/0复跑；同字节其它两suite不重复累计。无产品预算/默认栈/期限变化，不凭静态少6000文件写宣称提速。

63d原始附件[最终公开交付](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6247070485)已确认472文件、921394B、HTTP200/CRC/整包与逐文件SHA全过，11owner回收；前段待交付状态保持其原时间。当前Stage2先修有效性/冗余，再测覆盖；真实OS压力仅最小能力设计候选、coverage工具尚无正式接线。新测试由根唯一队列排，不改变已封存63d性能诊断产品身份。

### 当前阶段：Stage1离线范围结束，Stage2开始（2026-10-18 07:18 UTC）

固定产品 `63d47f29f6c9efe2511ada5969426a184eca1830`，sourceDigest `c3129a4b906142c92f4b048af363b501ff98fb3842b4bf80e10794bb4920da6e`。本轮已确认、可离线修复项均完成代码修复、非作者独立消费者复核和对应实际回归；以此有限范围结束Stage1，不声称所有环境/功能/性能验收完成。下方带时间的候选与失败段落为历史，不覆盖本当前结论。

- [同SHA Delivery逐名覆盖](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6246851973)：互斥exact1+prepared8+slots反例2+其余journal8+非addon140，共159项，无重漏，全部成功回收；独立all-targets通过。原0caa及63d单次整lib的600000ms deadline失败保留，不能称单次完整159成功。测试期限/默认栈未变；release沿既有1800000ms。
- [匹配产物](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6247021119)：首次一次release/assembly/fresh exporters成功；EXE45440000B，SHA256 `027baf4725da41b1a085eaf457050912a8f3a96972d334aef51d5a3bb5326e97`。三输入与原Emmy身份完整认证。
- [真实已安装Data Skill](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6247021457)：42children全部exit0，26/26checks，38公开CLI含16次evidence verify，16CAP全验；真实npm pack/隔离安装、catalog ANALYZE/CSV、缺钥partial及同PIN换key恢复均过。42是调用数，不是42测试函数。含内部metadata共11新owner全部退役回收，无deadline；原aux标记照原报告保留。
- `completeSkillAcceptance=false`：Live增量与真实OS Job压力拒绝/释放/恢复仍未验；logical headroom fixture不替代OS压力，压力不等于游戏验证。2个Windows file-symlink权限负例、不可达Unix-only测试有效性、真实CASC/key及原ignored等缺证分别保留。§11实机尾部原内容不变。性能至少70%改善目标仍未达到。
- 原始日志/receipt/owner附件公开上传已获用户精确批准，根协调线程正在交付，尚无成功结果；本次只更新已公开结果摘要和链接，没有上传受阻附件，不能称最终附件交付完成。

Stage2正式从测试价值和冗余开始，随后评估有意义覆盖：两名独立审者分别核Delivery真实159用例/重复durable fixture/互斥集合口径，以及不可达Unix-only、平台负例、Skill消费者和真实OS压力测试能力。先证明重复工作无独立价值才提出清理，保phase/恢复/unknown不重放/取消/预算边界；不凭名称相似删测试、不刷百分比、不扩大产品预算。当前为只读首轮，未产生新的测试通过结论；Stage3–5尚未开始。

### 0caa真实Delivery期限失败与测试fixture复用（2026-10-18 06:16 UTC）

9c2原正式队列已完整终态：[1960pass/0fail/7ignored](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6246301734)，新增32逐名各执行一次全部通过，7owner退役回收无deadline，132文件公开HTTP200/SHA/CRC验证。此成功绑定9c2，不覆盖后继新代码。

0caa新Delivery[原600000ms正式调用](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6246412512)失败：159 selected仅24启动，23已ok、最后1未完成、135未启动，无最终test summary；14新增中13ok。未完成原名slots_checkpoint_resumes_matching_and_reordered_receipts_without_replay。wrapper1/native8，forced=true/knownAuxiliaryCleanup=false/deadlineHit=true，owner2bea6b6e-5919-44fc-a971-df773de261fa retired/reaped，原证据保留，release/实际Skill不得据此继续。

云端作者与独立审者实读锁链：maintenance与installation资源不同，apply显式finish，verify不重取这些锁，底层try_lock冲突即时返回；该静态trace未发现重入等待，但原日志没有子case时间，不能宣布动态死锁已排除或IO原因已证。静态计数该函数12套真实500-slot fixture，18000次slot持久写、54000次完整slot读取，加36全tree快照中的54000次slot读取；600s同时覆盖编译与此前23个测试。

本机原现场只读补证：原进程05:58:22.3529936Z开始，编译13.02s；唯一PID39972残留fixture尾号45，其journal仍main/archive=null、slot manifest为installing计划500，实际连续slot1–449、资源未重排。该现场把未完成阶段收窄到真实slot构造尚未结束，不证明具体阻塞调用、死锁或准确耗时归因；未修改现场。独立Data numeric unit2/0及CLI3/0已实际通过，21query+21verify/21CAP均过，相关4owner回收；0caa新增17为16过1未完成，release仍关闭。

本后继只复用测试fixture：fresh/upgrade两种origin各真实生产一次durable slots checkpoint，保存原完整journal bytes；每个原request/reorder组合前核journal与全tree，全部guard实际close后才恢复原checkpoint，不手造phase。12组合、两次public apply、500slot及36次完整inspect全保留，构造12→2、slot持久写18000→3000，完整快照36→42。局部test-only elapsed进度直接stderr写出/flush，便于同限后继定位；生产、期限与预算不变，0新增测试函数。修后Rust NOT_RUN，不把工作量下降冒称动态原因或已通过。

### 本轮精确实测与再次状态机复核（2026-10-18 05:52 UTC）

38a7 Delivery完整Windows 145/0/0，新10逐名通过；原aux forced标记保留。9c2 all-targets、Core cache65/0/0（新6）、Probe16/0/0（新4）、Source lib682/0/4（新13）实际通过，见[本机报告](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6246251776)。截至本记录Data仍运行，catalog/WDC未开始，不能把Data9新算过或跨SHA拼整仓全绿。

三名新非作者在固定9c2按真实消费者再次复核：Data五条原source/checked-close/CAP/stream/scalar数组链，以及Source认证header条件、Core retention和Probe持久时间链未发现新可操作问题。Delivery全phase矩阵仍发现同一恢复身份范围的补漏：slots→complete只核版本；prepared归档无nested时未核冻结previous；已有nested previous/next未绑定combined；无archive fresh重新读取release所得receipt未在发布前绑定wanted。真实合法checkpoint/升级可触发，无需伪造journal。最窄后继在原读取点绑定身份，保合法prepared/old-moved/installed恢复及已有错误事实，不重放；本后继3文件14新入口覆盖真实durable checkpoint与same-version内容/commit变更、nested三合法恢复态及源重采样；新增回归仍NOT_RUN，Stage1继续。

[99a runner/0002产品真实Skill](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6246189044)已通过候选Skill认证与Node70/0/0，19/42 children中15次公开CLI执行，4/16 CAP已verify；checks13pass/1fail/12NOT_RUN。catalog-analyze在成功业务结果进入public renderer时exit5：EXPLAIN/ANALYZE发rows:null，数字布局却声明Indexed数组。owner sealed failed/reaped，无forced/deadline；原失败与已提交CAP事实保留，原失败CAP字节未由云端独立读取。修复按typed plan选择严格布局，普通空/非空数组、参数词元及canonical浮点不变；3新增真实CLI测试复用原catalog fixture，覆盖21 query与21 evidence verify调用（均NOT_RUN）。四文件独审通过，新增dev edges仅引用原已锁定依赖，无版本/checksum改变。通用render错误stderr不显示已提交CAP身份的既有边界未在此有限修复中扩大处理。

### 本轮其余Source/Core/Probe有限候选（2026-10-18 05:30 UTC）

本轮8个源码确认边界已分别完成候选：Delivery3保存38a7，Data2保存557，现其余Source/Core/Probe三项收成有限批；实际Rust复验仍由本机固定队列执行，不能凭代码/独审把Stage1关闭。Skill runner另保存99a，9文件257473B逐blob读回，Node70/0/0，真实0002产品+99a runner尚待后继结果。0002上一Skill原统计应含assembly metadata子owner：8 retired加1 sealed failed，217文件证据保持原字节；不是先前简写的7个总owner。

Source flow v2借用当前v15/v6完整认证index中持久TOC header symbols，重复/顺序/空值保留；原raw入口与新borrowed入口共用自有LoadContext helper，支持条件/Family/Game/嵌套XML并让未知明确partial。无需重新读取TOC原文；未提交v1的独立32MiB累计/每TOC1MiB限制全部取消。header计入原Lua输入total，共用单一32MiB，不宣称RSS上界；现有遍历/issue/取消边界保持。13新回归及12原遍历测试，新增均NOT_RUN，v2独审通过。完整公共worker/CLI仍需匹配树执行。

Core retention marker改为同handle、原control有界读取，65536B接受、最多读取65537B后保护错误，增长/损坏不得删对应对象，所有取得handle路径checked-close并保已删除事实/主错误。6新正式回归含11子案例、其中5经public cache dispatch，1走真实observed prune；没有新增native close故障注入，RustNOT_RUN。

Probe只拒持久Name.updatedAt/Revision.createdAt的Go零时刻，原chrono语法与合法near-zero/epoch保留，unnamed show合成零值不变。4新真实registry消费者回归覆盖等价零时刻表示、拒绝时无部分写入、正常值/合成fallback/原revision完整性；RustNOT_RUN。Core/Probe均经独立静态精确字节复核后保存，不能替代执行。当前仍有2 Windows symlink跳过、不可达Unix-only测试有效性问题、真实CASC/key fixture、Skill压力/最终分发重建/游戏/性能等独立未验范围。

### Skill runner精确候选身份修复（2026-10-18 05:25 UTC）

Data后继`557553731d2d90802686aa4ef324c5632f021637`已保存，9新Rust测试NOT_RUN。独立Skill runner修复不改产品/Skill正文：完整25文件Rust candidate Git tree及每文件Git blob/SHA256独立钉住，固定Go23文件另保；完整差异确认15原文件未变（包含Data/Live/launcher）、8引擎/安装/候选验收文字适配、2候选边界reference。命令路径/变更性质/flags共84项合同与固定Go一致。安装和归档均核全bytes，拒缺件/增件/重复/修改及自洽伪造inventory，替代错误的第二层Go SKILL.md全字节要求。

作者和独立审者各运行同两Node suites，均70/0/0，8新增反例包含身份与篡改边界；不累加为140个测试。六runner文件独审通过，Windows新实际流程仍NOT_RUN。正式runnerRoot与产品cwd分开，clean0002 witness仍须匹配原artifact/exporter/sourceDigest；六共享identity/owner工具逐字节相同，运行前后完整认证不放宽。可复用原0002已封存产物、在新owner/fresh fixture运行，不重建产品或修改原失败receipt。准确入口见tests/rust-skill-usage/data-README.md。

### 当前有限Data修复与0002真实Skill失败（2026-10-18 05:23 UTC）

Delivery三项后继已保存为`38a7a376ad5f74c1f63cd7c7a606827d3053859f`，7文件232005B完整读回、独审通过，10新增Rust入口NOT_RUN。随后Data10独占文件已完成独审：五个本地消费者保留原QuerySource/CASC观察owner至capture及checked-close，验证原选中idx generation/size/SHA和当前安装信息；DB2/stream、spell profiles、catalog、encounter不再仅验证workspace或重新采样source。stream已交付行不能撤回，失败保留原行事实、不发成功End、不新commit或重放；encounter第二次source变化保第一CAP。scalar外键/导航拒绝int-array字段并给原field_type_mismatch，普通search数组语义不变。

Data新增9正式入口包含33个profile变更子案例、三个stream sink变更/恢复、数组类型与普通search/scalar正例及第一CAP保留；均NOT_RUN。保留两个真实CASC owner使callback失败+取消多两层现有close错误信封，原主错误/细节、close与workspace事实均保留，仅一个既有测试按真实owner链更新，不引入通用error聚合框架。静态格式/前像/10文件完整字节/固定Go3文件与独立代码审查通过，不能代替Windows执行。

0002匹配release/assembly/exporter已完成，但[真实Skill结果](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6246011201)失败：4/42 children（两个exporter、npm pack、npm install均exit0），checks2pass/1fail/23NOT_RUN，CLI0、CAP0。installed SKILL.md与候选一致，却额外要求固定Go逐字节hash，阻于npm-installed-package；该批7owners退役回收，217文件证据公开HTTP200/CRC/SHA核过。原失败不改标。独立runner修复正在核既有Rust Skill差异、精确候选完整树和runner/product双身份，尚未实际重试；不得用38a7或后继sourceDigest冒0002EXE。

Source v1额外TOC原文读取候选未提交，现改为已认证持久header symbol借用视图，与原raw条件入口共享解析，不新增独立32MiB或每TOC1MiB限制；v2仍待独审。Core retention和Probe持久零时刻独立修复继续。Stage1有本轮8个确认边界未取得新实测，仍未结束。

### 当前修后再巡检与真实验证（2026-10-18 05:14 UTC）

`0002ce02bec5fb7c9c31ae40fd96eadf0da17dc6` 的[定向前置](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6245862158) all-targets通过，local partial4/0、remote partial2/0、catalog7/0/1、WDC16/0/1，合29/0/2；四项CSV失败逐名取得后继通过。`1f3454`原Data lib1161/4/1的四失败均为CSV，保持原历史，不把跨SHA组拼成全量0002通过。0002唯一匹配release正在本机原队列构建；assembly、fresh exporter和实际安装后的Skill尚未取得结果。

既有21项已保存修复都有相关Windows离线回归映射：Delivery128、Core cache59、Probe12保持6a93身份；removal15/coordination4保持e974身份；workspace33/archive4/CLI init6保持cbff身份；Source recovery2/inspect15保持22da身份；Data四消费者及catalog/WDC保持0002身份。通过的是这些精确入口，不是整仓全绿或Stage1收敛。

三名新独立审者在0002完整树按真实消费者再次静审，确认8个可操作边界（含原项补漏，不将旧21项测试结果撤销，也不简单相加为29独立问题）：Delivery三处acquire丢原control、已完成journal未核精确wanted、等价资源重排找不到旧journal；Core retention marker无65536B上限；Probe持久零时刻未拒；flow条件TOC静默漏文件而子LoadPlan为Known；Data本地source原索引观察未保至capture；scalar数值关系字段漏拒array。详见Stage1记录。本轮全为源码确认，新增回归尚未执行。

本有限Delivery后继保存三项修复：三个公开入口传原control；completed journal必须核当前完整receipt；exact路径优先，缺失时有界guarded扫描唯一等价旧journal，保历史filename/hash/schema，歧义失败不重放。两个独立文件组共10个新正式回归，静态独审/格式检查完成后仍标NOT_RUN。其他五项由独立作者继续，不混入此有限批；本机固定0002不追新head。

未测边界分开保留：两个Windows file-symlink EPERM负例；Data lib真实加密CASC/认证key fixture；catalog exporter与WDC same-work性能实验不是同一种ignored；Skill exporter与实际流程仍待运行。DLV-1 cfg(unix)重定向测试因Windows-only依赖图在Unix不可编译、Windows又不编译该测试，属于Stage2测试有效性缺口，不能永久冒称仅缺机器；未引入平台shim。真实OS Job压力恢复、最终安装/升级/卸载证据映射、独立fresh Cargo homes重建、H01–H09游戏项及性能/完整内存验收仍open。Stage2未开始。

### 当前执行：正式五循环 Stage 1 已发现实际业务缺口

2026-10-17 21:40 UTC 已以生产b4d/文档775启动三个独立读审。首轮确认18项，重建复审及实测另确认2项（capture提交后错误事实保留、init时间规范化），第二轮另确认可读字符串被后缀缺钥误拒，当前共21项：Delivery取消/缺席目标/原release恢复/receipt顺序，Probe分页/原子性与身份/有界读取，Core prune取消与字节账/init模式及归档全链control，Data准入/预算诊断/SQL位置/缺钥partial，Source inspect/flow有界性/Unicode路径。独立作者正分范围修复；代码候选仍需正式离线验证和独立复审，当前实现不能称全量完成，尚未进入Stage2。完整状态与验收入口见[Stage1记录](docs/toolkit/rust-audit-stage1-2026-10-17.md)。性能目标仍open，下一性能算法暂不抢已确认业务缺陷修复。

### Stage 1 当前保存、实测与后继候选

首批 `6a93a859d80efd6d757fcaa8b9e85be6285a734d` 的 39 文件已逐字读回；Windows all-targets 通过，正式组共 **567 pass / 2 fail / 0 ignored**，64 新入口为 62 pass / 2 fail，全部实际执行。Delivery128、Core cache59、Live probes12、Data navigate16/resource41/relational302/syntax3、CLI syntax1、BLTE1 全过；CLI init4过2败。[失败原证据](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6244909468)。11 owners 全退役、build leases 空、无 deadline；CLI wrapper 的 forced/unknown auxiliary 事实保留，不能冒称所有收尾无强制。[211文件证据最终公开交付](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6245133188)已确认：HTTP200、406651B、CRC和211/211 SHA全部通过。

两项 init 失败分别是 mixed-slash fixture 与平台规范路径的字符串差异，以及新建 identity 的固定9位小数和 Go兼容重开后的去尾零表示差异。后者表示同一时刻，未证明 identity 重建或数据损坏。独立后继将新建时间规范化，保原clock/ID推导与历史文件字节；CLI测试保完整identity、文件bytes与重复恢复断言。3新增确定性/真实初始化/历史marker回归及修正的CLI测试已在cbff实际通过，见下方准确分组。

第二批 `e9749e9d851cfefa9a02b4b8865aa5cbba3f80ef` 已保存批准的10文件，逐blob及完整字节读回一致：InstallationGuard取得失败和默认remove显式checked-close，以及init/archive原control全链。[本批Windows终态](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6245217858)：all-targets通过，23 pass / 4 fail / 0 ignored；19个Windows新增入口15过4败，1个Unix-only未编译/未执行。初始化1过3败、archive3过1败、coordination4过、removal15过。四失败均为新建与重开createdAt尾零字符串差异，完整ID和时刻一致；cbff canonical后继已实际复验通过，见下方分组；e974原四失败保留。5 owners全退役回收、无lease；removal native/wrapper0、forced=true且knownAuxiliaryCleanup=true、deadline=false，原事实保留。91文件原包后由本机完成公开GET/CRC/逐SHA交付，见[联合终态](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6245724068)，原失败保持。

后继60个去重路径载荷（58个实际变更，2个与父提交相同）已保存为 `cbff043e8cfe54cf30909c61dde94ce287736fb0`，完整1578501B、60 Git blob和字节均与批准载荷一致，最终tree `7b8cec2a020553eb62be1fe94aa02f6d2d6b1519`。该批实际前置已结束（下方逐组记录），以下为保存时的范围与验证状态：
- Source 9生产/测试文件和3 reader文件：真实inspect、Unicode/asset metadata v15、flow显式有界栈。5文件原SHA恢复、4文件重建；37 Rust入口 NOT_RUN。配套reader新Node247/247过，不能替代实际v15产物验收。
- Data partial 30文件：local/CDN缺钥部分表贯穿DB2/SQL/导航/stream/CSV/capture，原始assets/Root仍严格。基础lookup保missing-row语义，仅直接DB2 ID边界升级key_unavailable；公钥离线的完整alias回退真实partial decode。10新Rust测试 NOT_RUN。
- 独立Data capture 8文件增量：DB2、导航、catalog/encounter在提交后cleanup/control失败保留准确capture ID及原错误；仅Rust bookkeeping用safety-only预算。6新测试 NOT_RUN；close测试是实际close后的诊断注入，encounter取消在第二次commit前检查，不夸大故障覆盖。
- Skill 10独占文件使用partial批共享fixture工厂，真实npm安装launcher、HouseDecor IN/CSV及partial同pin恢复、16CAP完整验证/归档已接线。新独立Node62/62过（Data24+Source38）；实际Windows exporter/npm/Skill流程 NOT_RUN，压力与Live验收仍open，completeSkillAcceptance=false。

云目录重置后，精确恢复部分须命中原SHA；重建部分重新独审/测试，不继承丢失版本旧结果。所有后继原文、前像和manifest均另存，已推生产及历史失败不重标。Stage1还需当前真实缺陷修复复验及下一轮独审；Stage2未开始，性能目标仍open。

### cbff 实际前置进展与精确后继

本机固定cbff：all-targets通过；[workspace33/0](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6245556704)含3个新canonical回归、[archive4/0](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6245553521)、[CLI init6/0](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6245562764)均已实际通过，对应旧时间格式/平台路径失败已取得后继实证，历史失败不重标。

[Source真实结果](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6245595382)：lib667通过/4忽略，source_inspect集成13通过/2失败；另CLI inspect6/6已过。第一失败是小PNG缺原始缓存时，asset恢复把既有64MiB上限传入只准32MiB请求上限的source Git入口，在读取真实24字节前即报budget。后继新增asset专用入口，继续复用完整commit/tree/regular-file/SHA-1认证，保source32MiB与asset64MiB两原界限；新增2项真实帧/边界/取消回归及加强原恢复/腐坏测试，Rust NOT_RUN。第二失败是测试错误要求cold cachedIndex=false；既有Rust冷IO省略该键，暖才true。修测试核缺键、真实cold读/parse计数与warm零Git/parse计数，保完整coverage/capture；Go无这些Rust IO字段，不冒充Go协议要求。

Source原owner已退役，native101/wrapper1、forced=true/knownAuxiliaryCleanup=false/deadline=false保留。两项Source后继已保存为`22da4290f352daf10981568d9eee8ee3c1f2bcfd`，5文件完整字节/blob核过，定向复验准备中。DAT-6字符串独立小批已保存为`c9cd3e729c6b4b0b33eeac81a5adbcac1d8b9ccd`，8文件完整字节/blob核过；六新Rust待测，包含于22da固定队列。

### cbff 前置终态与 Data 测试设置修正

[完整终态](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6245678877)：七个Rust调用主组 **1881 pass / 9 fail / 5 ignored**；56个正式新入口逐名执行一次，47过9败，另1个ignored exporter未启动。Source两失败见上；[Data lib](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6245645368)1152过7败1忽略，WDC/catalog集成因lib失败未启动。9个owner全sealed/retired/reaped、无lease；Source/Data原forced=true且knownAuxiliaryCleanup=false保留。164文件430778B证据包已由本机完成公开GET/CRC/逐SHA交付（[报告](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6245724068)），SHA `68d7ed60ec072c0405c35618d8dc2fe8a26aba839b8373c7659c5799848a8afb`；云端未独立重下载该包。

Node原253过54败2skip是协调wrapper将外层6GiB变量泄漏到禁止继承该变量的测试子环境；原失败保留。[同argv独立新owner环境纠正复验](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6245673180)为307过0败2skip，两项file-symlink创建EPERM负例未执行。两次不合并为一次，不改正式工具或预算。cbff release/assembly/exporter/真实Data Skill均未启动。

Data七项实际失败正按真实合同最窄修复：四个BLTE partial leaf和一个table availability leaf直接创建等待workspace准入的预算即使用；另两个local/default-CDN消费者共用空`--query`而被合法search参数校验拒绝。三文件测试修正已完成独审：leaf使用现成fixed controlled默认预算并保原control，真实workspace消费者仍完整admit；search使用合法42并核回显。生产准入/预算和全部partial、capture、CSV、stream、补钥断言保持。0新增测试，原7失败入口后继均NOT_RUN；本机22da定向不追新head。

### 22da 定向实测与 CSV fixture 后继

22da all-targets通过，table字符串4/0、local2/2、remote0/2；[Source恢复边界2/0](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6245720466)及[完整source_inspect15/0](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6245729519)实际通过，cbff两项Source失败已取得准确后继实证。6个owner已回收；106文件包本地封存，未冒称公开交付。Data两个旧空query失败由`1f3454f253be5d1141517df76f027a5ecbff1c76`修测试设置，尚待其整组运行；另两个字符串消费者在CSV使用home内路径，触真实managed-root拒绝，1f未修该问题。

[实际CSV参数/原CAP](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6245744162)确认：local与remote均在gap16的CSV步骤失败，gap512/4消费者尚未执行；原错误保留已提交query CAP、outputPublished=false及嵌套错误。此前静审漏掉该路径。后继4测试文件同时修普通与字符串共享fixture：helper显式接收外部Path，local使用fx.root，remote使用其唯一home的全新sibling exports目录；成功后原测试清理两目录，失败保现场。完整静态核后续manifest/stream/capture/补钥及Skill独立外部输出布局；新增manifest等于返回值及query/CSV两CAP ID绑定断言，保原字节/partial断言。生产禁止向managed workspace输出的合同不变，0新增测试，4个既有消费者入口后继均NOT_RUN。运行中的1f固定队列不追head。

### Stage 1 第二轮：固定 cbff 的独立消费者复核

三名未参与相应修复的新审者只读复核Source、Data和Skill跨模块链。Source/Skill未确认新阻断；真实v15产物和Windows Skill仍待运行，Unicode全表等价未证明，reader仅承诺fresh及明确历史续验链。Data确认 **DAT-6**：WDC可读行的字符串已在NUL处完整终止，但`terminated`一次读取整个后缀，遇NUL之后的缺钥span会误报partition_unavailable，丢失原本完整可读值。最窄后继按连续已认证区间有界扫描，NUL前未知仍必须错误；6路径最窄修复和6项新增回归已完成独审：真实local/CDN gap16/512/4、DB2/SQL/CSV/stream/capture、strict raw和同pin补钥完整恢复，以及UTF8/无NUL/边界/取消/精确分配记账；Rust均NOT_RUN。

另两项保留非阻塞候选：partial空N块是否属于更宽合法支持；encounter第二次commit收尾失败时第二capture ID虽保留于原始嵌套错误、但未出现在外层completed列表。尚无数据丢失或公开合同违例证据，不为Go内部行为机械收紧，不计确认finding。

### 最新实测：b4d认证Facts所有权转移完整六场景

[前置](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6241017528) alltargets通过，index116/0/0含9新增逐名，facts48/0/0；164主Rust项不额外累加新9、Go104输入或helper。Go104完整严格比较通过，受影响Node1/0/0，完整Node仍引用ba0原304pass/2 file-symlink环境skip。

[正式终态/交付](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6241367241)、[17附件索引](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6241356459)：匹配release/assembly各一次。EXE44,728,832B SHA `73a38915daa9c2a696831bab8d6012b0f39bcee33c92edd3e60768b2892b3db4`；artifact `7e6e72f3741463e8fe5c54167e89aa5d729ce027743f90a8ced30a61f91ae500`；sourceDigest `745a539b73063a3fe199719d5dd21e335b32ae9455cc546b46ba72d120c02068`。

| 场景 | Go native ms | Rust native ms | Rust/Go | FactsRead ms | Serialize ms |
|---|---:|---:|---:|---:|---:|
| cold | 8336.8397 | 4380.7566 | 0.525470 | 257.496 | 91.496 |
| warm | 564.2785 | 368.6635 | 0.653336 | 0 | 0 |
| single-content-change | 2820.0586 | 1167.9437 | 0.414156 | 379.089 | 89.825 |
| root-insert | 2795.7986 | 1183.6059 | 0.423352 | 372.138 | 93.240 |
| first-delete | 2785.9070 | 1199.6514 | 0.430614 | 400.388 | 93.848 |
| cross-prefix-rename | 2744.3274 | 1200.7922 | 0.437554 | 393.979 | 91.764 |

12新index+4准备调用，wrapper0/finalIntegrity=true/NOT_RUN空，17新owner sealed/reaped，16retired、工具stage保留。五数组/字段/重复/顺序及共同summary全等；完整实际GoFacts/Rustpack或loose/index/manifest读取tuple归档核过后cleanup。本机17GET/CRC/片与wholeSHA过，52,819files/158,185chunks，包136,680,709B SHA `98ec1942a2bd378220da7ac106646557374f7677582cedaadc3f15104d751de2`；云端唯一作者已独立核全部17附件、52,819文件、六场景Facts重放、12native receipts与17owner；六份完整index SHA也与31a对应场景相同。11内存样本有gap，Rustwarm1项sampled_successful仍非完整RSS峰；CPU/IO未测。12产品native无forced/deadline，编译knownAuxiliaryCleanup原事实保留。

四变体派单仍9/1/8/9；FactsRead372–400ms仍含完整Value解析、canonical hash编码、typed转换与IO，去clone份额不等于整个桶。跨run观察增量总wall下降，但cold略慢、warm路径不经过此take，均不虚构因果或20对达标。下一先独立分析剩余真实成本，同时准备五轮第一阶段正式审计进入条件/剩功能差距，不无限以微优化代替审计。

### 已验证实现：认证Facts子树所有权转移

默认loose与pack两处在原schema/input/canonical FactsSHA认证完成后，用get_mut/map(Value::take)/unwrap_or(Null)替代整子树clone，再进入原typed反序列化。缺key不插入新null；两处原控制检查、miss回退、pack外层完整性与checked-close次序不动，不合并helper或更换缓存布局。只减少重复所有权树，仍完整解析/验证/哈希，不把约480ms FactsRead全部称可节省。

新增正式9项：`index::facts_decode_tests::take_facts_*`八项和`index::facts_cache::tests::take_facts_packed_inner_validation_keeps_outer_integrity_and_exact_recovery`，覆盖历史clone全bytes、五数组/Unicode/可选/重复/深unknown/数值、缺null/坏类型、取消/过期、真实loose/pack/legacy默认消费者与外层认证仍成立的内部损坏恢复。rustfmt解析、diff检查和独立静态审阅通过；b4d正式index116含9新增全部通过，匹配fresh六场景正确性及custody通过，性能仍未达目标（见上）。原31a实测不重标。

### 历史实测：31a借用canonical序列化完整六场景

[前置](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6240462842) alltargets通过，index107/0/0含10新增逐名，facts48/0/0；155主Rust测试不重复加新增、helper或Go104输入。固定Go104严格等价，受影响Node单项1/0/0；完整Node沿ba0原304pass/2文件symlink创建EPERM环境skip，不称31a重跑306或负例通过。

[正式终态](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6240736797)、[完整证据索引](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6240780687)、[交付](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6240808510)：匹配release/assembly各一次。EXE44,728,832B，SHA `72a0c9c09a431f663e3cd8f15e8c5f4197761edad6089f68d26c0200aa35e5f0`，artifact `286d1acce0ef2fa2bf61aaf4ea70b20184b6dcc7abd8f84ce76fe30a1d409800`，sourceDigest `035611e6703efe88f57f506a302d01ec866da7966bc81a74ca1bc6f6285cf62c`。

| 场景 | Go native ms | Rust native ms | Rust/Go | Rust serialize ms | 推导profile余量 ms |
|---|---:|---:|---:|---:|---:|
| cold | 8366.7413 | 4315.2788 | 0.515766 | 88.106 | 5.334 |
| warm | 568.3142 | 387.3169 | 0.681519 | 0 | 0.080 |
| single-content-change | 2834.1901 | 1268.5967 | 0.447605 | 90.648 | 5.191 |
| root-insert | 2937.4934 | 1278.4102 | 0.435204 | 91.469 | 4.823 |
| first-delete | 2910.6008 | 1275.5650 | 0.438248 | 93.667 | 5.078 |
| cross-prefix-rename | 2831.3599 | 1281.2283 | 0.452513 | 92.577 | 6.314 |

12fresh index+4准备调用，wrapper0/succeeded、finalIntegrity=true、NOT_RUN空。17新owner sealed/reaped，16retired、Node stage保留；12产品native无forced/deadline，编译knownAuxiliaryCleanup原事实保留。完整五Facts数组与实际Go/Rust pack/loose/index/manifest读取tuple均归档验证。16附件/52,802files、完整包131,137,681B SHA `b8fcd61effb36761a2dbe8048aa9c15a47313fe434a5aa0046229446a940345d`由本机GET/CRC/片与wholeSHA实际核过，云端唯一作者现已独立核全部52,802files/141,919chunks、12native receipt/ticks、17owner/52,446sealed引用及六场景custody/Facts重放。六份约82MB持久index与各ba0对应场景完整SHA相同，证明字节一致而非性能因果。

余量是profile.elapsed减8个已核互斥顶层span，不是独立测量或CPU/IO归因；不叠加嵌套worker桶、不扣native总wall。相比ba0观察到明显端到端下降，但非交错重复因果验收。warm原本零parse/serialize，不能归因本次serializer；12内存样本均有gap，不报完整RSS。四变体仍实际只派9/1/8/9；single/insert/delete FactsRead约477–485ms，下一最窄候选是在原schema/input/canonical digest认证后转移Facts子树，去clone，不改校验/错误/取消/缓存布局。

### 历史实测：ba0 cache-aware完整六场景

[匹配release/assembly](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6239892161)：EXE44,729,856B，SHA `dd0a750f815f87fbc10c618db060f927127c8aff824dba40dbf41bdbd40ecb36`；artifact SHA `901277b950dca0b7e63a61d09bfdaab370e5f1a0712c672ceee9c7e937817896`。sourceDigest `3e579f87ace9ff533bdd9af4bccdd46c4d1600ce093540b7367ad5e11dda7c47` 与fc001相同，原Rust176与8新增保持fc001身份而不重跑/重标。assembly启动引号错误在owner创建前被拒，修启动参数后首次实际组包成功，无重复release。

| 场景与实际报告 | Go native ms | Rust native ms | Rust parsed/cached | worker完成 / wait µs |
|---|---:|---:|---:|---:|
| [cold](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6239955550) | 8805.4093 | 5765.9033 | 4036/0 | 3983 / 1986310 |
| [warm](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6239960707) | 620.2505 | 538.5460 | 0/4036 | 0 / 0 |
| [single-content-change](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6239997236) | 3030.2354 | 2013.7919 | 9/4027 | 9 / 1658 |
| [root-insert](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6240030438) | 3051.5794 | 2159.3711 | 1/4036 | 1 / 117 |
| [first-delete](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6240082268) | 2991.7317 | 1946.3790 | 8/4027 | 8 / 3358 |
| [cross-prefix-rename](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6240085930) | 3181.9718 | 2133.5056 | 9/4027 | 9 / 3526 |

本轮finalIntegrity=true、NOT_RUN空、12实际产品调用，四变体8owner sealed/reaped/retired。完整读取tuple与归档一致，解决未来变体pack证据缺件；原e475缺件和全部旧失败不补造。[ba0完整索引](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6240361205)与[终态](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6240381566)现已由云端唯一作者独立复核：16 ZIP/CRC/片及wholeSHA、137965 chunks、52599 files全过；12原native receipts/ticks、14owner/52416 sealed引用无缺件，六场景原Facts全部字段/重复/顺序与custody逐字节重放通过。原尾4片403与partial记录保留，随后同URL正常恢复，没有替代route。各为一次诊断，不是20对；内存原采样缺口保留。cold worker_wait含不同等待边界，group drain未单独计时，不能把1.986s全部归因于drain，也不能将嵌套phase累计当native总wall。下一优先依据同run剩余约2s中完整index物化/编码与Facts读取结构，不继续优化已为零的命中worker等待。

[Windows Node](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6239635558)实际306总、304pass/0fail/2skip；仅pack file-symlink和dangling output file-symlink创建EPERM，负例未执行，不称通过。目录junction、Rust/Go replay、duplicate Go key真实mutation与原reject均通过。原fc001 Node299/4fail完整保留。

### 已验证实现：完整index借用canonical序列化

ba0四变体同row仍有FactsRead483–542ms、IndexSerialize465–596ms、publish91–115ms。已实现最窄默认链切片：`index::persistence_wire::encode(&map)`替代完整`to_value`副本与二次编码，只借用现有Index/Value；私有wrapper精确保对象排序、数组/重复、可选省略、locator小写hex与仅summary顶层io剔除。128MiB仍在完整Vec生成后由原publisher判断，原内容hash、manifest、锁/checked-close及取消边界不动；pack2/v14、公共Serialize与Emmy原版不变。原DOM析构在span外，潜在收益不能全归IndexSerialize。warm整index读取不受此片优化。

3源码/测试文件经rustfmt解析与独立静态审阅，31a完整index107包含10个`borrowed_index_*`正式回归全部实测通过：旧新完整bytes、32可选组合/Unicode/重复、locator极值、合法数值/deep/null、private-marker普通对象、非法summary、writer短写/失败、真实cold/warm blob+manifest、publisher大小/取消/损坏、非持久字段。匹配六场景结果见上，不将实现回归闭合冒充20对性能达标。

### 历史实测：e475/00d稳定分组完整六场景

[e475/00d正式终态](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6238394989)，[原始证据索引](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6238369445)：wrapper0/succeeded、finalIntegrityVerified=true、NOT_RUN空；恰7新产品调用，原cold/warm/Go单改5调用原身份复用，不重跑。全部六场景五Facts数组/字段/顺序与共同summary完整运行时一致。10新owner sealed/reaped，9正常retired、工具stage保留；7旧failed与3claims不改。

| 场景 | Go native ms | Rust native ms | Rust parsed/cached |
|---|---:|---:|---:|
| cold | 9658.291 | 5579.8315 | 4036/0 |
| warm | 638.8264 | 434.5645 | 0/4036 |
| single-content-change | 3544.9285 | 4402.9541 | 9/4027 |
| root-insert | 3028.1771 | 4376.3258 | 1/4036 |
| first-delete | 3424.6243 | 4288.7907 | 8/4027 |
| cross-prefix-rename | 3265.7518 | 4428.2166 | 9/4027 |

局部失效9/1/8/9已真实成立；四变体仍为Go的1.242–1.445倍，不算性能达标。各为单对且准备隔开，原Go单改来自前次失败；不是20对验收。原31phase重叠不相加、不从nativewall扣除；controller587307.5352ms不是产品wall。memory gaps保留，不代表完整RSS峰。

14附件已由云端唯一作者独立GET/CRC/片与wholeSHA、34565新+534旧file及10当前owner34463 sealed引用核过；四变体8原native receipts/ticks精确复算。1254个更早历史引用不在本包、cold/warm原raw缺失，明确未冒称复核。12片本轮Brotli93440390B/SHA c8e6db1d2998a1fd0b6b830af29ee5734e5149f3fd7ebd0a2202c71240b3186b；旧2d1两片10308120B/SHA0b2af57b730dbd5eb932f1656ba300406bc4152bfececf3e96b0b29b57a3e5a9。

证据真实限制：runner成功cleanup前没有归档新Rust Facts pack body，全部成功变体副本已清理；附件含完整GoFacts、双方index分片与实际比较结果，但不能独立完整重放新变体readFactDelta。禁止补造原件或重跑替历史。下一薄修仅把已认证index实际引用的pack/合法loose分片归档并核验后再cleanup；性能下一步依据同run缓存读取/整index构建序列化发布关键路径，不继续靠少parse宣传收益。

### 已验证实现：跳过命中缓存的无用pureparse

[完整同row互斥账与源码归因](docs/toolkit/source-pack2-incremental-attribution-2026-10-17.md)：四变体2.060–2.087s主线程等待近3984个最终丢弃的worker结果；Facts读解码577–605ms、整index序列化532–623ms/发布90–111ms。已实现metadata-only cache-aware调度，不派已认证命中pureparse，不排队/clone命中Facts；cold同组并行miss、组边界drain，后探测错误按绝对doc序号延迟返回，保原首错/控制/预算/EOF。pack2/v14不改。4生产/测试文件rustfmt解析与独审过，[fc001 Windows实测](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6239109511) alltargets通过、完整index97/facts48/lua_tree31均0fail/0ignore，8新`cache_aware_`逐名各一次包含于index97；Go104完整严格等价，11边界保持5等价/6已证endLine增强。后继ba0 Node/匹配release/完整六场景已实际完成（见上表），冷链重叠取舍与重复性能验收仍需测，不能只计worker减少。

同批正式runner归档薄修已完成：按已认证index实际引用去重归档Rust pack/合法loose和fresh Go Facts，8MiB分片/catalog绑定实际候选/index/manifest/partSHA。reader实际读取的bytes/SHA tuple逐项对归档，防“比较旧字节、归档新字节”；准入后增大/缩小/替换/删均在有界复制中失败并保owner。完整归档核过后才cleanup，历史e475缺件不补造。fresh使用显式配置精确commit/EXE/artifact/source与原完整verifyArtifact/sourceDigest/recipe/Emmy/cleanHEAD认证，禁止latest；历史三续验仍固定00d。用户批准的本任务6GiB可fresh显式`--scratch-6gib`，默认5GiB/其它预算不变。云端原六组Node303/303与25文件独立导出/import、独审通过；[fc001 Windows真实Node结果](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6239148684)为299pass/4fail/0skip。三项在symlink创建阶段EPERM；第四重复Go key变更注入使用POSIX斜杠匹配，Windows路径未命中，导致预期拒绝未发生。原失败日志保留，不能重标skip。本批仅修测试路径与平台输入准备：重复key改精确fixture目标，保原拒绝和确实变更断言；replay与file-symlink拆分，只有Windows创建阶段EPERM/EACCES可单项环境skip，目录junction另测。生产reader不变；云端修后六组Node306/306、0fail/0skip，Windows真实304pass/0fail/2项file-symlink环境skip，不能冒充该负例通过；匹配release/assembly及六场景已实际完成，详见最新实测。

新增Rust全名（均`index::facts_cache::tests::`）：
- cache_aware_default_consumer_avoids_all_workers_on_complete_pack_hits
- cache_aware_default_consumer_dispatches_only_missing_snapshot_groups
- cache_aware_legacy_presence_rechecks_same_key_replacement_and_growth
- cache_aware_later_probe_error_is_bound_to_its_own_document
- cache_aware_pipeline_preserves_earlier_consumer_failure_over_later_probe_error
- cache_aware_cancelled_pipeline_joins_and_recovers_without_losing_committed_groups
- cache_aware_non_worker_rows_and_corrupt_record_keep_original_order
- cache_aware_current_retention_and_encoding_prelude_precedes_deferred_cache_error

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

- **Data 默认链已接入**：本地/CDN、Prepared/cache、SQL、导航、asset、Hotfix、自适应内存与checked收尾已有实现。统一单堆SQL保持默认栈、原深度、逐行provider与真实预算lease；正式relational299及原6执行崩溃回归通过。local_query76/0/1、全Data库1063/0/1已有证据，原集成预算失败经7db27/0/0闭合。Data配对已执行并保留22组性能未达结果；剩代表性SQL/读写/解码性能、独立实际使用与最终审计。
- **Emmy 已使用原版848文件，语义验证未完成**：`3336d82`已保存 initializer/provider 重复查询、call quality 多余查询和完整键集合被未知值污染的修复。原固定参考与当前候选的全量类型/provider/coverage差分保留；ac8实际Adapter140项、context56项已通过；完整历史严格差分参考36通过、候选35通过1失败，4137行中仅object成功投影的两条正向call_result证据有差异。独立当前合同检查已在原ac8真实sealed产物通过（4135相同行+2精确当前receipt），原始严格失败、全部类型/provider/coverage原文和Cargo退出结果保留；不能将新增检查冒充原历史比较通过。
- **HTTP 后继已保存并取得定向实证**：`6051063`使用固定 native-tls/schannel 三源38加4删的最小适配，将SNI名与证书校验名分离，保原验证与tokio所有权。完整来源/许可/逆补丁和打包已接，Node20/20通过；固定fa封存Data EXE实际remote329项与私有根TLS五项Go1.27.1对照已全部通过，多尾点SNI一致且证书拒绝保留。后续ac8正式Cargo remote329及固定Go五loopback也已通过；不能外推系统根/撤销全部策略。Emmy零补丁不等于整个依赖树零补丁。
- **Source 正确性与性能仍未验收**：5bbb完整配对中暖query20对等价且Rust/Go比0.202388，只有该组达到≤0.30；describe两组约1.13、cold query约5.00未达标，cold index因完整公共事实不同而失败。Metadata前置与SQL预算缺口已实际闭合。当前完整修复统一Go公共Facts字段/顺序、两个不同validate入口及完整类别gap缓存；native XML/Emmy语义增强保留在独立真实消费者，不用summary扣数掩盖差异。e843已通过Rust编译及4036文档逐字段/顺序0差；7f畸形输入52项已全字段差分通过，冷性能仍不达标。旧52与新增32 XML边界已完整差分通过；不将有限输入集冒称所有非法文本已穷尽。完整Retail refs/context/batch维持原90秒，旧dev仍deadline失败；优化408普通native refs/context/API已实际通过；2d7 runner/a338产品完整full14项已通过，包含身份变更/恢复和同owner；e843真实600秒lifetime已退出0通过。完整B01–B07、内存与五阶段审计均未验收。
- **正式 Lua 已明确切到 Go323**：`4092344`保存113原blob认证、精确Rust3.5 overlay与220/9正式入口，旧320/321/322原清单保留。Node正式63及包/版本115通过；本机已安装官方固定MSVC14.51与SDK，Community三工具字节/版本/签名均匹配原Enterprise固定身份；Community精确路径已实际验证。固定7c07的Go323正式Lua220/0/0与Rust桥9/0/0现已真实通过，全部owner回收；只是该自动化范围，不含实机或性能。

### 当前全仓架构实施批

e734已保存四个真实消费者边界，详细设计见 [DESIGN](DESIGN.md#whole-repository-rust-native-implementation-plan)：Core 借用型 pin 请求已接默认 Data provider；Data typed stream 已接 CLI、逐行执行和统一 wire renderer；Live 三态结果已迁移 connection/activation/doctor/CLI，清理合并消费原结果而不 clone；Source 集中原 Lua51 配置、八类诊断顺序和准入策略。Live 新增12项、Data/CLI新增13项、Core/Data新增10项、Source配置新增3项回归已接正式组；[7a实际13组](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6224181570)622通过1个Core阶段fixture失败0忽略，all-target通过，包含Live348、CLI69、adapter57及Source新26项；不重复累计子进程与52行输入。Node合同/身份45项实际通过。后续跨模块静态消费者曾60通过1失败，原因是旧322整体文件身份未纳入三态迁移；精确52编辑可逆认证补齐后62/62通过，原失败保留，旧manifest/Go输入字节未动。保留原预算、错误次序、输出和已提交事实；本批不代表最终五阶段审计已完成。

Source 冷链同时实施有界 scoped 多线程纯解析，按序提交并共同占用原 retained 额度；不把所有模块强行并发。Core/Data 现有所有权与单堆 SQL 保留，Live 输入状态仍串行。成熟库按实际能力、成本、许可与升级验证选用，不为架构名义另造空 crate 或通用调度框架。

## 3. 当前实际验证与证据

### 当前 Live 离线量级入口与0890实测

新增独立 opt-in ignored 库测试与[正式 runner](tests/rust-performance-paired/live.README.md)，复用固定Go323 Lua peer/113文件认证与真实Driver、codec、durable journal、owned projection；原9桥入口不改。结果字符串256/16384/131072字节×原journal 1/255/256事件×3独立fixture共27样本，实际BODY长度另记。真实Instant覆盖setup、checkpoint、Driver、校验和checked收尾；互斥段+余量加回总wall，Lua交换与重复record解码嵌套在backend，不能再叠加或称游戏延迟。复制字节/容量有实际代码证据，allocator总数/RSS仍null。没有可比Go host时标，不能报Go速度比或B06验收。

独立静态审已纠正disconnect与service.closed不同层级、原close失败被空二次close遮盖、wrapper墙钟误命名三点，复核无剩余确认缺陷。无消费者的CLI artifact门禁已移除，只构建本次优化库测试EXE并认证其真实source/receipt/runtime，不重建整CLI。新Node32+原桥controller24=56/56已实际通过；[0890本机前置](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6234472215)Node56/MD5Node71及all-targets均通过、3owner回收。[真实Live27/27](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6234522594)及27peer寿命收尾全部通过，优化库测试build/run均回收。先校旧7c Lua report被runtime_support_identity拒绝，改用已有ab4认证report后通过，未改清单或重建解释器；保[原拒绝记录](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6234455923)。

[原证据索引](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6234550692)及7块已逐SHA核742引用/196唯一blob，原report每样本互斥账、27sample/peer、45journal文件5868event链与27BODY完整匹配。27sample墙钟合34.921s：实际fixture历史构造17.437s、peer启动6.380s、身份重验2.503s均属诊断完整成本，不能称产品热路径。Driver合7.551s中journal5.640s、backend1.884s、余27.875ms；journal里1215次append合5.346908s（每sample45次，占Driver70.81%）、54checkpoint132.072ms、27write_result160.611ms。nestedLua408.987ms/codec2.552ms已含backend，不再相加。真实BODY为362/16490/131178B；history255保172200B/255event，history256先checkpoint为682B/1event，二者不是单纯多一事件的增长对照。下一结构候选针对append重复链解析而非2.552ms codec；性能收益与Go host/游戏/20对验收仍未证明。

### Live journal认证链复用1360实测

按0890真实append大头实施lease内可选认证raw链缓存。每次append仍原owner检查、完整真实read与checked close、逐字节比较；只有字节完全相同复用count/head，变化原parser/hash/head conflict；成功write+sync+checked close+control后才推进，Unknown/失败仍poison并驱逐。load/history先驱逐，checkpoint全验重建，close释放。prepare_event内部一次返回bytes+已有checksum，保公共bytes API及恰2MiB JSON+LF旧可写/下次读拒绝边缘，不借cache吞错误。

缓存资格用现8MiB checkpoint阈值；保留的observable capacity≤8MiB，cache活跃时新read请求≤8MiB。长度变/增长分配前驱逐，返回capacity超资格立即驱逐；合格post-allocation pair观察≤16MiB，不能称任意allocator瞬时/总heap/RSS硬上限。较大合法log走原64MiB完整路径，无新用户输入拒绝或通用budget。新增7项覆盖真实同长度改动、owner优先、等价JSON重排、checkpoint/close、增长/容量fallback、LF边缘与prepared bytes/checksum；原cancel/Unknown断言亦增强。

新10个cfg(test) append互斥段在Driver接手后才启用，保原总wall及内部/外部余量，actual hits/replay/read/equality input/capacity另记；旧原27report已由新reader真实接受，新profile只允许全27或全无。Node42+原桥24共66/66由作者与整合者实际通过，Rustfmt解析/diff及独立静态审无剩余确认缺陷。[1360本机前置](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6234930599)all-targets通过、Live完整608/0/1（诊断ignored单列），新7逐名一次过，原journal35声明入口通过、helper不重复。[正式离线27/27](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6234976464)与[终态](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6234995631)27peer/5owner均回收，10append子段完整。8段698引用/165唯一blob共12,843,610B逐SHA核；原sample/peer、45journal共5868event链、27输入/BODY/history及全部45journal原bytes均与0890一致。

同run Driver合7.1078032s、journal5.238447s；append4.955697s中sync2.688615s(54.25%)、snapshot/artifacts0.850453s(17.16%)、完整read+verify0.159487s(3.22%)，子段及两层余量逐row精确加回。每sample45append真实为6次输入×7持久状态加3个operation/report/result事件，没有可证安全合并sync的结论；61次artifact访问中2首发、59已有全读，大BODY重复读取3,020,628B而首发131,271B。相较0890 Driver7.5512702s/journal5.640s仅跨run改善观察，非因果或20对验收；fixture历史构造变化不冒算产品收益。缓存仍O(log bytes)完整读取比较，后续优先有证据的snapshot/artifact重复构造，不以删durability追速度。

### Live短借用artifact 592实测

1360原27样本snapshot构造有1647个非空临时artifact Vec、30,736,260 payload复制字节。最窄后继改私有BorrowedSnapshot/Cow：合法code与所有raw result短借State；非法code保原逐坏字节替换并接管规范化buffer，raw binary result不规范化。public owned Snapshot继续兼容包装，digest首个owner/去重/BTreeMap顺序、metadata/INPUT投影、原owner与全部已有artifact完整read/compare/checked close/首发durability不变，不建立跨调用artifact缓存或删sync。

新5项与现独立旧全JSON-copy oracle覆盖owned/borrowed完整状态与artifact等价、非法UTF8/原二进制report/空值/重复key与512KiB边界、调用后State可变、同长度外改/poison等；cfg(test)只统计成功最终append map的borrow/owned字节，非全allocator且不含setup/checkpoint/被丢弃重复规范化buffer。Node52+原桥24=76/76实际通过，两份真实0890/1360旧report仍可完整解析；独立静态审无确认缺陷。[592正式前置](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6235302167)all-targets与Live完整613/0/1通过、新5逐名一次通过，Node76通过；[原27样本](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6235349943)和[终态](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6235359631)全部通过，27peer与5owner回收。build及普通lib两receipt保认证辅助进程forced/knownAuxiliaryCleanup，不能说无forced。8段700引用/167唯一blob共12,960,578B逐SHA核，45journal原字节与1360/0890相同，原输入/BODY/history、缓存计数及全部互斥计时逐row核对。

1215成功append snapshot的1647个最终map条目、30,736,260B均短借用；本矩阵合法code故normalized-owned与post-normalization-copy为0。确实消除原私有map的payload Vec复制，不代表全链零复制、allocator总数或RSS下降。同run sample总31.0522414s（含fixture/peer等）、Driver7.0607810s、journal5.2046063s；append4.9289097s中sync2.6914004s(54.604%)，snapshot/artifacts0.8339802s。较1360跨run Driver少47.0222ms、snapshot父桶少16.4726ms，但sample总反增167.0242ms；9cell Driver中位数6降3升且范围全部重叠，不能宣称因果提速或Go/20对达标。父桶仍含完整认证/metadata/哈希/已有artifact全读/首发，不能把约31ms/case全称可省；此窄候选正确性与减少重复所有权工作闭合，不由剩余混合桶无依据继续删sync或新增缓存框架。

### Data 哈希生命周期筛选0890实测

保原example的1/16/64MiB整片RustCrypto/CNG比较和历史64MiB负结果；另加认证44B Root与17MiB payload、64KiB增量及fused SHA256/MD5。payload真实272次update，Root独立1次，合计273不拼成一个消息。三个现RustCrypto模式分别测clone-finalize、consuming finalize、fused完整new/update/clone/finalize/drop；六次正反序迭代全部保留，完整hash和向量先验，实际CPU仅原root进程总体。CNG现安全入口只有整片生命周期，明确不伪造增量等价，不加新依赖/FFI。仅构建原Windows example；没有整CLI构建或生产哈希替换。详见[专用说明](tests/rust-performance-paired/md5-benchmark.README.md)。Node71/71已由作者与整合者实际通过，Rustfmt仅解析；独立审发现的Root错误分支checked close与准备期固定fixture前置边界已修。[0890真实报告](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6234539195)与[终态](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6234563055)原3行+新36/36全部通过、0skip、7owner回收；RFC7、8长度×4分片×3模式及取消/期限/完整摘要检查均过。payload完整生命周期中位数clone21.318650ms（20.9972–24.4811）、consume21.598150ms（21.2522–22.2369）、fused SHA256/MD5 consume28.431950ms（28.2561–28.8888）；44B Root分别0.0007/0.0006/0.00095ms。CNG整片均值1MiB1.332ms对Rust1.275、16MiB23.647对20.218、64MiB86.969对81.711，没有更快且非相同增量API；不换生产backend。当前主要update成本，不把clone改consume当已证收益，不将21ms子段视为可全部省去；微基准仍不是完整CLI性能验收。

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

正式facts reader新增显式`v11-pack1`及本run真实index blob路径/SHA，禁止扫描目录猜旧pack；历史v6–v11读者与旧报告保留。无全局locator/GC/新Windows接口，祖先身份验证未跳过。完整独立静态审阅发现的一处Document测试字段遗漏已修，未剩确认生产缺陷；[b3d实际前置](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6233354114)all-targets通过，Corestream10/0/0、Sourcepack11/0/0、retainedadmission1/0/0、旧cache26/0/0、Node64/0/0；22个新命名入口逐名核，其中1个惰性子进程helper不重复算独立断言。旧104严格全同，boundary11仍5相同/6精确endLine增强，10owner回收。[匹配release/assembly](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6233470665)已实际成功并回收，EXE SHA `bc8f67991a98d607ec7a0ca08293e8c34abe4ef2d1122c8b344ddd01a29c3e64`。[本机随后确认](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6233513933)同run冷index Go9199.0487ms/Rust5873.878ms，完整4036字段/顺序0差；单次比约0.6385，未达≤0.30且不是20对验收。原暖Rust509.5789ms已执行，临时编排错误套用cold的git_tree校验而中断；另一临时取证把82,228,560B原index直接交16MiB单文件seal触disk.log_budget。两原失败与旧scratch active receipt保持，不重标产品失败或清理成功。74d正式流式分片与cold/warm/changed reader经23文件认证/Node15通过，[新custody与原观察复验](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6235467351)按64KiB读取、8MiB十片完整SHA核，0产品重跑，warm原index与原Go cold4036完整字段/顺序相同；旧launch/outer wrapper未独立重认证，原wrapper失败不改。

[74d runner/b3d产品续验终态](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6235614924)新增7次CLI全部exit0/reaped、4个新owner succeeded/sealed/回收；preserve/diagnostic-inputs仍保留，不称目录已清理。新增Go warm615.6809ms与保留Rust509.5789ms不是无中断相邻pair；single-change Go2978.7351/Rust4029.9592ms（Rust cached3972/parsed64），insertion Go3133.0631/Rust5012.9957ms（0/4037），deletion Go2888.4973/Rust4961.4351ms（0/4035）。全部完整Facts字段/数组顺序相同，原call与新call身份分开，7内存采样均有gap。变更严格沿原计划：首文档BaseAPIMixin.lua追加LF、根插入000_LycheePackInserted.lua、删除原首文档；未挑有利组或补样。冷单次改善不能掩盖3种增量均慢、插删后缀全重算的实测代价；仍无20对性能验收。所有新大index用十片及整件SHA复验，原home/receipt字节不变。

下一窄结构候选把全局序号组改为稳定path二进制radix叶（最多512个eligible文档），叶内仍最多64记录/32MiB pack；不新增全局索引/GC/另一数据库。固定公开4036path静态筛选为74pack（原64），代价增加10次冷发布；64/128/256叶分别155/103/87pack。三个上述真实路径静态推导分别仅9/1/8重算，尚不是运行实证或一般上界；新算法与默认消费者已接：现64索引/16KiB group charge不增，两个scalar扫描求叶边界，whole-index改v14、group域facts-pack-v2-path-radix512；pack字节/MAGIC/locator/v11envelope不变。显式v11-pack2 reader必须绑定candidate与indexSHA，裸blob不自动证明分组算法；旧pack1读者/证据保留。

健康可用旧缓存下，单次membership变动导致的候选重算上界insert513/delete512/rename保守1025；单content仍≤64。不含首次格式迁移、独立损坏/驱逐/发布失败/eligibility变化，不能冒称所有cache miss上界。公开路径另110个跨位置rename静态范围2–511，未当运行耗时。独立复核发现叶恰64倍数且夹asset/超限文档导致空group的真实问题已修，新增1024真实文档加asset/32MiB+1输入回归；最终无剩余确认缺陷。Node组合66/66（reader29/candidates32/generated5）实际通过，另reader+resume+custody44/44为重叠验证不累加；Rustfmt仅解析；随后[00d实际前置](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6236073579)all-targets通过，pack18含7新逐名、旧cache26、retained admission1、Corestream10含1 helper、Node66全部通过；旧104严格相同、11边界保5全等/6endLine增强，8owner回收。1024+asset/超32MiB尾实际通过，原pack Cargo保认证辅助forced收尾。

[00d匹配release/assembly](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6236213281)及[终态](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6236225116)成功，EXE44717568B/SHA3169c2187b966b40ed3622fe14cfb81527e8dd3a52a6d99a888e8ab482c1b1fb、artifact SHA02ebe1eb57d552d1c8ebf6c86697fdbc77d6eba4ee847edd33c4d7d19b6253ad、sourceTree06a5a0e39c46c786d946b8c429c90c0eb4e9e1291028375e329fc9457776b4e3。前置与构建合11owner回收，六场景仍NOT_RUN，不能把新算法静态收益写成实测。

后继本机固定新SHA：all-targets→Source `index::facts_cache::tests`完整18、`index::facts_cache_tests`完整旧组、`facts_pipeline::retained_cohort_metadata_reduces_real_worker_admission_without_changing_facts`与Core `cache::source_facts::streaming_tests`，及旧104/新11正式Go边界→前置全过后唯一匹配优化CLI/assembly→显式v11-pack2全4036字段顺序、冷/暖与同一首文档LF/根插入/首删除，再覆盖不同位置rename。保原b3d原始结果、全部计时/预算，不能用旧EXE冒新分组。7新增完整名称均前缀`index::facts_cache::tests::`：
- `path_leaf_scalar_plan_matches_full_binary_trie_with_prefixes_utf8_and_skips`
- `path_leaf_insert_delete_rename_locality_matches_independent_complete_trie`
- `stable_path_leaves_default_consumer_preserves_full_facts_across_snapshot_edits`
- `stable_path_leaf_split_merge_corruption_and_concurrent_recovery_use_real_cache`
- `exact_pack_leaf_boundary_skips_real_asset_and_oversized_document_before_next_leaf`
- `path_leaf_planning_keeps_original_cancellation_deadline_and_rejects_unsorted_inputs`
- `legacy_ordinal_pack_and_v13_index_keep_identity_while_current_default_rebuilds`

[7d8/00d首次正式运行](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6236649962)：455584347B冷准入成功，两真实CLI均exit0、valid/reaped、无forced/deadline。Go9658.291ms、Rust5579.8315ms；Rust parsed4036/cached0，stage/seal/rename/close各74。完整Facts/共同summary尚未完成，不能报等价或性能通过。runner的captureMetadata仅因Go主DB12288B旁有0B WAL与32768B SHM而拒绝；这是读取器过严，不证明DB损坏。原outer2f0bcd33-f0f4-46b6-9bc5-5539dbc5b422与healthy058035ab-29e8-43c7-bbeb-ee54472f8147均failed/sealed/reaped、scratch保留，未删除sidecar。warm与4变体NOT_RUN；后继应认证并复用这两cold调用，不重新init/sync/cold。Node首次缺测试依赖114/2与补齐后116/116分别保留，[前置报告](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6236603320)。

[be168原cold完整补验](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6237188747)已实际通过：4036全部五数组/字段/顺序与共同summary一致，原Go9658.291ms/Rust5579.8315ms保留，freshColdCalls=0。新custody786d460f-cfd9-4a21-9322-6b79d5e5a08d succeeded/sealed/retired，8191日志/分片保留。随后[真实same-home warm](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6237209753)两侧warmReuse成立、全Facts/summary一致，Go638.8264ms/Rust434.5645ms，Rustcached4036/parsed0。Go内存gaps/Rust采样成功不当完整峰值。第一单改Goowner创建前used5005736596B+reserve380939478B超过原5GiB，缺17966954B；4变体0调用/NOT_RUN，旧rawrunning行不表示实际运行。新outerd1989b3b-573a-423e-9185-86b89fc6d2f4与continuationbff2649b-3f34-4e15-b14e-48855cff42fe failed/sealed/reaped，原claim保留不删除。

[2d1首变体实际结果](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6237641406)：178/import/inspect通过，局部6GiB准入已生效；Go单改run32ebcee6-42ee-46bf-ada0-5362b2cd457e真实3544.9285ms、exit0/valid/reaped、无forced/deadline，memory有gaps。随后runner `actual scenario footprint exceeded admitted estimate`失败：home464365379B超过reserve380939478B约83425901B，漏Go额外snapshot offsets.json93138293B和manifest552B；不是6GiBcap拒绝。尚未完整Facts custody/pair，Rust单改及后3场景共7调用NOT_RUN。outer620316bf-04c6-4e21-9c47-f25c901fbcd5、base-reference dd94aed1-1137-4bb5-8d4c-659bf367468c、Govariant6a2dc3a8-f133-4330-ac54-542ff403a1a4均failed/sealed/reaped，原cache/remainingclaim保留。后继需完整核两引擎所有派生格式增长，复用这一次Go结果，只续剩7；不得重播或把已执行Go标NOT_RUN。 后继四工具已实现完整whole-owner预算模型：Go整代records/offsets/manifest/newFacts，Rust index/整代pack/capture/assets，另计SQLite页/有限事务/WAL/SHM、Gitobjects/双临时index/project和证据free-space。README列逐项来源与限制；Go插入546495450B、删/rename510843864B，Rust保守上界522584261B，按已知used下界最坏距6GiB约425853519B，现场新增仍须真实准入。正式`--inspect-go NEW-reference`/`--resume-go reference --scratch-6gib`认证固定2d1三owner与已完成Go原调用，先只读custody后续剩7，旧claims/receipts不改、零cold/warm/init/sync/Go单改重跑。六组Node204/204与25文件隔离导出/import通过，Windows续7仍NOT_RUN。

用户已明确批准仅本任务测试scratch上限5→6GiB（Sentinel_a57db18d476081919e647f3195a645a1），一般默认5GiB与cache20GiB/free2GiB、全部产品预算不变。后继须认证上述已完成cold/warm/claim/健康inventory，只续4个未执行变体，不重跑cold/warm或改旧failed记录。 后继已实现README两步`--inspect-remaining NEW-reference`及`--resume-remaining reference --scratch-6gib`：固定认证be168完整sealed cold/warm/旧claim/健康base，只接现有四变体循环；新claim在首个变体owner实际准入后创建，纯准入拒绝不会耗掉执行机会。零cold/warm/init/sync，无base副本；每个beginScratch和后验准入显式同6GiB局部limits，其它入口不接受该override。Node六组178/178、25文件隔离导出/import通过；Windows剩余4变体仍NOT_RUN。

空WAL/原cold续验后继只改测试runner：允许稳定空WAL/有界SHM，原主DB/sidecars前后身份与内容认证，只在新owner打开主DB复制件；非空WAL与journal仍拒。README新增`--inspect-cold`生成四摘要引用、`--resume-cold`固定认证上述两owner/两run，先新只读custody封存完整cold Facts，再新owner登记两home精确借用执行same-home warm与四变体。旧failed receipts/logs不改、旧scratch不cleanup、freshColdCalls=0；单次claim阻重复执行，原memory gaps/时钟保留。原5GiB/2GiB策略不变，实测大小准入可真实阻塞。该后继6组Node151/151、25文件隔离导出/import通过，Windows续验NOT_RUN。

正式六场景runner经独立复审，6组Node共116/116通过、无skip；新隔离25文件导出与导入、syntax及diff检查通过。Windows六场景仍NOT_RUN。正式六场景入口缺口已由云端补最小runner-only后继，见[配置与完整流程](tests/rust-performance-paired/source-pack-scenarios.README.md)。复用上述00d产物，runner另目录25文件导出/身份认证，不在产品clean checkout覆盖工具，不重编。各engine真实init/sync认证后cold→warm成为健康base；4变体逐engine独立clone同base，精确Git tree/blob/输入与公共project pin先认证，再保原180s产品/210s调用/原native完整墙钟和31phase。Go完整已认证records/manifest/逐fileFacts作为必要比较证据流式8MiB分片封存后，才正常回收自己的成功scratch；Rust用独立baseclone与该完整证据逐字段/顺序比较，不把整个home移到evidence。原b3d/历史失败不导入、不替样。

[原配额阻塞](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6236089985)及[只读54owner清单](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6236196237)：现scratch4,612,057,341B/5GiB，余756,651,779B；原1GiB申请缺317,090,045B，非物理磁盘不足。没有已证明可安全释放的足够旧scratch，旧active/preserve/diagnostic-inputs未动。新runner以明确历史cold估计、当前健康base实际inventory及完整新index/Facts增长逐engine准入；环境quota override在首owner前拒绝，20GiB cache/5GiB scratch/2GiB free reserve保持。实际postusage检查不是事前hardcap保证，仍可能真实block；失败保原停止点与证据、不盲重跑已完成cold/warm或扩quota。

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

## 4. 第一阶段正式审计进行中

2026-10-17 21:40 UTC正式启动五循环Stage 1第一轮，固定生产`b4d8b22a666648ee0fe18a3f2656a84c586e5bd3`与状态文档`7753776964af6b84a0130e1afaf46bb65497dd8b`。先前三个专项review不追认为本阶段完成。三个独立只读组分别审架构/功能bugs（Core、CLI、Live、Windows、Delivery）、Source/Data结果质量、Skill实际工作流；协调者唯一整合写入，必要测试仍由父线程唯一队列执行。发现须绑定真实合同、消费者、文件位置和可验证输入，修复后集中测试并独立复审；一轮零新增可操作finding才收敛该轮，不承诺数学零bug。

- 逐当前公开命令与F01–F13/P01–P14映射实际路由、参数、完整结果、错误/取消/已提交事实和恢复。旧89/58命令盘点是历史，不充当前分母。
- Source核query/inspect/refs/context/flow/batch/session、来源坐标/不确定性/缓存；Data核SQL/DB2/Hotfix/资产/capture/adaptive真实消费者。parentArray两条是已知未建模增强能力，现诚实partial合同允许；先核实际可达与承诺，不预设Go回归或新增功能门禁。
- Skill两个Go323 Data实际任务仍缺完整独立使用证据：真实DataPin下IN/CSV/capture，真实压力释放后的同pin有界恢复。现Source runner不能替代，属于可自动离线项，不能挪到游戏人工末尾。
- 最终候选安装/升级/卸载/恢复和两fresh offline对应源码重建仍需映射准确证据。SQL默认栈、retained14/0、project选择、正式Lua及模块已测行为保原身份，不重列待首次实现。
- Source、Data、Live端到端/内存性能未达标仍独立open；可并行架构功能审查，不能以所有性能先≤0.30无限延后第一阶段。FactsRead性能作者仅只读提出候选，不与本轮审计写入冲突。

Source最新状态见顶部b4d，Data/Live/retained/Lua保各自已封存身份。后继只汇总源码变化、明确未覆盖消费者及审计finding形成固定SHA正式批，不重复旧统一批；旧EXE不归新树，游戏H01–H09保持文末NOT_RUN。

## 5. 验收与循环审计

完整[F01–F13功能、P01–P14协议、B01–B07性能和Go来源合同](docs/toolkit/rust-migration-acceptance.md)保留；[321增量](docs/toolkit/rust-go-3.2.1-increment.md)、[322增量](docs/toolkit/rust-go-3.2.2-increment.md)与[323增量](docs/toolkit/rust-go-3.2.3-increment.md)是累积要求。代码已实现、静态已审、某树历史通过、当前实际通过必须分开。

- 固定Retail数据为Gethe/wow-ui-source `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b`：4036 Lua/XML/TOC，32,269,051字节。每次核身份后全仓分析/代表查询/源码定位/refs/context/flow/cold-warm/session/释放，不以小fixture代替。
- Source、Skill、Data分别由独立使用者执行真实CLI任务，记录输入、完整输出、正确性/可读性/来源/错误和复测。实现者不自评通过，无法运行时记blocked。
- 全量实现后的五阶段**顺序循环**：①架构/全业务/Skill/全部代码；②有效覆盖与无独立价值冗余；③真实消费者确认后的死代码；④前三维度联合全仓；⑤全项目Rust原生度。第五项是用户2026-10-16新增要求，核crate职责与依赖、所有权/借用、typed状态、clone与分配、流式、CPU/在途任务字节与背压、错误取消/RAII及unsafe边界，要求具体结构与实测证据，不以语法风格或无收益重写替代。历史四轮要求/报告保留原身份。每阶段独审→修复→实际验证→独立复审至无新可行动finding，再进下一阶段。每批源码审不等于最终收敛。
- Go323测试映射为当前差分oracle，Go322映射仍保留，320/321原证据保留；尤其未知结果不重放、事务、取消、空map、槽迁移与错误恢复。记录固定树、覆盖分母、平台排除与停止条件，不保证数学零bug。
- 性能必须同机同数据/结果与校验等价，独立cold/warm逐路径重复；可控耗时Rust≤Go30%，外部等待另列。当前仍未达成验收，不能把缓存预热或删校验当提速。

## 6. 本机接力：当前执行与下一批

本机测试通道已正常恢复，原unknown历史保留，无需再次apply或重启。仅云端实现，用户指定本机线程负责固定SHA测试并将报告写回同一PR。普通push不自动切换其正在测试的树；不另排重复长CI。

当前Source按顶部b4d实证与Stage 1审计finding安排后继；Data/Live/retained/正式Lua保各自已封存身份。仅实际源码变化、明确未覆盖消费者及审计修复进入固定SHA集中验证，由唯一父协调队列安排alltargets、受影响正式组和必要匹配产物。旧EXE不归新树，旧通过不重标，游戏项仍在文末。

- 所有Rust检查沿仓库受控 `tools/rust-cargo.mjs`、固定工具链和原owner/cache合同，先all-target编译，再受影响组；默认线程栈不加大。完整SQL299及7db预算组已过，不机械重跑历史失败树。
- 正式Lua与9桥使用[现有入口](tools/rust-formal-lua-bridge.mjs)，每次新输出目录；原runtime与CLI封存产物必须通过全部身份验证，不能覆盖旧失败目录。当前正式Go323220/9已有实证。
- 性能沿[专用配对入口](tests/rust-performance-paired/README.md)，独立优化构建、原20cold/20warm及完整输出等价。单次profile只诊断；索引/query180秒，refs/context/batch90秒，整棵retained分析树2GiB不变。不可扣除未测成本来宣称达标。
- 完整Retail/native沿[已封存候选入口](tests/rust-source-native-acceptance/README.md#current-workflow-and-separately-sealed-runner)，runner与产品各自认证，记录所有依赖未执行、退出码、精确子进程回收和内存采样缺口。最终五阶段循环目前在Stage1，尚未收敛。

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
