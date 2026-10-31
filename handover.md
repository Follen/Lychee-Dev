# 当前交接

架构见 [DESIGN.md](DESIGN.md)，未完成项见 [todo.md](todo.md)，测试用途见 [test.md](test.md)，工作规则见 [AGENTS.md](AGENTS.md)。这里只保留当前快照。

## 当前目标与范围

- 用户目标：从生产应用角度充分审计死代码、冗余测试和文档，修复后复审；高强度精简Skill，把参数、版本/渠道与协议合同收进CLI自解释。不能用符号次数、旧通过或测试准备代替当前验证。
- 当前工作树`.worktrees/pr14-local-dev`，分支`rust-refactor`，基线HEAD`8b3280f00cd802710f87b9a38064291cdb4bba0c`；复用[draft PR #14](https://github.com/Follen/Lychee-Dev/pull/14)。main3.2.7已同步。版本/发布渠道仍由release清单决定，本批不发布、不变更标签。
- 已有提交/推送/PR维护/完整CI和正式同包验收授权继续有效。禁止Live、游戏进程读取/输入、真实客户端部署和全局Skill/npm安装；本机不运行全局keyboard-hook测试。所有Cargo/native由Root唯一队列串行，沿用target/cache、锁定离线依赖和编译默认60分钟。

## 当前实现与实际验证

- Core/Data：旧Table named-row/page、CASC LocalObject、BLTE顶层物化、decoded-cache全量旧路线、metadata旧读/删链、默认包装与孤立fixture/benchmark已退役。独立原始输入和负例迁当前leased/selected/streaming/controlled/scoped路径；重复DomainRequest binding提取合并，原两侧验证保留。`.build.info`畸形错误补真实sourcePath，保原code/exit/stop/details。
- Windows/Live/Delivery：无image/单root旧解析和默认包装已退役；实际layout/candidate/ambiguity仍保。全仓共八个空成功child helper已改显式ignored/必需角色，由原父测试精确启动；业务断言、原期限和回收保留。read_pool_controlled误留cfg(test)已在集成中修正。
- Source/Emmy：无消费者默认包装、旧binary exporter/profile/pack诊断整闭包删除；原测试迁controlled入口。Git核文件checked-close，Source compiler准备保原control。已清真实unused imports，两Visitor impl移模块作用域，未改serde方法体。
- CLI：参数错提供当前command/schema/help/acceptedFlags纠正，真命令拼写错保搜索。24个always-required命令已按全68命令独立矩阵补齐；原15处目录遗漏和实际typed admission正负例已修。Data Agent新增公开responseSchema与逐task结果schema，真实Session/transport帧测试联接；serve schema/help已描述真实matching-ID帧；元数据查询外层仍用普通CLI envelope，协议只嵌入一次。
- 更新缺陷：Skill错误声称固定latest；生产又手写版本→tag，导致未来同lane新版本误拒。现从编译时嵌入release/npm-channel.json单源导出updatePolicy，prepare selector、候选绑定、schema/help/describe复用；原Handoff tarball/targets/deadline恢复不重选。发布工具也去版本表与未知版本latest fallback，TGZ审计精确比原authority。
- Skill：references由20合并为11，主文保产品常用名/版本系列/TOC表、任务选择、发现和必要约束；删除手写工具版本/渠道、命令参数手册与重复操作步骤。当前参考链接/命令静态合同通过；独立只读复审未发现关键partial/精确数/不重放/cleanup约束丢失。README安装示例指定所选TGZ，修正旧doctor、准备期限与CN输入范围。
- Node相关758项中757通过、1个non-Windows专属分支在Windows显式跳过；测试构建隔离HOME/Skill/tmp并保原Cargo/Rustup缓存，受影响工具回归通过。version、Skill静态合同及35份Markdown链接通过。无消费者的phase v1—v4接受分支已移除，v5与历史ordinary报告保留，相关121项通过。
- owned全目标类型检查、89程序构建通过；广泛原生回归89程序实际完成、87通过、2失败且全部回收。CLI两项新测试非法附带--home已修正，未扩展生产flag；另一失败为mailbox_input_fixture原10s control到期。该程序间接安装全局keyboard hook，本机后续整程序排除，只由CI干净runner验证，保期限/断言并保留本次失败。不能把旧本地广泛运行写成全通过。
- 最后编译告警复审清除28份Source/CLI文件和2份Data文件的无用导入/mut；共享fixture/宏均核实其它实际harness消费者并保留。原ServiceOutcome改显式验证已完成报告及cleanup pending事实，未吞must_use结果。Live fixture不再记录无人读取的kind副本。
- 最终build-8为89程序+2support，源码摘要`ff3635d104b42d2e809f60edd1b527a00c800c8e9c4eea149de1b643326e8e5f`；沿用原target/cache、默认栈及60分钟编译期限。受影响CLI251、installed_inputs14、hotfix_tables15项全部通过并回收。实际元数据查询与89程序ignored清点完成，清点70项只证明列表，不能计业务通过。
- 另确认4个现有Windows guarded-spawn用例没有CI helper供应；producer现将harness=false helper封存为support，CI显式执行4项，filtered零执行拒绝计绿。相关CI/runner补验50项通过；本机同批sealed helper实际执行4项全部通过并回收。
- 模块代码已冻结。最终只读交叉审查未发现新的实际消费者缺口；本机还须跳过Windows identity_tests中的class/executable mismatch一项，其constructor间接安装hook，已同步test.md。新的独立Agent只按Skill/实际metadata写好三个Data场景与update策略说明，尚未执行，不能计通过。详细候选和临时证据在`.tmp/dead-redundancy-audit/`。

## 工作树清理

- 用户新授权清理闲置工作树。已清点53份并用git worktree remove移除48份：HEAD均是当前基线祖先且仍由分支引用，无独立代码更改，相关owner回收/无项目标记已核。两个witness的7项状态差异只是CRLF，独立字节比对成立。未删除分支、修改PR或触碰当前改动。
- 当前剩5份：主checkout、当前pr14-local-dev、此前明确保留的rust-refactor；另pr14-agent-e85-first-check保原prepared fence/ordinary.lease.original及未确认回收owner，pr14-fc001-offline保未确认回收的source-pack2 owner。不能用旧日期推断回收。
- 结果`.tmp/dead-redundancy-audit/worktree-removal-result.json`，完整只读库存和receipt分类同目录。既有CLEAN-07被拒绝的其他临时材料没有换工具重试；当前Microsoft缓存未跟踪、不暂存。

## 可复用基线与下一步

- 上批完整[CI 41182067569](https://github.com/Follen/Lychee-Dev/actions/runs/41182067569)及18个最终包非Live控制器已验收，仅作为本批之前的基线。其源码摘要`5e7159bdeb4474570bca74ca699257c716fbbc3955aabb7f277bf6e7459ba548`，原TGZ SHA256`3e2286a7b4c532e537a5b1f11645829cb7c84a889335a9950dad1231b6d9724a`；报告索引`.tmp/rust-audit-next/acceptance-summary.json`。复用固定Lua供应、当前缓存与独立原始输入，不把旧二进制用于验证新代码。
- 原输入/七产品独立PIN和有效catalog均保当前树。七产品Map/asset上批验收使用原真实BLTE与明确synthetic CASC外壳，不能扩大为现在真实安装新build或Live通过。
- 下一步：集中提交推送本批，等待完整CI/Release，再以该轮唯一TGZ完成正式包业务回归及独立Agent实际使用。相同输入/config产物复用，下游不重建。
- REG-01旧准备错误5未再复现、原因未定；旧未解决恢复`4747c1ed-d561-4984-8ffc-e4f8e7ef2f8b`、`de770010-ea9c-40fa-a48e-27141a6e0d64`、活动项目/CAP保留。旧结果不改写。

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
