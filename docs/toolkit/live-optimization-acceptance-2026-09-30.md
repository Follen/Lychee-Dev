# Native live 优化与自动首轮验收

日期：2026-09-30。任务分支 `codex/live-optimization`，独立基线
`8e4dab0455c8bee512c73912c26b31bc9605f6c2`。非 live 的 data/source/LuaLS 工作在
[PR #2](https://github.com/Follen/Lychee-Dev/pull/2)，本分支不夹带其改动。

## 实施范围

使用三个 GPT-6.1 sol（High）子 agent，分别负责 memory、观察集成与发布端。
组合实现沿用当前 200 槽、输入/恢复、身份与完整性合同：

- L0/L4：共享实际 Read 调用、请求/返回字节、RPM 与映射查询计数，以及候选/学习计费；
  同一次 Native 调用内预算不随 Find、查找阶段或 runtime 变化重置。
  `observation.scope=invocation` 明确表示跨独立 CLI resume 不是持久物理额度；
  原业务、控制与关闭的持久 deadline 继续由 journal 决定，不更新或延长。
- L1/L2：有 hints 也使用已知 nonce 精确锚点；附带学习单独限额。
  精确地址、同 runtime/kind 邻域、完整 fallback 分开调度与最终接受；
  未知 runtime、cache off 和完整审计不变成局部存在性结论。
- L3：已验证 HEAD 只提供授权 BODY 的调度种子，不使用固定相邻偏移，
  不把 BODY 加入通用 hints。返回 BODY 前再次确认 HEAD 与进程。
- L5：仅针对已测的 hints 分配做有界复用；默认 workers/chunk 与当前映射准入不变。
- L6-A：[发布端记录](live-publication-optimization-2026-09-30.md)包含 JSON/原始 token 复用、
  完整 wire 等价及 fresh sequence 测试。1s 周期、Refresh 和 500ms 新鲜度不变。
  不引入紧凑 schema、static beacon、固定地址、FFI、注入或进程写内存。

计数和查找时间不是 WGC、JSON、输入等待或 journal 的完整 CPU profile，
逻辑计费和 standalone allocation 也不是 WoW RSS/帧时间。
原始审计中的 SIMD/更多线程、紧凑输入及完整端到端 profile 仍需实测决策。

## 自动实机首轮

用户确认的方式是：只打开客户端并登录任意角色，剩余由 agent 自动处理。
候选准备好后使用 clean managed 安装与固定目标；不会因 blocker 换角色或实例。
详细入口见[自动首轮说明](../../tests/channel-live/README.md#automatic-optimization-first-round)。

[runner](../../tests/channel-live/optimization-baseline.mjs)首先 passive 发现，唯一候选才自动选择；
保留 PID 创建时间，连接后固定 GUID/角色/服务器/build/product/release。
正常请求的 hints 与 cache off、大结果 HEAD/BODY、历史只读请求、显式 reload、
换代后正常请求、断开及重复断开均通过生产 CLI。

每个命令在调用前记录 intent，保留 raw stdout/stderr、进程 outcome 与原 CON。
候选 CLI 与探针字节带 SHA256，探针在使用前复核；原证据目录拒绝覆盖。
pending/取消/未知结果停止，不新建请求、重发不确定输入、删 owner 或自动 abandon。
业务 `report.ok`、verified、cleanup 和正确结果分别断言；metrics 缺失不伪装成测量完成。

首轮是单客户端、固定 actor 的自动功能/I/O 验收。重登、战斗、多实例、其他客户端、
物理 IME 与主观卡顿观感分别记录，不能靠 fixture 或 WGC 单帧代签全部通过。

## 当前证据

定向 build/vet、memory/channel、Lua/Go wire 对照与 runner 故障测试在组合阶段执行。
最终冻结源码的 offline baseline 于 2026-09-30 07:16:04–07:21:45 UTC 完成，
状态 **passed**。`go build ./...`、`go vet ./...`、全量 `go test -count=1`
（强制 Lua 5.1，42 packages）、98/98 Node 测试、真实 LuaLS LSP、版本一致性、
Skill 合同与生成命令参考均通过；BASE-01–BASE-21 全部通过。
可选外部数据、交互桌面及子进程 helper 的 skip 由原始报告逐项保留，不能计入实机通过。

原始证据位于任务 worktree 的 `.tmp/baseline-offline-live-final-20260930/`：
`report.json`、`summary.md` 与每项 stdout/stderr。SOURCE gate 验证运行期间源码未变，
冻结树 SHA256 为 `506f8e79f1acb233029d79c7482fd78582c688863cbd200ffa9dca512769e1f2`；
本节验收记录在测试完成后追加。定向 memory/channel race 测试另已通过。
## Retail 实机首轮

2026-09-30 用户登录 Retail 后，由 agent 自动执行受管部署、激活、请求、恢复与清理。
候选 commit 为 `1920d25c894f66b80a0232364aa11b307e048611`，CLI SHA256 为
`a04e55661aa7d390810d60511c0f5ab0d08f0fd5e20e21b2f6ff76837bc6d53c`。
目标固定为 Retail 12.1.0.69933 / Interface 120100，同一 PID 创建身份与 actor；
完整身份仅保存在本地原始证据中。本次是 agent 自动实测，不是 owner 人工验收签署。

先从 clean managed 3.0.2 / `8e4dab0` 升级候选及 200 槽，显式 reload 验证新 runtime
和 fresh bind，再关闭激活连接。生产 runner 在独立项目中绑定同一角色。
表中 GiB 是该次 CLI 的实际累计返回字节，可重复读取同一进程页，不是游戏 RSS。

| 检查 | 状态 | 观测 |
| --- | --- | --- |
| 关闭 hints 的连接 | passed | 17.609s；36.170 GiB |
| 5 条带 hints 普通请求 | passed | 20.601–76.193s，中位数 55.200s；实际读取中位数 79.063 GiB |
| 第一条关闭 hints 普通请求 | blocked | 120.142s；233.097 GiB；confirm_ready / input_observation_unavailable |
| 带 hints 大结果 | passed | 75.983s；112.964 GiB；完整 Unicode/换行文本逐字核对 |
| 关闭 hints 大结果 | blocked | 120.146s；224.529 GiB；相同输入样本 blocker |
| 历史请求只读重试 | passed | 原 operation/report 相同，journal 字节不变 |
| 显式 reload | passed | 44.015s；验证 runtime 更换与 fresh bind |
| reload 后普通请求 | passed | 33.698s；42.999 GiB；结果及 cleanup 验证 |
| 首次断开 | blocked | 30.095s；输入样本等待，原 close 预算后来耗尽 |
| 关闭恢复与重复断开 | passed | 显式 reload 以 closing 目的证明旧 runtime 结束并释放原 CON；重复断开不改 journal |
| 原版恢复 | passed | 受管安装及激活恢复为 `8e4dab0` / 3.0.2，恢复连接关闭；200 槽 managed，pending=0 |

两条关闭 hints 的请求停止后，使用原 CON / operation 的生产 `live resume` 恢复，
不重新执行探针。恢复使用默认 hints 路径，取得 `reportState=verified`、`report.ok=true`
与 `cleanup=complete`，不把它算作关闭 hints 首测通过。
原 `optimization.json` 保持 **blocked / complete=false**，后续大结果与关闭的失败记录
也原样保留；恢复和清理证据单独记录。其余 4 条关闭 hints 普通请求未执行。

### 耗时发现与边界

- 首条普通请求包含多次 unknown-runtime 身份发现，累计约 47.8 GB / 23.4s，
  不是仅有启动扫描。该完整发现与 runtime 恢复调度沿用原版语义。
- nearby 存在完整 envelope 后 predicate 拒绝的记录，证明样本不符合当次条件，
  不证明地址移动。500ms 新鲜度、recent hint 与输入前重验规则未放宽。
- 首次发现耗尽本次调用的 64 MiB 附带学习额度，learningCandidates=0。
  `learned=0` 只表示附带学习，已接受记录仍可更新 hints；不表示 hints 为空。
- channel 的显式 nearby 尝试之后，memory 层仍可能再尝试一次 nearby。
  该额外工作有界，但当前 priority 计数不能独立归因其耗时。

本轮证明了成本高和两个关闭 hints blocker，尚不能把它归因于某一新改动。
没有同条件原版/候选对照，不声明提速或退化，也不将计数解释为 CPU、RSS 或 p95。
进一步定位需区分 Accept / finish / 最终 Input 时的样本时间和拒绝原因，
以及外层 nearby、hint 点读、内层 nearby 与 full scan 的分别成本。

### 实机证据

任务 worktree 的以下本地目录保留 raw stdout/stderr、命令 intent/outcome、
原 `.lycheedev/live` journal、结果、trace 与恢复数据；没有上传角色身份或原始游戏内容：

- `.tmp/retail-activation-1920d25-20260930/`：升级、激活与激活连接关闭。
- `.tmp/retail-live-1920d25-20260930/`：`optimization.json`、`functional-continuation.json`、
  `finalization.json`、两个原请求的恢复、`close-reload-recovery-summary.json`、`summary.json`。
- `.tmp/retail-restoration-1920d25-20260930/`：原版受管恢复、激活、关闭与最终槽位检查。

`optimization.json` SHA256 为
`ec26c82ded211f12d2ba2c082dda90f9da0ed2c50f18901125933af65bcdc033`。
汇总另外验证固定进程/actor，以及已记录各阶段计数之和等于 invocation total。
重登、战斗、多实例、其他客户端、物理 IME、主观观感和全矩阵仍为 **not_run**。
本轮未修改发行版本/渠道，未合并或发布，PR 继续保持 draft。
