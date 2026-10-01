# Data 查询资源预算与工作区准入

日期：2026-09-30。对应代码审计 R3、R4；本实现属于非 live 分支，不改变 addon、live、版本或发行渠道。

## 查询预算

`internal/records/resource` 提供查询拥有的、线程安全的五维预算。一次多维扣减必须全部成功；失败不部分扣减。SQL、导航、单表与文件读取通过 `FileQuery` 传递同一个实例，各表及 CDN source 不重置实例。

| 维度 | 默认上限 | 核算范围 |
| --- | --- | --- |
| retained bytes | 512 MiB | DB2 内容、Root/Encoding、保留的索引/辅助映射、定义解析、Hotfix、导航保留行；SQL 另预留执行器的 64 MiB |
| metadata bytes | 128 MiB | 定义、存储目录、WDC 布局、身份与复制索引、palette/common 映射及 Hotfix 元数据 |
| decode/work | 100,000,000 | 解码块、索引建立与所有解码字段元素；重复扫描、失败候选仍消耗工作预算 |
| network requests | 4,096 | 同一查询的 CDN source 与定义请求 |
| network bytes | 1 GiB | 同一查询的 CDN 传输与定义响应正文 |

上述内存计费是保守的查询生命周期预留，不是测得的实际内存，更不是 RSS 硬上限。定义解析计费包含原始字节与解析结构；未知长度的定义 HTTP 响应在读取前预留最多 8 MiB 的有界读取缓冲，再在解析前预留解析结构。WDC 在创建 Go backing arrays 和 maps 前计费；身份、复制与稀疏索引暂按每个逻辑行 192 字节，common 映射按原始字节的九倍预留，辅助字节与结构另计。临时结构也可保守保留在查询计数中，不通过退款让下一张表重新获得整份预算。

BLTE decoder 的有界临时缓冲另通过 `ReserveScratch` 与 retained 共享同一上限，当前 scratch 和生命周期 retained 相加后必须满足额度；步骤结束释放 scratch reservation。`peakScratch` 是逻辑同时存活缓冲的峰值，不包含运行时、GC 尚未回收的 backing memory，也不等于实测 heap/RSS。

SQL 执行器自身的工作/内存限制继续有效。source bytes 的原有限制继续有效，但不能替代新增的累计元数据与保留内存预留。新增预算耗尽返回 `records.query_resource_budget`（exit 3），不返回一份声称完整的截断结果。SQL `resources` 和导航上下文中的 `resources` 是逻辑计费；其中 retained bytes 不可解读成当前堆大小或峰值 RSS。

查询会话的存储目录、Root、Hotfix 与 CDN 计费接线和复用由同分支的 reader/session 实现提供。SQL 和导航在提交 capture 前调用 `CheckSource`，结束时关闭会话；基础视图命中由会话复用，不重新建立索引。单表定义准备与 asset 原始内容归档、file existence/encoding 元数据路径在整个准备周期持有准入。

CDN 的 budget transport 同时覆盖目录、配置与 range 请求，在发起尝试前计一次请求，按正文实际 `Read` 字节扣减，并保留部分读取附带的底层网络错误、取消和 deadline 身份。字节额度边界允许一次额外字节用于验证 EOF；存在额外正文时必须返回明确预算错误，不会成为成功数据。HTTP headers、连接内部缓冲及 wire bytes 不等于此正文计费。错误响应正文只有实际读取时才计费。

## 工作区准入

`vault.Store.AcquireDataResources` 使用工作区内 `locks/data-resources-v1/` 的 OS-owned leases。保留 lock 文件避免删除 inode 导致两个锁域；内核句柄代表存活所有权，进程死亡自动释放，不根据文件年龄或租期抢占活进程，不受 PID 重用影响。

Windows data 等待使用 overlapped `LockFileEx`、事件与 `CancelIoEx`，取消后等待该操作结束并回收 OVERLAPPED；不通过轮询文件时间戳或修改 live 的通用 lease 等待策略实现。

普通查询最多并行两份完整查询预算；`config.download.workers=1` 进一步序列化普通查询，零或更高值仍受两份额度的固定上限。每份查询默认 512 MiB 的保留估计，因此这是 workspace admission 的逻辑额度，并非操作系统保证的 1 GiB 总 RSS 上限。Go runtime、SQLite、外部依赖、其他 home 和其他软件不属于此估计。

`DataMeasurement` 先持有 admission gate，再等两份已有查询退出，取得全部 slots 后保留 gate 直到关闭，阻止新重型查询进入。调用方可显式使用 `vault.WithDataResourcePolicy(ctx, vault.DataMeasurement)` 将既有 data 入口的普通准入升级为 measurement。此内部 API 不新增 CLI command，不自动把普通查询当成 measurement，不声称已接入现有 live 性能调查。

等待遵守调用方取消/截止时间，并有独立 30 秒上限。等待阶段不持有 SQLite 写事务；metadata connection 的存在不等于写事务。所有失败释放已取得的 gate/slots，`Close` 可重复调用。

## 已运行验证

- `go build ./...`、`go vet ./...`：通过。
- `go test ./internal/records/resource ./internal/records/table ./internal/vault ./internal/records`：通过。
- 最后 file metadata 接线后，`go test ./internal/records -run 'FileExistence|FileEncoding|DefinitionsBudget|RecordReader' -count=1`：通过。
- `go test ./internal/records/navigatetest ./internal/command -count=1`：通过。
- budget transport 最后批次的 build/vet 与 `go test ./internal/records -run 'BudgetTransport|DefinitionsBudget|FileEncoding|FileExistence' -count=1`：通过；目录/404 响应、部分读取失败、同时预算及网络失败、EOF 边界与请求/读取取消均有隔离测试。
- 预算测试覆盖多维原子扣减、跨 source 累计、并发扣减与溢出，WDC 第二张表不能重置工作计数，身份索引预算不足时未读取记录区。
- 定义测试覆盖累计请求、实际响应正文计数、离线 metadata 预留、无 Content-Length 正文的网络字节边界。
- 准入测试覆盖两份额度、单 worker、取消/截止时间、measurement 排他、等待期间 metadata 可写，以及独立 helper process 持有 measurement、终止 holder 后重新获得额度。
- Windows event wait 与 scratch 最终接线后，`go test ./internal/records/resource ./internal/records/container ./internal/vault -count=1`：通过。scratch 覆盖 retained/scratch 共同额度、嵌套缓冲、预 body 分配拒绝以及 success/integrity/cancel 路径释放；`WriteAvailable` 使用同一额度。
- `go test ./internal/records ./internal/records/navigatetest -count=1`：通过。新增 session 测试验证回调绑定、关闭清空重型引用且幂等、同查询 verified bytes 共享、新查询重新发现同大小 blob 篡改及 warm blob 读取前预算拒绝。

## 未运行与边界

真实 RSS/heap profile、真实 CDN、多客户端实机及 performance measurement：`not_run`。未设置进程全局 Go memory soft limit；尚无 RSS 基线支持把该值作为性能门槛。现阶段验证的是查询计费与跨进程准入，不能声称已经测得特定内存下降或加速倍率。此文记录子任务测试；提交前强制 Lua 5.1 全量套件由分支总验收记录。
