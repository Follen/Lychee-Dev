# Data 准备链基准与 CascLib 交集差分

仅用于 Windows amd64 自动回归，测试工具不属于产品 CLI 或发行 payload。默认 Go tests 会编译基准，并将未配置的外部 oracle 标记为 skip / `not_run`；单独的 runner 不把 skip 当作 verified。

## 准备链基准

```powershell
node tests/data-regression/run.mjs benchmark --output .tmp/data-regression/benchmark.jsonl --benchtime 3x --count 1
```

二十个场景使用相同的合成、完整身份校验的 CASC 对象，分别从本地 archive/index 和预热的离线 CDN fragments 读取。每种来源覆盖单表、双表 JOIN、同表 static+effective、500 条 Hotfix 删除与技能导航；每类都有 cold decoded 和 warm decoded 两档。执行真实 Encoding、Root、BLTE、SQLite/blob、定义、WDC、SQL/导航及 evidence 链；没有用生成的 relational Source 替代准备链。

Cold decoded 只清理 decoded cache 的 metadata 映射，保留原始 CASC fragments、不可变 blobs 和 OS 文件缓存；清理在计时器之外。Warm decoded 在计时器之外先完成一次同样查询。两档都包含运行中的证据提交。它们不是冷机器、冷网络或真实安装基准。

标准 `ns/op`、`B/op`、`allocs/op` 分别是计时、累计分配字节与分配次数，均不是峰值 RSS。SQL 另报告 definitions/prepare/execute 时间、逻辑绑定 source bytes、网络正文/request 数、逻辑 work、decoded hits/reused bytes、content hash/decode 尝试、提取字节、verified blob 读取与 fragment/block 次数。content hash 不包含所有 EKey/page hash；blob 读取不包含所有 definition/metadata 磁盘 I/O。static+effective 共用同一 FDID 的统计去重，逻辑 source bytes 仍按绑定表计数。导航没有公开的 phase/hash/blob 统计，所以只报告标准基准指标与共同 budget 的网络/work；未测量指标不伪报零。

`charged-retained-B/op` 和 `charged-metadata-B/op` 是查询预算的保守预留，和 Go 累计分配 `B/op` 不同，也不是进程内存测量。Runner 要求完整二十个 physical/scenario/state 组合各有指定次数及必需 metrics，并有 Go package pass；只 exit 0、重命名/未执行基准或缺 metrics 都会失败。

## 固定 oracle 构建与 fixture 差分

需要已安装的 `gcc`/`g++`（本次使用现有 UCRT64 编译器）；不会自动安装工具链或下载源代码。

```powershell
node tests/data-regression/build-oracle.mjs <CascLib-checkout> <new-build-directory>
node tests/data-regression/run.mjs fixture-oracle --oracle <new-build-directory>/casc-oracle.exe --output .tmp/data-regression/fixture-oracle.jsonl
```

构建拒绝非 `38a34665624b8775bb875274b36191b21c38d97b` 的 commit 或 dirty checkout，按该 commit 的 CMake source list 编译，使用其 bundled zlib，不依赖系统 zlib。输出目录保留 CascLib LICENSE 和 executable SHA256 receipt；测试复核 commit 及 executable digest。

Fixture corpus 是仓库已有 provenance 固定的 264 字节 `JournalEncounterSection-classic-era.db2`，分别封装为有 chunk 目录的 N 与 Z BLTE。Go decoder 与 `CascOpenLocalFile` 严格读取后都必须匹配原始 DB2 的 SHA256。它验证 decoder 交集；没有验证 Root、FDID、locale、安装或真实客户端。Standalone headerless BLTE 的 local-file oracle 缺少已知 decoded size，本轮观察其失败，未纳入这一交集；不会用它推断 CascLib 的 storage handle 支持范围。

损坏 chunk checksum 的负例要求两端严格拒绝。缺钥负例独立验证 Go 返回 `ErrKeyUnavailable` 且不写替代字节；raw oracle 缺钥验证保持 `not_run`。本轮直接调用 pinned `CascOpenLocalFile` 的缺钥路径观察到 Windows access violation（`-1073741819`），该 raw API 没有 storage key owner，因此没有将它纳入共同 decoder 交集，也没有修改上游实现或放宽 Go 政策。完整 storage adapter 有 storage owner，但它的实际缺钥行为未验收。

Fixture runner 必须找到 parent、N、Z、checksum 与 Go 缺钥子测试的 pass 以及 package pass；任何 skip 保持 `not_run`，没有预期 pass 不能用 exit 0 推断成功。Storage runner 还要求本次独占生成的完整 verified comparison artifact。JSONL、summary 和 comparison 输出不覆盖已有文件。证据政策测试由 `node --test tools/data-regression.test.mjs` 执行。

## 完整 storage 交集

```powershell
node tests/data-regression/run.mjs storage-oracle --oracle <build>/casc-oracle.exe --manifest <fixed-case.json> --output .tmp/data-regression/storage-oracle.log
```

Manifest 必须明确给出 `schema=lycheedev.test.casc-case.v1`、`installation`、完整 `pin`（product/region/build/config/definition commit/language）、`fileDataID`、`localeMask`、`contentVariant=standard|low-violence`、`completeKeys=true`、预先固定的原始 `expectedSHA256` 和 `maxBytes`。需要钥匙时给 `keyFile`（两端共用的 text key 格式）与 `keySHA256`；钥匙文件不会写入结果。未加密的交集可省略 key file，Go 仍在最终输出上要求完整 CKey 校验。

Harness 先运行 Lychee 的严格 local/offline 选择：歧义、缺 locale、缺钥或 partial 直接失败，不让 oracle 替代产品政策。然后 CascLib 在 offline storage、固定 build key/product/build number/locale 下以 FDID 打开，核对实际 variant/locale/FDID/CKey。CascLib 使用 `CASC_STRICT_DATA_CHECK`，从未设置 `CASC_OVERCOME_ENCRYPTED`；缺钥不能用零字节成功。只有两端与预先固定的 SHA256/长度一致才写 `verified`。默认 variant 或 locale fallback、另一 CKey、无法证明 build 的 storage 都不能算交集通过。

此 harness 有 120 秒总截止时间、最多 512 MiB 输出、拒绝覆盖，临时工作区结束后清理。它只读取存储，不发送游戏输入；真实安装的数据与钥匙由明确的 case manifest 指定。本轮未运行真实 storage FDID/locale 差分，该项保持 `not_run`。CascLib 与本项目的客户端接受矩阵仍独立。
