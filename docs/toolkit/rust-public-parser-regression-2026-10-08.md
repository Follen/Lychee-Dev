> Historical reference for the Rust migration (2026-10-14): current decisions, baseline and acceptance status are in [handover.md](../../handover.md). This document preserves its original scope and evidence; conflicting requirements, protocol designs, performance targets and release instructions are superseded. Historical test results are not current-candidate passes.

# 公开命令解析差分回归（2026-10-08）

这是完整 89 条命令清单上的**解析拒绝场景**记录，不是完整业务验收。原始失败保留；源代码修复和公开产物复验分别记账。

## 方法与受测身份

独立入口：`tests/rust-public-contract/parser-differential.mjs`。从冻结的 Go `describe --format json` 取得全部 89 条合同，并与导入的 `crates/lycheedev-cli/src/commands.json` 深比较。每条命令运行未知参数、`--format` 缺值、非法 format 三种输入，共 267 对调用。输入都必须在解析阶段以退出码 2 拒绝，不发送有效业务请求或游戏输入。

比较完整 JSON envelope，或完整文本 stdout，以及 stderr 和退出码。仅忽略 JSON 对象成员顺序和文本 CRLF/LF 差别，不忽略错误文案、context、warnings 或 error 字段。独立 home 与受控 Windows Job 隔离；每个调用 10 秒、2 MiB 输出上限，整体 600 秒。原始 stdout、stderr、调用参数、退出码和哈希全部保留。

首次运行：`.tmp/rust-parser-public-symbols02-01/report.json`。

| 输入 | SHA-256 |
| --- | --- |
| Go v3.1.2 `.tmp/lycheedev-oracle.exe` | `5738418756ee57c536518ff71223f2e7843ac3495a846b4f063d3104d83aae87` |
| Rust `.tmp/rust-cli-timeout-symbols-02/lycheedev.exe` | `d15b8c886b34b2a087193c0298f64ac3f21660dcc702969619254145ec6b1cf4` |
| 测试脚本 | `b4537baad748d315bb7156424b5f09d2310c76d1e8e350ba4c466adaba2885ea` |

两个二进制运行前后哈希一致。Rust 编译来源及匹配 PDB 另见同目录 `artifact.json`。

## 首次实际结果与修复状态

267 对全部完成：89 对一致，178 对不一致，整体 exit 1。所有探针均退出 2，没有误入成功业务。两类差异分别覆盖全部 89 条路径：

| 场景 | Go 合同文案 | 首次 Rust 文案 | 状态 |
| --- | --- | --- | --- |
| 未知参数 | `unknown flag --__rust_parity_unknown_flag` | 一致 | 此产物通过 |
| format 缺值 | `missing value for --format` | `--format requires a value` | 源代码已修复；新版公开 EXE 待复验 |
| 非法 format | `format must be text, json, or jsonl` | `--format requires text\|json\|jsonl` | 源代码已修复；新版公开 EXE 待复验 |

修复后 CLI 包 15 个测试通过，受控 receipt 为 `.tmp/rust-disk/evidence/9eeb0c04-1692-459c-8b1b-840ac208d44c/receipt.json`。这不能覆盖首次产物的失败，也不能替代新版 EXE 上的 267 对复验。

首次运行的 `.tmp/rust-parser-public-symbols02-01/owned-process.json` 记录 root/tree reaped、handlesClosed、outputReadersComplete 为 true，activeProcesses=0；forced=false、deadlineHit=false。exit 1 来自差分不一致，而非清理超时。

## 此产物的公开目录边界

上述 Rust EXE 实际公布 66/89 条命令，已公布条目的参数合同与 Go 无差异；还有 23 条未公布。原始报告逐项列出缺失路径。未公布不等于没有内部 handler；例如 spell auras/summons 已有独立分项行为证据，但在此 EXE 的公开目录仍缺失。

解析拒绝可以先于业务分派发生。因此未公布命令的解析回归通过，不能用来证明其业务已经实现。此报告不覆盖正常业务、必要参数组合、JSONL、分页、截断、provider、取消、超时、中断恢复、性能或最终 Native 协议。

历史 `rust-command-coverage-2026-10-08.json` 绑定其原产物，不以本报告的 66/89 数量覆盖历史身份或扩大历史验证范围。下一次封存产物包含解析修复后，再执行本脚本并追加真实结果。

## 新封存 EXE 的实际复验

Root 已用 `.tmp/rust-source-rawqueue-symbols-02/lycheedev.exe` 重跑同一入口，完整 267/267 对一致，差异 0，整体 exit 0。证据为 `.tmp/rust-parser-public-rawqueue02-01/report.json`。此前两类解析文案差异在这个产物上关闭；首次 178 对失败保持原样。

新 EXE SHA-256：`12e20bb40589ff975bd129f2f569e624f898c49a02e3dcd7c3695cb46d064812`；同次编译 PDB：`1ba0b25be3a784bea298625ce684d31a8367274502eec183ef787fb8fb2e5552`。编译源码摘要为 `18ec3a809aa1ad928e3d4499282080b7201a524260754eb4cee1eadc221bdde1`，receipt 为 `.tmp/rust-disk/evidence/22619f37-88d4-4210-a82b-02e03f69e5a1/receipt.json`。运行前后二进制稳定。

公开目录仍为 66/89，已公布合同无差异；未实现或未公布业务没有因本次解析通过而扩大验收范围。完整 Retail 稳定性、正常业务、JSONL、取消、恢复和性能仍分别验证。

构建前一次同参数运行因用户中断失去统一执行会话，且无终态回执。Root 核验 owner/wrapper/编译进程已不存在，保全原 receipt、原始日志和精确 lease，独立记录 `.tmp/rust-supervision/source-build-interruption-01/report.json`。其 buildOutcome=unknown、原进程回收证明 unavailable，未采用旧输出、未删除 scratch 或缓存。此记录不能被后续成功反向改写为通过。

## 全部带值参数的错误优先级

Root 新增独立入口 `tests/rust-public-contract/valued-flag-differential.mjs`，从同一冻结 EXE 的 89 条合同取得除 format 外全部 452 个带值参数出现位置。每个位置检查“缺值后接未知参数”和“空 inline 值后接未知参数”，共 904 对。始终保留未知参数，使错误解析不会落入有效业务分派；没有游戏输入。比较完整输出、stderr 和退出码，只忽略 JSON 对象顺序及文本换行差别。

实际运行 `.tmp/rust-valued-flags-public-rawqueue02-01/report.json`：893 对一致，11 对不同，整体 exit 1。所有 Go/Rust 探针都退出 2；二进制前后稳定；受控 Job 完整回收、activeProcesses=0、forced=false、无 deadline hit。仍使用上述 `12e20...` Rust EXE 和冻结 Go，并保留参数清单、每次 stdout/stderr、逐例 JSONL 与完整最终报告。测试脚本 SHA-256：`9050ddc56f75b54d7de6d07c2f292ca8b11ebcfdc099539c3ac92e5a8bc14981`。

11 对差异都在 Data 数值 ID 参数的空 inline 值后接未知参数：Go 先报未知参数，Rust 在逐 token 的数值校验中先报 invalid ID。涉及 spell info/auras/summons、item get/models/geosets/textures、creature display、encounter get、decor get；summons 的 npc-id 另占一例。退出码和空 context 相同，但错误文案优先级不同，不能从此前 267 对通过推导这个场景也通过。

Foundation 已将 ID 校验移到完整 tokenization、route flag 和 format 校验之后，仍保留在 parse 阶段；新增回归覆盖未知参数优先和仅坏 ID 仍拒绝。目前只完成源码修复，尚未在新封存 EXE 上复验这 904 对。首次失败不覆盖；正常 ID 业务、非法 ID 的原公开 profiles 及必要旧测试继续保留。

## 同一新产物上的 1171 对实际关闭

Root 已在 `.tmp/rust-source-projection-symbols-02/lycheedev.exe` 上依次完成两套对照，均为终态 exit 0：

| 入口与证据 | 实际结果 |
| --- | --- |
| `.tmp/rust-parser-public-projection02-01/report.json` | 267 对全部一致；公布 66/89 条，公布合同无差异 |
| `.tmp/rust-valued-flags-public-projection02-01/report.json` | 452 个参数位置、904 对全部一致；原 11 处数值 ID 错误优先级差异关闭 |

受测 EXE SHA-256 为 `b748a02beaff2b8c42d79ae8e0282f0aa070e46e3477f2e1f5ab919ec240e370`，同次编译 PDB 为 `3156dc3644343e89ff827908387cf5d9d34947bd70aec548c213392def1e98a9`；源码摘要为 `411d8092f3ccd9e894d6d8e029ec03907730781cbd19d597fb78ff890863c907`。编译 receipt 为 `.tmp/rust-disk/evidence/7a1c53bf-9080-4243-8530-3e63772b813c/receipt.json`。Cargo exit 0，所有权终态已回收；`forced=true` 仅为已审核辅助进程清理，`knownAuxiliaryCleanup=true`、activeProcesses=0、root/tree/handles 均回收，不能记为无强制清理。

此前 CLI 数值参数优先级修复已在同一源码摘要上通过 16 项实际测试，receipt 为 `.tmp/rust-disk/evidence/1eb2969f-7785-49d1-ad6b-3629d21ef8fa/receipt.json`。两套公开对照的 Go/Rust 二进制前后稳定；各自 owned-process 记录 exit 0、完整回收、activeProcesses=0、forced=false。

首次 178 对文案失败和后续 11 对优先级失败仍保留。1171 对只关闭这两套拒绝型参数场景，不证明剩余 23 条业务、有效参数组合或完整候选通过；Source 完整负载与 Native 协议继续独立验收。
