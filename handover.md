# 当前交接

架构见 [DESIGN.md](DESIGN.md)，未完成项见 [todo.md](todo.md)，测试用途见 [test.md](test.md)，工作规则见 [AGENTS.md](AGENTS.md)。这里只维护当前状态。

## 目标、分支与范围

- 用户要求追加八轮全仓审计，每轮覆盖架构、业务、CLI/Skill、有效覆盖、死代码、全仓集成、Rust原生度与冗余测试；确认问题修复并验证后继续复审，直到各项不再发现新问题。非Live业务须用最终正式包完成实际任务，独立Agent仅靠安装Skill与CLI发现能力验证可用性。性能已接受，不补优化或比较轮次。
- 工作区`.worktrees/pr14-local-dev`，分支`rust-refactor`，既有[draft PR #14](https://github.com/Follen/Lychee-Dev/pull/14)。主修复`5a5e7a7fcd5ef41e848c317f29f4804dda2fc6f1`及两项PNG正例夹具修正`ff65b19779c63196a8fb8a65b00a6f7c778a5dae`均已集中提交推送。main3.2.7的行为与历史已接入；当前main为`cbf96df6465c661e2e5a21b0c972d6bd76beab83`。
- Rust版本以release/version.json的3.5.0为准，beta/preserveLatest3.2.7未变。已有提交/推送、PR维护、完整CI和正式包验收授权继续有效；PR仍draft，未合并、未发布、未变npm标签。禁止游戏进程查询/Live连接或输入、真实安装写入和全局Skill安装；自有合成客户端、进程/pipe/event及隔离npm前缀可用于离线验收。
- 各模块追加八轮阅读、修复及最终静态交叉复审完成，未发现新增确认产品问题。最终同包非Live业务已全部验收，AUDIT-08完成。协调者已核原始结果及独立预期，所有原生owner均回收，所有子代理STOPEDIT；结果文档与PR按当前实际状态维护；清理受阻的临时副本单独保留。

## 本批修复

| 方向 | 修复与保留边界 |
| --- | --- |
| 架构/模块 | Source会话与Core默认入口共用原control/checked-close；child、observer、retirement原因按Core同一结构保留，原fatal仍退出。 |
| 业务完整性 | Semantics只许static/空captures；partial Aura不生成否定事实；Source/Hotfix项目、最近锁与明确snapshot优先一致；doctor和正式安装保实际归档、原完成Receipt；npm完成journal故障保已完成事实。 |
| CLI/Skill | batch/matrix使用严格typed codec与完整文件schema/例子；命令路径、实际envelope字段和显式AgentJSONL一致；独立Agent实际任务见下。 |
| 有效覆盖 | 原Hello取消/期限、observer读错与复合错误、真实deny-delete/checked-close进入正式入口；离线NativeReader同ledger保配额与独立child取消。两个PNG正例补首chunk长度13/IHDR，保24bytes及原123×456断言。 |
| 死代码 | 退役旧Live observation、destination executor、Source/Emmy无消费者包装及重复JSON codec；实际消费者、RAII和受支持旧客户端格式保留。 |
| 全仓集成 | CI校验完整字节/集合/渠道/原copy关系，Lua37×6报告与Skill是正式gate；同一原TGZ用于审计、隔离安装、业务和候选，不重写producer身份。 |
| Rust原生度 | Windows Process/pipe/event/Job/线程操作逐项checked-close；npm子命令原structured details不丢失；Source默认fixture入口不保留第二条不受控生产路径。 |
| 冗余测试 | 删除镜像断言、空helper和旧执行器测试，独立业务预期迁入正式路径；测试、Release各编译一次，后续消费者只使用认证产物。 |

详细用途及证明范围见 [test.md](test.md) 和相应可复用回归；表中实现不等于实机通过。

## 当前完整CI与唯一正式包

- [CI 41182067569](https://github.com/Follen/Lychee-Dev/actions/runs/41182067569)十二个job全部success，含static、固定工具/ZIP、Lua、两配置构建、两组Rust、Release组装、Skill/隔离安装、TGZ内容审计、候选封存和ci-required。前次41179518527仅两项旧PNG正例夹具失败，已修复，不将旧run标通过。
- 实际验收checkout `a2e95cbd63fb7696d2adf868e98eaaf2e0a7b5a9`，与ff65b19779同Git tree `abc26153b99893db7a93d45b664b36c33fa81717`；源码摘要`5e7159bdeb4474570bca74ca699257c716fbbc3955aabb7f277bf6e7459ba548`。93程序+1support，24/69两组全部通过且逐程序无缺失/重复、哈希/身份与回收成立；record SHA256 `b4d97aba454bbf6aa406db2d0db153b4fb302a3776ad14f4814ec7eaced192cb`。两个文件symlink在CI各显式运行1项通过。Node309/workflow21、37套Lua×6配置共222项均通过；ignored和helper不冒充业务通过。
- [候选产物](https://github.com/Follen/Lychee-Dev/actions/runs/41182067569/artifacts/13035850632)已独立下载，ZIP SHA256 `e5c867ac1fb9f79bb68aacec9d91a3cf86fbd9fe4c41819936fe44293e603772`，全部11项SHA256SUMS及报告共同身份核对通过；证明`.tmp/rust-audit-next/ci-41182067569/independent-candidate-proof.json`。
- 唯一TGZ SHA256 `3e2286a7b4c532e537a5b1f11645829cb7c84a889335a9950dad1231b6d9724a`；Release CLI SHA256 `2793e8acf61111fd296843b1f8bbeaaf307100d4e1c652fece31363bcdf7186f`；luac SHA256 `5aa1605bfea56e6c2961c6b4b0f845a699b43f9b3666dd64f50289c3952d441b`。测试/Release各生产一次，成功配置缓存与Lua/ZIP/许可证closure复用；没有本机重编Release或重打包。
- 本机正式`tools/rust-package-smoke.mjs validate`已通过真实隔离npm/Skill启动器、完整payload与native version/commit/dirty绑定；报告`.tmp/rust-audit-next/ci-41182067569/local-npm/report.json`为partial_passed/npmSmoke passed，扩大业务另计。为原精确commit gate临时detach同一checkout后已回rust-refactor；原producer/record绝对root未被改写。所有后续业务统一使用该安装包。

## 最终包非Live实际业务

- 已通过Data27项业务检查/15会话，含12类navigate/17独立向量、Semantics三类拒绝及恢复、typed query/lookup/batch/related/explain/analyze、同handle分页/CSV/JSON/精确整数/CAP/release/close、三类partial选择器与恢复、原XFTH归档续读、固定Wago HTML离线重解析/游标和AVI导出/限额/冲突。报告`.tmp/rust-audit-next/final-data-41182067569-2/report.json`，inputsStable/handlesReaped均true。
- 独立cli_blackbox_usability代理仅用安装Skill、实际describe/search/schema及用户原输入，未读产品实现/预期/oracle。最终Source+三个Data用户任务及version共5case通过，源码定义/调用位置、注释字符串排除、静态partial、完整物品信息、精确整数分页CSV/CAP、partial缺密钥与同会话恢复均有可用回答；原token保9007199254740993和9223372036854775807。Source关闭、Data close与释放、输入稳定及整个Job回收均已确认。报告`.tmp/rust-disk/evidence/c7fa865e-ada4-432b-bd3c-b457a0cd884a/business-report.json`；代理只读复核与Root私有独立oracle核对均通过（`.tmp/rust-audit-next/blackbox-independent-proof.json`），无遗留实际使用阻碍。
- 前次exact-numbers使用的Root输入有19bytes故意损坏catalog，产品正确拒绝；旧输入/失败记录保留，业务另建与原PIN匹配的有效164bytes目录，新bundle SHA `bc585704e0ddaa7928906192c84e0f17c769b4375fd5dbf1dc3092eeaad8ed6a`。临时runner的CAP API、目录准备、timeout参数与可变manifest/receipt日志问题均在harness修正，不计为产品缺陷或改写既有失败。
- Source公开项目全流程、Forever4481文档原编码/冷暖索引/原字节/luac/native首次与复用、四产品固定API矩阵均通过，含项目/最近锁/明确PIN、真实调用/context/batch、UTF8续读、严格输入、TOC/XML顺序/内联Lua、证据bundle/keep/remove及checked close。三个最终包报告分别为`.tmp/rust-disk/evidence/7c7774e3-6d3d-41a3-8a9d-8a98c043ee9f/business-report.json`、`214cfc2d-d1d6-405e-956e-66b2506d712a/business-report.json`及`7005e180-5be6-417c-aec7-6cc16e50bfb5/business-report.json`（后两路径同evidence父目录）。
- effective另11项检查通过：正式CLI将两份原XFTH9归档成CAP，实际Agent核新增/替换/删除、push/index/历史顺序、精确CSV/JSON/CAP及三个错误后的同会话恢复，static不变；报告`.tmp/rust-disk/evidence/a19fb88b-7595-4044-8746-4580ed2c176e/business-report.json`。
- 七产品Map实际验收全部通过：Retail1187/Classic285/Era59/Anniversary83/Titan130完整行，以及两Forever各73/81行、8行不可读的原partial/缺key/分区身份。逐行全值/顺序、分页、CSV、CAP、release/close全部核对；Retail cold与显式prepare分别跑过。真实安装在任务期间被外部更新，旧PIN已按合同拒绝；未修改真实安装或重标旧PIN。复用共74175bytes原BLTE，在明确synthetic的最小CASC元数据中以七个新独立PIN执行，原加密块逐字节不变；manifest `.tmp/rust-audit-next/map-synthetic-inputs-41182067569/inputs.json` SHA `91c5326b8832b7822f4125d6c1447680ff5e2ef979949e2eb4a115ba2774ec49`。这证明固定真实原始数据业务，不扩大为当前安装新build或Live通过。
- 同一Retail原BLTE的134400资源验收通过：原BLP全字节、PNG/WebP64×64全部RGBA和CAP依赖/清单都匹配独立原文；报告`.tmp/rust-disk/evidence/4c969bb9-1542-4ccf-bf1b-b1c68acf00f3/business-report.json`。
- Delivery七阶段通过：新装/500槽/Skill、幂等、update plan/apply、已由最终CLI确认的合成3.0.0干净受管输入、slot250真实deny-delete失败、原archive/不传release的resume与重复resume、完整归档移除/无关文件保留；报告`.tmp/rust-audit-next/final-delivery-41182067569-3/report.json`。同一原TGZ的loopback registry完成两次真实npm pack→handoff→全局布局隔离prefix安装→apply，第二次新nonce、第一份完成history字节不变、500槽和sentinel、服务/进程回收全部通过，报告`.tmp/rust-audit-next/final-npm-loopback-41182067569/report.json`；没有发布或更改真实npm标签。
- 管理七阶段通过：配置持久化/非法值恢复，synthetic client的named/direct target及固定PIN/CAP，当前US可用清单原观察，cache损坏只读校验/显式prune/keep保护，doctor只读诊断及target配置修复、移除named target后原PIN/CAP保留；报告`.tmp/rust-disk/evidence/9e748408-cf27-47f5-974c-de80a0558f92/business-report.json`。doctor未带Live选择器/--fix，SQLite SHM与空WAL为临时读侧文件，主库/配置/PIN/CAP/合成client持久字节不变。零字节cache目标因明确保护内容而保持complete=false。
- 18个最终安装包控制器的独立汇总全部passed，均核相同TGZ/CLI/commit/source、inputsStable和handlesReaped；完整报告引用及哈希在`.tmp/rust-audit-next/acceptance-summary.json`。临时harness的路径/参数/返回字段/DDL准备错误已纠正，失败记录保留；未放宽产品校验，未触发新产品构建。

## 保留事项与收尾

- 当前没有未解决的确认产品缺陷；末次交叉复审和最终同包业务均通过。18个控制器不包含实机，也不把网络清单、partial、helper或输入构造算成完整游戏行为。
- REG-01旧升级准备错误5未再复现、原因未定，当前最终包正常安装/升级、真实错误5注入和原意图恢复全通过；旧记录不改写。未解决恢复记录`4747c1ed-d561-4984-8ffc-e4f8e7ef2f8b`、`de770010-ea9c-40fa-a48e-27141a6e0d64`、活动项目/CAP与明确保留的`.worktrees/rust-refactor`保留。
- 早前13个已解决scratch和`.tmp/rust327-audit`（465文件/4,950,653,420bytes，已核回收/归属/reparse）删除被自动审批拒绝，保持原处，不换工具或路径重试。本批8份退役/reaped的旧编译输出（约13.05GB）、以及新生成的Microsoft/Windows/PowerShell/ModuleAnalysisCache清理同样被自动审批返回blocked by policy，未执行删除、未改路径或工具重试。最终CI包、最新本地构建、原业务输入/证据及未解决恢复全部保留。
- 文档链接/路径与差异检查通过；本结果文档提交使用[skip ci]，实际验收继续绑定ff65对应的上述完整CI与同一原包，不重复构建。PR保持draft/未合并/未发布。剩余清理见CLEAN-07；实机仅按用户后续安排执行，见最后一节。

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
