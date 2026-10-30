更新日期：2026-10-30，Asia/Shanghai。历史审计见[审计报告](docs/Clean-repo-2026-10-14.md)；本文件只记录当前交接。

## 当前交付：3.2.7 与 RECEIVER-BINDING-SET

- 用户报告`ReceiverBindings.lua:70: Usage: SaveBindings(1|2)`并要求修复，随后授权推送GitHub和新版npm；本次发布3.2.7，沿用main及latest，基线`15f4db7a05`。主代理完成ReceiverBindings及相关Lua回归和文档，负责发行及按既有要求更新本机，无新分支、worktree或PR。
- 已用实际文件的延迟事件回调复现：GetCurrentBindingSet返回0或nil时，两个默认键已设置，SaveBindings抛出相同错误；返回1或2正常。用户日志未记录具体返回值。现有fixture固定返回2，未覆盖方案未就绪。
- 已实现：仅在需要补齐默认键时，于写入前验证并固定1/2方案；无效或secret值不写入，启动路径等待BINDINGS_LOADED/UPDATE_BINDINGS，战斗时切换为脱战等待。重试重新读取玩家设置，写入前注销监听防止同步重入；手动重置完成等待时同步清除旧诊断。
- 验证通过：原错误先在新增Lua回归中复现，修复后覆盖0/nil/非法编号/字符串/secret、账号与角色方案、绑定期间返回值变化、玩家修改、战斗切换、重复事件及手动重置；build/vet和Lua5.1必需的tests/protocol、tests/addon整包通过（后者覆盖六类客户端fixture）。固定Retail源码`18c6d677b605b312f3d813d498404631fb941977`确认两个绑定事件均为同步事件；各客户端事件目录均包含它们。
- 实现提交`dcb35d7f36`及固定同一提交的`v3.2.7`标签已推送；[main CI](https://github.com/Follen/Lychee-Dev/actions/runs/41053821612)和[正式发布流水线](https://github.com/Follen/Lychee-Dev/actions/runs/41053821606)全部成功，包含Lua必需完整Go套件、竞态检测、源码重建、真实LuaLS/Source、Windows隔离安装、OIDC发布及registry重新安装验收。实机验收保留在todo.md。
- 3.2.7版本同步后build/vet、版本/Skill合同、发行输入和93项Node测试通过。本地Lua必需完整Go运行中31包通过，delivery包因20分钟总时限终止（最后测试处于槽文件写入，无断言失败）；15个未完成测试已单独补跑通过，delivery的94个顶层测试均有完成结果，合并两次记录为3029项测试pass、42项skip、0测试fail。初次整包超时仍单独记录，云端完整复验已通过。
- 用户随后明确要求取消整包限时。本地baseline、CI、发行验收及文档统一显式使用`-timeout=0`，保留并发限制、产品期限与测试内部定向等待；历史验收记录不改写。修改后93项Node测试再次全部通过，云端按新设置完整复验通过。
- [GitHub Release](https://github.com/Follen/Lychee-Dev/releases/tag/v3.2.7)已发布，六项产物齐全；独立registry回读确认latest=3.2.7、next=3.1.2，integrity和provenance元数据存在。
- 本机默认update预检查遇到两处环境冲突：`_classic_beta_`仍有1.60.1.70450旧exe，但当前`.build.info`没有wow_classic_beta记录；`_cn_beta_`缺少`.flavor.info`。当前构建清单已含wowf 1.60.1.70658；本次不扩展产品支持或改写安装身份，两个旧测试服目录保留原状。
- 明确选择Skill及retail/classic/titan/anniversary/classic-era五个客户端的受管update已完成：complete=true、六个目标全部current，CLI为3.2.7、提交`dcb35d7f36`、workspaceDirty=false，npm更新事务不存在。正式服安装文件已含新绑定方案校验及等待处理。游戏等待后续reload，Skill供新Agent上下文使用；本次未操作游戏，实机覆盖不变。
- 发布、可识别目标更新和本轮清理已完成。固定文件清单的PowerShell脚本删除`.tmp/release-3.2.7`中的18份临时文件、空目录及一次性脚本；原项目记录、报告和其他worktree保留。两个旧测试服的安装身份问题未改动，不属于本次快捷键修复。

## 当前交付：3.2.6 与 doctor 恢复

- 用户已明确要求发布npm 3.2.6，沿用main及latest；发布后按既有要求更新本机CLI、Skill和受管插件。实现提交`0e04ef6690`及固定同一提交的`v3.2.6`标签已推送，没有新分支、worktree或PR。[main CI](https://github.com/Follen/Lychee-Dev/actions/runs/40294701830)和[正式发布流水线](https://github.com/Follen/Lychee-Dev/actions/runs/40294702788)全部成功，包含源码重建、真实LuaLS/Source、Windows隔离安装、OIDC发布及registry重新安装验收。
- [GitHub Release](https://github.com/Follen/Lychee-Dev/releases/tag/v3.2.6)已发布，六项产物齐全；registry独立回读确认latest=3.2.6、next=3.1.2，integrity和provenance元数据存在。
- 本机受管update已返回complete=true：CLI、1份Skill及7个客户端addon全部current=3.2.6；PATH实际执行提交`0e04ef6690`、workspaceDirty=false，npm更新事务已结束。正式CLI帮助及已安装Skill包含统一doctor修复入口。addon等待后续reload，Skill供新Agent上下文使用；本次安装没有操作游戏，不增加实机覆盖。
- 主代理完成单一doctor入口、整合及文档；slot_reset_worker完成delivery/slot_reset，slot_retirement_worker完成退休与槽修复关闭路径，doctor_repair_worker完成channel/doctor_repair及迁移fixture，均已交回并停止写入。
- 已用正式3.2.5复现：原CON持有有效runtimeEnd，但缺失`.lycheedev-slots.json`使精确槽退休失败；未关闭的窗口占用又阻止受管重装。现已将修复编排集中到channel，诊断与修复共用公开doctor入口。
- 用户要求先初始化JSON和500槽，再修改代码。已在原项目保留63份非空载荷和完整连接日志，初始化槽池并通过InspectSlots；原`CON-fc08a9d9b9382f1e9b55c6abae978023`已正常关闭，原未知业务状态未提升。新`CON-52addb75ebd21a0eb286625b593ff21d`在LycheeGroup原项目成功绑定，保留供其继续调查；未修改LycheeGroup业务或组队状态。
- 公开命令已统一为doctor：默认只读，--fix恢复，--fix --hard-reset归档并初始化500槽。损坏清单、损坏槽、原runtime尚存、默认归档中断续走、旧版升级、共享占用及未知输入保留均有离线回归；对应缺陷已先复现后修复。
- 后续新连接的重载绑定等待已处理：用户手动reload后，正常disconnect按新runtime证据关闭CON-52...，此前verified报告保留；新CON-67b2def81ac72b887cf22c766cdf7c51成功绑定，业务任务完成60秒声音采集后正常关闭。开发版统一doctor只读实机复验healthy=true、无owner；没有为回归重新破坏游戏槽池。
- 3.2.6版本同步后本地验证通过：build/vet、版本/Skill合同（83命令、228引用、0违规）、发行输入检查、93项Node测试；Lua5.1必需完整Go套件（包并发2、测试并发4、包时限20分钟），32个测试包通过、3061项pass事件、0fail。42项测试按既有环境或辅助入口跳过，另3个包无测试。新增重置流程未做破坏性实机复验，七产品矩阵未扩展。
- 本轮临时恢复脚本、只读诊断脚本与完整测试日志已从`.tmp/slot-pool-recovery-20261026`清除；原项目中的连接日志、已验证报告和63份原载荷归档保留。
- 发布、本机安装与临时清理均完成。按固定文件清单的PowerShell脚本移除`.tmp/release-3.2.6`中的8份临时文件、空目录及一次性脚本；原项目记录、载荷归档和其他worktree保留。本轮没有待交接任务。

## 当前交付：3.2.5 发行

- 用户要求推送GitHub并生成新Release；本次为3.2.5，沿用main及latest渠道。实现提交`c3450d9a32`及对应`v3.2.5`标签已推送；沿用此前发布后直接更新本机的要求，无新分支、worktree或PR。
- 主代理负责Source编码修复、版本同步、发布验收及本机受管更新。实现已完成，同一Forever PIN的8组query和异常行inspect通过，详见下方；版本源和渠道清单已同步为3.2.5。提交前build/vet、版本/Skill/发行输入检查、93项Node测试全部通过；Lua5.1必需完整Go套件通过（2962项pass事件、42项既有可选skip、0fail，32个测试包通过；包并发2、测试并发4、包时限20分钟）。
- [main CI](https://github.com/Follen/Lychee-Dev/actions/runs/39589863771)及[正式发布流水线](https://github.com/Follen/Lychee-Dev/actions/runs/39589869187)全部成功，包含源码重建、真实LuaLS/Source、Windows隔离安装、OIDC发布及registry重新安装验收。[GitHub Release](https://github.com/Follen/Lychee-Dev/releases/tag/v3.2.5)已发布，六项产物齐全；registry独立回读确认latest=3.2.5、next=3.1.2，integrity和provenance元数据存在，不声称完成其密码学验证。
- 本机受管update已返回complete=true：CLI、1份Skill及7个客户端addon全部current=3.2.5；PATH实际执行提交`c3450d9a32`、workspaceDirty=false，原npm更新事务已结束。此次Skill通过正常受管更新完成，未重启Codex或操作游戏；addon等待后续reload，Skill供新Agent上下文使用。
- 正式CLI按原Forever PIN复验query和397–403行inspect成功：4481文档、0语法诊断、1编码告警、coverage.complete=true；18128原字节及SHA-256不变。query capture为`CAP-0c07d22691698456bf52c47df4344e0f1f348fcfdefa09c62d2244362420743d`，inspect为`CAP-c2ff74d5dab50f2cf1a06e107ad11d0b9bf9374fc304f05e1e79255c5aa0a355`。
- 发布、本机更新、正式包复验及临时清理全部完成。按用户要求编写固定文件清单的PowerShell脚本，逐个删除`.tmp/source-forever-encoding`和`.tmp/release-3.2.5`中的18个临时文件，再移除空目录；已核实两目录及一次性清理脚本均不存在。原PIN、镜像、captures和其他项目保留。

## 当前修复：SOURCE-FOREVER-ENCODING

- 用户报告3.2.4的source query在Forever 1.60.1.70450固定提交`172a387e3ab85b26e6640d3058d5ec1e6ccd4c63`失败。沿用main，基线`c9bd90ea2b`；主代理负责Source分析/索引、回归和必要文档；explore_source_encoding完成下游编码边界的只读探索，没有产品文件写入。
- 已通过PATH正式CLI复现原命令和`codebase.analysis_encoding_or_budget`；原PIN身份已核对。初始`source list --snapshot`显示ready=false、snapshotFiles=0。固定树4481份文档中仅`Interface/AddOns/Blizzard_NewPlayerExperience/Camelot/Blizzard_TutorialData.lua`第400行注释含非法UTF-8字节0x92（偏移12719，文件18128字节），没有超过32MiB的文档；全局UTF-8拒绝导致整个索引中止，未进入LuaLS。
- 已按用户补充要求将编码告警与语法诊断分离：告警不降低coverage、AST文件数或addon加载有效性；query和inspect共用U+FFFD显示，原始归档与哈希保留，非UTF-8归档不声称UTF-8编码。真实语法错误和跳过的超限文件仍影响完整性，单文件超限单独报告路径/实际字节数/上限。索引与文件facts使用v3缓存格式自动重建，固定提交、PIN和parserRevision不变。
- 回归先复现错误分类、inspect拒绝和加载失败，再确认修复。build/vet及Lua5.1必需环境下addoncheck、codebase/...、command整包通过；覆盖告警位置、query/inspect显示及原字节归档、AST统计、同提交diff、非法token和超限。提交前完整检查和发行状态以上方3.2.5交付为准。
- 开发CLI在原PIN `PIN-4156b40776bf2bb2caa2c8517ea26b93b61a1c211241607c30db82297cf727f9` 完成precise/exploratory×api/lua×issecretvalue/SecretReturns八组查询，以及异常正文query和397–403行inspect；全部ok=true、exit0。4481文档/4481 AST、0跳过、0诊断、1编码告警，coverage.complete=true；limit/关系预览的真实截断仍保留。异常行显示一致，query/inspect SHA-256相同；inspect保留18128原字节。原命令capture为`CAP-60d8d999097090f67de1a26f01d03cdcc093c2637fac9b9ef432a381a2813015`，inspect为`CAP-c360177a7682bb2bb4e68e07f6895ce6cc5fb901bcf2ba935dd34ab805d7d547`。
- 实现、验证及临时清理完成，纳入3.2.5；提交、发布与本机安装状态以上方交付为准。可复用回归保留在internal测试；原PIN、镜像和查询captures保留，没有游戏操作，无未完成的本轮实现项。

## 当前交付：3.2.4 发行

- 用户明确授权提交到main、版号加一、发布npm并更新本机插件。本次为3.2.4，沿用latest；实现提交`29bdbdaff5`已直接推送main，`v3.2.4`固定同一提交，不另建分支或PR。
- 主代理负责发行及本机受管更新。范围为FB-COMBAT-01修复、回归和3.2.4版本同步；[main CI](https://github.com/Follen/Lychee-Dev/actions/runs/39533085883)及[正式发布流水线](https://github.com/Follen/Lychee-Dev/actions/runs/39533113368)全部成功，含源码重建、真实LuaLS/Source、隔离安装、OIDC发布与registry重新安装验收。
- 版本同步后的build/vet、版本/Skill/发行输入检查及93项Node工具/启动器测试已通过；Lua5.1必需完整Go套件通过（2947项pass事件、42项既有可选skip、0fail，32个测试包通过，包并发2、测试并发4、包时限20分钟）。游戏实机验收不由文件更新替代。
- 用户明确允许关闭LycheeGroup项目已完成业务的旧连接 `CON-fb153033bbcb72b47b486e45187e4395`；已在原项目正常disconnect成功（closed=true、bound=false，报告verified、cleanup=released/complete），报告和恢复记录保留，安装占用已解除。
- npm独立回读确认`latest=3.2.4`、`next=3.1.2`，integrity及provenance元数据存在，不声称完成provenance密码学验证。[GitHub Release](https://github.com/Follen/Lychee-Dev/releases/tag/v3.2.4)六项产物齐全。
- 本机CLI、1份Skill和7个客户端addon均已为3.2.4，最终受管update返回`complete=true`且全部`current`，PATH实际运行提交`29bdbdaff5`、workspaceDirty=false。Skill目录移动曾被拒绝，当前Codex进程持有其references目录句柄；按用户“把Skill弄到位、不重启Codex”的明确要求，核对23个Skill文件与正式3.2.4完全一致后，仅在原目录同步正式安装记录，再由CLI完成整套验收及原更新事务收尾。没有重启Codex或运行游戏；addon需后续reload，Skill供新Agent上下文使用。
- 本次发行、安装和清理全部完成；已移除本轮`.tmp/release-3.2.4`、诊断工具、一次性脚本及完成后的Skill事务备份，原npm更新事务已正常结束。没有未完成的本轮安装；其他项目的报告、恢复记录及worktree保留。

## 当前修复：FB-COMBAT-01 聊天框键盘传播

- 用户在原因调查后要求修复 `ChatFrame1EditBox:SetPropagateKeyboardInput()` 的保护调用错误。沿用 `main`（基线 `bae2c587d6`），主代理负责 `FocusBindings.lua`、`InputState.lua`、对应回归和文档；没有新PR或子代理文件分工，已随3.2.4提交推送，发行状态见上方。
- 已确认 `addon/Bridge/FocusBindings.lua:40` 在战斗判断前调用该API；原聊天回调也在这次调用之后。持续战斗足以到达此错误路径，无需假设进战竞态。Retail 12.1.5固定源码 `18c6d677b605b312f3d813d498404631fb941977` 的API声明标记 `HasRestrictions=true`，但不公开C层的具体限制；原报错未附战斗状态，不能确认5次实际发生时机。
- 已实现：按键回调在传播设置前检查战斗，原处理函数始终保留，并在其返回后复核状态；战斗中不接管新焦点、Ready为false。停用立即撤销INPUT/采样/焦点监听；若旧编辑框尚未恢复，仅用既有框架的脱战事件完成恢复后注销，不新增frame或轮询。
- 验证已完成：受保护API模拟先失败于原第40行，修复后通过；覆盖持续战斗按键、焦点切换、原回调改变状态、停用后的脱战恢复、清理前重启及无接管编辑框时的零清理事件。`go build ./...`、`go vet ./...`、Lua5.1必需的addon/协议整包通过，diff空白检查通过。3.2.4完整发行检查已通过，结果见上方。
- 本次修复已随3.2.4发布并安装到本机7个客户端；尚未运行游戏复验聊天框战斗按键及停用恢复，实机待办保留在 `todo.md`，不能由离线结果倒填。

## 当前交付：3.2.3 发行

- 用户授权的合入并推送main、版号加一和npm发布已完成：3.2.3沿用latest。按用户此前“npm发布之后，直接安装到本机”的要求，正式发布后也已完成本机受管update。
- 本次已直接提交并推送main，实现提交`3766f9c`；没有本任务PR，另一个Rust PR及保留worktree不属于本轮。`v3.2.3`固定同一提交；主代理完成正式流水线、registry核验、本机更新和正式包复验，当前没有待交接任务。
- release/version.json及npm-channel清单与派生文件均为3.2.3。本地build/vet、Lua5.1必需完整Go套件（2947项pass事件、42项既有可选skip、0fail）、93项Node测试、版本/Skill/导入/发行输入检查全部通过。
- [正式发布流水线](https://github.com/Follen/Lychee-Dev/actions/runs/38762706792)全部成功，包含必需CI、CGO、正式包构建、源码重建、真实LuaLS/Source、Windows隔离安装、摘要门禁、OIDC发布及registry重新安装验收；[main CI](https://github.com/Follen/Lychee-Dev/actions/runs/38762648214)也全部成功。独立registry回读确认`latest=3.2.3`、`next=3.1.2`，integrity和provenance元数据存在，不声称完成provenance密码学验证。[GitHub Release](https://github.com/Follen/Lychee-Dev/releases/tag/v3.2.3)六项产物齐全。
- 本机受管update返回`complete=true`，CLI、1份Skill和7个客户端addon均为`current`。PATH实际运行3.2.3、commit=`3766f9c`、workspaceDirty=false。Skill需新Agent上下文，游戏内addon需后续reload；本轮文件安装没有新增Live连接或游戏输入，不计入新的七产品实机覆盖。
- 已安装正式CLI按原pin复验：35-ID SQL返回35个唯一ID、无缺失，8组克隆名一致；spell info441776返回“致命一击”，元数据预算经3次扩容从128到320 MiB。缺密钥分区仍为partial/complete=false。SQL capture为`CAP-513f9257a9e0de402211d45b8ae37ef156881045cfe5ef7589a3dd8f8a198737`，spell info为`CAP-462699f3c27acb457edf99f86b51ea568606d829b7f41c29060ac9f3e13cd49f`。已清理本轮`.tmp/release-3.2.3`，CLI captures保留；本次发行和本机更新全部完成。

## 当前任务：SpellName 查询元数据预算

- 用户报告正式3.2.2对Retail CN zhCN 12.1.5.70391的SpellName查询报`records.query_resource_budget: metadata_bytes`，随后确认采用动态预算。沿用`main`（基线`f298633`），没有新PR。主代理完成预算/入口/索引/回归/文档及固定数据验证；pr13_review_data完成SQL IN优化，pr13_review_delivery完成限定只读复核，当前无待交接文件负责人。
- 原触发点：source/定义准备计费97259207字节，SpellName索引再需79935940字节，超出128 MiB共享预算。紧凑ID索引让复制记录共享物理位置，索引计费降为37247372字节；WDC2隐式别名计费已补齐。SQL数值IN复用Source.Equal，避免35项列表整表逐行比较耗尽执行预算；两项均有先红后绿回归。
- 固定384 MiB临时方案已替换：Windows在获得slot后按可用物理/提交/虚拟地址余量最小值的一半、除以并发容量，确定本查询天花板；初始retained/scratch512 MiB及metadata128 MiB按低内存缩减，随后按需扩容并重新查询内存余量。七处查询入口共用同一预算，去除导航和SQL提前固定截断；显式静态预算及非Windows开发保持原固定默认值。SQL/导航增加memoryBudget报告，失败保留维度及用量；网络/工作量、单文件、表头和SQL执行器限制独立。
- 动态版本实际复验：固定pin原SQL返回35/35唯一ID，8组克隆名一致；SQL未需扩容，spell info441776元数据计费285567127字节、经3次扩容从128到320 MiB。本地441776和CDN JSONL begin/record/end、exit0均成功。SQL capture为`CAP-b8bfc33c69e405d45c366b15a4a7426137cfda44f8c1b1113a44c36199d635eb`，spell info为`CAP-05b016882eafdd457961201091310e5beb5f7fb149657660c25555864bab230b`；加密分区仍partial/complete=false，本批全部ID有名称。此前固定方案的CDN441144也已成功。
- 实现阶段build/vet、Lua5.1必需的records/...、vault和command整包、Skill合同及生成命令检查通过。新增回归覆盖大表/scratch扩容、低内存、并发分额、计费原子性、OS读取失败、原生Windows读取和嵌套lease所有权；动态实现限定复核无可操作问题。完整Go/Lua套件和3.2.3交付进度以上方发行状态为准；`.tmp/dynamic-budget`和旧`.tmp/spellname-budget`均已清理，CLI captures保留。

## 已完成交付：3.2.2 发行

- 用户已授权提交3.2.2、合并main、发布npm，并在发布后直接安装到本机。沿用`latest`渠道，`next`仍为3.1.2；不改动已有发行产物。
- 当前在`main`；[PR #16](https://github.com/Follen/Lychee-Dev/pull/16)已合并，实现提交`969fe48`，合并提交`0bec60e`。`v3.2.2`已固定该合并提交并推送。主代理负责最终发布及registry核验；此前功能实测和限定范围复核已完成。
- 版本源与发行渠道清单已同步为3.2.2，CLI/addon/npm派生文件已生成。本地build/vet、Lua 5.1必需完整Go套件（2922项pass事件、42项既有可选skip、0fail）、93项Node工具/启动器检查、版本/Skill/发行输入检查通过；PR必需[CI](https://github.com/Follen/Lychee-Dev/actions/runs/38723851897)及分支CI全部通过，远端已准备固定公开输入并验收。
- [正式发布流水线](https://github.com/Follen/Lychee-Dev/actions/runs/38725056854)全部成功，包含必需CI、CGO预检、正式包构建、源码重建比较、真实LuaLS/Source检查、Windows隔离安装、摘要门禁、OIDC发布和registry重新安装验收。独立registry回读确认`latest=3.2.2`、`next=3.1.2`，integrity和provenance元数据存在，不声称完成provenance密码学验证。[GitHub Release](https://github.com/Follen/Lychee-Dev/releases/tag/v3.2.2)六项产物齐全。
- 本机受管`update`返回`complete=true`，CLI、1份Skill及7个客户端addon均为`current`。PATH实际运行3.2.2、commit=`0bec60e`、workspaceDirty=false。Skill需新Agent上下文加载，游戏内addon需后续reload激活；本轮只同步文件，未新增Live连接，不计入新的七产品实机覆盖。已按限定绝对路径清理`.tmp/release-3.2.2`；本次发行、本机更新及临时清理均已完成。

## 当前任务：槽位自动启用与 Mailbox 重载

- 用户要求主插件加载时检查当前角色的500个槽位，启用被禁用的槽位，并通过Mailbox通知CLI重载。保留现有角色、原连接、输入不重放和重载验证合同；不修改其他插件的启用状态。
- 实现基线`6df4390`；实现和Retail实测已完成，已受管部署测试版，发行状态以上方3.2.2交付为准。主代理负责Go连接/重载、文档和最终验证；pr13_review_live已完成Lua启用检查、Mailbox字段及对应回归，pr13_review_delivery已完成Go限定范围只读复核，无可操作发现。
- Lua在现有启动元数据遍历内按角色GUID启用禁用槽，复查并统一保存；成功修改通过`Mailbox.input.slotReloadRequired`请求重载，失败走启动诊断。CLI在bind发布前处理请求，保留原CON、重载请求、业务交易和期限；已有重载的新runtime再次要求启用时可继续重载，submitted/uncertain仍不重发。没有新增轮询或批量加载槽。
- 已确认此次故障来自另一个角色沿用的旧槽位禁用配置；此前成功角色500槽均启用。本次实测前doctor确认所选PID无占用、0预约；本任务没有操作原LycheeGroup项目或其恢复记录。
- 已通过build/vet、正常Windows环境下的channel/addon/protocol整包（Lua 5.1必需，包并发2、测试并发4、包时限20分钟）、submitted/uncertain/missing outcome分别跨重启不重放的定向回归，以及Skill合同和文档本地链接检查。旧测试已按新增绑定前读取调整，保留原输入不重放和两份未决预约的恢复约束。
- 本次实现、离线验证及Retail 12.1.5.70391实测已完成：灵止光／死亡之翼原有17个禁用槽，部署后启动自动启用并保存；新鲜INPUT记录slotReloadRequired=true，CLI在同一CON及原期限内追加一次槽启用重载，新runtime绑定成功。此前另有一次测试版激活重载；两者分别只提交一次。游戏内探针确认500/500启用，角色AddOns.txt禁用数为0、其他插件设置无变化，报告verified/released并返回42。共8次已提交输入交换无重复，7次等待均0消息。CON-f68a01a481d7cd9100d0694ef9644988已正常关闭；最终doctor全绿、无owner、0预约。已清理本次实现和实测临时目录。实测结束时游戏为测试版addon，全局CLI/Skill仍为正式3.2.1；后续发行及本机更新状态以上方3.2.2交付为准。

## 已发布基线：3.2.1

- [PR #15](https://github.com/Follen/Lychee-Dev/pull/15)已按用户授权合并，合并提交为`10b561db`；实现提交`83463a03`，最终PR提交`842823df`的[Windows CI](https://github.com/Follen/Lychee-Dev/actions/runs/38675723476)五项必需作业及汇总全部通过。`v3.2.1`固定合并提交，[正式发布流水线](https://github.com/Follen/Lychee-Dev/actions/runs/38676733974)全部成功；npm `latest=3.2.1`，`next=3.1.2`，已发布3.2.0产物不变。
- 本次范围为Retail 12.1.5/120105兼容与R1215-INPUT-01键盘优先等待，以及3.2.1版本和发布说明。主代理负责整合、文档、全部实机及发行；pr13_review_delivery已完成客户端/安装/包装审查，pr13_review_live已完成输入/恢复审查，两者均无可操作发现，未写文件或运行游戏。
- Retail基线固定Gethe live提交`18c6d677b605b312f3d813d498404631fb941977`（12.1.5.70391）；TOC、ClientGate、客户端基线与500槽声明新增120105并保留120100。安装按产品和构建系列匹配，旧槽通过既有受管路径迁移。
- R1215-INPUT-01已实现：Native持有被动键盘监听，所有键松开且连续500ms无事件后重新核验INPUT；忙时零消息返回原Driver等待，已开始组合键完成抬键，submitted/uncertain只等待原回执。不能保证消除与已开始组合键恰好重叠的误触。

## 已完成验证

- Retail兼容改动的build/vet、受影响Node 16项、版本检查及完整Lua必需Go套件通过（2891测试事件、2625叶节点pass、42skip、0fail）。这次完整运行早于键盘功能加入，不能代表最终3.2.1。
- 3.2.1最终build/vet、Lua 5.1必需完整Go套件通过：2902测试事件、2634叶节点pass、42skip、0fail、无未完成测试，包并发2、测试并发4、包时限20分钟。skip保留helper及本机缺少的固定输入/原始材料/权限；固定公开集成输入另由本次PR及发布CI验证，不倒填本地结果。
- Node工具和启动器92项通过，唯一缺archive的跳过项使用固定LuaLS archive单独补跑通过，共93项实际通过。版本、Skill合同/生成命令、发行源码输入检查通过。键盘改动此前desktop/channel整包及定向race通过，Windows被动监听实测收到按键/松键并满足500ms静默。
- 12.1.5实机受管升级后，125份报告全部verified/released、500个数值往返比较通过；prepare/commit/confirm/release各125次，另2次bind、1次unbind、1次自动容量reload，共504次输入交换各提交一次。跨两个runtime，不称单runtime逐个执行500个物理槽。
- 按用户要求，本机全局CLI通过本地开发包升级并同步受管Skill和Retail。用户在游戏按住W约4秒，两次held检查均0消息；松开后469ms仍等待静默期，随后完成原请求。四个业务交换各提交一次，报告verified且返回42，原期限不变。
- 两轮实机连接均正常disconnect；当时最终doctor全绿、无owner、0预约。本机已从3.2.0开发包切换到npm正式3.2.1；PATH CLI确认commit=`10b561db`、workspaceDirty=false，受管Skill与Retail addon更新complete=true。Skill供新Agent上下文使用，游戏内addon需下一次reload激活；本次发布后未再操作游戏或新增CON。

## 7项实机结果

以下为此前3.2.0修复在Retail 12.1.0的已完成范围，不代替本轮版本检查或七产品完整安装矩阵：

| ID | 实际检查及限制 |
| --- | --- |
| R3-01 Trace | 真实页面回调错误、自有方法调用与停止；非硬件点击，临时hook已reload清理 |
| R3-06 拾取器 | 用户真实进入/退出战斗；dock、键盘脚本及输入保护关闭，F/Enter/Esc正常 |
| R4-01 输入与恢复 | 完整交易、running原请求resume、28次交换及采样间隔；真实崩溃/缺outcome仍仅离线覆盖 |
| R4-04 bridge状态 | 默认option、off/on、保存off后reload及恢复新runtime连接 |
| R4-07 Objects | 真实页面错误处理、5000字partial/limited、健康对象恢复；临时快照已reload清理 |
| R5-01 数值 | 四种double与%.17g文本往返相等，报告verified/released |
| R20 连接故障 | 公开map固定schema后新runtime空回执及完整交易通过；旧3.1.2原包唯一根因仍未知 |

## 发行与剩余范围

- 3.2.0已由[PR #13](https://github.com/Follen/Lychee-Dev/pull/13)合并后发布，发行标签`v3.2.0`固定`3057a966`；[发布流水线](https://github.com/Follen/Lychee-Dev/actions/runs/38608441597)及[GitHub Release](https://github.com/Follen/Lychee-Dev/releases/tag/v3.2.0)完整成功，`next`仍为3.1.2。
- 3.2.1发布流水线完成合并提交CI、真实LuaLS/Source、源码重建、Windows正式包安装运行、摘要校验、OIDC发布、registry回读和重新安装验证；[GitHub Release](https://github.com/Follen/Lychee-Dev/releases/tag/v3.2.1)六项产物齐全。独立registry回读确认版本、latest渠道、integrity与provenance元数据存在，不声称完成provenance密码学验证。
- 七产品干净首次安装、重新登录及完整升级矩阵仍见[todo.md](todo.md)。上述发行验收结束时没有未关闭的测试连接；后续任务状态以上方当前任务为准。
- 3.2.1实现、审查、合并、发布及本机正式包更新均已完成。发布后的游戏内addon等待用户下次reload，七产品剩余实机范围按todo.md保留。
- 本轮临时目录`.tmp/keyboard-priority/`已按用户要求通过限定绝对路径的脚本删除；清理前已确认实验连接关闭、报告verified/released，无待恢复操作。删除1368个文件，共90338196字节，清理脚本随后移除。用户此前要求留下的其他任务材料、原ZIP、恢复记录及`.worktrees/rust-refactor`继续保留。
