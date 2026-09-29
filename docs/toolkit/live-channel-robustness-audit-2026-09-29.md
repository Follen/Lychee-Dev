# 通道恢复边界审计

2026-09-29，基于 `codex/live-channel-recovery` 未提交修订。
本轮为审计，以下三项已离线复现，尚未修复，也未向游戏发送输入。

## 已确认

1. **P1：reload intent 遇到外部 runtime 更换会持续等待。**
   在保存 reload 意图、尚未发送 reload 输入时，用户自行 reload 或其他 runtime
   更换使旧输入采样消失。`stepReload` 先要求旧 runtime 的输入观测；
   `Continue` 又因为 reload 未 complete 而跳过 RuntimeCandidate。
   后续 resume 保持同一路径，不会发现已经存在的新 runtime。
2. **P1：首次绑定期间 runtime 更换没有自动恢复入口。**
   Bound=false 时，Continue 不发现新 runtime。旧 bind 无回执或旧输入状态消失后，
   连接可持续 pending。已有 RecoverBinding 方法没有生产调用点。
   不能通过简单重发旧 bind 修复；需要准确验证候选身份、旧事务退役与新绑定。
3. **P1：丢回执但槽位已前进时，显式 reload 仍被输入门禁拒绝。**
   例如主机保留 confirm/index=4，游戏已经消费它并发布 nextSlot=5，但主机无法取得
   confirm 回执。reload 不加载槽位，却仍使用主机 index=4，inputObservation 要求
   nextSlot 完全相等，因此回报 input_target_changed。新增显式 reload 恢复出口
   覆盖原现场“槽位未前进”的情况，不覆盖这个变体。

## 复现

临时测试保存在 worktree `.tmp/robustness_audit_test.go`，未加入正常测试发现路径，
避免把已知红灯留在产品测试中。复跑时复制到 `internal/live/channel/` 同名路径：

```text
go test ./internal/live/channel -run '^TestAudit' -count=1 -v
```

测试调用真实 Driver.Continue 和 inputObservation；只替换系统输入/内存后端。
无客户端、无原始按键、无真实项目日志修改。

输出摘要：

```text
reload_intent: live.channel_pending / context deadline exceeded; discoveries=0 input=0 bound=true
initial_bind:  live.channel_pending / context deadline exceeded; discoveries=0 input=0 bound=false
reload slot advance: live.channel_input_target_changed
FAIL (0.889s total)
```

这些证据确认状态机路径与校验缺口，不证明每次实机超时都由它们造成。
修复应分别补充崩溃续跑、身份变化拒绝和禁止未知输入重放的回归；
不能去掉所有 nextSlot 校验或对未知动作直接重试。
