# 正式服同 build 双实例验收（2026-09-29）

结论：本轮 14 组实机功能套件通过，包括同目录槽位争用、双方容量换代、单边 reload
隔离及两类中断恢复。**功能通过不等于延迟稳定，也不等于未知槽位预约可以独立恢复。**
本次没有发布或升级游戏 addon，没有操作同时在线的 Forever 客户端。

## 目标和候选

两端均为 Retail `12.1.0.69933` / interface `120100`，共享
`D:\Game\World of Warcraft\_retail_`。受管 addon 为 `3.0.0`，基于提交
`2e34e489b776beea9df5e3ffb322da1aad3a4e94`；本次生产修改在 Go 主机侧。

| 目标 | PID / 创建时间 | 角色 |
| --- | --- | --- |
| A | `75904 / 134351123636813815` | 灵止光－死亡之翼 |
| B | `16592 / 134351215343391864` | 人帅网卡－主宰之剑 |

证据根目录为工作树下 `.tmp/retail-dual-20260929/`。版本号相同不能代替二进制身份：

| 阶段 | CLI SHA256 | 范围 |
| --- | --- | --- |
| 含扫描完成后冷却的候选 | `5CAAB73B336140C93DCBE56F841EB609B564DFC347156AB5BF940036BAD89B2C` | B public 续跑及两端单实例矩阵 |
| 再加入样本过期分类的最终候选 | `7AC0397E63E943A5081DD7DF9DA71A3C5C3469E16D0D4A2CDEC903CEDBD3D8A1` | shared / cross 双实例矩阵及只读扫描基准 |

对应辅助程序哈希见 `binaries.json`、`final-binaries.json`。最初 B 连接、normal-1、
normal-2 阻塞及旧连接交接使用更早的构建，没有保存其二进制哈希；这些证据不作为最终候选
的性能对照。单实例套件没有在最后的窄调度修正后全部重跑，最终候选的实机覆盖是下方
shared / cross 两套；addon 本身未改动。

## 实机矩阵

| 套件 | A | B | 覆盖 |
| --- | --- | --- | --- |
| public | 通过 | 原请求恢复后通过 | 13 次普通请求、容量换代、历史只读、编译失败、cleanup 失败自动 reload、诊断、重复断开 |
| focus | 通过 | 通过 | 单行/多行自建输入框各三次 ESC；内容及光标位置保持不变 |
| reload-readiness | 通过 | 通过 | 就绪时零额外 ESC；cache-off reload、同请求只读、观测式 fallback |
| input-policy | 通过 | 通过 | 主动保护/释放、超时、异常、场景 guard、无残留保护 |
| automation-history | 通过 | 通过 | 成功/失败报告的 ID、字节、角色归属跨 reload 保持 |
| workbench | 通过 | 通过 | 八页程序断言和 WGC；无页面主动抢焦点、旧设置/QR 入口 |
| shared-installation | 双端通过 | 双端通过 | 六秒发布锁、原 nonce 续跑、13 对并发请求、双方容量换代、单边 reload 不影响另一端异步任务 |
| cross-client | 双端通过 | 双端通过 | 并发异步、无缓存大报告、探针超时、observation 与 opaque 的中断语义 |

原始报告分别在 `<suite>-a/`、`<suite>-b/`；B public 的成功报告是
`public-b/baseline-resumed-1790649009532-53524.json`，原失败 `baseline.json` 未覆盖。
共享与中断报告为 `shared/report.json`、`cross/report.json`。

故障测试中的 exit 6 是原请求待续，opaque 恢复的 exit 5 是预期的未知结果，均不是
把非零退出忽略后算通过。两端 observation 保留逻辑操作并在新 runtime 重新观察；
opaque 连续两次 resume 均保留 `execution_unknown / reportState=unavailable`，
断开后仍 `complete=false`，没有把连接收尾冒充业务成功。

共享安装独立审计 `shared/journal-audit.json` 已通过：每端保持一个原 prepare nonce，
各有一次真实容量换代、各 13 个 paired 请求；A/B 分别 64/66 次非零提交及 53/55 次
可靠零发送。没有已提交或未知输入重放，审计时 `pendingSlots=0`、`claims=0`。
后续中断矩阵的完整日志审计见 `cross-audit.json`；最终安装状态见 `final-addon-status.json`。

## 原故障现场也进入了验收

初始发现 A 仍由 LycheeNote 的 `CON-ed964a4e98b09477d89715969731601b` 持有。
经用户授权协调交接后，沿原项目和 CON 执行 request `handoff-retail-dual-20260929`，
验证新 runtime、关闭原连接并退役准确预约。旧业务 `layout-data-and-hover-check-v2`
仍为执行未知，没有重跑或伪造报告。

B 的 normal-2 实际被该旧会话的 slot 8 阻塞，返回了准确 consumer/runtime/nonce。
原 owner 清理后，B 沿同一操作、同一 confirm nonce 完成报告验证与 release。
`public-b-audit/audit.json` 核验了 3 个分段、623 条事件、8 个内容寻址文件；
69 次提交和 32 次可靠零发送没有已提交输入重放。失败报告、恢复记录和成功报告全部保留。

## 测试中修正的内容

1. 慢 runtime 发现从**完成时间**开始冷却，间隔按扫描耗时限制为 1–30 秒。
   原来扫描耗时大于一秒时，开始时设置的一秒间隔已过，可能连续全扫。
2. 仅对“严格匹配时新鲜、Find 返回后过期”的输入样本重新观察，不立即再找 runtime。
   500ms 门槛和发送前身份/owner/slot 复核不变；缺失观测仍能进入换代发现。
3. shared / cross runner 等准确依赖变化后续跑原 CON，保留每次调用证据，不更新请求预算。
   故障注入必须观察到 running；探针已经完成时仍判失败，不冒充中断覆盖。
4. public runner 支持原连接 `--resume`，不覆盖失败证据。status 错带 `--wait-seconds`
   的一次 runner 失败也保留在证据中。
5. 成功收敛用例的 350ms 测试期限改为 5 秒，保留真实 pending 用例的短期限；
   没有放宽生产预算。新增七项纯 Node 编排测试纳入离线 baseline，不在 CI 操作游戏。

## 性能、视觉及未覆盖范围

最初 B connect 为 109.519 秒，查找记录累计 106.342 秒；重复 identity 全扫占主要部分。
单实例阶段有普通请求达到 75–95 秒，最终候选 shared 中也仍有约 40–60 秒波动。
因此不能宣称性能问题已全部解决。不同构建、缓存和场景的样本不作为严格 A/B 提速证据。
量化明细见 `performance-notes.md`；最终候选的单路/八路扫描原始覆盖率和耗时保存于
`benchmark-a/`、`benchmark-b/`。存在读缺口时不得把进程变化中的局部覆盖算成完整覆盖。

两端单实例 30 张 WGC 已逐张审查，当前空态/短报告布局未见明确裁切、重叠或中文缺字；
保护、普通运行、结果待收取、断开标签分别核对。中断矩阵 WGC 另见 `visual-audit-cross.md`。
图片和程序断言不证明真实鼠标点击、拖动、任意第三方拦截器、中文长正文或所有 DPI 下的行为。

以下仍未由本轮证明：Classic/Titan/其他 build、新装/升级/重登录、真实战斗与 IME、
进程强杀/系统重启、长时稳定性、任意第三方 UI 副作用，以及同安装未知 slot 1 的独立恢复。
`tests/slotset` 仍是未接入生产的模型，v1 共享槽位仍可能被未知预约阻塞，不能据正常竞态
通过而宣传实例完全隔离。

## 最终离线门禁

前两轮报告 `offline/` 和 `offline-rerun/` 保留：第一轮发现已修正的测试期限问题；
第二轮 2315 项 Go、81 项 Node 和必测功能均通过，但两轮均在运行中修改了源文件，
`SOURCE` 失败，均不算冻结通过。

最终只认 `offline-final/report.json` 中的结果。该入口包含 build/vet、强制真实 Lua 5.1
的全量 Go、Node、版本及 Skill 合同、BASE 必测、真实 LuaLS 与完整 SOURCE 指纹核验。
报告记录最终计数、允许的环境 skip、执行命令与源码摘要；只有总状态和 SOURCE 均通过
才算最终冻结验收。此文在最终 baseline 启动前保存，测试后不再改写以绕过指纹检查。
