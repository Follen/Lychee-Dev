# 共享 slot 1 恢复闭环：改动范围前置验证

2026-09-29，`codex/live-channel-recovery`。三个子 Agent 分别做生产发布路径复现、
协议选项审查和 Skill 编排审查；主 Agent 独立复跑、核对与汇总。
本轮只新增离线诊断测试与文档，不修改生产执行逻辑、不输入游戏、不改真实安装或日志。
后续[架构复审](live-architecture-review-2026-09-29.md)补充了每实例配额、路由身份、关闭
与预算合同。本记录的集合仍是原型候选，不是已选定并验证的正式协议。

## 已确认的结果

测试：`internal/live/channel/slot_cycle_test.go`。

```text
go test ./internal/live/channel -run '^TestSlotCycle' -count=1 -v
PASS 9.528s
```

这些是现状诊断测试：PASS 表示准确复现阻塞并保护原负载，不代表缺陷已修复。
后续实施要把恢复目标写成新的正向验收，不能把“永久阻塞”当产品保证。

| 场景 | 结果 |
| --- | --- |
| 同实例旧 bind 未确认，占物理 slot 1；选择新 runtime 恢复 | 新 bind 发布得到 `delivery.slot_reserved`，映射 `ErrPublicationPending`；未进入 Supersede |
| 同安装/同 build 另一实例占 slot 1 | 同样阻塞；不覆盖对方 payload |
| 以上两种场景重新 Load 日志，两次 finishRecovery | 原新 bind nonce 不变；仍然阻塞；输入次数=0，退役次数=0 |
| slot 1 阻塞时，另一实例发布空闲 slot 2 | 成功，证明未长期持有安装级 OS 锁 |
| foreign owner 后来取得准确消费回执 | 原恢复完成恰好一次 bind、一次旧事务退役 |
| slot 1 空闲的独立对照 | 恢复完成恰好一次 bind、一次退役 |

发布、消费、退役、文件预约、OS publication lease、日志读写均调用生产实现。
游戏候选、输入与回执来自测试 adapter；测试从 RecoverRuntime 进入，不是实机 reload
或完整公开 CLI 回放。初绑的 Bound 门控是另一个已知缺口：此实验说明即使修好它、
允许进入恢复，未知 slot 1 预约仍会阻止新 bind。

`go build ./...`、`go vet ./...` 通过；`go test ./internal/live/channel -count=1`
通过（27.465s）。未重新执行完整跨客户端实机矩阵。

## 为什么仅改协调器不足

当前契约同时要求：新 runtime 从 nextSlot=1 开始；每个物理文件只能有一个未知预约；
旧 runtime 的预约要在 fresh bind 后才允许退役。
这三条在上述场景形成循环。它们各自都有安全理由，不能简单删其中一个判断。

已有 RecoverBinding 可以处理“已取得准确旧 bind 的 runtime_changed 拒绝回执”的窄场景；
它不凭空产生缺失回执，也不能证明另一实例的预约可以覆盖。观察门控、错误分类与
总预算等主机改进仍有价值，但不能宣称已解决全部共享槽位隔离。

## 变更决策

若接受一个未决实例使其他需要同槽的实例明确阻塞，可维持 v1，完成中等规模的主机收敛。
若要求在有限并发/恢复预算内，未知旧预约不阻挡另一 runtime 的新绑定，则需要改变
单文件单预约的协议语义，或另设独立身份控制路径。这是局部协议变更，不必推倒工具链。

当前优先验证的候选：**同一物理槽保存有界多封套集合**。

- 保留固定 Ctrl+Alt+F12、64 LoD、nextSlot、只读内存、prepare/commit 和实例排他。
- A/B 发布各自不可变封套，在短锁内原子合并；未知旧封套保留，不先强行退役。
- 新 runtime 的 bind 可与旧 runtime 封套并存，接收端只选择精确 runtime 的唯一匹配，
  然后沿用 owner/fence、GUID/build、nonce/challenge 校验。歧义必须拒绝，不能选第一条。
- 精确 consumer 与 PID/创建时间由主机掌握；不假设 Lua 能读取 Windows PID。
- 封套摘要与物理文件 generation/hash 分开。发键前验证准确成员仍在且未变，
  不把其他实例合法追加视为本封套篡改。发布中断恢复必须涵盖整个集合的前后字节。
- fresh bind 后只退役对应 consumer/runtime/nonce；旧回执晚到不删除后来成员。

**它目前只是代码层候选，尚未实现或实机验证。** 相比新增按槽快捷键，复用面更大，
但改动涉及 Go 发布账本、Lua loader/协议、安装与日志版本和故障测试，不能称小补丁。

候选必须通过的前置门槛：

1. 同/异 consumer、旧/新 runtime 共存，迟到回执与并发追加均不串请求。
2. 文件发布崩溃、成员删除崩溃、摘要不符、重复匹配、runtime 身份歧义均安全失败。
3. 限定成员数和总字节，并按最大接纳实例数/未决事务数/恢复次数预留控制容量；
   满时返回明确背压，不按 TTL 删除未知封套。不承诺无限故障隔离。
4. 已加载但无匹配的物理槽也已耗用，必须显式推进/拒绝/恢复，不能原槽补写后重读。
   现有 Retail 实测 `transport-experiment-2026-09-28.md` 已证实 LoadAddOn 重复成功
   不会再执行新文件；这项限制不能靠 Lua 表路由绕开。
5. 迟到 Wake 仍可能消费后续槽；集合不等于精确寻址，不取消未知输入不重发的约束。
6. 协议显式版本化，先处理共享安装所有实例再升级，旧 loader 不得加载集合格式。
7. 实测单实例开销、集合大小和多实例吞吐；集合会增加文件/Lua 分配开销，收益不可凭空宣称。

## Skill 审查的修正结论

现行 Skill 已要求固定实例、不切角色绕阻塞、等待 active driver、不重发 unknown。
因此不能把现状归因于 Skill 缺少这些禁令。

真正缺口：Native 将短 OS 发布锁超时与持久预约冲突压成同一个 `shared_publication`，
ProjectResult 没有结构化预约槽位、owner 引用和所需变化条件；Agent 无法准确决定等什么。
`--wait-seconds` 每次新建 context，Skill 提到的任务总预算并没有持久实现。

最小调整是 CLI 提供准确 blocker 和持久预算，Skill 只串行继续原请求或等待具体变化。
不新增恢复技能/脚本、不让 Agent 修改预约，也不在 CLI 未实现前编造新命令说明。

## 下一步边界

前置验证已完成，闭环从风险升级为离线复现事实。架构实施范围应据此分成主机收敛与
有界集合协议原型两个可独立验收的部分。先验证协议原型再决定正式迁移；本轮没有把
“候选设计可解释”当成“已解决”。
