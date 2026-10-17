# Rust 迁移交接：唯一当前事实来源

更新：2026-10-17 20:53 UTC。分支 `rust-refactor`，持续交付到[草稿 PR #14](https://github.com/Follen/Lychee-Dev/pull/14)。最新实测产品/runner `31a873a677e355f39b54ea5635ac22135c271904` 六fresh场景全Facts/字段/重复/顺序/共同summary等价，完整实际读取证据归档核过后cleanup。冷4.315s、暖0.387s、四变体1.269–1.281s；六个Rust/Go比值0.435–0.682，均未达≤0.30，仍是单组诊断。完整性能与最终五阶段循环审计未完成。

### 当前未测候选：认证Facts子树所有权转移

默认loose与pack两处在原schema/input/canonical FactsSHA认证完成后，用get_mut/map(Value::take)/unwrap_or(Null)替代整子树clone，再进入原typed反序列化。缺key不插入新null；两处原控制检查、miss回退、pack外层完整性与checked-close次序不动，不合并helper或更换缓存布局。只减少重复所有权树，仍完整解析/验证/哈希，不把约480ms FactsRead全部称可节省。

新增正式9项：`index::facts_decode_tests::take_facts_*`八项和`index::facts_cache::tests::take_facts_packed_inner_validation_keeps_outer_integrity_and_exact_recovery`，覆盖历史clone全bytes、五数组/Unicode/可选/重复/深unknown/数值、缺null/坏类型、取消/过期、真实loose/pack/legacy默认消费者与外层认证仍成立的内部损坏恢复。rustfmt解析、diff检查和独立静态审阅通过，Rust/业务/性能NOT_RUN。下一固定批alltargets→完整index（9新逐名）及Facts/Go104→唯一匹配release/assembly→完整fresh六场景，原31a实测不重标。

### 最新实测：31a借用canonical序列化完整六场景

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

12fresh index+4准备调用，wrapper0/succeeded、finalIntegrity=true、NOT_RUN空。17新owner sealed/reaped，16retired、Node stage保留；12产品native无forced/deadline，编译knownAuxiliaryCleanup原事实保留。完整五Facts数组与实际Go/Rust pack/loose/index/manifest读取tuple均归档验证。16附件/52,802files、完整包131,137,681B SHA `b8fcd61effb36761a2dbe8048aa9c15a47313fe434a5aa0046229446a940345d`由本机GET/CRC/片与wholeSHA实际核过，云端独立复核待原唯一作者完成。

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

- **Data 默认链已接入**：本地/CDN、Prepared/cache、SQL、导航、asset、Hotfix、自适应内存与checked收尾已有实现。统一单堆SQL保持默认栈、原深度、逐行provider与真实预算lease；正式relational299及原6执行崩溃回归通过。local_query76/0/1、全Data库1063/0/1已有证据，原集成预算失败经7db27/0/0闭合。当前剩余为新Data配对入口真实执行、代表性SQL/读写/解码性能和最终独立审计。
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
