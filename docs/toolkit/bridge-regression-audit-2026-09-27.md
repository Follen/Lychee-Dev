# 桥接整合前后回归审计 — 2026-09-27

审计对象：整合前 `41af9cb616dcb7a9e604619a4fc0ad6c8d6d8210`（1.2.0 后最后提交）
与当前 `141ae172e5cd5a817486622d6f524fba9e0e0bb7`（已发布 2.0.5）。
主审计负责升级；三个独立子审计分别检查生命周期、光学传输、Skill/旧业务。
只读取旧源码，没有执行退役入口、读取旧用户数据或向游戏输入。

**结论：有真实退化，主要集中于异常恢复与程序级收尾保证。**
不是“Go 代替 Python 必然不稳定”，也不能归因于合并后的全部业务。
新身份/证据/窗口独占防护解决了旧版缺少的约束，但若干阶段只实现了拒绝路径，
没有完整设计用户如何恢复或退出。正常链路测试通过不能证明这些出口存在。

初次审计没有修改生产实现。以下缺陷存在于 2.0.5；已修复的 abandon schema
证据丢失不重复计数。合成或模拟复现不等于新一轮真实游戏验收。

## 后续修复：2.0.6 候选

用户已授权修复、发布与安装。以下为审计后的实现状态，原始发现保留用于对照。

| 项目 | 修复与永久回归 |
| --- | --- |
| B01 | 受管 from/to reload、目标 commit/字节复核、旧 session 升级前后重连、公开同 request 零重复刷新；`reload_upgrade_test.go` |
| B02/B03 | 原子 probe/bugs 同报告幂等 ACK；零/部分/未知输入明确 abandon；真实生命周期和 `abandon_recovery_test.go`，不重派探针 |
| B04 | 暂停与显式隐藏分离，交错 combat/loading/chat、actor 变化与旧 callback；`receipt_recovery.lua` |
| B05 | 不放大二维码，增加有界首覆盖像素采样；实际 Lua encoder 7 个物理缩放值均 decode=1；`fractional_test.go` |
| B06 | 默认左上最多 1024×1024、保留 explicit window、保存实际区域；`capture_area_test.go`、`capture_region_test.go` |
| B07/D2 | reset/hide 未观察到保留未知；恢复性能方法及 skill 路由；skill 校验与命令合同通过 |
| D1 | live finish 执行 ACK→清屏确认，持久化 capture；真实生命周期 pending→cleared→第三次零输入 |

ACK 缓存只保证同运行态/session/actor/generation，不声称跨任意 reload 存活。
无法确认时 pending 与显式 abandon 为有定义的出口。发布前未完成本候选新的
真实游戏测试；离线回归不会升级成各客户端真机验收声明。

## 优先级与证据

| ID | 优先级 | 问题 | 证据强度 |
| --- | --- | --- | --- |
| B01 | P1 | 版本升级无法通过当前 reload 流程闭合 | 实际版本拒绝 + 同版本/跨版本成对 Go 测试 |
| B02 | P1 | ACK 回执丢失后，verified 任务持续占用窗口且没有显式退出路径 | 真实生命周期 fixture 的故障注入 RED |
| B03 | P1 | 零输入/部分派发留下无法恢复或放弃的 owner | 实际派发入口 + 持久状态/所有权断言 RED |
| B04 | P1 | 战斗开始隐藏报告后，脱战不恢复 | 生产 Lua 模块的事件复现 RED |
| B05 | P2 | 小二维码在部分低 effective scale 下解码失败 | 实际 Lua 编码器 → 合成栅格 → 生产解码器 |
| B06 | P2 | 默认全窗捕获拒绝 5K/超宽画面，旧 ROI 策略可读 | 同一合成画面的全窗/ROI 对照 |
| B07 | P2 | reset 指引把“未观察到”错误解释成“从未显示” | 文档与发送/捕获顺序直接矛盾 |

另外两项迁移损失分别是 ACK→hide 的机械保证被拆到 Agent，以及性能调查参考被删除。
前者符合新原子命令合同，但解释了收口体验下降；后者只证明指导丢失，不证明某次运行误判。

## B01：升级时，旧运行态和新 CLI 互相卡住

当前 [reconnect_action.go:21](../../internal/live/reconnect_action.go#L21) 要求保存的
session release 等于 CLI 的 `buildinfo.Version`。新 CLI 的身份探测也固定这个
release（[identity_probe.go:151](../../internal/live/identity_probe.go#L151)）。
`live reload` 首先必须成功 reconnect
（[standalone_reload.go:66](../../internal/live/standalone_reload.go#L66)）。

即使保留旧 CLI 来发起升级重载，reload intent 又把旧运行态 release 固定为
重载后预期（同文件 :103）；新版 addon 返回的新 release 被 `Signal.Match`
过滤（[signal.go:366](../../internal/bridge/signal.go#L366)）。普通观察路径只会继续等待，
不会像身份探测一样返回明确的 release mismatch。

触发流程：更新 CLI/addon 磁盘文件 → 游戏仍运行旧版本 → 新 CLI 无法连接/重载；
若旧 CLI 重载后进入新版，原 reload 操作也无法完成预期验证。
之前实际正式服已读到 `observed=2.0.3 expected=2.0.4`，并未绕过校验。

本轮复制 `TestStandaloneReloadIsCorrelatedAndIdempotent`，只把重载后回执 release
改成 `2.0.6`：同版本控制组 PASS；跨版本组 `TestAuditReloadAcrossAddonRelease`
返回 EOF（模拟帧耗尽；真实采集对应有界等待）。其他 nonce、身份、epoch 保持不变。

旧版 [reload.py:22](https://github.com/Follen/Lychee-Dev/blob/41af9cb616dcb7a9e604619a4fc0ad6c8d6d8210/Lychee%20Dev%20skill/scripts/automation/reload.py#L22)
按协议 v1、kind、nonce、ready、client/build 确认，没有把 npm 包版本绑成重载后的身份。
旧版并非拥有完整受管升级事务，但不存在这道包版本耦合。

建议：保留普通业务的版本/协议校验，增加明确的升级握手。
记录受管 manifest 的 from/to release，限定同窗口/角色、同 nonce 的运行态切换，
然后创建新的正常 session。不要以全局放宽 release 校验代替升级设计。

## B02：ACK 已执行，但回执丢失后没有出口

当前 [execute_probe.go:80](../../internal/live/execute_probe.go#L80) 对 `ack_requested`
只观察，不再次提交；[abandon.go:49](../../internal/live/abandon.go#L49) 不接受该阶段，
且 :63 拒绝 ACK 输入痕迹；[cancel.go:34](../../internal/live/cancel.go#L34) 仅允许
prepared；[reset_window.go:95](../../internal/live/reset_window.go#L95) 拒绝有 owner 的窗口。

客户端 ACK 后已删除报告，缓存的 lastAcknowledgement 仅存在于当前运行态，
不跨 reload/login（[ReportStore.lua:164](../../addon/Bridge/ReportStore.lua#L164)）。
因此不能假定丢失的回执之后一定会重新出现。

复现复用完整生命周期 fixture，只移除 ACK 的接收帧：

```text
stage=ack_requested report=verified
resume=EOF cancel=journal.invalid_transition abandon=journal.invalid_transition
occupied=true
```

旧版 [Controller.lua:729](https://github.com/Follen/Lychee-Dev/blob/41af9cb616dcb7a9e604619a4fc0ad6c8d6d8210/add-on/Modules/Automation/Controller.lua#L729)
保留 ticket，重复 ACK 仅首次递减 backlog，再次请求可产生新 nonce 回执。
旧 `automation.py:452–476` 支持重新调用 ACK 并清屏。新版丢失了这条幂等收尾能力。

建议：在身份与已归档报告核验下，提供同 operation 的 ACK 状态重取/幂等 ACK，
不能重跑探针。若永久无法确认，应允许用户明确放弃宿主收尾，保留“ACK 未确认”
而不是宣称成功。跨 reload 的 ACK 记录保留策略必须一并定义。

## B03：没有完整发出去的命令也可能留下永久占用

派发先写 intent，然后再检查输入 freshness。现有
`TestProbeDispatchPersistsIntentBeforeInputAndNeverReplays/expired` 已能生成
`dispatch_requested/unresolved` 且 `MessagesQueued=0`；部分派发也有同样阶段。
abandon 只接受完整派发，cancel/reset 又拒绝当前状态；resume 等待不一定会产生的报告。

通过真实派发接口的故障注入结果：

```text
expired: queued=0 complete=false cancel=invalid_transition abandon=invalid_transition occupied=true
partial: queued=3 complete=false cancel=invalid_transition abandon=invalid_transition occupied=true
```

旧版 `automation.py:388–402` 记录发送错误后退出，没有新增这类只能经特定终态释放的
持久窗口 owner。这是新协调机制缺少退出协议，不是要求恢复旧版较弱的协调。

不自动重放未知输入是正确防护。需要分别处理：可靠零输入证据允许重新校验后恢复；
部分/未知输入只允许显式终止宿主等待并保留未知结果。不能自动把后者当作从未执行。
当前 `faults.go:218–229,254–259` 已有 `unsentAckIntent` 路径，可作为一致性参考。

## B04：短暂战斗永久隐藏了未完成报告

[ReceiptView.lua:185](../../addon/Bridge/ReceiptView.lua#L185) 把
`PLAYER_REGEN_DISABLED` / `LOADING_SCREEN_ENABLED` 直接交给 `hide()`；
`:18–28` 清除 producer、显示与所有事件监听。脱战没有恢复路径。
但 ProbeQueue 仍 Busy，Identity 拒绝重建身份，flush 又要求 fresh readiness。

在生产 Lua 模块的 `receipt_recovery.lua` 场景中，reported 后注入战斗开始/结束：

```text
AUDIT: immutable reported receipt permanently lost after combat; busy queue still owns runtime
```

原 fixture 通过；注入后的恢复断言失败。loading 走同一隐藏函数，但本轮没有单独
完成跨 loading 的状态复现，保留为同类待测项。
旧版 [AutomationOverlay.lua:180](https://github.com/Follen/Lychee-Dev/blob/41af9cb616dcb7a9e604619a4fc0ad6c8d6d8210/add-on/UI/AutomationOverlay.lua#L180)
保留通知直到显式隐藏/重载。

建议区分暂停显示与永久失效，保留不可变报告引用；脱战后重新验证 actor、session、
epoch 和输入资格，再产生新的 readiness。不能继续使用战斗前的输入许可。

## B05：固定小模块产生低缩放解码空洞

[ReceiptView.lua:61](../../addon/Bridge/ReceiptView.lua#L61) 的模块固定为 2 UI 单位，
物理像素随 effective scale 变化；[receipt_card.go:65](../../internal/desktop/receipt_card.go#L65)
又跳过低于 2 物理像素/模块的候选。

实际 Lua MatrixSymbol 编码器 → 合法 ready JSON → 确定性栅格 → 生产 DecodeSymbols：

| 物理像素/模块 | 1.28 | 1.42 | 1.6 | 1.8 | 2 | 2.35 | 3 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 解码数 | 0 | 1 | 1 | 0 | 1 | 1 | 1 |

1.28 / 1.8 分别对应 2 UI 单位 × effective scale 0.64 / 0.9。
旧 overlay `:116–117` 按物理像素换算使用更大的模块。
这证明合成输入存在失败点，**不代表已完成这些缩放的真机截图验收**。

建议按物理像素约束最小模块，或补充有界低分辨率采样。缩小二维码必须同时通过
Lua 编码→物理栅格→读取器的分数缩放矩阵，不能只看某个客户端的一次成功。

## B06：默认全窗口捕获拒绝超宽屏

[frames_windows.go:253](../../internal/desktop/frames_windows.go#L253) 对默认全窗口
任一边大于 4096 直接拒绝；[symbols.go:23](../../internal/desktop/symbols.go#L23) 同样限制。
旧 `windows.py:57,542–548` 使用左上 30% ROI。

同一 5120×1440 合成图：全窗 `desktop.invalid_decode_region`；裁剪旧策略的
1536×432 ROI 后成功解码 1 个符号。当前显式 `--capture-area` 可绕过，故为 P2。
建议默认采用裁剪到窗口边界的有界 ROI 并保存实际区域，而不是取消解码预算。

## B07：reset 的失败解释过度确定

[live-startup.md:51](../../skills/lycheedev/references/live-startup.md#L51) 说没有回执
“means the trigger never displayed”。但 `reset_window.go:129–143` 先发送，再捕获；
未观察到不能证明未显示/未执行。应保持未知，保存输入与捕获证据，不能据此盲目重发。
现有 `TestResetStaysPendingWithoutReceipt` / `TestResetRejectsForeignReceiptNonce`
通过只证明 CLI 没有误报成功，不能证明 Agent 对原因的解释正确。

## 两项编排能力迁移损失

### ACK→hide 从程序保证变成额外 Agent 步骤

旧 `automation.py:469–474` 的 ACK 自带 nonce-scoped unidentify，并验证回执消失；
旧 `selftest_offline.py:968` 有相关断言。
当前 [finalize_ack.go:54](../../internal/live/finalize_ack.go#L54) 已返回 cleaned/completed，
却仍需额外调用 [hide_receipt.go:54](../../internal/live/hide_receipt.go#L54)，后者还要 reconnect。

这符合当前原子命令合同，但增加一次 Agent 决策和连接失败点；结果里的 `complete=true`
也容易被当成用户任务完成。2.0.5 Skill 已要求继续 hide，不能把文案修正当成恢复了
旧程序保证。建议保留原子接口，同时提供可靠的整任务编排或明确的 display-cleanup
状态；端到端验收应检查最终画面清理，而非只断言操作 completed。用户明确保留回执
的需求仍应被尊重。

### 性能调查参考删除，没有替代内容

旧 `references/runtime-investigations.md:25–64` 区分 GC/保留内存/观察者开销，要求
私有 replay、恢复 profiler/GC 状态、区分粘贴输入与执行耗时。当前 Skill 仅保留一般
有界探针指导。可迁移精简后的方法到当前 API；不需要恢复旧 Python 或用户数据。
这是指导覆盖损失，未证明已有一次实际性能结论因此出错。

## 为什么测试很多仍遗漏

- 现有生命周期测试部分以“拒绝重放、仍 unresolved、owner 保留”为终点，没有继续验证
  用户能否安全退出或恢复。上述新退出断言 RED，而原定向测试全部 PASS。
- reload 测试覆盖同 release 的新 epoch，没有覆盖受管升级后的 release 切换。
- 二维码测试覆盖隐藏，却未检查脱战后的报告恢复；小模块测试主要从 2 物理像素起步。
- skill-contract 只验证命令路径/参数。79 命令、185 引用、0 violations 不能证明编排正确。
- 历史确实有 Skill 前向模拟测试与若干真机记录，不能说完全没测；但它们不是上述故障
  场景矩阵，也不覆盖所有显示缩放、升级与中断组合。

## 修复与重新验收顺序

1. 先补 B02/B03 的显式恢复出口与持久 ACK 语义，确保每个有副作用的阶段有下一步。
2. 修 B04 的暂时失效恢复，保留防战斗/secret 防护。
3. 设计 B01 的明确升级握手，分别验证首次安装、旧运行态升级、重启/重连。
4. 用 B05/B06 的物理缩放与 ROI 矩阵确定最小可靠码尺寸。
5. 恢复完整编排的机械保证并修正 B07，最后做正式服 WGC 真机闭环。

保留：身份绑定、nonce、managed 文件校验、单 writer、未知输入不自动重放。
验收不能只看 sum=55：还要包括丢 ACK、零/部分输入、脱战、升级、新 Agent 收口，
并分别检查报告、磁盘 owner、运行态队列与最后的二维码状态。

## 本轮证据位置

本机 `.tmp/audit-20260927/`：

- `upgrade-control.log` / `upgrade-repro.log` / `upgrade-repro-test.go`
- `lifecycle-existing-tests.txt`、`lifecycle-ack-lost*`、`lifecycle-dispatch-lost*`
- `transport-combat-recovery.lua` / `transport-combat.log`
- `transport-small-scale-test.go` / `transport-small-scale.log`
- `transport-ultrawide-test.go` / `transport-ultrawide.log`

临时 Go 复现文件已移出源码目录，避免把刻意失败的审计断言混入正常测试。
没有执行完整旧程序；“旧版对照”指固定提交的源码行为与同输入策略对照。
本报告没有把未覆盖的工作台功能或手工导出→CLI 路径列为确定退化。
