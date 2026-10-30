# 当前交接

以 [DESIGN.md](DESIGN.md) 为业务与架构目标；待办见 [todo.md](todo.md)，测试用途与入口见 [test.md](test.md)，工作规则见 [AGENTS.md](AGENTS.md)。本文件只保存当前状态。

## 工作区、目标与分工

- 2026-10-30用户要求将Go 3.2.6变更同步到Rust，继续此前修复、业务回归和审计，暂时不占用Live。main版本与本机npm包均为3.2.6，参考HEAD `15f4db7a05`、实现`0e04ef6690`；Rust仍为3.5.0。此次只做代码、离线/隔离业务与CI，不连接游戏、不发送Live输入或更改实际游戏安装；历史性能结果不重跑或改写。
- 当前任务是修复CI并避免重复构建。go326_delivery已修复Skill staging并交回只读复核，无新增确认问题；go326_native已交回两类构建入口的jobs参数及编译fixture摘要。只有协调者持有写权限，负责workflow、文档、Git和真实Actions。本轮未运行本机原生构建或操作游戏。
- GO-326实现及离线回归已完成：Delivery持久归档重置与精确退休、Doctor原CON关闭/默认归档续走/hard-reset及统一CLI入口已接通；旧schema须有实际双键INPUT才允许升级Reload，业务回执和新runtime证明保持当前协议。审查补齐旧代与迁移槽清单拒绝退休、进程结束后hard-reset保留原槽字节两处边界。旧测试将可升级协议一律拒绝的断言已改为拒绝未知协议，保留独立的升级正反例。
- 最新all-targets构建通过（93个测试程序、1个支持程序），源码`d6bd7d7796dc199c47eb47377e45066612281c4294ae594f666cd19983a0cfc0`，receipt `cb41e4fd-dc72-4a40-93fc-37c0c94963f6`；Live完整库516项通过、2项ignored，报告`.tmp/business-regression/go326-audit-report.json`。新增四项激活身份回归先复现漏检，修复后全部通过。其余11个受影响程序此前已通过且后续路径未改：CLI215、实际CLI两组各3、自有窗口2、Delivery166、Delivery集成23（1 ignored）、Mailbox合同7、Lua桥接11；报告为`go326-regression-report.json`中前四项及`go326-regression-rest-report.json`中七项。原失败保留，测试与辅助进程均已回收。
- 用户要求的本任务Retail Live占用已释放。手动reload后，原CON `CON-6dba6d65e83c8fd4bcbbcc6fff011684`通过正式resume退出0、`closed=true / complete=true`；新runtime连续新鲜样本证明旧运行时已结束，原not_sent/0及期限不变。报告`.tmp/business-regression/live-retail-audit/release-live-after-reload.stdout.json`，receipt `e2e1ee40-5cd1-4aac-add9-c5a06bd537bc`。本任务未建立后续连接，原记录保留；该结论不描述其他任务当前的Live归属。
- 工作区：`.worktrees/pr14-local-dev`；分支：`rust-refactor`；既有 [draft PR #14](https://github.com/Follen/Lychee-Dev/pull/14)。本批本地验证基线为`9fbfd1d84568f7ec92b5656dbd7c63d337c6696d`加任务改动；当前提交以分支为准。版本以 `release/version.json` 为准，当前3.5.0。
- 用户目标：完成设计文档全部设计；当前性能已接受，先回归全部业务，再循环审计。最新要求是先统一梳理Source调用语义，集中修改、集中验证，再恢复完整业务回归。停止性能优化和补跑，保留已有测量事实。
- 已有提交/推送本任务、更新draft PR #14及运行真实CI的授权继续有效。最新要求暂停Live占用，之前Retail实机授权本轮不使用；全局Skill安装、合并、发布及渠道变更均不在本轮范围。
- 本批业务实现已推送至`7ff4bacbd01ff9d591e0e9d52f38e26125a51fc7`，PR保持draft。[旧CI 41008314464](https://github.com/Follen/Lychee-Dev/actions/runs/41008314464)静态/测试构建与上传通过，Release构建后因Skill嵌套父目录ENOENT失败，未上传Release。当前CI修复已完成待提交：完整Node工具222项、workflow22项、actionlint、版本/Skill/文档与差异检查全部通过，Skill目录和jobs/fixture回归均已包含；没有重复本机原生构建。原生Release与许可证closure提前封存，打包独立复用；下载与两种配置的编译缓存分离，CI显式双job且传入当前提交身份；PR与开发分支push重复触发已移除。旧测试产物绑定旧提交，未改写身份复用，旧Release不可取回。真实Actions待本轮推送后验证。
- LIVE-01已定位到INPUT采样后的两次500槽准备耗时，两项真实DiskPublications回归曾复现发送前拒绝，receipt `3d3c6e38-00dd-47d0-b0ba-b9507d21eac2`。Invoke准备后重新采样、Reload独立于槽池均已通过本批Live完整库；原目标、持久意图、one-use、profile、最终publication与deadline约束保持。临时计时已删除，实机复验按用户当前要求暂缓；诊断dev CLI仅保留原问题证据用途。
- 主分支核对确认当前addon已包含main代码，只有3.5.0版本号不同；Go3.2.6相关行为完成移植后，以ours策略接入main历史，已核实整合前后Rust tree完全相同。2026-10-30只读`npm view lycheedev dist-tags --json`确认latest=3.2.6、next=3.1.2，本地发布清单`preserveLatest`已更新为3.2.6，不改beta目标或远程标签。此前已删除无消费者且调用退役EncodeSignal的`tests/protocol/writer.lua`，当前正式bridge的四项输入与精确数值覆盖不变。
- 两轮审计修改及本批安装包的受影响验证全部通过。安装包基线源码`3ff2a83c4a0e5f03f5d70883a564423bad53bdbbce70ab7d39f7618053f936be`的`audit-2` all-targets构建通过（93+1，receipt `de1e643f-a86d-41ec-828b-dea633a36178`）；adapter全部19个测试程序、Source完整库712项（5项ignored）、真实checker families及nominal通过。CLI库211项及两实际CLI程序各3项、Data transport101项、CI工具回归和18项workflow结构检查通过。新Release、隔离npm安装、包内receiver/发现信息及Retail/Forever均通过。这些结果不覆盖后续Live修复；其余Actions、symlink权限和实机缺口见下文。
- 原生构建/测试共用既有owner队列，本机默认单Cargo job、CI显式双job；默认栈、50 GiB build cache、5 GiB scratch、2 GiB空闲预留及隔离用户目录保持。Cargo编译批次默认60分钟，测试执行保持各自业务期限。

## 已接通的业务范围

- **类型化边界与使用面：** Source、Data、Live、Core、Delivery接收各自请求，Invocation留在CLI；入口冻结项目/安装选择。68项命令的解析、help、describe、schema、示例与Skill发现来自唯一`commands.json`，实际无home发现与正常退出已通过。
- **Data：** 通用查询和12项领域任务走Agent Session → Engine → DataFusion。完整值、关系、顺序、UnionAll重复项、Iterate previous-delta、精确整数、Page/CSV/CAP及Analyze已回归；既有句柄不依赖原输入继续可读。raw/图片/Hotfix CSV/demux统一发布与计量，Unknown或清理失败保留未结算额度；不自动收编历史未归属数据，`total_data_coverage_complete`仍为false。
- **Source：** 固定原版Emmy、自有adapter、TOC/XML、API环境、声明/引用/调用/关系/差异、二进制Facts/Index和受管luac已接通。非UTF8原字节、哈希和位置保留，编码告警独立；未知或条件写保持partial。32 MiB+17超限文件不丢失同快照合法文档。冷查询复用刚发布的完整索引，热查询继续认证并投影。
- **Live/Delivery：** 当前addon、Mailbox v1、SessionOwner、500槽、有效键位、持久输入意图、原请求/deadline恢复、不重放、报告与cleanup分离已有离线回归。正式Lua bridge已接当前addon和受管Lua；离线结果不证明H01—H08游戏行为。
- **清理与CI：** Go产品源码、旧执行器/协议/测试及冻结副本、旧CI和失效文档已移除；Data旧借用游标与无生产用途转发已退役。CI按static → 每种配置一次构建 → 下游复用 → release-ready接通；本机build/seal/verify/run及显式许可证closure复用通过。真实Actions和跨job绝对路径仍待核实。

## 本轮业务回归与未闭合问题

审计修改与实际验证：

| ID | 已确认的问题与修改 | 验证状态 |
| --- | --- | --- |
| A01 callback | 可空callback的泛Function实参未比较具体签名，却可能记为完成；完成度现复用实际参数分支，兼容与诊断规则保持。 | 四场景及adapter全部19个测试程序通过；报告`.tmp/business-regression/audit-2-adapter-report.json`。 |
| A02 receiver | 显式typed self漏比较；普通隐式receiver诊断指0..0或显式参数。比较已统一到实际参数检查，位置取receiver自身。 | 旧已安装包三例复现，receipt `fc423c80-3b31-4057-a78e-8521f00dae1d`；adapter与真实Source三例通过。 |
| A03 CLI发现 | 连接/恢复/断开/重载/bugs误标无游戏输入，status/session误标接触进程；live doctor修复效应未表达，status仍提BTP。 | 效应改由commands.json声明，live doctor的gameInputWhen标明--fix，清除失效BTP说明与npm README开发分支badge。静态Skill合同、版本、6项Skill工具、CLI库211项及实际CLI两程序各3项通过；报告`.tmp/business-regression/audit-1-cli-report.json`。 |
| A04 CI回收 | 普通测试未回收后继续启动，后项可能覆盖未回收事实。 | 已停止后续启动并保留错误/证据/报告；11项纯Node回归通过。 |
| A05 Data冗余 | 两条HTTP读取链的read_bounded/collect共4包装仅测试调用。 | 已删除，测试直接调用正式collect_admitted；body/gzip/取消/收费等101项通过，报告`.tmp/business-regression/audit-1-data-transport-report.json`。 |
| A06 CI报告 | 三个validate报告上传默认仅成功后执行，失败报告留在临时runner。 | 三步加always，原job失败状态保持；18项结构测试通过。 |
| A07 泛型receiver | 显式T=number的colon receiver未代入类型，table被接受；等价dot能报错。 | 旧包四场景已复现，receipt `658ccabf-c12d-4ad9-bba2-cb67a6d61ec8`；receiver已走现有泛型代入/别名/条件类型规则，adapter共16场景及真实Source四场景通过。 |
| A08 adapter冗余 | plain_atom无任何调用；旧scope_metrics计数器只写无消费者。 | 已删除函数及测试计数，不变更生产语义；adapter全部测试通过。 |
| A09 激活身份 | Prepared和旧协议手动等待的自动重载快路径漏核候选build/product，可能向不符的runtime发Reload。 | 两阶段×两身份字段的四项回归先复现，入口检查修复后Live516项通过；原owner、请求、期限与零输入日志均核对。报告`go326-activation-red-report.json`及`go326-audit-report.json`。 |

当前Source完整库报告`.tmp/business-regression/audit-2-source-unit-report.json`；真实checker families和nominal报告`.tmp/rust-disk/evidence/3cc1f90d-c144-4319-9e6d-4aad59cff234/summary.json`，原断言及partial边界保留。此前完整业务回归的默认程序原报告`.tmp/business-regression/default-report.json`保留Delivery首次失败。

| ID / 范围 | 当前事实与下一步 |
| --- | --- |
| REG-01 Delivery | `main_checkpoint_rejects_same_version_replacement_without_replay`曾在升级准备阶段报Windows错误5。同一程序exact、完整addon事务19项及最后完整库156项均通过，原因仍未定。完整库报告`.tmp/business-regression/repaired-3-delivery-report.json`；原失败fixture保留，不增加重试或改写业务预期。 |
| REG-02 进程回收 | 已验证测试helper关闭根句柄后，在原15秒deadline内等待同一Job归零；真实supervisor与Source/Data隔离通过。原失败时成员身份未知，不声称曾有worker泄漏。 |
| REG-03 Source语义 | 已验证。调用许可、参数比较、返回类型/数量及coverage各守其证据边界；统一后的真实nominal、返回数量/控制流、公开semantic validate、catalog及checker families全部通过。最后非尾vararg由list入口正确调整，精确空/单/多值、SourceCall/Exact Entry、参数/self偏移、赋值和未知尾经Source完整库及真实原失败场景验证；unknown仍为partial，未降低数量断言。 |
| REG-04 Lua桥接 | 已验证。统一解释器报告`.tmp/business-regression/lua/build-report.json`；当前addon35套×6客户端配置通过（`1160f78e-b14f-4415-bce4-df5bdb8764a4`），正式Rust桥接11项通过（`d9e0f50c-20a4-4300-8ced-46d3fc0f5623`）。 |
| REG-05 Data缺钥 | 已验证。真实准备链和重试次数通过；删除没有合同依据的总decode_work大小不等式，保留实际阶段次数和成本观察。新鲜fixture与业务用例通过（`d6eba767-2bee-4648-aa9e-45a84cdf88f0`），报告`.tmp/business-regression/data-key-repaired-3-report.json`。 |
| REG-06 XML Facts | 已验证Script与handler的声明、关系、原始行映射及新缓存身份。Source库、公共XML查询和最后修复产物的16个显式XML场景均通过；最终XML及四产品Source矩阵报告为`.tmp/rust-disk/evidence/6f8668a5-c634-475a-b65c-68722a4d5383/summary.json`。 |
| N07 CPU取消 | 新实际worker CPU进展后取消用例通过，连同其余supervisor用例共7项通过；核原15秒deadline内Job回收、旧句柄失效与不重放。报告`.tmp/business-regression/repaired-3-supervisor-report.json`。这不证明某一次DataFusion poll完全不yield。 |

此前真实return count、semantic validate及catalog已通过，报告分别保留于`.tmp/rust-disk/evidence/ce11292f-8e31-43c5-83bb-5b912eac3d51/summary.json`和`.tmp/rust-disk/evidence/000afdda-3def-45d5-8b23-47a680181d6c/summary.json`；后者同时保留非尾vararg原失败，修复已由当前完整库和families验证。Delivery完整库156项通过的源码为`5d30313a798614abac33d03afcd07e549815916ef05cf0de9afa18ca9f2bfe5c`，不受本批Source改动影响。

其他仍有效的业务验证：

- 实际CLI flow、Source会话复用/取消/原请求恢复、Data断管后的Source隔离及公共XML三组通过（`876a600a-775d-4ca2-ac7a-dfc91025d2a1`）。Live隔离部分仅用正式持久化API构造输入，不涉及游戏。
- 320 MiB内容检查与完整导出通过（`082cc2d5-3aa1-4f67-81a9-9e0d70c1f698`）；实际Retail Data Session通过（`9cb3a31f-a0ef-4f62-93f4-c323c274bc10`）。
- 四产品Source矩阵以固定Classic提交`1028c1e687f721ba9d3af14d1b12a5745e4227c7`在最终语义修复产物通过（`6f8668a5-c634-475a-b65c-68722a4d5383`）；最初临时输入与固定TOC不匹配的失败保留。最终同批XML 16例均通过。
- 两项文件symlink测试因Windows 1314权限受阻，未执行重定向断言。缺依赖、ignored与环境受阻均不计为业务通过。

## 已安装包与固定输入

当前Release已包含两轮审计修改：artifact `.tmp/business-regression/release-audit/artifact.json`，源码`3ff2a83c4a0e5f03f5d70883a564423bad53bdbbce70ab7d39f7618053f936be`，二进制SHA-256 `62222ecd6c6f57c726dafc0ed8f75ddd8d9f7da4c19bf982d6e3695e4ad87553`，build receipt `2871f477-73ea-4c87-af90-13022fa38e86`。实际编译17m56s，当前运行期限为60分钟。隔离npm包报告`.tmp/delivery/npm-audit/report.json`：安装、包内Skill launcher、受管luac、显式许可证closure复用及实际Source语法检查均通过，未发布。包内七个receiver场景及命令效应通过（`3ee94983-ce37-46d9-afdc-839ef021df0c`）；完整Retail Source/native/会话复用通过（`908e5b33-76ef-417d-9fa4-9830f2390e66`，输入与产物保持一致）；Forever冷/热/原字节/编码告警/inspect/diff/luac/native复用通过（`8fe70325-7d31-4cf2-8281-3bdba19cc075`）。

审计前v12包的公共Source flow/native会话隔离/XML（`e9687ca5-84b0-4266-927c-335f931eac02`）、显式refs/context（`136211d9-2e7b-4d55-8ea0-1376af16b630`）、luac/inline/ordered/validate_contract（`f963d2db-a194-4f1e-8dec-c4a2f2b967a6`）通过，未受本批修改影响。v12完整Retail及Forever报告分别为`b55e25ec-6ad3-48e6-ac81-aaf957f21d26`和`65d5eeeb-094a-4e2a-813b-9acaa4057034`；它们不代替本批安装包验证。

七产品Map各自固定PIN，完整核对值、顺序、CSV、CAP、来源、缓存复用和关闭：

| 产品/build | 已验证结果与receipt |
| --- | --- |
| Retail 12.1.5.70448 | 1,187行完整；`7ca9bdc5-3cf6-4f74-9c39-80c6fdf39c68`；预备缓存`ff0cd19f-193d-4329-be73-1d0fb68cab0f`。 |
| Classic 5.5.4.70531 | 285行完整；`37ae35bd-d555-4942-b57d-b0fe97c3b0e0`。 |
| Classic Era 1.15.9.70003 | 59行完整；`fa9eb4a3-7ff5-415e-abf1-1d6db06b3c28`。 |
| Anniversary 2.5.6.70006 | 83行完整；`022c7e3f-dabc-4ba5-82c5-0f6ef2c3f9de`。 |
| Titan 3.80.2.70522 | 130行完整；`e19fd8a1-9cc7-4ad7-9b0c-7163688cb332`。 |
| Forever Asia beta 1.60.1.70450 | 73行可读、8行不可用，partial全路径通过；`8611ee01-9ee8-4178-b8a0-62f00b95d34e`。 |
| Forever CN beta 1.60.1.70450 | 独立PIN，73行可读、8行不可用，partial全路径通过；`20789daa-2f7d-4092-90eb-44c3cc2780bb`。 |

Forever缺失key ID为`0da4670db36fb2a9`、`d2c028cb7ac14d0b`、`da0e7785727a0a65`；外部固定参考报告相同分段和记录。已核对缺失记录及CAP的`complete=false`。Retail FID 134400的inspect/raw/PNG/WebP/CAP也通过（`daf1fc07-ee18-4e5d-9b4f-51913331912c`）：raw 3,916字节、图片64×64完整RGBA与独立预期一致。这些结果不证明其他表、全部资源或游戏行为。

## 已接受的性能与清理限制

- Source正式四组各20对完整一致，index冷/热中位比值38.75%/37.69%，query冷/热46.40%/29.12%，达到原50%目标；报告`94f3afa3-0cb3-4239-91ed-c3e2c5bb28d6`。describe冷/热只计正确性，整份报告仍为`fullB01B07Acceptance=false`。
- Data内容与数值完整单对及独立复核通过正确性，未达到原50%目标。内容冷Rust/Go 440.59/188.79 ms、热166.40/146.15 ms（`d607974a-079d-4bd8-8ef7-dd7d2facf718`）；数值fresh 592.04/138.32 ms、prepare-inclusive 761.05/142.40 ms、first-after-prepare 376.29/142.40 ms、subsequent 371.93/112.38 ms（`3d5dc8c2-b367-4663-9a80-01d18a96eff8`）。双方完整业务生命周期与原始控制器总时均保留，不回写历史口径。
- 用户指定Cargo target/scratch清理已执行，target随构建重新生成。13个已解决实验scratch的删除曾被自动审批拒绝（`blocked by policy`），仍保留，不重试绕过。外部参考、活动输入、性能失败输入和未解决恢复记录`4747c1ed-d561-4984-8ffc-e4f8e7ef2f8b`、`de770010-ea9c-40fa-a48e-27141a6e0d64`保留；旧receipt的active不代表当前进程仍运行。`.worktrees/rust-refactor`明确保留。

## 下一步

1. 本地完整业务回归与两轮审计已完成，修复已在本批安装包验证。后续只因新改动、实际失败或明确未解决的问题补验。
2. 完成本轮CI静态检查与只读复核后提交推送，验证Actions下游与跨job产物复用。后续失败只重跑受影响job，保留已编译产物和缓存。两项symlink测试需要创建文件符号链接的权限；当前令牌没有该权限，系统未读取到开发者模式启用标记，环境设置未变更。
3. Go 3.2.6业务同步和Live修复的离线验证已完成；本任务两个Retail CON均已正式关闭。用户当前要求暂不占用Live；再次实机工作前核对当时安装版本、实例与归属。

## 最后执行：需要真实客户端或用户配合的验证

`D:/Game/World of Warcraft`的七产品安装、executable与共享CASC文件已确认存在；Retail为12.1.5.70448，两Forever Beta为1.60.1.70450。用户仅授权当前Retail；下列跨客户端项目仍保留具体缺口。

当前Retail只读doctor报告`.tmp/rust-disk/evidence/ca9928b7-aec8-4845-8210-a9a1a047437d/readiness.json`：PID `12032`，进程创建FILETIME `134371419009029958`，HWND `242684304`，build `12.1.5.70448`。磁盘为干净受管3.2.5（commit `c3450d9a32d6ea1334f0209897cc02ab06502ba6`）；500槽、0待处理预留、无owner。新CLI报告`mailbox_release_mismatch`，需受管升级后手动reload。此次没有创建CON或发送输入；诊断未证明游戏业务通过。报告中另有临时脚本误调用顶层`instances`的参数错误，正式命令为`live instances`；已知目标不再重复发现。

授权后用本批已安装候选包执行`live doctor --fix`，受管交付成功后进入`activation_waiting_manual_reload`，退出6。原项目为`.tmp/business-regression/live-retail-audit/project`；CON为`CON-772d4ff02d794e1da489f49fea92651e`；请求为`retail-audit-activation-20261023`；PID/创建时间/HWND未变。报告`.tmp/business-regression/live-retail-audit/activate.json`及receipt `9c578590-60cd-4c17-8435-048d33477882`保留。用户已明确回复完成手动`/reload`；后续恢复原CON。doctor响应中的deployment是修复前的3.2.5诊断，不能当作修复后的磁盘状态。

升级后只读复查`.tmp/business-regression/live-retail-audit/after-upgrade-doctor.json`确认干净受管3.5.0、500槽、0预留及原CON归属。用户手动reload后，原CON发现runtime `000000150001da9537dd9ada27e93a3c`，GUID `Player-741-066A34E3`；开始原600秒激活预算。首次恢复遇到过期INPUT；随后的只读`.tmp/business-regression/live-retail-audit/input-doctor.json`全部健康，取得新鲜INPUT、`escapeLimit=0`、执行`ALT-CTRL-F12`和重载`ALT-CTRL-F11`，receipt `91f6d434-1ba7-4ecc-98a5-4a3f1d33e86e`。该诊断不证明绑定或业务完成。

**LIVE-01｜原发送前复核阻断证据。** 当时四次输入记录均为`not_sent / messagesQueued=0`；sample至outcome为1487、1409、1346、1268ms。旧路径在采样后多次完整验证deployment，并要求复读仍为同一地址/时间戳且500ms内，分段结果见下段。普通disconnect进入`closing_exchange_unconfirmed`；用户第二次手动reload后，`resume-cleanup.json`退出0、`closed=true / complete=true`，新runtime `000000160001dcf86a866d9ed3405688`的连续新鲜样本完成旧运行时退役，receipt `e97efacd-f54e-4d64-b3c9-471f9ff6744a`。原CON已关闭且未重放，业务报告仍不可用。后续诊断激活使用独立请求`retail-input-diagnostic-20261023`，只针对同一PID/创建身份；不得改写原期限或清除原记录。

临时诊断的同实例实际分段见`diagnostic-activate.json`，receipt `b68fcf9d-97d0-424f-aaf6-aa8d295b0365`：采样128ms、样本年龄223ms、intent落盘4.8ms、input.check55ms、monitor0.26ms、publication acquire519ms、verify517ms、首guard51ms、复读76ms，最后`same_publication=false`。主要耗时在采样后的publication准备。Invoke准备后重新采样与独立Reload已完成离线回归，尚未实机复验。诊断CON `CON-6dba6d65e83c8fd4bcbbcc6fff011684`已通过用户手动reload后的正式resume关闭，原reload intent仍为not_sent/0，原期限`1792695381145`未改。

| ID | 验证目标 | 需要的实际结果 |
| --- | --- | --- |
| H01 | Retail、Classic、Classic Era、Titan、Forever Asia Beta、Forever CN Beta、Anniversary分别连接与执行。 | 产品/build、实例、报告、清理和断开；两个beta分别记录。 |
| H02 | 当前500槽受管安装、更新与首次激活。 | 无兼容新鲜INPUT时手动reload；原连接恢复，发现/启用/加载各有事实。 |
| H03 | 有效绑定、用户输入活动、焦点、战斗与protected/secret边界。 | 零Esc、零聊天文本、不抢前台；只释放本次注入按键，原deadline不延长。 |
| H04 | 同安装多进程与跨产品实例。 | PID/创建时间/HWND、GUID/runtime/owner/fence独立，发布与恢复不串线。 |
| H05 | 500槽容量边界、reload/relogin。 | 原请求与期限保持，槽不违规复用，报告与运行时退役事实准确。 |
| H06 | 可控中断、未知输入与cleanup恢复。 | submitted/uncertain不重放，原连接/请求/期限保留；已验证报告与cleanup pending分别记录。 |
| H07 | 按角色GUID的槽启用与故障恢复。 | doctor默认只读，受管修复按原连接完成；未就绪不报ready，不改foreign元数据。 |
| H08 | 确需实机的有界Live耗时场景。 | 同一候选产物、业务与输入，记录原始总耗时和分段；不设Go耗时比值门槛。 |
