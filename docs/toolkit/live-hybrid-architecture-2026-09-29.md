# Live 三色块、内存优化与代码清理方案

2026-09-29；审计工作树 `codex/live-channel-recovery`。
状态：本页保留设计与静态审计决策；用户批准后已开始实施，当前源码及验证见[实施记录](live-hybrid-implementation-2026-09-29.md)。
下文“待验证”是设计门槛，不应从源码落地推断全部通过；现有验收事实仍以 implementation-status / regression 为准。
三个子 Agent 分别审查主机调用链、槽位与观测边界、Lua 生命周期及 Skill；主 Agent 核对源码和证据。

## 1. 选定的结构

保留一个 CLI、一个 Driver、现有内存协议和 64 个 LoD 槽位。
色块负责持续观察按键条件，内存负责接收者身份、槽位、回执和结果。
这次更换观测来源并收拢生命周期，不再增加一套恢复状态机。

```text
Skill：选择固定目标、提交意图、根据 continuation 继续原请求
                         │
Driver.Continue：唯一推进者、固定预算、先记动作再执行
                         │
Native：三色块观测 ── 内存身份/回执 ── 槽位发布 ── 单次物理输入
                         │
Lua：一个采样器 ── 三块显示；现有 SlotRuntime/SlotProtocol 执行业务
```

必须保留：准确 CON/request/nonce、未知输入不重放、prepare/commit、进程生命周期校验、
每实例 owner、共享安装发布锁与预约、已验证报告和清理义务分开、持久截止时间。
本次不引入 daemon、额外消息队列、Agent 恢复脚本或按颜色手动发键的入口。

### 相比当前代码的实际收益

| 当前问题 | 本次处理 | 不夸大的边界 |
| --- | --- | --- |
| 为等待焦点/战斗解除反复寻找新内存字符串 | 等待按键条件改读固定位置三块 | 真正发键前仍需内存身份核验，不承诺零扫描 |
| 输入身份、就绪状态、样本时间混在 InputObservation | 内部区分内存控制证据和光学就绪证据，外部仍是一个输入接口 | 不从主机期待值伪造观测字段 |
| 扫描枚举、重复筛选、证据磁盘写占关键路径 | 保留前三项通用优化 | 实机延迟必须重新量化 |
| 旧灯、新灯、采样器分别收尾 | 一个生命周期负责 Start/Refresh/Stop，停止不能复活旧状态 | 开启期间 1s 采样常驻，这是用户明确选择 |
| 中断草案和旧功能混杂 | 依据实际调用分类清理 | 正在使用的旧功能不借“死代码”名义删除 |

## 2. 屏幕契约：固定三块

```text
游戏客户区左上角：[红/绿/蓝/黑/白] [红/绿/蓝/黑/白] [白色明暗心跳]
                       第1块            第2块            第3块
```

每个数据块都可以显示红、绿、蓝、黑、白；黑白是该块的可选颜色，不是独立色块。
两个五色数据块有 5×5=25 种颜色组合，另有一个心跳块。原稿将它拆为五块和 8 位信道是误读，已撤回。
位置相对于各自游戏客户区，不能使用整张显示器的左上角，也不能误把窗口标题栏作为原点。
首个实现目标为每块 2×2 物理像素、连续排列，共 6×2 物理像素；无标签或大型背景。
这个尺寸是待验证目标，DPI、缩放、渲染比例与 HDR 未通过前不宣称可靠。
坐标由同一布局契约转换，Go 不另猜 Lua 的位置；WGC 窗口内容与客户区的偏移必须实测核对。

使用自己的非保护 frame，三个纹理复用，不接收鼠标或键盘，不挂 Blizzard 对象或抢焦点。
采用可用的最高非保护层级及明确 frame level；无法保证盖过系统画面或所有第三方遮罩。
任何遮挡、缺块、模糊边界、非法色值都关闭输入资格，不能沿用之前的绿灯。
ActivityView 为信号留出条带，不能靠新信号遮住原有内容。

四种业务状态保持简单：可按键、普通键盘焦点占用、战斗、未知。
不在世界内或桥已关闭时隐藏全部三块；无信号是传输不可用，不新增第五种业务状态。
未知不能当作 ready；只有普通焦点占用允许 Driver 决定单次 Esc，战斗/未知不允许。

两个数据块仅编码四种按键状态；第三块表示采样活跃性，不承担业务传输。
从 25 种组合中选择合法状态码并保留无效组合，用于拒绝误读；具体颜色映射和检错能力待原型验证。
撤销原稿的 8 位分配、两位相位和汉明码建议，不把 RGB 当作三条独立二值通道。
Go/Lua 共用一份小型颜色码表 fixture，匹配不确定或组合非法时拒绝，不纠错猜测。
颜色阈值与几何经原型后冻结；码字和心跳不是 runtime、时间戳或认证。

## 3. 一个采样器，一个生命周期

沿用 1s 周期，启动、已知输入释放和相关状态变化时即时 Refresh；Refresh 不推迟下个周期。
心跳和四态在同一次无 yield 的 Lua 回调提交；不另开一个“假活跃”心跳定时器。
一次成功提交翻转心跳。API 不可用可以提交 unknown；回调异常不能保留可发送旧状态。
高频事件合并处理，周期不补发积压 tick；UI 更新不分帧写三块。

InputState 作为唯一状态生产者，InputSignal 只是显示器。初期保留必要内存控制证据的生产，
不能删除它之后又由另一个模块重新采样身份。模块内管理事件、周期回调和纹理，不向 Driver 暴露细节。

| 场景 | 行为 |
| --- | --- |
| 从未启用 | 不创建信号 frame、事件、timer 或 OnUpdate；既有批准 bootstrap 例外不扩张 |
| 开启且已在世界内 | 核验世界状态后立即采样，再显示；不能先亮旧 ready |
| CLI 退出、取消或崩溃 | 游戏侧继续 1s 采样；不用猜 CLI 是否还活着 |
| 加载画面、离开世界、角色身份不可用 | 隐藏并清空本次可用状态；世界内恢复后重新采样 |
| bridge off | 统一 Stop：移除周期/事件，隐藏信号，停止 StartupBeacon，释放输入保护 |
| Stop 后迟到回调/Refresh | 无效；不能重新显示或注册工作 |
| UIParent 隐藏后恢复 | 跟随隐藏，重新采样后才显示 |

世界状态和跨客户端差异集中在 Compat；不能把 IsLoggedIn 等同于进入世界。
Retail 的 IsPlayerInWorld 声明已有 pinned source 证据；Classic/Titan 的可用性及晚启用行为
仍需按 pinned baseline 核实，不凭一个 Retail API 推断全矩阵。
若保留 OnUpdate 节流，回调仍每帧发生，只是采样每秒一次；不得声称所有工作只每秒执行。

## 4. 输入证据：省等待扫描，但不删安全门禁

当前 `input.go:160` 同时核验 runtime、owner/fence、NextSlot、GUID/build 和样本时间。
`input_windows.go` 在发键前重读对应地址。这些不全是“按键四态”。
如果把整份记录换成绿灯，会同时删掉身份保护，这是本次审计否决的捷径。

最小实施路径如下：

1. 在固定目标 WGC 上连续等三块；光学阻塞期间不反复扫描内存就绪记录。
   输入阻塞不妨碍读取业务回执、处理关闭、进程退出或既有有界恢复；不能把颜色门禁套在整个 Driver 上。
2. 确实准备执行一个物理动作时，在输入锁外取得内存控制证据，优先点读、必要时发现。
   暂沿用现有带时间记录及其身份/槽位/500ms 规则，不以长期保留的 Identity 描述符替代。
3. 记录动作意图，取得必要短锁，复核固定进程和准确发布内容。
4. 临近发键重读内存控制证据并校验最新光学证据；任一失败均返回可靠零发送。
   过渡期旧内存记录中的 blocked 仍可作为否决，不得在两者矛盾时选更宽松的一方。
5. 只发本次一个动作；用既有 nonce/challenge 回执证明接受，光学变化不算 ACK。

内存发现、WGC 初始化、等待新帧和停止 capture 不在目标窗口输入锁或安装发布锁内进行。
临近输入仅做有界点读和现成帧校验；失败由唯一 Driver 继续，不在 Native 内嵌重试循环。
扫描耗时导致证据过期时仍不发键，不能放宽授权来获得更好性能数字。

### 帧新鲜不等于样本新鲜

WGC SystemRelativeTime 是合成器渲染帧的 QPC 时间，不是 Lua 采样时间。
这是 [Microsoft API 的定义](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.direct3d11captureframe.systemrelativetime?view=winrt-28000)。
CapturedFrame.ObservedAt 也只是主机观察时间。二值心跳会重复，漏帧可能混叠。
因此不得用它们填造 SampleMillis，或声称仅凭三块已证明原有 `after+100ms` 条件。

光学证据独立记录帧 QPC、主机观测时间、码字和观察到的心跳边沿。
首次连接、capture 重建、窗口/DPI变化、缺块或失活后，先清空历史资格，看到有效变化后才重新使用。
帧年龄初始候选上限 500ms、观察到边沿的活跃窗口 1500ms；这两个参数只限制光学证据，
不是 Lua 样本年龄证明，必须通过低 FPS/丢帧/暂停测试再冻结。
锁前取得的帧若仍符合本动作证据条件可以复用；不强制每次锁内等一张新帧，避免低 FPS 永远追不上。
动作后要求新的光学变化作为额外屏障；原有样本 after 限制仍由内存证据证明。
恢复日志只保留决策摘要，CLI 重启后重新建立光学资格，不能把已落盘绿灯当当前事实。

这一步确实消除按键阻塞等待中的高频内存查询；不保证消除每次动作前的扫描。
1s 周期与 500ms 样本有效期仍有正常等待区间。若身份核验仍主导延迟，应单列测量结果，
先验证控制证据的更有效读取方式，再决定删除旧路径；不能拿本方案声称该性能问题已解决。

正常已接入 runtime 的 ESC 和聊天 `/reload` 没有游戏槽位 nonce 保护，所以它们也必须走上述双重核验。
尚无 runtime 的首次激活不能提供这类证据，严格保留 §5 的独立例外，不把它扩展到正常连接。
即便保留核验，用户/系统在观察与发键之间改变界面仍有竞态；不承诺绝对不会关闭刚出现的界面。
invoke 在游戏侧重新验证目标，但错误/迟到 Wake 仍可能耗用一个 LoD 槽，不能声称“拒绝就零影响”。

## 5. 内存优化保留范围

| 保留项 | 当前位置 | 不变的正确性要求 |
| --- | --- | --- |
| 已知 runtime 地址直接读，免先枚举全部区域 | memory/hints.go | Verify → ReadRecord → Verify；失效地址仅为缓存未命中 |
| 已有 chunk 预筛、完整验证小记录后学习地址、接受后停止扫描 | memory/records.go、scan.go | 有界数量/字节，不缓存 BODY 授权；停止未扫与读失败分开 |
| 扫描诊断与可丢 hints 延后写 | channel/native_windows.go、diagnostics.go | 权威 input_intent/journal 仍先持久化；诊断失败可见，已验证报告保留 |

未知 runtime、首次发现、cache-off 仍按其真实发现路径处理；不得用旧地址证明新 runtime。
每个 Native 诊断最多 256 条，其一次 Close 收尾共用 10s 软预算，沿用当前实现事实；
多次 Close 可能累计，系统调用可能超出软截止，不能表述为整个命令的 10s 上限。

FindNearby、固定 2s 等待、1500ms 近期样本、高水位、Input 专用 top-8 hints 优先级，
现在仍有生产消费者。三块接上不代表可以全部删除，因为动作前还需要新鲜内存控制证据。
先把调用限于有实际动作需求的观测，量化其必要性；只有消费者被完整替代后才成组删除。
不能把“计划移除”写成已证明死代码，也不把附近扫描变成所有内存业务的新通用机制。

新 runtime 用明确能力标识选择混合输入路径。旧 `lycheedev.input.v1` 可在升级兼容范围内走旧适配，
Driver 和结果合同相同；色块缺失不能自动降级盲发或每次在两种实现间试探。
activation / reload fallback 的能力分派必须同步更新，避免把新能力误判为无遥测旧 runtime。
首装尚无运行时的既有固定激活流程保留原适用条件与记账限制，不扩展到正常连接失败。

## 6. 多 build、同 build 多实例与共享槽位

build 是协议/数据能力信息，不是实例锁键。每个 Native/capture/hints 会话绑定
PID、进程创建时间、HWND 与已核验可执行文件；角色、runtime、owner/fence 另有协议校验。
同 build 的 A/B 分别读各自 WGC 与内存，绝不因颜色相同互相借用观测。
各 build 不散落分支判断，差异进入 Compat / 能力声明 / 生成 catalog。

| 资源 | 隔离范围 |
| --- | --- |
| capture、输入资格、进程地址 hints、逻辑 owner | 固定进程生命周期；HWND 变化重新核验 |
| 目标窗口输入 | 沿用按 PID/创建时间/HWND 命名的短互斥；不持锁等待帧、战斗或全扫描 |
| 槽文件与预约 | 规范化受管安装目录，不能用 build 或 CON 区分物理文件 |
| journal、request、历史结果 | 原项目/CON，恢复沿用原请求，不另开连接逃避未知结果 |

当前 input_lock_windows.go:14 的锁是每窗口跨 CLI 进程共享，不是整个桌面一把全局锁。
本方案保留当前定向窗口输入，不无故增加全桌面串行；用户实际按键和系统输入状态仍是外部并发条件。

三块不解决共享 slot 1 的未知预约闭环。[现有离线实验](slot-concurrency-feasibility-2026-09-29.md)
已复现它；[复审](live-architecture-review-2026-09-29.md)要求有界多封套与每实例配额的独立原型。
这一工作继续列入总体方案，不用“已经有锁”代替解决活性问题。

推荐保持 64 个物理 LoD 文件，同槽容纳有界不可变封套集合，游戏只选唯一精确 runtime 匹配项；
主机按固定实例生命周期分配业务额度与恢复预留。A 连续 reload 不能重置额度或用掉 B 的份额。
发布在短锁内原子合并，按准确成员校验/退役；迟到回执不能删除新成员。
未知成员不按 TTL 擦除。集合格式必须显式版本化，旧 loader 不得混用新安装。

这是待过门槛的协议候选，尚不是生产能力。实施前必须冻结支持实例数、每实例未决成员数、
恢复次数和总字节，证明双实例容量及可恢复性。路由碰撞/歧义拒绝，Lua 不能假装知道 Windows PID。
无匹配仍耗槽、迟到键消费后续槽、合并/删除崩溃、单实例反复故障与共享磁盘故障都要覆盖。
原型不通过就重审槽位协议，不能继续往主机叠补偿状态。

## 7. live 分支代码审计与清理清单

以下为当前工作树静态调用审计，非全仓形式化不可达证明。行号会随实施变化。

| 分类 | 证据 | 处理与删除前提 |
| --- | --- | --- |
| 确认可清：中断的 64B 草案 | addon/Bridge/InputSignal.lua:20、InputSignalProtocol.lua:15；internal/bridge/input_color.go:11；protocol/input-color/golden.json。TOC:45–46 未加载，未接生产调用 | 成组撤销旧协议及其专用 fixture，按三块契约重建；不发布旧方案 |
| 确认可清：错误几何草案 | desktop/frames.go:33 TopCenterCapture 及 frame_clock_test.go 的顶中测试 | 去掉顶中 sentinel/分支和专用断言；客户区左上使用最小明确 ROI |
| 未接线但可能复用 | CaptureFramesWithStartupTimeout、CaptureSystemTicks | 仅在新 capture 生命周期/帧年龄实际消费时保留；现有 CapturedFrame.SystemTicks 已有 receiver 消费，不能连带删除 |
| 条件删除：旧内存输入辅助 | channel/input_sample_wait.go、input_windows.go、memory/nearby.go 与 Input 专用 hints 策略 | 动作前身份检查仍可能需要；无消费者且替代验收完成后删除专用代码/测试 |
| 条件删除：Windows 旧分派 | entry.go:179 的 native 路由截获 connect/fallback，旧 :279、:334 在 Windows 不达；other stub 返回 false | 先明确非 Windows unsupported 路径，再删遮蔽分支和仅服务它的包装 |
| 当前 manifest 下被截断的 Lua 输入入口 | Core/Runtime.lua:46–47、Core/Controls.lua:82–95 优先 SlotRuntime | 清理前核对旧模块的 UI/测试调用，不整文件盲删 |
| 活跃公开功能，不是死代码 | command_contract.go:84、88、184、194–198、225–228；entry.go 的 run/ack/finish/reset/probe 分派 | 本次不以清死代码撤销功能；若后续停用创建入口，要同步 contract/help/Skill 并保留旧操作恢复 |
| 活跃 UI/历史依赖 | Modules/AutomationView.lua:207、223 收旧报告，:312–326 Execute 调 ProbeQueue/ProbeRunner；TOC:31–39 加载显示/报告模块 | ReportStore、ReceiptView、QR 不能整块删；先明确迁移 UI 功能和历史读取 |
| 活跃恢复和历史 | wait.go:79 RecoverBinding；request.go:26 / reload_windows.go:70 historicalState；driver.go checkpoint；budget_driver.go 旧日志预算迁移 | 保留；不是重复调度器。不得破坏旧 CON/activation v1/v2 与历史链读取 |
| 研究/测试代码 | tests/slotset；PrepareOperation 被 tests/channel-live/main.go:503、tests/protocol/slot_driver_test.go:191 使用 | 标为实验或迁移调用后清包装，不谎称生产死代码 |

额外发现一个生命周期缺口：Core/Controls.lua:86–91 的 native bridge off 停 InputState 和 ActivityView，
却未 Stop StartupBeacon。后者由 Runtime.lua:49 Arm，可有启动期最长 45s 的 timer/event。
应在统一 Stop 中修正，并测试在启动 45s 内关闭。只藏新三块并不能满足关闭后无持续工作的要求。
StartupBeacon 目前不是死代码：旧 reload 提示仍有调用；新路径完成迁移后才能决定仅保留兼容还是退役。

清理的完成标准是调用链、TOC/生成清单、公开 contract、构建平台和测试同时一致；
零文本引用只是线索。历史设计和验收证据保留为历史，不当死代码删除。

## 8. Skill 编排

沿用 skill-creator 的原则：Skill 负责选择和意图，CLI 承担可执行保证。
不新增第二个恢复技能，不在 Skill 写采样频率、颜色解码、槽位分配或 Esc 重试算法。

- 单目标只有一个实际执行 Agent；其他子 Agent 可审计/读证据。同安装双实例按各自 CON 并行，发布冲突由 CLI 处理。
- 依据 continuation 区分继续原请求、等具体条件、预算耗尽或需要缺失信息，不能无限 resume。
- active driver 等待交接；颜色消失不切 PID，不新开 CON，不手改预约。
- verified 报告与 report.ok、cleanup 分别检查；未知执行不重跑，关闭成功不等于业务成功。
- 更新 live-investigation.md:118、122–123 的“纯内存输入、无需色块”旧描述，说明 CLI 封装混合观测。
  “No QR to scan”仍可保留，但 Agent 不自行看颜色发键。
- commands.md 仍由 command_contract 生成；不在实现前宣传新 flag/命令。

实现后以原始 pending/unknown/cleanup/双实例冲突结果做独立行为评估，不告诉评估 Agent 预期答案。
验证它能保持同一请求、等待真实变化、停止无效重试；格式检查不能代替编排行为测试。

## 9. 实施顺序与验收门槛

| 批次 | 工作 | 完成门槛 |
| --- | --- | --- |
| A | 清未接线 64B/顶中草案，统一 Stop，验证最小三块显示与码表 | 关闭零持续工作；缺块/坏色失败关闭；2×2 物理块、DPI/HDR 实机证据 |
| B | Native 接 WGC；拆分光学与内存证据；更新 activation 能力分派 | 正常 runtime 输入保留身份/时间门禁；首装例外范围不扩张；无锁内扫描或等帧；捕获取消能释放 |
| C | 保留前三项通用内存优化，测量等待与动作扫描，逐项删除被替代辅助 | 光学阻塞等待中的就绪内存扫描为零；正常动作端到端无明显回退；不能只报告扫描微基准 |
| D | 独立验证共享槽集合与配额，再迁移生产协议 | 同槽未知 A 不阻塞已接纳 B 的声明容量；崩溃恢复、耗槽、满容量与旧 loader 兼容过门槛 |
| E | 清遮蔽分派及失去调用的旧入口，同步 Skill/生成文档 | 不删除活跃功能或旧未知操作的恢复路径；调用/构建/TOC/contract 一致 |
| F | 最终组合候选全量验收 | 同一源码、同一候选二进制；离线与实机分别记录 |

每个源码批次按仓库要求运行 build、vet 与受影响包测试；最终强制 Lua 5.1 的
`go test -count=1 ./...`、`node tools/baseline.mjs` 及生成契约检查通过后才呈现为完成。
协议原型可与光学只读原型并行；生产切换分批，不能用不同候选的通过拼成最终通过。

必须覆盖的故障：旧 ready 定格但新 WGC 帧持续、心跳停止/漏边沿/重复混叠、短时遮挡、
UIParent 隐藏、resize、DPI/HDR、低 FPS/后台 1 FPS、最小化、capture 取消/初始化超时、
runtime reload、relogin、进程退出/PID 复用、错误角色、旧内存记录、日志写失败、未知按键结果。
低 FPS 不承诺可用性不降；可以明确 pending，不能忙等烧 CPU 或降低门禁硬发。

实机矩阵：Retail 双实例同安装同 build 为必测；另测独立安装同/异 build，Classic 50504、
Titan 38002；Forever 16001 单列实验。首装、升级、reload、relogin、bridge off/on 都必须单列。
采集每请求扫描次数/读取字节/Regions 次数、输入锁占用、WGC CPU/内存、诊断耗时、
等待及总耗时 p50/p95。对照同场景、同设置、明确样本数；尚无数据时不写提速倍数。
小 ROI 减少复制/解码量，但 WGC frame pool 仍基于窗口，不能声称捕获成本只有十几个像素。

## 10. 当前证据与未完成项

此前 Retail 14 组验收属于早期候选；不能继承到三块或最新 1s 组合。
`.tmp/one-second-20260929/cadence-v2-b.json` 的 report.ok=true，但命令约 89s。
`cadence-recent-a.json` 已返回 verified/cleanup complete，report.ok=false，报 534ms 周期间隔；
这是尚未诊断完的采样断言失败，不能称双实例采样通过。
`cadence-close-a.json` 与 `cadence-close-b.json` 均记录 closed=true，原两连接已收尾。

本次没有新增实现测试或实机测试。三块几何/码表/光学时间参数、控制身份读取性能、
共享集合协议容量及跨客户端行为均待上述门槛验证。计划保留现有安全检查，先减少确定冗余，
再凭证据删旧路径；不会用删校验、扩大超时或伪造新鲜度来声称“架构已经健壮”。
