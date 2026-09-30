# 常驻 1s 输入采样与主机优化实施记录

日期：2026-09-29。当前工作树候选，未发布。本文记录实现与离线验证，组合候选实机状态为
`in_progress`，尚未通过完整验收；下列分阶段真实结果不继承此前 14 组双实例验收。
[实施前调研](input-cadence-research-2026-09-29.md)保留原始模型和当时状态。

## 当前实现

游戏端保留 `lycheedev.input.v1` 常驻采样器，周期从 100ms 改为 1s。Start 立即采样；
已知 wake/close 释放输入后立即 Refresh，不重置周期计时。显式关闭桥移除 OnUpdate。
没有接入按需短窗口、后台 daemon 或第二套恢复状态机。OnUpdate 仍每帧调用；减少的是
周期状态读取、编码和字符串分配次数，不能宣称游戏总 CPU 下降 90%。低帧率会延长实际间隔。

主机保留 500ms 新鲜性、输入后 `after+100ms`、runtime/owner/fence/角色/build/槽位匹配及
发送前原地址复核。完整校验通过但样本过期时，同一目标可等待固定 2s；后续旧样本不会
续期。新鲜样本成功后结束本次等待，超时或无有效等待依据时恢复完整发现路径。
这是本次命令内的调度状态，resume 仍受原持久预算约束，不重开执行预算。

实测后的主机修订允许有 hints 的完整 Find 在发现年龄不超过 1500ms、结构和身份完全合法的过期
样本时提前结束扫描，仅作为继续观察的调度线索。返回后再次验证 1500ms 边界；500ms
输入授权门槛不变，过期样本不会变为可发送的 InputObservation。每个 Native 保存
runtime/sampleMillis 高水位，同 runtime 的重复旧样本不能反复遮住新记录；过旧或
错误身份样本不能据此阻止恢复。该线索不续期固定 2s 附近等待，也不刷新持久预算。

已知 runtime 的小记录可先重读 hints 并搜索附近地址，适应 Lua 新建不可变字符串。
单次附近查找共享最多 8MiB 读取量、4096 次 Read、250ms 软时间预算，包含点读与扫描；
不用于未知 runtime 首次证明或 BODY 授权。局部覆盖始终是 partial，未命中不等于不存在。
新字符串落在范围外可能等待后转为全扫。`--no-cache` 仍走完整扫描，不能承诺相同延迟。
2026-09-30 的 cache-off 补查修订见[输入合同](live-input-architecture-2026-09-28.md)：
cache-off 完整遍历不被旧样本停止；只在本次扫描结束后用本次合法近期旧样本位置做一次
有界 INPUT 补查，地址不跨调用。原扫描 gaps 与局部覆盖保留，所有输入授权条件不变。

Hints 按 runtime 字节序降序优先排列，作为较新 runtime 的调度偏好，不能当作身份或
跨进程唯一性的证明。同 runtime 的 Input 按 Sequence 降序最多保留 8 个地址，总数仍为
64。有效的更高 Input Sequence 可越过本次扫描前 64 条学习限额：仅对缓冲内小记录完整
验证后学习，不扩大输入授权，也不从旧地址直接证明新 runtime。此修订针对实测中旧
runtime 小记录占满 hints 的问题；旧失败证据保留，不追溯改写为通过。

前三项内部优化同时保留：

1. 已知地址点读无需先枚举所有区域；仍验证进程和记录，未知 runtime 不靠旧地址证明连续性。
2. 扫描先筛完整头、接受有效结果后及时停止，学习可复用地址；保留最终记录验证及部分覆盖事实。
3. Find 不同步写扫描 JSON/hints。每个 Native 最多 256 次带证据观察，诊断有界缓冲至 Close；
   可丢 hints 合并写入，权威 input_intent/journal 的持久化和发送前检查保持不变。

## 诊断收尾与失败含义

Close 释放底层资源后使用独立 10s **软截止**刷诊断，不是整个命令的硬 10s 上限，
也不是 256 条各自获得 10s。系统 I/O 不保证按截止立即返回。activation 先完成权威预算
记账，再释放 window driver，最后刷诊断；普通 Connect/drive 也先释放 driver。
收尾各阶段即使失败仍继续执行，并合并错误。

写诊断失败返回可识别的 `live.channel_trace_flush`，命令 exit 5，优先于同时存在的
pending/cancel。已经验证的业务结果仍保留，不能因此重跑业务。未写诊断留在当前对象中
可再次 Close；进程退出/崩溃则可能丢失尚未落盘的扫描缓冲，这不改变已持久化动作事实。
Hints 是可丢缓存，不是权威证据。观察次数上限不因 flush 重置。

## 验证与未验边界

离线定向检查已通过：

- `input_cadence_windows_test.go` 九组及原 post-lookup 定向检查：采样间隙局部等待、
  新地址发现、旧样本不续期、错误身份/未来时间/矛盾状态拒绝、缓存关闭、输入后新样本、
  取消，以及近期过期样本的调度边界与重复样本高水位。
- `native_diagnostics_windows_test.go`：Find 不受诊断磁盘阻塞影响、收尾写失败可见、
  共享截止与未写证据保留、取消后独立收尾、预算与 driver 先于诊断完成。
- `live_channel_error_test.go`：trace 失败与 pending/cancel 任意合并顺序均为 exit 5，
  普通完成/pending/cancel 分类不变，错误原因不丢失。
- memory 的 7 组 nearby 与 8 组原优化定向检查通过，覆盖附近发现、读取预算、
  密集候选点读、共享截止、未知 runtime/BODY 拒绝及局部未命中语义。
- 本批 `go build ./...`、`go vet ./...` 及上述 channel/command 定向测试通过。
- 缓存排序与后续高 Sequence 学习新增 3 组 memory 回归通过。
- Lua 5.1：`$env:LYCHEEDEV_REQUIRE_LUA51='1'` 后运行
  `go test ./tests/addon -run 'TestWorkbenchLuaSuites/t_(input_state|slot_runtime).lua' -count=1`
  通过（2 suites × 4 profiles = 8 子测试）；随后 `go test ./tests/addon -count=1` 通过，
  23 suites × 4 profiles = 92 Lua 子测试，另 TOC/locale Go 检查通过，耗时 7.611s。
  profiles 为 retail/classic/titan/forever，Forever 仅模拟测试，不算客户端验收。
  解释器为 `C:\Program Files (x86)\Lua\5.1\lua.exe`，runner 核验为 Lua 5.1。

这不是完整发布门禁。最终源码的强制 Lua 5.1 全量 Go 测试、正式 offline baseline 和新版
多实例矩阵尚未运行完成；缓存关闭、低帧率与其他客户端仍未验，不能用定向通过代替。

## 实机阶段结果：失败与恢复均保留

证据目录为 `.tmp/one-second-20260929/`，以下为分阶段二进制，不能合并成最终候选全通过：

- B 首轮激活已 reload，但绑定 published 等待约 120s 后 pending：`activate-b-run.json`。
  扫描批次 `4424a77aacdeb0f6675f455520d9fdda`。观察到旧 runtime 小记录占用 hints；
  缓存排序修订后，`activate-b-fixed-resume.json` 在原 CON 恢复成功，没有换 request 重跑。
- 首个 `cadence-b.json` 探针因测试脚本 `callback_limit` 失败；修正测试后重新运行。
  `cadence-v2-b.json` 为 verified、complete、cleanup complete、`report.ok=true`，
  gaps 为 `[900,1000,1000,1000,1000]` ms，`inputReleased=true`。该命令约 89s，
  不能把游戏端约 1s 的样本间隔解释为整个命令低延迟。
- A 首次激活 `activate-a-run.json` 仍 pending，随后使用 `lycheedev-recent` 的
  `activate-a-recent-resume.json` 在原 CON 恢复成功。随后 `cadence-recent-a.json`
  的传输 complete、reportState verified、cleanup complete，但 `report.ok=false`，
  错误为 `unexpected periodic interval: 534`。原因未完成诊断，不能将它计作采样通过。
- `cadence-close-a.json` 与 `cadence-close-b.json` 均记录 `closed=true`；两条原 CON 已收尾。
  B 通过与 A 失败分别保留，不以连接成功关闭代替业务断言通过。

修订后仍观察到约 9s 全扫描。当前结论是部分真实路径已成功、总体仍 `in_progress` /
`not_passed`，不能宣称根因完全解决、双实例矩阵完成或延迟稳定。
三色块与后续优化的目标和验收另见[新方案](live-hybrid-architecture-2026-09-29.md)，尚未实现或部署。

性能仍可能波动：局部搜索失败会全扫；1s 周期与 500ms 门槛之间存在正常等待；
输入后的新采样等待、磁盘收尾、主机负载及 256 观察上限都可能先于业务完成影响命令。
此前正常命令从数秒到数十秒的差异不能仅归因于离线测试负载。本批只能声称减少了明确的
关键路径冗余工作，不能在复测前声称延迟稳定或给出实机提速比例。
