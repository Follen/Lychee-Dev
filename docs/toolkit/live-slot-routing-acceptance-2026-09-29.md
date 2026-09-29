# 200 槽正式服双实例验收

2026-09-29，本次工作区候选通过正式服同安装、同 build 双实例验收。
对应合同：[200 槽前向分配与迁移](live-slot-routing-2026-09-29.md)。
候选版本 3.0.0，基于 `b71ab1b3905c9ab8b3cf01121073e0e3ad13ff7f` 的未提交工作区；
使用 `release.mjs dev-package` 生成的开发包并通过受管安装部署，没有发布 npm 或 GitHub Release。

## 环境与范围

| 实例 | PID | 角色 | GUID |
| --- | --- | --- | --- |
| A | 9480 | 灵止光 / 死亡之翼 | Player-741-066A34E3 |
| B | 48276 | 暗夜荔枝 / 罗宁 | Player-729-06AA5F07 |

两实例共用 `D:/Game/World of Warcraft/_retail_`，实际 build 为 **12.1.0.69933**。
受管安装从旧 64 槽迁移到 200 槽；两实例分别 reload 激活并证明 slots=200、inventory=200。
本机这次不需要重启客户端，不代表所有客户端的目录缓存行为相同。
Classic、Titan、Forever 的本次 200 槽实机验收均为 **not_run**，不继承旧 64 槽结论。

## 实机结果

| 检查 | 结果 |
| --- | --- |
| A 已发布、未发键时占用第 2 槽，B 继续执行 | B 自动发布到第 3 槽并完成，A 原槽字节及预约不变 |
| A 恢复原请求 | 原请求完成，业务 attempt=1，唯一 commit 提交及 accepted 收据链 |
| A 再次执行同一请求键 | 返回既有结果，连接日志字节不变 |
| 人为持有共享发布锁 6 秒 | 两实例各自原请求返回等待，锁释放后 resume 完成，prepare nonce 不变 |
| 双实例连续任务 | A、B 各 47 轮，共 94 轮，全部角色 GUID、任务标记、报告和清理正确 |
| 高位槽位与容量换代 | 两边分别用到 nextSlot=187、188；各出现 1 次真实容量 reload，随后继续完成任务 |
| 一方 reload、另一方异步执行 | A reload 成功，B 异步结果正确且 runtime 保持不变，cleanup=complete |
| 正常断开与后验审计 | 两条连接 closed=true；200 槽未决预约 0，窗口 owner 文件 0 |
| 输入效果账本 | 全部分段日志中，无重复提交已发送或结果不确定的非 Escape 输入 |
| 安装状态 | 测后公开 addon status 返回 managed |

长测连接：A `CON-7b50beb398a20ecabc758bc77b464dba`，
B `CON-1436822b4fd057847af57e2deb27b7a5`。
审计统计 A/B 已提交效果分别 200/202，明确未发送的尝试分别 44/52。
未发送尝试没有排入按键消息，不能将它们误记为重复发送。
本验收证明可观察的提交及回执链，不声称读取了不可观察的游戏内部执行计数。

## 离线与 Skill

最终 `tools/baseline.mjs` 通过 BUILD、VET、GO、NODE、VERSION、SKILL、
SKILL-GENERATED、SOURCE 全部门禁；GO 为 **42 包、2572 项、失败 0**，其中 34 项
因专用环境或辅助进程条件跳过，保留原始 skip 记录，不能计作实机通过。
强制 Lua 5.1，`-count=1`，`-parallel=4`，本机以 `GOFLAGS=-p=2` 限制同时测试的包数。
最终回归时间为北京时间 18:36:45–18:46:52；GO 耗时约 579 秒。
该次源码指纹为 `67a7320c94cb997ab91960cfcd19e29236e490de864f0ad3e3380f6bae2b0847`。
回归结束后仅补入本验收文档及索引链接，生产代码未改。

此前失败日志保留：旧实验 Lua 夹具与 v2 不匹配、成功生命周期夹具的短现实时间预算、
提前生成的模拟实时帧，以及串行全库回归的 20 分钟总时限。
修复仅涉及相关测试夹具；生产输入新鲜度、请求预算和独立超时测试保持原约束。
不能把这些失败批次计作通过，最终完整通过批次单独留存。

Skill 已同步固定 CON/PID、原请求恢复、由 CLI 分配槽位、满池等待实际变化、
64→200 先收尾后迁移以及每个 PID 分别激活的编排。
独立 Agent 完成离线恢复场景走查；这不是实机 Agent 自动编排验收。
本轮更新版本化 Skill 源码与开发包，没有替换全局安装的 Skill。

## 耗时与证据

本机长测每轮端到端耗时：A 中位数 43.809 秒、范围 15.023–105.864 秒；
B 中位数 43.192 秒、范围 18.463–110.710 秒，包含容量换代轮次。
与离线回归同时执行，不作为稳定性能基准或延迟承诺。
定向跳槽样本中内存扫描占 B 执行约 68%、A resume 约 58%；
没有独立文件校验计时，不能将余下耗时归因于 200 槽。

证据保存在此次 worktree 的 `.tmp/slots200-20260929/`：

- `skip-live/report.json`：预约跳过、原请求恢复、只读重复调用与断开。
- `shared/acceptance/report.json`：94 轮、发布锁等待、换代、异步隔离与断开。
- `shared/acceptance/journal-audit.json`：完整轮转日志、单次输入、预约与 owner 清零审计。
- `offline-complete/report.json`：最终完整离线基线；较早失败目录原样保留。
- `post-status.json`：测试后的受管安装状态。

复现入口位于 `tests/channel-live/slot-skip-baseline.mjs`、
`shared-installation-baseline.mjs` 及 `shared-installation-audit.mjs`。
测试所用 lab 只在正常 Driver 发布后、发键前注入观察暂停，
没有手工改写预约、日志、收据或伪造已发送输入。
