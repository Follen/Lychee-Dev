# Native memory 查找与计量优化

日期：2026-09-30。基线：`8e4dab0455c8bee512c73912c26b31bc9605f6c2`。
本页记录审计 L0/L1/L2/L4，以及经分配测量支持的局部 L5 改动。
渠道的观察生命周期与 HEAD/BODY 组合由 channel 实施说明记录。

## 查找路径

已知 nonce 的业务查找始终使用其 16 字节精确锚点并向前 8 字节恢复记录地址；
hints 开关不再切换到通用 magic。未知 nonce 的发现仍用 magic。
SIMD、跨块重叠、短读后的有界 page salvage 与完整头尾/校验和校验保持原规则。

普通 `Find` 对已知 runtime、缓存开启、`first=true` 的小记录依次尝试：
精确 hint 地址的新鲜重读 → 同 runtime/类别的有界邻域 → 全进程扫描。
邻域种子仅匹配 runtime/类别；它们可以来自另一 nonce/ticket。
实际接受仍要求 selector 的全部身份、完整记录及业务谓词成立。
邻域窗口合并后最多 8 个 1MiB 范围，共享 8MiB 请求字节、4096 次读取和 250ms 合作式截止。
局部未命中、局部额度耗尽或局部截止后，调用方上下文仍有效时继续全扫描。
局部命中保留 `complete=false`/`truncated=true`，不构成全进程覆盖。
未知 runtime、`hints=nil` 以及 `first=false` 均保留全扫描语义。

附带提示学习使用扫描缓冲中的独立 magic 搜索，受独立候选数和检索字节预算限制。
学习预算耗尽仅停止学习，不停止业务查找、不改变扫描覆盖结论。
学习计数包含错误 runtime、损坏头、无用候选等淘汰工作，不以保留的 64 条 hints 代替工作量。
完整小记录的校验在 hints 锁外完成；合并与保存受锁保护，集合上限仍为 64，
每 runtime 最多保留 8 个 Input 地址。BODY 不进入一般 hints，包括显式成功结果。

`FindNearbySeeded` 供已经核验 HEAD 的渠道使用。它要求完整 BODY 授权 selector，
传入的 HEAD 地址只是邻域调度提示，没有固定相邻偏移；返回结果仍经过
`ReadRecord` 的 runtime/nonce/ticket/长度/校验和/完整头重读/业务谓词检查。
调用方负责验证 HEAD 的领域含义及在最终接受前重读 HEAD；局部未命中继续原全扫描。

## 一次调用链共享的预算和指标

`Session` 可被一个 Native 调用链的多个 Find、Input 点读和 runtime 观察共享。
`Session.Source` 是幂等包装；显式 `FindWithSession`、`FindNearbyWithSession`、
`FindNearbySeeded` 和 `Options.Session` 复用同一对象，不在每个 Find 或阶段重置。
预算预约与统计加锁；同时发起的 worker 不能共同越过预算。
同一 owner 的包装不重复计量；不同显式 Session 包装已经有 owner 的 Source 时返回
`memory.session_ownership`，不静默绕过较紧的预算。隐式 nil Session 则沿用已有 owner。
统计快照计数单调，正在进行的学习搜索使用单独的预约账本。

默认上限是安全边界，不是客户端吞吐调参结论：

| 额度 | 默认 |
| --- | --- |
| Source.Read 调用次数 | 1,048,576 |
| Source.Read 请求字节 | 256GiB |
| 业务候选工作单元 | 1,048,576 |
| 附带学习候选 | 4096 |
| 附带学习搜索字节 | 64MiB |

业务工作单元包含扫描锚点以及每次 `ReadRecord` 验证尝试。
因此一个有效记录的头部 nonce 与 trailer nonce 会产生 2 个锚点，完整点验证再产生 1 个工作单元。
`Worker.Candidates` 保留锚点计数；`Stats.Candidates` 是共享预算使用的工作量，二者不混用。
精确 hints 点读及独立 point validation 同样占用此预算。
物理调用/请求/实际字节包括 hints、固定头、完整记录、重叠、salvage 和后续观察。
请求字节在调用前预约；实际字节仅按 adapter 返回的合法读取数计量。
Windows 的 `RPMCalls` 与 `MappingQueries` 另记真实 ReadProcessMemory/VirtualQueryEx 次数，
因此映射准入失败不会被误计为一次成功的 RPM。

同时记录验证/枚举次数、短读、业务/学习候选、学习字节、学习成功、完整验证及淘汰计数。
指标只含数值和状态，不保存进程字节或 payload。
`LookupResult.Stats` 是整个查找区间，`Coverage.Stats` 是扫描区间；它们是嵌套区间，不能相加。
`Stats.Delta` 提供区间计数，`Stats.Add` 只聚合互不重叠的区间；共享并行调用的区间会重叠，
此时以 Session 总量为权威，不把每个调用的快照差当作独占归属。

物理/业务预算耗尽返回 `memory.operation_budget`，未观察部分保持 unknown/partial，
扫描缺口记录 `budget_exhausted`，不得解释为内存不可读或记录不存在。
学习耗尽仅记录 `learningExhausted`。
局部 250ms 软截止记录 `localStopped`；请求取消记录 `cancelled`，请求期限耗尽记录
`deadlineExceeded`，避免成功全扫描回退被误报为请求取消。
所有截止均合作式，不能抢占已经执行的 Windows syscall。

Session 本身不持久化。当前物理额度 scope 为一次 CLI invocation 内的 Native 生命周期；
跨进程 resume 的物理消耗没有持久账本，不能声称它是跨 CLI 的持久 operation 总额度。
原 Driver 的持久截止与 lease/journal 规则仍由既有编排控制，本改动未修改它们。
进程 identity 与每次读取的当前映射继续重新检查，不使用缓存 map 授权读取。

## 分配测量与边界

`BenchmarkHintLearning` 的 64 条当前 runtime receipt fixture 先测得每轮
18,968B、17 allocs。预分配至多 65 条临时 hints 并复用原有有界目的切片后，
三轮测得每轮 5,016B、4 allocs。排序、runtime/sequence 偏好与容量规则不变。
wall time 受同机并发负载影响，不能据此宣称时延改善。
没有变更 Process.Verify、读取准入查询、workers、chunk、payload 所有权或缓冲池。

4MiB fakeSource、10,000 条其他 runtime Input 加 1 条目标 receipt 的短合成基准：
cache off 和空 hints 都保持每轮 2 个业务锚点；hints 附带学习最多 4096 个候选。
三轮 cache off 为 642–671µs，hints 为 981–1099µs。
这是当前计量实现的合成开销，不是对旧版本或真实游戏的吞吐承诺。
审计中的旧 3.2 倍差异也不能移作本实现的实机收益。

## 验证

`go build ./...`、`go vet ./...`、`go test -count=1 ./internal/live/memory` 和该包 `-race` 检查通过。
新增 fixture 覆盖：精确 nonce+hints、独立学习候选/字节额度、跨 Find 与 point read 共享预算、
并发预约、请求取消、局部截止后成功 full fallback、重叠/短读/salvage 计量、
新 nonce 的邻域与远地址回退、BODY 授权及禁止一般 BODY hints。
另外覆盖不同 Session owner 冲突与 caller 自建超长 hints 的分配/集合上限。
原跨 chunk、HEAD/body 撕裂、损坏 hints、旧 runtime、cache off/full audit 测试保持通过。
Windows 测试子进程同时核对真实 RPM 与 mapping query 的点读计量，仍为只读 adapter 验证。

真实 WoW 客户端的冷/热查找、双 PID、reload、GC 搬迁、游戏帧时间、RSS、first-install/upgrade、
relogin 及客户端矩阵：本模块实施期间 `not_run`，不能用 synthetic 或子进程 fixture 替代。
根任务负责最终 Lua51 全量门禁与提交。测试和基准可复现：

```text
go test -count=1 ./internal/live/memory
go test ./internal/live/memory -run '^$' -bench 'BenchmarkExactNonceWithHints|BenchmarkHintLearning' -benchtime=500ms -benchmem -count=3
```
