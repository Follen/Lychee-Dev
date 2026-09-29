# 待确认报告的显式 reload 恢复

日期：2026-09-29。基线提交 `2e34e489b776beea9df5e3ffb322da1aad3a4e94`，
分支 `codex/live-channel-recovery`；未发布。

## 现场与修复范围

原项目 `D:\Code\wow\addons\LycheeNote`，连接
`CON-749b5998e1db149c06b926ebf1f22a79`，请求 `lycheenote-settings-1`。
日志显示 `confirm_ready`、confirm `input_attempted`、六条消息 submitted，
随后有 `disconnect_intent`。候选报告为 `ok=true/resourcesReleased=true`，
但没有 fresh confirm，不能升级为 verified。原探针打开 UI 后未注册 OnCleanup，
也没有关窗代码；“超时导致关窗代码没跑”与源码不符。

`active window driver` 是主机 OS 执行锁冲突，不是游戏 UI 框持有的锁。
本次调查时无仍在运行的 lycheedev.exe，不能重建当时并发 CLI 的精确时间线。
UI 吞掉确认键是可能解释，未通过物理输入对照试验证实；本修复不声称修复任意第三方拦截器。

确定的恢复缺口：原 CLI 拒绝有待处理事务的显式 reload；resume 只能继续观察原回执，
disconnect 又要等待该事务。不能以重复快捷键或改写日志解决未知输入。

修复允许显式 reload 保留待处理事务并接入既有 RuntimeRecovery。
仍使用输入就绪观测、原目标、主机执行锁、先意图后输入及新 runtime 准确绑定。
绑定确认后才 Supersede 原槽位。opaque 未确认报告保留 execution_unknown；
observation 可按原策略留待后续 resume。已有 closing 时两种策略均不重新执行探针，
保留未知结果并完成 unbind。reload 自身完成与业务报告是否 verified 分开呈现。

恢复日志新增 `reload.reconcilePending`。本轮中途日志需由本修订 CLI 恢复；
旧 CLI 不支持此恢复状态，不应混用。

## 自动检查

- 红灯：`go test ./internal/live/channel -run TestExplicitReloadCanRecoverUnconfirmedReport -count=1`
  在原约束下报 `explicit recovery reload rejected ... live.channel_journal_invalid`。
- 修复后上述测试覆盖 opaque/observation、open/closing、Supersede 故障后从日志续跑；
  断言只有 reload/bind/必要的 unbind，无原确认键重发或业务重放。
- `go build ./...`、`go vet ./...`、channel 包全量测试通过。
- 强制 Lua 5.1 的 `go test -count=1 ./...` 全量通过；其后 reload 完成结果映射的小修订
  再次通过 build/vet/channel 全量测试。
- `node tools/skill-contract.mjs --bin .tmp/lycheedev-recovery.exe`：87 contracts，204 references，零违规。

## 实机结果（与离线测试分开）

Retail 120100 / 12.1.0.69933，PID 75904，进程创建身份 134351123636813815。
仅调用公开 CLI，固定原项目、角色与进程；未手改日志、槽位或发送原始按键。
游戏 addon 为干净受管 3.0.0，使用本 worktree 编译的 CLI。

1. 原 CON 执行显式 reload，请求 `recover-settings-channel-20260929`，45 秒 host wait。
   runtime 从 `0000003900069eb988011333555f421b` 变为
   `0000003a0006a6bbb9db309da6a221ca`，新 bind 后完成旧连接关闭。
   原 LMO `LMO-6530e6eede5d59d41f25c00c410ed63f` 保留未知、未重跑。
2. 同进程/角色重新 connect 得到 `CON-cfd88d49a8a31e44c8249b228745386a`。
3. 只读 `return { channelRecovery = true }`，request `channel-recovery-smoke-20260929`，
   budget 5 秒：`complete=true/reportState=verified/cleanup=complete/report.ok=true`。
4. 新连接 disconnect：`closed=true/complete=true`。安装 owner 目录无 JSON claim。
5. 用最终 CLI 重读同 reload 请求：`closed=true/complete=true`，原报告仍 unavailable。

证据保存在原项目 `.lycheedev/live/connections` 及 scans：
`d6082eb8d3259918eb35376678238f35`（恢复）、
`6f08caba475cd492cdb6d02dc09f0a89`（重连）、
`f502b620d5a8b9b2e41091d4be9876e9`（只读往返）、
`b38dc0f6390ba7232d715bd9202f8a8f`（断开）。

Classic/Titan/Forever、任意键盘拦截器及人工交互验收均为 not_run。
本轮未修改 addon UI，不把通道往返当作视觉或人工验收。
