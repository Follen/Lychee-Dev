# 当前交接

架构见 [DESIGN.md](DESIGN.md)，未完成项见 [todo.md](todo.md)，测试用途见 [test.md](test.md)，维护规则见 [AGENTS.md](AGENTS.md)。这里只保留当前快照。

## 分支、范围与状态

- 工作树`.worktrees/pr14-local-dev`，分支`rust-refactor`；main3.2.7已同步。代码与11份Markdown集中提交推送`66aa8f5e0bb57cf3b974b5a48b27dca36e6b7c8d`，复用[draft PR #14](https://github.com/Follen/Lychee-Dev/pull/14)。开发版本3.5.0/beta；latest继续3.2.7。
- 本轮死代码、冗余测试、CLI/Skill和文档审计，以及正式Forever非Live实现与包验收均已完成。最后两侧只读闭包复审未发现新确认缺陷；两个仅供测试读取预算/数量的薄getter保留为可收窄候选，没有增加另一套执行器。
- 正式Forever规范ID/Source family为`forever`，官方远端/flavor为`wowf`，目录`_forever_`，可执行文件`Wow.exe`；当前支持六产品。两个Beta退出当前目标准入，历史PIN/CAP/raw和恢复身份保留。
- 未合并、未发布、未改npm标签、未全局安装Skill/npm。游戏进程读取、输入、真实部署和Live实机测试继续留待用户安排；本次正式Forever只读游戏文件。
- 所有工作者已完成；Root负责文档、集成和唯一native队列，当前队列空闲。未新增工作树或PR，无关未跟踪`Microsoft/`目录保留。

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
| 文档、Skill与工作区 | Skill references由20收敛11；11份Markdown按设计/用途/待办/事实分工更新，移除陈旧状态；闲置工作树清理48份、保留5份。 | 静态/动态Skill、版本、链接和包装合同通过；保留原失败、活动输入及未解决恢复，不把离线结果计为实机。 |

## 最终CI与唯一包验收

- [CI #21 / 41209679482](https://github.com/Follen/Lychee-Dev/actions/runs/41209679482)：十二个job全部成功，含静态检查、固定Lua/addon ZIP、两组完整Rust测试、优化Release、隔离npm/Skill、同TGZ内容/业务及候选封存。producer为PR合并快照`b8601d89bc111216342b51bd507362ac707e185a`，与代码分支66aa的Git tree相同。
- 源码摘要`0b4c32ea233c0f242951292f5e856489ec79e38f94a51ca77ee9ea72005a9a5f`；原TGZ SHA256为`f798106c1bd877ec27c1e24951e68cb91c75fa1907e9fd1e8c608d721c6bd861`。候选11项SHA256SUMS逐项核对；本地消费原包，未重建CLI/luac或重打TGZ。
- 17个非Live控制器通过：Source公开调查/四产品语义矩阵/Forever原编码；五产品固定Map（Retail另核cold与prepare）；原资源导出；27项Data领域业务；effective；管理；受管main/500槽/Skill交付恢复；两次npm更新；独立Agent三个场景；真实正式Forever数据。所有输入稳定、owner/进程回收，CAP原文另核。
- 正式Forever当前安装已更新至1.60.1.70707；新PIN为`PIN-3263ce85fc4b8b24c6377a6bd234ff7fc68a1aea34fb65dfafeb1c8df98fa2f6`。独立IDX/BLTE/zlib核验新raw，与历史Go样本逐字节证明schema/偏移及两艘飞艇中文名称的精确变化，再生成新预期。全部分页、CSV、CAP核73条可读记录；81条逻辑记录中的另8条缺钥，3个分区保持partial。
- 验收控制器曾有过期Skill引用、私有fixture DDL格式及PATH旧CLI选择问题，均只修临时验收材料后重跑；原失败和70658→70707输入变化保留。新预期未由Rust typed输出生成，原Beta数据没有重标为正式输入。
- 临时证据索引`.tmp/dead-redundancy-audit/final-acceptance-41209679482.json`，其中逐项绑定17份报告、包和原producer；正式数据独立预期在`formal-map-independent/oracle-proof.json`。当前验证事实已写入本快照，测试用途见test.md；临时材料按当前证据与恢复归属清理。

## 工作树与输入保留

- 原53份工作树已用git worktree remove清理48份，无独立代码变更；两个witness的7项差异经原字节核对仅为CRLF。分支与PR保留。
- 剩5份：主checkout、当前pr14-local-dev、明确保留的rust-refactor；pr14-agent-e85-first-check保原prepared fence/ordinary.lease.original及未确认回收owner，pr14-fc001-offline保未确认source-pack owner。回收按owner证据判断。
- 此前13个scratch、rust327-audit、8份旧编译副本及ModuleAnalysisCache删除被自动审批拒绝，未换工具绕过。当前产物、活动项目/CAP、固定输入和未解决恢复保留。
- REG-01旧准备错误5未再复现、原因未定；恢复`4747c1ed-d561-4984-8ffc-e4f8e7ef2f8b`和`de770010-ea9c-40fa-a48e-27141a6e0d64`保留。旧CI与旧包结果只作输入/方法基线。

## 下一步

由用户安排下列Live实机。重新接入时使用当前验收包和新鲜身份；后续变更按实际风险补验，复用已通过产物。

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
