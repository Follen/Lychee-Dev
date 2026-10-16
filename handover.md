# Rust 迁移交接：唯一当前事实来源

更新：2026-10-16 03:13 UTC。分支 `rust-refactor`，持续交付到[草稿 PR #14](https://github.com/Follen/Lychee-Dev/pull/14)。最新已核远端批次：`2720ccf497c7ded042d15068491d5c1e422876a2`。实现进行中，尚未完成全量验收。

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

## 2. 当前实现与未闭环事项

默认 local/CDN asset export与demux现已贯通真实query准入、同Budget、单遍发布和checked收尾；demux源→frame→manifest捕获与引用保护、同目录租约和最后manifest发布接入真实路径。新两种发布/提交事实沿既有error.details进入CLI，保已发布/unknown及所有主错/cleanup，未新增顶层字段。15实现回归加2CLI回归未跑，既有asset组扩大覆盖全部assets子模块。Data有限剩余为listfile/search共享受控transport/cache、Wago/Raidbots外层pin/receipt两组，正在继续。

SQL有限两项后继已实现：Go primary首个多余点号立即Syntax，公共Pratt仍move AST，通过原infix/precedence观察RHS活跃深度，BETWEEN分低/高相，RAII失败只退本入口标记；7新边界回归未跑。Emmy有限来源收口18路径已组合：selected initializer与Lua/XML字段的实际函数值贯通VM、参数/泛型/返回/赋值/await/assert，不被旧声明overload/docs复活覆盖，原静态声明检查仍保留；17新Rust未跑，43调度入口的静态边界已扫完。SQL与Emmy本轮有限实现现转集中真实回归，未宣称全行为或资源验收完成；Data剩余两组默认入口仍在实施。

实现收口限已有Go323合同及43checker：Data的asset export/demux准入/CDN链已实现待测，仍需asset search/listfile共享可取消transport、Wago/Raidbots外层pin/receipt受控收尾两组；SQL非法三段字段首错与Pratt RHS深度、Emmy selected实际函数来源统一收口已实现并保存，尚待集中验证。Source默认存储已接。完成这些既有差距并集中修闭编译/实际回归后进入第一轮架构与使用质量审计；不扩为通用SQL数据库，不以基础切片数量代替完成度。

默认evidence remove现已闭合原metadata-write→blob→SQLite锁序、同事务分页认证/删除CAS/tombstone/recovery、原句柄删除及checked收尾，保六字段receipt与恢复事实，15新Rust未跑。e8空scope已在实际call执行入口补仅空栈时RAII root，已有嵌套/回调scope不变、原两红测试不改；Nil/Field/Await/Export/Assert消费者保selected来源和shape，19新测试/56场景未跑。该e8原cargo exit101后另有vctip留至drain并forced=true，最终treeReaped/handlesClosed/active0；真实panic与异常辅助进程收尾并列保留。

c5 SQL/Data断言后继按固定Go修正fixture：DISTINCT临时准入与累计账分开、COUNT(*)不读无用NaN、NULL实际Equal回调、合法CTE与原非法语法双测、96MiB失败/256MiB固定预算成功；semantic仅三声明timing字段核exact f64 bits，其他字段及capture仍严等。CASC固定预算/未admission拒绝、64MiB增长640−584=56MiB、local target canonical身份三测试文件修正，生产规则不放宽。原失败仍保留，全部修后Rust待集中验证。

e8实际owned context已编译执行：declaration等三组4/9/6全过，lychee_context 22过2失败，合计41过2失败；失败均environment.rs58 `expression execution scope`，实际环境栈为空，后继已补真实scope且保原断言，尚未重跑。3owner/17sealed日志全SHA核且retired/reaped。c5完整185owner/1093sealed全SHA核；三个interrupted是custody协议故障fixture，其余182为handlesReaped=true。真实SQL/Data失败交原作者按固定Go诊断；Core4项fixture已修真实阶段注入、历史raw设置用正确generation及target首错，Rust未重跑。

SQL默认retrieval入口现统一顶层/WITH/derived/scalar/EXISTS/IN，同一token stream及公开Pratt；FROM首错、各UNION分支后缀的实际preflight、分支CTE声明扫描与执行scope分别保留。21新Rust回归未跑；细粒度Pratt递归深度混合首错和三段非法字段分类后继已实现，7新回归待运行。既有多target关系回归加--no-fail-fast只防首失败跳过后继目标，失败退出保持。

Source默认存储链已组合原控制：index/query/research/semantic缓存、同key manifest CAS、session startup intent/cleanup与generation读取/单次消费贯通受控Metadata、原文件/lease及checked收尾；取消留intent、generation变化不误删，未知提交不重放。缓存配额仍原即时竞争策略，不自动prune；native wire、90秒原期限与2GiB cap未改。25新Rust回归未跑，4个不重复目标已接集中验证；普通ReadDir/native process及不参与授权的best-effort生命周期诊断保原边界。

默认evidence keep/bundle已完成原控制接线：keep持同一writer lease完成历史读取/retention/CAS/tombstone；bundle保输入顺序、拒重复、objects与最后manifest、32MiB单体/128MiB唯一体、原stage同目录no-replace和checked收尾，已发布/不确定结果进入既有error.details。12新Rust未跑；recoverable remove后继已接原控制并保存，15新回归待运行。自有诊断后继恢复return self所属类、alias/声明泛型上下文，并保union分量selected carrier，避免不完整对象绕过赋值结构检查；4新测试/12场景未跑。部分return缺口历史patched session同样存在，旧参照不改、不全局忽略差异；以固定原版普通语义与增强合同分别验证。

本批补齐历史capture读取、Hotfix archive/origin/定义/scan与effective SQL的原invocation及独立Budget，保typed停止、历史身份和checked关闭；8项Core/Data新回归未运行。SQL默认编译入口已贯通lexer、公共parser hooks及lowering控制，17项新回归未运行；wildcard公共hook不能直接返回错误，记录停止后在parse完成且lowering/provider之前返回。非最终UNION后缀与复合错误优先级后继已接；keep/bundle/remove和Source默认存储受控链已保存，全部等待集中运行验证。

修复后的集中验证 [38836247524](https://github.com/Follen/Lychee-Dev/actions/runs/38836247524) 针对e8ae9a3已启动，workspace all-target失败、CLI构建成功，早期58owner/170sealed已核；仅example的4处ByteRange字段仍当方法调用导致E0599，现最小修正字段访问，未重编译；不能把排队或toolidentity成功当产品编译成功。后继同session跨文件member/metatable两处CFG缓存已共享，新增真实三文件回归未跑，不宣称内存/耗时收益。

当前集中验证 [38835081872](https://github.com/Follen/Lychee-Dev/actions/runs/38835081872) 针对 c5b3962：静态组成功，all-target 编译与 owned context 组失败，产品终态失败；冻结三方组按策略跳过。context完整产物3个owner/17份sealed日志及product早期55个owner/76份sealed日志已逐SHA核实，原版引擎树身份正确；实际15条编译诊断为公开MemberList借用迭代、字符串类型与临时值寿命适配问题，后继已一次聚合修正全部15条诊断，并保空返回候选为Unknown；当前过渡Continue测试改为已实现的完整类型/carrier/缓存及真实quality事件断言，历史参照/448不变。17路径只通过语法检查，修后编译和语义执行仍待。生产零patch recipe 已保存不等于已编译或语义等价。

捕获收尾后继现已组合：默认Data捕获的原invocation与独立meter贯通Metadata打开、实际BEGIN/引用读取/INSERT/COMMIT等待和checked关闭；仅重试busy SQL语句，不重放body发布。已提交或不确定的CAP身份进入既有error.details，外层Scope关闭错误仍保typed事实。Source refs/context/diff/query/inspect及验证/语义的11个捕获调用也已改用原control的bounded入口，保持原字节、provenance和128MiB上界。新增15项Core测试和3项CLI事实/组合测试尚未运行；新metadata组已接集中CI，Source消费者实际取消仍待集中验证。同步系统调用与遗留无control API仍有明确边界。后继默认evidence show/list/verify已接同一受控只读Metadata、固定历史CaptureRef认证及checked收尾；list按完整有界页中的embedded ID重新inspect，verify保持原库receipt/CLI投影并流式核SHA，不误套生产大小cap。10新Rust未运行；keep/bundle/remove及其他legacy消费者继续闭合。

当前零补丁生产候选已组合：Source freeze→OwnedBindingSession→OwnedQuery→43项诊断调度使用自有实现；上游恢复固定原版848文件、8,382,174字节，树SHA256 `375d92c5d5e503d77232fabf22a21a82f504c2b38dc5def8fcca1d6ecf06e104`，生产patch清单为空，删除旧补丁文件及引擎对自有binding-contracts依赖。增强测试和examples已转向自有公开入口；独立历史参照固定cd871181，测试专用417文件归档不进入生产引擎。原生差分保留历史真实expected及独立失败状态，不把新实现当旧oracle。40 Rust文件语法检查和39项Node检查通过；当前产品锁编译、完整差分、Retail与资源验收仍未运行。恢复后在主树执行版本工具发现退役Lua fixture引用导致6项失败，现明确当前report及5个signal样本为required、历史归档不参与版本同步；修后10/10与version --check通过。当前rust-parity的7条Node测试命令219通过、0失败、3个Windows专属launcher在Linux跳过。后继已把已认证签名结果的真实owned VM执行、独立Unknown参数不可用、结果完成后的call_result与真实返回比较后的return_check接入同一ledger，保持原公开native_projected/native_checked状态字面及proof来源；4条新真实管线回归已写未跑。五个被换行规范化破坏的固定Lua/XML输入恢复原合同字节/SHA，并只对这五路径禁文本转换；expected原字节不改。缺runtime observation的后继仅允许明确返回声明或已认证EnvironmentCallable，无声明函数仍保持原Unknown，不从任意body猜值。两侧历史/当前差分启用--no-fail-fast并收集全部精确字段差异，任何真实失败仍使整组失败；原448输入与完整旧trace保留。新增真实quality receipt与公开重建参照的已知缺口独立报告，不全局忽略字段。不能因生产recipe归零或源码接线完成就称功能等价或验收完成。

默认target list/add/show/remove现已接同一typed配置与受控分页/SQLite owner：取消旧10000截断，精确name/大小写优先与raw历史引用验证，show输出resolvedPins；remove按Go先独立CAS删除配置、再读取引用generation、每64项删除，保可见部分提交与removedPins。17项新Rust回归已写未跑；target resolve --file现已接实际65537字节受控读取、独立Go typed JSON规则、同一Metadata writer的parent/CAS/winner验证及checked input close；按Go不额外生成snapshot context，已知提交pin保在既有error.details。14 Core＋3 CLI新测试未运行；generic JSON/type/time精确错误文案仍待补齐，不误套project更严duplicate/depth限制。后继project init/status/lock已组合原control、严格项目读取、同一Metadata原Vec认证与held Stage同目录no-replace/replace/checked关闭，默认resolve_pinned/optional和doctor项目边也已接；21新Rust测试未跑。cd576旧持有reader替换失败的回归现走真实project lock并保新旧reader断言。doctor/cache后继23路径现已组合默认独立检查组、全部target配置分页、流式SHA256 cache verify、只读archive检查、原control和独立Budget贯通workspace marker/Metadata；35新Rust回归未跑。当前文件owner采用checked close，保取消与部分报告；旧workspace通用入口、capture内部受控链与native Drop、generic JSON错误文案/延迟type-error首错仍继续闭合。

默认SQL进一步接入typed quoted identifiers与真实单次schema preflight按需RootResolver：local/CDN/effective/meta不再提前准备全部根源；owned WDC Table移动原FileBytes并保留资源owner，local复用Prepared Encoding/Root。32物理源、固定Go100万rows/4096columns/partitions及caller ContentBytes边界已接，capture仍在同一checked-close范围，后续关闭失败保已提交ID。24源与41项新增Rust回归已组合，尚未执行。后继默认local SQL的Encoding/Root/selected现共用query内CASC索引owner：64MiB缓存、XOR首选bucket、原累计逻辑账、真实内存释放与成功capture前原Directory逐块SHA256/最新generation复核已接，8新Rust回归未跑。独立legacy local API仍原路径。后继19路径现已实现默认local/CDN/effective的EXISTS、标量子查询、EXPLAIN ANALYZE、CAST和LIKE；ANALYZE取同一次真实执行的provider回调行数/逻辑账而非RSS。32新增与2更新Rust测试未跑，typed aggregate alias断口及bit函数NULL/首错顺序一并修复。后继18路径已组合真实执行顺序：逐行投影与ORDER表达式先完成，再DISTINCT、稳定排序和slice；每组HAVING→投影→ORDER、MIN/MAX增量驻留、JOIN索引精确成本与仅局部预算fallback退款、真实work与临时安全账均接默认消费者。25新增/1替换回归未跑；cd576子查询类型推断与compound测试适配已修，原expected不改。后继12路径已将malformed aggregate/普通函数DISTINCT验证延后至实际schema binding、保真实child/parameter/resolver首错，LIMIT/OFFSET按Go仅整数或命名参数，星号节点分别保bind/EXPLAIN计费。18新Rust测试未跑；继承的全部SQL语法/错误定位仍须集中差分，不以源码实现声称全SQL等价。 新13路径已将Go词法映射接原公开sqlparser tokens与Dialect hooks，保Unicode/quoted标识符、普通关键词名字、operator优先级、原256KiB/32768tokens/16comment/128depth边界，去掉Rust独有小cap；EXPLAIN不再改写SQL原文。25新Rust回归未跑；全Unicode码位源表检查通过仅证明类别表，不证明SQL执行。复合坏语法首错、非尾UNION suffix时机与编译器内部逐token控制仍是已知后继。

默认 named target 现已组合完整配置-only分页查找、严格解码/名称匹配、真实local/remote pin准备及create-only引用；不读取旧引用历史，不因准备期间配置删除拒绝已认证目标，named结果不冒认底层capture为公开provenance。默认local同时恢复固定Go的client/build→parent/definitions→复查→pin→配置blob→capture顺序。12源与17项新Rust测试已写，未编译执行；原目录/客户端读取的native Drop边界仍待后续收口。

集中验证 [38801126956](https://github.com/Follen/Lychee-Dev/actions/runs/38801126956) 针对 cd576 已终态失败；完整产物171 owner/1011 sealed日志逐SHA已核。context 组实际编译失败：owned signature 调用上游私有 `signature_by_position`，后继已改为公开 signatures 逆序按位置查找，保原索引重复位置 last-wins 及 typed checkpoint，不新增引擎补丁；product all-target 亦失败，早期51个receipt/72份sealed日志已核，all-target/CLI均仅上述E0624。静态入口测试仍把已更新的 Skill 判为 Go3.2.2，后继已改为精确323并证明命令表来源与322字节相同；历史322 Lua环境测试也改为分别认证原351记录与现323全源，仅应用已授权3.5版本字段。71项本地首组及其余142项Node检查已通过，3项Windows launcher在Linux准确跳过；这些修正未包含在 cd576，Rust修正尚未编译。 在已读回 b18 当前树重新运行 static job 的全部 Node 测试，219通过、0失败，3项Windows launcher因Linux准确跳过；Skill 84命令/313引用及自有unsafe清单均无静态违规，322差分仅既有明确引擎文案和授权版本字段差异。无Rust运行或全业务通过含义。另Lua checker旧21个addon extra断言已随完整退役收紧为0，本地34/34通过，真实Lua运行仍待测。冻结三方实验本轮按独立触发策略跳过，既有6699成功仍仅证明其冻结输入。 终态还确认Data lib tests的subquery_tests.rs键类型E0282，作者同完整SQL后继修；compound差分一项本应成功却返回错误，正补真实case诊断。project pin9通过1失败为旧原子替换遇持有reader时AccessDenied，正由完整project Stage替换链修。槽maintenance16/1、removal4/4、repair21/4实际失败均180秒：多数在fixture安装500槽，repair另两在apply；四重磁盘准备并发有实证，尚未证明唯一瓶颈，不扩大生产deadline。后继仅这三组libtest调度改为单线程，保专门并发测试内部线程与所有断言；串行通过也不能抵消原并发超时或当作生产性能修复。Mailbox 1+342真实通过，Core/部分独立Data组亦有实跑；全Retail/IPC/Skill因CLI编译未启动，不能报新内存或完整Source结果。

内存当前实际实现：API定义原文副本在验证重建后立即释放；身份唯一性临时sets在syntax索引前释放；managed reads/calls冻结上下文共享Arc而不深拷贝；XML生成文本去掉Script重复Vec，仅compiler文本保有并供现消费者读取。后继把同一请求的 XML 原文 Arc 在 Script 与各 manifest Program 之间共享，避免每个 manifest 重建原文分配；每份 AST、模板、动作和加载上下文仍独立。原行映射/hash/未知处理不变，Rust与完整Retail降幅均待集中实测。

6699终态产物168 owner/987 sealed日志全SHA核验；owned binary4/dense3/execution3/plan2/function5实际通过，query backend4通过1失败，正常局部变量触发尚缺的DeclarationFlow，后续selected448未执行。此缺口正按完整flow链实现，不改成Unknown掩盖。Data旧可见域错误与三处adaptive测试移出PinnedSet的E0507已最小修；Core旧immediate-busy测试已按真实blob等待合同改取消，历史metadata测试目录加唯一序号。新测试均未运行；同候选旧context fixture失败已在c8后继保存正负用例。

owned查询后端已保存完整累计源码：单文件按需CFG、局部声明flow/赋值/join/基本guard/casts、三种类型关系、member producer与结构化carrier、substitution和本地/跨文件signature。后继33个Rust源已组合通用call、重载/generic、回调参数环境、builtins/返回转换、迭代变量、member flow与相关窄化、来源carrier，均仅通过语法检查，未编译。真实batch现携带贡献顺序Arc，供显式owned构造使用；旧开发构造缺该事实时保IntakeOrder未完成状态，现有oracle还需集中迁移，不能当兼容通过。该历史候选当时尚未切Source实际诊断；后继已组合真实入口与43 checker，见本节首段，实际编译/集中差分仍待。当前不新增生产patch删除，不宣称6699正常局部变量失败已修复。依赖只增加既有锁定rowan0.17.0的adapter直接边，无新包或版本升级。

历史摄取批已把 adapter declaration 与 Source PreparedSnapshot 两处摄取切换为原版 Emmy 公开 batch API，并把原35个测试迁到自有 adapter；删除 checked/shared ingestion、DB shared setter 和 VFS Arc接收改动。重建 recipe 从31降为26 changed paths、生产源从26降为24，完整源码身份重建已核，Rust运行待集中验证。公开API要求String，新增N常驻及N+最大文件转换瞬态文本成本已明确记录；不提高原2GiB worker上限；该批当时仍有补丁。当前93acdcb已恢复848文件原版生产树，完整功能与资源验收仍待集中执行。

Data当前批已将实际默认DB2/navigation/JSONL消费者接入同一lexical Session：owned Encoding双目录、≤8MiB page cache、Root compact index/Memo、首成功读取统计和capture持有贯通。保持页缓存超大不插入也不清旧缓存、原独立控制/预算；18源文件已组合，Rust未执行。默认local/CDN/effective SQL现已组合真实lazy Equal/Scan、query内完整bundle复用与行临时安全账，包含普通/相关IN子查询、CTE/derived/recursive、RHS UNION、EXPLAIN及Unicode binder区别；97新增与40继承测试均未运行。嵌套UNION尾expression-subquery的继承callback作用域后继现已实现，保词法/运行时字段、derived无outer callback、JOIN前缀、quoted-dot输出名与preflight complete，22项新增回归尚未运行。

本地CASC/Selected、decoded cache及streaming消费者现已保原invocation＋独立Budget至目录/index/archive读取、decode、metadata、单遍staged发布及消费式关闭；默认SQL两处materialized读取已接新入口。仅逻辑解码一次，随后required Vec从已验证本地blob有界读取；没有第二次网络读取或解码。同步系统调用仍只在有界边界检查，ReadDir仍为标准Drop。自动/显式project-pin已接实际Data provider，原invocation＋Budget贯穿最近项目发现、64KiB严格JSON/UTF8/重复键/depth16、SQLite创建/冲突winner验证与checked close；专用ProjectReader允许原子lock替换，其他Reader共享策略不变。10新回归未运行；旧local decoded256MiB硬限现已按固定Go改为实际caller max，仍由同一逻辑/adaptive与safety预算准入；3边界测试和8控制/关闭测试已写，未运行真实512MiB大文件。Root/build/cdn/selected四处本地blob发布也保原invocation及Budget。该累计组合未运行Rust。

最新实际结果：bc2终态产物177个owner、7059份sealed日志已逐SHA核验。该精确候选完整Retail ordinary refs约78.53秒、context约78.92秒、API约65.14秒已成功；仍失败的是retained session冷启动EOF/回收，后续常驻warm/steady未测。这些成功不包含后来XML子索引/统计复用，不能归因后续优化，也未证明≤Go30%或内存目标。真实Driver→正式Lua桥9/9通过；其他范围按各自结果，不称全仓绿。

当前已将公共miniz inflater接到实际gzip transport，保持现有前缀drain、独立取消/预算和已读字节计量，用于修复压缩错误与长度限制优先级。既有独立harness实际233向量一致、71/72测试通过（stalled-header peer-close失败保留），当前产品组合尚未运行；没有修改第三方源码，旧被拒本地重跑未执行。

最新验证批 6699bbf 已保存 XML 子节点索引、静态 WDC IN 候选筛选、owned Function/call 与三方工具身份修复，run38781154924已开始，all-target与CLI构建因Data content::verified::read可见域错误失败；当前主树已最小修正到session域，尚待重编译。该次本地静态检查仍有历史addon差异；当前后继已将最后14个旧生产文件及全部旧自动发现消费者迁到原字节历史归档，正式Go323的36套Lua入口恢复到六profile，Node60/60通过且addon静态差异为0，Lua/Go/Rust实际执行待集中验证；三方 Node 31/31通过。更早bc2的真实Driver→Lua桥、槽维护/移除/修复步骤现已成功，待最终sealed证据核具体用例；all-target曾因Core测试模块路径失败，6699已包含修复，不能泛化整轮通过。

owned 后端已累计实现表达式执行、函数/对象/member 与返回 carrier，6699 已实际通过其中 binary/dense/execution/plan/function 测试。正常局部变量 flow 仍有真实失败；当前正在贯通同一 session 的 CFG、类型关系、调用与诊断消费者。这是6699的历史状态；后继93acdcb已切Source生产查询与43项诊断并归零上游patch，当前实际编译/语义等价仍未通过集中验收。

498 的槽维护 13 过/4 超时，移除 8/8、修复 25/25 实际通过；原 180 秒期限、持久化与并发均未放宽。同字节前轮通过不能证明当前没有磁盘争用或性能回归，仍待受控单次业务诊断。

| 模块 | 已实现/已接线 | 当前缺口 |
| --- | --- | --- |
| CLI / 版本 / 打包 | 3.5.0 beta 源码配置；当前 Go describe 合同；正式 addon ZIP；默认 Live 路由 | 最终包安装/更新/卸载全链、参数/输出完整差分；内部 Invocation 空值处理不等于实际 argv 空值已等价 |
| Live / Delivery | Go Mailbox v1 默认 CON、RIP 定位/输入/持久恢复；321键盘静默与322槽启用/reload；500槽发布/维护 | 精确最终树自动回归与真实 Lua Driver 桥；多实例/真实客户端最后验收；未知结果绝不重放 |
| Source | 完整固定 Retail 静态导入/查询；Emmy direct worker/session；Graph move、一次 XML catalog、90秒 research 原预算、调用者身份候选已落地 | bc2普通refs/context/API已实际通过；常驻会话仍失败，新优化全仓时长/内存待实测；生产已切原版上游和自有查询/诊断，零补丁组合的编译、语义和资源验收尚未完成 |
| Data | 本地 DB2/CASC/SQL/Hotfix；可取消 HTTP、remote catalog/target/encoded Session；direct remote target 已接实际 CLI；decoded alias 与共享 body 的 retained 责任转交已组合；Core 原控制贯通 blob/pin/capture | named target、streamed assets；JSONL scoped输出已组合待实际验证，SQL/DB2/navigation默认provider及外层Metadata已接，内部Store/Fragments/target预算错误身份修复已组合，自动pin/effective旧等待与实际使用仍待闭合；archive计量、gzip边界、合法raw-URI差异仍有缺口 |
| Skill | 固定 Go 工作流及实际 CLI 验收入口 | Source长语义阻塞后的完整任务复测；Source/Skill/Data各自独立真实使用循环未收敛 |
| 清理 / 质量 | 旧 addon 额外文件与旧协议消费者已迁到精确历史归档；当前55个正式addon文件与36套Go323 Lua入口恢复，Node60/60通过、addon静态差异0 | 新组合Lua/Go/Rust实际执行待集中验证；最终四阶段审计尚未开始收敛 |

Core单遍流式发布与Session scoped verified body已独审组合：只解码一次，原句柄暂存后追加有界磁盘copy并同目录发布；23+7新增Rust尚未运行，默认asset/export接线另片。额外磁盘读写及最多两份decoded暂存是真实成本，未证明性能收益。现workspace::open整读、外部Destination Drop仍有受控关闭缺口；同Session Encoding/Root/Memo和ready/rootReported默认复用现已实现并保存，14项新增回归尚未执行，不能仅据代码宣称统计与缓存验收。

Data 323自适应基础R23.1已独审并组合：真实workspace准入后采样Windows可用内存，配置容量分摊、固定ceiling与按需增长、scratch峰值及独立安全账；growth probe后复核原控制与独立预算，取消不批准新分配，已发生网络账仍保留。26新增Rust未运行；R24.1已将七个实际SQL/DB2/raw/asset inspect/JSONL/catalog/encounter默认消费者连接到真实准入后的同一Budget，并在Go公开边界输出memoryBudget；22新Rust仍未运行。已修独审发现的raw读取校验/取消先后与已提交asset capture错误事实，尚未完成新合同验收。固定预算历史证据不改标。见[源码清单](tests/rust-parity/adaptive-query-r23-1-freeze.json)与[Go测试映射](tests/rust-parity/adaptive-query-r23-1-test-map.json)。

默认remote asset export已接真实准入/同Budget/verified body借用/raw与PNG/WebP/有序source→image→manifest依赖和原子输出，10新增Rust未运行。已提交capture的后续错误保ID、原typed原因和输出已发布事实；CLI沿既有JSON error.details透传，纯旧metadata_limit投影不变，2新增Rust未跑。外部Destination与capture内部旧关闭边界、默认完整SQL/本地CASC控制与自动project-pin受控读取仍在贯通；same-Session准备复用已实现待集中运行。

323紧凑记录R2已独立审查并组合：ID→u32 physical location索引、copy/WDC2 alias共用位置，Go逻辑计量与Rust独立安全峰值分账；畸形Rows不足时在增长前额外安全准入，足预算仍保原晚期格式拒绝，不削正常行。11新Rust与既有WDC回归待执行；理论容量模型不等于实测内存降幅。

### Emmy 全量替换当前顺序

当前生产recipe为零源码补丁，固定848文件与上游原始树完全一致。源码迁移完成不等于行为验收；以下工作继续：

1. public declaration rendering的历史27+27/450 trace与6699独立Windows三方31×3/450/6相等，仅证明各自冻结范围。constructor中间checkpoint、资源压力及产品硬deadline未测。
2. 完成正常查询与43项诊断的完整增强合同，包括已知签名结果、未知参数独立不可用、真实返回类型检查与同proof证据；不以Unknown或空coverage代替正常能力。
3. 新测试runner先运行固定历史引擎，再运行当前原版引擎＋自有实现，认证完整trace并逐字段比较；任一侧失败仍使整组失败。历史参照仅用于测试。
4. 集中执行当前产品锁all-target、全部增强/取消/资源回归及完整Retail cold/warm/session，再进入最终四阶段循环审计。旧Linux未确认回收owner保持保留。

## 3. 最近已验证证据与限制

- 本次集中验证按`[rust-native]`运行产品/context完整组。已成功且输入冻结的独立A/B/C实验只在`[emmy-contracts]`或dispatch时重跑，未触发记SKIPPED而非当前树成功；不重复旧实验替代新产品验证。SQL/CASC/decoded/project-pin目标已接对应非重复测试组。

- **8e2dc296**：15文件上传后逐SHA/长度读回一致。新增direct remote CLI route及Core optional blob R4已独审；7项CLI和20项Core Rust测试仍NOT_RUN。R3目录walk未贯通控制的finding由R4五个native callsite修复，原R3证据保留。
- **同组合本地静态**：干净Git基底加精确15文件执行实际static job的14条Node命令；221测试中218通过、3项Windows launcher在Linux跳过。前13命令成功；最后真实parity报告仍退出1，保留21旧addon额外文件差异。未执行Rust或游戏。
- **b16a2d87**：Windows run `38740832295` 已结束失败。精确 all-target 编译、sealed CLI build、真实 describe、静态 full Retail 成功；final artifact `11981206048` 的168 owner/2655 sealed logs逐hash核验。真实native refs/context/API/session及IPC bootstrap在约27–31ms即被`delivery.invalid_release`拒绝：Rust delivery reader仍硬编码next，与获准3.5.0 beta包绑定冲突，分析器没有开始，本轮没有Graph/XML优化后的完整语义内存/耗时样本。现候选共享既有精确version→channel函数，公开manifest读取与update的5组合回归已写，Rust仍NOT_RUN。normal control和Mailbox的三项fixture偏差已独审修正，原失败保留。66c2/311250/8e2dc后续新增不在b16编译证明内。
- **本地GNU typecheck**：fixed b16首轮600秒冷编译超时并完整回收；暖轮因git archive漏掉export-ignore的4份真实profile而exit101。已按完整Git blobs重建5244文件闭包并独审，173处include无缺失；第三轮在原600秒上限内实际exit0（Cargo 5m10s），完整前后305官方包/源码/工具/config复核、16封存日志核hash及进程回收通过；owner `bdfa539a-0c87-4ca6-8abb-bd20c81fc2b4`，receipt SHA256 `b34ef64e2402887d6e8ec6e75042fe9a7edd60fc3aa267cd540aae8b50ac2362`。GNU检查不等于MSVC或Windows运行。
- **b16正式Lua与静态内存**：固定Go322正式Lua的220项observations实际通过，输入未变且全部句柄回收；不等于真实游戏通过。静态Retail source-process采样working set冷索引675.54MiB/暖索引224.04MiB/查询362.52MiB，private bytes783.52/329.69/539.76MiB；与未启动的全语义路径严格分开。
- **479e57a6终态**：run `38754201940`失败，artifact `11987686231`的175 owner/2755 sealed logs全hash核验。all-target/CLI/describe/静态Retail通过；正式Lua220 observations通过。Driver桥被Lua加载路径格式阻塞；IPC三组小真实native bootstrap成功，但PowerShell peer编译把反射字段未赋值警告当错误，quota/idle/lifetime尚未执行。Data remote159通过4失败：两项同digest blob并发锁立即busy、一项Session暴露raw cancel token、一项既有gzip边界，分别继续修复，不改失败期望。

- **9ba0bd1 SQL组合**：R17.1实际`--cdn --snapshot`静态/effective/meta SQL及EXPLAIN/CSV走scoped remote provider，持有admission至capture并借用Session共享body；六项真实consumer测试仍NOT_RUN，现有remote测试组会匹配。local同受64MiB executor reservation与bindings提前的错误优先级调整。Go外层Metadata生命周期、自动pin/effective旧等待仍未完成，不宣称完整SQL准入闭合。
- **b0fe353 definitions组合**：R3把原invocation与独立Budget贯通metadata等待、可取消HTTP、解析及发布；实际已读网络先记账、consumer留存后准入、原30秒request上限/4MiB+1前缀和Go逻辑计量保持。R1/R2已发现的取消fixture竞态与解析错误被stop覆盖均已独审修正；24新增Rust未运行。现有definitions::和remote::组覆盖，无重复新job；实际SQL/DB2调用已在当前R18.3组合改接；Core隐藏publication Directory checked-close与既有gzip优先级缺口仍保留。
- **bc85a4c DB2组合**：R18.3接实际record/page/schema/search/FK远程provider，outer Metadata→pin/lease顺序逐Go命令保持，owner持至capture。独审修正两项：外层/读pin直接保原invocation+Budget回调；资源cleanup-only后统一stop，避免把同一stop当close event。15新增/11继承Rust未运行。后继6文件已独审修复Store/Fragments/target内部Budget child错误身份，保target真实120秒上限与独立预算，7新增/2修Rust未跑；normalizer内部回调、资源lock单次同步边界及capture/Directory隐藏close仍未全部闭合。
- **当前navigation组合**：R19实际item/spell/decor/creature/encounter远程路径共享Metadata→admission→pin→Session→capture；encounter仍保两个独立capture。6新增Rust未跑，local aura/summon补充source复核已明确，不能冒称整个远程业务验收。
- **498c2ea三方契约**：51文件已逐SHA/长度读回，独立Windows run `38763797404` 产品组运行中；三方契约组已在Cargo身份认证前置失败，已按官方dated manifest→archive→cargo.exe独立认证修复copied policy，原严格owner认证保持；Node12/12通过，新三方测试仍未执行；该focused锁实验和产品all-target/业务组各自报告，31×3未运行不得写通过。
- **479新语义及内存**：refs/context/API均约90.47秒exit5，retained cold约90.21秒deadline；worker完成XML约15.94–15.97秒、source admission约31.19秒，但Emmy prepare尚未完成。source-process采样working set1957.62MiB/private2062.78MiB，retained cold1897.12/1999.23MiB；warm/steady未测。此候选含Graph/XML优化，但原90秒更早终止、执行阶段不完整，不能将低于旧ce6约2356.48MiB解释为已证实内存降幅。100ms采样有缺口，是观测下界而非RSS/绝对峰；整体性能仍未验收。
- **479测试入口修复**：真实Lua桥首例因verbatim路径进入Lua5.1 C文件API而启动失败，后八例被安全guard阻断；候选在全部Lua参数/payload边界转普通路径并重验canonical身份。IPC三组因C#反射字段CS0649导致Add-Type失败，候选显式初始化required字段，未禁warning或放宽schema。本地IPC Node26/26通过；两项实际Windows仍NOT_RUN。
- **当前Source context R2b**：仅同scope内证明无outgoing的文件move复用incoming行，XML尾部原路；独审发现混合charge重放step的return-reserve边缘缺陷，已用收费前安全域检查/边缘原路fallback修正（同时修旧incoming）。9新增Rust未跑；复用改变poll频率，不承诺异步取消竞态逐counter同构，保原control/deadline/typed stop，未实测耗时收益。
- **当前远程读取修复**：Core仅同digest bytes publication改为原File上的真实锁竞争等待，保原deadline/独立Budget/一次checked close；Go323的blob/lease源码与322相同。Session五处metadata不再暴露raw cancellation token，仍在相同边界执行原invocation-first/Budget回调，修复读取后ordered控制来源丢失。8项锁回归与2新增/11继承Session Rust仍NOT_RUN，不据静态审称479失败已消失。
- **当前JSONL R20.2**：实际remote stream持外层admission至capture与checked finish，首次sink失败后不再写；已知capture已提交而后失败时message明确真实ID、typed内部保多因，不新增公开frame字段。9新增Rust未跑；capture内部旧Metadata/Directory Drop及unknown commit未返回ID仍是明确缺口，不能据外层finish宣称全部checked-close完成。
- **Source历史失败**：62d cold refs在约182秒runner成功但不满足Go90秒总预算；context超时、retained worker EOF仍是实质缺口。ce6全部长语义路径失败。新90秒、phase、Graph/XML修改不能在新全仓结果出来前宣称修复。
- **执行/写入边界**：被明确拒绝的重复gzip验证未重跑；PR14标题/正文刷新两次拒绝后保持暂停。代码、文档与其它已授权验证继续，不改rules、不换路绕过。未发布、未合main。

所有旧批次、owner/hash、失败与修复来源见[历史证据记录](docs/toolkit/rust-migration-evidence-2026-10-15.md)。旧状态按原树/时间理解，不覆盖本文现状。

- **498c2ea2终态**：176owners/2763sealed logs独立SHA核一致。S2 atomic oracle实际成功，S3首个NotManaged-call组合失败且后续比较未全执行，原因与待补后继见上节；全Retail仍90–92秒失败，source进程采样WS fullsemantic2353.78MiB/private2481.14，retainedcold2257.33/2373.06失败，暖/稳态未测，不证明降幅。XML约11.7–11.9秒、Emmy约38.1–38.4秒，warm prepared76.05秒后未完成；API内部queried78.45秒仍未在原总预算完成响应。Data安装fixture在create_dir同tick重名，最小序号修复已审未跑；两effective SQL fixture已补真实完整44B XFTH/精确DataPin capture输入，缺失/重复/不完整/截断/异pin负例保留，新测试未跑；旧archive锁/ordered cancel修复属后续bc2、gzip既有差异保留。

## 4. 接下来按可交付批次执行

1. bc2普通refs/context/API已在90秒合同内成功；继续处理retained session冷启动接近2GiB上限的EOF与回收。6699编译可见域错误已修，集中重编译后验证。Data推进完整Prepared与lazy SQL默认链，Emmy推进真实摄取和诊断链及相应补丁退役，不再逐小片等待审查。
2. 本地symbols因Emmy owner未确认回收而保留，不接管/新channel绕行；当前479e57 GNU完整输入已准备但未执行。独立Windows三方契约job已真实成功，资源压力和产品deadline仍需后继验证；未来Linux观察修正不追认历史owner。
3. decoded alias与body owner已组合，接续真实DB2/SQL/static/effective/meta/JSONL外层admission到capture。EXPLAIN/schema-only不能扫描内容，callback不得重放；大资产保持流式。
4. named target完整local/remote默认链已组合，下一集中批验证真实分页、历史坏配置、准备期间删除/引用冲突及CLI输出。SQL typed标识符与真实按需解析已接线；local SQL的query内CASC索引缓存/原路径rehash也已接线；EXISTS/标量/ANALYZE/CAST/LIKE已组合待集中运行，继续投影/DISTINCT/排序和完整执行计账差异。
5. 继续全部Emmy hook实际外移、真实Source可读性、Skill/Data使用质量及旧消费者替代；完成后依次执行下列四阶段审计。
6. 每批及时普通push保留工作。只有有意义的验证批使用 `[rust-native]`；fast成功/长组skip不等于产品通过。只维护此handover，历史细节移至证据文档而不持续堆叠当前页。

## 5. 验收与循环审计

完整[F01–F13功能、P01–P14协议、B01–B07性能和Go来源合同](docs/toolkit/rust-migration-acceptance.md)保留；[321增量](docs/toolkit/rust-go-3.2.1-increment.md)、[322增量](docs/toolkit/rust-go-3.2.2-increment.md)与[323增量](docs/toolkit/rust-go-3.2.3-increment.md)是累积要求。代码已实现、静态已审、某树历史通过、当前实际通过必须分开。

- 固定Retail数据为Gethe/wow-ui-source `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b`：4036 Lua/XML/TOC，32,269,051字节。每次核身份后全仓分析/代表查询/源码定位/refs/context/flow/cold-warm/session/释放，不以小fixture代替。
- Source、Skill、Data分别由独立使用者执行真实CLI任务，记录输入、完整输出、正确性/可读性/来源/错误和复测。实现者不自评通过，无法运行时记blocked。
- 全量实现后的四阶段**顺序循环**：①架构/全业务/Skill/全部代码；②有效覆盖与无独立价值冗余；③真实消费者确认后的死代码；④前三维度联合全仓。每阶段独审→修复→实际验证→独立复审至无新可行动finding，再进下一阶段。每批源码审不等于最终收敛。
- Go323测试映射为当前差分oracle，Go322映射仍保留，320/321原证据保留；尤其未知结果不重放、事务、取消、空map、槽迁移与错误恢复。记录固定树、覆盖分母、平台排除与停止条件，不保证数学零bug。
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
