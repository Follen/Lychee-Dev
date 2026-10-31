# 当前交接

架构见 [DESIGN.md](DESIGN.md)，剩余事项见 [todo.md](todo.md)，测试用途见 [test.md](test.md)，工作规则见 [AGENTS.md](AGENTS.md)。这里只保存当前状态。

## 目标、分支与授权范围

- 2026-10-31用户要求循环审计八项：架构/模块边界、业务完整性、Skill-CLI一致性、有效覆盖、死代码、全仓集成、Rust原生度、冗余测试。每轮修复并验证确认问题后继续复审，直到每项不再发现新问题。当前从已验证HEAD 210d3f934d27f12171503d983060920ab4e5da13继续；先独立深审，再集中修改和受影响验证。前批已通过结果在路径未变时继续有效，不能充当本批新改动通过。Live实机仍留后，不连接/读取游戏进程、发送输入、更改真实安装或全局Skill。
- 工作区`.worktrees/pr14-local-dev`，分支`rust-refactor`，既有[draft PR #14](https://github.com/Follen/Lychee-Dev/pull/14)。产品提交`d6a3a8dd293cdbb117c703cb903786f18c023d01`，main历史整合提交`1e7b1dd98e5d19fbf01327a05bd138aa7954f92f`，均已推送。main为`cbf96df6465c661e2e5a21b0c972d6bd76beab83`；先移植行为再接入历史，整合前后Rust tree均为`8fae72bf0dc39d2b1c3cd84757f723311cada0ec`。Rust版本仍以release/version.json的3.5.0为准。
- 当前PR可合并但仍draft，未合并、未发布、未修改远程npm标签；preserveLatest仅随main更新为3.2.7。已有提交/推送/PR维护/完整CI与正式包验收授权已执行；不新增实机或发布授权。性能已接受，不补优化或比较轮次。
- 用户追加再做8轮，每轮覆盖原八项；发现问题修复后复审，末轮仍有新问题就继续。非Live业务须跑实际任务与结果核对，可安排子代理模拟使用者；复用固定输入和最终同一CI包，不按轮数重复编译。Live范围保持不变。
- 当前分工：rust_business_audit负责Data/Core清理与Source native_session/analysis_process、CLI Data进程消费者；go326_doctor负责Windows（不含broker/destination）、Delivery/Live/formal_live及Core storage；go326_delivery负责CLI/Skill、Source/Emmy清理和Windows destination/Data assets旧发布路线及相应测试迁移；协调者独占Windows broker、Source batch/assets/index、Core process测试helper、tools/CI和共享文档。所有原生构建由协调者串行执行。

## 当前补审状态

- 各模块追加八轮阅读已完成，当前修复、实际验证和交叉复审仍在进行，尚未达到八方向均无新问题。所有改动未提交，不能用前批完整CI证明本批通过。
- 最近成功的第三批fixed构建产出93程序+1support，源码12b6e5366da4500ed8ebb1e88afe46205175a8252ae2088dfb4362b3dbc8296b，receipt 4d6e3aa2-1042-4eff-8d26-b54c0c9b3f76。18组均handlesReaped；Core842通过/1辅助ignored、Data920通过/1ignored、Source715通过/3新增红/5ignored、Live离线521通过/2新增红/2ignored、Delivery182通过/1新增红/1ignored、CLI227通过/6新增红；Emmy所有19程序与destination71单元+6集成通过。Source公开IPC默认ignored，尚须最终显式执行，未计通过。
- 第三批真实红已实施修复待第四批绿：CLI显式jsonl/Hotfix project锁/discovery合同；PNG首块IHDR；Source缓存miss保留清理与退休错误；npm结果与原error.details；input_observed保存双错；broker、detached、Job及KeyboardActivity的checked-close和原消费者收尾。broker transfer另有一项fixture时序错误，已用实际accept/receive同步修正，未冒充产品缺陷。
- 退役无消费者Live runtime_observation及衍生描述/非正式lookup、旧destination发布路线、Source/Emmy死包装和重复镜像断言；正式Mailbox路径、所有权检查、真实独立业务预期保留。第四批首次编译发现误删相邻的transient_publication，已恢复原正式毒化判定所需函数，失败owner已retired/handlesReaped。
- 第四批fixed all-targets已成功，源码a33740af509e00beef23441731674e8dad6ddf176c8787700b255fdd308e4748，owner receipt 8782dcf5-ec75-40e4-aadd-dde0c5d84b28，6m38s。21组全部handlesReaped；CLI233、Source research82/核心105/assets4、Windows broker11/detached3+2/Job1/event3/guarded4、npm12、Data generation45/effective12/content8、Emmy19程序、Data supervisor7均通过；各helper/外部ignored另计。实际新红：Hello取消/期限2项、observer吞错1项、NativeReader额度丢child取消1项；Source会话其余75和Live离线其余517通过。现只解冻这些原边界修复及Core私有first_only死参数清理。
- 修后交叉复审又确认doctor普通主addon升级漏回传实际archive、Delivery phase=complete后清理错误丢原Receipt；两份最小独立红测试待下一次统一构建，产品未修。Delivery/CLI/Skill/CI集成复审未确认其他新增缺陷；静态无发现不计业务通过。最终Release的非Live实际任务计划正在复用既有Source/Data固定输入准备，尚未运行。
- 第五批all-targets成功（源码f31db2d0009d710437fb866558f38d785650359068262adc4e38b21993f74ec3，receipt bec00e94-2049-457a-b1bb-f31702b78b7e，5m33s）。8组全部回收；Core842/1ignored、Data920/1ignored、Source726/5ignored、CLI237、NativeReader8全绿。原control的Source环境/diff读取、sync PIN发布、Live snapshot离线preflight和Core默认API共用controlled生产入口已验证。新增默认update发现取消、doctor归档、formal完成Receipt三项均真实红，前两项正常对照及formal真实main+500slots成立。按Git LF blob消除224份换行漂移，未改vendor原CRLF或固定-text输入。
- 三项产品修复已实际绿：第六fixed构建93+1，源码b245b93371ca347614ab77deac758bdf793664d9b3bd9bbd2bf970c322945224，receipt e2f64a3b-8d1b-4cb7-89f1-ce4b962ad848，76s。formal25、update26/1辅助ignored、doctor repair8、显式Source公开IPC1全绿并回收。IPC用本次预编译开发CLI和既有认证luac资源，实际会话复用/快照切换/故障恢复/关闭/原60s idle退出109s通过；不是最终安装包Release验收。此前消费Snapshot后的借用编译错已修，失败owner已回收。
- Source的ChildError/observer组合已实际红绿修复。第七raw组14绿/1红证实external.io/5覆盖child_business/4；第八93+1构建成功（源码4abbaa2c65d32ba8dc630917b727ed9b8f9b579b483c52ea962484d9b743c353，receipt b90c3d99-271b-4513-9d98-aa46efce1d5f，157s），Source会话84和显式公开IPC1全绿，全部回收。修复保借用有界原wire、Core cleanup主因/观察错误、退休事实；真实panic仍fatal退出原owner，控制/write错按原序收尾；单错精确字节、原frame地址/容量、宽数/默认栈/32MiB帧上限均有独立回归。最新交叉静态复审八方向未发现新增确认问题；回复准入前control拒绝的相邻候选按当前合同否决，不改优先级。
- 最终非Live实测须使用同一CI Release/TGZ和既有独立固定预期，覆盖完整业务任务；CLI自解释另用独立代理只读包内Skill/help/search/describe/schema完成任务，记录命令选择、必填项、partial和错误恢复是否清楚。当前计划不是验收结果，也不能用编译期debug CLI的集成目标冒充安装包Release实测。
- 临时实际任务控制器已准备：Data包含12种navigate、17独立结果、query/Page/CSV/CAP/partial恢复、固定Hotfix与AVI；Delivery包含真实隔离文件安装/恢复，以及loopback registry仅提供同一TGZ的真实npm prepare→handoff→apply。installed-only任务只认证包，不误依赖下载test record的原CI绝对路径；正式CI gate未放宽。新增cli_blackbox_usability代理仅读Skill形成7组独立任务，尚无CLI实测；固定输入与脚本语法检查不计业务通过。当前文档47份链接和实际unsafe清单通过。
- CI seal的manifest哈希/字节/完整清单/渠道、复制后字节、当前完整Lua矩阵和addon/Skill实际报告校验已红绿验证。安装与候选共用正式Windows/Rust/version/commit/dirty身份校验，拒绝同版本旧包；最近受影响Node36项通过，完整Node308和workflow21此前通过，unsafe实际清单通过。未修改真实标签或发布。

## 前批实现与审计结果

| 范围 | 最终行为 |
| --- | --- |
| main3.2.7绑定 | ReceiverBindings归一化内容与main一致：先核有效方案，等待加载/脱战，重读玩家修改，停止同步事件重入，保存原方案，手动重置清旧诊断。旧protocol独立场景已并入当前addon套件。 |
| Data领域 | 法术EffectBasePointsF/Speed/RangeMin/Max保留原浮点/数组；DecorGet与CreatureDisplay按文件两阶段的partial空选择报unavailable，完整空来源仍not_found。完整法术闭包、光环/召唤、遭遇树、生物显示/资源与自动CAP用独立预期回归。 |
| Data内核 | 删除SQL-only planner胶水和旧SQL测试；精确SUM/AVG、负零、除零、双分区聚合、Decimal schema、HashJoin、COUNT边界迁到typed生产入口。关闭sql feature；datafusion-sql仍是固定上游必选依赖，不宣称整个依赖消失。 |
| Agent使用面 | requestSchema完整公开十五种wire请求、字段/默认/限制/示例与恢复含义，复用原查询/领域合同。Catalog/Close改为空struct变体，复用serde拒绝未知字段。doctor无效no-cache参数和兼容说明移除。 |
| Delivery/Core | status共用既有受控、有界、checked-close库存检查器，保留原control及I/O/清理错误。Core根进程挂起入Job后恢复，按原control等同Job自然清空并join pipe workers；失败只用原5秒资源收尾窗口，保留primary/cleanup，未确认回收不报成功。取消与PSS清理复合错误保留原错误码。 |
| 死代码/冗余 | doctor测试改走正式report/close helper，无消费者包装删除。镜像/重复断言和旧Lua builder删除；字符隔离、INPUT、非空History迁入当前addon。bridge证明测试实际读取memory/numeric两份证据，逐字段负例到达对应校验。CLI数值目标改名data_agent_public_numbers。 |
| CI | 每配置一次构建、固定资源和唯一TGZ传递保持；两个文件symlink用例在各自Windows runner显式执行，同一预编译程序/record，无重编或跳过。 |

## 当前完整验证与产物

- [完整CI 41114329228](https://github.com/Follen/Lychee-Dev/actions/runs/41114329228)已completed/success，十二个job全部success：静态、固定资源、Lua/ZIP、测试构建、Release构建、两组Rust、唯一TGZ生产、隔离安装/业务、独立内容审计、候选封存及ci-required。
- 实际验收checkout为PR合成提交`b13e2fe6dbec312b0386f9405b93f3b3832e68e7`，源码摘要`b5ccdb4b7ceabaa540e1250e37f3e265d4afcda7da37ac2a5ba51a5078230bf8`。测试产出93程序+1 support，两个组24/69全通过；汇总和独立下载核对确认93个程序恰好一次、无缺失/重叠、身份与回收成立，record SHA256为`aed4308412c7cdecc6d1f964ed53bef0a787cb3b97f64409fbb5de21b1b4abae`。
- 37套addon×六种配置共222项全部通过；Node工具300项和workflow合同21项通过。Data字段/schema缺陷先实际复现再修复；最终完整CI覆盖全部当前程序。Source库712、Core844、Delivery175及Live离线516均通过；ignored辅助/环境项目不冒充通过。两个文件symlink在CI各实际运行1项并通过，原本机1314覆盖缺口已闭合。Windows另两项sealed helper本地通过，helper来自同次Cargo输出，没有另编。
- [当前候选](https://github.com/Follen/Lychee-Dev/actions/runs/41114329228/artifacts/13007625875)已独立下载核验：ZIP SHA256 `c80a17fc9f83f98febe0e08e8a7e3f1c9f14944239bfe52386633dfad9172d91`，全部11项SHA256SUMS成立；候选、安装、审计、包内业务报告绑定同一TGZ及CLI、原提交/源码/record，未改写身份。
- TGZ SHA256：`9a0ed9aec011a2901206f82af0ea26fd776e4cd340f06ac06db118b750206f3f`。Release CLI SHA256：`0e384b8c317f2e181a9fc73cc5707ea6e58411243a89a40954da07203117b2ea`。正式TGZ隔离安装、Skill launcher、内容审计、已安装Release的固定Source native与Data query/Page/CAP/release/close均通过，inputsStable/handlesReaped成立。此为固定最小包业务，不扩大为所有数据或游戏行为。
- 测试/Release配置各生产一次；Lua供应包原字节、addon ZIP、许可证closure、原receipt/日志/timings和唯一TGZ均通过明确本轮产物复用。依赖下载缓存命中；本次依赖合同改变后两配置新key首次构建并已保存缓存。只记录结果的文档提交跳过CI，避免再次构建；候选继续保留上述实际验收提交。

## 保留事实与后续

- 未改路径的既有完整Source/Emmy、四产品XML/源码矩阵、已安装Retail/Forever调查及七产品Map/资源业务仍有效。两个Forever beta各自73行可读、8行不可用，保持独立PIN/partial。原证据仍在`.tmp/business-regression`与`.tmp/rust-disk/evidence`；本轮不补性能轮次。
- REG-01：此前Delivery升级准备曾报Windows错误5，原失败记录`default-report.json`和fixture保留；同程序exact、完整事务组、当前完整库及CI均通过，原因未定，不增加重试或削弱断言。
- 活动项目/CAP、外部固定参考、未解决恢复记录`4747c1ed-d561-4984-8ffc-e4f8e7ef2f8b`/`de770010-ea9c-40fa-a48e-27141a6e0d64`、明确保留的`.worktrees/rust-refactor`均保留。此前十三个已解决scratch删除遭自动审批拒绝，未重试绕过。本轮`.tmp/rust327-audit`已检查为465文件/4,950,653,420字节、无reparse/活动项目，全部构建receipt retired且handlesReaped；删除同样被自动审批返回`blocked by policy`，已保留，不换路径或工具绕过。
- 前批非Live业务与审计已完成。本批下一步：完成当前资源与旧publisher处理、统一新红/旧绿，再推进追加八轮，修复后执行受影响验证；最终集中提交推送、跑完整CI及同一Release/TGZ验收。实机仍按最后一节留后。

## 最后执行：用户后续安排的Live实机

本轮没有占用Live。此前本任务两个CON均已正式关闭；不能从旧进程身份或receipt的active字样推断现在连接。恢复前使用当前候选与新鲜安装/PID/创建时间/HWND、GUID/runtime/owner/fence重新确认。

保留项目`.tmp/business-regression/live-retail-audit/project`。原激活`CON-772d4ff02d794e1da489f49fea92651e`和诊断`CON-6dba6d65e83c8fd4bcbbcc6fff011684`均经用户手动reload后正式resume关闭；最后释放报告`release-live-after-reload.stdout.json`，receipt `e2e1ee40-5cd1-4aac-add9-c5a06bd537bc`。原输入not_sent/0，原期限未延长、未重放。

LIVE-01原因为INPUT采样后500槽acquire/verify约519/517ms，发送前publication复核拒绝。Invoke准备后重新采样、独立Reload及激活build/product检查已离线通过；实际发送和清理仍留待实机。原失败/分段记录保留，不改写成业务成功。

| ID | 待实际验证 |
| --- | --- |
| H01 | Retail、Classic、Classic Era、Titan、两个Forever beta、Anniversary各自连接/执行/报告/断开。 |
| H02 | 当前500槽受管安装/更新、首次手动reload与原连接恢复。 |
| H03 | 3.2.7绑定等待/有效方案、焦点、活动输入、战斗及protected/secret边界；零Esc/聊天文本。 |
| H04 | 同安装多进程、跨产品的进程/窗口及GUID/runtime/owner/fence隔离。 |
| H05 | 500槽容量、reload/relogin、原请求/期限与运行时退休。 |
| H06 | 中断、unknown/submitted不重放、可用报告和cleanup pending分别恢复。 |
| H07 | 角色GUID启用、doctor默认只读、受管修复与foreign元数据保护。 |
| H08 | 确需实机的有界Live耗时；同候选/输入记录原始总时和分段，不恢复原Go比值目标。 |
