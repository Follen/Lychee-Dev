# Lychee Dev mailbox protocol v1

日期：2026-10-04。状态：本轮用户指定的新架构与实施合同；实现、离线测试、游戏只读观察、游戏原生写入验收分别记账。

本合同是当前分支 live mailbox 的目标权威合同，替代本分支之前的 duplex 设计。它不改变已发布版本的历史事实，不表示已经发布或完成实机验收。通用安全、客户端选择、安装与发行规则仍由 `design.md`、`regression.md` 和当前 release 合同约束；实际完成范围写入 `implementation-status.md`。

核心模型只有三部分：CLI 反复填写一个固定的小数据表，插件维护唯一当前命令，CLI 读取插件发布的状态与结果。**1 MiB 是每条命令的逻辑上限，不是常驻数字表的大小。** 没有命令队列、256 个常驻数据行或旧传输回退。

## 1. 名称与兼容范围

公开协议名为 **Lychee Dev mailbox protocol v1**，schema 为 `lycheedev.mailbox.v1`。旧 `lycheedev.duplex.v1`、旧 magic、旧 hash domain、旧 256 行布局以及更早的 LoD/按键/色块输入协议全部拒绝，不做兼容推断。

| 对象 | v1 标识或约束 |
| --- | --- |
| mailbox 与状态 schema | `lycheedev.mailbox.v1` |
| mailbox 与 sendbox 的 `layoutId` | `single-data-row-v1`；恰好一个数据行 |
| 320 字节头 magic | `LYCMBX01` |
| 状态封装 magic | `LYCMSB01` |
| 独立结果页编码工具 magic | `LYCMRP01` |
| hash domain | `LYCMBX/header/v1\0`、`LYCMBX/frame/v1\0`、`LYCMBX/request/v1\0`；actor/reuse 等辅助 domain 同样使用 `LYCMBX/.../v1\0` |

内部既有 Go 包名、Lua 模块名或函数名可以暂时保留；这不授予旧协议兼容性。不得只替换文档标题，却继续接受旧 schema 或旧摘要。动态 Go/Lua fixtures、安装包与游戏加载的 addon 必须使用同一新协议。

布局身份、schema 和精确形状都必须验证。不能因为旧 mailbox 恰好也有 `frames[1]` 就把它识别为新布局。迁移通过新的干净托管安装与 runtime 激活完成；旧会话证据保留为旧会话，不转换为可重发的新命令。

## 2. 三个职责清楚的区域

| 区域 | 写入方 | 读取方 | 内容与上限 |
| --- | --- | --- | --- |
| 数据表 | CLI | 插件 | 一个固定数字行：320 字节头 + 4096 字节 payload，1104 个 uint32 数值单元 |
| 控制区 | CLI | 插件 | 七个独立小行：bind/repair、close、cancel、commit、result ACK、reload、lease；每行暂保留 320 + 1024 字节容量 |
| 状态/结果区 | 插件 | CLI | 当前阶段、身份、进度、控制回执、心跳、终端 manifest，以及最多 32 个 16 KiB 结果块 |

现有对象路径可保留为 `inbox.request.frames[1]`，但 `frames` 必须恰好只有一行。它只是单个 data 表的寻址路径，**逻辑 `frameIndex` 不再作为 Lua 数组下标**。Go publisher 和 Lua reader 始终访问该行；`frameIndex` 仍可取 1–256，用来说明它属于命令的第几段。

控制区在产品概念上是一个区，内部保留七个小 lane，不增加控制队列。各 lane 有独立的 host 写租约与回执，close/cancel 优先于新业务 commit。取消尚未得到回执时，disconnect 仍能独立推进；整条命令的数据传输也不能占住控制入口。

公开表是交换数据的地方，不能成为插件的权威状态。插件只从完整校验后的快照更新私有状态。公开 `ready`、进度、结果和回执由插件私有状态投影，CLI 不能通过改一个布尔值使命令变为可执行。

## 3. command、SHA、ready、time、seq 的分工

用户需要理解的是：命令是什么、是否写完、是否已接受、是否正在执行、结果是否已取回。下面的字段共同实现这些含义，不能相互替代。

| 字段 | 回答的问题 | 不能证明什么 |
| --- | --- | --- |
| `requestId` + 完整请求 SHA256 | 是哪一条不可变命令 | 不能单独证明它是新请求或已经执行 |
| runtime、arena、session、owner、actor binding、fence | 属于哪个进程 runtime、哪一代存储、哪个连接和角色 | 不能锁住 Lua 对象的物理寿命 |
| `requestSeq` | 当前连接内的命令顺序 | 不是时间，也不是可复用内存的许可 |
| `frameIndex`、总长度、总帧数、transport attempt | 是当前命令的哪段输入 | 不表示该段已经完整写入 |
| `publicationSeq` 与 begin/end 发布标记 | 这次发布是否完整、是否已经处理过 | 不能让多次 WPM 变成原子事务 |
| header/frame/request SHA256 | 所读元数据、分片、完整命令是否相符 | 不提供目标地址寿命，也不替代归属验证 |
| `createdUtcMillis` 与预算 | 创建时间、诊断、超时策略 | 不作唯一 ID、不作提交标记；时钟跳变不产生新请求 |
| `ready` / 细分 readiness | 插件当前允许什么动作 | 一个 `ready=true` 不是所有写入的通行证 |

保留既有 320 字节头字段布局与 uint64 高低字表示，但所有 magic 和 domain 使用本合同的新值。摘要必须包含原本约定的身份、长度、序号和逻辑分片元数据，不能退化为只对 command 文本计算一个 SHA。

帧摘要覆盖头部前 232 字节及实际 payload；头摘要覆盖规范化的完整头部，其中 header SHA 和发布 begin/end 区间清零；请求摘要覆盖 request ID、actor binding、不可变执行预算、创建时间、总长度和完整源文本。保留固定 reserved 字节及零 padding 验证，禁止未定义字段参与执行。

`ready` 是业务准入的简化投影。实现仍区分 transport、actor、business、control readiness。角色未进入世界时，不接收新 bind/frame/commit；只要 runtime 与控制归属仍有效，取消、结果 ACK、断开连接可以继续。观察到 reload、teardown 或生命周期未知时，所有 native 写入都停止。

## 4. 一条命令如何通过同一个表

1. CLI 固定目标、连接身份、request ID、请求序号、总长度、SHA 和预算，将不可变输入与发布 intent 写入本地日志。
2. CLI 取得 business driver 与 data lane 的唯一写权限，把第 1 帧写入唯一 data 行。先发布 odd/incomplete 标记，写入头部、payload 与 padding，最后按既定顺序发布匹配的 even begin/end。
3. 插件先读取少量 magic/发布标记。全零、未完成、未变化的已处理 publication 不进入完整解码。候选新发布才读取完整快照，复核前后头/发布标记一致，再校验形状、身份、长度、顺序和所有 SHA。
4. 插件将 payload **复制为私有不可变字符串**，更新私有滚动请求摘要与连续接收进度，然后发布这一帧的接受事实。未复制完成前不能发布接受。
5. CLI 验证帧接受事实属于同一 runtime/arena/session/request/attempt 和逻辑帧号，且上次 Publish 已退出、写入事实已落盘，才覆盖同一物理行发送下一帧。不能把“写出了字节”当作帧 ACK。
6. 最后一个分片到达后，插件校验完整长度与请求 SHA，拼接并编译源文本。编译不会执行。成功后进入 `prepared`，产生只绑定该请求的私有 challenge。
7. CLI 的独立 commit 必须匹配当前请求和 challenge。插件再次检查角色、关闭状态与 challenge 有效期，然后执行一次。
8. 执行结束后，插件发布有界结果、结果摘要和资源清理状态。CLI 验证所有结果块及完整结果，将结果持久化后发送精确 result ACK。
9. 插件收到正确 ACK 且执行资源已释放，清除当前请求/编译闭包/结果块，仅保留最小 release 记录。CLI 观察到对应 RELEASED，才允许下一条命令。

### 两种 ACK 不得混淆

- **帧 ACK**：该分片已经进入插件私有缓冲区，允许复用 data 行。当前命令尚可继续传输，不能因此开始下一条命令。
- **结果 ACK**：CLI 已校验并持久保存这条命令的结果，允许释放其结果和当前命令槽位。必须随后观察到插件的精确 release。

一次命令仍然最多 1,048,576 字节、256 个逻辑帧；一次传输只有一个物理 data 行、一个未确认的数据发布。不用扩大窗口或新增队列来补偿内存设计。

### 新发布、未完成、重复与未知

| 观察 | 处理 |
| --- | --- |
| begin/end 不匹配、odd、短读或两次快照不一致 | 不接受、不执行；等待或按预算返回 pending |
| 完整且符合当前身份、预期 request/attempt、下一帧号 | 校验后只追加一次 |
| 已接受的相同 publication/逻辑帧 | 不再追加或执行；保留原接受事实 |
| 旧序号、旧 runtime/arena/session、不同内容的重放、乱序帧 | 拒绝，不修改私有累计输入或执行状态 |
| native 写入 partial/unknown，尚无精确接受事实 | 只恢复观察原 intent；不自动覆盖同一行重试 |
| 原 runtime 消失且没有终端证据 | 结果与执行保持 unavailable/unknown，不在新 runtime 重跑 |

空闲快路只能决定“不处理”，不能凭一个变化的 timestamp 或 ready 位直接接受输入。若攻击性或损坏的公开内容保持同一发布标记，快路不会把它变成新工作；再次进入完整解码时仍须完成全部验证。接受水位只在完整私有复制成功后推进。

单槽复用依赖真实的 stop-and-wait，不仅是 Lua 检查。上一 Publish 必须已完成/停止，host driver 与 lane 租约必须保证没有迟到的旧 writer，再写下一帧。checksum 可以拒绝覆盖造成的坏数据，却不能补回被迟到 writer 覆盖的新帧。

## 5. 插件的最小私有状态

插件持有一个 current record；没有历史命令数组，也不把所有 frame header 永久保留。

| 状态 | 保留内容 | 可推进动作 |
| --- | --- | --- |
| `idle` | 连接身份、序号水位、必要 release 记录 | 接受下一条命令的首帧 |
| `receiving` | 当前不可变 manifest、attempt、连续帧水位、字符串片段、滚动 SHA | 接收下一帧；独立 cancel/close |
| `prepared` | 已校验/编译的闭包、challenge、原请求身份与预算 | 匹配的 commit；cancel/close |
| `running` | 当前执行句柄、预算、取消状态 | 发布结果；异步取消/关闭 |
| `result_pending` | 终端 manifest、最多 512 KiB 结果块、资源清理事实 | 校验后 result ACK |
| `released` / `closed` | 最小请求 ID/SHA、连接退役证据 | 新命令或合法的新连接 |
| `fault` / `execution_unknown` | 当前证据与失败原因 | 只允许有证明的恢复/退役，不能推断成功或重放 |

连续分片只需一个 `acceptedThrough` 私有整数；公开进度若继续使用 `acceptedFrames`，可由这个连续前缀按需投影，不保存第二份 256 项权威状态。每个控制 lane 只保留当前回执、发布水位和必要 fingerprint。

结果通过 manifest 的 request ID/SHA、完整结果 SHA、逐块 SHA、长度与页数校验。当前共享表可继续发布最多 32 个 16 KiB raw chunks；独立 page-wrapper 编码工具如保留，其 magic 必须是 `LYCMRP01`，但未参与实际传输的 helper 测试不能算作结果路径验收。

同步 Lua 不会因 CLI 超时自动停止，不能被本协议承诺为可抢占。异步资源取消后须确认释放。等待期限、传输期限、prepared challenge 期限、执行预算是不同事实；恢复不会重置已经开始的执行预算。

disconnect 先阻止新业务，再取消/收集/ACK/释放已存在请求。只有实际 release、所有 host writers 已排空、原连接身份吻合，才退役 owner。新 owner 必须证明准确的旧 owner/session/fence/released request；不得单靠 idle/ready 猜测可接管。

## 6. 原生 reload 与进入世界状态

原生状态读取先于 Lua heap 遍历，doctor 即使遇到 addon 不存在或旧 schema，也应独立报告 native 状态。固定 PID、创建时间、可执行文件 hash、build/product 与模块布局；当前指令 anchor 和 RIP 数据目标必须通过验证。未知 build 可以尝试有界唯一模式定位，但未验证语义与真实行为的跨 build 结果不能自动获得 writer 资格。

当前 Retail 12.1.0.69933 已观察到的状态含义：

| 原生观察 | 含义与准入 |
| --- | --- |
| mode word bit 4 | 来自 `IsPlayerInWorld` getter；缺失/歧义为 unknown，不能用登录状态替代 |
| bit 7 | GameUI reload 请求已提出；停止写入 |
| bit 8 | reload worker 正在执行；即使 bit 7 已清除也停止写入 |
| Glue reload pending byte | Glue 分支的 reload 请求；停止写入 |
| bit 1 | teardown candidate；保守拒绝写入，不声称覆盖全部 logout 路径 |
| bit 9 | GameUI 模式候选；不是 world-ready 证明 |

用户手动 reload 的同进程只读观察已经记录：Lua 根更换期间 bit 8 为真，bit 4 先清除又恢复，随后 bit 8 才清除。因此 world-ready 与 no-reload 必须同时检查。见 [本 build 只读研究记录](research/reload-state-69933/README.md)。这段观察不证明下一次 WPM 时地址仍存活。

新 bind/frame/commit 要求 `world_ready`、没有 reload/teardown、有效角色与 runtime。cancel/close/result ACK/lease/repair 不要求角色已进入世界，但仍要求目标 runtime、控制归属与原生生命周期允许写入。reload 使用准备 challenge 和记录后的 commit，先保存/释放原请求并排空 writers；新 runtime 只用于原会话退役与重新连接，不自动继续原命令。

## 7. GC、固定存储与 repair

1. **私有强引用。** 当前 data 行、每个 control 行、calibration 以及父表分别由 addon 闭包内的私有 roots 持有。公开父表被替换，不能使仍可能被 host 使用的叶子行失去根。roots 是重复引用，不是内容副本。
2. **先完整分配，再发布。** 数字行在 Lua 内一次建立为固定形状，所有单元是普通数字。发布后不 append、删除、改 metatable 或主动 resize。host 只改已验证 TValue 的 8 字节数值 payload，不改 tag、指针、表头或 GC 元数据。
3. **公开内容不可信。** 每次候选输入的 shape、数值类型、secret 值、身份、头和 payload 都验证。protected UI、Blizzard 对象和 secret 值不进入外部 writer 的可写集合。冻结表如经客户端验证可阻止普通 Lua mutation，可作为额外保护；它不代替原生寿命临界区。
4. **有界 repair。** 损坏使当前 arena generation 失效。最多建立一个新小 arena，并保留一个 retired arena 及其叶子 roots。只有 host 排空全部八个物理写 lane、addon 接受精确 repair proof，才释放旧 roots。超时、一个公开 `repaired=true` 或 root 仍非零都不构成释放许可。第二次未解决损坏进入 quarantine，不无限堆积代际。
5. **不自动推断未执行。** 只在私有 ledger 保留同一请求的 not-started 事实、身份和新 attempt 的严格转换成立时允许修复传输。运行中或执行事实未知时不重送。
6. **disable 不丢证据。** 初次禁用不分配 arena、不创建 transport ticker。启用后的关闭先撤销新输入；有未完成资源或 result ACK 义务时保留最小状态，不能假称已经完全清理。停止采样后也不能在 writer 未排空时释放其可能持有的行。小 arena 可以保留到准确退役或 VM 正常销毁。

这些规则解决普通可达性、公开 topology 损坏与本工具之间的 writer 竞争。它们本身不证明 Lua backing array 不会被其他 native 路径移动，也不能阻止整个 VM 被 reload/logout 销毁。

## 8. 真正闭合 native write 的生命周期窗口

### 8.1 必须提供的保证

允许一次 WPM 的条件不只是“刚才读到的字段正确”。从**重新解析目标对象开始，直到最后一次 WPM 确认完成**，必须有目标侧可执行的机制保证：

- 该 Lua VM、Table 与数值 backing array 此刻确实存活；停止点不能已经处于部分销毁状态。
- 所有可能 free、resize、move 或销毁这些对象的参与者都无法在这个区间运行。
- 区间结束前没有尚未完成、可能迟到的 host 写入；取消或 watchdog 不能先恢复目标、再让旧 writer 继续。
- 授权绑定固定 PID/创建实例、可执行文件、runtime、arena、允许的行/单元和一次有界发布；不能序列化为 JSON 或复用为下次写入许可。

把它封装在 native writer module 内的 `WithWritableArena` 一类私有 interface 后面。调用者提供已校验的协议消息，不能提供任意地址，也不能传一个永远返回成功的 guard 取得能力。profile 的 layout 证明、reload/world 读门禁与这个 lifetime grant 三者独立，缺一不写。

现有 Windows 自有进程实验已证明：同一 PID、同一 writable mapping、同一数字 tag，即使所有普通检查通过，allocator 在最后一次观察后复用子块，WPM 仍可覆盖新对象。重复读取、SHA、时间戳、challenge、OS 文件租约、强引用和 `table.freeze` 均不单独关闭这个 ABA 窗口。[WriteProcessMemory](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-writeprocessmemory) 的 interface 接受地址与长度，没有 Lua 分配身份。

### 8.2 可真正满足条件的机制

**机制 A：目标侧拥有生命周期的入口。** 由目标支持的 native 消费入口或原生 rendezvous 在活的 Lua VM 内持有对象生命周期，取回 host 数据后由目标完成赋值，或持有所有 teardown/resize 参与者遵守的锁，直到 host 明确排空。这样归属者能够把接收、释放和 VM 销毁排成顺序。

当前没有已验证的 WoW addon API 能提供这种 native lifetime lock 或读取任意外部 shared-memory 数据。一个普通 Lua `ready`/ack 握手、ReloadUI wrapper 或有限时长 busy-loop 不满足条件：引擎和其他入口可绕过 wrapper；超时释放后仍可能到达迟到 WPM；无限等待则不能处理 host 死亡。不得为闭合证明虚构此能力，也不在本轮引入注入 DLL、代码补丁、指针伪造或旧输入回退。

**机制 B：已验证 VM 安全点上的 OS 全线程停止临界区。** Windows 在 pending debugging event 期间停止该进程全部线程，直到 `ContinueDebugEvent`；新线程也在进入用户态前产生事件。这是可以排除停止期间用户态 allocator/teardown 并发的原生机制。[Windows debugging events](https://learn.microsoft.com/en-us/windows/win32/debug/debugging-events)

但任意时刻 attach 或暂停仍不够：它可能把 VM 停在对象已经失效、旧 root 字节尚未清除的半途。因此可评估的具体研究路径是：

1. 对指定 build 找到、证明并到达**活的 addon Lua 调用中的 native 安全点**，其栈/私有 nonce 明确绑定当前 mailbox；该点没有正在执行的 teardown/GC/resize，也不重入或 yield。一个经验证的无副作用 native API 调用点可以作为研究候选，尚无现成合格配方。
2. 如果需要硬件执行断点或线程上下文变更来在该点产生 debug event，先获得单独明确授权，并验证当前 executable 的调用语义与安全点身份。不能以给任意线程打一个断点代替这一步。
3. 由专用 helper 的同一 debugger 线程持有 pending event。**停止以后重新解析** runtime、arena、行和数值单元；不沿用停顿前缓存地址。未知、退出事件或安全点不匹配时零写并退出该次尝试。
4. 在同一个临界区内写完一个有界 publication，仅修改数值 payload，读回并记录结果；不在临界区内执行 Lua、不等待插件 ACK、不做网络、打包或无界扫描。
5. 最后一次 WPM 已结束且本次写权限不可再使用之后，才继续事件/恢复目标。插件恢复运行后再校验消息并发布 ACK。用户的 reload 若随后销毁 runtime，host 保留该次输入的不确定状态，不重放到新 VM。

这条路径是**待验证候选，不是已经合格的 writer**。它需要 owner 明确接受 debug attach、短暂停顿，以及如确需使用的硬件断点/线程上下文操作；当前“更新协议并做游戏验证”的任务不自动批准这些具体控制手段。未取得许可前不 attach、不暂停、不设置断点。

helper 必须处理 attach/退出失败，不能把游戏存活寄托在主 CLI 正常返回。Windows 默认 debugger 线程退出会终止 debuggee，且 `DebugSetProcessKillOnExit(false)` 要求调用线程先建立过 debug connection；若研究此方案，可先在同一线程对自有无害 child 建立连接并设置 detach-on-exit，再接入指定游戏进程，避免游戏 attach 到设置保护之间的空窗。这仍需用自有进程验证故障恢复，不是仅调用一个 API 就完成保证。[DebugActiveProcess](https://learn.microsoft.com/en-us/windows/win32/api/debugapi/nf-debugapi-debugactiveprocess)、[DebugSetProcessKillOnExit](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-debugsetprocesskillonexit)

独立 watchdog 必须在 helper 卡住/崩溃时恢复目标，并证明恢复前旧 writer 已停止；不能直接让 watchdog 解除暂停而保留还能继续写的线程。故障注入须覆盖 syscall/写入/日志/恢复各阶段。暂停预算是经过测试的操作期限，不把 Windows 调度描述为硬实时保证。若无法证明 helper 死亡与最后一次 kernel write 的顺序，同样不能把该方案标为合格。

直接枚举线程逐个 `SuspendThread` 存在新线程及中途状态问题，不作为本合同的实现。`VirtualLock` 或仅锁物理页不拥有 Lua 子分配；另分配一块远程内存也不会自动给 Lua 提供合法读入口。这些都不能替代上述 lifetime grant。

### 8.3 实施决策与实际验证范围

在 lifetime 机制尚未选定、授权和证明之前，可以完成新协议、单槽内存改造、离线互通、自有进程实验、真实 addon 安装以及只读游戏观察；**不能把 profile 改为 true 以便先运行一个探针**。所有 profile 继续 false 是当前能力事实，不是本架构的最终产品目标。

要完成真正的 CLI→游戏执行验收，下一项决策是选择并批准一个可证明的寿命机制。推荐先用自有 Windows helper 验证机制 B 的停止/故障恢复骨架；在实际游戏上，只有特定 build 的安全点与生命周期证明通过后才授予该单一 profile，其他 build 不继承。若 owner 不接受该机制，必须换成目标侧拥有生命周期的输入入口；不能同时保持所有旧限制又宣称纯外部 WPM 已获得原本不存在的保证。

## 9. 内存与空闲工作预算

旧方案的 256 × 1104 数值单元，仅按已知 Retail TValue 24 字节就至少 6.47 MiB；若每行 backing 扩容到 2048，则数据行约 12 MiB。一个 1 MiB payload 大数字表同样有约 6 MiB 下限，加上 header 后跨容量边界可接近 12 MiB。把很多表合成一个大表没有解决这种放大。

新方案预计：单 data backing 约 48 KiB；七个 control backing 约 84 KiB；物理交换区合计约 132 KiB，另有少量 table header、roots 和状态。repair 双代约 264 KiB physical backing。若空闲快路为逐单元比较保存完整数字 shadow，所有 lane 用过后还会增加每代约 132 KiB；它也要计入固定保留，不能只测全零邮箱。weak-key shadow 不得反向引用被观测的表，否则 retired generation 仍不能回收。以上是按容量推算的预算，不是尚未进行的游戏测量。

接收 1 MiB 命令时私有片段累计最多 1 MiB，最终拼接阶段可能同时保留片段和连续源字符串，约 2 MiB 文本再加编译产物。编译后释放源片段/连续源引用；完成后释放闭包；待 ACK 结果最多 512 KiB。probe 自己建立的全局对象单独计量，不冒充 transport retention。

已发现的空闲分配包括：20 Hz 重复把八个空/旧行的 80 个 header words 转为字符串、运行时每 tick 两次完整 Snapshot 深拷贝、重复创建常量表，以及心跳的 JSON/SHA 临时对象。整改顺序为：

1. 零/不完整/已处理 publication 的无分配数值快路；只对新候选做完整解码与 hash。
2. tick 查询私有标量状态，完整 Snapshot 仅用于诊断/实际状态发布；常量不在 tick 内重建。
3. 心跳与状态转换分开计量，复用内部计算 scratch；保持状态摘要与身份完整，不以省内存为由删除校验。
4. disabled 时没有 arena 首次分配、OnUpdate、timer、event 或 hook；enabled 时仅运行本 mailbox 所需的有界采样。不得对空闲邮箱反复创建对象，也不得用周期全局 `collectgarbage` 隐藏问题。

首轮验收预算：所有 lane 已使用后的单代 arena、shadow 与 transport 私有空闲状态，full-GC 后增量目标不超过 384 KiB；repair 双代不超过 768 KiB，均排除已有 addon 静态代码/目录及正在保留的命令/结果。无新 publication 且不发布心跳的 1000 个 poll 不应产生随 lane 数量线性增长的 header/payload 分配；带真实 CaptureWriter 的持续空闲分配目标低于 64 KiB/s，并单独报告 heartbeat 成本与峰值。这些是要验证的目标，超标要解释和修正，不能写成既有实测结果。

现有 standalone Lua 的旧 arena/空闲垃圾测量不是 WoW 实际内存值；用户看到的约 70 MB 尚未被本合同解释或解决。实机报告必须同时列出启用前、启用后、稳定空闲、full-GC 后、最大输入、结果待 ACK、ACK 后和 repair 两代的数据，区分 retained、allocation rate、peak 与整个 addon 的统计。

## 10. 验证与交付条件

| 层次 | 必须验证的内容 | 不能外推的结论 |
| --- | --- | --- |
| Go/Lua 协议 fixtures | 新 schema/magic/domain 严格拒旧；1 KiB、4 KiB 边界、1 MiB、超限；单行反复复用；新/旧/乱序/损坏/未完成 publication；双 ACK；challenge；cancel/close 独立；结果落盘先于 ACK | 不证明游戏内存 ABI 或 lifetime |
| runtime/内存 fixtures | disabled 零采样；真实 encoder 空闲分配；140/1000 请求回收；private leaf roots；损坏与双代 repair/drain；重复关闭与新绑定 | 不证明真实 WoW peak 或所有 native mutation |
| 自有 Windows 进程 | 原有 ABA 反例必须保留；候选 lifetime 机制下 allocator/teardown 不得穿过临界区；helper/CLI/watchdog 故障时无迟到写，目标可恢复 | 不直接授予游戏 profile |
| 游戏只读与安装 | 干净 release 安装、手动 reload 后新 schema/布局/心跳；实际单槽与数字 ABI；native world/reload；真实内存数据 | 不算 CLI probe 执行成功 |
| 合格 profile 的游戏执行 | 固定目标的一条小命令、最大输入、异步取消、结果 durability/ACK、disconnect、同 build 双实例、reload/退出中断、repair、内存与暂停/传输时延 | 不自动覆盖其他 build/客户端 |

必要构建与测试遵守仓库规定：`go build ./...`、`go vet ./...`、受影响包、强制 Lua 5.1 的 uncached 全套测试、baseline、版本/skill/包装检查。真实游戏动作在 owner 已批准的范围内进行；暂停/调试候选另按第 8 节取得具体授权。所有 `not_run` 保持原样。

交付报告分开陈述“文档已确定”“实现已完成”“离线通过”“实际只读通过”“native writer 已资格验证”“游戏执行通过”。不得用一个成功安装、一个稳定心跳、一次手动 reload 或一个新 profile 布尔值代替整条执行链的证明。
