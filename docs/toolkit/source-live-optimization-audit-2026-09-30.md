# Source 与 native live 深度优化审计

日期：2026-09-30。代码基线：`8e4dab0455c8bee512c73912c26b31bc9605f6c2`（3.0.2）。
相关的 [data 审计与 CascLib 对照](data-code-audit-2026-09-30.md) 单独保留。

本轮检查 source 的固定提交索引、搜索、关系、上下文、LuaLS 和缓存容量管理，以及 native live 的内存读取、记录匹配、地址提示、发现和 HEAD/BODY 观察链。
这是优化调查与后续实施建议，没有修改生产代码、执行游戏输入、提交或发布。没有把 receiver/QR 的历史操作规则套到 native 200 槽连接上。

## 结论与证据边界

Source 的首要机会是查询内共享已验证索引，并合并同一文件的 Git 读取；其次才是增加派生索引和复用 LuaLS 分析。
Live 的首要机会是让已知 nonce 的查询保持精确锚点，把提示学习从关键路径拆开；再增加同 runtime 的局部查找层，减少全进程扫描次数。
SIMD 和更多线程不是当前最有证据支持的第一步。

| 编号 | 优先次序 | 证据 | 机会 |
|---|---|---|---|
| S1 | 第一批 | 已复现 | EnsureIndex + Search 重复验证同一索引；同文件的多条结果重复读取 Git blob |
| S2 | 第二批 | 静态确认 | Search/Relate/Context 多次顺序扫描 records.jsonl，缺少查询方向的派生索引 |
| S3 | 第二批 | 静态确认 | incoming/outgoing 语义查询分别启动 LuaLS；可合并同一请求内的分析 |
| S4 | 第三批 | 静态确认 | body 搜索在 Git 输出之后过滤 topic，不相关命中也占输出预算 |
| S5 | 先测量 | 静态确认 | 容量 Grow 多次遍历 source/v1，并持有共享容量锁 |
| L0 | 第一批 | 静态确认 | 当前扫描计数不能完整表示真实进程 I/O，需先补全阶段指标 |
| L1 | 第一批 | 已复现 | hints 非 nil 会把已知 nonce 的扫描换成通用 magic，候选数量可明显放大 |
| L2 | 第二批 | 静态确认 | 新操作的 nonce/ticket 不匹配旧提示，普通观察缺少通用的局部查找层 |
| L3 | 第二批 | 静态确认 + 待验证假设 | 一个操作内重复发现/观察；HEAD 后还可能再次扫描 BODY |
| L4 | 第二批 | 静态确认 | 全扫描预算主要计逻辑范围，缺少覆盖点读/重读的统一物理 I/O 预算 |
| L5 | 先测量 | 静态确认 | 进程验证、区域查询、hint 排序和扫描缓冲分配仍有局部优化空间 |
| L6 | 后续实施决策 | 研究与候选合同已记录 | 先保持格式复用稳定编码；紧凑 input 有条件推进，static beacon/固定地址本轮不采用 |

“第一批”等是建议实施顺序，不代表所有项都是正确性缺陷或都需要发布阻断。
只有 S1 和 L1 做了本轮隔离探针；其余依据调用链与合同。实际客户端时延、RPM 吞吐、游戏帧时间和峰值 RSS 均为 `not_run`。
历史文档中的 9 秒、89 秒等实测属于当时的候选与场景，不能当作当前 3.0.2 的基准。

## 应保留的设计

- Source 继续使用确切提交、parser/runtime/environment 身份和内容摘要；precise 的分层停止、分页、truncation 和 partial 语义不能被性能优化削弱。
- Live 提示仍是不可信的调度数据。PID 创建身份、映像、runtime、nonce、ticket、完整头尾、校验和及业务谓词都要重新验证。
- 只读 private/committed/readable 区域，排除 guard 和模块回退；记录头与完整读取之间的变化检查保留。
- BODY 仍须由已验证 HEAD 的长度与校验和授权。局部未命中、早停和取消都不能证明全进程不存在记录。
- 200 槽分配、prepare/commit、owner/fence、reload 隔离、输入样本新鲜度和持久预算继续遵守现行合同。
- addon 禁用零成本与事件驱动要求不变；不能为了 CLI 加速引入常驻 addon 轮询或未经预算的后台工作。

现行依据：[设计](design.md)、[实施状态](implementation-status.md)、[200 槽合同](live-slot-routing-2026-09-29.md)、[native 输入架构](live-input-architecture-2026-09-28.md)、[输入节奏实现](input-cadence-implementation-2026-09-29.md)。

## Source

### S1：一个查询持有一个已验证快照，并按 blob 合并读取

位置：[EnsureIndex](../../internal/codebase/semantic.go#L159)、[openIndex/verify](../../internal/codebase/index.go#L426)、[Search 结果读取](../../internal/codebase/search.go#L277)、[document](../../internal/codebase/index.go#L521)。

`EnsureIndex` 通过 `openIndex` 完整哈希 records.jsonl；随后的 `Search` 又打开并完整哈希一次。
`Context`/`Relate` 的组合也会分别打开快照。验证中的 `io.Copy` 只在复制完成后检查 context，大索引取消的响应粒度可改善。
搜索结果逐条调用 `document`；真实 Git 路径每次执行一次 `git cat-file blob`，读取、SHA256 和 UTF-8 验证同一文件。

本轮真实 Git fixture 的隔离计数探针：一个文件中 40 个 Widget 声明，执行 EnsureIndex + precise Search，得到 40 条结果，发生 **2 次完整索引哈希、1 次索引扫描、40 次 document 读取**。
计数是在测试 overlay 中添加，不是生产埋点；本用例不证明真实 Blizzard 源码的绝对时延。

建议设一个请求拥有的 source 快照会话：把 Ensure/Open 合成“获取或构建已验证快照”，Search/Relate/Context 内部消费它；按 blob ID/内容摘要缓存已验证文件，并受查询总字节数与文件数预算限制。
先把同文件 40 次读取收敛为一次；跨文件再用 Git batch。已有 [visitDocuments](../../internal/codebase/tree.go#L201) 使用 `cat-file --batch`，可以沿用其身份与 framing 校验经验。

会话必须绑定确切 pin 与已验证文件版本，持有适当句柄/租约并处理发布替换和外部改写；不能把“一次验证”变成任意后续文件内容的授权。
每个新 CLI 请求仍须验证来源，不用 mtime/size 代替内容证明。缓存淘汰只能增加 I/O，不能改变结果。

### S2：让索引服务查询，而不是反复解析完整 JSONL

位置：[Search 全扫描](../../internal/codebase/search.go#L120)、[Relate 两轮扫描](../../internal/codebase/relations.go#L177)、[Context](../../internal/codebase/context.go#L143)、[scan](../../internal/codebase/index.go#L481)。

目前 records.jsonl 同时承载文档、符号、关系和资产；Search 全扫描，Relate 查声明与边，Context 又扫描加载/上下文关系。
单独返回少量结果也仍需处理许多无关 JSON 记录。S1 可以省重复打开，却不能消除这些方向不同的扫描。

建议在现有文件缓存中增加固定提交的派生索引：符号 ID/名称前缀、path→文档与符号、incoming/outgoing、TOC/XML 加载关系；按类别分片或使用经过验证的 offset 索引。
派生文件的 schema、parser revision、完整摘要和覆盖范围进入 manifest，发布时原子封存；缺失时可重建，损坏时遵守现有完整性错误合同。
不能直接退回已退休的 source SQLite 路线，也不能为每次精确查找把全部 JSONL 装入无界 map。

验收需比较精确/前缀/正文的排序和停止层级、重复符号消歧、旧 cursor、truncation、语法失败文档、索引损坏与中途取消。
查询字节数、JSON 解码数和磁盘放大比先于单一 wall time，避免 OS 文件缓存掩盖重复工作。

### S3：合并一次请求内的 LuaLS 工作

位置：[semanticReferences](../../internal/codebase/relations.go#L336)、[semanticOutgoing](../../internal/codebase/relations.go#L395)、[AnalyzeWorkspace](../../internal/luals/lsp.go#L85)。

语义缓存未命中时，incoming/outgoing 分别调用 AnalyzeWorkspace；每次建立临时 definitions/config/log/meta，启动新的 language server 并等待 readiness。
references 已与 hover 合并，outgoing 也会批量提交 definition queries；这些是可继续扩展的现有能力。

建议先将同一固定 worktree/environment/runtime 的 incoming、outgoing、hover 合成一个有界 query batch，统一 readiness 和诊断收集。
超过 128 条等限制时显式分批并保留共享 deadline、partial 与覆盖范围。跨请求复用先继续用现有完整结果缓存。
只有实测证明冷启动长期占主导，才评估受控会话或 daemon；不应先引入常驻进程与新的工作区生命周期。
语义缓存继续绑定 recordsHash、runtime 和环境，不能去掉 readiness barrier 或缓存不完整结果为 complete。

### S4：正文搜索先限定路径，再使用输出预算

位置：[bodyCandidates](../../internal/codebase/body_search.go#L19)、[bodyTopicPath](../../internal/codebase/body_search.go#L102)。

真实 Git 路径对确切提交执行 grep，输出上限 4 MiB；解析输出后才按 api/lua/xml/toc 过滤。
大量不相关文件的命中可以占满输出上限，让相关主题的结果更早 truncated。现有 truncation 可见，不能把这项描述为静默完整性错误。

建议把经过验证的 topic→Git pathspec 策略前移，同时与索引路径白名单一致。保留语法失败源码的正文回退，不能要求解析成功才允许检索。
重点测量宽泛词、API 主题和超长行；限定路径后结果的完整性声明仍受本轮输出/候选预算约束。

### S5：容量核算需要量化锁内目录遍历

位置：[Grow/容量租约](../../internal/codebase/cache_budget.go#L38)、[sourcePathBytes](../../internal/codebase/cache_budget.go#L141)、[流式索引预留](../../internal/codebase/index.go#L166)。

source/v1 有 4 GiB 派生文件预算和 500,000 节点边界。流式 writer 持有容量锁，每次增长预留又做目录核算；Grow 及后续 ensure 都有遍历。
这保证未写入的 reservation 与并发配额正确，但大量小缓存下可能让锁内工作随目录规模增长。

先测锁等待、遍历节点数与核算耗时。需要时增加锁保护的预留/提交账本、一次请求内增量计数和有界对账；崩溃、孤儿暂存、隔离旧文件、symlink 与并发 reclaim 都必须覆盖。
不能用失真的 manifest 数值代替真实容量，也不能去掉全局配额锁。这是磁盘容量预算，不是 source 总内存预算。

## Native live，尤其是内存扫描

### L0：先使指标覆盖真正的开销

位置：[Coverage/Worker](../../internal/live/memory/scan.go#L44)、[readSpan](../../internal/live/memory/scan.go#L403)、[Process.Read](../../internal/live/memory/process_windows.go#L98)。

Worker.ReadCalls 统计块扫描和 salvage 的 src.Read，不包含 hints 点读、候选 ReadRecord 的两次读取和 VirtualQueryEx 枚举/准入查询。
PlannedBytes/ScannedBytes 是逻辑扫描覆盖，不等于实际跨进程复制字节；重叠、重读、候选验证和其他观察仍有成本。
所以仅凭现有 readCalls 或扫描吞吐无法定位整个 live 操作慢在哪里。

建议由一次 Driver/操作持有统计器，分别记录：身份验证、region 枚举、RPM 次数与请求/实际字节、短读/salvage、SIMD、候选数量及淘汰原因、完整校验、hint 学习、JSON、WGC、输入等待、journal 与清理。
给每种查询附 kind、已知/未知 runtime、cache path 和 coverage；聚合 operation 总数，保留阶段 p50/p95 和取消延迟。
避免把 body 或全部扫描缓冲写入指标证据，统计自身也须有界。

### L1：已知 nonce 查找不应因启用 hints 丢失精确锚点

位置：[lookup 锚点选择与学习](../../internal/live/memory/records.go#L82)、[Hints.Load/Find](../../internal/live/memory/hints.go#L31)。

只有 `hints == nil && nonce != zero` 使用 16 字节 nonce 锚点，并向前 8 字节恢复记录头。
启用 hints 时，即使提示文件不存在、集合为空，也改扫通用 MemoryMagic，以便沿途学习其他小记录。
header 预筛仍会拒绝错误 runtime，所以这不是错误接受；但无关的旧副本也进入候选回调。

隔离探针使用 4 MiB fakeSource、10,000 条其他 runtime 的合法 InputState 记录，以及一条目标 Receipt。
两个路径都扫描完整 4 MiB、正确得到同一条目标记录，差别如下：

| 路径 | 候选数 | 扫描块 ReadCalls | 合成基准 ns/op |
|---|---:|---:|---:|
| hints=nil，nonce 锚点 | 2 | 4 | 169,340 |
| hints 非 nil 且为空，magic 锚点 | 10,001 | 4 | 543,160 |

同机短基准约 **3.2 倍单次耗时差**；稀疏场景为 165,044 对 172,277 ns/op，差别小得多。
启用分支还包含每轮失败的 LoadHints 文件打开；这是整段探针的耗时对照，不是纯扫描 CPU profile。
这是内存字节数组加 fakeSource，不包含 ReadProcessMemory、真实 heap 分布、游戏并发或 GC 记录数量；不能承诺真实 live 提速 3.2 倍。
“2 个候选”包括记录头及 trailer 的 nonce 命中，最终只有一条有效记录。

建议拆分职责：确切操作查找继续用 nonce 锚点；未知 runtime 的 discovery 保留 magic；学习通过明确的有界 warmup/discovery，或受独立候选/CPU 预算限制的扫描附带任务执行。
如果一轮读缓冲同时查 nonce 和 magic，必须分别限额；这只能复用复制成本，不能自动消除 magic 的候选开销。
学习预算耗尽不得停止业务查找或改变 coverage。

保持 SIMD、跨 chunk overlap、nonce 的 -8 回退、首块/边界候选处理、头尾/校验和及新鲜读取验证。
不能把命中扫描缓冲中的 nonce 当作已验证的结果，也不能把旧 InputState 的 sequence 当新的业务凭证。

### L2：分开“在哪里优先找”和“什么记录可接受”

位置：[Find 优先区与点读](../../internal/live/memory/hints.go#L145)、[FindNearby](../../internal/live/memory/nearby.go#L23)、[Native.findPath](../../internal/live/channel/native_windows.go#L99)。

Find 的点读要求旧 hint 的 header 精确匹配 selector；新操作会有新的 nonce/ticket，通常不能复用旧地址作为结果。
FindNearby 也先要求 selector.matches(hint.Header) 才形成窗口，因此同 runtime 的其他操作记录不会成为邻域种子。
普通 Native.Find 路径没有启用 nearby；当前局部路径主要服务专门的输入观察。
启用 hints 的全扫描虽然会前置旧地址所在块，仍先枚举整个进程并计划扫描任务。

建议形成三层查找：精确地址重新验证 → 同 runtime/记录类别的少量邻域 → 当前全进程扫描。
邻域种子匹配 runtime/kind/进程 scope，最终 ReadRecord 仍精确匹配 nonce/ticket；两种匹配必须是不同的内部概念。
优先保留最近成功区域与有界地址热度，合并重复窗口，按热度而非仅 VA 排序；旧 runtime 只能降低优先级，不能影响发现完整性。

邻域只是 allocator locality 假设，Lua 字符串搬迁、GC、容量换代和 reload 后都可能失效。
局部未命中后按调用目的回退；每层输出实际覆盖与 budget，不能声称局部已证明全局不存在。
未知 runtime、cache off 和 `first=false` 的全审计保留完整发现语义，不能被旧地址的早停替代。

### L3：以一次操作组织观察，并减少 HEAD/BODY 的重复扫描

位置：[ObserveRecords](../../internal/live/channel/observations.go#L26)、[授权 BODY 查找](../../internal/live/channel/observations.go#L97)、[Scan](../../internal/live/memory/scan.go#L148)。

一个 native 操作涉及 runtime/input/receipt/HEAD/BODY 等多次观察；各次 Find 的 fallback 都可能重新枚举与扫描。
现有 hints 已在 Native 内复用，进一步的空间是合并兼容的发现任务、保留近期查找区域和共享一次操作的预算，而不是另起一套内存 reader。

建议在现有 RecordReader/ObserveRecords seam 内引入有界的操作观察会话：固定进程创建身份，统一地址提示、计量、deadline 和预算。
可以复用调度规划，但 cached region map 不能授权未来读取；实际 src.Read 仍检查当前映射，进程变化、reload 与新 runtime 必须失效。
当前 heap 不是原子快照，发现时存在的记录也不代表之后业务状态仍成立。

HEAD 验证后，先在已知 HEAD 附近或近期成功 BODY 区域进行授权的精确查找，再回退全扫描，是值得实测的选项。
BODY 与 HEAD 是否靠近尚未证明；不能用硬编码相邻偏移。完整长度、校验和和全部身份检查仍来自 HEAD。
当前明确禁止通过一般学习保留 BODY；若研究“发现阶段只保存不可信 BODY 头候选”，须先修订合同，且 HEAD 授权前不能读取或解释其载荷。
同样，不能把前一轮扫描结果直接当 fresh receipt 来绕过原业务谓词。

### L4：统一物理 I/O 与候选预算，再调 workers/chunk

位置：[默认扫描参数](../../internal/live/memory/scan.go#L94)、[任务构建](../../internal/live/memory/scan.go#L178)、[候选重读](../../internal/live/memory/records.go#L44)、[nearbySource](../../internal/live/memory/nearby.go#L126)。

全扫描默认 8 workers、1 MiB chunk、最多 64 GiB 计划范围和 4096 结果；每 worker 分配扫描缓冲。
逻辑 MaxBytes 不包含 candidate 的两次点读、salvage、overlap、hint 读取或同一操作的后续 Find。
nearby 已有 8 MiB、4096 reads、250 ms 的独立限制，且 wrapper 包含点读和重试，值得将这种核算推广为共享操作预算。

建议分别限制实际 Read 调用/字节、业务候选、学习候选、scan CPU/时间和单操作总量，保留明确的预算耗尽与 partial。
外层预算不能只包扫描块而漏掉 ReadRecord，也不能在每次 Find 重置 operation 总预算。
250 ms context 是合作式时间界限，不能宣称能抢占正在执行的 Windows syscall。

用真实客户端比较 1/2/4/8 workers 与不同 chunk，同时观察游戏帧时间、GC、工作集和取消响应。
任务队列、缓冲复用和小记录分配优化只在 profile 支持后实施；缓冲池必须有容量上限，payload 保有期须明确，不能归还仍被结果引用的字节。
“多线程更快”与“SIMD 已快”都不足以决定当前配置；减少跨进程复制和扫描轮数往往更有价值，但真实收益仍待测量。

### L5：优化 syscall 与 hint 处理，但不删安全条件

位置：[Process.Verify](../../internal/live/memory/process_windows.go#L37)、[Process.Read](../../internal/live/memory/process_windows.go#L98)、[hint.learn](../../internal/live/memory/hints.go#L71)、[lookup 学习锁](../../internal/live/memory/records.go#L110)。

Verify 每次分配 32,768 个 UTF-16 元素并调用进程时间、映像和退出码查询；Find/Scan 的分层入口会重复验证。
每次 Read 对所跨区域做 VirtualQueryEx 后再 RPM，这是实际准入检查，不是可随意删除的多余调用。
学习在共享锁内进行完整小记录校验、排序及 map 重建；InputState 随 sequence 增长可多次更新，64 条保存上限并不等于学习 CPU 上限。

建议先 profile。可研究复用有界映像缓冲、避免同一受控入口重复身份查询，以及将小记录解码移出锁、短锁合并每批最新 hints。
hint 政策按当前 runtime/kind 优先并预先去重，再应用 64 MiB 优先扫描额度；不能让无关旧记录挤掉最有用的区域。
各项改变需测试并发、sequence、旧 runtime、伪造/损坏 hints、fresh mapping 变化和进程退出，不能把排序顺序当身份或新鲜度证明。

Windows 的 [VirtualQueryEx](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-virtualqueryex) 返回映射状态/类型/保护属性；
[ReadProcessMemory](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-readprocessmemory) 检查读取范围可访问性，但不证明我们的 private-only 政策、进程身份或业务记录原子性。
所以“RPM 成功就足够”和“旧 region map 表示现在仍可读”都不是本项目可接受的替代条件。

### L6：发布端研究已形成候选合同，不能承诺固定地址

2026-09-30 补充：[D 阶段研究合同](phase-d-publication-luals-session-2026-09-30.md) 已深入检查 L6 与跨请求 LuaLS，
包含发布探针、真实 LuaLS 复用/取消实验、取舍、生命周期和 owner 实机配合范围。研究已记录，生产实现及实机收益仍未验证。

位置：[SlotProtocol.Describe](../../addon/Bridge/SlotProtocol.lua#L55)、[InputState](../../addon/Bridge/InputState.lua#L33)。

Describe/槽状态与定期输入样本会发布新的不可变编码字符串。旧副本留在 heap 中的时间取决于 Lua GC，地址和邻近性没有 API 保证。
可研究把较稳定的 runtime 发现信息与动态状态分开、减少无变化重复编码，并明确记录可达寿命；这可能降低通用 magic 的背景噪声。
若减少发布，必须证明 NextSlot、owner/fence、关闭/stop、样本 sequence 和新鲜度的全部状态变化仍可观察。

不要假定 Lua 有可原地写的固定缓冲，也不需要先引入 FFI、注入、进程写内存或其他地址发现工具。
维持已批准的输入采样节奏和 500 ms 新鲜度判断；不能为了更易扫描擅自改为更高频 timer。
新增固定发现 beacon、发布协议或 timer 都是后续合同变更，需要单独设计与完整验收。

## 建议实施与验收顺序

| 阶段 | 工作 | 完成标准 |
|---|---|---|
| A | data R1/R2，source S1，live L0/L1，分别小步变更 | 错误身份/恢复不回退；40 命中单文件读取合并；已知 nonce 不因 hints 开启退化；指标覆盖真实 I/O |
| B | live L2/L3/L4，source S2/S3 | warm 场景少扫；局部 miss 明确回退；HEAD 授权无绕过；共享预算、排序、分页与语义结果一致 |
| C | S4/S5、L5 与基于 profile 的 workers/chunk/分配调整 | 测量证明收益，容量账本/崩溃恢复与进程准入保持正确 |
| D | [L6 与跨请求 LuaLS 研究合同已记录](phase-d-publication-luals-session-2026-09-30.md) | L6 先稳定编码复用；LuaLS 先单请求合并，跨 CLI opt-in 且有界回收；生产/实机验收 not_run |

各 live 改动需覆盖冷启动、热提示、cache off、无效/旧 hints、同安装双 PID、同角色/不同角色、nonce/ticket 变化、同 runtime 搬迁、短读/guard/区域撤销、取消、进程退出、reload、容量换代、异步 HEAD/BODY、重复调用与清理。
disabled/enabled、first-install/upgrade、relogin 与现行客户端矩阵不能被 synthetic fixture 替代；互动桌面验收仍由 owner 手工记录。
建议按真实读取字节、全扫描次数、候选/学习工作量、p95、RSS/游戏帧时间评估，优化前后使用同一操作集合，保留失败和 fallback 的证据。

本轮没有运行新的真实游戏验收，以上全部优化均未实施；不新增未经证明的产品吞吐或时延承诺。

## 本轮验证与复现

- 基线 `go build ./...`、`go vet ./...` 通过。
- 同一生产 HEAD 的 `LYCHEEDEV_REQUIRE_LUA51=1 go test -count=1 ./...` 在本轮 data 审计通过；随后只增加审计文档及隔离 overlay，没有生产代码变化。
- Source 隔离计数探针通过；memory 候选探针与 200 ms 短基准通过。探针验证的是现有开销，不表示优化已实现。
- Source 探针使用真实临时 Git 仓库，但不是完整 Blizzard 源码性能测试；memory 探针没有打开游戏进程。

本机原始证据：
[source 日志](../../.tmp/source-live-audit-20260930/source-probes.log)、
[memory 日志](../../.tmp/source-live-audit-20260930/memory-probes.log)。
`.tmp` 为本机忽略目录；下面保留探针源码及生成方法，使调查不依赖这些文件存活。

### 可移植 overlay 生成方法

从本报告基线 checkout，将下方两段 Go 代码各保存为临时文件。Source 探针另需复制 index.go 并作以下纯计数插入：

1. import 增加 `sync/atomic`，包级增加 `auditVerifyCalls`、`auditScanCalls`、`auditDocumentCalls`，类型均为 `atomic.Int64`。
2. 在 `snapshotCache.verify`、`snapshotCache.scan`、`snapshotCache.document` 函数入口，分别调用对应计数器的 `Add(1)`。
3. 生成 Go overlay JSON 的 Replace：真实 index.go → 计数副本；虚拟 internal/codebase/audit_overlay_test.go → source 探针。生产文件不改写。
4. memory overlay 将虚拟 internal/live/memory/audit_overlay_test.go 映射到 memory 探针。路径使用当前机器的绝对路径。

执行：

```text
go test -count=1 -overlay <source-overlay.json> ./internal/codebase -run TestAuditRepeatedSourceReads -v
go test -count=1 -overlay <memory-overlay.json> ./internal/live/memory -run TestAuditAnchorCandidateAmplification -bench BenchmarkAuditAnchors -benchtime=200ms -benchmem -v
```

基准计时是短样本，仅用于确认候选放大；以后需要多轮统计和真实 RPM 对照才能决定产品参数。

### Source 探针

```go
package codebase

import (
    "context"
    "fmt"
    "strings"
    "testing"
)

func TestAuditRepeatedSourceReads(t *testing.T) {
    var text strings.Builder
    for i := 0; i < 40; i++ {
        fmt.Fprintf(&text, "function Widget%03d() end\n", i)
    }
    b, pin := indexedSearchFixture(t, map[string]string{
        "Interface/AddOns/Test/Core.lua": text.String(),
    })
    auditVerifyCalls.Store(0)
    auditScanCalls.Store(0)
    auditDocumentCalls.Store(0)
    if err := b.EnsureIndex(context.Background(), pin); err != nil {
        t.Fatal(err)
    }
    response, err := b.Search(context.Background(), "PIN-AUDIT", pin,
        SearchQuery{Mode: SearchModePrecise, Text: "Widget", Limit: 40})
    if err != nil || len(response.Results) != 40 {
        t.Fatalf("results=%d err=%v", len(response.Results), err)
    }
    t.Logf("results=%d unique files=1 full map hashes=%d map scans=%d document reads=%d",
        len(response.Results), auditVerifyCalls.Load(), auditScanCalls.Load(), auditDocumentCalls.Load())
    if auditVerifyCalls.Load() != 2 || auditScanCalls.Load() != 1 || auditDocumentCalls.Load() != 40 {
        t.Fatal("read amplification behavior changed")
    }
}
```

### Memory 探针

```go
package memory

import (
    "context"
    "testing"
    "github.com/follenfang/lycheedev/internal/bridge"
)

func auditMemoryFixture(tb testing.TB, noisy bool) (*fakeSource, Selector) {
    tb.Helper()
    s := source(4 << 20)
    if noisy {
        for i := 0; i < 10000; i++ {
            h := bridge.MemoryHeader{Kind: bridge.MemoryInputState, State: 1,
                Runtime: [16]byte{1}, Nonce: [16]byte{1}, Sequence: uint32(i + 1)}
            raw, err := bridge.EncodeMemoryRecord(h, []byte("old input"))
            if err != nil { tb.Fatal(err) }
            copy(s.data[128+i*256:], raw)
        }
    }
    h := bridge.MemoryHeader{Kind: bridge.MemoryReceipt, State: 1,
        Runtime: [16]byte{2}, Nonce: [16]byte{7, 8, 9, 10, 11}, Ticket: [16]byte{3}}
    raw, err := bridge.EncodeMemoryRecord(h, []byte("wanted"))
    if err != nil { tb.Fatal(err) }
    copy(s.data[3<<20:], raw)
    return s, Selector{Kind: h.Kind, Runtime: h.Runtime, Nonce: h.Nonce, Ticket: h.Ticket}
}

func TestAuditAnchorCandidateAmplification(t *testing.T) {
    s, selector := auditMemoryFixture(t, true)
    for _, enabled := range []bool{false, true} {
        var hints *Hints
        if enabled { hints = LoadHints("missing", "audit") }
        records, coverage, err := lookup(context.Background(), s, selector, Options{Workers: 1}, false, hints)
        if err != nil || len(records) != 1 || !coverage.Complete {
            t.Fatalf("records=%d complete=%t err=%v", len(records), coverage.Complete, err)
        }
        t.Logf("hints=%t planned=%d scanned=%d candidates=%d reads=%d", enabled,
            coverage.PlannedBytes, coverage.ScannedBytes,
            coverage.Workers[0].Candidates, coverage.Workers[0].ReadCalls)
    }
}

func BenchmarkAuditAnchors(b *testing.B) {
    for _, noisy := range []bool{false, true} {
        for _, enabled := range []bool{false, true} {
            name := "sparse/nonce"
            if noisy { name = "foreign-records/nonce" }
            if enabled { name += "-hints-magic" }
            b.Run(name, func(b *testing.B) {
                s, selector := auditMemoryFixture(b, noisy)
                b.ReportAllocs()
                b.SetBytes(int64(len(s.data)))
                b.ResetTimer()
                var candidates uint64
                for i := 0; i < b.N; i++ {
                    var hints *Hints
                    if enabled { hints = LoadHints("missing", "audit") }
                    records, coverage, err := lookup(context.Background(), s, selector, Options{Workers: 1}, false, hints)
                    if err != nil || len(records) != 1 { b.Fatal(err) }
                    candidates += coverage.Workers[0].Candidates
                }
                b.ReportMetric(float64(candidates)/float64(b.N), "candidates/op")
            })
        }
    }
}
```
