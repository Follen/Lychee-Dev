# Lychee Dev data 代码审计与 CascLib 对照

日期：2026-09-30。Lychee Dev 基线：`8e4dab0455c8bee512c73912c26b31bc9605f6c2`（3.0.2）。
CascLib 对照固定为 `38a34665624b8775bb875274b36191b21c38d97b`，不以浮动 master 作为依据。

本轮重点检查 `internal/records`、DB2/table、Hotfix/effective、relational SQL、local/CDN CASC、vault，以及它们的命令和预算接线。
这是 data 及其依赖链的深入审计，不是 addon、native live 和所有资产格式的完整安全审计。没有修改生产代码、提交或发布。

## 结论

优先补齐错误恢复与资源核算，再优化查询内复用。CascLib 最适合借鉴的是存储会话、索引和读取缓存；无需为了参考它而替换现有 Go 实现。
本轮没有确认 P0 数据串版本或完整性绕过问题；这不等于整个项目不存在此类问题。

| 编号 | 优先级 | 类型 | 发现 |
|---|---|---|---|
| R1 | P1 | 已复现 | 缓存哈希的短读丢失网络错误及取消身份 |
| R2 | P1 | 已复现 | 编码恢复只覆盖 open 失败，后续读取失败不尝试下一 EKey |
| R3 | P1 | 静态确认 | 查询资源预算未覆盖整个数据准备及导航生命周期 |
| R4 | P1 | 合同接线缺口 | 工作空间重型 data 资源准入未接入读取链 |
| R5 | P2 | 性能机会 | 多表查询重复准备 Encoding、Root 与本地索引 |
| R6 | P2 | 静态确认 | CDN 小读重复加载并验证同一大块，解码命中仍多次完整读 |
| R7 | P2 | 静态确认 | Hotfix 删除标记保留整份 capture 字节；各表重复解析 capture |
| R8 | P2 | 验证机会 | 端到端性能与固定版本差分回归仍值得补齐 |

## 与 CascLib 的业务区别

| 维度 | CascLib | Lychee Dev |
|---|---|---|
| 产品核心 | Blizzard CASC 存储读取库，调用方持有 storage/file handle | 固定上下文的 WoW 调查工具，输出结构化结果与可追溯证据 |
| 覆盖范围 | 多游戏、多 root handler、通用文件访问 | WoW pinned build、DBD/DB2、Hotfix、SQL、技能/物品/生物导航与资产 |
| 生命周期 | 打开存储时建立映射，后续文件读取复用 | 表内扫描复用 View，但跨表重新准备部分存储元数据 |
| 缺钥 | 可显式选择 `CASC_OVERCOME_ENCRYPTED`，缺钥块填零并返回成功 | 记录缺失字节、不可用分区和 partial，不能把占位字节当完整业务数据 |
| 完整性 | 文件打开标志可要求严格校验 | 完整性与来源校验是产品合同，缓存也不能掩盖损坏 |
| 选择语义 | root handler 与调用方 flags 决定通用读取策略 | locale/变体歧义显式处理，不自动换 build、安装或 CDN |
| 查询语义 | 通用文件 I/O API | static/effective/meta 分离，Hotfix 来源、顺序与合并政策纳入证据 |

上游依据：

- [库范围及用法](https://github.com/ladislav-zezula/CascLib/blob/38a34665624b8775bb875274b36191b21c38d97b/README.md)。
- [CKey/EKey 映射与存储准备](https://github.com/ladislav-zezula/CascLib/blob/38a34665624b8775bb875274b36191b21c38d97b/src/CascOpenStorage.cpp#L185)。
- [本地索引加载](https://github.com/ladislav-zezula/CascLib/blob/38a34665624b8775bb875274b36191b21c38d97b/src/CascIndexFiles.cpp#L510)。
- [严格校验及缺钥标志](https://github.com/ladislav-zezula/CascLib/blob/38a34665624b8775bb875274b36191b21c38d97b/src/CascLib.h#L95)。
- [文件读取缓存](https://github.com/ladislav-zezula/CascLib/blob/38a34665624b8775bb875274b36191b21c38d97b/src/CascReadFile.cpp#L814)。

因此，可以学习它的生命周期和数据结构；不能继承填零成功语义，也不能把通用库支持范围直接算作我们的客户端验收。

## R1：短读覆盖底层错误

位置：[decoded_cache.go:47](../../internal/records/decoded_cache.go#L47)。

`decodedCacheKey` 在检查 `ReadAt` 返回的错误前，先比较字节数；不足时直接返回 `io.ErrUnexpectedEOF`。
真实 CDN 读取可能返回 `(0, ErrRemoteHTTP)` 或部分字节加网络错误，这会丢失恢复和分类依据。读期间发生取消，也可能被改写为 EOF。

隔离 overlay 探针已确认：开启稳定 key source 后，第一 EKey 的后续读取返回 `ErrRemoteHTTP`，最终变成 `unexpected EOF`，`errors.Is(err, ErrRemoteHTTP)` 为 false。
另一个探针确认 `ReadAt` 返回 `context.Canceled` 时同样丢失取消身份。

建议：优先保留非 EOF 底层错误；真正无错误的短读才生成 UnexpectedEOF。需要补充零字节、部分字节、取消及 deadline 在读取过程中发生的用例。
不能把所有 EOF 都标成可恢复网络问题，损坏与不完整协议仍须失败。

## R2：打开后的编码恢复不完整

位置：[reader.go:237](../../internal/records/reader.go#L237)、[reader.go:266](../../internal/records/reader.go#L266)。

当前循环只在 `open` 返回 availability 错误时 continue。对象成功打开后，哈希或提取失败会直接 return。
CDN reader 采用延迟 range 读取；打开成功不代表剩余对象已经可用。因此在后续块不可用时，同一 CKey 下健康的另一 EKey 不会被尝试。

隔离探针使用现有 `recoveryEncoding` 的两种合法编码：第一对象 open 成功、read 失败，最终只调用一次 open。
缓存关闭时错误仍为 `records.remote_http`，缓存开启时又叠加 R1。第二候选从未到达；探针没有模拟或承诺某个真实 CDN 当时确有健康副本。

建议：让一次候选尝试拥有 open、hash、decode、close 的完整生命周期，在这一边界统一分类。
仅 availability 可以转下一候选，保留最终来源与 HTTP 不确定性。取消、超时、完整性、格式及预算错误仍立即停止。
候选失败留下的缺钥覆盖、cache stats 等可变状态不得串入下一候选。关闭失败也要明确保留。

现有 [recovery_test.go](../../internal/records/recovery_test.go) 测试覆盖 open 失败；应扩展到对象内部失败。

## R3：资源上限只约束部分层

位置：[sql.go:101](../../internal/records/sql.go#L101)、[query.go:117](../../internal/records/query.go#L117)、[table/records.go:83](../../internal/records/table/records.go#L83)、[navigate.go:203](../../internal/records/navigate.go#L203)、[cdn_files.go:131](../../internal/records/cdn_files.go#L131)。

已经存在很多有效的单层限制，并非完全没有预算：SQL 有累计 512 MiB DB2 content、32 表、执行器 64 MiB；table 有行/列/分区限制；CDN 有请求和传输限制。
但是这些边界没有合成一个完整查询预算：

- SQL 的 64 MiB 是执行器计费，不含 adapter 保留的原始 DB2、locations map、palette/common map 与 Hotfix patches。
- SQL 的 512 MiB 统计源内容字节，不能代表索引和解码过程的实际内存。
- 导航按表计数限制，没有 SQL 那样的累计 source bytes 预算。每个 View 持有原始字节及索引，32 表上限本身不足以限制内存峰值。
- 每次 `prepareCDNFiles` 新建 requests/transferred 计数，4096 请求/1 GiB 的限制属于每个 prepared source，跨表会重置。
- 总 deadline 已正确共用，不能误报成每表重置超时；它也不能替代内存及累计 I/O 预算。

建议：建立查询级 Budget，由 reader、prepared view、Hotfix 与 SQL 共同扣减。至少分别统计 retained bytes、decode/work、network bytes/requests、metadata bytes，并在分配前检查。
内存可以保守估算 map/索引成本，配合实测；不要把逻辑预算宣传为严格 RSS 上限。

本条确认的是接线范围，未测得真实多表 OOM，不把理论极值写成实际机器消耗。

## R4：工作空间重型资源协调未接线

依据：[design.md:489](design.md#L489)；读取入口与 [vault/access.go](../../internal/vault/access.go)。

设计要求下载、解码及索引预算在工作空间层协调。当前 data 入口使用 WriteMetadata 打开存储与连接，CDN fragment 使用同键 lease 防止重复写；这两者不构成跨进程重型任务准入。
检查的数据读取链没有看到工作空间 aggregate decode/index admission 或配套内存控制。`config.download.workers` 的存在也不能证明 data CASC 已接入它。

影响：多个 CLI/Agent 对不同对象工作时，可同时使用各自完整预算；普通 data 负载也缺少 measurement 资源政策的统一协调入口。

建议：先接入少量明确的工作空间资源令牌，使用 OS lease 或短事务、稳定进程身份，等待时不占 SQLite 写锁。把它与 R3 单查询预算分别验收。
这是文档合同落地缺口；不能用现有游戏窗口所有权机制代替，也不需要在本轮重做 live 协议。

## R5：存储准备复用不足

位置：[reader.go:87](../../internal/records/reader.go#L87)、[file_source.go:72](../../internal/records/file_source.go#L72)、[local_object.go:49](../../internal/records/local_object.go#L49)、[archive/index.go:36](../../internal/records/archive/index.go#L36)。

每张表会重新准备 fileSource/Encoding，提取并读取 Root，然后为一个 FDID 查 Root。
本地每次对象解析重新枚举最新 index generation，并按需要依次打开、全段验证和扫描索引；同一查询的 Encoding、Root、各表会重复这些步骤。
已有 `PrepareTable` 很好地解决了同表重复扫描，但 static 与 effective 同名表也会分别准备 base，因为执行器按 catalog+name 缓存 Source。

CascLib 在 storage handle 生命周期内建立索引/映射，是直接值得参考的部分。

建议：在内部引入查询拥有的存储会话，固定 pin、规范安装身份或 CDN 来源、key source、offline 与预算。
会话复用 Encoding directory/page、Root 的有界 FDID 查询结果、已验证本地 index generation 和基础 table view；static/effective 继续保留不同语义。
不要直接常驻全游戏所有映射。按需有界缓存足以先解决重复准备，避免以吞吐换失控内存。
本地查询结束仍复核 launcher/build 与涉及的 generation，缓存不能绕过损坏检测。

## R6：缓存命中仍有读取放大

位置：[cdn_files.go:218](../../internal/records/cdn_files.go#L218)、[decoded_cache.go:40](../../internal/records/decoded_cache.go#L40)、[decoded_cache.go:99](../../internal/records/decoded_cache.go#L99)。

哈希按 64 KiB ReadAt，CDN non-sparse reader 按 256 KiB range 对齐。
同一块内的四次连续哈希读取，当前会四次经过 fragment lease、metadata 查找、blob ReadBlob 和 SHA256 验证。
已缓存时通常不会增加网络请求，但会重复磁盘读取、分配与校验。

decoded cache 命中还要完整哈希 encoded source，读取并验证 decoded blob/MD5；返回 BlobRef 后，Root 和 DB2 消费层再调用 ReadBlob。
所以命中表示免重复解码，不表示免完整输入扫描或免物理 I/O。此行为有明确安全意图，不能简单删除校验。

建议：会话内增加有界块缓存，先实现单块或小 LRU；复用已验证、不可变的 decoded reader/字节，避免只返 ref 又立刻读一遍。
跨 CLI 生命周期仍重新确认来源，当前 corrupt cache 必须失败的合同继续保留。
新增 logical read bytes、physical read bytes、cache bytes、hash/decode 次数，避免单看 Hits 判断收益。

## R7：Hotfix 删除路径与重复解析

位置：[effective.go:52](../../internal/records/effective.go#L52)、[effective.go:147](../../internal/records/effective.go#L147)。

effectiveEntry 包含指向整份 capture 的 raw。正常 replacement 会裁剪到 CacheEntry/source；删除路径却存 `replacement{entry: e}`，保留 raw 引用。
只要某个删除标记保留在 patches，输出 Source 的闭包就可使整份 capture 保持可达。
多张 effective 表各自 FetchCapture、InspectCache、walkCache，存在重复读取、验证、扫描与保留。

建议：删除和有效替换都仅保留必要的来源元数据；capture 在查询会话内验证一次、按 table hash 建立有界目录。
继续检查每个表的定义绑定、按 push/capture/index 排序、unknown status 和 TACTKey 例外政策。
先用 heap profile 验证引用释放，再考虑优化历史版本排序；本轮没有更改合并语义，也没有声称测得具体 RSS 节省。

## R8：验证下一步应覆盖真实准备链

当前 relational 基准使用生成的 Source，适合验证执行器，但不覆盖 CASC、Root、SQLite/blob、DB2 准备和 Hotfix。
本轮在 Windows amd64、9800X3D 上一次 100 ms 基准得到：join-1000 约 0.58 ms/次；top20-20000 约 4.89 ms/次；无 Equal 的 id-20000 扫描约 3.57 ms/次。
采样时全量测试仍在并发运行，属于带并发负载的观察值，不能作为稳定性能门槛。
对应总分配约 0.78/3.53/2.40 MB 每次。`B/op` 是累计分配，非峰值 RSS；这些数不能代表真实 data 命令响应时间。

建议增加：本地/CDN/offline 的冷缓存、热 fragment、热 decoded 三档；单表、多表 JOIN、static+effective 同表、导航、Hotfix 删除场景。
记录 prepare/index/hash/decode/scan 各阶段、物理字节、请求、分配与峰值内存；先测基线，再设回归阈值，不预先承诺加速倍率。

CascLib 可以做测试用差分 oracle：固定 commit、同一 WoW build、FDID/locale/显式变体、完整钥匙，比较原始完整输出摘要。
它的默认文件选择和缺钥填零结果不能拿来覆盖我们的严格歧义与 partial 判定。上游 test-only 使用需要保留 license/notice。
Forever 实验与 Retail/Classic/Titan 验收继续分开；未运行的真机和客户端项保持 not_run。

## 保留的设计

- 固定 DataPin、DBD commit 与实际 layout hash，拒绝 build/source/locale 悄悄替换。
- local index 的完整 guard/order 校验与 9 字节 prefix 碰撞复核。
- CDN exact range 与 EKey/chunk/page/CKey 校验，缓存不掩盖损坏。
- 缺钥覆盖、不可用分区、partial、truncation 和 provenance。
- SQL preflight、exact integer、LEFT JOIN/duplicate/NULL 与 LIMIT 不隐藏后续错误的合同。
- 短 SQLite 事务、不可变 blob、同 fragment lease；新增资源协调不应把下载包进长写事务。

## 建议实施顺序

1. 小范围修复 R1/R2，先增加真实内部读取失败的红测，保持错误分类和候选完整性门禁。
2. 修复 R7 删除引用，接入 R3 查询累计预算；跨进程 R4 独立验证。
3. 建立 R8 端到端基线，再以同一个内部会话完成 R5/R6 复用，避免增加第二套读取路径。
4. 增加 CascLib 交集差分 corpus 与多客户端离线 fixtures。真机及新格式支持另列验收。

每批仍遵循 build/vet、受影响测试及提交前强制 Lua 5.1 全量测试；本审计不构成变更发布授权。

## 本轮验证

- `go build ./...`：通过。
- `go vet ./...`：通过。
- 隔离 Go overlay 探针：通过，确认当前错误行为；不是修复后的验收通过。
- 探针源码：[audit_test.go](../../.tmp/data-audit-20260930/audit_test.go)。
- 探针输出：[reproduction.log](../../.tmp/data-audit-20260930/reproduction.log)。
- `LYCHEEDEV_REQUIRE_LUA51=1 go test -count=1 ./...`：通过，退出 0，包括 addon/protocol Lua 5.1 套件。
- 未操作游戏窗口、未做新的真实 CDN 故障验收或真机性能测试。

## 复现探针源码

以下探针描述当前缺口；运行时用 Go overlay 映射到 internal/records/audit_overlay_test.go，不写入生产源码。依赖基线已有的 recoveryEncoding/newCDNFilesTest helper。

```go
package records

import (
 "context"
 "errors"
 "io"
 "testing"
)

type auditFailedObject struct { failure error }
func (o *auditFailedObject) Read([]byte) (int,error) { return 0,o.failure }
func (o *auditFailedObject) ReadAt([]byte,int64) (int,error) { return 0,o.failure }
func (*auditFailedObject) Close() error { return nil }

// These probes characterize current behavior; they are not acceptance tests.
func TestAuditLateReadRecovery(t *testing.T) {
 for _, cached := range []bool{false,true} {
  body := []byte("same content, later healthy encoding")
  index,_ := recoveryEncoding(t,body)
  _,store := newCDNFilesTest(t,&cdnTestTransport{},false,nil)
  q := FileQuery{cacheStats:&DecodedCacheStats{}}
  if cached { q.keySource=&KeySource{Kind:"file",SHA256:"audit"} }
  calls:=0
  open:=func(context.Context,string,int64)(encodedObject,error) {
   calls++
   return &auditFailedObject{failure:ErrRemoteHTTP},nil
  }
  _,_,err:=OpenReader(store).extractContent(context.Background(),q,open,index,md5TestKey(body),1<<20)
  if err==nil || calls!=1 { t.Fatalf("unexpected behavior: %v, calls=%d",err,calls) }
  t.Logf("cache=%t: encodings=2, opened=%d, error=%v, retainsHTTP=%t",cached,calls,err,errors.Is(err,ErrRemoteHTTP))
  if cached && (!errors.Is(err,io.ErrUnexpectedEOF)||errors.Is(err,ErrRemoteHTTP)) { t.Fatal("error-loss behavior changed") }
 }
}

func TestAuditCacheHashDropsCancellation(t *testing.T) {
 _,err:=decodedCacheKey(context.Background(),FileQuery{keySource:&KeySource{Kind:"file",SHA256:"audit"}},&auditFailedObject{failure:context.Canceled},64,"audit")
 if !errors.Is(err,io.ErrUnexpectedEOF)||errors.Is(err,context.Canceled) { t.Fatalf("unexpected behavior: %v",err) }
 t.Logf("ReadAt cancellation returned as %v; cancellation identity lost",err)
}
```
