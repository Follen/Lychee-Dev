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
自动首轮 runner 当前只有故障/策略测试，不代表已经连接、安装或操作真实游戏。

所有本轮真实客户端、部署激活与端到端收益均为 **not_run**。
本轮未修改发行版本/渠道，未合并或发布。
