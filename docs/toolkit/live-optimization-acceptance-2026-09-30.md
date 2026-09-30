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
