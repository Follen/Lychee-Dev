# 同进程换代后的独立收尾

本轮修复已通过 Retail 12.1.0.69933 双实例原连接恢复。旧连接可以在同一个游戏进程
换角色或 reload 后关闭，不要求先完成新绑定，也不要求退出客户端。未发布。

## 规则

业务执行与资源退役共用原 Driver、journal 和锁，使用不同的证据。
新业务仍要求新 nonce 绑定。旧资源退役只要求证明旧 runtime 已结束；
原业务预算过期不禁止只读收尾，但不恢复业务预算、不重发未知输入、不改原角色。

`disconnect`、关闭中的 `resume` 和预算耗尽后的 `resume` 可做有界只读观察：

1. 核验原 PID、进程创建时间、窗口所有权及受管安装。descriptor/hints 只提供候选地址。
2. 在候选附近先完整读取最多 4 个内存窗口。两轮观察无证明（或没有可读快照）时，
   最多重新定位一次，并以剩余 4 个窗口额度追加快照；总计最多 8 MiB，失败和短读也扣预算。
   重定位只选择比原种子序号更新、位于原快照之外的合法记录，命中后及时止扫。
   序号和新地址只用于选址；重新找到的记录本身已经包含在新快照里，不能作为新写入证明。
3. 定位、追加快照及等待共享最多 15 秒的有界观察，不刷新截止时间或 14 轮上限。
   在已完整快照的同一地址范围内寻找随后实际出现的新 input 记录。
   完整字节必须不同于先前快照；校验 header、checksum、身份后，再次点读确认一致。
   旧堆记录、未变化的字节、缺失区域、截断、冲突身份均不构成证明。
4. 另一 runtime 的新写入形成 `runtimeEnd`。先持久化证据，再在发布锁下精确退役
   原 consumer/runtime/nonce；同时处理原 transaction 和 recovery transaction。
5. 保存关闭结果后释放原连接占用。中断后复用已持久化证据继续收尾，不要求再次在线。

证据保存精确地址、长度、前后摘要和解码样本，不将附近随机内存写入 journal。
不比较游戏时间与 Windows 时间，不按 runtime token 大小判定换代，不增加色块、
游戏端采样器、常驻服务或业务状态机。正常输入的新鲜度规则不由收尾证据替代。

该证明沿用可信单一 Lua producer 的协议边界：SlotRuntime 在同一 Lua 生命周期内只创建一次，
完整 input 封套由当前 sampler 生成，Lua 字符串非移动，历史完整封套不被复制到新地址。
它不是对任意注入代码或伪造协议记录的认证。摘要是本次可信读取的审计记录，
不是可由外部 JSON 自行提交的退役授权。

角色选择界面没有新 sampler；尚未取得证据时保持 pending，不能把画面、缺块或旧 descriptor
当作 runtime 结束证据。已保存的 `runtimeEnd` 不受随后登出影响。
未确认的业务始终为 execution_unknown/unavailable；已验证报告保留原字节和业务成功/失败。

正常断开不能被可选观察拖死：未取得证明、父命令仍有效时，单纯观察超时回到正常断开。
取消、身份或读取故障不当作普通超时忽略；已经持久化证明后只能继续退役，不能恢复旧输入。

关闭期限已耗尽而旧 runtime 仍在运行时，重复 resume/disconnect 不续期。
在任务授权包含 reload 且没有另一个未完成 reload/recovery 的条件下，显式新 reload 请求
使用自己的持久期限。Closing 路径只发送该次 reload，再以新写入证明退役旧 CON，
不绑定新 runtime、不借新期限发送旧 unbind、不重跑业务。相同请求只恢复或返回原完成结果。
发送前也可观察到旧 runtime 已由外部操作结束，直接完成销毁目标并关闭；
这不证明 CLI 发过 reload。原 Input 记录原样保留，不能把 intent 阶段等同于尚未发送。
这不是所有 pending 都自动 reload：已有 reload 输入不确定、未决 recovery、角色选择无新采样器
等条件仍保留原证据并等待，不能覆盖旧控制请求。旧运行环境随后真正结束时可只读收尾。

## 原现场验证

候选 CLI：`.tmp/runtime-retirement-20260929/lycheedev-bytes.exe`，SHA256
`49cc94de13fb8ae77feb41a2b59a82c7de680a131e71babaaf03273b944c75ac`。
游戏仍加载此前安装的 hybrid 候选；本轮恢复不要求先升级插件。

- A：PID 9480 / 创建时间 134351349917989638。原次年雪连接
  `CON-c5709ace7cc7f2e9b2f09977735ea7c7` 已过期且 slot 1 输入未知。
  新灵止光 runtime `0000005500001ffb27fa0bd8d57c1cc9` 的新写入取得证明。
  公开 `live resume` 约 9.44 秒返回 closed=true；仅追加两条退役事件，没有新 input 事件。
  原角色与原输入记录保留。池中仅 slot 1 新增 retiredRuntime，consumed 仍为 false。
  B journal SHA256 恢复前后均为
  `2c40ad60ae388fe29afab6cb57f44c26c65dd40db0947b521894486f44c09bc1`。
  重复 disconnect 成功，未重复发键或清理其他预约。
- B：PID 34876 / 创建时间 134351350176547581。原连接
  `CON-2da6f56586e7d960508cabc78d23312e` 在等待 A 期间也已换代。
  公开 disconnect 核验 runtime `0000001a00001ee6d72121e4baa63ef4` 的新写入后关闭，
  原 reload 输入记录保留，没有新的游戏输入。

结果文件：`resume-a-bytes.json`、`disconnect-a-repeat.json`、`disconnect-b.json`，
原始日志、槽池前值和只读原型均在 `.tmp/runtime-retirement-20260929/` 及原项目 journal。
最初未观察到新记录的一轮发生于角色选择界面，保留为环境未就绪证据，不当作协议通过。

## 回归与边界

定向回归包括：未知/已验证报告、双事务精确退役、中途失败、最终日志写入失败、
保存证据后禁止旧业务继续、重试幂等、PID 复用、损坏/不完整内存、旧字节、新地址写入、
跨窗口记录、矛盾 producer、取消以及失败读取的次数/字节上限。
首轮完整离线结果为 `.tmp/runtime-retirement-20260929/offline/report.json`；
追加关闭恢复修订须另行运行完整基准，不能继承首轮结论。

## 部署后的追加验收

受管安装已更新为含绑定有效性门禁的候选，两个原进程均完成激活并建立新 CON。
开发包主机 SHA256 为 `f5bde410c8f05549b6519d18e0f628111934a8f2a7aaa58fccc7166014f9174d`，
包和安装证据在 `.tmp/runtime-retirement-20260929/package/`、`install-same-volume.json`。
未覆盖复制文件，未修改玩家绑定，未发布或替换全局 CLI。

B 的公开基准前 18 步通过，包括 13 次正常执行、容量 reload、历史请求只读重试、
语法错误报告、清理失败自动 reload 和 bugs 读取。第 19 步正常断开暴露可选退役观察超时
阻断正常关闭，保留原失败报告 `post-update-b/baseline.json`，不改写为通过。
该失败发生于业务结果及清理均已完成之后；诊断期间原关闭期限耗尽，另行验证显式恢复。
A 和修订后的最终验证结果在本节后续记录，不由前一批离线或实机通过结果代替。

A 的 `post-update-a/baseline.json` 最终 20 步全部通过，包含容量换代、语法错误、
清理失败自动 reload、正常断开和重复断开。CLI 为 `lycheedev-close-fallback.exe`，
SHA256 `5f014365860770d7292fa5dff04d43884d372230ebb011fa02b8d85b557c65fb`。
正常断开约 32.0 秒，重复断开 56ms；不据此宣称总体延迟已优化完成。

B 的后续恢复又暴露前置观察调度问题：每次输入采样 pending 都重做退役扫描，
120 秒内未发键。修订将 intent 前置观察移到持久预算生效后、调度循环之前，
每次命令只执行一次；仍由原身份和新鲜度门禁授权输入。控制预算耗尽后也保留
独立只读退役入口，判断依据为本请求预算，不能被旧关闭 blocker 误导。

最终候选 `lycheedev-recovery-scheduled.exe` 的 SHA256 为
`85c03c2696a9aa00fc02977983650a99838df0dca269c8026bd86a5e815a87b7`。
B 续原 request 后只发送了一次 reload；随后原 PID 34876 退出，命令按身份变化停止，
没有据旧 PID 继续输入。公开 disconnect 以 `process_exited` 关闭旧 CON。
这证明进程退出收尾，不能记作该次同进程 reload 退役通过。
`b-recovery-audit.json` 核对原 CloseBudget、原 Identity、A journal 均未改变。
用户换到 PID 48276 / 荔枝小月亮 / 燃烧之刃后，新 CON 的连接、普通执行、正常断开、
重复断开全部通过；旧 CON 重复关闭亦只读。记录为 `final-smoke-b/report.json`。

最终完整离线基准 `offline-scheduled/report.json` 的 8 项检查全部通过：
Go 42 包 / 2484 测试事件，33 项环境或辅助进程检查跳过；Node 88 项、0 跳过。
SOURCE 摘要为 `ca3adb093f5520aa71ef1e156cfdff694c0137ae1fa4ff88e8f73896edc88ad6`。
一次并行 channel 单包运行的 350ms 自动容量收敛测试超时，单独连跑 3 次及最终完整基准通过；
未为通过测试调整业务预算。此后仅补记文档，生产源码未改。

同进程 Closing + explicit reload 的最终实机追加结果：
`CON-b5f39ea48b32c46f7f75e6aff862913f` 在 PID 48276 上先以 1 秒调用预算留下关闭请求，
随后 `close-control` 只发送一次 reload。首次 120 秒观察未取到证明，保留
`closing-reload-live/report.json` 的失败/待续状态；沿原 CON 公开 resume 后取得新 runtime
`0000000200002ce122182ba50afd1283` 的序号 201 新写入并关闭。
`closing-reload-live/resume-acceptance.json` 核验：原 Identity、CloseBudget 不变，
关闭后仅有一次 reload input intent，没有 bind/unbind/业务输入；重复原 reload 请求
返回完成且 journal 字节不变。最终受管安装 64 槽、pending=0、窗口 owner 文件=0。

首次证明等待并非 sampler 停止。只读原型发现序号从 155 增到 222，但记录地址相距
约 255MB；原 8 个观察区域连续读取成功仍没有新字节。重新定位后可恢复，
不能保证一次调用完成，也不能把固定区域未变化当作 runtime 未换代。
以后若优化选址，仍须保留总时间、字节预算和新写入证明，不能靠扩大盲区推断或重新发键。
本次不凭该单例贸然改变内存预算；原始只读证据为 `nearby-closing-probe.jsonl`、
`closing-discovery-after.jsonl` 和 `closing-hints-probe.jsonl`。

## 远地址重新定位优化

上段描述优化前的现场。随后已将固定区域等待改为上述一次有界重定位；不改变 Lua、
采样周期、输入协议、持久状态或证明格式。原窗口保留参与冲突检查，新窗口必须先完整
建立 before，再接受之后的字节变化。全进程重定位复用原扫描器的 64 GiB 规划上限和
8 worker 上限；它的读取量不包含在 8 MiB 的**快照**额度里，但共享原观察时间上限。
持续跳出所有观察窗口、未出现后续样本或剩余时间不足仍保留 pending，不保证每次完成。

确定性测试 `TestRuntimeReplacementRelocatesDistantSamples` 用两个相距 255 MiB 的区域
复现：修改前 14 轮仍无证明，修改后第 3 轮取得合法证明；原区域内更高序号的历史记录
不会抢占唯一重定位机会。独立测试覆盖重定位后的历史记录、未知身份、旧 runtime、坏校验和、
共享截止时间、取消及两批失败快照仍总计最多 8 次/8 MiB。

本次候选 CLI 为 `.tmp/runtime-relocation-20260929/lycheedev.exe`，SHA256
`42803a76267751bb15b06b46cec035750ba18e4513079c4b24c876e8728a71d1`。
Forever `1.60.1.70009` / PID 39828 公开 CLI 复验：连接约 12.0 秒，短关闭留下原 CON，
Closing reload 一次调用约 23.3 秒完成，重复请求约 32ms 且 journal 不变。
原 Identity、CloseBudget 保留；仅一次 reload 输入，无新绑定，最终槽位 pending=0。
CON 为 `CON-cae6afe66b07c38c150b7a329352c5c3`；结果见
`.tmp/runtime-relocation-20260929/live-report.json`。
该调用中的只读重定位扫描 `df54a7b3bab55007754942873d3a5c23/004.json`
约 189ms 提前命中（计划约 7.61 GB，实际扫描约 263 MB），并不代表整条调用只需 189ms，
也不能据单次游戏分配行为承诺固定延迟。正式服本修订实机性能与压力矩阵仍未重测。

最终完整离线基准 `.tmp/runtime-relocation-20260929/offline/report.json` 全部通过：
build/vet、强制 Lua 5.1 的 Go 42 包/2494 测试事件、Node 88 项、版本、Skill 契约与生成文档，
以及执行期间 SOURCE 不变。33 项可选环境/辅助入口跳过单列，不当作通过项；Node 无跳过。
冻结摘要 `6bec33d2fcfd3f519bbbf5f3ac20d6a06bbc717bacef2d378afb1e0968d65f13`，
随后只补记验收文档，未改生产代码。未发布、未替换全局 CLI。

本轮证明原现场的同进程换代收尾；不等于全部业务和显示矩阵通过，
也未切换共享槽位集合协议。Classic/Titan 此恢复分支实机验收仍为 not_run。

## 无限服指定 build 追加验证

用户授权后，在 Forever `1.60.1.70009` / Interface `16001`、PID `39828`、
创建时间 `134351381520737918` 上验证。角色 Aotu / Aotu，GUID `Player-4618-01188FA2`。
目录虽为 `_classic_beta_`，实际身份来自 `.flavor.info` 加 build catalog；该目录无 version.txt。
受管安装由旧 100ms 纯内存实现更新为与 Retail 相同的 hybrid 候选，备份在
`D:/Code/wow/addons/Lychee Dev/.tmp/runtime-retirement-forever-20260929-install-backup`。
主机仍为上述 SHA256 `85c03c…815a87b7`，未改生产源码或发布版本。

证据根目录 `.tmp/runtime-retirement-forever-20260929/`：

- 激活约 30.3 秒，确认 `lycheedev.input.hybrid.v1`；普通探针 verified / cleanup complete，
  正常断开约 25.1 秒，重复断开 39ms。
- 新连接 `CON-7cd18b563fe4a366bdb603ffdfa4910f` 使用 1 秒调用预算留下关闭请求，
  `forever-close-control` 一次 reload 后约 20.1 秒取得 runtime `0000000600002fd361433141714ae22d`
  序号 12 的新写入并关闭；这次不需要 resume，不能据此否定 Retail 上的选址超时。
- 日志中关闭后只有一次 reload input intent，无 bind/unbind/业务输入；
  原 Identity 和 CloseBudget 不变。重复原 reload 请求 34ms，journal 字节不变。
- `report.json`、`cleanup.json` 确认本次通过、安装仍为 managed、64 槽 pending=0、窗口 owner=0。

这是 Forever 指定 build 的实验性恢复测试，不改变正式客户端验收矩阵；
也不是全量功能、DPI/HDR 或共享槽位并发验收。
