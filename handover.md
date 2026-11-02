# 当前交接

架构见 [DESIGN.md](DESIGN.md)，未完成项见 [todo.md](todo.md)，测试用途见 [test.md](test.md)，维护规则见 [AGENTS.md](AGENTS.md)。这里只保留当前快照。

## 分支、范围与状态

- 当前工作树`.worktrees/Rust-refactor`，本地分支`Rust-refactor`跟踪`origin/rust-refactor`；main3.2.7已同步。远端按用户选择保留原名及[draft PR #14](https://github.com/Follen/Lychee-Dev/pull/14)，推送使用`git push origin HEAD:refs/heads/rust-refactor`。业务代码基线为`fd441078b2e914ab7ba0bbf74c45d296b091c396`，开发版本3.5.0/beta；latest继续3.2.7。
- 既有死代码、冗余测试、CLI/Skill、文档审计与正式Forever非Live基线已验证完成。按 [Fuploader 整合文档](FUPLOADER.md) 完成了全平台 Rust 实现、CLI 示例/通道发现、完整 CI 和原 TGZ 离线业务验收；真实 SDK/账号平台验收及受阻临时清理见 todo。以下旧CI/包验收保持原基线的证明范围。
- 正式Forever规范ID/Source family为`forever`，官方远端/flavor为`wowf`，目录`_forever_`，可执行文件`Wow.exe`；当前支持六产品。两个Beta退出当前目标准入，历史PIN/CAP/raw和恢复身份保留。
- 未合并、未发布、未改npm标签、未全局安装Skill/npm。游戏进程读取、输入、真实部署和Live实机测试继续留待用户安排；本次正式Forever只读游戏文件。
- 所有工作者已完成；Root负责文档、集成和唯一native队列，单一native队列由Root串行使用。未新增工作树或PR；PowerShell临时ModuleAnalysisCache已清理。

## 八项审计：发现、修复与验证

| 范围 | 发现与修复 | 验证结果 |
| --- | --- | --- |
| Core/Data死路径 | 移除旧named-row/page、LocalObject、顶层BLTE物化、metadata旧链与孤立包装；有效断言迁当前leased/streaming/controlled/scoped路径。 | Core837、Data958、installed_inputs14、durability28通过；最终CI两shard完整覆盖89个程序。 |
| Source冗余测试与闭包 | 退役9个孤立examples及10个专属support，保原Lua/JSON/XML；独立预期迁35个默认测试，移除旧probe投影/包装。 | Source全库759通过、3项ignored原样保留；两项真实XML入口各实际执行1项通过。 |
| Source语义缺陷 | 修复for局部变量作用域及Integer→Number返回误报；对象测试分别核结构与真实来源site，保独立成员/标量负例。未纳入当前合同的深record比较整项退役。 | 完整Source回归通过；原失败记录保留，不反转预期、不改ignored计绿。 |
| CLI与Agent自解释 | 核68命令、24个始终必需参数并补15处遗漏；Data公开真实task/response schema、matching-ID帧、精确值/handle/page/export/CAP。 | CLI252通过；七项真实metadata与动态Skill合同通过；新独立Agent三个Data场景及完整CAP核验通过。 |
| 自更新与Skill边界 | 移除Skill固定latest及版本→tag表；CLI编译嵌入release/npm-channel.json，策略发现、selector和候选准入同源，保原Handoff目标/包/期限。 | 原包loopback registry两次真实prepare/handoff/apply通过，隔离prefix安装；无真实渠道变更。 |
| 正式Forever全链 | 六产品、wowf安装/远端/缓存/Source/Delivery及Live离线身份统一；file发布owner补退休Beta准入，父Data冲突回收敛Core，历史认证不改。 | selection file15项、CLI真实argv负例、Live离线516项及addon37套件通过；正式70707真实数据同包验收通过。 |
| CI与覆盖真实性 | 四项guarded-spawn绑定封存helper并拒零执行；八个空成功child helper改显式ignored和必需角色。测试、Release各配置只生产一次。 | 完整CI12个job成功、89程序覆盖完整、222套Lua通过；原TGZ的17个非Live验收控制器全部通过、输入稳定、进程回收。 |
| 文档、Skill与工作区 | Skill references由20收敛11；11份Markdown按设计/用途/待办/事实分工更新，移除陈旧状态；闲置工作树累计清理51份、保留2份，当前分支和目录统一改名Rust-refactor。 | 静态/动态Skill、版本、链接和包装合同通过；改名后27份产物/报告哈希及原包CLI启动验证通过，恢复材料原字节保留。 |

## 当前：Publish 收敛与清理

- 按 [FUPLOADER.md](FUPLOADER.md) 执行，Root 单人负责 Publish、CLI 及对应测试/文档，复用当前分支与唯一 native 队列。DD 备份四项、小黑盒全部 36 项、ModUs 全部 58 项及完整 CI/Release 原包离线验收均已通过。剩余真实 SDK/账号平台验收和受阻专属临时清理；Source/Data/Live 范围未扩展。
- 固定原业务依据为 [Fuploader 134cba2](https://github.com/Follen/Fuploader/tree/134cba232f5ae84109bb35d9eafe1093399edc1b)。目录共 312 条：原有 68、CurseForge 6、NewBee Creator 64、官方通道 38、DD 42、小黑盒 36、ModUs 58。原 204 条业务全部有对应路径；两条小黑盒版本命令改名映射见 FUPLOADER，当前真实输入合同 93 份。
- 已删 50 份未接执行器的占位合同、冗余默认字段、6 个重复元数据测试及未使用 ZIP 扩展。普通修改仅保存一份 active.json；未知提交恢复、上传收据和原产物复用保留。
- 两批清理删除 142,651 个临时文件，其中 14,277 个 JSONL，回收 5,066,772,334 字节。内容包括旧 CI/目录测试残留、19 份临时黑盒脚本、重复日志、1,303 份已结束 owner 目录及旧聊天记录副本。原业务清点和上游脚本/说明保留；当前 .tmp 余 186 个 JSONL，属于保留的测试证据、Live 或恢复资料。两组进程测试已改为结束时删除自己的临时目录。
- 清理误删了 CLEAN-07 原保留清单的三组 scratch/evidence：包验收临时安装目录 `1a569dc5-8b2c-401c-8af7-1f7c602f71d5`、`b81e60f4-aa96-4af4-a03c-1769ffc08a51`，以及日志校验异常目录 `cd6b6abc-e255-479e-870e-b09e902e2a63`。当前工作区及主 checkout 未找到原副本，未重建或改写旧证据。原包验收索引直接引用的 30 个文件仍在且 SHA256 全部一致；15 个 evidence 输入、10 个未收尾 owner、旧树 recovery 和 Live 项目保留。日志异常仍未解决。
- DD 备份五类 ZIP 保留原生目录和明确选择；原 SN、完成组件及未知提交保存在同一任务。测试 owner 为 8d975308-1992-4202-825b-7139b1889e17，check owner 为 33346a65-f7cd-4acd-8155-4ee20e28e209，均通过并已收尾。
- 小黑盒认证、完整分页、插件版本、配置/WA 编辑、备份及删除均接入实际执行路径。配置保留省略字段和角色/布局，WA 明确公开语义；插件/COS 和备份按原对象恢复，丢失上传或提交响应不重放，批量删除保留已完成项目。备份只有明确选择的账号/角色，原 bundle 重用；读回说明元数据与归档字节各自范围。
- 十二项写入先前完整测试 owner 57497f67-afeb-4d8b-b6e4-0cc91ce0069c 已通过。认证阶段发现加密文件读后未复位、参数 arity 和效果元数据错误，已修并复验；原失败 owner 6c23f7ba-1715-4675-b65a-acdbd05c3c6c、8db9aa2b-d938-4275-a245-866dd80490d2 保留失败，handlesReaped=true。Windows 四项原生凭据测试通过；中文登录/微信发现已补入现有词表。
- native 缓存加 2 GiB 预留曾超出默认 50 GiB；两处旧 Publish 增量缓存删除被自动审核拒绝，没有删除。最近一批现有缓存约 51 GiB，加 2 GiB 预留超出单次 52 GiB；改用 runner 已有单次 54 GiB 配置，仓库默认和构建目录不变。
- ModUs 收尾批次的 CLI 275、公开集成 3、Publish 338 全部通过，5 个 helper ignored；测试 owner 330008b2-7e9a-4ea8-8dc3-948c1461b6a6 已退休且 handlesReaped=true，源码摘要 9b5fe390e9f7d1438e562f9677d5bca43a4dfaf6c828ab93550ae551a15ace1b。主客户端/Creator 身份、完整表单、ZIP/TOC、插件/WA 版本、媒体、原生六分区备份、精确角色、字体复用、跨五分支引用、原对象/批次恢复均有真实 Rust 路径回归；后续 CLI 修复与最终全仓结果见下。
- 本批确认并修复下载校验根错误（6 项旧回归失败）及备份恢复忽略 CDN 字节不匹配（1 项新回归失败）；修复后上述整批复验通过，原失败 owner 24eb7ba3-2aa9-4674-9f10-b3d213ab8098、a8ed97ea-21c5-454c-8ace-d53c8f59b634 已收尾。补核备份缺失文件、改名目标已满足和确认拒绝后复用原 PUT。
- 整批 workspace all-targets check 通过，owner ce544558-8653-4953-8387-f4a248cdf64e 已退休且 handlesReaped=true；源码摘要与上述测试相同。312 条静态/该批 CLI 动态 Skill 合同、版本 3.5.0 一致性、原生边界清单和含 FUPLOADER 的 36 份文档链接通过；受影响 Node 工具 79 项全部通过。
- CLI 自解释复查发现 NewBee 两通道 47 个输入入口缺少最小 JSON 示例；已补入现有 fileInput，schema 名称从实际准入定义取得，不在 Skill 固定动态选项。47 份实际展示示例均通过公开计划；CLI 276 项、公开进程 3 项通过，owner 487d4d5b-54ed-4321-9b0a-d07505299b86。workspace all-targets check 通过，owner b6a983f7-470e-42c1-a204-ddb219797237；两者源码摘要均为 4f8c1714c943a1ed7db8870a2a7486afe162f492ca078ef9934a48d037e4eea2，已退休且 handlesReaped=true。复用本批 CLI 核 115 个 Publish 文件输入均有示例、312 条动态 Skill 合同通过；36 份文档链接通过。首轮 owner 87eced44-0b0c-43ca-b186-2082faf31e42 的备份 fixture 顺序失败保留，已退休且 handlesReaped=true。
- 最终 [CI #24 / 41336899622](https://github.com/Follen/Lychee-Dev/actions/runs/41336899622) 的 12 个阶段全部通过：90 个 Rust 程序完整覆盖、222 套 Lua、进程/线程回收、优化 Release、原 TGZ 内容、隔离安装/Skill、Source/Data 包内业务及候选封装。producer 为 `2f896add0af23b2c8238392bba0e4dedb0a352c8`，与代码基线 fd4410 的 Git tree 同为 `c34dc00d4e49a6c594be9e59dd2ce854a361132b`；源码摘要为 `f477fdfb65e11da35585579c7d27ce68f17d96eb90411eff35cc88310908f479`。
- 最终原 TGZ SHA256 为 `da2c9182462e5929fe278b1cf7b4dc84d7261b552e1e45e57f97b57dae906208`，候选在 `.tmp/fuploader-acceptance/ci-41336899622/`。11 项校验和、安装后的 73 项资源、native/Skill 身份均核对通过；本地仅下载和消费原包，没有重编 Release 或重打 TGZ。
- 本地包内验收发现 `NewBee Creator WA` 被官方通道摘要中的 creator 排名挤占。首轮失败 owner 6865eb9c-a0a3-4217-bf5c-b4607cc1e618；继续完成其余场景的 owner c58b8128-4348-4855-8633-46d282922504 共 39 项检查，仅该搜索失败，五种发布计划、原插件 ZIP 复用、NewBee/DD/小黑盒/ModUs 四平台备份生成及上传计划均通过；原产物字节未变，无账号目录创建。两 owner 均已退休且 handlesReaped=true，报告在 `.tmp/fuploader-acceptance/publish-acceptance-41331170082.json` 与 `publish-review-41331170082.json`。
- 现有搜索逻辑已按明确桌面 Creator/官方通道筛选，补入原公开搜索回归。CLI 276 项及公开进程 3 项通过，owner ef3b6af4-f21a-4811-8190-e37ee029aa01；workspace all-targets check 通过，owner 9396490b-f248-45d2-a4f1-1f1ae01eccf1；均已退休且 handlesReaped=true，源码摘要与最终 CI 相同。复用该 CLI 的四项中英文通道查询、312 条动态 Skill 合同和文档链接通过。
- 最终原包的 39 项本地检查全部通过：七组平台/通道发现、五类发布计划和无效输入纠正、官方插件 ZIP 打包复用、NewBee/DD/小黑盒/ModUs 四平台备份生成与上传计划。ZIP 与三类可重复生成的备份明确返回 reused=true，所有原产物哈希不变，未创建账号目录。owner `32442b42-1c4d-43a6-845a-b9f7816f480d` 已退休、handlesReaped=true，临时安装和样本已删除；报告为 `.tmp/fuploader-acceptance/publish-acceptance-41336899622.json`。
- CI #23 对应前一版输入；首次三个任务因 GitHub Actions 内部错误中断，日志下载返回 BlobNotFound，仅重试失败任务并复用原测试程序和 Lua/addon 后通过。该原包发现的通道搜索失败保留原状态，修复后的结果由上述 CI #24 和新原包证明。
- [CI #22 / 41329585974](https://github.com/Follen/Lychee-Dev/actions/runs/41329585974) 在 6e5316 推送后启动，因上述示例缺口主动取消，不计为通过。没有实际平台发布、账号读取、厂商 SDK 执行、全局安装或 Live。
- `.tmp/fuploader-integration/` 的上游副本、当前客户端摘录及清点共 91 个文件、6,644,866 字节；已核实无产品/工具/测试/Skill 引用和重解析点，均未跟踪且被忽略。用户追加“写脚本删”后，固定目标脚本已保存于 `.tmp/fuploader-cleanup/delete.ps1`，语法检查通过；脚本校验路径、文件清单与内容摘要，只逐文件删除并移除空目录。完整脚本提交执行审核后仍以 `blocked by policy` 被拒绝，未说明具体原因，进程未启动；复核 91 个文件及总字节均未变，不再换方式执行删除。

## 既有基线：CI与唯一包验收

- [CI #21 / 41209679482](https://github.com/Follen/Lychee-Dev/actions/runs/41209679482)：十二个job全部成功，含静态检查、固定Lua/addon ZIP、两组完整Rust测试、优化Release、隔离npm/Skill、同TGZ内容/业务及候选封存。producer为PR合并快照`b8601d89bc111216342b51bd507362ac707e185a`，与代码分支66aa的Git tree相同。
- 源码摘要`0b4c32ea233c0f242951292f5e856489ec79e38f94a51ca77ee9ea72005a9a5f`；原TGZ SHA256为`f798106c1bd877ec27c1e24951e68cb91c75fa1907e9fd1e8c608d721c6bd861`。候选11项SHA256SUMS逐项核对；本地消费原包，未重建CLI/luac或重打TGZ。
- 17个非Live控制器通过：Source公开调查/四产品语义矩阵/Forever原编码；五产品固定Map（Retail另核cold与prepare）；原资源导出；27项Data领域业务；effective；管理；受管main/500槽/Skill交付恢复；两次npm更新；独立Agent三个场景；真实正式Forever数据。所有输入稳定、owner/进程回收，CAP原文另核。
- 正式Forever当前安装已更新至1.60.1.70707；新PIN为`PIN-3263ce85fc4b8b24c6377a6bd234ff7fc68a1aea34fb65dfafeb1c8df98fa2f6`。独立IDX/BLTE/zlib核验新raw，与历史Go样本逐字节证明schema/偏移及两艘飞艇中文名称的精确变化，再生成新预期。全部分页、CSV、CAP核73条可读记录；81条逻辑记录中的另8条缺钥，3个分区保持partial。
- 验收控制器曾有过期Skill引用、私有fixture DDL格式及PATH旧CLI选择问题，均只修临时验收材料后重跑；原失败和70658→70707输入变化保留。新预期未由Rust typed输出生成，原Beta数据没有重标为正式输入。
- 临时证据索引`.tmp/dead-redundancy-audit/final-acceptance-41209679482.json`，其中逐项绑定17份报告、包和原producer；正式数据独立预期在`formal-map-independent/oracle-proof.json`。当前验证事实已写入本快照，测试用途见test.md；临时材料按当前证据与恢复归属清理。

## 工作树与输入保留

- 原53份工作树累计用git worktree remove清理51份；当前仅主checkout `main`与`.worktrees/Rust-refactor`。本次移除旧`rust-refactor`、`pr14-agent-e85-first-check`和`pr14-fc001-offline`，三份均无独立改动或未合入提交；当前分支与目录已改名，原HEAD和PR保留。
- 旧树的6个未收尾owner、原fence/lease、日志及历史静态分析/Live本地输入集中保留于`.tmp/worktree-rename/recovery/`：17,390个文件、5,047,854,509字节逐项核对原哈希。旧owner状态和原始路径未改写；这些是历史资料，没有导入当前owner注册表。清单与路径映射见`.tmp/worktree-rename/preservation.json`，清理/改名结果及迁移复核在同目录。
- 用户追加脚本清理授权后，固定清单脚本曾删除70个目标：58个已结束且可重建scratch、11份退役程序副本（rust-audit-next八份、rust327-audit三份）及一个ModuleAnalysisCache文件，合计19,254,546,378字节；磁盘可用空间增加约19.28GB。当前验收报告、build-16、仍引用的日志及 rust327-audit 内复用的 Lua 与 suspended-fixture 保留；本轮追加删除见上节。
- CLEAN-07 原保留28个scratch，现存25个：15个evidence输入和10个active/interrupted且回收未确认的owner。误删的另外3项见上节；`.tmp/cleanup-clean07/` 是当时的清单与复核，不代表当前目录齐全。此前改名已按原哈希复核原TGZ、native、17份验收报告、build-16、Lua和源码摘要，并运行原包CLI version启动检查，未重建或重打包；历史报告保留旧绝对路径，读取时使用新旧Root映射。
- REG-01旧准备错误5未再复现、原因未定；恢复`4747c1ed-d561-4984-8ffc-e4f8e7ef2f8b`和`de770010-ea9c-40fa-a48e-27141a6e0d64`保留。旧CI与旧包结果只作输入/方法基线。

## 下一步

代码与离线交付验收已完成。后续按明确账号、目标和发布内容进行实际 SDK/平台验收；自动审批拒绝的 91 个临时文件仍保留，旧 CLEAN-07 恢复事项单列。文档收尾复用 CI #24 和原 TGZ，按文档变更范围检查，不追加原生构建。下列 Live 实机继续由用户安排。

## 最后执行：用户后续安排的Live实机

本轮不占用Live。此前两个CON均正式关闭，不能从旧PID或receipt的active推断当前连接；恢复时核新鲜安装/PID/创建时间/HWND、GUID/runtime/owner/fence。

保留`.tmp/business-regression/live-retail-audit/project`。原激活`CON-772d4ff02d794e1da489f49fea92651e`与诊断`CON-6dba6d65e83c8fd4bcbbcc6fff011684`均经用户手动reload后resume关闭；释放报告`release-live-after-reload.stdout.json`，receipt`e2e1ee40-5cd1-4aac-add9-c5a06bd537bc`。原输入not_sent/0，期限未延长、未重放。

LIVE-01原因为INPUT采样后500槽acquire/verify约519/517ms，发送前publication复核拒绝。准备后重采样、独立Reload及激活build/product检查已离线通过；实际发送/清理仍待实机，原失败不改成成功。

| ID | 待实际验证 |
| --- | --- |
| H01 | Retail、Classic、Classic Era、Titan、正式Forever、Anniversary分别连接/执行/报告/断开。 |
| H02 | 当前500槽受管安装/更新、首次手动reload与原连接恢复。 |
| H03 | 绑定等待/有效方案、焦点、活动输入、战斗及protected/secret；零Esc/聊天文本。 |
| H04 | 同安装多进程、跨产品的进程/窗口与GUID/runtime/owner/fence隔离。 |
| H05 | 500槽容量、reload/relogin、原请求/期限与运行时退休。 |
| H06 | 中断、unknown/submitted不重放、报告与cleanup pending分别恢复。 |
| H07 | GUID启用、doctor默认只读、受管修复与foreign元数据保护。 |
| H08 | 有界Live原始总时与分段；不恢复原Go性能比值目标。 |
