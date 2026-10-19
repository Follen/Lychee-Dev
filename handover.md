# Rust 迁移交接：唯一当前事实来源

## 1. 当前目标和执行顺序

当前执行已批准的 [完整架构计划](DESIGN.md#authorized-architecture-implementation-plan--2026-10-19)，不是开始新一轮正式审计。顺序是：**Source/Data 业务质量不低于固定 f811 → 全部实现门禁 → 同任务 Rust 可控耗时 ≤ Go 的 30% → 五项循环审计**。必要开发独审、差分和回归不是最终五循环完成证据。

- 质量基线 `f811fc7f75fc206158cecb01dd0eb5d39e88b35f`：实际 check 与71项针对性测试通过，16新增逐名一次；不是全仓通过。准确性、完整性、排序/可读性、来源和已提交证据不能退步，已认可的 Rust 语义优势保留。
- Go 业务/性能参考固定 v3.2.3 peeled commit `3766f9cacb52e93a803a626187e2a1d54a445468`。既有增强单独有任务级 oracle，不机械回退正确结果以求原始字节全等。
- 产品仍为3.5.0 beta配置；无发布、合并或游戏操作授权。Skill 七产品定义和双 Forever 分离、Mailbox v1、未知结果不重放、精确数字/partial/证据合同不变。Emmy使用固定未修改上游；自有业务Rust不新增unsafe，必要Windows边界独立审查。
- 当前仓库只在 `rust-refactor` / [草稿PR #14](https://github.com/Follen/Lychee-Dev/pull/14) 分批保存。用户本机只负责固定SHA测试及必要协调，由唯一父队列派发；运行中的身份不会随新head改变。

## 2. 当前实现批次与验证状态

| 范围 | 已保存/已实现 | 当前证据和剩余门禁 |
| --- | --- | --- |
| Core/Data/Source基础 | 8c9：配置、Data派生目录/准入、真实Arrow cache prepare、Source Postcard Facts | 070f实际check通过；原copy/发布receipt路径/generation取消四个修复目标均通过。19e check与67项定向回归通过，070f全部23失败逐项闭合；完整实现门禁仍未闭 |
| Live资源归属 | 5c60：私有 SessionOwner，native先于journal关闭与claim退役 | 8bfe connection61/durable36通过，六新增逐名一次；无新线程/daemon，真实客户端未验 |
| 编译修复 | 6ea恢复原SYM SHA writer；2b400显式Data结果类型；66d空slice断言类型 | 8c9、6ea、2b400、66d原编译失败保留。8bfe实际check通过，config14通过；disk26为24通过2失败（copy注入阶段不确定、Windows路径文本）。其余11组续跑392通过/1失败/1 ignore；新增失败为generation取消测试期待operation.cancelled、实际command.cancelled，后续断言未执行，单独修正和复验 |
| Skill精简 | 308efa：22指南文件、工具与身份/历史fixture，43路径逐字节/ Git blob核验 | 六Node套件101/101通过；25文件/57链接独审通过。两类MODEL_ONLY独立前后任务均通过修正模型：恢复旧/新调用8/11次、身份核查旧/新6/7次，不能推断新版更优；原宽松模型接受非法wait=0的记录保留。四类真实CLI任务未跑；新增逐调用测试入口已源码独审，最终组合三套Node108/108通过；业务非零退出可供Agent纠错、每臂独立home，指南与评分隔离。Windows入口和真实盲评仍未执行。文档缩短或Node通过不是质量通过 |
| Source正式二进制证据 | 740a：已独审10文件工具片，完整Facts exporter与六场景适配 | 七Node套件382/382通过；新Rust exporter/Windows/性能待验证。历史v11读取与新v13显式分开 |
| DataFusion适配资格 | 独立可选资格片已保存并静态独审，固定54.0.0/Arrow58.3.0 | 278默认/feature check实际通过，14数值/计划回归全部通过；默认依赖图不引入DF，真实provider/任务资源适配仍待，生产执行器未切换，不能宣称性能通过 |
| Data磁盘后继 | 共享blob准入、精确decoded spool与CAP独立引用保护已整合28代码路径 | 070f的56新增为55通过、1个uppercase历史CAP删除失败；该失败在19e实际通过，新增两项归属/签名回归亦通过；只覆盖已接入归属，历史blob/CAP/SQLite等全口径和冷缓存淘汰仍未闭，不宣称5GB已全闭合 |
| Source主Index二进制 | 已整合24文件r2与8文件正式Node适配，采用共享typed模型/Postcard，公共JSON不变 | 070f的25 active新增逐名一次通过；Source全lib699通过/21旧fixture失败/6 ignored；19e validate32全过并逐项闭合这21失败，inspect15与flow4原通过，正式luac真实ignored用例另计1通过。七Node357通过/0失败/4权限skip；完整语料导出容量与性能仍待 |

已完成的DBCD四项修复保留：WDC5 palette物理顺序、多词metadata、官方BUILD预设、非SQL实际读取限制传播。c571与9004分批合计113 distinct通过、3原ignore；旧红例及四个admission测试失败保留，不能将小fixture称作真实>256MiB表验证。产品定义未改变，完整产品矩阵仍待。

Source 后继借用 Symbol 投影片已实现并完成源码独审：summary 重建与完整查询复用同一四来源映射，完整 Index 字节、SYM 身份及查询 owned 结果保持；summary 不再保留全仓第二份 owned Symbol 字符串。10 项新增 Rust 回归尚未运行，精确完整组合树的云端九套 Node 382/382 通过；首轮因副本缺少历史 Git oracle 为380通过/2失败，补齐既有只读历史后完成复验。此片不改变 warm 读取；ID/hash工作移入 IndexSerialize，不能把 Symbols 桶下降算作全部收益，需固定产物后比较总 wall。128MiB输出容量与原owned预算保持，单个临时ID与输出buffer同时存活；没有RSS改善实测。

## 3. Home、容量和所有权边界

Data默认容量为**5,000,000,000字节（十进制5GB）**，配置支持明确单位；与测试scratch 5/6GiB不同。最终统一核算Data下载/共享blob归属、物化/索引/目录、staging/spill/导出峰值和证据占用，按内容去重防双计。活跃lease和证据引用保护；旧已超额或配置缩小先拒新增并说明，不删除证据。

CAP删除会识别同body的独立Data保留权：仅删除CAP引用并返回仍共享，不承诺物理释放。判定在原blob lease内认证固定Data记录和body类型/长度，不倒序获取quota或重复全读hash；这是保留权判断，同长度坏内容仍留存，真实消费继续完整认证。损坏记录、孤立完成记录或已登记body缺失/类型长度不符均硬拒，不静默减账。Data引用的冷缓存释放与完整淘汰生命周期仍须后继闭合。

当前基础只覆盖已接入的Data派生写入。共享blob、decoded spool后继未全部运行，历史未登记blob、CAP、SQLite/WAL/SHM等尚未闭合归属。不能把派生对象helper当全Data硬上限。只缓存实际查询过的表，临时查询可不落盘；不默认长期保留raw、IPC、Parquet三份。尚未实现并验收的淘汰策略不称已生效。

Source主Index采用明确v18身份与LYCIDX01帧，query metadata同步版本化。完整文件SHA认证后，共享typed字段校验供完整与摘要消费；摘要不构造全量owned Symbol。128MiB限制新缓存实际文件字节，构造分配另行有界准入，不再为旧JSON内部表示额外全量计数。因新版本及时取消可先于尚未读取的损坏尾返回；已经确认的语义/预算错误保持，不把坏缓存作为健康结果。正式诊断保完整公共Index与Facts，两个exporter输出共用原128MiB证据额度，超过明确失败，不能截断比较。资产比较保f811已认可的大资产前置稳定分区和组内完整顺序/重复。

Source新格式旧cache失效后重建，不维护生产旧格式迁移。新格式完整认证、边界/预算/取消和损坏拒绝仍必须保留；坏新缓存不能通过旧格式fallback隐藏。公共JSON和query/inspect业务输出保留，私有wire变化单独版本化。

## 4. 性能与已知证据范围

最新Source诊断固定86f2bbb：一次匹配release/装包后六场景完整Facts/Index、summary、custody与最终完整性校验全部通过（finalIntegrityVerified=true）。同run Go/Rust毫秒：cold7801.86/3885.1199；warm587.1695/255.1399；change2965.3629/982.6756；insert2910.4461/984.6361；delete2848.9826/997.6467；rename2849.0348/1158.9573。六项比率仍均大于0.30，仅一轮诊断，不是20对正式门禁。21个owner、40个native调用均正常退出并回收；2次forced均已认证辅助进程，无未知辅助或超时，进程/lease归零。当前仅非敏感汇总，原始本机记录不公开。

历史2d23695：匹配release、91文件装包及exporter认证通过。同run Go/Rust毫秒：cold10687.5959/5095.8654；warm667.0608/267.0469；change3958.0999/1344.9300；insert3962.2321/1357.1795；delete3871.5698/1225.4815；rename3704.7494/1240.8674。六场景逐项完整Facts/Index、summary与custody比较通过；原最终runner因owner receipt的1MiB上限拒绝合法约5.2MB封存记录而失败，原finalIntegrityVerified=false保持。666修正验证器后的独立只读复核已exit0，结果为sealed_complete_scenarios_reverified，认证52,494文件；没有新增产品、build或exporter调用，也未改旧记录或六组wall。666 Windows Node505项为498通过、0失败、7个symlink权限skip；两个owner均回收，无forced或timeout。该新复核闭合封存证据完整性，不把原失败改成功。六比率均大于0.30，只一轮诊断；内存采样缺口不证明RSS峰值改善，不能把异负载绝对耗时变化全归优化。

同次冷路径visit为4587.845ms，内含工作与等待；串行解析76次累计1278.543ms，worker等待2202.707ms，23任务放弃。增量Facts读取/认证/解码321.305–363.620ms、Symbols构造114.992–138.115ms；warm完整缓存读/SHA/typed校验188.943ms。各阶段嵌套、worker解析并行累计，不相加或差减成伪独占。FactsRead次数包括组目录、probe、记录与close，不能称重复磁盘读取次数；摘要路径已借用typed字段，不再构造完整owned Symbols。下一窄片已实现并静态独审：Lua递归临时预留使用当前子树跨度，保原共享额度和真实超限回退；Facts编解码移除仅复刻旧JSON私有缓存大小的全遍历，沿现有Wire编码同时核规范decoded ownership，实际binary32MiB和decode256MiB额度保持。完整输出/证据预算仍独立，0bca已实际check通过，Source lib730通过/6忽略、inspect15、flow4及正式luac5.1.5精确用例1通过，合750通过/0失败/6忽略；13个新增或调整名称及1个既有细化各执行一次。86f新性能结果见上；同次冷串行53份累计765.792ms、worker等待1921.308ms、abandoned=0，旧2d为76份/23弃用。现FactsRead增量243.809–265.784ms、Symbols88.793–110.011ms；warm cached172.061ms、Index读取54,464,537B。本轮abandoned=0，历史2d为23；该差异不独立证明因果，跨负载绝对秒数不能全部归因此片；解析并行累计和嵌套phase仍不可相加。下一步按真实准入原因分析剩余串行与聚合表示，不先假定全部大文档或线程数不足。

Data CSV9693工具对旧63d与a860的单pair：cold401.4790/389.9062ms，warm349.0380/324.7937ms；完整CSV/sidecar与8CAP核验。该对比是旧Rust/新Rust，不是Go性能门禁；retainedBytes减少46,988,672B与decodeWork减少81,920是逻辑计量，不是RSS。其余Data、Live正式性能与全产品矩阵仍未验收，不把历史结果归新产物。

历史coverage十二integration实际130通过/1权限ignore，报告是独立对象分母，不能平均成全项目覆盖率。f86 companion真实四CLI产生profile后超原raw128MiB总预算，merge/export/witness未执行；不加预算，覆盖工具扩展暂停，先完成业务与性能门禁。真正OS压力、游戏、当前Skill实际使用仍有缺证。

## 5. 当前阻断与下一批

070f已实际完成：all-targets通过；Rust 986 matched为957通过、23失败、6 ignored；七Node套件361项为357通过、0失败、4文件链接权限skip。13个owner均已回收，无deadline；Source lib和disk的未知辅助进程强制回收标记保留。此为有限固定队列，不是全仓通过。

23个失败分三处：Source validate共享fixture仍用旧summary字段，21项在缺失schema的构造阶段失败，未到业务断言；Data磁盘cleanup测试仍期待9个目录，而新增shared-blobs-v1后实际拥有10个；历史大写SHA的CAP在Data引用保护中先经过仅接受小写的校验，错过canonical归属查找，最终返回vault.blob_integrity。最小修复保持真实typed schema、每个owner恰关闭一次及全部重复错误事件、原CAP签名和完整认证，不放宽生产serde默认或删断言。19e实际all-targets通过；validate32、data_disk13和remove_commands22合计67通过、0失败/忽略，原23失败全部逐项执行通过，两新增也各一次；原070f失败保留。这是分批针对性闭合，不冒称完整新SHA全仓通过。

原2808四处编译错误和一个Node版本断言已在070f闭合；原copy/发布后receipt路径/generation取消四个回归目标均通过。25个Index新增逐项通过；56个CAP新增为55通过1失败。source_inspect15、flow4及显式执行的正式Lua真实用例1均通过。原2808、278和更早失败不改标。

集成流程缺口已确认：原隔离Node工作目录的Rust源码仍是v17，不能以该361/361替代最终组合树验证。后续Node/合同检查须在精确待提交组合树执行并记录sourceTree，明确git archive导出范围与被export-ignore排除的历史文件，不能只合并文件hash。静态审查、rustfmt与patch适用性不等同Rust编译通过。

19e匹配release实际成功；assembly因固定flatbuffers crate未携带许可证正文而失败，该历史记录保留。2d已用官方原文补现有catalog并完成新匹配release/assembly及六场景；原末尾receipt验证失败及666独立只读复核通过分别见上节。采用既有release/licenses目录补入经官方固定版本认证的原文；该资源纳入sourceDigest，因此修复后须新匹配build，原19e成功构建及失败装包记录保留。10ed仅修正式runner显式--scratch-6gib贯穿，默认仍5GiB；作者与独审各12套Node452/452通过；Windows实际447通过/0失败/5个symlink权限skip，均未冒计通过。Data后继45个源码路径已组合独审：认证read-through、可选DataFusion真实TableProvider、私有SQLite元库和缺体精确重建，共94新增Rust测试按实际所属组逐名核验，不能将部分组通过称为全部通过。私有元库沿成熟SQLite正常恢复，计逻辑文件长度，不自研journal/VFS；只读路径不冒写入准入，写入仍严格。缺体修复仅接受已认证原alias/pair对应的确切缺失body，从原来源重提取并完整认证；现存损坏仍拒绝，CAP/keep/alias不删改。精确repair记录保留额外stage峰值，未知/清理失败留事实；旧reader遇未完repair会拒绝而非漏账，成功后只由本次handle撤回。未执行的Data组及原session/asset缺体回归仍待Windows实跑，不把静审称作通过。Data50已保存为ea58347c，50文件完整字节回读通过；Windows五组Core/Windows131项及11套Node121项均通过，真实名单无缺漏或重复。ea两条编译前置因测试中部分移动paths的E0382失败，该历史保留。e486仅修该测试借用后两check通过；10组Data155项实际152通过、3失败、0忽略且逐名无缺漏（45新增为43通过2失败，110既有为109通过1失败）。失败分别为prepared Root缺体后的配置发布归属、首次cold Arrow导入工作预算、可选DF查询physical Wildcard；后续未到达的业务断言不能算通过。对应最小修复保留原Root缺体复用正例，以已认证内存字节通过既有精确repair恢复；Core文本读取仍预留最大真实分配和初始化工作，仅将后续扫描收费限定实际有界读取请求，保留所有来源复核及原100M预算；DF复用官方AggregateFunctionPlanner，并在测试错误中记录查询及阶段。旧DF日志没有具体查询文本，源码定位不冒称已证明旧失败调用。86f两条check及四组回归已实际通过：Core client23、prepared7、generation33、engine33，共96通过/0失败/0忽略，8新增与88既有各一次，原3失败均进入原目标断言并通过；历史e486失败保留。这些结果不替换2d性能身份；全Data5GB归属和冷缓存淘汰尚未闭合。首轮匹配产品和完整Source/Data任务性能不等整个计划结束；首次导入成本计入实际生命周期。Skill继续同一封存CLI/fixture的独立真实任务，模型和脚本不替代。

后继质量清单见[Data缓存测试映射](tests/rust-parity/data-cache-quality-test-map.json)：两条keep-going编译前置、15个互斥模块静态286项（94新、192旧），均须实际libtest名单确认后运行。最终组合11套Node121/121通过、无skip；旧CLI oracle的110/111及10ed基线5/6失败保留。修正预期来自固定Go323原文和明确批准的cache prepare/Emmy说明，旧fixture未改；七product、双Forever、命令顺序、参数与错误合同仍逐项比较。此为元数据/脚本验证，不能替代85条CLI真实任务。

当前摘要只列决策与开放项。完整历史测试、失败、旧SHA和各次报告留在[308efa历史handover](https://github.com/Follen/Lychee-Dev/blob/308efa2766d00273b13c9c7e24a0b956bb60e9e9/handover.md)及对应PR评论。所有用户本机测试仍由唯一父队列派发，受限原始记录不公开。

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
