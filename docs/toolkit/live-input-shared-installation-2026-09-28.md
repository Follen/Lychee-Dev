# 正式服同目录双进程竞态验收

2026-09-28，`codex/nonce-memory-transport`，2.5.1 本地候选，未发布。
本轮两进程共享 `D:/Game/World of Warcraft/_retail_` 的同一组 64 个槽位；
不同于之前 Classic/Forever 两个独立安装目录的补测。

| 目标 | 固定进程 | 已绑定角色 | Build |
| --- | --- | --- | --- |
| A | PID 39704，创建时间 134350482653742652 | 灵止光—死亡之翼，Player-741-066A34E3 | 12.1.0.69933 / 120100 |
| B | PID 61232，创建时间 134350482856605529 | 人帅网卡—主宰之剑，Player-1955-029A873B | 12.1.0.69933 / 120100 |

用户明确授权两个实例的竞态测试，因此无需再从两者中选择一个。普通调查仍须对
未解决的多候选歧义询问。Forever 在线实例未参与本轮。

## 发现与修复

首次并发异步调用中，A 成功，B 在发布阶段返回 `vault.lease_busy` / exit 5。
失败原件保留在 `.tmp/channel-live/retail-shared-initial/report.json`，B 原操作
`LMO-348830b66a752dfb73b43ca540a071a2` 保留原项目和 CON 恢复，没有弃用重建。
恢复期间还经历了 runtime 更换及战斗等待，最终 `fixed-resume-4.json` 报告
`complete=true`、`reportState=verified`、`cleanup=complete`、`report.ok=true`。

改动集中在共享槽位协调：

- 发布锁改为预算内等待；超时为 `shared_publication` / pending，保留原 intent/nonce。
- 主插件、客户端及连接归属校验与可变槽位池校验分开；后者在发布锁内进行。
- 短 OS 锁只保护校验、磁盘写入、发键时的精确负载核验和回执收尾。
  任务等待战斗、焦点、内存或业务结果时不占整个安装目录；持久预约保护确切槽位。
- CLI 恢复进入已发布阶段时，发键前重新获取短锁并校验 nonce/runtime/consumer/
  完整负载摘要及未消费状态，不能仅凭日志中的 published 发送输入。
- 已持久验证的旧回执在崩溃后重复收尾，即使物理文件已被另一 nonce 复用，
  也只完成原日志，不能标记新预约已消费。

## 实测证据

证据位于 `.tmp/channel-live/`，原失败不覆盖。

| 场景 | 状态 | 证据 |
| --- | --- | --- |
| 原失败操作沿原 ID 恢复 | pass | `retail-shared-initial/retailb/fixed-resume-4.json` |
| A 新查询与 B 原任务并行收尾 | pass | `retail-shared-initial/retaila/blocked-peer-progress.json` |
| 第二项目争用同一进程 | pass，输入前拒绝 | `retail-shared-foreign/rejected.json` |
| 战斗中的 reload 不盲发 | pass，pending | `retail-shared-races/retailb-align.json` |
| 双端争锁、预算取消和原任务续跑 | pass | `retail-shared-races-2/*-wait-resume.json`、`*-wait-resumed.json` |
| 反复同时执行与自动容量 reload | pass，两端各 13 次，各一次容量换代 | `retail-shared-races-2/report.json` |
| 单边 reload，另一边异步任务不换代 | pass | `retail-shared-races-2/retaila-peer-reload.json`、`retailb-peer-finish.json` |
| 关闭连接、完整日志与资源审计 | pass，pending=0、claim=0 | `retail-shared-races-2/journal-audit.json`、`final-addon-status.json` |

`tests/channel-live/shared-installation-baseline.mjs` 是本轮可重复入口。
`hold-publication` 测试模式只暂持 OS 发布锁，无键盘输入，不修改槽位/日志。
操作系统锁竞争、持久预约冲突、不同槽位独立推进、旧回执晚收尾、恢复前负载校验，
以及取消后 nonce/intent 不变均有离线回归。

最终成组报告 `retail-shared-races-2/report.json` 为 complete=true。日志审计包含
轮转段，两端 wait-resume 各保留唯一 prepare nonce，未重复提交相同
runtime/exchange/kind 的输入；各有一个真实 capacity reload intent。
完整连接链（含本轮先前恢复）有 46 / 39 次已证明零发送的输入等待/重试，
因此不能把成功结果等同于稳定的低延迟。单轮成对查询约 9.8–105.4 秒，受实际
按键占用、采样新鲜度和机器负载影响；本轮重点是正确性，没有声称完成性能优化。

重叠验证见 `overlap.json`：A 新 runtime 的绑定采样比 B commit 前的输入采样
晚 34,208 ms，早于 B 的 35 秒异步定时器下限；因此确实覆盖了 B 执行期间的
A 换代，而不只是先后运行。B 的最终报告身份及 runtime 保持不变。

两个 CON 均经公共 disconnect 关闭，主插件和 64 槽位仍为 managed。
最终 WGC 位于 `retail-shared-races-2/{retaila,retailb}-final/evidence`，已检查
两端场景没有残留接收面板、桥活动荔枝或输入保护层。本轮不修改 Lua、受管安装
或 npm 包；CLI 改动与 Skill/设计契约留在当前分支。

## 边界

首次 full-suite 执行出现旧协议测试 `bootstrap-full-lifecycle` 的 EOF，原失败保留；
随后在最终实现上强制 Lua 5.1 完整重跑通过，输出为 `retail-shared-go-tests.txt`。
未将一次重跑通过解释成已定位该旧测试偶发失败的根因。build/vet、Skill 命令合同
和 2.5.1 版本一致性检查通过。当前文档不构成发布许可，也不声明零错误率。
游戏中受用户操作和战斗约束的未完成步骤保留 pending，不能用离线测试替代真机通过。
