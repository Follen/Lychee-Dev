# Source Facts 分段缓存设计候选

状态：设计与源码读审，尚未实现。2026-10-17，依据固定 Rust d090b95e9222108fa474cd9d49b8453834d47813 与实际受测产品 30d061be1ca928ff5e8c353dbead820bc967153c。

本文不是新的交接入口；当前决定、执行次序与验证状态仍以 [handover](../../handover.md) 为准。全仓架构实现已有授权，本文不另设逐小片用户审批门禁。下面列出的错误次序、缓存可回收性与成本取舍必须在实际实现中明确，不能把设计候选或内部布局变更写成已证明的合同等价。当前没有修改生产缓存、增加 release 或运行 Cargo。

## 1. 结论与问题范围

建议保留一个可实施的结构候选：

1. 至多 128 条、总长至多 8 MiB 的不可变 Facts segment，内装原 v10 envelope。
2. 沿用原 path/input/parser key。全局 key → locator 使用现有 Metadata 的小文档行，新增一次至多 128 个 locator 加至多一个容量 counter 行的受控事务；不另造全量 catalog/HEAD 存储引擎。
3. 段对象参加现有 cache/objects 配额及显式 prune，但需要 Source 专用的 held-directory 发布适配，不能直接替换成现有 pathname publisher。
4. 大 envelope、可选内存未准入或优化目录容量已满时保留原 loose 路径。正常 I/O、锁、取消、close 或不确定发布错误不被伪装成缓存 miss。

这能把许多小文件发布合为少量段发布；仍需逐输入解析、生成完整 Facts、序列化、校验和构建公开索引。它不是已测收益，也不是保证 9.55 秒能降到某个数字。

最重要的新增成本是：每个段的 SQLite FULL 事务、locator 查询、段读放大、额外有界内存、共享 quota 扫描和串行提交。当前 d090 的锁细分尚无运行结果；后续一次匹配产品的 4036 profile 会补充判断，不为本文另建发行产物。

## 2. 实测证据，不能相加的时间

[30d 原始结果](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6228621264)：

- 4036 文档，五个完整 Facts 数组、全部字段及顺序 0 差异。
- 冷索引单 pair：Go 8457.3107 ms，Rust 9551.7776 ms；不是 20-pair 性能验收。
- Rust 内部 elapsed 9362.670 ms；facts_publish 4036 次、4930.703 ms。
- 其内 lock 1593.900、stage 1496.224、rename 1313.853、write 276.911 ms；前三项合计 4403.977 ms 是发布内部的路径 wall time。
- parse 累计 6493.808、worker 累计 4775.167、worker_wait 383.340 ms 与主链有重叠，不能与发布或命令总时间相加。
- 没有 CPU/I/O trace，不能把路径时间称为物理磁盘瓶颈、真实锁竞争，或推断换 SSD 有效。

不能用 4.93 秒减掉若干假定成本后声称得到新命令时间。减少发布可能暴露原来重叠的 parser 等待。

## 3. 为什么不先迁移 content key

### 3.1 固定输入的真实分母

使用正式 [oracle](../../tests/rust-source-full-corpus/retail-oracle.json) 的固定 Gethe commit **31c7f7b9cc79e56c986b365c06a6afbcf3c9177b**，读取 [完整 Git tree](https://api.github.com/repos/Gethe/wow-ui-source/git/trees/31c7f7b9cc79e56c986b365c06a6afbcf3c9177b?recursive=1)，truncated=false。

复用 [assertCorpus](../../tests/rust-source-full-corpus/assertions.mjs) 的 Lua/XML/TOC 后缀筛选。4049 文件/32665129 B，4036 文档/32269051 B，扩展名计数均与 oracle 一致。这里按 **精确 parser mode + Git blob SHA-1** 分组；同一 Git blob 表示相同原字节。没有把 Git SHA-1 冒称输入 SHA-256，也没有声称本次重新完成了 archive/每文件 SHA-256 认证。

| mode | 文档 | 不同 mode/blob | 可省重复发布 | 重复原字节 |
| --- | ---: | ---: | ---: | ---: |
| 普通 Lua | 1939 | 1909 | 30 | 8559 |
| generated Lua | 612 | 612 | 0 | 0 |
| XML | 1091 | 1085 | 6 | 10761 |
| TOC | 394 | 393 | 1 | 232 |
| 合计 | 4036 | 3999 | 37 | 19552 |

只有 18 个重复组、37/4036 = 0.917% 的额外路径副本；最大组是 50 B 的 Localization.lua，共 15 份。

全新缓存若采用“新内容键 → 旧 path key fallback”，只省 37 次发布，却新增 3999 次旧键 miss。第二次 miss 的 Windows 成本未测，不能从旧 facts_read 289.979 ms 直接推出新增耗时。worker 已可能在查缓存前完成投机解析，因此重复命中也不保证省掉 parse。结论是**不优先实施此冷索引优化**，不是声称已经测得变慢。

相同 path+bytes 跨 snapshot 本来已能复用；内容键新增的是不同 path 的复用和 rename 场景，不能把已有收益算进去。

### 3.2 可复核分组清单

每行是 Git blob、mode、copies、每份原字节；额外副本数为 copies−1。路径可在上述固定完整 tree 中按 blob 精确还原。

| Git blob SHA-1 | mode | copies | bytes |
| --- | --- | ---: | ---: |
| 0e89f959f7ad6d81c137f0ed0892132439bd969d | lua | 2 | 209 |
| 93b61b1ee65d34c5777953d4efbc73b7b934f130 | toc | 2 | 232 |
| e7b2086e12e8a1c59610914ea4d0ed9c4272e5a7 | lua | 15 | 50 |
| d32f88117441d7d83cf048f65c2fdf6bcc567ee6 | xml | 2 | 5548 |
| 0f9e592cb9c6640f5c920e8cd4a3c906678d800a | lua | 2 | 475 |
| 182a98d4d0bac538b29afd1207ed434b2964eee0 | lua | 2 | 1345 |
| ba1cb79a78eca63f2db601e2bd42708f615ff6e6 | xml | 2 | 606 |
| 6d6d5c3adf7d7e2e2d1e2d677b2d942aaf5aebef | xml | 2 | 551 |
| 9f22d0ee453f5e3048089c82eabffdb8c6d688d4 | xml | 2 | 2508 |
| f71e709cfebf5c13a2ead179859d647b5b65739f | xml | 2 | 1305 |
| 533eeafebdad494674586da2602162b1b1dc7cde | lua | 2 | 46 |
| 3bf6772593af570cb970f02dabc96d64b3d7fe84 | xml | 2 | 243 |
| 9137abf96a3694be8f8ac3a1379d7bbafa04ca9a | lua | 2 | 870 |
| 74b0eb62a9939cc8709fd2fd45b324cd535a6ef8 | lua | 3 | 2244 |
| 4ba280517af592b8942721394f804259e7b2d6dd | lua | 5 | 3 |
| ff05cc7cde8d600a3fa8096912c81414f596fae6 | lua | 2 | 312 |
| 98262e55c1c5aabd3e38d87b0052664e721d4af9 | lua | 3 | 49 |
| 7489accbf21dadfed2aa6bba3be0273032214a16 | lua | 3 | 2 |

复核算法：只取 type=blob；文档匹配大小写不敏感的 Lua/XML/TOC 后缀；Lua generated 仅由原字符串包含大小写敏感的 /Blizzard_APIDocumentationGenerated/ 决定；按 mode、blob SHA 分组。不得规范化目录大小写、反斜杠或根目录缺少的前导斜杠。

本次完整统计 JSON SHA256：7d0d7b9943d1a78481a288191424997fe956d1884cd0bc1ef89b7a612eb24767。它包含全部 18 组路径及证据来源；本表与固定 tree 足以独立重算分母。

### 3.3 语义证明与保留边界

[facts.rs](../../crates/lycheedev-source/src/facts.rs) 的公开 parser 只收原字节，Lua 另收 generated bool。line、endLine、scope、诊断列号及 XML owner/tag 都来自原字节；loads.path 是文件内的引用目标，不是当前文档路径。[index.rs](../../crates/lycheedev-source/src/index.rs) 在生成 Symbol、diagnostic、LoadFact、stable_id 时另附当前路径和 pin。

raw_relationship_targets 为 serde(skip)，磁盘命中一直会丢失 fresh-only 原始字节身份。其 validate 消费者使用 [Closure](../../crates/lycheedev-source/src/closure.rs) 的独立 fresh parse，不能把此缓存扩到 Closure/native 分析。[现有验证测试](../../crates/lycheedev-source/src/validate/tests.rs) 必须保持。

若以后单独恢复 content-key 候选，保留四态 mode、原 v10 envelope 和 32 MiB 边界；旧命中不强制迁移发布。不能把新加 envelope 字段造成的字节边界变化混入性能优化。

## 4. 当前合同与具体代码落点

| 当前事实 | 落点 | 设计约束 |
| --- | --- | --- |
| 原 path+input+parser key，v10 envelope，32 MiB | index.rs 的 facts_cache_path/document_facts_profiled | segment v1 不改键和 envelope |
| 每 doc 同步 publish、检查 close 后返回 Facts | document_facts_profiled；Core SourceCache::publish_locked/finish | 批量改变此时间边界，必须显式描述 |
| 缺失、无效 JSON/schema/hash/超长缓存可重建 | SourceFactsCache::read；document_facts_profiled | 只有已定义的内容 miss 可以 fallback |
| held Directory，每次必要操作验证真实祖先身份 | Core cache/source_facts.rs；filesystem/destination | 不能以一次路径检查替代 |
| rebuildable seal 不执行 fsync | filesystem/destination/staged.rs | 不把新 SQLite FULL 提交成本隐藏 |
| 全部文档完成后 index blob，再 ready manifest | index.rs build_material_with_control；Source persistence.rs | Facts cache 可见不等于 index ready |
| metadata 每文档 ≤1 MiB，controlled BEGIN/COMMIT/rollback/close | metadata.rs；metadata/controlled_open.rs | 可增 bounded multi-row transaction；不是每条单独 put |
| cache/objects 才进入通用 quota/prune | Core cache.rs、cache/publication.rs | 私有新目录不会自动受其管理 |
| 原 256 MiB source/worker 准入，8 个 in-flight slot | index/facts_pipeline.rs | 不是 8 CPU 上限，也不是整进程 RSS 上限 |

当前 invalid_utf8_git_blob_aborts_index_without_publishing_a_ready_manifest 测试明确：前面成功文档可能留下可复用 Facts，后面编码错误不能产生 ready index。它没有承诺一次失败必须为所有先前文档提供磁盘 checkpoint。

## 5. 段格式与精确上界

采用一个可选的 SourceCacheSession，消费原按序 parser 结果。不把 parser 变成存储线程，不并发提交文档，不新建发布线程。

### 5.1 Segment v1

- 一个内存 buffer，含 header/frame/payload 总长度 ≤ 8388608 B。
- ≤128 条记录；空段不发布。
- header 固定 64 B：magic/version/recordCount/totalLength/保留位；所有保留位必须为零。
- 每条 frame 固定 68 B：旧 key 32 B、payloadLength u32、envelopeSHA256 32 B；后接原样 v10 envelope。
- 总长的 checked arithmetic 为 64 + Σ(68 + payloadLength)。单记录放不下（最大 8388476 B payload）时，先处理前一批，再走原 loose 路径。
- 原 envelope 仍允许最多 33554432 B；不得把 8 MiB 当成新的用户 Facts 限制。
- 名称是整个最终段的 SHA256。内容不含 snapshot、document ordinal、时间、PID 或 owner token。
- 大小、记录数、重复 key、frame 边界、整数溢出、尾随字节、整段 SHA、记录 SHA、内层 inputSHA/factsSHA/schema 均验证。
- 任何 JSON 解析与反序列化仍受原完整 envelope 长度上限。offset/length 不能变成任意文件读或分配请求。

buffer 是明确取舍：多一次有界字节复制，换取无需新增跨进程磁盘 reservation 协议。只缓存一个段，不保留所有 4036 个 envelope；原 Facts/源码所有者不复制。

### 5.2 Locator

Metadata key 为新固定命名空间加旧 64 位 hex key，例如 source/rust-facts-segment/v1/<old-key>。不是新 content key。

每行 JSON ≤512 B，含 schema、segmentSHA256、segmentBytes、payloadOffset、payloadBytes、envelopeSHA256。key 已绑定 parser/path/input 身份；读时仍使用当前原始输入的 SHA 验证 envelope。每次事务 ≤128 locator、总序列化 locator ≤65536 B；另至多更新一个 ≤512 B 的容量 counter 行，因此这两类序列化 value 合计 ≤66048 B，低于现有每行 1 MiB 上限。此上界不包含 SQLite 的 generation/history/control 写入及页开销。

读侧必须先用 SQL length 投影限定 ≤512 B，合格后才取 value；不能直接调用最多先分配1MiB的通用 read_controlled 再 serde 检查。批量清理也使用相同投影、行数和累计字节上界。这是拟新增的窄读取方法，不是现有 API 已提供的保证。

不得保存绝对路径、任意相对路径或 locator 提供的文件名。实际对象位置只能由验证后的 64-hex digest 推导。

## 6. 为什么全局 locator 必须存在，以及为什么选 Metadata

只在某个 snapshot 的 index manifest 放 pack descriptor 不够：下一个 snapshot 不能按旧 key 找到旧 segment，会失去现有跨 snapshot 文件缓存复用。

首选复用现有 Metadata：
- 每个 key 一行，两个不同 snapshot 的 writer 可以增加不同 key。
- 同 key 的两个完整结果按现有 generation/history 规则更新；不能绕过 deleted_generations。
- 一次 ≤128 个 locator 加至多一个容量 counter 行的受控事务，全提交或全回滚。
- 事务内读当前 generation，再按相同规则更新；不把操作开始时的陈旧 snapshot 当成可覆盖全表的目录。
- 不需要重写全量 catalog、替换 HEAD 或另写 LSM/B-tree。

不选 full immutable catalog 的理由：100000 行、每行 104 B，每 128 新 key 重写一次且保留旧 generation，累计目录行字节约 4.08 GB；这还没有算段、header、临时文件。它虽限制单个 catalog，却未限制累计磁盘和读写放大。

### 一操作连接

Metadata::open_controlled 已支持保留一个连接；read_controlled/put_controlled 是该连接的方法，close 显式消费它。不能在每个 doc 调用会重复 open 的 Source persistence::read/put_manifest wrapper。

当前 build_material_with_control 的初始 manifest 读取连接在 cold build 前已经 checked close，最终 put_manifest 另有独立 owner。因此这里是新增一个贯穿 Facts 操作的 locator 连接，不是假称复用仍然打开的初始 manifest 连接，也不是整个 index 命令只 open 一次 SQLite；最终 ready manifest 保留原独立关闭与错误边界。

SourceCacheSession 在真正需要 Facts 时懒开一个 locator Metadata owner：
- 完整 ready-index warm 命中且没有读取 Facts，不开启它。
- build 的 session 覆盖整次有序消费。
- load_facts 操作传入同一 session；不要为每个 XML/TOC 重开 SQLite。
- 保留原 loose owner 已有的 close 次序；新增 locator/segment owner 在外层唯一 finish 合并关闭错误。
- 不缓存逃逸的 Statement/Rows/Transaction；使用短 scope 或受控 prepared statement，并在 owner close 前销毁。

现有 put_controlled 不是 batch API。需要小而明确的新实现：复用原 BEGIN IMMEDIATE、零内部 busy_timeout、逐语句 OperationControl、原绝对 deadline、COMMIT 可重试 guard、显式 rollback、恢复设置及 checked close；禁止循环 128 次现有 put 来冒充一次事务。

### SQLite FULL 的真实代价

现有 Metadata 写连接设置 synchronous=FULL。每批 locator 因而新增一个 FULL 事务，而旧 loose Facts 的 seal 没有 fsync。保留现有数据库的 durability 设置，不在共享连接上切 OFF/NORMAL 来加速缓存。

这可能抵消部分段收益。必须实测 locator_begin/wait/commit/rollback/close 的 wall time、事务数及总命令时间。不能把此提交成本藏到 manifest 阶段，也不能先承诺更快。若 FULL 成为主导，再评估独立可重建目录；不先写自定义 SQLite VFS 或引入 unsafe。

## 7. Locator 容量与状态数据库膨胀

1 MiB 是每行上限，并不限制 locator 总行数或 SQLite 物理大小。v1 必须同时限制可选优化目录：

- 至多 100000 个曾准入的不同旧 key（包含随后删除行留下的 generation history）；每行 payload ≤512 B。
- 此为优化目录容量，不改变每次可分析的文档数。新 key 超出容量时走现有 loose 路径，不自动淘汰别的 snapshot，不报新的 source_budget。
- 一个专用小 counter 与首次插入在同一事务更新。首次准入同时查询 documents 和 deleted_generations：删除后重新出现的同 key 不增加计数。100000限制两表 key 的并集；同 key 可以同时有live/history行，不是两表物理行总数上限。
- counter 缺失检查涵盖 live 与 tombstone-only 状态；首次启用/不一致恢复检查只能作有界 namespace 投影，遇到第100001个不同key停止新增准入。删除不回收 lifetime 名额，避免无界 deleted_generations。
- 所有准入入口使用相同 batch commit lease；preflight 容量读取只是提前路由，最终 counter/generation、已有key/history和准入在同一写事务重新核验。与持lease时的preflight发生意外冲突，则保留 generation_conflict 及已发布段状态，不重放、不覆盖。
- counter 不存在且已有该 namespace 的 live 或 history 数据、计数异常或 locator 格式损坏时，不“修好”共享数据库；禁用该缓存目录的新增准入并保留诊断，正常 loose 重建继续。SQLite 数据库/权限/close 错误仍传播。
- 显式 locator cleanup 删除缺失段的引用时保留现有 generation/tombstone 合同，分批 ≤128 行、总扫描 ≤100000；不对共享 state DB 自动 VACUUM 或删表。
- 该保守 lifetime 上界意味着长期多个版本后可能回退到 loose。它是已说明的 v1 局限；不能声称目录无限扩展或 cache.maxBytes 包含共享 SQLite 全部页面。
- payload、key、history 数量可界定；SQLite 页、journal 和整个共享 DB 的物理峰值必须测量，不能拿 payload 上界冒称磁盘严格上界。

此容量计数是设计中的新增实现，当前源码没有此保证。

## 8. 发布协议、锁次序与并发 writer

最小新增状态为 Buffered → SegmentPublished → LocatorsCommitted。Unknown 必须单列，不能用一个 bool 把未确认提交当成未发生。

每批：
1. 在原有序消费者内生成完整 v10 envelope，检查原 32 MiB 规则，再追加到已准入 buffer。
2. 满 128、下一记录放不下、外部 side-effect barrier 或成功结束时处理该批。
3. 获取 Source 专用 bounded commit lease。它只串行该批容量判断/段发布/locator 提交，不在解析整批时持有。
4. 在无 SQLite 写事务时读取 namespace 容量、重查本批必要身份；容量不准入则按原顺序 loose 发布，随后清空 buffer。
5. 获取真正 cache:quota 与 cache:obj:<segment digest> 锁，在 quota 内作最终 admission、创建/写入/封存/替换段，显式关闭 stage、object lock、quota owner。
6. 仅段发布与这些 close 成功后，开始 ≤128 locator 的 SQLite transaction；提交后所有 locator 同时可见。
7. 释放 commit lease；新 owner 的 close/late control 错误并入原结果。失败不自动重试整个 batch。

锁次序：
- 已有 snapshot index lock（若有） → Source batch commit lease → quota → segment object。
- quota/object 彻底释放后，才进入 locator BEGIN/COMMIT；绝不持 SQLite write transaction 去等 quota。
- reader 不持 SQLite transaction 去读段。先取得一个小 locator 值并结束语句，再打开段。
- GC 遵循 quota→object；locator 清理单独在短事务执行，不持 object/quota 等 SQLite。
- quota/object close 与 locator COMMIT 之间，显式 prune 可能已经删除该段。因此 LocatorsCommitted 只证明行提交，不保证对象此刻仍在；立即悬空的 locator 也走正常 missing-segment fallback。不能同时声称无此间隙又释放全部文件锁后才提交。
- metadata schema lease 只在操作连接初始化时取得、释放，不在 batch 内反复打开。

这些次序必须用当前全仓消费者验证；新 Source 适配不能改变已有 Data immediate/wait lock 调用合同。全局 batch lease 会增加不同 snapshot writer 的串行区，是待测成本。

跨进程：
- 不同 key 的事务不会相互覆盖未涉及的行。
- 相同完整段用同一 content digest/object lock；existing winner 必须逐字节/长度验证。
- 同 key 指向不同但完整合法段时，只更新 locator，不原地改旧段。现有 reader 保留旧 owner/已验证字节。
- 不承诺只解析一次；两个进程可能同时 miss，随后按序安全提交。
- 段分组按原文档消费顺序且边界确定。相同序列可复用整段；插入文件改变后续分组会降低整段去重，locator 仍保留跨 snapshot 单 key 查找。

## 9. Quota：真实复用，不是假定私有目录受管

推荐对象放入 cache/objects 的已有 digest 命名空间，复用真实 cache.maxBytes、quota/object locks、对象计数及显式 prune。旧 source-facts-rust loose 文件保持原作用域，不迁移、不自动删除。

不能直接调用 publish_derived_controlled 代替 SourceFactsCache：
- 它目前是 pathname publication，调用 atomic_write_controlled/sync。
- 当前 Source 要求 held strict Directory、逐次祖先身份验证、relative stage/rename、checked close 与 rebuildable seal。
- 需要 Source 专用 held-directory object publisher：共享已有 quota/admission 计算和锁名称，保留既有 generic 调用行为；不能更改 Windows 打开 flags 或省略祖先校验。
- 新 helper 的 cache seal 保持 Source rebuildable 语义；不把其他 cache/objects 发布改成不 sync。
- 段 content digest 相同而已有对象损坏的恢复必须单独实现、验证：仅在同 object lock 下，由已认证原输入产生的完整候选替换；不能误采纳损坏 winner。
- 修复损坏对象的 quota admission 按已实际计入的旧长度与新长度计算净变化；不能把旧对象仍算在total里，又重复加一整个新对象。并发和overflow检查沿同一quota owner完成。

一个 buffer 准备好才进入 quota。stage 创建/写/rename 在 quota owner 内完成，不提前在磁盘写 8 MiB 后再请求配额，所以初版不需要 operation-wide reservation 协议。

配额不足的处理必须区分：
- 在任何段变更前，确知该可选优化不能 admission：回到原 loose 发布。这不是增加配额，也不改变后面的 index blob 原配额检查。
- permission、锁错误、stage/write/rename/close 错误或不确定 publication：保留原 typed 错误和状态，不改走 loose 隐藏它。
- 正常发布但 locator 提交失败：段是计入配额的 orphan，失败仍返回，不把它叫作回滚成功。

另一个不能隐去的差异：新段占用 shared quota 后，可能使稍后的 final index blob 无法准入；原 loose Facts 不占该 quota，因此“final index 的检查代码没改”不代表低配额时结果仍相同。当前 batch 的 pre-mutation loose fallback 不能消除这种影响。实现必须明确选择并回归这项共享容量行为；若要求保留原低配额结局，则还需等价的整体容量安排/共享 reservation，而不是猜测给 index 留一点空间。并发 writer 也须参与该安排，单操作本地预留不能证明空间仍可用。

现有 quota 统计 committed objects，cache status/prune 的 uncommitted 扫描仅覆盖 cache/tmp，且删除识别 put-<32hex>.part 及对应 stage lease。Source 同目录 .lycheedev-cache-* stage 若放进 cache/objects bucket，不会自动获得该 accounting/recovery 覆盖，会被视作非标准对象。实现需新增精确、可核验的 Source stage inventory/recovery 适配，或另作保持 held-parent/rename 身份的 staging 设计；不能只改目录名就宣称复用完成。

现有 quota 不是所有 staging/残留字节的严格总上限，本文不扩大该声明。新的 stage 必须沿原 bounded slots/owner/uncertain-residue 合同；没有 terminal/ownership 证明的残留继续保留。如果要把跨进程 staging 也纳入硬上限，需要真正共享、可恢复的 reservation 协议，不能用本地计数补上。

## 10. 部分进度、首错与取消

批量无法保持“每个文档返回前都已 rename 并 close”的原时序。这是具体行为差异，不等于改了五数组结果，也不能因为 cache 可重建就忽略：

| 情况 | 旧 loose | 分段候选 |
| --- | --- | --- |
| doc A 成功，随后 doc B 编码失败 | A 已完成发布 | A 可能仍 Buffered |
| 第 A 条 rename/close 失败 | 在进入后续消费前返回 | 可能到 batch barrier 才暴露 |
| 进程在 batch 中崩溃 | 已完成的每条 loose 可复用 | 最近未提交 batch 最多128条需要重做 |
| SQLite COMMIT 结果不明确 | Facts 路径没有此阶段 | 保留 Unknown，不重放、不删已发布段 |
| operation 已取消/deadline | 原错误与 cleanup 合并 | 不为了保缓存进度继续 flush |

建议规则：
- flush 在后续不可逆/外部持久化 side effect 前发生，包括 loose 超大记录与 asset/evidence publication；不是每个纯解析前 flush。
- 固定错误排序如下，不能交给实现临时选择。令E为后续已观察到的错误：E本身是stop时不flush；E非stop但flush前control已停止时也不flush，保留E为主并附加该stop。只有E非stop且进入时control允许，才恰好尝试一次先前batch的flush。
- 上述flush成功则返回E；flush产生F（含开始后的typed stop）则以逻辑上更早batch的F为主、完整E为附加cause。join/close/late-control错误继续按既有所有者顺序附加。没有E的成功收尾中，flush失败本身就是主错误。任何Unknown发布状态都不触发整批重试。
- 这是明确的新batch错误排序选择，不是finish_historical会自动推导的规则。调用者必须显式排列两个完整错误，再使用原合并函数；不得丢失原typed code、details或先前已附加的join原因。测试需区分flush前stop与flush中stop，不延长deadline。
- 这仍不能保证故障发生的时间、系统调用次序与旧版完全相同，也不能声称未提交的 buffer 已持久化。
- 原 parser pipeline 的 ordered consume、typed cancellation、worker first-error、所有 join 不变；不得把 pending Facts/text 与 scratch 一起退款。
- 完成索引前 flush 全部 batch，并 finish 所有新 owner；之后才是原 index serialize/publish/ready manifest。
- 段/locator 已可见而 index 尚未 ready 是允许的缓存部分进度；不产出成功 partial index。

现有公开错误 schema、exit code、cause 层级应尽量复用。新增 mutation 状态必须明确属于 segment/locator，不能伪造旧 per-file facts 已 committed。应以故障 fixture 对比确定实际公开差异；内部布局改变本身不另加用户审批门禁。

## 11. Lazy 读取、完整内容身份与首次查询成本

读取次序：
1. 原输入的 path/size/SHA 验证及 control check。
2. 在同一操作连接查旧 key 的 locator。
3. locator 结构合法时，从 held directory 打开指定 digest 对象，验证整个段，再取完整 record 并按原 v10 规则验证。
4. locator 不存在、格式/版本无效、段缺失、hash 不符、截断或 record 无效：尝试原精确 loose path。
5. 两者均 miss：原 parser → 原完整 envelope → 正常选定的发布路线。

不要先信任 range，再异步校验全段。全部 hash 与提取必须基于同一个真实 owner 的字节；把一次 pathname 检查当成未来身份不允许。unsafe path、权限、checked close、typed stop 等并非内容 miss。

读放大是明确的代价：
- 一条很小的 Facts 请求可能读取并 hash 整个 ≤8 MiB 段。
- v1 最多保留一个已验证段 buffer；按实际 capacity 准入，换段前释放旧 buffer。不是无界 HashMap。
- 一个段多次被交错访问可能重复读；最坏不能按每段只读一次算账。
- 不 preload 全部段，不在 ordinary query 没有请求 Facts 时打开 locator DB/段。
- XML/TOC load evidence 继续认证原字节并附当前 document.path；不能拿 locator 代替原文证据。
- source-originals 的 lazy 获取和 Git fallback 保持；warm index 不因为 Facts 段化而预读所有源码。

必须分开测 cold index、warm ready-index、首次 load/首次 query、重复 query、删段后重建。不能把冷索引省下的工作移到第一次用户查询后只报告索引变快。

## 12. 内存与可选 admission

新增可追踪内存：
- 一个 ≤8 MiB 段 buffer（以 capacity 计费）。
- ≤128 frame/locator 描述符，locator 序列化总量 ≤64 KiB。
- 一个 ≤8 MiB 已验证读段 buffer；构建时可复用同一缓冲或明确计双份。
- SQLite 自身 page/cache/journal 内存单独观察，不能宣称由上述 Vec 上限完全界定。
- 当前单条 ≤32 MiB envelope、Facts/Symbols/JSON 与 worker/source 所有者仍有其原峰值。

把新的 owned buffers 纳入 facts_pipeline 原256 MiB source/worker admission，不能另开免计费队列。优先使用一个显式辅助 reservation，并在原 worker 计算前扣除；允许因此少派 worker，但不固定 CPU 数。

可选优化不得把原本可处理的近256 MiB输入变成新 source_budget：准入不合适时从一开始用 loose；若运行中需要退出，先在已计费容量内完成当前 barrier、释放新缓存 buffer，再继续原路线。不能借未来退款补现在的内存，也不等到 allocation failure 后继续堆积。

这不是整个进程 RSS 上限；需报告 sampled WS/private、采样缺口和额外 SQLite 内存。

## 13. GC、孤儿、损坏与恢复

- 完整段是 cache/objects 的可重建对象。现有显式 cache prune 可回收它，已有 retention marker 仍受尊重；不自动按 age、PID 或“看起来没用”删除。
- 普通 source prune 当前明确排除 facts/evidence，不能把新段悄悄纳入那个命令；使用现有 cache prune 的明确范围。
- segment 已发布、locator 未提交：orphan 仍计入 quota，等待显式 cache prune；不冒认成功、不后台重放。
- locator committed、段被显式 prune/丢失/损坏：读时 miss→loose→原输入重建。locator 不是证据，也不保证段仍存在。
- locator 数据本身缺失：不扫描全部 cache/objects 猜段归属，不从未知 orphan 自行恢复关联；重新解析/发布即可。
- SQLite 整个库不可用/较新 schema/权限失败不是“目录 miss”，保持 metadata 错误；不得把共享数据库损坏隐藏成 parser 重建。
- batch COMMIT unknown：保留已知 segment identity、key范围和原错误；后续只读核状态，未确认前不自动重放或删除。
- stage unknown 保留原 bounded residue，显式确认 terminal/ownership 后处理；不扩成清理他人临时文件。
- locator cleanup 是新增显式、有界能力，必须保持 generation history；不能让 generic cache prune 顺手长时间锁住 SQLite。
- cleanup 在查到缺失段后，只能按当时观察的 generation **及完整 locator identity** 条件删除；进入事务后不匹配则跳过。否则会擦掉另一 writer 已指向新段的 replacement。容量 counter/history 的更新与条件删除同事务，不能按旧扫描快照覆盖。
- 新旧程序并发：旧程序只写 loose，忽略新 namespace；新程序可读 loose。两种路径不能删除对方数据。

缓存可回收性与新 metadata 行是可观察的运维变化，必须在实际 status/prune 文档与回归中写明。仅保持结果 JSON 一致不足以覆盖恢复合同。

## 14. 调用数、收益上限与未测成本

令 N 为本次需要新 Facts 的文档数，S 为实际段数，L 为 loose bypass 数。S 由8MiB/128条、side-effect barriers、容量和旧缓存命中共同决定，不能只取 ceil(N/128)。

- 原每 doc lock/stage/rename/stage-close/lock-close 约 N 套。
- 候选约 S 套段文件发布 + L 套 loose；另有 S 个 locator transaction、batch lease、quota/object locks。
- 序列化及完整内容总字节没有消失；新增 frame/header、buffer copy、SQL 行与 FULL commit。
- 通用 quota 每批扫描已有对象，必须记录扫描次数/对象数；不能以换了目录名假定 O(1)。
- 缓存旧 namespace 不自动迁移，因此初期两类数据并存；explicit prune 和 orphan 的空间成本必须计入。

锁/stage/rename 的4.404秒只是当前可针对的累计发布内部路径时间，不能作为保证可回收的秒数，更不是全部 Go/Rust 差距。没有逐doc发布时间、实际 segment bytes/分组及 SQLite 事务成本时，不给数字提速承诺。

## 15. 最小实现范围与正式回归

建议代码落点（本设计未改）：
- Source index.rs + 新 index/facts_segments.rs：session、有序 buffer/barrier、读取 fallback、完整 finish。
- Core cache/source_facts.rs 或相邻 Source 专用模块：held-directory 段 reader/publisher，复用 quota 核心，不改现有 generic 合同。
- Core metadata.rs 下小型专用模块：≤128个 locator 加至多一个 counter 行的 transaction、现有 generation/rollback/close 规则。
- facts_pipeline.rs 仅接新增实际辅助 reservation，不改调度、并发上限、join、首错。
- tests/rust-performance-paired/read-fact-delta.mjs 增加显式 segment-v1 存储布局及测试；历史 v10 仍指原 loose key/schema。

正式回归应覆盖：

1. **格式与边界**：0/1/128/129条，8388608 B段边界，frame+header开销，超过段但≤32MiB的loose路由；原32MiB+1错误不变；短header、溢出offset、重叠range、尾随字节、重复key、错误digest/schema全部真实fixture。
2. **完整语义**：现4036与原84边界输入，五数组全部字段、重复行、顺序；generated目录大小写和Lua/XML/TOC隔离；fresh-only raw identity现有验证不动。
3. **兼容**：旧loose-only、新segment-only、混合、旧程序写loose并发、同路径字节变更、跨snapshot保留/rename、未知layout，不扫描猜测旧格式。
4. **事务**：128个locator与至多一个counter行单COMMIT；第1/中间/末行及counter更新失败全回滚；busy/cancel/deadline；COMMIT unknown；rollback+close多cause；generation delete/recreate及容量counter原子性。
5. **目录与锁**：held ancestor替换race、reparse、parent identity、same-key/different-key多进程；锁超时不释放他人owner；stage/segment/Metadata唯一消费。
6. **部分进度**：已有好doc后坏UTF8、后续parse/asset/evidence错误；batch flush首错和附加cause；取消后不flush；未完成batch崩溃；只有最终ready manifest发布成功才算index完成。
7. **恢复与GC**：rename前/后、segment close后、locator COMMIT前/后故障；全部段丢失、部分截断、损坏winner重建；explicit prune与正在读/写进程并发；orphan计quota；unknown stage保留；locator cleanup保留history。
8. **预算**：额外capacity实际计费；强制低余量回loose不引入新的source拒绝；较大envelope；quota不足的pre-mutation路由与真实I/O失败不混；达到100000 lifetime key后旧/新key行为。
9. **lazy行为**：暖index不预读段；首次load/query、重复query、交错段读取、删除缓存后的Git fallback、原文完整hash与path归属；记录读放大。
10. **正式测量**：同一封存候选、原native时钟/owner回收/公开输出等价；profile显示实际S/L、段字节、locator rows/transactions、SQLite FULL耗时、quota scan、读取字节和fallback原因。保留既有20-pair目标，不用单次profile代替。

现有正式组：Source index::、facts::、validate::，Core cache::source_facts::、storage::、metadata controlled/历史generation相关组；新增文件测试并入相应原组。Node reader test 仍逐字段比较，不能导出一套loose文件作为未计时预热来伪装原读取路径。

本轮执行的是源码/connector读审、固定tree重复分组和文档静态校验；未运行上述 Rust/Windows/产品回归。下一步由当前统一批的真实锁细 profile与设计取舍决定生产实现，不插队当前唯一Windows验证队列。
