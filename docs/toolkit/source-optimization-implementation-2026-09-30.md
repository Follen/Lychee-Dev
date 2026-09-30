# Source 查询与容量优化实施记录

日期：2026-09-30。基线：`8e4dab0455c8bee512c73912c26b31bc9605f6c2`，工作分支 `codex/data-source-optimization`。
本页记录 [Source 审计](source-live-optimization-audit-2026-09-30.md) S1、S2、S4、S5 的生产实现与 fixture 证据。
S3 与跨 CLI LuaLS 会话由独立实施记录说明。本页记录开发分支验收；没有发布、修改版本或执行 live/game 输入。

## 已实现

### S1：请求拥有验证快照和文档读取

`QuerySource`、`RelateSource`、`ContextSource`、原文检查与相关组合入口建立请求上下文。
`EnsureIndex` 和后续查询共享同一确切 pin 的已验证 records 文件句柄，Context 内部的 Relate 继续共享它。
最多保留四个快照；超过这个请求内复用上限时按独立打开与关闭处理，结果不变。
每个新 CLI 请求重新验证 records 的完整 SHA256，未使用 mtime/size 作为内容证明。

验证时每个至多 64 KiB 读取检查取消。已认证 offset 包含每条记录的原始 SHA256；
按 offset 读取仍校验精确字节和换行，因此持有句柄不授权外部改写后的记录。
完整扫描回退也重新核对原始 records 摘要。缓存目录替换后，请求继续使用原有句柄。

文档按 Git object 与内容 SHA256 合并读取；请求内 memo 最多 256 个条目、64 MiB，超限清空后重新读取。
所有返回的文档仍核对长度、SHA256、UTF-8；Git `cat-file --batch` 额外核对对象 framing 和 Git blob SHA1。
同一请求中的不同文件共用一个 Git batch 进程，结束时关闭句柄、输入、输出并回收进程。
淘汰只增加 I/O，不改变排序、结果或内容摘要。

### S2：有界认证 offset 索引

新 snapshot 在同一原子发布目录写入 `offsets.json`，manifest 记录 schema、完整摘要与长度；
sidecar 自身绑定 records 的完整 SHA256，并覆盖原始 JSONL 连续位置。
元数据包含类型、path、symbol ID/name/target/category/signature/line，保留原记录顺序。
Search 在元数据层筛选匹配；Relate 按 ID/名称与关系方向筛选；Context 按 callee、加载目标和 frontier 筛选；
文档 lookup、API 环境与语义 definition 投影也可只解码需要的记录。

每个 sidecar 至多 128 MiB、1,000,000 条。解码逐条检查条目上限和取消；
构建按已编码元数据长度增量限制，避免构建完无界对象后才判断超限。
超限 manifest 标记 `derivedState=budget`，保留原文件缓存的完整扫描，未使用旧 SQLite。
旧 manifest 在验证同一 records 流时有界重建内存 offset；已声明但缺失的 sidecar 从已验证记录重建，
并核对重建 sidecar 的既有摘要。损坏 sidecar 或记录仍返回 `ErrSourceIntegrity`，
保留具体 `codebase.index_*` 原因，组合命令按无效输入映射退出 4；不触发重新下载回退。

这一步减少完整事实 JSON 解码和后续 records 读取；它仍在请求内遍历有界元数据进行筛选，
没有宣称所有名称/正文查询都变成常数时间。`coverage.scannedFacts` 继续计声明查找阶段实际访问的记录，
因此方向筛选后该工作计数会降低；来源覆盖、partial 与 truncation 判断不改。

### S4：正文 topic 在 Git 输出之前限制

`git grep` 的固定 commit、禁用外部 Git 配置与无 textconv 行为保持不变。
Lua/XML/TOC 使用大小写不敏感的 glob pathspec，API 使用生成文档目录 pathspec。
原来的 `bodyTopicPath` 后置检查继续保留。这样无关 topic 命中不消耗 4 MiB 输出预算。
fixture 正文扫描也先筛文档路径。

### S5：同一容量锁内增量核算

初始 reservation 在持有 `source:v1:worktree-capacity` OS 锁时对真实 `source/v1` 做一次有界 census，
包括 worktree、事实、阶段目录和所有已存在未知内容。此前 reservation 会立即重复遍历一次。
Grow 使用 census baseline + 累计 reservation grants 的保守上界，包括已写入和尚未写入字节，
不再每个 chunk 重走 namespace。接近额度时只枚举可回收环境/语义缓存，复核其内容并取得对应键锁；
活跃、受保护或未知缓存不删除。

锁持续到发布或放弃阶段结束，所有在 namespace 内写入的合作写者必须先获 grant。
没有跨 CLI 的 mtime 账本，也没有把崩溃阶段文件当作零字节：下一次 reservation 重新 census。
4 GiB byte quota、500,000 节点 census 上限、link/special-file 拒绝和取消检查保留。
LuaLS 自发写入的临时目录须位于单独的 `home/tmp` namespace 并遵循独立有界资源合同，
不能绕过该容量锁写入 `source/v1`。

## 本机 fixture 验证

新增测试位于 [source_optimization_test.go](../../internal/codebase/source_optimization_test.go)。
以下是隔离 Git/文件 fixture 工作计数，不是 Blizzard 完整源码或真实客户端时延。

| 场景 | 实测 |
|---|---|
| 一个文件 40 个 Widget 声明，EnsureIndex + precise Search | 40 结果；完整 records 验证 1 次，同文件 document 读取 1 次，Git batch 进程 1 个 |
| 同请求再读取第二文件 | document 总读取 2 次，Git batch 仍 1 个；结束后 records/batch 均已关闭 |
| 2,002 条记录中查找一个固定声明 | JSON 解码 2,002 → 1；records 读取 598,913 → 271 bytes；仍访问 2,002 条小型元数据 |
| 上述索引磁盘放大 | records 598,913 bytes，offsets 510,791 bytes，二者合计 1.853 倍 records 大小，不含 manifest/逐文件事实 |
| 初始 reservation + 7 次 Grow | namespace 完整遍历 1 次；下一 reservation 重新 census 并拒绝超过 quota 的崩溃阶段残留 |
| 无关 Lua 正文输出超过 4 MiB；查询 XML topic | 返回唯一 `Relevant.XML` 正文命中，无无关预算截断 |

测试还覆盖 precise/exploratory、topic、cursor 下一页、重复符号消歧、关系与 Context 深度/字节分页的
offset/完整扫描结果等价；损坏 sidecar、缺失重建、旧 manifest、同句柄外部改写、新请求重新验证、
每 chunk 取消、空 snapshot，以及容量活跃锁/保护路径/未知内容/未写 grant。

每个编辑批次的 `go build ./...`、`go vet ./...` 通过；`go test -count=1 ./internal/codebase`
已全量通过。全仓库强制 Lua 5.1 验收由整体任务统一记录，不以本页局部测试代替。
本机 symlink fixture 因创建权限不足跳过，故该 OS symlink 创建路径为 `not_run`；生产拒绝规则仍在。

## 未测边界

完整 Blizzard source 的冷/暖 p50/p95、峰值 RSS、真实磁盘放大与多年累积 cache 的 census 成本为 `not_run`。
fixture 证明去除重复工作和预算/结果边界，不证明完整源码绝对时延或内存收益。
互动游戏、WGC、reload/relogin 与三客户端 addon 验收未执行；本页实现不修改 addon 或 live。
