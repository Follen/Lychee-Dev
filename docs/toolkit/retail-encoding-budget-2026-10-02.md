# Retail Encoding 查询预算修正

日期：2026-10-02。基于已发布 3.1.0 的源码提交 `9edc06b4591264eaac68e34575bc02f750ffe758`。这是开发修复，不修改已发布的包、版本或默认预算。

## 直接证据与复现

用户提供的 Linux 源码构建报告指出：`fileSource.encodingIndex` 在打开 Encoding 之前，将配置声明的完整逻辑解码长度计入 metadata/retained。此 build 声明 199,496,988 字节，单项已超过查询 metadata 的 134,217,728 字节上限。源码核对确认该计费发生在 `s.open` 前，和实际按目录/页面读取的结构不符。它不是已读取字节或实测 RSS，也不涉及 LuaLS。

Windows amd64 使用正式安装的 3.1.0、同一提交，另建隔离调查 home，通过 CLI 导入报告中已解析的 selection，返回完全相同的 pin：

| 身份 | 固定值 |
| --- | --- |
| Product / build | retail / 12.1.0.69933 |
| Region / locale | us / enUS |
| BuildConfig | `dcfc90fffd79ba00406ae46f5f657592` |
| CDNConfig | `43061ca8e9f0e2603c8ab50bcae97c2d` |
| WoWDBDefs | `005c13a9a101e64014eeb02af3a42ccbeaf8513d` |
| Pin | `PIN-92355a9281bea40c39603f614c156fb38a6d7b551a4c5cae9d6731547c28fe1f` |

在线通过正常 CLI 准备定义和配置后，以下正式版本命令真实返回 exit 3、`records.query_resource_budget: metadata_bytes`，stderr 为空：

```text
lycheedev data db2 --home <isolated-home> --snapshot PIN-92355a9281bea40c39603f614c156fb38a6d7b551a4c5cae9d6731547c28fe1f --cdn --table ChrClasses --limit 2 --offline --timeout-seconds 180 --format json
```

当前远程 resolver 对同一 build 返回了不同 CDNConfig `7bfef39ed156aa00019d651013213326`，得到另一个 pin。该观察单独保留；没有用它替代报告里的 pin，也没有手工改缓存或从 latest 取代固定 build。

## 实现

- 公开 `OpenEncoding` 保持无预算、无缓存解析行为。查询的内部入口接收原查询 budget，不建立或重置独立额度。
- 在 header 和所有范围验证后，CKey 目录读取前预留 `pageCount × 32` 字节及 512 字节保守 index/cache 结构估计。
- EKey 目录在缓存锁内首次实际读取前预留目录字节；成功后复用，失败后重试继续累计。
- 每次页缓存 miss 在读取前预留页长度及 128 字节保守 map 开销。成功命中不重读/重扣。原 8 MiB 页面缓存保持，驱逐、失败与取消不退款。
- 累计费用覆盖查询生命周期内分配的页，即使驱逐后仍有其他调用者在锁外解析该页；不能把当前缓存容量当作所有存活切片的上限。
- 每文件尺寸上限、BLTE 范围和摘要校验、scratch、网络与 decode/work 预算，以及 Root 的真实保留字节/索引计费保持。

默认 metadata 128 MiB、retained 512 MiB 不变。该核算是保守分配估计，不能解释成 RSS 测量或内存下降倍率。

## 验证范围

真实 `fileSource.encodingIndex` seam 的合成 fixture：逻辑长度 202,381,366 字节、压缩对象 409,204 字节。每个页面有独立校验，未伪装成 Retail 采样。旧版在打开对象前拒绝；修复后可以读取目标 CKey/EKey，重复命中不增加读取/计费，不读无关页面。

新增回归还覆盖 CKey/EKey 目录和页分配前预算拒绝、metadata/retained 原子失败、32 个并发首次 lookup 与顺序路径一致、同预算第二 source 累计、9 个 1 MiB 页面触发驱逐后的重读累计、短读/取消/完整性错误的失败重试，以及真正取消上下文的 warm lookup 不读取/不扣费。

已完成 `go build ./...`、`go vet ./...`、records/container/resource 包测试、相关 race 检查及新增取消/完整性测试。Windows 全量离线 baseline **passed**：43 Go packages、3,034 个命名结果（40 个环境/helper 显式 skip、零失败）、强制 Lua 5.1、真实 LuaLS、100 项 Node 测试、版本/Skill/生成命令门禁，以及运行期间 source 未变化。报告为 `.tmp/retail-budget/baseline/report.json`。全量后仅补充本文和实施状态中的实际结果，生产代码和测试未变化。

开发候选在原 home、原 pin 的首次离线复测推进到 `records.remote_unavailable_offline`，因为旧版从未准备 Encoding 范围缓存。在线真实准备的首次 180 秒运行下载取得进展后返回 `command.deadline`；沿同一 home/pin 只延长一次到 600 秒，在 02:05:28 UTC **exit 0**。根目录 67,167,107 字节和 ChrClasses DB2 6,452 字节完成校验，返回 ID 1/2、Warrior/Paladin，layout `AFC9B0C2`。

随后完全按报告的原命令（`--offline --timeout-seconds 180 --limit 2`）在同一 home/pin 重读，**exit 0、contentVerified=true**，与在线结果的两行及内容摘要一致；decoded cache 命中 Root/DB2，未重新解码。`complete=false`、`truncated=true` 是请求两行分页的合法结果，不能表述成完整表扫描。正式 3.1.0 对现在已充分准备的同一缓存仍返回原来的 metadata 预算错误，排除了仅因补齐缓存而成功。

这证明该固定 Retail 的 Encoding → Root → DB2 查询路径已恢复，不代表所有表、产品或其他 build 已验收；未测 RSS/heap 或加速倍率。首次冷准备超时仍保留，不宣称冷准备在 180 秒内通过。

原始 stdout/stderr、开始/结束时间、退出码和版本身份位于任务 worktree 的 `.tmp/retail-budget/evidence/`；不将 raw 调研日志加入发布源代码。正式全局安装的 3.1.0 和游戏 addon 未替换，未执行任何游戏输入。
