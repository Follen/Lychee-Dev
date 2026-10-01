# Data R8 自动回归实现与运行记录

日期：2026-09-30。工作分支：`codex/data-source-optimization`。对应 data audit R8；未改版本、发行、游戏或 live。

## 已实现

- `BenchmarkDataPreparation` 覆盖二十个真实准备链场景：相同合成 CASC 数据从 local archive/index 或 offline CDN fragments 读取；每种来源有单表、双表 JOIN、同表 static+effective、500 条 Hotfix 删除与技能导航，并分别有 cold decoded / warm decoded 状态。
- cold decoded 只删除 decoded metadata，并在计时外完成；原始 fragments、不可变 blobs 和 OS 缓存继续存在。热档在计时外先运行同样查询。所有测量均包含证据提交，不把它们叫做真实 CDN 冷下载或冷机器。
- 基准记录时间、Go 累计分配、work/network 预算及 SQL prepare/hash/decode/blob/fragment 观测。共享 static/effective 来源的 telemetry 按 FDID 去重；逻辑 source bytes 继续按查询绑定计数。导航没有公开的细分阶段或读取 telemetry，未测的字段不输出伪造的零。
- 构建脚本要求 CascLib commit `38a34665624b8775bb875274b36191b21c38d97b` 且 checkout clean。现有 gcc/g++ 加 pinned source list 与 bundled zlib 构建 test-only adapter；不安装工具、不下载源码，保留 LICENSE 与 executable digest receipt。
- decoder corpus 用已存在 provenance 固定的真实 264 字节 DB2，以 headered N 和 Z 两种合法 BLTE 与 CascLib 严格读取逐字节摘要对照。
- 损坏 chunk checksum 要求两端严格拒绝。缺钥政策由 Go 独立断言 `ErrKeyUnavailable` 和零输出，raw 缺钥 oracle 不属于已验证交集。
- 完整 storage harness 已实现：明确 pin/build/config、FDID、locale、variant、完整选定文件钥匙政策和固定 expected SHA256。先执行 Lychee 严格 local/offline selection，再验证 oracle 的 product/build/FDID/locale/variant/CKey；禁用 encrypted overcome，拒绝 partial、歧义、默认选择替代及输出覆盖。
- Runner 使用 JSONL 保存执行证据与独立 summary；外部 oracle 的 skip 不算 verified。完整操作说明在 [tests/data-regression/README.md](../../tests/data-regression/README.md)。
- Runner 必须找到全部二十个基准组合、指定重复次数、必需 metrics 和 package pass；oracle 必须找到 parent 与四个已知子测试 pass；storage 必须本次生成完整 verified comparison artifact。仅 exit 0、空测试、部分结果与缺少指标不能算 verified。Node 政策测试覆盖完整/缺失/改名/重复/skip/fail 与 artifact 缺失。

## 已运行

Windows amd64、Go toolchain 1.27.1、9800X3D。采样时分支其他并行验证仍在运行，所以时间属于本次观察，不是稳定基线门槛。

| 验证 | 结果 |
| --- | --- |
| `go build ./...` / `go vet ./...` | pass |
| 固定 CascLib 38 个 translation units 构建 | pass；使用既有 UCRT64 gcc/g++ |
| `go test -tags=casc_oracle ./tests/data-regression/compare` | pass；manifest shape / bounds / explicit arguments |
| fixture oracle runner | verified：headered N、Z 均与 Go/原始 fixture 一致，checksum 损坏两端拒绝；Go 缺钥零输出独立 pass |
| 二十项端到端基准，`-benchtime 3x -count 1` | verified；全部场景完成 |
| 二十项端到端基准最新复跑，`-benchtime 1x -count 1` | verified；包含数量与 Hotfix 删除断言 |
| `TestDataPreparationFixtureCacheStates` | pass；local/offline CDN 冷热提取次数与 hash 覆盖断言 |
| `TestReadSession*` | pass；回调绑定、关闭释放、预算先于 blob 读取、同查询共享与新查询篡改检查 |
| `node --test tools/data-regression.test.mjs` | pass；runner 证据完整性与错误分类 |

原始 fixture 与两端输出一致：264 字节，SHA256 `992c9edb6bdaa11a0001e846d267b89a8477d3fed6fb7b4497c9dc642235c73a`。它是 decoder-component 验证，不是完整 CASC storage 身份验收。

同一采样中，单表与 static+effective 的 cold 档为两次完整内容提取，warm 档为零次完整内容提取、两次 decoded cache hit；双表 JOIN 为三次/零次提取、warm 三次 hit。热档仍重新 hash source，并读取已验证 blob；因此 cache hit 不能被解释为没有 I/O 或严格内存减少。所有 offline 场景的网络正文和请求数为零。

新基准另报告 `charged-retained-B/op` 与 `charged-metadata-B/op`，用于区分保守查询预算预留和 Go 累计分配 `B/op`，三者均不是 RSS。Runner 末次复跑使用隔离工作区 GOCACHE：首次共享 cache 读取 export data 的 `bufio: buffer full` 失败保留为 failed，不把它改写为代码测试通过。

可复核证据（ignored 工作文件，不进入产品发行）：

- [benchmark JSONL](../../.tmp/data-regression/benchmark.jsonl) 与 [summary](../../.tmp/data-regression/benchmark.jsonl.summary.json)。
- [最新 benchmark JSONL](../../.tmp/data-regression/benchmark-final.jsonl) 与 [summary](../../.tmp/data-regression/benchmark-final.jsonl.summary.json)。
- [完整性检查及 reservation metrics 最新 benchmark](../../.tmp/data-regression/benchmark-complete-isolated.jsonl) 与 [summary](../../.tmp/data-regression/benchmark-complete-isolated.jsonl.summary.json)。
- [fixture oracle 交集 JSONL](../../.tmp/data-regression/fixture-oracle-intersection.jsonl) 与 [summary](../../.tmp/data-regression/fixture-oracle-intersection.jsonl.summary.json)。
- [完整性检查最新 oracle JSONL](../../.tmp/data-regression/fixture-oracle-complete-isolated.jsonl) 与 [summary](../../.tmp/data-regression/fixture-oracle-complete-isolated.jsonl.summary.json)。
- [raw 缺钥尝试失败证据](../../.tmp/data-regression/fixture-oracle-final.jsonl) 与 [failed summary](../../.tmp/data-regression/fixture-oracle-final.jsonl.summary.json)。该历史失败没有改写为 pass。
- [oracle build receipt](../../.tmp/casc-oracle-20260930-final/oracle-build.json) 与 [CascLib LICENSE](../../.tmp/casc-oracle-20260930-final/CascLib-LICENSE)。

## 保持未验证

真实安装的 FDID/locale/variant storage 差分、真实 CDN 冷下载、真实客户端矩阵、heap/RSS、性能门槛均为 `not_run` / `not_set`。没有声称具体加速倍率、峰值内存下降或 CascLib 的通用游戏支持等于产品验收。

尝试 standalone headerless N 的 `CascOpenLocalFile` 时，本 adapter 缺少可证明的 decoded size，读取失败；该形状没有进入 component corpus。不由此推断 CascLib storage handle 对该格式的支持，也不放宽 Lychee 的 headerless 路径。

固定 raw oracle 的缺钥调用发生 Windows access violation `-1073741819`，留下零字节输出文件。`CascOpenLocalFile` 无 storage key owner；该负例不属于已验证 decoder 交集。最新 runner 仍验证 Go 严格拒绝且零输出，raw 缺钥 oracle 明确为 `not_run`。完整 storage adapter 使用 storage owner，但真实 storage 缺钥路径没有运行。

## 回归覆盖复核

R1 的 `TestDecodedCacheReadErrors` 覆盖零/部分读、HTTP、取消、截止时间、EOF 和异常短读分类。R2 的 `TestContentRecoveryAfterOpen` 在启用/停用 decoded identity 的两档覆盖可恢复 HTTP/missing、全部候选失败、integrity/cancel/deadline/short 不恢复及 close failure；还验证失败对象关闭和替代内容摘要。R7 由 `TestExplicitEffectiveSQLKeepsBaseAndProvenance` 覆盖 push/capture 顺序、替换、删除、static/effective 共同查询与 provenance，基准补充 500 条删除及累计分配观测。这些测试不声称证明进程 RSS 下降。

`TestReadSessionBindsCapturedKeyProvider` 验证同 call site 的另一闭包、缺少绑定和外部会话绑定均在源 I/O 之前拒绝。`TestReadSessionCloseReleasesHeavyReferencesOnce` 验证两次关闭只释放一次句柄，同时清空 Root、Encoding、base views、Hotfix、钥匙、verified bytes 和 admission 引用。`TestReadSessionVerifiedBlobBudgetAndQueryBoundary` 验证同查询只分配/读取一次；下一查询会重新发现同大小 blob 篡改；不足 retained budget 比 corrupt blob 检查更早失败。BLTE scratch 由共同 retained cap 计算当前逻辑存活缓冲与 peak scratch，释放不等于 Go GC 或 RSS 立即下降。
