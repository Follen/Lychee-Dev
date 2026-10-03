# 双向 Mailbox 的 Skill 编排方案

日期：2026-10-04。状态：**下文保留设计阶段记录；当前候选已实现协议和 Skill 编排，未实机验收**。
当前实现已经更新 Go/Lua 协议、命令表与版本化 Skill，删除 LoD 执行、按键和色块路径。
仍未改变发布版本或安装中的 Skill。定向 doctor 与 writer 拒绝原因见
[实施合同](live-duplex-implementation-2026-10-04.md)，不把下文 3.1.1 历史描述用于新候选。

分支目标：只采用双向 mailbox，删除全部 LoD 输入及槽位运行依赖，不保留
LoD 备用、自动降级或旧协议兼容。以下 3.1.1 描述只界定已安装工具的事实，
不把旧运行方式列为本分支的新版本方案。目标能力不足时明确不可用，不回退 LoD。

## 当前可用事实与边界

3.1.1 的 doctor 是只读聚合检查：workspace、target catalog、捆绑 LuaLS，
以及最近项目声明 product 时的 source mirror。现有命令没有按 PID、installation、
CON 或 scope 定向的 doctor 参数，也没有 sendbox ready、heartbeat freshness、
活动业务或已加载运行时的检查结果。`healthy: true` 不是游戏可执行的证明。
事实来源是 `internal/command/command_contract.go`、
`internal/command/dispatch_workspace.go` 与生成的
`skills/lycheedev/references/commands.md`。

现有 native live 仍通过 build 绑定 Lua 根、公开 Mailbox、受管 200 槽及当前输入
协议完成身份、执行与恢复。研究用数字 inbox 的局部实测不能推广为生产协议已可用，
也不能推广到未验证的 build、多个进程或其他客户端。

实际 Skill 入口增加一条共用规则：每次可能驱动游戏的 connect、execute、resume、
reload（含 fallback）、cancel、disconnect 调用前，先在相同项目与 workspace
上下文运行现有 doctor。上一次成功检查不能替代下一次。详细决策集中在
`skills/lycheedev/references/live-doctor.md`，不复制到所有 live 引用。
只读 status、历史和保留结果不要求 drive preflight；source/data 授权保持原范围。

## doctor 的两阶段演进

当前阶段只做可运行的 Skill 编排，不伪造目标检查。doctor 的 general error/warn
应保留并解释；LuaLS/source 问题不是已授权 cancel、disconnect 或无输入恢复的
通用阻断。真正所需的身份、ownership、协议资格和 continuation 仍由现有 live
命令校验。诊断不能授权自动安装、升级、reload、删除 claim，或重发未知输入。

未来定向 doctor 的输入拟包含固定项目/CON、installation、PID/创建时间、build、
runtime、actor 及请求身份。这些**拟议参数和结果当前不可用**，不加入 Skill 命令
示例或生成命令目录。其目标是返回可用 capabilities、具体 blocker 与 continuation，
让 Agent 选择已授权的下一步。能力至少分开描述：读取状态、开始新业务、继续原请求、
cancel/close/ack、验证运行时退役。不能只返回一个全局 ready 布尔值。

原生 CLI 必须对所有可能驱动游戏的入口执行同一目标检查与动作前复核；不能依赖
Agent 恰好运行了独立 doctor。独立结果只是诊断快照，不是可跨调用沿用的执行令牌。
未来定向 doctor 也保持只读：不 bind/claim、不修复文件、不发送输入或 ACK；
capabilities 只说明对应动作是否有已验证条件，不执行该动作。
native enforcement、公开参数及机器可读 schema 需要后续源码实现和验证，本轮不实现。

## 双向区域与单请求生命周期

未来 `Mailbox.inbox` 是 CLI→addon 的数值写入区，`Mailbox.sendbox` 是 addon→CLI
的只读发布区；详细布局和编码以协议总方案为准。Skill 不暴露原始地址、
槽编号、tag、key 或内存写接口；它只表达任务与解释结果。每个方向都需绑定同一
process creation/build/runtime/owner/nonce/request，并包含递增序号、时间采样与
精确字节的 algorithm SHA256。内容摘要不取代身份校验；game time 与 host clock
不可直接比较，freshness 必须由原生协议的递增采样和有界观察规则判定。

用户更新后的单请求源码上限为 1 MiB（1048576 字节），header 和独立控制区另计；
结果仍为独立的 512 KiB 合同。实际生产能力须以已实现 contract/协商为准，
当前 3.1.1 不因方案修改而获得 1 MiB 能力，Skill 不能提前发送超限内容。

同一进程只允许一个数据 writer、一个 outstanding business request。
接收、执行、结果 success/fail、结果落盘、准确 ACK、runtime release 是不同事实。
无论业务 success 或 fail，只有对应结果已验证落盘、准确 ACK 与 released 已确认，
才允许下一项新业务。已验证失败应交付其结果并继续收尾，不能当作结果不存在。
结果未知、ACK 未确认或 release pending 都不允许换 request 跳过原请求。

cancel、close、ack 使用独立控制通道和自己的有界恢复资格，不因业务 busy
被同一门禁永久挡住。busy 阻止新业务；既有控制动作仍要验证准确目标、owner 和
原请求。未来实现使用独立 control journal/lease，不能让长时间持有的数据 driver
锁挡住取消和关闭；控制 writer 只能写自己的地址集合，不能借此接管数据区或
绕过外来 owner。当前版本仍遵守其现有 driver 门禁。重复控制只收敛原事务，
不重复未知业务效果。

连续命令复用固定 arena。完成后的源码、编译 closure、执行环境与受管回调必须
解除引用，已 ACK 的完整结果释放；addon 不保留每个命令的完整历史。Skill 解释
内存时须分开 payload 容量、Lua 布局开销、GC 前临时峰值和探针自身副作用；
不能以“140 次写入”直接算成 140 份 mailbox。连续 140/1000 条的保留量和峰值
属于后续真实验收，不把有界设计当作已无泄漏的证据。

sendbox ready 与新鲜 heartbeat 计划替代颜色采样作为输入就绪证据。移除颜色路径
必须等能力协商、原生检查、失败语义与跨 build 验收通过；目前 Skill 仍遵守现有
hybrid 协议，不宣称纯内存 readiness 已实现，也不允许手工解码颜色来绕过 CLI。

## 状态、界面和目标隔离

诊断需要区分以下事实，不能统称“插件缺失”：受管文件不完整/modified；客户端尚未
进入角色；加载/选择角色导致 runtime 未发布；发布存在但 sample stale；公开布局
或能力不支持；当前已绑定但业务 busy；原请求 cleanup pending。只有明确的文件
检查证据才能支持修复安装，只有明确的运行时证据才能支持连接或控制动作。

界面仅在 addon 的 Executor 已开始且存在 active probe 时跳动，唯一文字为
“Agent执行中”。空闲连接、终端
报告或主机落盘 ACK 不维持活动遮罩或键鼠接管。界面显示既不是执行回执，也不是
清理证明。显示逻辑应消费协议状态，不能反向控制协议。

相同 build 的多个窗口、共享安装的实例以及不同 build 分别绑定完整目标身份。
跨 build 的 Lua 根、私有对象路径、numeric tag/secret/layout 与写入区间资格均要
独立确认；不能从目录名、build hash 或研究地址推断另一个进程可写。实际原生实现
仍需动作前检查、同步意图日志和写后证据，并明确 reload/GC TOCTOU 残余风险。
拟议固定 arena 要由长期强根保留到 runtime 结束；业务 ACK/released 不意味着
释放 arena。ready、SHA256 或双读校验都不是外部写入期间的对象生命周期锁，
即使预先分配和强根保留，也不能宣称所有 GC/reload 时序都已安全。

实际丢失或回收 arena 时，由独立 supervisor/operation ledger 撤销旧区域资格，
重建新对象并发布新的 arenaGeneration 和修复挑战。宿主必须先确认旧 writer
全部停稳，再重新定位、校准和握手；不能继续向缓存地址写入。只有撤权后的
准确 not_started 证明允许以原 request ID/digest 重传，并递增 transportAttempt。
已接受 commit 或已开始执行时，只继续原操作与控制；已有结果时只恢复结果。
ledger、controller 或原 runtime 丢失且原执行状态无法证明时，保留 execution_unknown；
宿主已经验证落盘的结果仍然有效。不能把新邮箱为空当作未执行。
修复不刷新原预算，也不消除已进入内核的旧写入与对象释放之间的竞态。
完整流程与故障上限见 [架构评审第 14 节](live-duplex-architecture-review-2026-10-04.md)。

## Skill 迁移和验收

1. 当前仅修改仓库内简短入口与 live-doctor 引用；未安装或发布。所有示例仅使用现有 command contract。
2. 后续实现定向检查、双向协议与控制通道后，先更新命令表和验证事实，再把已可用
   capabilities/continuation 编排迁入对应 Skill 引用；保持未实现设计与可运行示例分离。
3. 用真实场景验证逐次 doctor、pending 原请求恢复、busy 下控制收尾、角色选择与
   文件缺失区别、不同 build/同 build 多实例隔离、结果 fail 后落盘 ACK/released。
4. 结构校验使用 skill-creator quick_validate；示例校验使用仓库 skill-contract。
   这两项只证明 Skill 结构与命令引用，不证明游戏 readiness 或生产协议可用。

本轮不实施生产协议、不运行游戏、不安装外部或用户 Skill、不发布、不创建新 live
连接。后续生产实现与实机验收必须另行记录，不将本方案的目标当作完成事实。
