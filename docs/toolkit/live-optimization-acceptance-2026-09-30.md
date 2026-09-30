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

## Blocker 修复与第二轮实测

`1c32e94ac92b41e903329863947a60622507335e` 修复了三个由失败测试确认的问题：

- 关闭 hints 的完整输入扫描不再被较旧的前缀样本提前终止；已发布的新样本可被找到。
  只有近期旧样本时采用不可续期的 2 秒调度等待，只保存时间元数据，不保存地址或载荷。
- 已绑定、无活动 reload/recovery 时，合法且新鲜的当前 runtime 记录可延后完整身份发现。
  这是调度事实，不授权输入或退役；没有 hints、记录过期或不匹配仍走完整回退。
- 断开前先建立外层 observation 范围，可选 30 秒退役检查不再污染整个 120 秒调用截止。
  已有截止、共享读取额度与持久关闭预算不会重置。

该提交的冻结 offline baseline 全部通过，目录为
`.tmp/baseline-offline-live-blocker-fix-20260930/`，SOURCE tree SHA256 为
`ca49440cce5ac2132f55792bdc54a69bfa878bdf349ba3ed67024608a1ce8709`。
完整 channel race 通过；push/PR CI runs `36691872424`、`36691876747` 均成功。
clean-commit 私有开发包通过资源审计、隔离 offline `--ignore-scripts` 安装及 version/describe smoke，
包 SHA256 `5b0b0c5cefd64bcd08863d0d990de546f338e095fda73a3383d2b7325d92a0f5`；
CLI SHA256 `515439cdc9314bd4b8e77604095ceac089dc7cb01eb0ae0abf87d414c6ed4b1a`。

受管部署、显式激活、同一进程/角色验证通过，激活连接在 20.782 秒正常断开。
生产 runner 的 `.tmp/retail-live-blocker-fix-20260930/` 保留以下第二轮数据：

| 检查 | 状态 | 观测 |
| --- | --- | --- |
| 关闭 hints 的连接 | passed | 31.123s；52.078 GiB |
| 5 条带 hints 普通请求 | passed | 71.680、41.160、25.799、19.888、20.094s；均 verified / cleanup complete |
| 关闭 hints 普通请求 1–3 | passed | 61.162、79.476、64.715s；均直接完成，无 resume |
| 关闭 hints 普通请求 4 | pending cleanup | 120.116s；118.739 GiB；业务报告已 verified，release_ready 清理等待 |
| 原请求关闭 hints resume | pending cleanup | 原 operation 未变；仍保留 verified 报告，不重执行探针 |
| 其余请求、大结果及最终断开 | not_run | 保留原 CON，等待恢复后继续 |

恢复尝试后，使用 `internal/desktop` WGC 只读采样确认客户端停在掉线界面，
错误码为 `WOW51900323`。连续输入 ROI 为黑色且不符合信号码，不能授权任何输入。
该证据证明采样时已不在游戏中；没有逐帧证据确定掉线发生在哪次 lookup，
也不能把掉线归因于候选代码、扫描或光学 tracker。
原 CON、operation、已验证业务报告、发布槽与 journal 保留，尚未宣称清理或原版恢复完成。

第二轮首个带 hints 调用的 67 次 lookup 累计记录 53.647 秒：4 次未知 runtime 身份发现
占 22.836 秒 / 38.766 GiB，11 次输入状态广域搜索占 21.251 秒 / 52.850 GiB，
receipt/BODY/confirmation 的 6 次广域搜索占 9.523 秒 / 24.345 GiB。
其余约 18.033 秒没有独立阶段计时。局部 predicate 拒绝原因未记录，
不能断言是合法的 500ms cadence gap；首个 warm 名称也不代表地址 hints 已预热。

后续独立调度修复给予同一已绑定、正常 published exchange 的首次 `input_signal_waiting`
一次固定 2 秒观察窗口，重复等待不续期，到期仍完整发现；不适用于未知/缺失信号、
未绑定、活动 reload/recovery 或 uncertain 输入。真实 Driver 红测证明旧调度在第二次观察前
进入昂贵发现；修复后第二次新鲜观察完成一次 journaled 输入。持续等待仍按时回退，
不改变持久预算、500ms 门槛或输入权限。该调度问题独立成立，不作为本次掉线的原因。

包含该调度修复的最终冻结 offline baseline 于 09:07:28–09:14:24 UTC 全部通过，
原始目录为 `.tmp/baseline-offline-live-signal-grace-20260930/`；SOURCE tree SHA256 为
`163ae69de9d5d9c37a73351f3fe46b76e1413ccf5644b8feb05139dc2ae54c9e`。
强制 Lua 5.1 全量 Go、build/vet、98 个 Node 测试、真实 LuaLS、版本/Skill/生成参考与
SOURCE 稳定性检查全部通过。该修复尚未部署或复验；本验收摘要在 SOURCE 检查后追加。

原第四条 operation 的持久恢复预算现已到期，状态为 `budget_exhausted`，
但原业务报告仍为 `verified`、cleanup 为 `pending`，不是业务失败。
后续需同一角色重新登录后取得正向 runtime 换代证明，再退役原发布槽并完成清理；
不重启旧 operation 预算，不重执行探针，不伪造 closed 或恢复状态。

## 重新登录后的恢复与第三轮

用户重新登录后，passive inventory 确认旧进程已结束，新 Retail 进程在同一安装。
旧原 CON 依据 `process_absent` 正向证据在 0.131s 关闭，保留原 verified 业务报告，
cleanup complete。随后受管恢复 `8e4dab0`、fallback 激活、同一 actor fresh bind 和
关闭均通过，200 槽 pending 为零；原始记录为
`.tmp/retail-restoration-blocker-fix-20260930/restoration.json`。
这是后续明确的恢复证据，不改变此前掉线时的 pending 事实。

`3a20899` 的 push/PR CI `36694957137`、`36694970917` 均通过。
其 clean-commit 私有开发包、archive 审计、隔离 offline `--ignore-scripts` 安装与 smoke
通过；包 SHA256 `d978ef5bcc51e64650b8d92d8113ae60320184155546053b08bbc72666d23fd8`，
CLI SHA256 `5a096ef42caca1bf80dcc21b75b9a8cfddfd5e9aa62f21ea8c0a0477f90c08f9`。
受管安装、激活、同一角色证明与激活连接关闭通过，随后生产 runner 得到：

| 检查 | 状态 | 观测 |
| --- | --- | --- |
| cache-off 连接 | passed | 20.777s；32.471 GiB |
| 5 条带 hints 普通请求 | passed | 64.378、28.808、10.107、26.953、36.930s |
| cache-off 普通请求 1 | passed | 82.137s；138.172 GiB；直接 verified / cleanup complete |
| cache-off 普通请求 2 | pending | 120.096s；176.791 GiB；prepared，尚未发送业务输入 |
| 第二条原 operation 用 hints resume | passed recovery | 24.391s；31.796 GiB；同一 operation verified / cleanup complete |

该轮 `.tmp/retail-live-signal-grace-20260930/optimization.json` 保持 blocked，
恢复不是 cache-off 通过。十次输入全扫均未接受输入样本；其中三次完整覆盖且无 gaps，
其余可见短读/不可读 gaps，只有最后一次达到调用截止而截断。不能再用重复截断前缀
解释所有失败，也没有保留每次完整的 eligible-region 列表来证明具体地址被排除。

为进一步定位，只增加固定计数的私有开发诊断包以 dirty workspace 明确标记，
CLI SHA256 `cf135dc7944492cf2b70c4960edd3f9db5318ddb7d397c0ee22a490052646943`。
addon 字节未变，仍为 clean managed `3a20899`。新只读 cache-off 请求的报告已
verified，release_ready 清理在 120s pending；输入 predicate 计数为：
8867 次、fresh 6、stale 1380（均超过 500ms，其中 25 次也未超过 after+100）、
target changed 7478（全部为旧槽位；其他目标字段匹配）、clock invalid 3。
最后一次合法样本在 predicate 时年龄 854ms，扫描结束时为 5682ms。
输入全扫阶段共有 11 次、47.241s；没有 integrity 或结构拒绝。
这些计数不保存原地址、角色、载荷或绝对 sampleMillis，也不改匹配或输入结果。

该诊断原 operation 用 hints 在 13.110s 恢复，业务和 cleanup 均 complete。
随后同一连接继续通过带 hints 的大结果（11.964s，完整文本断言）、历史只读请求
（0.109s，原 operation/report 相同且 journal 字节不变）、显式 reload（31.762s，
已验证 runtime 换代）、换代后普通请求（5.723s）、正常断开（6.119s）及重复断开
（0.029s，journal 不变）。`functional-continuation.json` 为 passed；该记录也不代替
其余 cache-off 样本或 cache-off 大结果。

诊断支持一个具体调度缺口：扫描经过合法样本所在位置时样本已过输入新鲜期，
扫描继续耗时数秒，期间新建的不可变字符串可能落在已经访问过的位置。
已为 cache-off 增加仅同次调用、扫描遍历结束后的一次有界邻域补查，并用真实 Source
失败测试验证发布时序。旧样本只提供读调度，新结果仍必须满足完整校验和、当前目标、
500ms、after+100 与发送前复核；不会把旧地址缓存跨调用或延长原恢复预算。

只保留本次扫描中一条最新合法种子：读取时年龄不超过 1500ms，扫描结束时读调度
寿命不超过 10s。整轮遍历完成但有不可读 gaps 时可以补查，原 gaps / incomplete
coverage 原样保留；调用截止、取消或预算截断不能据此补查。局部仍为 partial，固定
250ms / 8MiB / 4096 reads，使用原 Session，不学习 hints、不保存跨调用地址。
局部读到鲜样本后若最后进程 Verify 超时，必须拒绝样本；实际局部超时计入 LocalStopped，
成功返回时不能因自己的收尾 cancel 被错误标记为停止。

真实 Source 的发布时序与最后 Verify 超时两个 RED 均已复现，随后修复转 GREEN。
新增反例覆盖旧/未来/错角色/槽位/owner/fence/runtime/build/结构、种子 10s 失效、
gaps 不得被修复为 full coverage、每次调用仍全扫、共享预算/取消/局部截止与未启用学习。
完整 affected channel/memory tests 为 108.810s / 0.678s，race 为 120.775s / 4.269s；
末次 Verify 修正另有最新定向 race（2.574s / 2.271s）和 build/vet。
原始 RED 与定向检查记录在 `.tmp/input-seed-refresh-20260930/`。
源码已冻结；包含最新改动的正式全量离线基线和实机复测单独记账，尚未由上述定向检查替代。

## 有界补查实机 smoke

最新冻结修复以明确标记 dirty workspace 的私有开发包实测，addon 字节仍为受管
`3a20899`，CLI SHA256 为
`a533b9445585e328ca6192ca20a00f8b175aa0c4c44a8183c40727e01eafedd0`。
固定新进程创建身份与同一 actor，所有命令保持关闭 hints，原始记录为
`.tmp/retail-live-input-refresh-smoke-20260930/smoke.json`，状态 passed：

| 检查 | 状态 | 观测 |
| --- | --- | --- |
| cache-off 连接 | passed | 42.568s；fresh bind，输入样本年龄 215ms |
| cache-off 普通请求 | passed | 46.944s；81.880 GiB；verified / cleanup complete |
| cache-off 大结果 | passed | 77.996s；126.556 GiB；完整文本断言、verified / cleanup complete |
| cache-off 断开 | passed | 45.155s；closed / cleanup complete，无恢复或换预算 |

普通请求有 5 次输入 full_scan 和 2 次 partial nearby_scan，补查累计 1,977,389 返回字节，
其中一个新记录通过完整门禁。大结果的 3 次局部查找未命中，仍通过原完整 fallback
得到 4 次合法输入观察，未将 local miss 改写为成功。两次 execute 的 learningBytes 均为零。
这证明新增路径实际被触达并完成原业务/清理，但单次 smoke 不能证明延迟稳定或整个
五次 warm/五次 cache-off 的最终冻结候选 runner 已通过；后者单独复验。

最新完整冻结 offline baseline 于 10:44:55–10:50:29 UTC 全部通过，目录为
`.tmp/baseline-offline-live-input-refresh-20260930/`；SOURCE tree SHA256 为
`c5215e7e374c775d745608ca71fe90ed1269980b2642abeb32944a4dfca3eef5`。
强制 Lua 5.1 全量 Go（42 packages）、build/vet、98 Node 测试、真实 LuaLS、版本、
Skill、生成参考、BASE-01–21 与 SOURCE 稳定性均通过，含最后 Verify 拒绝和局部超时计数
修正。offline 的 LIVE/REAL not_run 不继承其他实机报告；本段仅在 SOURCE 检查后追加。

## 冻结补查候选仍失败，原连接已收尾

clean 私有候选 `8c8dd9b` 的 CLI SHA256 为
`fe14d5c065cb6f2d543eb7b306e6398ccbfa29b9a62511c7e15c81d4055f0a5b`，
安装与 runtime 均匹配该 commit；push/PR CI 已通过。生产 runner 的证据为
`.tmp/retail-live-input-refresh-final-20260930/optimization.json`，仍为 blocked：

| 检查 | 状态 | 观测 |
| --- | --- | --- |
| cache-off 连接 | passed | 41.698s；58.667 GiB |
| 5 条带 hints 普通请求 | passed | 25.455、21.711、12.474、13.109、9.077s |
| cache-off 普通请求 1 | passed | 71.762s；107.950 GiB；verified / cleanup complete |
| cache-off 普通请求 2 | pending | 120.129s；162.539 GiB；commit_ready，业务 commit 尚未发送 |

第二条有 9 次输入全扫、6 次扫描后邻域补查，邻域全部未命中；最终调用截止截断。
输入 predicate 共 3257 次，其中 fresh 2、stale 430（均因年龄）、旧槽位 2822、
非法时钟 3；没有结构或 CRC 拒绝。最后合法样本在 predicate 时年龄 782ms。
这些计数支持发布与扫描时序仍有缺口，但没有保留具体种子位置，不能据此断言
实机新字符串一定移出某个邻域。

保留原 CON / operation / budget，以默认 hints 完成该 operation 的恢复。随后同连接
通过大结果（55.758s，完整文本断言）、历史只读重试（0.062s，原 operation/report 与
journal 字节不变）、reload（34.760s，已验证 runtime 换代）、换代后请求（18.167s）、
正常断开（2.066s）及重复断开（0.026s，journal 不变）。
`functional-continuation.json` 为 passed；原 blocked 报告没有重写，恢复不算 cache-off
通过。尚未完成的其余 cache-off 请求及 cache-off 大结果保持 not_run。

后续实际 Source RED 已复现：扫描访问旧样本后发布邻域新记录，在完整遍历收尾前
新记录移到另一邻域，原扫描后补查丢失它。正在将补查前移到完整扫描的 CRC 合法
predicate 拒绝处；地址只供调度，成功仍须原 selector 重读与最后进程 Verify。
局部未命中继续同一完整遍历，共享预算、取消和多 worker 排空回归完成后再实机复验。

## 扫描中补查：离线通过，实机仍待修复

实际 Source 发布移动 RED 与并发排空 RED（首个结果之后额外排空 751.5ms）均修复转
GREEN。扫描提前结束只取消候选的局部补查，原扫描读取及最终进程 Verify 保留外层
context；等待/取得共享额度时均响应取消。回调只返回地址，独立严格重读完成后才释放
同一 scope，miss 继续原遍历。全部补查共享 1s 活跃预算、8MiB、4096 reads 与四次
scope，每次最多 250ms；WGC 只以既有 reader 的容量 1 通知等待新合法边沿。
定向 channel/memory 为 1.077s/1.239s，race 三次为 4.202s/3.766s，build/vet 通过。
记录在 `.tmp/input-traversal-refresh-20260930/`；没有把这些测试当作全量离线基线。

私有 dirty CLI SHA256 为
`9779f7a09186b81b0bec310bb7796a73031de218828dc139f2fdec9febdd49e4`，
受管 addon 字节仍为 clean `8c8dd9b`。隔离包安装与 version/describe 通过。
固定同一进程/actor 的 `.tmp/retail-live-input-traversal-refresh-smoke-20260930/smoke.json`
仍 blocked：cache-off 连接 22.370s 直接通过，普通请求 120.130s 停在 confirm_ready；
commit 已 accepted，有 report_candidate，但当时未完成权威报告确认和清理。
该请求的四次输入正向 lookup 各包含邻域校验与独立重读，合计 fresh predicate 8，
对应 finishFresh 3、finishStale 1；不能误解为八个结果全部过期。另有七次完整身份发现
累计 48.718s，输入全扫十次累计 45.729s。扫描中补查实际奏效但不足以完成限时验收。

原 operation 用默认 hints 在 16.879s 完成 verified / cleanup complete，无重放业务。
同一连接正常断开 32.456s，重复断开 0.028s 且 journal 不变，受管 200 槽 pending 为零。
`cleanup-continuation.json` 为 passed；大结果未运行，原 blocked 报告保留。
接下来单独诊断 cache-off 身份发现调度，尚未宣称修复完成或延迟稳定。

## 固定诊断识别光学阻塞

后续只增加诊断的私有 dirty CLI SHA256 为
`6e261b3941198e5e7a61c972668c74d2d6b500bc08e7d01a883fe7a486390672`。
定向测试、三次 race、build/vet 通过，未改接受条件、I/O、停止规则或截止。
诊断分为固定 pre/post 光学分类、补查结果计数及最多 16 条正向 INPUT 相对时间记录；
未知年龄为 null，截断明确可见，不写地址、载荷、角色或绝对 sample 时刻。
`acceptedAtOffsetMillis` 是 scope 的首次早停时刻，`acceptedSampleAgeMillis` 是返回
样本同时间戳的最后一次合格检查年龄；并发时二者不一定属于同一 worker/检查，不能
把它们精确拼成首次接受事件或据差值精确归因排空。`immediateHit` / `afterEdgeHit`
只是局部候选命中，后续独立重读与最后 Verify 仍可能拒绝。

`.tmp/retail-live-input-timing-diagnostics-20260930/smoke.json` 保持 blocked：
cache-off 连接直接通过（22.923s）；其中一个样本最后合格检查为 490ms，返回时
522ms 而被拒绝，同 scope 记录收尾 34ms。普通请求直接通过（41.921s，verified /
cleanup complete），四次 memory finish 均 fresh，post 光学检查均通过；pre 为
13 次，其中 accepted 5、unavailable 5、waiting 3。该请求仍有 20.420s 完整身份扫描。

cache-off 大结果 120.157s 停在 confirm_ready。四次 memory finish 均 fresh，返回
年龄分别为 318、490、147、365ms；post 四次只有两次通过，其余两次 unavailable。
pre 52 次为 accepted 11、unavailable 17、waiting 24。八次完整身份扫描 47.772s，
输入全扫 11 次 54.895s。首次明确记录了鲜内存结果被随后光学门禁拒绝；不能将所有
waiting/unavailable 都归因于同一机制。

原大结果 operation 用默认 hints 恢复到 verified / cleanup complete，完整文本断言
通过；同连接 33.062s 正常断开，重复断开 0.027s 且 journal 不变，200 槽 pending 为零。
实际 Source + 既有异步 reader 另复现：post 检查使合法帧过期并清掉比较历史，随后
1500ms 内的新鲜翻转帧仍 waiting。正在窄修单纯观察过期的处理；真实 invalid/reset
继续清历史，帧 500ms、edge 1500ms 与 after 屏障保留。新行为完整实机复验待完成。

## 光学比较历史修复的开发 smoke

窄修明确改变单纯帧过期后的处理：仍返回 unavailable 并清 EdgeTicks，仅保留最多
1500ms 内的合法 decoded heartbeat 比较历史。下一张新鲜翻转帧建立自己的新 edge；
同 heartbeat 不得复活旧资格。真实 invalid/nil/reset、未来/重放、过长 gap 仍清历史，
frame 500ms、edge 1500ms、after+100ms 与高水位限制保留。原 expiry 测试断言已明确
修订；实际 Source + 异步 reader RED/GREEN、否定测试与重复 race 均通过，build/vet 通过。
此机制可复现，但不能将所有实机 unavailable/waiting 都归为该机制。

私有 dirty CLI SHA256 为
`8fe4f1971d073533ccb99d3c9b9729717d620f98e462c38e71d92f8dc35b3f1d`，
受管 addon 字节仍为 clean `8c8dd9b`。
`.tmp/retail-live-input-signal-expiry-smoke-20260930/smoke.json` 为 passed：
cache-off 连接 23.350s，普通请求 114.713s，大结果 90.220s（完整文本断言），断开
29.187s，均在原固定 120s 调用内完成，无恢复或重放，报告/cleanup 完整。
普通请求的 post 光学八次均通过，但仍有四次可靠零发送后的重试，六次完整身份扫描
43.979s；大结果另有重试。因此这次 smoke 不能证明延迟稳定或全候选验收完成。
全量冻结离线基线与 clean 候选五次 warm / 五次 cache-off 的完整 runner 单独验证。

冻结离线基线 `.tmp/baseline-offline-live-traversal-signal-20260930/report.json` 已通过
（2026-09-30 12:02:39–12:08:01 UTC）。源码树 SHA256 为
`e124caec441b9718e2cc16073911b4c4b710ecca629f4fd2ebf1946ba176eea1`；
SOURCE 稳定性、build 1.905s、vet 1.014s、强制 Lua 5.1 的全量 Go 测试 301.661s
（2731 个测试通过）、98 个 Node 测试 17.048s、版本/skill/生成命令合同均通过。
此后仅追加本段验证记录；clean 候选完整实机 runner 尚待运行。

## clean d149bf6：释放阶段仍受扫描调度阻塞

`d149bf674c7d198bf75f81b79c47c2dda315772d` 已通过 push / PR CI。
clean 私有包 CLI SHA256 为
`4a2501d4d6e8ce8732e8ca61a4c3809b2deefaaa0e4c50a5405f973ff7f16d10`，
归档 SHA256 为 `265e4657c538c9f7b9c400fec5a37036a44ec5610598ff6e84e5f130afdcc79a`；
隔离安装、version/describe、受管更新与同进程/actor 激活均通过。
`.tmp/retail-live-input-traversal-refresh-final-20260930/optimization.json` 保持 blocked：
cache-off 连接 49.711s，五次 warm 为 49.516/8.681/14.884/20.051/8.878s；
cache-off 第一项 43.816s，第二项 120.159s 停在 release_ready。
第二项报告已经 verified，cleanup 当时 pending；不能将该报告说成业务失败。
原 operation 默认 hints 恢复 30.749s 后 verified / cleanup complete，同连接断开
29.249s，重复断开 0.025s 且 journal 不变，受管 200 槽 pending 为零。
其余 cache-off、大结果、reload 保持本次 not_run，不拼接先前 smoke。

该项四个 memory finish 全 fresh，年龄 493/69/147/397ms，post 光学四次均通过。
第一 prepare 正样本最终 reliable not_sent，outcome 时年龄 587ms，但该时刻包含
前置操作完成时间，不能精确分摊每段耗时；submitted 的 outcome 年龄同样包含键消息
burst，超过 500ms 不能据此推断输入门禁失效。该原调用 release 尚无 input_intent。
七次身份全扫累计 43.590s，输入全扫九次 47.952s，其中五次未命中各约 7s。
光学 pre 为 accepted 9、unavailable 15、waiting 15。现有 published exchange 的
一次 2s 光学调度窗口只覆盖 waiting；unavailable 会立即进入身份发现。下一轮窄修
仅扩展同一窗口到 unavailable，交替原因不续期，不新增地址/样本缓存或输入授权。

## unavailable 共用调度窗口的开发复验

真实 Driver 的 unavailable → waiting → ready RED/GREEN 通过：原代码第一次 unavailable
进入发现并被 fixture 取消；修后约 0.55s 经三次观察、零发现、一次 journaled submitted
关闭。持续 unavailable / 混合状态到期仍发现、不发键；原因交替不续期，各限制条件
双状态验证。build/vet、channel 全包 112.780s、定向 race 三次 17.002s 均通过。
这仅修正调度，不修改任何 INPUT 或光学接受门槛。

私有 dirty CLI SHA256 为
`10854391feb9afa357bb81d66d30e0f17c72c676feb4fdea8b74cb8d699165f9`；
`.tmp/retail-live-input-signal-unavailable-grace-smoke-20260930/smoke.json` 仍 blocked。
cache-off 连接 37.180s，普通请求 118.277s 直接 verified / cleanup complete；
大结果 120.154s 停在 prepared，原调用未获得报告，不能算业务已执行。
原大结果 operation 用 hints 恢复 20.120s，完整文本断言与 verified / cleanup complete
通过；同连接正常断开 48.818s，重复断开 0.036s 且 journal 不变，200 槽 pending 为零。
普通请求仍有输入全扫九次 53.617s、身份全扫五次 35.272s；大结果分别为九次 57.809s、
七次 50.397s。大结果唯一 fresh memory finish 年龄 366ms，随后 post unavailable。
调度修复不足以完成整体验收，尚未作为已完成提交。

读取路径另识别待复现竞态：evidence 在取 mu 前读时钟，既有 reader 可在二者之间
接受更新帧；旧 now 可能把合法新帧误判 future。下一轮先锁住真实异步 reader RED，
仅调整时钟与锁的顺序，不放宽 frame/edge/after 条件；不能在实机证据前归因所有失败。

## 时钟与帧快照同锁的修复及实机边界

实际异步 reader 与 cache-off Source 的 post gate 稳定复现 RED（10/10）：锁外读时钟
后 reader 接受更晚的合法帧，旧 now 错判 future / unavailable。将同一次 QPC 读移到
tracker mu 内后重复 20 次 GREEN；clock 错误、真正未来/重放、501ms 过期、after 屏障
仍拒绝。build/vet、全 channel 116.681s、光学定向 race 三次 21.564s 通过。
没有增加生产读时钟、采集线程或 timer，也没有修改帧年龄/edge/输入接受条件。

私有 dirty CLI SHA256 为
`9053222ba577dce49a1f1081a93f32211c1fe6f502a509352082adcfbf748909`；
`.tmp/retail-live-input-signal-clock-order-smoke-20260930/smoke.json` 仍 blocked。
cache-off 连接 54.850s；普通请求 120.154s 停在 confirm_ready，原报告 unavailable。
两次 fresh memory finish 年龄 389/92ms，post 两次均 accepted；输入全扫九次 57.432s、
身份全扫六次 44.226s，刷新 fourteen attempts 均无局部命中，nearby 四次实际读取
4.60MB / 229 reads。该修复不等于已解决全部扫描未命中或实机超时。
原 operation 用 hints 恢复 12.090s 到 verified / cleanup complete；正常断开 16.155s，
重复断开 0.026s 且 journal 不变，200 槽 pending 为零。大结果本轮 not_run。

现有原生 hints 的同 runtime 相邻 sequence 对，14 对中 7 对位置在旧 1MiB 邻域外，
距离约 3MiB–4.46GiB；这些是已验证的历史提示位置，不是失败补查的实时发布链证据。
不可据此归因所有 afterEdgeHit=0 或猜定迁移方向。1s 采样周期保留，既有 1MiB 局部
窗口与总 1s / 8MiB / 4096 reads / 四 scope / 250ms 限制保留。
下一步先复现首遍已读取后发布位置移出邻域的真实 Source RED，再尝试单次 Observe
内的一次完整优先重扫。它使用原 Source/physical session/lookup/deadline，优先位置
只排序 fresh Regions 的完整工作队列，不排除剩余范围、不继承缺失结论或输入授权。
最坏额外一遍完整扫描会增加 I/O，必须实测且不能提高 256GiB 调用物理预算。

## 调用内优先完整重扫：新鲜命中改善，完整请求仍超时

Source RED/GREEN、独立范围完整遍历、严格种子/记录否定、跨 Observe 冷扫描、
物理额度/lookup/取消与首遍 gaps 保留测试通过；build/vet 通过，定向 channel
1.847s、三次 race 5.518s、memory 相关测试 0.670s。此时尚未跑新冻结全量基线。
重扫只排序本次 fresh Regions 全队列，代价全部记入原调用；没有持久地址提示。

私有 dirty CLI SHA256 为
`af97d763dd7462f46a2cc33a2ee2ab88d16ff08d9f359b882d1d72f31a9794e4`，
归档 SHA256 为 `e91b91a310fd75ba39640b91af1275dfe5dc6f29bffd99b2db4405a2d1b5b7e9`。
`.tmp/retail-live-input-priority-rescan-smoke-20260930/smoke.json` 保持 blocked：
cache-off 连接 39.564s，普通请求 120.120s 停在 release_ready，报告 verified、
cleanup pending。七次 memory finish 均 fresh；输入扫描累计 60.091s，身份全扫
五次累计 36.239s。新扫描中出现 65ms、565ms 的早期接受，但不能据此宣称稳定提速。
原 operation 用默认 hints 4.058s 恢复到 verified / cleanup complete，正常断开
28.911s，重复断开 0.024s 且 journal 不变；clean managed 200 槽 pending 为零。
大结果本轮 not_run，不能拼接先前 smoke。

下一轮只改昂贵 RuntimeCandidate 的完成后冷却：不足 1s 仍为 1s，其余耗时
乘三且封顶 30s；第一次发现立即可用，成功 RecoverBinding 的原间隔保持。
这是调用内调度，不保留身份/输入授权，不改变 120s、物理预算及任何新鲜度门禁。
实际换代发现延迟还包含下一次协议观察与发现本身，不能承诺严格最多 30s。

真实 Driver 先运行 RED：旧冷却令第二次昂贵发现耗尽 5.01s 调用，observations=3、
discoveries=2、inputs=0；修后 3.26s 完成准确 unbind 回执，observations=4、discovery=1，
仅一次 journaled submitted。持续缺失约 4.33s 后再次发现且零输入；取消、关闭显式
reload、原目标期限及上下界检查通过。另一个 RED 验证失败绑定恢复的 1.05s 成本
不得混入短 RuntimeCandidate 的倍率，修后该候选按自身约 25ms 成本仅冷却约 1s。
成功 RecoverBinding 原间隔仍保留，乘法在 10s 前封顶以防 duration 溢出。
build/vet 与定向现有恢复测试 13.243s、race 三次 41.138s、diff check 均通过。
本轮 SOURCE 全量冻结基线与 clean 完整 Retail runner 待运行，不能沿用 d149bf6 报告。

私有 dirty CLI SHA256 为
`2b1102a46f6dea97a6d1becc99093835f128fea59fa3ce2f9166623ea5aa21f8`，
归档 SHA256 为 `862d46fa5884f620d5c8a7976aaa336fcc8aa77ae735efedf7c73997772b6873`。
`.tmp/retail-live-input-discovery-scheduling-smoke-20260930/smoke.json` 仍 blocked：
cache-off 连接 24.715s，普通请求 100.332s 直接 verified / cleanup complete；
大结果 120.128s 停在 confirm_ready，报告当时 unavailable，不能称为已验证业务成功。
大结果八次 memory finish 均 fresh，但 post 光学仅三次 accepted、五次 unavailable；
INPUT 全扫十五次累计 73.637s，身份全扫三次累计 21.534s。普通请求 post 四次
accepted、一次 unavailable。身份发现成本下降仍不足以完成本轮请求；单轮数字不构成
旧新匹配性能比较。后续先区分帧过期、解码/历史丢失与采集路径延迟，不能猜定原因。
原大结果 operation 用 hints 13.231s 恢复到 verified / cleanup complete，完整文本
断言通过；正常断开 31.923s，重复断开 0.028s 且 journal 不变，200 槽 pending 为零。

本批冻结全量离线基线
`.tmp/baseline-offline-input-discovery-scheduling-20260930/report.json` passed，
2026-09-30 13:25:36–13:31:05 UTC；源码树 SHA256 为
`b607df3c3d75ab98d422124f302f131a8bc4bae340725d698f79e8d0145f9a54`。
SOURCE 稳定性、build 1.935s、vet 1.050s、强制 Lua 5.1 全量 Go 307.253s
（2785 个测试通过、35 个 package 通过）、98 个 Node 测试 17.188s、版本/skill/
生成命令合同均通过。仅随后追加此验证事实；实机 blocked 及恢复状态保持原记录。
下一轮仅增加有界 reader/tracker 诊断，再针对实测具体原因修复；本批不代表完整验收。
