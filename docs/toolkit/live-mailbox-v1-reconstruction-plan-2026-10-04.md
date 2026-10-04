# Lychee Dev mailbox protocol v1：完整重构与验收计划

状态：**待审阅的实施计划，不是完成报告**。日期：2026-10-04。实施范围为当前 `codex/duplex-mailbox` 工作树的 Go CLI、游戏内插件、`/dev` 工作台、版本化 Skill 与合同文档。不恢复 LoD、按键、色块、旧槽位或旧 wire 兼容；正式写入不挂起游戏线程，也不安装游戏函数 hook。

本计划以 [v1 协议合同](live-mailbox-v1-architecture-2026-10-04.md)、[direct Retail 实测](live-mailbox-v1-direct-retail-trial-2026-10-04.md) 和[先前挂起写入事故](live-mailbox-v1-retail-trial-2026-10-04.md)为依据。后两份记录不能互相覆盖：挂起路线已禁用；direct 路线在一个精确 Retail 映像上通过了小命令、1 MiB 命令和最终断开，但仍有外部 reload 与写入相撞的瞬态风险。

## 现有证据和目标边界

| 事项 | 当前事实 | 尚不能宣称 |
| --- | --- | --- |
| Retail direct 写入 | `12.1.0.69933`、精确映像、同一真实进程；149 字节与 1 MiB 源码分别在请求创建后 267 ms、19,156 ms 被观察到执行；各自一次 6,293,376 字节 WPM、完整读回、结果与 ACK/close 均验证 | 所有 Retail build 或所有生命周期场景安全 |
| GC | 同一正式服运行时在探针内连续做三次 Lua GC；结果验证、断开成功 | GC 与 WPM 恰好同时发生或长期运行已经验收 |
| reload | 写入前有独立 world/reload 观察；direct 路线不挂起线程 | 写入完成后 reload、新运行时再执行，或最终检查到 WPM 的竞态已验证/消除 |
| 客户端范围 | 本协议 writer 只放行一个精确 Retail 映像 | Classic、Titan、Forever 或未知 build 的 writer 已验收 |
| UI | 已释放结果在 CLI 有证据，但游戏内页面仍提供无法打开的报告；全局 `lastFailure` 被错挂到已确认请求 | 当前工作台能准确表达请求终态 |

目标是**一个协议合同、一套状态事实、一个受限 native 写入边界和可追溯的逐场景资格表**。无需为了“完全重构”改变已正确的 wire 字节或重复实现持久化：当前 journal 已有命令正文压缩/恢复，native trace 保存摘要与长度，不应再将旧“三份 base64”问题列为未修复事实。

## 不可破坏的协议不变量

1. `inbox.command` 是一张可复用、最多 1,048,576 源码字节的行。CLI 对每条业务命令只尝试**一次**整行 WPM；`inbox.stop` 是独立的小控制行；`sendbox` 只由插件发布。命令写入、取消、关闭和显式 reload 是不同意图，不人为拆成 4 KiB 片或常规 bind/commit/ACK 往返。
2. 插件只编译、执行已完整校验的**私有副本**；共享行和 WPM 读回不是执行凭据。候选头的开始/结束标记只帮助发现，不代表 OS 写入原子完成。
3. 一次性挑战只能由有效命令接纳或精确的未接纳取消消费一次。无效新命令不得消耗挑战，也不得删除上一个结果。一个请求只能有一个与证据一致的终态。
4. 旧结果只在有效新命令携带精确前结果 ACK，或最终 ACK/close 到达时释放。取消未接纳的请求须留下 `executionStarted=false` 的结果，直到其自身被确认；仅发出 cancel 不能当作取消成功。
5. 部分写、短读、超时、CLI 崩溃、缺少回执和运行时消失均不能授权重发原命令。执行是否发生与资源是否释放分别记载；续接保留原源码、请求 ID、预算、截止时间和目标身份。
6. PID＋创建时间、映像、runtime、arena、actor、owner/session/fence 均属于身份。地址不能跨其中任何断代复用。同一 build 的不同进程也不能共享命令、租约或结果。
7. 普通正文校验失败可在同一候选上有界重读；只有根、行归属或拓扑真的损坏才触发换代修复。修复前必须确认 command/stop writer 已排空，旧 arena 强引用保留到安全释放；最多 active＋retired 两代，再次损坏则隔离。

威胁模型只覆盖误选目标、陈旧/重复请求、两个遵守协议的 CLI 竞争、混合写入、意外损坏及生命周期变化。能够任意读写游戏内存的同权限恶意程序不在 mailbox 可防范围内；无密钥 SHA256 和 ready challenge 都不是针对它的认证机制。这一点不能成为删除身份、挑战或私有副本完整性校验的理由。

## 单一状态事实与写入流程

请求的私有状态收敛为 `idle → validating → running → sealing → terminal`；`disabled/quarantined` 是运行时可用性，`closeRequested` 是连接事实。`ready_unbound`、`result_pending`、`closing`、`closed` 等对外名称由这些事实和 owner/ACK 状态投影，避免独立布尔字段拼出矛盾状态。`sealing` 表示执行已结束、结果仍在有界编码和封存，不能再显示“Agent执行中”，也不能提前接纳新命令。reload 的 prepare/lease 是连接控制，不混入业务请求状态。

一个正常请求的顺序：

1. CLI 的 doctor 识别精确进程、干净受管插件、角色已进入世界、fresh sendbox、可用 writer profile；取得该**进程实例**的独占 driver claim。`live connect` 只选择并记录，不谎称 addon 已绑定。
2. CLI 根据新鲜挑战、actor、runtime/arena、owner/fence、请求序号、预算和源码长度生成不可变命令；持久化写入意图与源码引用。写前即时重核 PID/映像、Lua 根、类型/冻结/页面、world/reload、sendbox 身份和挑战。
3. CLI 构造完整数值行，调用一次 WPM，读回并记录实际字节数。该阶段没有调试器 attach、线程挂起、磁盘等待或 UI reload。插件发现候选后按帧预算复制并校验，最后一次性消费挑战、处理旧 ACK、登记新请求。
4. 插件给出显式成功、失败或取消的终态，说明 `executionStarted`、结果摘要、资源释放和页信息。CLI 验证全部结果页，**先持久保存**，再允许下一命令携带精确 ACK；最后用 stop 行 ACK/close。
5. 每种失败只能从当前 journal 和 fresh sendbox 恢复。若证据不足，结果保持 unknown；不选另一实例重试，不改 claim 文件来消除阻塞。

业务新写入以 native `IsPlayerInWorld` 结果和 `no_reload_observed` 为门槛。控制消息按其自身权限处理，但仍不得写到已销毁的 VM。Agent 在 CLI 写入窗口内不自行发起 reload，并提醒用户这段时间不要手动 `/reload`；写入结束后正常 reload 不应被永久禁止。用户接受的剩余风险是：外部 reload 恰在**最后检查之后、WPM 期间**开始，强引用也不能保住被销毁的整个 Lua VM。必须在产品文档、Skill 和实测结论中一致说明，不以有限次测试宣称消除。

## 1 MiB 完整性与性能决策

当前 `DuplexProtocol.lua` 的大正文验证每次最多复制 8,192 数值单元，native-bit 路径每次只哈希 2,048 字节。1 MiB 至少约 32 次复制推进、512 次哈希推进和少量收尾推进，即约 **546 帧**；30 FPS 时仅固定调度就约 18.2 秒。19,156 ms 的实测与此相容，**不能直接归因于 SHA256 计算 CPU**。fallback 的 256 字节哈希推进更慢。先测阶段 CPU、墙钟、帧数和 FPS，再改变算法。

**路线 A，先实施：保留 SHA256，重构增量验证器。** stop 优先；复制/数值类型/零尾检查与同一私有字节流哈希按短微批次推进，每批后检查本帧 CPU 时间预算，耗尽则下一帧继续；最终接纳仍重核身份与挑战。起始预算取每帧约 0.5–1 ms 进行真实客户端调参，不把该数值当作已验证上限。sendbox 进度限频，不能每个微批次都重新编码。短命令的固定零尾也须有界检查，不能换来单帧尖峰。编译、执行和结果封存分别测量，不能因为 validator 受限就假定它们也受限。

**路线 B，只有 A 的测量不达标才决策：更换大正文传输完整性校验。** 候选是明确多项式、字节序、覆盖范围及跨语言向量的 64 位快速校验；不选单一 CRC32 作为执行任意 Lua 前唯一终检。Lua 用两个精确的 32 位半字表示，不能把任意 uint64 放进双精度数。校验必须覆盖域分隔、执行相关元数据和完整源码。若采用 B，给 wire 一个新的确定布局/算法标识，字段不再假称 `requestSHA256`；Go 持久源 SHA256 可以保留，但要说明插件私有副本通过的是另一种校验。禁止运行时算法协商和失败后自动降级。头部、结果 ACK、身份、挑战和执行次数检查保持原强度。

性能记录覆盖 0、149、512、513、4 KiB、64 KiB、256 KiB 和 1 MiB；前台 30/60/144 FPS 及后台低 FPS；拆分源码持久化、构造整行、最终门禁、WPM、读回、复制/零尾、哈希、编译、执行、结果读取和持久化。记录 p50/p95、总 CPU、最大单帧增量和插件内存；不能只报告一次最快耗时。初步产品目标是在相同正式服场景下让 1 MiB p95 明显低于当前 19.2 秒、且无可察觉长帧；具体发布阈值须由路线 A 的帧预算实测冻结，未测时不宣称“几毫秒完成命令”。

## 多 build：配方复用与写入资格分离

用结构化 profile 取代孤立的 exact-hash `Eligible` 布尔开关。每份 profile 记录 product/full build/映像 hash、recipe 版本、ABI ID（Table 数组、TValue stride、number/tag/secret、冻结语义）、world/reload 解释、试验模式、CLI/addon commit、通过场景和缺口。资格为当前观测与这些证据的推导结果：

| 级别 | 验证内容 | 权限 |
| --- | --- | --- |
| L0 已识别 | 产品、完整 build、exe hash、模块、PID＋创建时间 | 只读 doctor |
| L1 可定位 | 指令配方唯一匹配、Lua 根与邮箱 typed path、独立 world/reload 状态 | mailbox 只读 |
| L2 writer 条件成立 | 数值布局、数组容量和冻结、GC 强引用、页面及生命周期语义 | 受控 direct 试验候选 |
| L3 精确映像试验通过 | 正式 CLI＋干净插件，小/大命令、读回、执行、结果与 ACK/close | 该精确映像的受限使用 |
| L4 场景验收完成 | 取消、故障恢复、reload/角色、双实例、性能与内存 | 宣称对应场景受支持 |

配方可在新 build **自动定位并给出 L1 候选**；相同 exact hash 的已验证静态条件可复用，但每个新进程仍即时复核 root、actor、arena、页面与生命周期。换 hash 即使 RVA 配方成功，也不能自动继承 L2/L3；更改 stride、secret/tag、freeze 或 reload 语义必须更新具名 ABI/Compat 实现，不能让生产 writer 猜偏移。无匹配、多匹配或语义不明则只读失败并说明缺少哪层证据，不退回堆扫描或旧 RVA。

| 目标 | 当前资格 | 必需验收 |
| --- | --- | --- |
| Retail `12.1.0.69933` 精确 hash | 小/1 MiB/close 的 L3 子集，另有三次 GC 观测 | 补 L4；如实保留 reload 瞬态风险 |
| 同 Retail build 的另一 hash | 未资格化 | 重做 L0–L2，再进行精确映像试验 |
| Classic `50504` | 本协议 writer `not_run` | 独立 L0–L4，检查可能不同的 ABI |
| Titan `38002` | 本协议 writer `not_run` | 独立 L0–L4，检查可能不同的 ABI |
| Forever `16001` | 不在当前验收矩阵 | 保持未验证/只读，除非另行纳入 |
| 未知 build | 可尝试只读配方定位 | 未达对应证据级别前拒绝写入 |

## 同 build 多实例、GC 和 reload 验证

所有真实结果必须附 product/build/hash、CLI/addon commit 与安装收据、PID＋创建时间、runtime/arena、actor GUID、CON、请求/控制消息 ID。另行标注夹具与真实客户端，不能互换。

| 场景 | 操作 | 通过判据 |
| --- | --- | --- |
| 同安装目录 A/B 两真实实例 | 同时 doctor，交替并并发执行不同标记，分别 close | 两套独立 CON/claim/runtime；报告只回目标；关闭 A 不改变 B |
| 同进程两个 CLI/项目 | 同时 connect/execute；分别试不同路径别名 | 只一方获得 driver；另一方零写入，不能靠新项目/CON 绕开 |
| 同请求竞争与恢复 | 两个调用同时 execute/resume；在意图落盘、WPM 后、结果保存后、ACK 前分别中止 driver | 至多一次发布；原请求恢复；unknown 不重发；保存后才允许 ACK |
| 身份断代 | 夹具模拟相同 PID、不同创建时间；真实客户端退出/重进与换角色 | 旧地址、旧 claim、旧挑战均失效，结果不跨进程/角色迁移 |
| GC 与内存 | 空闲、1 MiB 校验前后及已释放后反复 GC；140 条顺序命令和长空闲 | command/stop 可继续使用；active/retired 有界；插件内存不随命令数持续增长 |
| 写入后 reload | 先取得终态并 ACK/close，再经协议 reload，持续只读观察，随后新 CON 再执行 | 旧 runtime/root 失效，新 runtime/actor/arena 重新绑定；旧挑战零执行；worldReady 提前恢复不得绕过 reload 门禁 |
| reload/取消边界 | 校验中与运行中分别触发允许的取消/关闭；受控生命周期夹具注入 teardown | 仅有一个真实终态；旧请求无精确结果时保持 unknown，不能从未见 ACK 推断未执行 |

真实游戏无法稳定制造 PID 复用或最后几微秒的 reload 竞态，分别用确定性 owned-process 夹具测试边界；夹具通过不等于游戏已证明“竞态不存在”。无法启动 Classic、Titan 或第二真实实例时写明 `not_run` 和环境缺口。

## `/dev` 工作台、Skill 与文档

工作台从同一状态投影取数据，不在 UI 猜协议事实。`AutomationView.lua` 当前将全局 `lastFailure` 写进当前任务卡；已确认释放后只剩请求 ID/SHA，页面仍启用“查看报告”并显示“代码未提供”。重构为连接错误、候选拒收、请求终态错误各自带 ID；旧控制错误不得挂到成功请求。released 摘要只保留请求 ID、字节数、摘要、结果状态、是否执行和释放时间，不保留完整源码或结果。没有结果正文则关闭“查看报告”，可提供“执行摘要”；明确提示完整结果保存在 CLI。Mailbox 状态移到标题旁固定连接区域。跳动荔枝只在 `running` 执行探针时显示“Agent执行中”，验证、封存和等待 ACK 各有准确静态文案。中英文 locale 同步，使用真实客户端 WGC 检查就绪、验证中、执行中、结果保留、已释放和连接错误六种画面。

按 `$skill-creator` 的渐进披露原则打磨版本化 `skills/lycheedev/`：主 `SKILL.md` 保留任务路由、身份与 unknown 不重发等必须改变 Agent 决策的约束；doctor、live 启动、执行、reload、恢复等细节放在对应 reference。source/data 既有路线只做冲突与断链检查，不把 live 风险泛化为所有任务。命令和 flag 仍来自 CLI 合同生成。对 Skill 运行 `quick_validate.py`、项目 skill-contract，并用“尚未进入角色”“同 build 双实例”“1 MiB pending”“reload 后续接”“结果不明”做独立前向测试。CLI 自己强制最终写前检查，Skill 不维护第二套可绕开的门禁。

当前分支 `AGENTS.md` 与旧架构段落仍把 stopped writer/首次试验写为现状；实施第一阶段须按 direct 实测统一这些文档。历史事故记录保留，不改写为 direct 成功。当前 Go journal 已修复的正文去重也不再列入待修复项。

## 实施批次与阻断门槛

| 批次 | 交付物 | 进入下一批的证据 |
| --- | --- | --- |
| 0. 冻结事实与合同 | 统一 AGENTS、design/status/acceptance、错误归属、wire/layout 选择和未验证项 | 文档与代码现状一致；旧/新记录明确分离 |
| 1. 协议核心 | Go `duplex` 与 Lua `DuplexProtocol` 共享黄金 wire 向量和状态转移；去掉 frames/bind/commit 的过期公开语义 | 身份、挑战、一次执行、ACK、取消、unknown 的双语言用例通过 |
| 2. 增量验证 | 时间预算 validator、限频进度、阶段性能计量；先保留 SHA | 1 MiB 的 CPU/帧数、p95、最大单帧、内存已测；若不达标才评估路线 B |
| 3. native 与生命周期 | `memory` 负责合格 ABI＋一次 WPM/读回，`duplexhost` 负责 driver/租约/日志；删除生产 direct 路径的 stopped 命名和可达入口；落实 reload/actor 断代与 bounded repair | own-process 故障注入、短写/短读、坏类型/页面、进程替换及 no-replay 测试通过 |
| 4. 多 build 与多实例 | 分层 profile、可复用配方、精确映像资格、进程实例唯一 claim | 按上表逐客户端、逐实例提交真实证据；缺项保持 `not_run` |
| 5. UI 与 Skill | 准确的游戏内投影、双语文案、Skill 路由和 generated command 检查 | 六种真实 UI 画面、Skill 前向测试及合同检查通过 |

每个行为批次跑相关测试、`go build ./...` 和 `go vet ./...`；交付前跑强制 Lua 5.1 的完整 Go 测试、Node baseline、version/skill 合同，真实游戏验收单独归档。任一错误身份导致执行、挑战重复消费、unknown 自动重发、错误 ACK 丢失结果、旧地址跨断代写入、无资格 build 放行、关闭后新接纳或 writer 未排空即释放旧根，均阻断交付。Classic/Titan、真实双实例和 reload 场景缺失时，阻断相应兼容性宣称，不抹掉已验证的 Retail 子集。
