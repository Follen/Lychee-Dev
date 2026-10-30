# 当前交接

架构以 [DESIGN.md](DESIGN.md) 为准，剩余事项见 [todo.md](todo.md)，测试用途与入口见 [test.md](test.md)，工作规则见 [AGENTS.md](AGENTS.md)。这里只保存当前任务状态。

## 范围、分支与分工

- 用户要求：同步 main 3.2.7，完成原设计剩余业务、回归与架构/业务/Skill-CLI/有效覆盖/死代码/全仓集成/Rust 原生度审计，额外做一轮冗余测试审计。Live 实机留到用户后续安排。本轮不连接或读取游戏进程、不发送游戏输入、不更改真实游戏安装或全局 Skill。
- 工作区 `.worktrees/pr14-local-dev`，分支 `rust-refactor`，既有 [draft PR #14](https://github.com/Follen/Lychee-Dev/pull/14)。本批基线 `d13e3374267249b80d13b5d14818abc96d6d322a`；main 已 fetch 核实仍为 `cbf96df6465c661e2e5a21b0c972d6bd76beab83`，3.2.7 发行实现 `dcb35d7f36`。Rust 版本仍以 release/version.json 的 3.5.0 为准。
- 已有提交/推送本分支、更新 draft PR、跑完整 CI 与 Release/正式包验收授权继续有效；不合并 PR、不发布、不改远程 npm 标签。性能已接受，停止优化和补跑性能轮次。
- 分工已结束，三个代理均 STOPEDIT：rust_business_audit 完成 Data/Engine；go326_doctor 完成 Core/Windows/Delivery/Live 离线路径；go326_delivery 完成冗余测试与 Agent 使用面。协调者负责整合、根依赖/lock、CI、共享文档、唯一原生队列及最终验证。

## 本批已实施，待最终验证

| 范围 | 当前修改 |
| --- | --- |
| main 3.2.7 | ReceiverBindings 与 main 当前文件逐字节归一化一致；先验证有效绑定方案，等待加载/脱战，停止监听后写入并保存原方案，重读玩家修改，手动重置清诊断。独立旧 protocol 场景合入当前 addon 套件。preserveLatest 仅更新为 3.2.7，beta 与 Rust 版本不变。 |
| Data 领域 | 法术 EffectBasePointsF、Speed、RangeMin/Max 保留原浮点/数组；DecorGet 与 CreatureDisplay 按文件两阶段的 partial 空选择报 unavailable，完整空来源仍报 not_found。新增正式 Session、Engine 与自动 CAP 的完整结果回归。 |
| Data Engine | 删除无生产消费者的 NumericPlanner/SQL aggregate planner 与 SQL 前端测试；独立数值、双分区聚合、Decimal schema、HashJoin、COUNT 等断言迁到 typed 生产入口。关闭 DataFusion sql feature，锁定 metadata 已核；上游仍必选依赖 datafusion-sql，不声称整个依赖消失。 |
| Agent 使用面 | requestSchema 覆盖全部十五种完整 wire 请求，复用当前领域/查询合同；公开示例走真实 serde/Session 验分页、CSV、CAP、release 与限制。doctor 无效 --no-cache 兼容参数及 Skill 说明已删除，外部固定 Go 参考不变。 |
| Delivery | addon/Skill status 共用既有受控、有界、checked-close 检查器，贯穿原请求 control。只把实际损坏投影为 modified/incomplete；取消、期限、I/O 与清理错误原样失败。 |
| Core/Windows | 复用挂起启动、附 Job、恢复的单一实现，不新增内存上限。原 control 等根、pipe workers 与同 Job 自然清空；失败保留原 5 秒资源收尾窗口，join/checked close 并保留 primary/cleanup。超窗只报告未确认回收，不续业务或报成功。 |
| 死代码/冗余测试 | 删除无生产用途 doctor wrappers，测试直接调用正式 report/close helper；去掉镜像/重复断言和无消费者 Lua builder。字符隔离、真实 INPUT 与非空 History 场景接入当前 addon；bridge 证明测试实际读取 memory/numeric 两种证据并逐字段拒绝。CLI 数值集成目标改名 data_agent_public_numbers。 |
| CI | 两项文件 symlink 的 ignored exact 用例在所属 Windows runner 显式执行，复用同一测试记录，独立保存报告；环境缺权限仍失败。原两组逐程序汇总、唯一 TGZ、Release 和包内业务链保持。 |

## 实际验证

- 3.2.7 绑定用例在移植前六客户端均复现 SaveBindings 方案错误；移植后四套 × 六客户端 24 项通过。当前完整 addon 为 37 套，完整 CI 将执行 222 项；本轮尚未声称它们全部通过。
- Red all-targets 构建通过，93 个测试程序和一个 support，源码摘要 `520c3d9d7a3d49334f118f3b81d6b88a11fe359bee9cad189243b96b6f546176`。Data 九项 4 通过/5 失败；Delivery 六项均失败；Core 保留原 deadline/exit5/reaped，但 jobEmpty 确认缺失。未据此声称旧 Core 曾提前返回成功。
- 冗余清理相关 Node 77 项、workflow 21 项、版本检查、固定 Lua 供应校验、静态 Skill 合同（68 命令）、文档链接与 diff 检查通过。Windows 修改未新增 unsafe 块，边界清单检查通过。
- 首次 green 构建报 schema 测试 E0382（值 move 后再用），已修；非零退出后 MSVC vctip 辅助进程由 owned runner 强制收尾，原 receipt `4a7139fd-57ad-446a-aa00-c18859745c86` 确认树/句柄全部回收，不作为构建通过。整合复核另补取消/deadline 与 PSS 清理错误并存时的原错误码分类，保留完整失败，并增加纯映射回归。
- green-2 all-targets 已通过，93+1，源码摘要 `f5eac3f115dcf816ef8cc40be4bccf8ee1755e4c4da65b0bc19f27e8d5e78063`。受影响整批 `.tmp/rust327-audit/run-green.mjs` 已结束，全部进程回收确认。Delivery 五程序共198通过/1 ignored（库175，含新增六项，543.46秒）；CLI 214+6通过；Core进程五项通过；Live库516通过/2 ignored；Windows默认两项与显式sealed helper两项通过。独立helper直接取自同次Cargo的原exe并保存哈希/原生产证据，不重复编译。报告各在`.tmp/rust327-audit/*-green.json`，总表green-summary.json。
- Data首次完整库912通过/1失败/1 ignored，195.60秒；新增九项领域均通过。失败正文 `catalog accepted an unknown field` 定位为serde内部tag unit visitor忽略未知字段。Catalog/Close已改为空struct，并移接五文件七处构造/match；schema和独立断言不变，也排除其他五个公开对象枚举同类问题。最终green-3 all-targets已通过93+1，源码摘要`0b7a12f5fa6cfa2ccd72d31f7e0e4ce9163eabfac3112ec0e75515fd2870fdcc`；schema4、catalog1、CLI214、实际数值IPC3与supervisor7全部通过、回收确认。其余未改路径复用green-2结果。报告final-summary.json和原失败记录仍在本轮临时目录。
- 所有原生工作使用现有 owned 队列、默认栈、本机单 job、50 GiB cache/5 GiB scratch/2 GiB 预留；未另建 target。仅在持有原 channel 并检查绝对路径/reparse 后清理旧 incremental 26,405,516,864 字节，依赖、程序和 Release 留存。最近 cache 28,440,101,363 字节，后续按实际 admission 处理。

## 可复用的既有结果与具体缺口

- [上一轮完整 CI 41051431104](https://github.com/Follen/Lychee-Dev/actions/runs/41051431104)的十二个 job 全通过，验收产品提交 `95f2b152764be0669da0787b9ad4130667d0b8c4`，实际 PR merge checkout `b6497ff51e768bb82a8c1f9f73e07fdda1a3e812`。93+1 构建、两组 24/69、Lua、Release、唯一 TGZ 生产/安装/审计/包内 Source/Data 和候选封存均通过；它不代替本批新源码验证。
- 上轮 [候选](https://github.com/Follen/Lychee-Dev/actions/runs/41051431104/artifacts/12976227345) TGZ SHA-256 `900523d58ad0ddf724277c6c074d67b114bba662f95da2c0241052e145bafdc0`、Release CLI `93ddfb3acaf44c8762b87e0922da86ce55c0062f5cb92f9668ff8470f969ccc4`，原日志/receipt/timings/许可证 closure 随产物保留。记录结果的 d13 文档提交跳过 CI，没有改写产物身份。
- 未改 Source/Emmy 的完整业务证据继续有效：`.tmp/business-regression/audit-2-source-unit-report.json`、adapter 全部十九个程序、真实 checker families/nominal、四产品 Source 与 XML 矩阵，以及已安装包的 Retail/Forever 冷热、原字节、编码告警、inspect/diff/luac/native 会话。各领域状态独立，unknown 仍为 partial。
- 既有七产品固定 Map 全值/Page/CSV/CAP/close 已通过；两个 Forever beta 分别 73 行可读、8 行不可用，保留独立 PIN/partial，不合并身份。Retail FID134400 raw/PNG/WebP/CAP 已通过。细节仍在原 `.tmp/business-regression` 与 `.tmp/rust-disk/evidence`，不补性能轮次。
- REG-01：Delivery 升级准备曾报 Windows 错误 5；同程序 exact、整个 addon 事务组、完整库及上轮 CI 后续均通过，原因未定。原 `.tmp/business-regression/default-report.json` 与失败 fixture 保留；不加重试或削弱断言。
- 两项文件 symlink 本机曾因 1314 未执行实际断言；当前已接 CI，真实结果待本批 CI。普通 ignored 不能计为通过。
- 原活动项目/CAP、外部固定参考、未解决恢复记录 `4747c1ed-d561-4984-8ffc-e4f8e7ef2f8b` / `de770010-ea9c-40fa-a48e-27141a6e0d64`、保留 worktree `.worktrees/rust-refactor` 均保留。此前十三个已解决 scratch 删除遭自动审批拒绝，未重试绕过。任务结束只清本轮已完成临时材料。

## 下一步

1. 集中提交并接入main3.2.7历史，核整合前后Rust树相同，单次推送。
2. 跑完整CI，核两组93程序、两项symlink实际断言、222项Lua、Release、同TGZ安装/审计/包内业务及候选身份；PR保持draft。
3. 保存本轮真实CI身份/结果后收尾文档；不为结果记录重建原生产物，清理本轮已完成临时目录，保留原失败/恢复记录与明确保留的worktree。

## 最后执行：用户后续安排的 Live 实机

本轮没有占用 Live。此前本任务两个 CON 都已正式关闭，不能从旧进程身份或 receipt 的 active 字样推断当前连接。历史安装、PID、build 和 INPUT 观察不代表现在仍有效，恢复前用当前候选与新鲜身份重新确认。

保留项目 `.tmp/business-regression/live-retail-audit/project`。原激活 `CON-772d4ff02d794e1da489f49fea92651e` 及诊断 `CON-6dba6d65e83c8fd4bcbbcc6fff011684` 均经用户手动 reload 后正式 resume 关闭；最后释放报告 `release-live-after-reload.stdout.json`，receipt `e2e1ee40-5cd1-4aac-add9-c5a06bd537bc`。原输入为 not_sent/0，原期限未延长、未重放。

LIVE-01 原因：INPUT 采样后 500 槽 acquire/verify 约 519/517ms，发送前因样本不再同一 publication 拒绝。Invoke 准备后重新采样、独立 Reload、激活 build/product 身份检查已离线验证；实际发送与清理仍留待实机。原分段与失败记录继续保留，不改写成业务成功。

| ID | 待实际验证 |
| --- | --- |
| H01 | Retail、Classic、Classic Era、Titan、两个 Forever beta、Anniversary 各自连接/执行/报告/断开。 |
| H02 | 当前 500 槽受管安装/更新、首次手动 reload 与原连接恢复。 |
| H03 | 3.2.7 绑定等待/有效方案、焦点、活动输入、战斗及 protected/secret 边界；零 Esc/聊天文本。 |
| H04 | 同安装多进程与跨产品的 PID/创建身份/HWND、GUID/runtime/owner/fence 隔离。 |
| H05 | 500 槽容量、reload/relogin、原请求和期限及运行时退休。 |
| H06 | 可控中断、unknown/submitted 不重放、可用报告与 cleanup pending 分开恢复。 |
| H07 | 角色 GUID 启用、doctor 默认只读、受管修复与 foreign 元数据保护。 |
| H08 | 确需实机的有界 Live 耗时；同候选/输入记录原始总时与分段，不恢复原 Go 比值目标。 |
