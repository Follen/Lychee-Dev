# 当前交接

架构见 [DESIGN.md](DESIGN.md)，剩余事项见 [todo.md](todo.md)，测试用途见 [test.md](test.md)，工作规则见 [AGENTS.md](AGENTS.md)。这里只保存当前状态。

## 目标、分支与授权范围

- 2026-10-31用户要求循环审计八项：架构/模块边界、业务完整性、Skill-CLI一致性、有效覆盖、死代码、全仓集成、Rust原生度、冗余测试。每轮修复并验证确认问题后继续复审，直到每项不再发现新问题。当前从已验证HEAD 210d3f934d27f12171503d983060920ab4e5da13继续；先独立深审，再集中修改和受影响验证。前批已通过结果在路径未变时继续有效，不能充当本批新改动通过。Live实机仍留后，不连接/读取游戏进程、发送输入、更改真实安装或全局Skill。
- 工作区`.worktrees/pr14-local-dev`，分支`rust-refactor`，既有[draft PR #14](https://github.com/Follen/Lychee-Dev/pull/14)。产品提交`d6a3a8dd293cdbb117c703cb903786f18c023d01`，main历史整合提交`1e7b1dd98e5d19fbf01327a05bd138aa7954f92f`，均已推送。main为`cbf96df6465c661e2e5a21b0c972d6bd76beab83`；先移植行为再接入历史，整合前后Rust tree均为`8fae72bf0dc39d2b1c3cd84757f723311cada0ec`。Rust版本仍以release/version.json的3.5.0为准。
- 当前PR可合并但仍draft，未合并、未发布、未修改远程npm标签；preserveLatest仅随main更新为3.2.7。已有提交/推送/PR维护/完整CI与正式包验收授权已执行；不新增实机或发布授权。性能已接受，不补优化或比较轮次。
- 用户追加再做8轮，每轮覆盖原八项；发现问题修复后复审，末轮仍有新问题就继续。非Live业务须跑实际任务与结果核对，可安排子代理模拟使用者；复用固定输入和最终同一CI包，不按轮数重复编译。Live范围保持不变。
- 当前分工：rust_business_audit负责Data/Core清理与Source native_session/analysis_process、CLI Data进程消费者；go326_doctor负责Windows（不含broker/destination）、Delivery/Live/formal_live及Core storage；go326_delivery负责CLI/Skill、Source/Emmy清理和Windows destination/Data assets旧发布路线及相应测试迁移；协调者独占Windows broker、Source batch/assets/index、Core process测试helper、tools/CI和共享文档。所有原生构建由协调者串行执行。

## 当前补审状态

- 主修复已集中推送5a5e7a7fcd5ef41e848c317f29f4804dda2fc6f1。各模块追加八轮阅读、修复及最终静态交叉复审完成，最新未发现新增确认问题；非Live最终包全业务和独立Agent自解释仍待实际验收，AUDIT-08未勾选。
- 本批已修：Semantics只许static/空captures、partial Aura不生否定事实；Source/Hotfix项目与最近锁、明确snapshot优先、严格batch/matrix/schema/Skill合同；Windows进程/pipe/event/Job/线程checked-close；doctor实际archive、正式安装完成Receipt、npm完成journal/结构化错误；Source Hello原control、observer读取错误、child/retirement/observer组合与原fatal退出；NativeReader独立child取消和原ledger。旧Live observation、destination executor、Source/Emmy无用包装、JSON codec、镜像断言及空跑helper已退役或改接当前路径，独立业务预期保留。详细用途在test.md和相应回归，不保存逐轮日志。
- 实际本地绿：Core842/1ignored、Data920/1ignored、Source726/5ignored、CLI237；最新Source会话84、显式公开多进程IPC1（含原60s idle退出，110.77s）；formal25/update26/1helper ignored/doctor8，及Windows/Emmy/Node专项。最终业务源构建4abbaa2c65d32ba8dc630917b727ed9b8f9b579b483c52ea962484d9b743c353，93程序+1support，receipt b90c3d99-271b-4513-9d98-aa46efce1d5f，回收确认；IPC只证明该开发CLI及已认证luac资源，不能冒充安装包Release。
- [本批CI 41179518527](https://github.com/Follen/Lychee-Dev/actions/runs/41179518527)实际checkout15704913e6152282207642c53f1e1d103b647b8a，与5a5e7a7fcd同Git tree。Node309/workflow21、Lua/ZIP、两配置构建、data-delivery、优化Release/唯一TGZ、内容审计和隔离安装均绿；source-runtime有两项PNG正例夹具仍无IHDR而失败，candidate因此跳过。已仅补两夹具首chunk长度13/IHDR，保24bytes及123×456等原断言；新owned all-targets与两个实际目标6+13已绿且回收（source5e7159bdeb4474570bca74ca699257c716fbbc3955aabb7f277bf6e7459ba548，receipt12dc3d96-8c1d-4209-b640-d2e1ebe90d5b）。下一步推夹具修复、跑新完整CI，不将前次失败run标通过。
- 上述原TGZ已下载并独立核ZIP SHA498a24ad6917bfe0c1960b719b41fc0222cbe0585bf7829b4d4fc7256c52e11d。TGZ8f0df308acf499077c4f4b4b2e673f0cc91142e289b68dbf406812e72395c9d7/CLI443095fc80b803ec67061b5b8943e1bf7b617e4749667fea2ee3a5db89648104已通过本机真实隔离npm/Skill安装、commit/dirty/source与原产物绑定；报告.tmp/rust-audit-next/ci-41179518527/local-npm/report.json为partial_passed，业务另计。为现有精确commit gate临时detach后已回rust-refactor；handover有本批结果待最终文档提交。最终业务统一消费新完整CI同一TGZ，禁止改写producer身份或下载record的原CI绝对root来假装本机测试。
- 临时实测控制器已准备但未执行业务：Source公开/Forever编码/四产品矩阵、七产品Map（含Retail冷/prepare和两Forever73/81 partial）及资源RGBA；Data12种navigate/17独立向量、query/Page/CSV/CAP/partial恢复/Hotfix/AVI；Delivery真实文件升级故障/原意图resume/remove与loopback registry的完整npm prepare→handoff→apply。独立cli_blackbox_usability代理仅读安装Skill和已生成describe（exit0/输入稳定/回收确认），没有读实现或答案；首次metadata临时runner把自身可变receipt算入日志而cleanup失败，已修caller并原样保留cd6b6abc-e255-479e-870e-b09e902e2a63记录，不重跑describe。该代理native目前暂停，待本地fixture验证结束再恢复metadata写worker；业务只用新最终报告，必须实际核有用回答/partial/错误恢复/关闭，不以exit0或脚本语法检查算通过。

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
