# Lychee Dev：本机开发接手说明

更新时间：2026-10-21 UTC。此目录是后续开发的公开交接入口。
后续开发与验证集中到接手工作区，避免多个实现者并发修改产品。此目录只交付开发说明、验证计划和两个一次性维护工具，不包含原始本机日志或重复源码归档。维护工具公开可见不代表任何使用者获准删除任意目录。

## 1. 唯一代码基线与身份

- 仓库：https://github.com/Follen/Lychee-Dev
- 分支：rust-refactor；草稿 PR：https://github.com/Follen/Lychee-Dev/pull/14
- 当前已发布完整提交：cfdefd46a11ec8dce470d296b424a3eaa1372b8b
- Tree：1361adffc9b3c5e0d8969e451bb44d456ab63a2e
- 同版本代码：https://github.com/Follen/Lychee-Dev/tree/cfdefd46a11ec8dce470d296b424a3eaa1372b8b
- 真实 Skill source anchor：75cd762e31df0dfd614c8687f9fd3fdb26b6a7f7
- Skill 子树：c1768f92d55b454cafb202eb6c2835af1ecc2811；25 个源码文件。

当前版是重新实现并复核的 194 文件恢复版及后续窄修，不是此前丢失且从未发布的 206 文件候选。不要使用旧 a853 或本地未发布 anchor 97c8 作当前证据。历史 Go/早期 Rust 合约证据保持独立，不能重标签为新内核结果。

接手先只读核 Git HEAD、tree、工作区修改和远端分支，不覆盖他人变化。保留完整 Git 历史，历史合约测试需要真实旧 blobs；不要浅克隆后拿 fixture JSON 自证。不要合并 PR 或发布版本。

## 2. 当前架构与实现范围

生产链是 clap / 71 命令合约 → data agent schema / serve JSONL → Session → typed DataFusion Engine → 受认证 static/effective 来源 → owner-backed 分页、JSON/CSV export、capture 与 close。十八个旧 Data 命令不再是正式生产路由；共享 scalar、decoder、row traits 可以保留，旧 SQL/Program 不作为生产 fallback。显式 migration-oracle 用于历史对照，Release 不启用该功能。

- Data：typed Scan/Lookup/Filter/Project/Join/Aggregate/Sort/Limit，加 UnionAll、query-valued IN/EXISTS/scalar subquery、outer reference、受界 Iterate/Delta。非递归 CTE 可内联。不是任意 SQL 语法兼容；动态 query-valued CASE/COALESCE、相关 LIMIT 等限制需要按源码和业务矩阵逐项核，不得用十二导航冒充任意递归查询。
- 来源与资源：受认证 DBD definition/layout/reading/verification，owned reader，真实 Arrow IPC/ID 缓存与覆盖范围匹配；部分缓存不能冒充整源。fresh request control、会话 retained、operator/result/page/work 限额仍有所有权约束。逻辑账本不是完整 native allocator/RSS 峰值证明。
- 领域：navigate 的 item_get、item_models、item_textures、item_geosets、spell_info、spell_auras、spell_summons、encounter_get、decor_list、decor_get、creature_display、creature_model 共十二条成熟业务路径。physical locator 不猜 public ID；完整关系报告自动 CAP，同 Session 请求绑定，提交后错误保留已提交身份。普通 Query 不自动 CAP，需显式 Capture。
- Semantics：真实 static pin-bound DBD 元数据，不伪造 Hotfix 历史语义。Explain/Analyze 返回真实 DF 计划表格 handle；Analyze 原执行中采样整个 request 时间/work，明确 root/prerequisite scope，不重跑也不伪称所有前置节点的 native metrics 已合并。
- Session/transport：每请求一帧 protocol/id/ok/result 或 error，不是旧三帧 result.v1。库取消与正式 supervisor 硬取消不同；硬取消/异常退出令整会话及 handles 失效。Windows owned worker/Job 监督时间、退出及背压；非 Windows 不宣称 OS 硬限。
- Data 存储：默认十进制 5 GB 的 Data 逻辑文件长度治理、metadata 私有数据库、quota/recency、durable staging/spool、证据目录和 native anchor。此项与开发构建缓存 20 GiB 完全不同；不声称覆盖所有物理分配块或用户外部输出。
- Source：既有 Postcard Facts / 二进制 Index 与 Emmy 认证保留，没有为迁移重写共享解码器。
- Live：既有 go_mailbox_v1 SessionOwner、线程归属句柄、持久边界、协议 v1 保留，没有无关 daemon/通讯改造。
- CLI/Skill：clap 负责正式解析，本地 cli search、精确 schema、分层有界 help；发现命令不启动业务、不读缓存或联网。Agent-only，不新增交互/彩色 UX。当前 paired、pipe、npm-installed Skill workflows 均转真实 Agent 路径；历史报告只用于历史。

进一步阅读已发布的 docs/architecture/agent-task-business-coverage.md、domain-navigation-migration.md、reconstruction-whole-plan-ledger.md，以及仓库 DESIGN/AGENTS。它们部分写于实现冻结时，NOT_RUN 描述以本交接的后续验证状态补充，不能反向把源码声明当运行通过。

## 3. 已验证与未验证，务必分开

当前 cfdefd46：
- 默认和 migration-oracle 两套 cargo check 已通过。
- Data/CLI 实际测试枚举 codegen 已通过。Data 44 targets：1805 active、25 ignored；CLI 10 targets：204 active、3 ignored。
- Core all-list 列到 853 项，但 ignored-list 被磁盘准入拦住，因此不能据此给 Core active/ignored 完整数。
- 当前统一版实际业务 case 执行数：0。没有完整业务、性能或 Release 验收。
- CI：Rust parity foundation run 39515270056 的 windows-static-parity 成功；toolchain observation 39515270136 成功；performance 39515270198 跳过。其他 native/product/context/contracts 跳过不能算通过。
- 旧 9d84/b5e/2060 的编译或 CI 失败均保留。cfdef修正了 metadata 泛型递归 codegen；check 通过不等于 test codegen 或业务通过。

cfdef 前已修：Arrow Array trait import、DF54 first_value order_by 参数、PinnedSet 持有与字段取值、clap 未启 color feature 的调用、当前71命令/13 custody文件的测试断言。cfdef只修改 Core metadata.rs 两处私有稳定回调边界，不提高 recursion_limit，保持取消/事务/close 顺序。

此前 d3 业务对照曾 1 pass / 1 fail，失败发生在旧 oracle 对数组 A 绑定，typed 比较尚未执行；后来修为明确数组元素后重组。这个旧结果不能算当前194恢复版通过。

## 4. 当前阻断：构建缓存，而非新的编译错误

隔离验证 worktree：由维护操作者显式提供并独立确认的项目工作区路径；不在公开文档中保存个人目录。

2026-10-20 23:55:03.9064499Z 的 core-lib-lycheedev_core-ignored-list 在 Cargo 启动前被 disk.cache_quota 拒绝：wrapperExit=1，Cargo exit 不存在；launch.owner 是预定标签，不是创建成功的 Cargo owner UUID。没有 owner/scratch/receipt 创建。最后观测进程和 leases 都为 0/0，接手时必须重核。

缓存 19,718,385,845 B，加 2 GiB 准入预留超过 20 GiB；不提高上限，不禁检查，不改 one-shot。已只读盘点：
- target/debug/incremental：6,819,246,069 B，此一次性方案的唯一候选清理范围；执行须有该工作区所有者的明确授权。
- deps：11,771,416,148 B，其中 109 PDB / 57 EXE 全保留。
- build：661,142,003 B；direct：462,976,512 B；target-symbols 不存在。
- 全部 reparse=0；缓存为该任务复用，但不能泛化为任意目录可删。
- 112 封存 manifest / 2135 引用文件已逐 hash 核对，均在缓存外。失败阶段六文件存在但尚无 seal，整批缺 final。

先核现状，不把这一小时前的进程/路径结论当实时事实。只允许维护脚本固定路径，其他源码、deps、EXE/PDB、原始 logs、longtype、已封证据和用户成果全部保留。清理完成也不是业务通过。

## 5. 维护包的安全用法

maintenance/ 下两个脚本来自云端实现，独立静态审查已通过，Node --check 通过，Windows 实际执行 NOT_RUN。不要本机临时改脚本以绕过拒绝。

1. 暂停测试调度，重核没有 producer/consumer。确认脚本 SHA 与 SHA256SUMS 匹配。固定 cfdef HEAD，保留工作区。
2. 执行 node <本目录>/maintenance/seal-blocked-retirement-input.mjs --worktree <已确认的工作区绝对路径>。它验证 112/2135 和六失败文件真实终态，创建独立 blocked receipts 和 retirement-input.json，绝不覆盖旧 inventory 根缺失的 final 文件。
3. 取得上步返回的实际 input 路径后，执行 node <本目录>/maintenance/retire-authorized-incremental.mjs --worktree <同一工作区绝对路径> --execute-authorized <input路径>。必须已有该指定路径的清理授权；若路径/大小/HEAD/进程/证据与固定快照不同，脚本必须停止并重新核查，不能扩大范围。
4. 脚本持有普通及 symbols 两个原格式 lease，核 native reparse、活动进程、准备期不可变 receipt；只删固定 incremental 逐文件清单。全部其他 cache 文件和证据前后 SHA 核对，两 lease 释放并最终 receipt 成功后才报告 retired。
5. 硬中断或持久化失败可能只剩 prepared，结果为不确定，先只读对账，不盲重跑。静态 reparse 检查不防恶意并发替换，仅适用于合作式、真正暂停的隔离目录。
6. 成功后保留 receipt，执行既有 node <同一工作区>/tools/rust-disk.mjs inspect <同一工作区>，再次核 leases/process。仍使用原 20 GiB/2 GiB 准入，不删额外目录。完整维护边界见 maintenance/README.md。

## 6. 后续队列与交接边界

见 VALIDATION-QUEUE.md。恢复缺失枚举后按组立即执行并封存，避免先把所有 target codegen 撑满再执行零用例；所有目标仍必须覆盖。保留每个失败/timeout/清理结果、实际选择的case和ignored理由。不得把部分枚举或零case算成功。

先 correctness、Source/Live/Skill 与资源生命周期，再 Release/安装验收，再已有 paired 性能协议与最终审计。没有业务门禁不能启动性能。所有本机 Rust 命令走官方 owned runner、600秒/default stack/--locked/--offline/-j1/no-one-shot，不直接cargo绕过。现有错误逐根因修，不抬限额、不删断言、不混入无关改造。

后续开发由指定接手方负责；其他实现者不得并发改产品。不要自行扩大公开上传数据范围，不上传原始日志/凭证。当前代码在 GitHub，无需从旧丢失候选恢复。交接包本身只保存必要开发成果。

## 7. 本公开版本与先前私有交接包的区别

公开脚本使用必填的 --worktree 绝对路径参数，不含个人硬编码路径或会话编号；input.root 必须等于所选工作区 realpath。仍锁定 cfdefd46 维护基线、target/debug/incremental、原盘点字节数、112/2135 证据集合和发布 runner hashes。两个脚本的新 SHA 见本目录 SHA256SUMS，不与先前私有 ZIP 的脚本或 ZIP SHA 混用。本次仅文档与维护工具变更，不改变产品功能或 Skill 源码。维护工作区必须仍为 cfdefd46；若已拉取后续提交，应停下重新核定维护前提，不强制 reset 覆盖改动。
