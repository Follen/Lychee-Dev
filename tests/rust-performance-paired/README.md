# 当前成对性能比较

正式入口为 `node tools/rust-performance-paired.mjs <Rust artifact.json> <外部 Go 参考目录> <machine-conditions.json>`。从仓库根目录运行；使用已经构建的 Windows optimized Rust 产物，不在计时过程中编译或替换候选程序。当前结果与待办分别见 [handover.md](../../handover.md) 和 [todo.md](../../todo.md)。

## 覆盖与判定

- 现有入口只执行 B01 `describe`、B02 固定 Retail `source index` 和精确 `source query`。每个 case 各有冷、热两组，每组 20 对，共 120 对、240 次被计时调用。Go/Rust 顺序逐对交替。
- 所有样本必须完成原有业务结果对照、退出和进程回收；热组还须证明本引擎自己的派生缓存得到复用。Source 组继续要求 Rust 中位耗时 / Go 中位耗时 ≤0.50。按用户要求，describe 只核正确性并记录原始耗时和比值，`targetRatio=null`、`performanceAcceptanceEvaluated=false`，成功状态为 `passed_correctness_only`。Live 同样不设耗时比值门槛，其真实业务验证范围不变。
- `failed_performance` 表示 Source 组结果完整且正确，但速度未达标；`failed_correctness_or_missing_samples` 表示正确性或样本不完整，对 describe 仍适用。describe 两组正确、Source 四组性能达标也仅为 `passed_selected_paths_full_acceptance_open`。
- 报告始终明确 `fullB01B07Acceptance=false`。本入口不证明其他 CLI、Source、Data 或 Live 路径的功能和性能；不能用准备成功、单次诊断或旧报告替代验收。

## 固定输入

- Rust 输入由 [rust-build.mjs](../../tools/rust-build.mjs) 生成并验证，profile 为 `release` 或 `symbols`；产物、源码和 runner 输入在执行前后保持一致。当前构建批次上限为 900 秒，单 Cargo job；磁盘预算以 [rust-disk-policy.json](../../release/rust-disk-policy.json) 为准。
- Go 输入是仓库外显式提供的固定 3.2.4 参考目录，包含 `artifact.json`、`dev-npm-stage` 和对应 tgz。校验规则在 [inputs.mjs](inputs.mjs)。当前仓库不构建 Go 产品，也不重新使用已删除的 Go 准备工具。
- Retail 输入固定为 `wow-ui-source` 提交 `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b`，共 4,049 个 Git blob、4,036 个源码文档和 32,269,051 文档字节；路径、大小、哈希和业务锚点见 [retail-oracle.json](../rust-source-full-corpus/retail-oracle.json)。
- Go/Rust 分别初始化、同步并认证自己的 seed，随后每对从各自 seed 创建独立 home，不跨引擎共享派生缓存或元数据。源码准备记录在 preparation 中。
- 机器条件 JSON 必须给出非空 `machine`、`powerAndThermal`、`competingLoad`、`disk`、`operator` 字符串。未知值如实说明；运行前后记录同一电源策略，计时期间不并行构建或执行其他原生验证。

## 冷热与计时

“冷”仅指应用派生缓存为空，不表示已清空 Windows 文件缓存。“热”使用该引擎已成功完成冷调用的相同 home、PIN、二进制与命令。describe 不产生派生缓存，其冷/热标签只表示首次与重复调用。

可控耗时使用 owned runner 的原始高分辨率时钟：从 `CreateProcessW` 前到根进程及整个 Job 回收、输出排空结束。该口径包含计时区间内的进程创建、身份落盘、观测准备、程序执行和清理；排除 PowerShell wrapper 自身启动与观测器关闭。不按观测后的分段减去启动成本。未执行下载或外部等待的被测调用，其相应排除值为 0。

公开结果的差异仅按 [contract.mjs](contract.mjs) 中显式列出的当前命令目录/字段/摘要变化处理，其余既定字段、顺序、标识和结果严格比较。具体准备、执行和缓存复用规则见 [run.mjs](run.mjs) 与 [inputs.mjs](inputs.mjs)。

## 输出与诊断

每次执行在 `.tmp/rust-disk/evidence/<id>/` 输出 `report.json`、`owned-report.json` 和 receipt。报告区分样本正确性、速度、候选/runner 稳定性与进程回收；命令退出 0 只表示该入口定义的成功，不能扩大为完整 B01—B07 通过。失败结果保留原始日志，已关闭的临时 home 按任务归属清理。

同一入口可追加一个互斥的诊断参数：

- `--startup-diagnostics`：仅运行一对 describe，记录进程创建、身份落盘、观测准备、恢复主线程以及 root CPU。状态为 `startup_diagnostics_observed`；不评价性能达标。CPU 只来自原始根进程句柄，不代表整个 Job。
- `--cold-index-diagnostics`：仅运行一对固定 Retail 冷索引，状态为 `cold_index_diagnostics_observed`；保留原始计时和正确性规则，不替代 20 对验收。
- `--source-diagnostics`：只运行固定完整 Retail 的 index、精确 query，各一对冷调用和一对热调用，共四对、八次被计时调用，不执行 describe。沿用各引擎独立 seed/home、同 home 冷热复用证明、完整语义对照和 owned 清理。仅 Rust 被计时调用启用现有 `LYCHEEDEV_SOURCE_PHASE_PROFILE=1`，分段输出保留在原始 stderr 日志，可定位 warm 的读取、哈希和解码成本；不从总耗时扣除这些分段。四对全部正确、完整且回收成功才为 `source_diagnostics_observed`，缺样本或失败为 `source_diagnostics_failed`。`groups=[]`、`performanceAcceptanceEvaluated=false`，不计算验收比值，也不替代正式20对模式。

诊断不能改变目标、计时口径、输入或资源上限来使结果通过。重跑须由新的改动、实际失败或明确未解决的问题触发。
