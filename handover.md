# 当前交接

架构见 [DESIGN.md](DESIGN.md)，未完成项见 [todo.md](todo.md)，测试用途见 [test.md](test.md)，维护规则见 [AGENTS.md](AGENTS.md)。这里只保留当前快照。

## 当前目标与范围

- 从生产应用角度持续审计死代码、冗余测试、业务与文档；精简Skill，强化CLI自解释。修复后复审，测试清点和旧产物不代替实际验证。
- 当前工作树`.worktrees/pr14-local-dev`，分支`rust-refactor`，已推送HEAD`3142ee0311802b30aa23af975cad2a1d65beed6e`；复用[draft PR #14](https://github.com/Follen/Lychee-Dev/pull/14)。main3.2.7已同步；开发版本3.5.0/beta，release清单保护latest3.2.7，本批不改标签。
- 用户已确认Forever只支持正式版：规范ID/Source family `forever`，远端/flavor `wowf`，目录`_forever_`，可执行文件`Wow.exe`。范围贯穿产品、安装与build信息、Source/Data/Changes、Delivery、addon、Live及恢复。
- 既有提交、推送、PR维护、完整CI/Release与同包验收授权有效。不合并、不发布、不改npm标签、不全局安装Skill/npm。Live代码和离线适配继续，游戏进程读取/输入、真实部署和实机测试留待用户安排。
- Cargo/native只由Root单队列串行，复用target/cache、锁定离线依赖、默认栈，编译批次允许60分钟。模块已STOPEDIT，Root正在复编和精确复验。本机排除全局keyboard-hook用例，入口见test.md。

## 已提交批次与验证

| 范围 | 主要变更与实际证据 |
| --- | --- |
| Core/Data | 退役旧named-row/page、LocalObject、BLTE顶层物化、decoded-cache全量路线、metadata旧链及孤立包装；独立raw/负例迁leased/selected/streaming/controlled/scoped。`.build.info`畸形错误保原code/exit/stop/details并补sourcePath。 |
| Windows/Live/Delivery | 删除无消费者解析/默认包装，保layout/candidate/ambiguity；八个空成功child helper改显式ignored及必需角色。verified report与cleanup pending分别断言；read_pool_controlled误留cfg(test)已修。 |
| Source/Emmy | 默认包装、旧binary exporter/profile/pack诊断闭包退役；测试改接controlled入口，Git核文件checked-close，compiler准备保原control。固定Emmy上游未修改。 |
| CLI/Agent | 全68命令独立矩阵核24个always-required并修15处遗漏；参数错给适用的纠正metadata。Data发现提供真实task响应schema、matching-ID帧和精确值/handle/page/export/CAP；普通envelope不重复嵌套。 |
| 自更新 | 移除Skill固定latest与生产版本→tag表；编译时嵌入release/npm-channel.json，updatePolicy、registry selector、候选准入同源。保原Handoff tarball/targets/deadline；发布工具无未知版本latest fallback，TGZ authority精确核对。 |
| Skill/工具/覆盖 | references由20收敛为11，保客户端词汇、partial/精确数/不重放/cleanup。Node原758项中757通过、1项Windows不适用跳过；受影响CI/runner50项及generation/profile/evidence121项通过。四个guarded-spawn现有用例获得同批封存helper供应并拒绝零执行，本机四项实际通过。 |

[CI #20 / 41197065886](https://github.com/Follen/Lychee-Dev/actions/runs/41197065886)对应3142ee快照，十二个job全部成功，包括两组原生测试、Release、222项Lua、隔离安装、同一TGZ内容/业务检查和候选封存。测试与Release各配置只构建一次。本机钩子相关失败保留，干净CI完整验证通过。该轮不包含以下未提交代码，当前最终包业务验收尚未开始。

## 当前未提交实现与回归

- 9个孤立Source examples及10个专属support文件退役，原Lua/JSON/XML输入保留；独立预期迁五份文件共35个默认测试。删除test-only refs/context投影、旧build/build_execution/query/entry-by-name包装、XML严格probe profile及孤立字段；保当前research投影、正式XML extractor、Graph/Program action与inline编译映射。
- 最新全目标build-16成功，89个测试程序+2个support，源码摘要`0b4c32ea233c0f242951292f5e856489ec79e38f94a51ca77ee9ea72005a9a5f`。当前Source全库759通过/3忽略，CLI252通过；报告`.tmp/dead-redundancy-audit/cli-source-16.json`，两程序exit0且全部回收。原build-11/13/15失败保留，不改写为当时已通过。
- cold回归发现的Integer→Number返回误报已在现返回兼容分支修复，General/arity/context不改；D精确断Integer/Number并保三上下文原预期。对象改核相同结构/字段/identity/epoch/view/proof，NameExpr/CallExpr来源site各自验证，成员/动态负例保留。当前完整Source回归通过，35个迁移默认测试均已执行通过。
- build-15七程序全部运行并回收：Core837、Data958、installed_inputs14、durability28及Live离线516通过；对应ignored保留、不计通过。CLI旧数量断言已修并由build-16全库252项验证。Delivery188项当时通过，新Beta status预期保持生产外层metadata_conflict/内层client_identity，修正后精确1项通过，不重复十分钟整库。原报告forever-affected-15保留；当前精确报告delivery-status-16确认exit0、实际执行1项且全部回收。
- 循环头/循环结束后for局部变量遮蔽是实际缺陷，已在自有syntax适配修复；XML身份测试补齐原XML生成的inline输入。三项旧返回预期分别断言实际slot、声明、真实diagnostic与未知arity coverage。另两组旧record深比较目标经独立复核不属当前固定Emmy合同，退役整对象断言，保所有成员/epoch/身份/generic/动态预期，并以实际member_type+scalar比较保number/string负例；原record-union Lua保留，不反转预期或移ignored计绿。最新代码尚未重新编译/回归。
- 正式Forever只读证据：本地`.flavor.info=wowf`、Wow.exe与根`.build.info`同为1.60.1.70658，无version.txt；CN/US官方wowf/versions内容相同，五区域最新1.60.1.70707，buildConfig`7feb61f3c994bcb168526b8db521514a`。本地build不改为最新build，不以70707推导最低边界。证据在`.tmp/forever-formal/`，不计Live通过。
- 已有源码baseline的1.60.1/Interface16001与完整build分别维护，不凭版本拼TOC。历史legacy forever DataPin可能曾指向Beta；须分离历史记录认证与当前目标准入，不将同名视为wowf来源。
- 正式Forever模块已静态完成：Core六产品目录、无version.txt安装发现、历史记录认证/当前准入分离；Data远端只wowf；新建Beta项目拒绝而旧project/PIN字节保留；Source/CLI只公开正式目标；Live精确wowf身份并拒旧lease复用；Delivery沿Core识别正式安装。保原始Beta负例与恢复，退役全空slot/全false标记。固定Data由Core inherit_parent统一拒变更，remote重复guard已移除，恢复原fixed_data_conflict并补local/remote真实入口负例；CLI解释--from只补缺失选择。当前Rust修改仍待新产物验证。
- 另确认`target resolve --file`的独立writer漏了退休产品准入，现于最终继承/格式验证后、提交前精确拒两个Beta ID，保共享历史校验/stored_pin/load与旧PIN字节。新增Core四组及CLI实际argv回归，拒绝包括parent继承；完整正式Forever与CAS原winner正例保留，文件错误沿原command.failed/5，不添加另一套全局validator。
- 当前selection file原11+新增4项全部通过，报告file-admission-16；CLI新增真实file负例包含于上述252项，输入字节不变、无PIN/CAP发布。两个受影响真实XML入口xml_ordered_copy_versions与xml_invalidated_mixin_source各实际1项通过，Source正式关闭、全部回收；报告`.tmp/dead-redundancy-audit/xml-current-16/owner-proof.json`，复用build-16、固定mirror与已认证luac，未重建。
- 当前CLI七项真实metadata查询及与实际binary的Skill合同通过；89程序重新清点70项ignored，仅listing不算业务。证据`.tmp/dead-redundancy-audit/metadata-inventory-16/`。当前本地必需补验已闭合，准备集中提交推送并执行最终CI/同包验收。
- 修改后的Forever addon离线37套件全部通过，固定Lua供应直接复用，全部进程回收；报告`.tmp/dead-redundancy-audit/addon-formal-forever.json`。覆盖70658/70707新鲜build、16001与未知16002拒绝、零frame；生产ClientGate/TOC已正确，未机械改写。该结果不计Live实机通过。
- 新独立Agent仅按Skill和实际metadata准备了三个Data场景及更新策略说明，尚未执行。最终用新CI安装包实际运行，Root保留独立oracle；准备材料在`.tmp/dead-redundancy-audit/fresh-agent/`。

## 分工与下一步

| Owner | 文件边界与下一步 |
| --- | --- |
| Root | 共享Markdown、Skill、tools/CI与集成；核Source对象关系合同，维护唯一native队列及最终CI/TGZ验收。 |
| data_effective_acceptance | Core/Data主体回归已通过；file publication退休Beta准入及对应四组测试静态完成并STOPEDIT，待Root复验。 |
| go326_delivery | Source/CLI正式Forever、Integer返回修复及D当前合同迁移已静态完成并STOPEDIT；对象深比较未列为已实现能力。 |
| go326_doctor | Live/Delivery及addon离线输入/测试已静态完成并STOPEDIT；Windows无产品表无需改，原恢复不重标。 |

模块静态完成并STOPEDIT后，复用缓存生成新测试产物，复验Source失败、受影响XML真实入口及Forever全链；推送同PR，跑最终完整CI，消费唯一TGZ完成独立Agent与非Live包业务。下游不重建CLI、luac或重打包。

## 工作树与输入保留

- 清点53份工作树，已按授权用git worktree remove移除48份，无独立代码变更；两个witness的7项差异独立字节比对确认为CRLF。分支与PR保留。
- 剩5份：主checkout、当前pr14-local-dev、明确保留的rust-refactor；pr14-agent-e85-first-check保原prepared fence/ordinary.lease.original及未确认回收owner，pr14-fc001-offline保未确认source-pack owner。不能用日期推断回收。
- 结果`.tmp/dead-redundancy-audit/worktree-removal-result.json`。此前CLEAN-07材料删除被拒绝，未换工具绕过；无关未跟踪Microsoft目录不暂存、不删除。
- [CI 41182067569](https://github.com/Follen/Lychee-Dev/actions/runs/41182067569)及18个旧最终包控制器仅作输入/方法基线，索引`.tmp/rust-audit-next/acceptance-summary.json`；不计当前代码通过。复用固定Lua供应、原BLTE/DB2、源码镜像与独立预期；七产品历史输入保原身份，两个Beta不能改名计正式Forever通过。
- REG-01旧准备错误5未再复现、原因未定；未解决恢复`4747c1ed-d561-4984-8ffc-e4f8e7ef2f8b`和`de770010-ea9c-40fa-a48e-27141a6e0d64`、活动项目/CAP保留。

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
