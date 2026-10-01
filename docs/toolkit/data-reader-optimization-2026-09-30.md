# Data 读取恢复与查询会话实施记录

日期：2026-09-30。基线 `8e4dab0455c8bee512c73912c26b31bc9605f6c2`，分支 `codex/data-source-optimization`。
对应 [Data 审计](data-code-audit-2026-09-30.md) R1、R2、R5、R6、R7；
R3/R4 见 [资源预算与准入](data-budget-implementation-2026-09-30.md)，R8 见 [回归与对照](data-regression-implementation-2026-09-30.md)。

## 读取错误与候选恢复

编码缓存摘要读取先保留底层非 EOF 错误，再判定短读。因此包含部分字节的网络失败、取消或 deadline
不再被替换为 `UnexpectedEOF`。不完整的 EOF 仍是短读错误。

每个 EKey 的打开、完整缓存摘要读取、BLTE 解码及关闭属于一次候选尝试。
可用性失败发生在 lazy reader 中时也可进入下一个已认证的 EKey；最后仍失败时保留远程错误来源。
取消、deadline、预算耗尽、损坏、格式错误、缺钥和对象关闭失败均立即停止。
失败尝试的工作与网络费用不退款；残留 staging 内容不作为完整内容发布。

`recovery_read_test.go` 覆盖启用/禁用 decoded cache、部分错误读取、打开后 HTTP/missing、
所有副本失败、校验损坏、取消、deadline、短读与关闭失败。健康副本必须返回完全相同的原始内容。

## 一次查询拥有的存储会话

SQL、导航及单表组合路径共享固定 pin、安装/CDN、离线选项、内容变体、KeyFile 与 key provider 的会话。
会话延迟准备 Encoding 和 Root，定义或空查询不必打开 CASC。
callback provider 通过私有会话 binding 使用；函数代码地址不作为闭包身份。
准备、返回缓存视图和发布前继续验证固定来源，关闭会话释放句柄、准入及重型对象引用。

Encoding、Root、至多 32 个定义绑定的静态表视图在查询内复用。
Root 的共同解析器先完整验证 framing 与 delta，再保存每个 group 的紧凑 FDID 数组并做二分查找；
locale/content flags、名称摘要、所有匹配和歧义判断仍保留。
没有跨 CLI 的可变来源信任缓存。

本地会话只枚举一次当前 local index generations，逐文件验证既有 checksum、guard、排序和碰撞规则，
按需保留最多 64 MiB 的紧凑索引。淘汰不会省略验证，重读消耗累计预算。
完整 EKey 及 archive extent 验证继续执行，不用九字节前缀授权对象。
发布前重新枚举 generation 并对实际路径重新读取 SHA256；同大小替换或改写也会被拒绝。

## 减少重复缓存读取

非 sparse CDN `ReaderAt` 保存最近一个 256 KiB block，使用互斥锁保护；
连续的 64 KiB 摘要读取可在该 block 内复用，跨 block 淘汰只增加 I/O。
sparse 的精确 range 路径保持原请求范围。新 CLI 仍重新验证落盘 fragment 的 SHA256、range shape 和 size；
对象身份、BLTE chunk/page 校验及完整内容 CKey 不变。

decoded cache 命中及紧接着的表消费者复用同一份已经验证的原始 bytes。
缓存键仍包含原始编码内容摘要及 key document 身份，callback provider 不参与落盘 decoded cache。
新请求重新验证，不以 mtime/size 代替摘要。原有 partial 内容不可进入完整 decoded cache。

## Hotfix 快照与引用

同一查询最多保留 16 个已验证的 Hotfix capture，原始内容合计不超过 128 MiB，并受共享预算约束。
每个 capture 的来源、固定 pin、build、完整性与 layout 验证后只扫描一次，按 TableHash 保存有界目录。
排序、重复记录选择、状态及 provenance 保持原语义。
删除 patch 只保留条目和来源；有效替换只保留解码结果，不把整个原始 capture 留在 patch 闭包中。
原始快照由会话拥有，关闭后释放引用。

## 临时解码预算

BLTE layout、range offsets 与缺钥目录在分配前扣减累计元数据/保留费用。
encoded、decoded、嵌套 frame、解密、zlib 内部状态和缺钥占位写入使用可释放的临时额度，
临时额度与长期保留 bytes 共享 512 MiB 上限，嵌套层同时占用的额度不得相互覆盖。
额度一直保留到该有界复制或目标写入结束，在失败和取消后也释放。
原有单块 64 MiB、65,536 块、16 层及完整性错误分类保留。

这是保守的逻辑分配估计，较小 framed block 可能按允许的输出上限预留；
不是 heap/RSS 实测，也不保证 Go GC 立刻归还物理内存。
测试证明预算不足时没有读取 chunk body 或写出当前 chunk，覆盖成功、checksum、取消、嵌套与 range 写入的释放。

## 验证边界

局部 build/vet、records/container/resource 与导航 fixture 测试通过。
端到端工作计数、原始内容对照和数据集身份详见 R8 实施记录；完整分支验收另见总验证记录。
真实网络冷缓存、实际游戏安装的完整 FDID storage oracle、实际峰值 RSS、三客户端交互验收为 `not_run`。
没有执行 addon/game 输入、调整版本或发布；live 内存扫描及 addon 发布端优化留待后续任务。

最终审查追加：CDN loose/group/archive 三阶段均拒绝混合的预算、取消或 deadline 错误，
不会因为同时具有 HTTP 错误身份而继续探测。语义 metadata 逐行预留，坏行只收集有界 token；
文件/公共 key 文档在读取和解析前预留，公共 key 的首次下载及每次 redirect 共享查询网络预算。
原有固定 key document SHA256、惰性加载及 offline 行为保持不变。
