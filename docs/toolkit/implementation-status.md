# Toolkit 2.0 实施状态

日期：2026-09-25。2.0.1 已发布；当前版本源为 `release/version.json`，候选版本为 2.0.2。
发行条件及本候选的 Windows/npm 合同见 [2.0.2 发布合同](release-2.0.2.md)；
2.0.1 的后验恢复记录保留在 [2.0.1 发布合同](release-2.0.1.md)。
本页新增命令状态针对下一主版本工作分支；
已发布的 2.0.1 标签和 npm 包不含这些未发布改动。2.0.2 在未完成静态门禁、Windows CI、发行组装和发布前回读前，不得宣称可发布。WGC 原生崩溃根因（进程级 MTA 引用提前释放导致 GraphicsCapture.dll 卸载后执行）已于 2026-09-24 定位并以 60 轮真实窗口回归验证修复，2026-09-25 全日真机操作无复发；细节见下文 2026-09-24/25 条目。

## 当前状态

- 2026-09-25 Forever（16001）非插件侧真实网络验收：数据侧 `target resolve
  --product forever` 无 region/locale 时按合同拒绝（unsupported_data_language，
  永恒服无默认语言）；显式 `--region cn --locale zhCN` 后真实请求暴补丁服务
  `cn.version.battlenet.com.cn` 返回 404——`wow_forever` 不是暴雪产品，
  数据身份在真实世界不可解析，fail-fast 正确。`data hotfix --source wago
  --product forever` 真实到达 Wago 并取得分页数据，但被分页完整性守卫拒绝
  （`records.hotfix_page_drift`：记录 id 跨页重复）——远端数据质量问题被
  fail-closed 拦截，未静默返回。源码侧完整通过：`source sync --product
  forever` 真实拉取 Gethe/wow-ui-source forever 分支（固定
  `PIN-7fdd5a88…`，commit `bd2470ae`）；`source index` 4412 文档、71981
  声明、255143 关系、0 解析诊断；`source query/inspect` 命中并返回完整
  出处三件套（发现该镜像含现代 generated API 文档，证实永恒服为混合
  内核）；`source validate` 以真实 1.60.1 源码校验 `Lychee
  Dev_Forever.toc` 闭包 44 文档 staticValid=true，引用解析 strict 模式
  4056 调用 4040 未解析（样本为 Lua 内建/引擎全局，无权威声明属设计内），
  binding/combat/taint 明确 notChecked。结论维持合同："保留未验收"——
  源码研究对 forever 实际可用，数据侧真实世界无暴雪身份、Wago 数据
  质量存疑，插件侧真机仍未验证。未提交变更仅本轮文档；此前工作已于
  `900ef2e`/`238c912` 提交。

- 2026-09-25 死锁恢复命令 `live reset` 与 Lychee 插件真机测试：回执收起后
  的实际使用暴露一个协议缺口——abandon 只清理磁盘队列条目，运行中插件的
  内存队列按角色 fail-closed 拒绝身份触发（聊天可见 `Lychee Dev:
  identity_busy`），而一切输入都需要在屏回执，形成死锁；昨晚交接中
  "身份回执超时"的机制与此同源。修复为 bootstrap 例外的第三条固定命令
  `/dev bridge reset <nonce>`：仅允许发往磁盘无占用者的窗口，回执必须
  携带同一 nonce 且 actor 齐备（新增 `bridge.DiscoverReset` 与
  `parseResetSignal`，reset 类与会话无关、免 sequence 新鲜度但强制 nonce
  关联）；插件侧 `ProbeQueue.Reset` 只把当前角色的未确认条目置为已确认
  墓碑并丢弃格式有效的残留重入票据，不删除报告、不触碰其他角色条目。
  合同 79 命令；四客户端 Lua `TestReceiptResetCommand`、Go
  `TestReset{UnblocksBusyQueueAndConnects,RefusesOwnedWindow,
  StaysPendingWithoutReceipt,RejectsForeignReceiptNonce}` 及白名单回归；
  design 例外条款修订（2026-09-25 扩展）、regression 新增 RUN-20。发行根
  与 hide 版同根重建部署（备份 `.tmp/retail-upgrade-reset-20260925`）。
  真机自证：烟测探针 load→run（sum=55 verified）→abandon 制造死锁→
  `live connect` 被拒（candidate_missing）→`live reset` 一次触发解锁并
  重连（reset=true，`SESSION-4f82353e…`），WGC 捕获 reset 后 ready 回执
  （epoch 36）。Computer Use 按交接复核仍不可用（native pipe），未用于
  本次恢复。Lychee 插件真机测试用 live 探针执行：健康探针
  （19 Provider 全部 enabled、成就目录 6616 条 schema 4、全部内部组件
  在位、namedFrames=0 且无 OnUpdate——零成本禁用合同实测达标；
  `I.Builtin` 已随架构重构移除，旧 Audio 探针面失效；报告正文与捕获
  已归档，其 ack 因回执被用户关闭未能观察，经 `live abandon` 释放，
  cleanup=abandoned 如实记录）；搜索探针经 StaticIndex 真实查询：
  "宏伟宝库"/"炉石"/"坐骑"命中对应 Provider，"剧毒冲击"（近期历史可
  解析但索引为空）与 garbage 查询为空、诊断计数 FUZZY_CANDIDATE_LIMIT=2
  ——属 Provider 索引范围问题待源码核对，未定为缺陷；探针身份段曾用
  已移除的 GetAddOnMetadata，改 C_AddOns 后正常（0.3.38 荔枝启动器）。
  两个探针 operation 均 ack 收尾、`live hide` 清屏验证 symbols=[]。
  未提交、未发布。

- 2026-09-25 回执显式收起（`live hide`，用户反馈驱动）：显示的回执默认常驻
  （跨进程晚到观察依赖它），但操作终了后无人再读的卡片会一直占据游戏画面。
  新增协议命令 `/dev bridge hide`（`ReceiptView.Dismiss`）：请求作用域操作
  在飞时 fail-closed 返回 `receipt_busy`，成功清屏不打印、不产生新回执、
  幂等；新增 CLI `live hide --session`：经固定 bootstrap 重连并验证 readiness
  后恰好发送该命令，占用窗口直接拒绝（`live.receipt_window_busy`/exit 3），
  有效帧连续零符号才报告 cleared，残留回执返回
  `live.receipt_hide_pending`/exit 6，唤醒等待 15 s 有界。合同表增至 78 命令，
  skill commands.md 再生成，live-investigation 补充"读完即收起"编排
  （隐藏是整洁性要求，不替代读取与归档）。回归：四客户端 Lua
  `TestReceiptHideCommand`（解析/忙拒/幂等/无新回执）与 Go
  `TestHideReceipt{ClearsDisplayedReceipt,RefusesOwnedWindow,PendingWhenStillVisible,
  ReadinessPending,MatchesSessionIdentity}`；design 补显示策略段，
  regression 新增 RUN-19。发行根
  `.tmp/release-2.0.2-receipt-hide-20260925/npm-stage`（dirty 本地组装）经
  `addon install --output` 升级事务部署，回执 ReceiptView/Controls 哈希与
  工作树一致，备份 `.tmp/retail-upgrade-hide-20260925`；`live reload`
  （cleanup=complete，`CAP-41933cfe…`）激活。真机闭环：bugs
  `OP-37311b59…` verified → 独立 ack cleaned/completed → hide 前 WGC 捕获
  acknowledged+ready 两卡在屏 → `live hide` cleared=true → 隐藏后捕获
  symbols=[]、无新聊天输出。全量 Lua5.1 必需测试与门禁通过。
  未提交、未发布。

- 2026-09-25 身份回执复核与 faults ACK 修复：游戏 08:42 重启（新 PID 41860、
  HWND 122624736），原授权角色灵止光—死亡之翼在线，磁盘仍为 2.0.2 managed
  （commit 28c0a99）。用当前工作树构建的 CLI 一次限定 connect 成功
  （`SESSION-e8ab82cfc6c01905d67da934d1966c5d6551cf97e3392904dee4d531946733eb`，
  capture `CAP-da83a101…`），身份识别、WGC 解码、会话绑定全链路正常；
  昨日 20:04 的 identity_unreadable/超时在今日栈未复现，其根因仍无定位证据
  （原进程内状态已随重启销毁），不宣称已修复。WGC MTA 崩溃修复整日真机
  操作中未再出现原生异常。独立 reload `OP-b8b389dd…`（cleanup=complete，
  nonce 关联新运行态回执 `CAP-f1aa64b5…`）首次真机验证输入恢复版运行态激活；
  probe load `OP-c5e4d9b5…` 正确停在 loaded，run 首次即 verified
  （sum=55、iterations=10，正文 SHA-256 `18879cbb…`），ack 完成
  cleaned/completed、无额外 reload——完整原子链首次真机通过。
  随后 bugs 任务 `OP-f24f3d91…` verified 后独立进程 `live ack` 失败：
  `bridge.signal_not_observed`、messagesQueued=0。离线红绿复现定位根因：
  faults 清理重载后，新进程的 SignalReader 无任何观测，`sendAck` 首输入
  guard 的 `RequireFreshSignal` 直接拒绝，而旧代码在输入前从不观测 readiness
  （probe 路径有对称的 `prepareAcknowledgement` 等待）；`ack_requested`
  恢复另有 `observeAcknowledgement` 无 15 秒界的无界等待缺陷（真机表现为
  持续解码空转，修复前一次 resume 进程被终止）。修复：`sendAck` 输入前
  锚定归档重入锚点观测当前 inputReady 回执，超时映射
  `live.ack_readiness_pending`；落盘回执证明零键盘消息入队（无任何
  PostMessage 成功）时 `ack_requested` 恢复安全重发，阶段保持
  ack_requested、不经过 verified 门，否则只观察且 15 秒有界。
  新增 `TestFaultsAckFreshProcessObservesReadinessBeforeInput`、
  `TestFaultsAckResendsAfterUnsentIntent` 红→绿回归（后者先以无界等待
  复现错误结局），既有 faults 生命周期测试夹具补充回执持续显示的帧。
  design 的 ACK 恢复不重发条款补充零消息回执例外，regression 新增 RUN-18。
  真机验证：原 ID `live resume` 完成 `OP-f24f3d91…`
  （cleaned/completed/cleanup=complete）；下一任务 bugs `OP-b056a5eb…`
  在已释放窗口 verified，独立 `live ack` 一次通过（cleaned/completed）——
  同时验证窗口释放与修复后的 faults 工作流。本轮无 addon 变更、未提交、未发布。

- 2026-09-24 深夜 WGC 原生崩溃修复（本轮补记，证据在 `.tmp`）：CDB 捕获
  CLI 子进程 `0xc0000005`，执行地址落在 `<Unloaded_GraphicsCapture.dll>+0x1115c`
  （执行非可执行地址），只调试 CLI、未附加游戏；`.tmp/capture-check/main.go`
  只读最小复现（开 WGC、读一帧、关闭、等 500 ms 循环，不发输入）在第 8 轮
  崩溃过一次，证实竞态。根因为进程内最后一个捕获流关闭后 MTA 引用归零、
  GraphicsCapture.dll 卸载，而原生 worker 尚未返回。修复：
  `internal/desktop/capture_windows.go` 以 `sync.OnceValue`+`CoIncrementMTAUsage`
  懒初始化并进程生命周期保留一个 MTA 引用，`frames_windows.go` 在身份与
  ROI 校验后、启动捕获前调用；每流仍释放线程、事件委托、纹理、设备、
  会话并配对 RoInitialize/RoUninitialize。红绿：修复前回归第 0 轮即报
  `capture runtime unloaded between sessions`；仅保留 MTA 的对照实验与
  正式修复各通过真实窗口 60 轮。新增
  `internal/desktop/lifetime_fixture_windows_test.go`（需
  `LYCHEEDEV_TEST_DESKTOP=1`）。修复版二进制 `.tmp/lycheedev-native-fixed.exe`
  构建 2026-09-24 19:59。build/vet 与 Lua5.1 全量当时已通过。

- 2026-09-24 19:32 +08:00 后续恢复：新增原子命令 `live abandon`，仅允许
  已归档验证且尚未提交 ACK 的 probe；先持久化 abandoning 意图，再精确移除
  原磁盘队列条目，最后释放窗口占用。保留报告，不发送输入、不改 SavedVariables，
  返回 cleanup=abandoned、complete=false，不伪称 ACK 成功。覆盖执行租约冲突、
  意图/队列阶段中断恢复、其他条目保留、损坏归档及已提交 ACK 拒绝。
  用户明确授权后，真实旧任务 `OP-6aa7e30a25a525bdfbd9f5b1790f2236` 已
  abandon，原 sum=55 报告与 capture 保留；此结论替代下文历史记录的“仍占用”。
  新版后台输入/重入修复已从正规发行根
  `.tmp/release-2.0.2-input-recovery-20260924/npm-stage` 部署，旧安装备份于
  `.tmp/retail-before-input-recovery-20260924`。这是 dirty 本地测试组装，非发布包。
  部署后仅对灵止光—死亡之翼（PID 31732）进行一次限定 connect，子进程以
  `0xc0000005` 原生访问异常退出，没有 JSON/session；输入提交进度未知，未重发。
  WGC `.tmp/retail-after-connect-crash-20260924.png` 确认该角色仍在线且无 QR。
  新磁盘版本尚无 reload/运行态验证，完整 load/run/ACK 链仍未通过。
  只读三轮共 90 帧捕获解码与独立窗口输入、WGC 二维码各 10 次均通过，
  不能据此排除连接路径的原生崩溃；未找到对应 Application 崩溃事件或本地 dump。
  新增独立窗口并发 WGC 捕获/逐字 bootstrap 测试，核验完整输入与二维码正文，
  交互桌面模式运行 10 次通过（30.239 s），GOGC=1 高频回收下再跑 10 次通过
  （30.521 s）；仅覆盖原生传输组合，不是游戏接入证明，仍未复现该原生崩溃。
  abandon 改动已通过 build、vet、Lua 5.1 必需的全量 Go 测试
  （live 157.642 s、protocol 15.050 s）、77 命令/171 引用 skill 合同、
  version --check 和 skill-creator quick_validate；skill 已编排显式放弃与恢复边界。
  补充并发原生测试后再次通过 build、vet 与 Lua 5.1 必需的全量测试
  （live 148.642 s、protocol 12.542 s）；diff --check 无错误。
  未提交、未发布；原生崩溃根因未定位，真机验收不得标为通过。

- 2026-09-24 后台输入/重入修复：对照 Git v1.2.0，恢复打开聊天后
  150 ms、逐 UTF-16 单元 50 ms 的发送节奏；保留新版逐消息身份检查、
  多客户端绑定、窗口独占和不确定效果不重放。重入等待进入世界与加载结束
  两个事件，支持任意顺序；聊天焦点释放后重建 readiness，加载/离开世界
  不复活旧回执。报告归档保留输入与捕获诊断材料。回归先复现 10 ms
  节奏偏差和加载结束前提前 ready，再修复；这些缺陷不证明原真机事故的
  唯一根因。19:04:27 +08:00 WGC 只读观察同一 PID 31732 / HWND 38342164，
  `.tmp/retail-input-fix-observe-20260924.png` 仍无可解码符号，decodeError=nil。
  本轮尚未部署新代码或发送游戏输入，原任务 ACK 与后续任务仍未通过实测。
  本轮修复后离线门禁已通过：`go build ./...`、`go vet ./...`、
  `LYCHEEDEV_REQUIRE_LUA51=1 go test -count=1 ./...`（live 143.718 s、
  protocol 13.218 s）、version --check、76 命令 skill 合同及 skill-creator
  quick_validate。skill 补充缺失回执不能推断战斗或 reload 失败；未提交、未发布。

- 2026-09-24 Retail 定向实测只操作灵止光—死亡之翼（PID 31732，
  Build 12.1.0.69933 / interface 120100），另一个在线角色未接收本轮输入。
  自动连接 2.0.2、独立 reload 成功：`OP-4021e959ea66c8b7cbf44d956da31ca3`，
  新运行态回执 `CAP-878823b6cf956bc416cac8efd9d7b89cb4006b38edc8f8b6d980711b90cc4970`。
  修复完成 reload 被误报 complete=false，以及下一项动作误读重载专用 ready。
  有界求和探针返回 55，但操作 `OP-361f7eade0aa4907f0c565ff0b4f9aea`
  暴露 load 越过运行/确认停止点，不能算原子链通过。已补真实状态机回归并修复。
  修复后新 load 请求在接入前超时，未创建 operation；WGC 确认仍为目标角色，
  聊天显示 identity_busy。Lua 回归复现 ACK 后内存队列仍占用身份识别，
  并覆盖共享队列其他角色隔离；修复保留精确 ACK、禁止重载已确认源码。
  此 Lua 修复随后已通过正规发行根部署（安装回执 2.0.2，ProbeQueue.lua
  SHA-256 `a826ab1c03868286d6814c8220c98447b0779010a74556532c88b358e43b4938`），
  旧安装备份于 `.tmp/retail-before-ack-fix-20260924`。用户明确改为操作同窗口
  的晴昼秋岚—白银之手后，重新绑定并完成独立 reload：
  `OP-7d7dd3fc435c092fbc27668ec9b05963`，complete=true、cleanup=complete，
  新运行态证据 `CAP-aefbb65111cf1ef5dab163dcddbe50b05d06c6ed33fb12135158f058636b02ff`。
  后续真机原子链操作 `OP-6aa7e30a25a525bdfbd9f5b1790f2236`：load 正确停在
  loaded、report unavailable；run 到 flush_requested 后等待超时，一次原 ID resume
  仍超时（当时误报 command.cancelled / context deadline exceeded）。
  WGC 截图 `.tmp/retail-qingzhou-atomic-flush-20260924.png` 显示目标角色在线、
  无可解码二维码；聊天中保留 reported 文本，但这不替代完整报告或重载证据。
  后续回归复现并修复 CLI 错误地把报告归档绑在瞬时重入 QR 上：持久正文
  已精确匹配时，原子操作可在原执行租约下离线恢复报告，无需先看到 QR。
  同一真机 operation 的 `live resume` 已成功返回 report.state=verified，
  正文为 completed / iterations=10 / sum=55；cleanup=pending、complete=false。
  正文 capture `CAP-8394736ced93e90e45039276271e7c307e9ed7c957fe1912ebb0e137626857e8`，
  回执 capture `CAP-8dd435ef2d1f25041b0f5bdd6abeb55d269d019ffbac449581d102c71a3eab70`，
  正文 SHA-256 `18879cbb9010f54c0d507b2513cbbefb0d7bc1b3d379ba5c53ee0b5403c247af`。
  ACK 有界尝试未取得有效 readiness，未发送 ACK 输入；修复后 CLI 返回
  live.ack_readiness_pending（子程序 exit 6，go run 外壳 exit 1），保留
  verified 正文、retryable=true 和原 resumeOperationId，不再误报取消。
  WGC 只读捕获 `.tmp/retail-report-recovery-20260924.png`（17:58:59 +08:00）
  显示同一角色在线，但未解码出 QR。真实瞬时 QR 丢失/未重入的原因仍未证明，
  完整报告恢复不证明该 reload 成功；保留原操作与窗口所有权，不重放执行、
  不强行清理。ACK 收尾仍 blocked，ACK 后下一任务 not_run，完整原子链未通过。
  新增离线恢复、损坏正文、writer 冲突、幂等、错误 ACK readiness 及 ACK
  意图前后崩溃的 fixture 回归；它们不替代真实 ACK 收尾证据。
  2026-09-24 18:09 +08:00 再次真机复测：原 PID 31732 / HWND 38342164
  仍在线，WGC 捕获 `.tmp/retail-retest-20260924-180909.png` 显示晴昼秋岚，
  解码结果为空（decodeError=nil）。原 operation 的 status 仍返回 verified
  报告与 sum=55；一次有界 ACK 再次返回 live.ack_readiness_pending / exit 6，
  保留原恢复 ID、cleanup=pending、complete=false。未提交 ACK 输入、未重跑
  探针、未强行释放窗口；ACK 后下一任务仍 not_run，本次未通过完整实机链路。
  本轮最终离线校验通过：go build ./...、go vet ./...、启用 Lua 5.1 的
  go test -count=1 ./...、version --check、76 命令 skill 合同及
  skill-creator quick_validate；未提交、未发布，未覆盖个人目录旧 skill。

- 2026-09-24 Classic 接入尝试：正式安装后磁盘状态为 managed；身份识别
  返回 unreadable/超时，尚未建立 session，未执行 reload、probe 或 ACK。
  带正斜杠路径连接时空候选暴露了路径比较缺陷。本次统一发现、选择和
  session 约束的路径规范化，并修订首次加载 skill 编排。完整方案及待实机
  验收项见 [首次加载与连接恢复](live-startup-recovery.md)。
  超时原因仍未验证，不能写成“已确认插件未加载”或“死锁已修复”。

- 游戏内工作台八类能力的源码和四份 TOC 已进入当前 2.0.2 工作树。用户确认已完成
  工作台真机手测；本仓库没有逐项 WKB 记录、实际游戏 Build、截图和检查者签名，
  因而只记录“用户确认”，不把缺失细节补写成自动测试通过。
- CLI `live instances` 自动发送一次性身份标记，`live connect` 自动完成
  候选选择、游戏内 `/dev connect` opt-in 和新鲜 ready 核验。用户不手输
  `/dev connect`；真实候选歧义仍需用户选择。`live run` 在 verified 报告处
  停止，`live ack` 精确回收该 operation 的报告、队列项和窗口所有权，且不
  额外触发 cleanup reload；需要重载时单独使用 `live reload`，重载造成的
  readiness 变化由 CLI 自动重新连接。
- 2.0.2 候选只发行 Windows amd64。Retail `120100`、Classic `50504`、
  Titan `38002` 为支持客户端；Forever `16001` 保留代码/TOC 但
  未获真机验收。CI 保留托管 Windows 上的 Go、Lua、进程和安装包验证，
  不再要求 self-hosted 交互桌面或 desktop-evidence 门禁。
- wowdoc/wowdata 已建立逐业务的离线 parity 台账：`tests/parity/coverage.json`
  共 55 个 case（wowdoc 19、wowdata 36）。当前统计为 `passed=25`、
  `fixture-backed=6`、`fixture-backed-partial=8`、`intentional-change=16`、
  `not_run=0`。这里的 `passed` 只表示当前实现对该 case 的断言有自动化证据，
  不表示旧可执行程序被重跑，也不表示真实 CDN、所有 Build、完整 Hotfix
  服务覆盖或所有客户端的真实数据都已验收。
- `v2.0.1` 指向 `baa83e9d7ec80091dce5de68c4295bb233139613`；
  标签 CI、Windows 原生组装、安装 smoke、封存和 release-gate 均通过。
  npm Trusted Publishing 成功，`latest` 为 2.0.1，registry integrity 与
  封存 tgz 一致，provenance 和隔离安装回读已验证。GitHub Release 及六个
  封存附件已人工补齐并逐项核对。标签工作流因发布后即时回读延迟和
  Release 步骤缺少 `GH_TOKEN` 留有失败记录；不能称整条自动流程全绿。
  主分支已修复这两个工作流问题，已发布的标签和 tgz 不变。

## 历史实施记录（以下为当时快照，不代表当前缺项或发布状态）

此前来源、样本摘要及逐轮测试记录另见
[历史实施记录](implementation-history.md)。以下内容保留用于追溯早期决策，
其中 `2.0.0-dev`、五平台、四端实机、未迁移工作台与手动 connect 的描述
均是历史快照，以本页“当前状态”和 2.0.2 合同为准。

## 架构与接口

- `actions` 已撤销。源码用例归 `codebase`，数据/资源归 `records`，安装归
  `delivery`，目标与存储归 `selection` / `vault` / `evidence`。CLI 直接调用用例。
- 游戏会话、执行、报告及恢复集中在 `live`；原 `jobs` 移入 `live/journal`，
  是游戏操作记录，不再作为所有命令共用的任务框架。
- `live probe put` 保存不可变代码修订；`live probe load` 从保存连接读取目标及截图
  区域，只加载修订并返回 operationId；`live run <operation-id>` 只执行已加载操作到
  verified；`live ack <operation-id>` 精确确认并回收且不再触发清理 reload。
  `live reload` 只做 nonce 相关独立重载，`live bugs` 只做 1..100 条既有错误快照。
  这些动作不能覆盖 PID、安装、角色、
  snapshot 或区域。首次连接按 2026-09-23 用户修订改为自动发现/选择/连接：
  `live instances` 发现安装与在线候选并逐窗口做一次性身份识别，`live connect`
  自动完成 opt-in 与 ready 核验并保存会话（唯一匹配自动选中，真实歧义交用户
  选择）；`live bind` 保留为只观察已显示回执的手工路径。宿主仍不自动登录游戏；
  `/dev bridge identify <nonce>` 与 `/dev connect` 是仅有的两条 bootstrap 输入，
  均须新的 nonce 相关回执核验后才成立。nonce 不再是 CLI 参数。
- 运行时自动定位唯一匹配当前角色/服务器的账号目录，固定进操作请求；缺失或
  歧义才需显式账号，恢复不重新扫描。发现只读目录元数据，不继承旧 SavedVariables。
- `live resume <operation-id>` 从原记录取得全部身份及区域。完成态只整理所有权，
  不输入游戏；原子探针已持久化的报告可在无在线窗口时精确核验并归档，
  不释放所有权。其他游戏阶段仍核验新证据，不重发已记录输入。
- run / resume / status 返回任务结果视图，不暴露内部 observation。
  `report.state=verified` 带完整正文和归档引用；`cleanup=pending` 不抹去有效报告。
  `complete` 仅在报告已验证且收尾完成时成立。这是读取视图，不是第二套持久状态。
- 运行记录不再重复保存 bootstrap/load/cleanup requested、queue prepared 与
  retirement requested/completed 六个布尔标记。输入前提交的证据引用及队列内容
  摘要是单一依据；缺少发送回执不授权重发。
- 使用 skill-creator 修订统一 skill：调查、证据解释和恢复决策留在 skill，机械
  协议留在 CLI；命令参考由 `tools/skill-commands.mjs` 从 describe 生成。
- 命令准入、help 和 describe 共用一份命令定义。子命令 help 只返回自身参数；
  不存在的命令即使带 help 也返回参数错误。运行 skill 不再列出未实现命令的调用示例。
- 本地数据目标直接由 `target resolve --installation <client> --region <region>
  --locale <locale>` 准备，内部读取产品、Build 和已验证的 CASC 配置，固定 DBD
  commit；无需手写配置键。`--from` 不可变地扩展已有源码目标。数据和资源读取接受
  同一个客户端目录，也接受 CASC 根目录；安装、运行与数据共用客户端身份读取。

新原子链已实现不可变探针修订、稳定请求幂等、加载/执行/确认分离、独立 reload、
内置 bugs 和异步探针有界 API。本轮 Go/Lua 全量静态回归、Go build/vet、Node
工具测试、skill 合同、skill quick_validate、开发包隔离安装、实际组装 tgz 的
隔离安装和 sealed digest 校验均已通过；仍需 Windows required CI、干净提交的
正式发行门禁及真机验收后才能发布。
阶段/执行状态继续分别保留协议进度与未决信息。命名目标配置已在当前工作分支接线，但尚无发布版验收；
不能把目录迁移、命令接线或读取视图当作这些工作的替代。

## 能力覆盖

`go run ./cmd/lycheedev describe --format json` 是实际命令目录。

### 工作台迁移范围确认（2026-09-23）

原插件工作台是 2.0 的发布阻断范围，已补入 design.md §12、regression.md WKB-01..13
与 capability-inventory。必须迁入并逐项验收的八类能力：运行（编辑执行/结果文本与树/
有界历史）、对象（检查/浏览/拾取/搜索）、事件（目录/筛选/有界监听）、追踪（函数追踪
启停/记录）、诊断（错误采集筛选详情/有界快照）、导出（落盘/记录/复制交互）、自动化
（队列/执行/历史查看）、关于（版本/双语/导航）。裸 `/dev` 打开工作台，connect 等
子命令共存，本地交互不得要求先连接；游戏内工作台与 CLI 共用同一游戏侧能力实现；
`LycheeToolkitDB` 不导入旧数据。早期源码盘点曾显示新 `addon/` 仅有桥接模块；该
盘点是历史快照，不代表当前实现。当前源码已包含八类能力，但 WKB-01..13 逐项真机
验收及前后截图仍未完成；界面可打开不等于功能通过。

| 范围 | 已有实现 | 主要缺项 |
| --- | --- | --- |
| 工作空间/目标 | 新格式初始化、命名目标（list/add/show/resolve/remove）、本地及远程目标准备、精确固定引用、项目锁与默认选择、对象存储及缓存维护 | 55-case parity 中仍有 fixture-backed/partial 边界；逐目标历史远程 Build 来源仍受发布清单限制 |
| 源码 | 同步、索引、topic/tier 查询、查看、比较、TOC/XML 静态验证及矩阵验证 | case-level 离线 parity 已建立；完整历史源码树、真实远程来源和多客户端运行语义仍需单独验收 |
| 数据/资源 | 本地/CDN CASC、DBD/WDC、DB2/schema/search/foreign-key/stream、只读 SQL、多领域查询、Hotfix（Wago/DBCache/Raidbots）、资产 search/inspect/export/demux 与 BLP2 转 PNG/无损 WebP | CDN 冷索引定位优化及完整真实样本覆盖；Hotfix 来源/筛选不等于完整服务器覆盖 |
| 游戏运行 | Windows 输入/捕获、自动发现/连接、不可变探针注册、原子 load/run/ack、独立 reload、内置 bugs、同步及有界异步探针、恢复、prepared cancel、报告读取；工作台八类能力已迁入源码 | 发布候选的真机故障恢复与 WKB-01..13 逐项、逐支持客户端证据；运行中取消仍只承诺协议可证明的安全范围 |
| 安装 | 清单验证、安装、归档升级、恢复和移除 | 新旧产品切换体验、正式载荷验收 |
| 证据 | capture 查询、校验、ZIP bundle、retention ledger、显式 CAS remove | 真机无关；仍需随全量回归确认发布候选证据 |
| CI/npm | Windows amd64 CI、发行与隔离安装验证（2.0.1 已发布，2.0.2 候选待门禁） | 后续版本门禁与许可/发布验收以 2.0.2 合同和新证据为准；本页不表示当前分支可发布 |

当前分支的命令目录以 `internal/command/command_contract.go` 为准。命令存在
不代表相应真实数据覆盖或发布验收已经完成。证据生命周期已在 Go 模块、CLI
dispatch 和离线回归中接通：keep 写入 retention ledger；remove 进行引用检查、
generation CAS 和 shared-blob 保护。仍待完成的是 parity 台账中明确标出的
fixture-backed/partial 真实来源补强、原子 live 链与 `live cancel` 已实现范围的
真机及故障验证，以及 Retail/Classic/Titan 各自的 WKB 逐项证据。不得据此状态
宣称本分支已具备发布条件。

新链路不调用旧 wowdoc、wowdata 或 Python。旧 `add-on/`、`packages/cli/` 和旧
skill 已于 2.0 发布准备中退役（2026-09-23，检查点 `6b08e14` 之后），历史版本
保留在 git；根 README/README_zhCN/AGENTS.md 已改述 2.0 产品与三客户端矩阵。

## 2026-09-22 增量验证

### BLP2 图像导出

- `asset export --encoding png|webp` 使用原始导出的项目选择、CASC 读取、证据
  和原子发布链路；没有额外下载器或游戏状态。默认仍为 raw，扩展名不改变编码。
  `--mipmap` 选择已有层级，`--channels` 实际转换通道；单通道为不透明灰度。
- 解码支持 palette 0/1/4/8 alpha、BC1/BC2/BC3、BGRA8。所有声明跨度在分配前
  检查，所选层级要求精确长度；保留空洞层级，不用零填充掩盖截断。
  默认像素预算 16 Mi，最大 64 Mi；WebP 单边不超过 16384。编码输出也受
  `--max-bytes` 限制，但不代表整个进程的内存上限。nativewebp 1.3.0 编码时
  内部缓冲且不支持中途 context 取消；前后与发布前检查取消，不宣称硬截止时间。
- PNG 使用标准库，WebP 使用纯 Go 无损编码器；新增许可通知随开发 npm 载荷
  打包。combined-work 许可审查、完整上游通知与对应源码交付仍是发布门槛。
- Luna 的独立手算解码测试通过；主 agent 添加编码后独立解码、透明 RGB、通道、
  输出限额与取消测试。真实样本首次失败：BLP2 头部 byte 11 为 `0x11`，被误当
  布尔值拒绝。156 字节最小复现连续两次失败，去掉这一错误限制后测试和真实命令
  通过；跨度与长度校验不变。10 秒有界 fuzz 9,528,770 次执行通过，非格式穷尽证明。
- 真实 Retail `12.1.0.69875` / zhCN，项目沿用下文固定 PIN；只读本地 CASC
  FDID `134400`，BLP 3916 字节，SHA-256
  `4fe1c9232ac7a5ad2ced3be9789384ebe13a906813f7df9a660322b5009e0a02`。
  输出目录 `C:/Users/follen/AppData/Local/Temp/lycheedev-texture-a4a238050f394186bf356406e011fead`。
  PNG 2274 字节，SHA-256 `bbbd9eee0ecc936e1fab80b2f9cd0718b31896f1385df46f5e5bc6d93fdbadb7`；
  WebP 2248 字节，SHA-256 `bc88016813d528f93a3587684d16c9bdcf86973eba3427478024b510a35e4421`。
  独立 PNG/WebP 解码逐像素相同：64×64 BC1，RGBA 摘要
  `f6d8f582f17c8c6c780dcfc91a1ee46a40cebcee3865ec6063463812cea7b630`。
  可设置 `LYCHEEDEV_IMAGE_SAMPLE` 指向该目录，运行
  `go test ./internal/records -run TestImageExportRealSample -v` 只读复核；默认跳过，
  仓库不携带游戏图片。另导出 mip 2 的 alpha 灰度图，得到 16×16 PNG。
- PNG 清单 `CAP-bb623dca9f176beeda48154d9fe1ed9ce47e2088ae1ab119bea3e4e69d38ce6c`、
  WebP 清单 `CAP-181cee3fd744deba3bc1b6b07bbe5efdafd167fa3869e82a339cfe6af6ccc26f`、
  alpha/mip 清单 `CAP-e607d1d9d1809cb1f06aab143ea1a3d0d06b140abe536152a8df5614408fdd5e`
  及各自原始/产物 capture 均通过 `evidence verify`。这不是游戏输入验证，
  也不是全部四端纹理样本覆盖。skill-creator 指导更新编码选择、通道语义和证据区别；
  quick_validate、`go vet ./...`、启动器/版本工具 9 项测试通过。
- 图像实现后的全仓 `LYCHEEDEV_REQUIRE_LUA51=1 go test -count=1 ./...` 通过：
  live 104.962 秒、records 17.583 秒、CLI 13.975 秒、跨进程 8.592 秒、
  四端 Lua 协议 fixture 9.426 秒。导出/解码五轮竞态测试通过：records 8.784 秒、
  texture 2.807 秒、CLI 7.625 秒。随后新增的 CLI 图像集成测试定向通过，覆盖
  固定项目、离线 CASC、三份证据、mip/通道和失败保留；没有把新测试计入先前全仓运行。
- Windows amd64 npm 隔离安装通过，安装后的 CLI 执行原始/PNG/WebP 导出、
  逐像素与透明度核对、mip/通道选择、错误保留和证据验证；新增第三方通知在实际
  安装中与源文件逐字节一致。报告：
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-vwWqCw/report.json`。
  包 SHA-256 `d500f7de66f8bf146048f8309a7db123828208f5a79b4d8c6f0179c73a2d167f`，
  压缩 12,146,474 字节、解包 21,687,757 字节。仍是未发布的 `2.0.0-dev`
  单宿主开发包，不代表全部平台、游戏实机、完整许可或正式发布验收。

### 原始资产导出

- `asset export --file-id <id> --output <file>` 复用项目/固定 snapshot 选择、
  本地/CDN 文件读取及 source capture。没有独立下载器或调用旧工具；返回
  `lycheedev.asset-export.v1` 清单，保留精确来源、原始字节摘要/长度和证据引用。
- 输出父目录须已存在且位于受管理 workspace 外。先校验及归档完整内容，再在
  输出父目录暂存并发布；默认不覆盖，显式 `--overwrite` 才替换普通文件。
  原文件不被截断，其他硬链接不随替换修改，持有旧 Root 句柄的读取者仍取得旧字节。
  清单 capture 证明内容与来源，不独立证明外部发布成功或当前文件仍存在。
- 新增完全合成的离线 CASC fixture，真实经过配置、Encoding、Root、BLTE、范围
  缓存及证据链，测试不是用 mock 返回一个预先成功的文件。已安装 npm CLI 同样
  使用该 fixture 验证项目默认目标、精确二进制输出、覆盖冲突、失败保留与两份证据。
- `go test -race ./internal/records ./internal/command -run Export -count=5`
  通过，分别 4.908 / 5.991 秒；跨进程测试 4.579 秒通过，包含两个原生 CLI
  同时导出一个目标，恰好一个成功、另一个返回稳定冲突，产物完整。
  本机创建文件/目录符号链接缺权限，对应两个测试跳过，不能记为已验证；
  磁盘满和进程强杀暂存清理也尚未实测。
- 真实数据：项目固定 Retail `12.1.0.69875` / zhCN，离线导出 FDID `1349477`，
  文件 `C:/Users/follen/AppData/Local/Temp/lycheedev-asset-e3c57addf2754594bd21d46ac6e15e1b/Map.db2`。
  128845 字节，SHA-256 `f4914e00483fec93f634cff52682518eefaf1921226f6e679929551a49a49a40`，
  用文件系统重新计算摘要一致；原始 capture
  `CAP-961c19bd35a9a753bd7603a96f92942ff1e58c40b1d850ef377b4fa36918b923`
  与清单 capture `CAP-06718d0de7d7558225d5b2c8c74f0ec9f799168e860d5f9feae772bdf6b0e80c`
  均通过 `evidence verify`。没有操作游戏或修改旧工具数据。
- Windows amd64 npm 隔离安装报告：
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-N9GHC8/report.json`。
  包 SHA-256 `45009a0f109a6db8140f6c21d157df25f2f7a7bee6044eb36ef2b2fcc841b0b4`，
  压缩 11,733,481 字节、解包 20,997,265 字节。此为未发布的 `2.0.0-dev`
  单宿主开发包；并不证明 BLP/PNG/WebP 转换、文件名定位、demux 或完整发行验收。
- skill-creator 指导资产工作流修订：原始导出与图像转换分开，说明覆盖意图、
  归档与外部文件的区别以及输出丢失后的检查。skill 格式验证、`go vet ./...`、
  npm 启动器/版本工具 9 项测试通过。
- 主 agent 最终执行 `LYCHEEDEV_REQUIRE_LUA51=1 go test -count=1 ./...` 全部通过：
  live 105.959 秒、records 20.127 秒、CLI 13.532 秒、跨进程 8.121 秒、四端 Lua
  协议 fixture 7.822 秒。随后将 Windows 设备路径测试从可跳过的 symlink 用例中
  分离，单独执行通过；本轮没有将 fixture 当作四客户端真实游戏验证。
- Luna 按统一 skill 独立完成离线原始导出，使用实际安装的开发包，保留原项目目标，
  没有网络或游戏操作。主 agent 重新核对 `Map-skill.db2` 摘要与上面一致，并验证
  capture `CAP-24a30f214ef2fe6cd9c74af688d70582066b82268f993359e4260f9f00dc79f0`
  及清单 `CAP-7ae2577e26f5650b94801c31593f04877892d80c711b6322e899b5f7ac829049`。
- 随后的空资产回归实际失败于 `vault: invalid blob input`：读取模块将精确长度 0
  直接作为要求正数的流式预算。修正为最小正预算，仍校验精确长度和 CKey，
  不把空文件当不存在，也不允许非空文件冒充空内容。最终重跑结果以下列补充为准。
- 空资产修复后的 Export 竞态测试连续五轮通过（records 6.879 秒、CLI 6.328 秒）。
  最终 npm 隔离安装通过，报告
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-C9BWmm/report.json`；
  SHA-256 `1b3cacef3f2c88b795df6d14b530276efd7edc4b40461a51122bf9dcf232eac9`，
  压缩 11,733,550 字节、解包 20,997,777 字节。上面的真实数据及 skill 样本来自
  空资产修复前代码，最终包另经合成 300 KB 多区间资产的安装后完整导出回归。
- 空资产修复后的 `go test ./...`、`go vet ./...` 通过：live 103.948 秒、records
  14.432 秒、CLI 10.861 秒、跨进程 6.449 秒；未变化的包使用测试缓存。
  该轮 Go 主进程在测试子进程退出后仍有收尾耗时，最终正常退出；没有将等待
  当作通过，也未强杀或重启原运行。

### 项目固定上下文

- 新增 `project init / lock / status`；项目声明只保存产品，锁保存自包含的
  PinnedSet。复用原有固定引用校验与存储，没有项目专用的第二套版本解析器。
- 消费 snapshot 的源码/数据/资源查询与 live bind 共用项目默认选择；显式
  snapshot 完全跳过项目文件。最近声明损坏或未锁定时不跳过它借用上层目标。
  live run/resume 只用既有 session/operation，不能用项目锁覆盖原始身份。
- CLI fixture 已验证本地 Hotfix 查询的项目默认值、祖先发现、显式快照优先、
  损坏项目错误、help 与未锁定时不创建 workspace、跨目录/新 workspace 导入
  同一完整引用。实际 CLI 进程测试已验证项目锁驱动源码检查与查询及正确证据引用。
- 只复制项目声明与锁，不复制内容缓存、父集合历史、游戏连接或账号。当前锁定
  接受已解析 snapshot；命名目标及从可变配置刷新全部来源仍需完成。
- 真数据验证：从新临时项目的 `src` 子目录，省略 snapshot/project 参数，离线
  CDN `Map[0]` 返回“东部王国”，上下文记录预期固定 PIN；capture
  `CAP-d5100749ae597cf8fea1da1958b4e48b2f43aa73ed07ff69c0063429d1f18b4d`。
  Luna 独立按统一 skill 执行“按项目离线查 Map[0]，不改锁、不碰游戏”场景，
  使用既有固定 PIN 查询并核验证据；主 agent 再核验其 capture
  `CAP-73c87fd736e80845afe3bb8abfefca254cab560e85ec1c1911db2cab81b18a24` 通过。
- 并发回归实际暴露 Windows 文件共享错误：`os.Open` 不允许 delete sharing，
  `os.Rename` 无法替换仍被读取的目标。补充一读一写最小失败测试及“持有旧句柄、
  发布新文件”的确定性失败测试后，改用标准库 `os.Root.Open/Link/Rename`；
  临时平台特例已撤销。Go 1.27 的 Root 实现使用 Windows POSIX rename 语义，
  旧句柄继续读取旧文件，新打开者取得新文件；没有增加失败重试或读命令写锁。
  对应语义见 [Windows 文件替换说明](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_rename_information)。
  三个原始失败用例在 `-race -count=5` 下通过，耗时 21.487 秒。
- 最终代码的全部项目用例以 `go test -race ./internal/selection ./internal/command
  -run Project -count=5` 通过，分别耗时 32.205 / 6.050 秒。`go test ./...` 与
  `go vet ./...` 通过；全仓测试复用了未变化包的 Go 测试缓存，不声称这一轮重新
  执行了全部 Lua/游戏生命周期用例。
- Windows amd64 最终开发包隔离安装通过，新增已安装 CLI 的项目初始化、锁定、
  子目录自动发现与 Hotfix 查询，并逐项校验结果证据。报告：
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-iNKt3G/report.json`；
  包 SHA-256 `cfef605dcd79efa6ff08ea46316f6e1bc7673bd4914a0222af8f17a9681f8682`，
  压缩 11,718,267 字节、解包 20,960,242 字节。仍是未发布的单宿主
  `2.0.0-dev` 包，不代表真实游戏、五平台发布包或正式 2.0.0 已验收。

### 共享本地/CDN 内容读取

- DB2 单记录/分页、SQL 和 asset inspect 共用 `FileQuery` / `ReadFile`，仅
  编码对象读取随来源变化。`--installation` 与 `--cdn` 二选一；`--offline`
  禁止 CDN 与定义网络请求，不回退安装目录，不重新解析 latest。
- CDN 范围请求拒绝完整响应、重定向、压缩、多段、错位及截断；片段按对象/区间
  加锁并存入 SHA-256 vault。索引验证 footer 身份、TOC 与选中页，BLTE 验证
  EKey/块，Root 与最终文件验证完整 CKey。未声称扫描完整 Encoding CKey。
- 真网发现 CN 配置的 `archives-index-size` 与对应索引实际长度不同：
  `0017a402f556fbece46c38dc431a2c9b.index` 提示 135988、实际 173068；实际 footer
  MD5 与键匹配。读取改为 1 字节请求获知长度，再校验 footer/TOC，不放宽区间校验。
- 同一 Retail `12.1.0.69875` / zhCN PIN 的 CDN `Map[0]` 返回
  `Azeroth / 东部王国`。文件 SHA-256
  `f4914e00483fec93f634cff52682518eefaf1921226f6e679929551a49a49a40`
  与此前本地读取一致；128845 字节，Root 67167107 字节。
  在线 capture `CAP-eb0df19da96a669eb6e0afed3f3cd1457896b1a317c488f40ac3b26474abb4b3`；
  离线 capture `CAP-139719a25b52d744166bf9989db1b89affbc1a55156609535c34c42a5a0249d3`，
  正文同为 `8366cb51368ffc7092b0640204aad6fc2836b61e8dfb8e2b4a2ef5758900ecc1`，
  两份核验通过。离线重读含 go run 启动耗时约 1.32 秒。
- 同缓存离线 SQL 返回 `[0, "Azeroth", "东部王国"]`，capture
  `CAP-a3d543900bff56c1f9fa621ba3895a547bd524191af621db09e94318a52812c4`；
  资产 capture `CAP-b1a930c41a2eb05ce9ed7beaf9c5029756c7b5c7c27222b46374b3a2daabd2a6`，
  均核验通过。分页返回 ID 0/1，正确标记 more/truncated，并非全表完成。
- 该发布的 archive-group 在 CDN 返回 404；当前逐索引定位首次查询较慢，仍需
  优化并验收其他产品/镜像。每次文件读取网络上限 4096 次请求、1 GiB 请求字节；
  不把当前单样本通过当成完整 CDN、npm 或真机发布验收。本轮没有游戏输入。
- 本轮 `go test ./...`、`go vet ./...`、npm 启动器/版本 9 项测试、skill-creator
  validator 通过。索引反例另验证“校验和正确但结构错误”的重复/空键、短对象、
  group 越界、非零填充、TOC 末键不符，避免所有反例都只命中 MD5 不符。
- 该轮 Windows amd64 开发包隔离安装报告位于临时目录
  `lycheedev npm 隔离-OrLTuD/report.json`，包 SHA-256
  `2e10c4d2131442f8d4e85647621fb18828782d13310df69e01ee5ce0458e7533`，
  压缩 11696917 字节、解包 20904849 字节。实际安装入口的 CDN 离线 Map 查询通过，
  capture `CAP-d85d15d5f3430978f137cd2a26d9804d4d66058923b213205e53865418c7786b`。
  仍是 private `2.0.0-dev` 单宿主验收，不是远端 CI 或正式发布。
- Luna 的 CDN fixture 经主 agent 审查、修正测试数据后，records/command 全量
  重跑通过。覆盖独立读取者共用范围只下载一次、离线零 HTTP、损坏缓存不重下、
  loose 与 archive 解码对象、错误 EKey/CKey 和错误索引长度提示。
  最终索引 fuzz 10 秒完成 10431 次执行，通过。三个实际安装入口进程并发执行
  DB2/SQL/asset 离线读取均成功；此处没有并发执行游戏输入，不能替代游戏共存验收。
  CDN/范围测试在 `-race -count=10` 下通过，最终 `go vet ./...` 通过。

### 远程发布目标与共享身份

- `target resolve --product <track> --region <region> --locale <locale>` 从
  HTTPS 版本/CDN 清单固定当前发布身份；`--build` 是完整 Build 约束，不做历史
  查询或跨地区回退。支持现有四产品映射，未据此扩展插件安装范围。
- 本地与远程共用配置解析、内容键校验和 DataPin。版本/CDN 原始清单作为一组
  离线观测存储，保留 observedAt；在线失败不回退旧观测。精确配置按键加 OS 锁
  并复用 SHA-256 对象。并发解析各自返回自己的观测，不读取全局 current-target。
- Luna 实现清单解析与反例，主 agent 检查后修正测试要求：同地区重复行不能被
  Build 筛选掩盖；非法 host 测试必须使用有效 path，避免另一个错误掩盖测试目标。
  主实现覆盖下载/归档、父集合冲突、8 个并发请求的配置单下载、离线重放、损坏
  缓存不重下、CDN 镜像失败回退、响应中断关闭、取消及错误准入。
- 真实国服 Retail `12.1.0.69875` 解析通过，build config
  `9258fbe8b88a178e130b0318b6b86217`、CDN config
  `5525ea1ce6668e895569c89c2d6a154c`，DBD commit
  `83057bdc0cbe13062850ebf8ad530031e128a1cd`。结果与既有本地准备相同：
  `PIN-567602cc1c15be06597fd08ae2b1ab000040d2ac776a15aa2dac2fa917dd5021`。
  在线 capture `CAP-875a48b65329767525722f0666607085e50b7f8053aecc3cbb8bdbf8da118385`；
  离线 capture `CAP-345f34a87ee1b40f7cf0738d9163e220b5ccb41fa53a6718620e31fe54f7eebc`。
  离线保留观测时间 `2026-09-21T18:01:43.7598216Z`，两份证据核验通过。
- 使用同一 PIN 离线读取真实本地 `Map[0]`，返回 `Azeroth / 东部王国`；capture
  `CAP-d4b5796e5cb4b4206104f752137e817a8cd4fb0cc2651555301da3d0c808438e`
  核验通过，正文 SHA-256
  `427fb1ec2713a29bf2f531436ce0e7e36c5e87be0faa29edeb18f197b9dcc433`。
  仍使用独立临时 target 工作空间，无游戏输入或安装写入。这不是 CDN 文件读取
  或真机游戏执行；远程内容访问仍须继续实现。
- skill-creator 指引已更新远程选择、离线观测和内容读取的区别；quick_validate
  与 `go vet ./...` 通过。records/command/selection 定向测试通过。首轮全仓测试
  在 `TestHotfixConcurrentArchiveReads/6` 遇到一次 exit 5，原日志未展开错误。
  已补失败诊断，单独重复 100 次通过；尚不能据此声称该并发故障已解决。
- 随后命令测试包完整重复 20 次通过（84.533 秒），远程准备/解析测试重复 10 次
  通过（19.479 秒）。第二轮全仓 `LYCHEEDEV_REQUIRE_LUA51=1 go test -count=1
  ./...` 通过：command 10.415 秒、records 15.923 秒、live 116.263 秒、
  四端协议 7.666 秒、跨进程 6.831 秒。`go vet ./...` 与 `git diff --check`
  通过。初次并发失败仍未定位，列为发布前待确认问题，不用后续绿灯撤销记录。
  本轮未重新打 npm 包；上一节之后记录的开发包不包含这次远程目标准备实现。

### Hotfix 命名字段与最新批次

- `data hotfix --table <name>` 使用共享 Definitions 模块的固定 commit/Build，
  在不依赖静态 DB2 文件存在的情况下解析字段；表哈希碰撞、缺失/歧义定义不猜测。
  `--table-hash` 仍是原始查询。`--offline` 禁止定义下载，不转用另一来源。
- 解码使用零结尾 UTF-8、准确整数宽度/符号、数组、头部 non-inline ID、payload
  non-inline 关系。inline ID 不匹配、非法 UTF-8、非有限浮点、截断与剩余字节拒绝。
  每个有效记录保留原始 hex 和 fields；无 payload/非有效状态只保留原始记录。
  共享 schema 与表身份 JSON 改为既定 lowerCamelCase，DBD 字段名保持原样。
- `--latest` 在表/记录筛选后、游标/limit 前选择最大 signed push，保留整个批次
  的重复 ID、无效记录和并列 push。普通查询仍只扫描一次，不新增副本索引或状态机。
- 真实缓存 `ItemSparse[282425]` 在线准备固定 DBD 后离线重复查询通过：
  push `109870`、`Display_lang=猎兽者指环`、`ItemLevel=197`、
  `OverallQualityID=4`，321 字节 payload 完整消费。定义摘要
  `331751cbb17f2fd11d30602a49926847cf02684db12c06781a4d84f13ea56596`。
  最终离线结果 capture
  `CAP-fea09488cd6ef442c1472af0a97811f6819c942196e089a78f9d030d571acf96`
  已通过核验，正文 SHA-256
  `aaea2090bfc3205eff15887437083aa1752fedebb4938c6453ab181a6ff13196`。
  来源为下节固定缓存与派生目标，不代表服务器最新值或游戏运行验证。
- CLI 临时目录回归覆盖命名表无 DB2 文件 ID、缺定义离线失败、固定定义来源、
  解码/原始切换、完整 latest 批次分页、哈希碰撞及字段长度错误拒绝。
  解码 fuzz 20 秒（79,244 次执行）通过；没有错误样本，不代表穷尽格式证明。
  npm 隔离测试已增加实际安装启动器的命名 Hotfix/分页/证据验证链路。
- 最终全仓 `LYCHEEDEV_REQUIRE_LUA51=1 go test -count=1 ./...` 通过：
  records 11.857 秒、command 9.196 秒、live 101.838 秒、四端协议 7.765 秒、
  跨进程 CLI 6.635 秒。`go vet ./...`、npm 测试脚本语法检查和 skill
  quick_validate 通过。四端协议测试使用测试宿主，不代表四客户端真机验收。
- Windows amd64 隔离 npm 安装通过，实际安装的 CLI 完成命名 Hotfix 解码、
  latest 分页及证据核验；自有窗口绑定、完成态恢复也通过。报告：
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-CxEhkv/report.json`；
  包 SHA-256 `5363c6e91f1989678c26115eea2015cba3e8ad8c95ff4b6934c6bdc0b02bab2f`，
  压缩 11,618,283 字节、解包 20,747,236 字节。Hotfix 安装回归使用合成缓存与
  固定离线定义，窗口与恢复使用合成场景；这是未发布的 `2.0.0-dev` 单宿主开发包。
- 按 skill-creator 做独立 Luna 前向测试，仅提供 skill、相关参考和模拟截断页。
  回答正确区分本地最新 push 与服务器最新数据、不把 `no_payload` 当空字段记录，
  并用原始 capture、同一 snapshot/表/latest 与返回游标继续分页。此项验证编排
  决策，没有执行真实查询，也不替代上面的 CLI 与真实缓存验证。

### 本地 Hotfix 记录链路

- `data hotfix --snapshot <pin> --file <DBCache.bin>` 校验 Build 和整份文件，
  归档原始字节并派生固定 Changes 的目标；不覆盖静态 DB2、不调用旧入口。
  支持表哈希/记录 ID 筛选，物理顺序分页保留重复 ID、原始状态及 payloadHex。
- 分页必须使用 `--from <source-capture> --after-index <index>`，不在变化中的
  原文件上继续游标。CLI 集成测试通过文件变化后继续读取原归档、证据校验、固定
  Changes 冲突拒绝、错误 capture 类型、参数准入、错误 Build 和损坏尾部拒绝。
- 真实 Retail `Cache/ADB/zhCN/DBCache.bin` 只读查询通过：Build `69875`、XFTH
  v9、31,513 条物理记录，2,390,643 字节；前两条状态 3、空 payload 均原样返回。
  SHA-256 `e49a8491a4c81fbf5468d20a915259432113f125b78e4cb96252bb34126c6aff`；
  原始 capture `CAP-6286ffdfb1ceccd2ee1a6b2dd1697258e88e43301b6ca4a089d69a9e7d999d3b`；
  派生目标 `PIN-0dc66aead1a9bd4cf7e888278aae07f8df2f98a659a3ce368e145d3cf64621e7`。
  工作空间复用下文独立临时 target 工作空间，没有读旧工具数据或写游戏安装。
- 真实归档分页返回物理索引 2、3；按表哈希 `2442913102`、记录 `282425` 筛选
  返回一条状态 1、321 字节 payload。原始 capture、分页 capture 和筛选结果
  `CAP-60dbb3e231a65e20a4a9a4eff380268012f1278e0da7f52d6f51178503fe1844`
  均通过 `evidence verify`。没有据此推断表名称或解码字段。
- Luna 编写格式测试，主 agent 审查并补充歧义/截断反例、并发分页及损坏归档测试。
  全仓 `LYCHEEDEV_REQUIRE_LUA51=1 go test -count=1 ./...` 通过：records
  12.714 秒、command 8.448 秒、live 100.611 秒、四端协议 9.093 秒、跨进程
  CLI 7.066 秒。Hotfix fuzz 20 秒、468,681 次执行通过；`go vet ./...`、skill
  quick_validate 与 `git diff --check` 通过（仅原有 CRLF 提示）。
- 此前使用 skill-creator 新增原始 Hotfix 调查与分页说明，区分缓存字节完整、查询
  完整和服务器覆盖。本节记录仅覆盖原始查询阶段；命名字段与 latest 见上一节。
  远程来源仍未实现，不能称为完整 Hotfix 替代。

### 连接后的报告账号选择

- 省略 `--account` 时，只从当前会话的安装、角色和服务器查找唯一目录；空结果
  和多个匹配返回 `live.account_selection_required` / exit 2，候选在
  `context.accounts`。最多读取 256 个根目录项，超过时要求显式账号。
- 账号选择发生在操作创建、队列交付和输入之前。运行测试确认选择失败时没有
  operation、队列变更或输入；完整生命周期模拟使用自动选择，并在首次提交后
  增加一个同名角色目录，仍从原固定账号读报告，不重新选择。
- Windows 自有测试窗口通过真实 CLI 验证新参数、空候选/歧义错误和候选列表。
  Luna 的目录测试发现 junction 可能表现为 irregular 而非 symlink，主实现已修正
  为只忽略普通文件，让 junction 进入共享路径校验；定向测试通过。
- 没有读取真实用户 SavedVariables 或修改游戏安装。当前账号选择证据是临时
  目录及自有窗口测试，不是真实登录账号或游戏报告的验证。
- 本轮 `LYCHEEDEV_REQUIRE_LUA51=1 go test -count=1 ./...` 全部通过：live
  103.028 秒、四端协议 8.217 秒、跨进程 CLI 6.761 秒。随后补充的 256 项边界和
  SavedVariables 哨兵用例经账号选择定向重跑通过（0.997 秒）。`go vet ./...`、
  Node 启动器/版本工具 9 项测试、skill 格式和版本一致性检查通过。
- Windows amd64 隔离 npm 安装通过；安装后的 CLI 自有窗口绑定（含账号选择错误）
  和完成态恢复均为 passed。报告：
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-MyiVA1/report.json`；
  包 SHA-256 `4c5eb4c3306463673d999f984a884ee1038dd0e9c11b1eab98b40d75aa887770`，
  压缩 11,585,454 字节、解包 20,682,216 字节。这是未发布的 `2.0.0-dev`
  单宿主开发包，不是真实游戏执行或正式发布验收。

### 首次连接体验

- 2026-09-23 修订：首连改为宿主驱动——`live instances` 发送 `/dev bridge identify
  <nonce>` 身份标记做候选识别，`live connect` 自动完成 opt-in 与 ready 核验；
  本节以下描述的“用户手打 `/dev connect`”流程已被替代，`live bind` 保留为观察
  路径。现行契约见 design.md §6。以下为本节此前验证记录：
- `/dev connect` 启用桥接并生成当前连接标识，重复调用保留已有身份；
  `/dev disconnect` 停止连接但保留报告。用户不用手工生成 nonce 或连续提交三条
  协议命令。没有新增游戏 API、计时器或禁用时运行的机制。
- `live bind` 只要求 snapshot；唯一窗口自动发现，安装目录来自已验证进程路径。
  PID、安装、角色和服务器可作为约束。默认只捕获选定游戏窗口，不捕获桌面；
  可选区域改为 `--capture-area`，不与数据 `--region` 混用。
- 首次回执发现独立于完整身份匹配：只接受选定 release/product/build 的 ready
  信号，歧义拒绝，角色约束不放宽。后续输入/恢复仍要求保存的精确身份与新画面。
  普通连接结果不再返回 nonce、序号或历史 inputReady；详细证据仍可归档核验。
- 四客户端实际 TOC 加载的 Lua 连接测试通过：默认关闭、初次启用、聊天焦点释放、
  重复连接、断开重连、secret/缺失角色拒绝及保留报告。窗口发现、光学回执的
  歧义/过期/错误身份，以及 CLI 参数回归通过。
- Windows 自有测试窗口通过实际 CLI 和 WGC 完成首次身份发现、归档、校验和
  历史连接读取，覆盖显式区域及整个窗口。不等同于游戏首连或探针执行验收。
- 完整 Run fixture 改为从首次回执发现进入绑定，再通过保存的连接执行，覆盖
  完成、报告未落盘和清理未落盘三种结果。解析/归档/执行共用真实临时存储，只在
  窗口与输入 I/O 替换；没有绕过连接接口预先拼装持久身份。
- 最终代码的 `LYCHEEDEV_REQUIRE_LUA51=1 go test -count=1 ./...` 通过：live
  99.476 秒、四端协议 7.850 秒、跨进程 CLI 6.922 秒。`go vet ./...`、启动器/
  版本工具 9 项测试、skill 格式和版本一致性检查通过。
- 最终 Windows amd64 隔离 npm 安装通过，实际安装的 CLI 自有窗口身份发现与
  完成态恢复均为 passed。报告：
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-gh3aEf/report.json`；
  包 SHA-256 `eaad4dd816e50f49e1a9ffc3fb57be9f3c01ff9dec3d443879cae4f4a931ac23`，
  压缩 11,579,368 字节、解包 20,669,127 字节。此为未发布的单宿主开发包，
  不证明四端游戏实际运行或正式发布验收。

### 本地目标准备与数据查询

- 四客户端安装元数据 fixture、固定父目标扩展、不匹配/损坏配置拒绝、客户端更新
  拒绝、CLI 参数冲突和重复解析得到相同 pin 的测试通过。
- DBD 引用在线固定、离线读取、损坏/缺失记录、HTTP 错误/重定向/超限、取消与
  并发写入测试通过。两个并发请求各自保留观察到的 commit，不继承另一个请求的
  目标；缓存别名不是全局 current-target。
- 独立新工作空间读取真实 `_retail_` 安装，得到 Retail `12.1.0.69875` /
  Interface `120100`，DBD commit `83057bdc0cbe13062850ebf8ad530031e128a1cd`。
  在线与离线准备返回相同 pin：
  `PIN-567602cc1c15be06597fd08ae2b1ab000040d2ac776a15aa2dac2fa917dd5021`。
- 使用同一个客户端路径读取 `Map[0]`，得到 `Azeroth` / `东部王国`；离线重复读取
  返回相同正文摘要 `189ecdad673d230b865bae2fe91accaca321c6ab47b813e3304189a905ac5de2`。
  原始 capture `CAP-38862782e7be8f3e6c44441a9433652ca0bbba3e380c9f5ff40bf0864bc1ab0e`
  已通过 `evidence verify`。
  工作空间：`C:/Users/follen/AppData/Local/Temp/lycheedev-target-48a24d8a5d7e449c8d2ade8a635c6713`。
- 上述为真实安装文件与网络/缓存查询，不是游戏内执行验收。未输入游戏、未覆盖
  安装、未读取旧工具工作空间。skill-creator 指导下更新数据与资源编排引用，
  不把目标已固定等同于全部表已准备，也不让 agent 手工构造 CASC 身份。
- 此轮全仓 `LYCHEEDEV_REQUIRE_LUA51=1 go test -count=1 ./...` 通过：live
  93.910 秒、records 10.793 秒、四端协议 8.087 秒、跨进程 CLI 6.295 秒。
  `go vet ./...`、Node 启动器/版本工具 9 项测试、版本一致性、skill
  quick_validate 和 `git diff --check` 通过。没有执行新的 npm 打包或发布；下列
  隔离包记录属于此前代码，不作为此轮新增目标准备命令的安装验收。

### 运行状态与命令收敛（此前验证）

- 精简记录后的 `go test ./internal/live ./internal/live/journal -count=1` 通过。
- 完整生命周期模拟扩展到 33 个场景，六次提交分别覆盖发送前进程退出、发送后
  未保存回执、部分提交后恢复、缺证据、外来证据，另含完成与两类落盘失败。
  崩溃由 fixture 中断调用栈模拟，不冒充进程强杀或真机测试。
- 重定向防护 fixture 改为新格式，检查目标变化仍拒绝；skill 格式与版本检查通过。
- CLI 契约和局部帮助测试通过；全仓 `go vet ./...`、npm 启动器及版本工具的
  9 项测试通过。没有引入新的命令框架。
- `run_lifecycle_test.go` 从保存连接进入 Run 使用的完整内核，自行创建任务，
  固定窗口、区域、snapshot、代码和恢复 ID，覆盖完成、报告未落盘和清理未落盘。
  使用真实临时存储与队列，只有捕获/输入被替换；尚不等同于安装后 CLI 真机运行。
- 增加中断场景后 Windows 本地 live 包约 90–100 秒；基础 CI 包级测试超时从
  2 分钟调为 5 分钟，留出 runner 与 race 检测余量，不改变游戏请求的时间预算。
- 该轮代码的 `LYCHEEDEV_REQUIRE_LUA51=1 go test -count=1 ./...` 全部通过，live
  包 97.153 秒，包含新 Run 入口用例、33 场景恢复、四客户端协议和跨进程 CLI。
- 该轮 Windows amd64 npm 隔离安装通过，原生自有窗口绑定和已安装 CLI 的完成态
  恢复均为 passed。报告：
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-refgeC/report.json`；
  包 SHA-256：`21ae9a9eebda0bd330af1353686926d3d0b149bd7a67514f5e3e69fcf53b7b01`。
  压缩 11,556,596 字节，解包 20,610,780 字节。仍为单宿主开发包，没有发布。
- `git diff --check` 通过（仅现有 CRLF 提示）；skill quick_validate 通过。
  下节保留上轮证据，不替代以上最终重跑结果。

## 2026-09-23 实施批次（历史工作包快照；状态不可代表当前实现）

统一交付曾拆为互不重叠写集的并行工作包；下表记录该批次开始时的计划与状态，
不是当前事实台账。其 `实现中` 状态不得覆盖本页当前状态及后续完成记录。集成职责
当时归主 agent：Controls.lua 双接线
（`bridge identify` + 裸 `/dev` 工作台）、四 TOC 汇入、`internal/command` 统一
接线、skill 终稿验证。

| 工作包 | 写集 | 验收关联 | 状态 |
| --- | --- | --- | --- |
| 自动发现/识别/选择/连接（`identity` 信号、bootstrap 输入、`live connect`/`live instances`、session 复用恢复） | internal/live·desktop·bridge、protocol/、addon/Bridge/ | SEL-10、WIN、RUN、SKL-12..16 | 实现中 |
| 工作台基础层（widget/window/序列化/值树/存储/Locale/Safety + About 页 + tests/addon 框架） | addon/Core·UI 基础、四 TOC | WKB-01/02/04/11/12 | 实现中 |
| Run + 落盘记录页 | UI/Pages/{Run,ExportRecords}.lua | WKB-02/03/04/09 | 实现中 |
| 对象 + 追踪 + 诊断页 | Modules/{ObjectInspector,FunctionTrace,Diagnostics}.lua、UI/Pages×3 | WKB-05/07/08 | 实现中 |
| 事件 + 自动化页 | Modules/Events/*（含四份署名目录）、UI/Pages/{Events,Automation}.lua | WKB-06/10 | 实现中 |
| wowdata 领域查询 + db2 模式 | internal/records 新文件 | DAT-03..09、AST-04 | 实现中 |
| 资产名称检索/文件查询/video demux | internal/records 新文件 | AST-01..03a | 实现中 |
| wowdoc 源码 query/explore/inspect/validate-matrix | internal/codebase | SRC-01..08 | 实现中 |
| 命名目标/缓存/doctor/旧根隔离 | internal/selection·vault | SEL-09、STO、CON-05..07 | 实现中 |
| Hotfix 远程提供方 + CSV | internal/records hotfix/编码 | DAT-06/06a/06b/07 | 实现中 |
| 文档增补（inventory 工作台范围+修正+关闭 unmapped；regression WKB-01..13、SKL-12..16） | docs/toolkit/capability-inventory.md、regression.md | 发布判定完整性 | 已交付（主 agent 复核并补决策 14） |
| 第三方许可闭环（AGPL 派生审查、完整通知、对应源码交付） | THIRD_PARTY_NOTICES.md、packages/npm/lycheedev/THIRD_PARTY_NOTICES | PKG-07、许可门槛 | 实现中 |
| Windows CI + 发行闭环（required job、Windows 产物、tgz 封存/OIDC/回读；2026-09-23 收缩为仅 Windows amd64） | .github/workflows、tools/、packages/npm 测试 | REL-01..14、PKG-01..08 | 实现中 |

当时尚未开工/待前置（历史记录，非当前状态）：`internal/command` 统一接线（含 `live bugs`/`reload`/`cancel`、
`evidence list/bundle/keep/remove`，等 Go 连接包完成后避免同文件并发）；skill
终稿对安装载荷验证 + quick_validate + SKL-12..16 独立前向测试；四端 Lua 与
npm tgz 全量回归；真机闭环（扫描→选择→连接→探针→结果→恢复）与工作台逐项
真机验收；旧实现退役与双语 README/AGENTS 全面修订；正式 2.0.0 发布（仅在
全部验收通过后执行）。golden 捕获/比较语义按决策 14 保留在 tests/tools。

### 第三方许可闭环（2026-09-23 完成审计，owner 裁决待定）

- 完整第三方清单已落入 `THIRD_PARTY_NOTICES.md` 与
  `packages/npm/lycheedev/THIRD_PARTY_NOTICES`（后者含各组件完整许可正文）：
  15 个编入二进制的 Go 模块（nativewebp、gozxing（MIT+Apache-2.0 zxing 双文本）、
  gopher-lua、modernc sqlite 全家、x/image（经 nativewebp reader 实际编入，非仅
  测试）、x/text、x/xerrors 等）、vendored luaqrcode（BSD-3）、Bob Jenkins
  lookup3（公有领域）。
- 逐文件派生分类（技术溯源，非法律意见）：wowdata（AGPL-3.0-or-later）派生
  22 文件约 4.9k 行，均带 `SPDX-License-Identifier: AGPL-3.0-or-later`（BLTE/
  CASC idx/TACT 索引/encoding/root/DB2 表与 schema/DBCache 扫描/BLP 解码/
  listfile/video AVI 等）；`cache_fields.go` 与 `relational/` 派生存疑（前者建议
  补标注）；wowdoc（MIT）派生集中于 `internal/codebase`（role/assets 近逐行移植、
  诊断码与哨兵串沿用），MIT 署名已入通知；SQL 引擎等其余区域判定倾向干净室
  重写（全树对 wowdata 错误串/注释片段的 grep 为零命中）。
- owner 裁决选项（详见通知文件的 decision aid）：(i) AGPL 合并作品发行（保守
  默认）；(ii) 干净室替换派生区后 MIT；(iii) **同作者再授权**（git 证据显示三仓
  均为同一作者 Follen/FollenFang、无外部贡献者，成本最低）；(iv) 双许可+逐文件
  标注。建议 (iii)+(iv)；**绝不可**在无再授权的情况下以 MIT 发行含 wowdata 派生
  代码的包。AGPL §13 对本地 CLI/插件不触发，但不得移植 wowdata 的 MCP 服务端。
- 对应源码交付机制（集成时实现）：`release.json` 增 `correspondingSource`
  {repository, commit, tag, archive, sha256}；发布作业 `go mod vendor` 后
  `git archive` 打源码包（可离线构建）入 SHA256SUMS 与外部封存清单；GitHub
  Release 附源码包+校验和+验证报告；发布前从源码包重建并比对二进制摘要。
- 发布前待修一致性问题：`packages/npm/lycheedev/package.json` files 声明的
  README.md/LICENSE 均不存在（README 归 CI/发行包补、LICENSE 内容待 owner
  裁决）；无根 LICENSE；design.md §12 旧"Lychee npm MIT"表述已修正；根 README
  缺 license 一节；`cache_fields.go`、`remote_catalog.go` 的出处标注缺口由代码
  作者补。全部结论基于 2026-09-23 未提交工作树快照，发布前须对冻结 commit 复核。

### 待集成接线清单（2026-09-23，各包交付材料汇总，主 agent 执行）

**状态：已执行完毕并通过验证**（四 TOC 三段插入含 AutomationView 载入顺序位与
按客户端事件目录行、`Bridge\Identity.lua`、八页行；Controls.lua identify 三段；
design/regression/implementation-status 陈旧行修正；TOC 共享段测试扩展为允许
客户端目录数据行）。验证证据：`go build ./...` + `go vet ./...` 干净；
`go test ./tests/addon/...` ok（四端工作台套件 + TOC/Locale 锚点）；
`LYCHEEDEV_REQUIRE_LUA51=1 go test -count=1 ./...` **25 包全绿**（含
tests/protocol 四端 conformance 加载新 TOC 闭包与 Identity 模块）。以下为
执行依据原文：

四份 TOC（全部相同插入）：
1. `Bridge\Reentry.lua` 之后、`Core\Runtime.lua` 之前加一行：`Bridge\Identity.lua`
2. 同位置再加工作台模块块：
   `# --- Workbench feature modules (object inspection, function trace, diagnostics) ---`
   `Modules\ObjectInspector.lua` / `Modules\FunctionTrace.lua` / `Modules\Diagnostics.lua`
   （事件/自动化包若回报 Modules\Events\* 与 AutomationView 行则一并加入此块）
3. `UI\Pages\About.lua` 之后（"later page modules"锚点）加：
   `UI\Pages\Run.lua` / `UI\Pages\ExportRecords.lua` / `UI\Pages\Object.lua` /
   `UI\Pages\Trace.lua` / `UI\Pages\Diagnostics.lua`
   （事件/自动化包的 `UI\Pages\Events.lua` / `UI\Pages\Automation.lua` 行同段补入）

`addon/Core/Controls.lua` 三段（Go 连接包提供，锚点如下）：
(a) `local nonce = string.match(command, "^bridge bind ([0-9a-f]+)$")` 之后加
    `local identify = string.match(command, "^bridge identify ([0-9a-f]+)$")`
(b) usage 白名单加 `and not identify`
(c) `if command == "connect" then ... end` 块与 `if action then` 之间插入 identify
    分支：`ns.Identity.Trigger(identify)` → `ns.ReceiptView.ShowIdentity(receipt)` →
    `ns.Identity.WhenInputReady` 一次性复显 `ns.Identity.Refresh(identify)`；忙碌时
    Trigger 失败返回且绝不触碰他人回执。

文档陈旧行修正（对照 Go 连接包报告核对，其中 design.md 首连段与
implementation-status L17-19 已于本轮修订，其余待改）：design.md §2 游戏入口行补
`/dev bridge identify <nonce>` 机器动词；§6 live 动作表补 `connect`（instances 现为
发现+识别）；regression.md WIN 首连场景段更新为 `live connect` + 身份标记流程；
SKL-10 补注：`live instances` 发送身份触发（Mutates:true）非只读列举；
implementation-status「首次连接体验」节与能力覆盖「游戏运行」行补身份标记自动化表述。

集成后验证：`go build ./...`、`go vet ./...`、`go test ./tests/addon/...`（TOC 共享段
与 Locale 锚点测试）、`LYCHEEDEV_REQUIRE_LUA51=1 go test ./tests/protocol/...`。

## 真机 UI 反馈（2026-09-23，用户实测）

- `/dev connect` 实机已工作（回执二维码正常显示、chat 返回连接结果）；对象页鼠标
  拾取提示正常。
- 待修：回执二维码**过大**且位置不符——用户要求与原版一致放**左上角**。目标规格
  （旧 `UI/AutomationOverlay.lua`）：TOPLEFT 锚点、白底卡片上限 480px、模块尺寸
  = max(2, ceil(3×物理像素)) UI 单位、静区 4 模块。涉及 `addon/Bridge/ReceiptView.lua`
  的 Show 与 ShowIdentity 两条显示路径（当前 TOPRIGHT、4px/模块无上限）。
  **（已修复并真机验证，见下节。）**

## 真机验收（2026-09-23，Retail 单实例，用户裁决：只测正式服）

用户最新裁决：真机测试只做 Retail（正式服）单实例；不允许 Computer Use，全部经
工具链（CLI 自身 WGC 捕获、原生输入）操作并截图验证。

前置修复与重建（本日）：
- `ReceiptView.lua` 按旧版规格重写（TOPLEFT、480px 上限、物理像素模块、4 模块静区）；
  协议四端测试（`tests/protocol` 身份标记+宿主解码）与工作台 13 套件全绿。
- 发行工具三处修复并并入检查点提交 `82aea78`：`tools/release.mjs` 的
  `go version -m` VCS 正则（行首制表符）、GNU tar `C:\` 路径（改相对路径提取）、
  离线重建比对（GOTOOLCHAIN 模块校验死锁 → 解析 GOROOT 内 go 二进制 +
  两侧对称 vendor 构建；autocrlf 工作树 CRLF 与 archive LF 差异 →
  `.gitattributes` 强制 `* text=auto eol=lf` 并 renormalize 工作树，
  比对侧改为 `git checkout-index` 禁用过滤器导出）。
- 三客户端安装经正规链路重建：`release.mjs assemble` 产出完整发行根
  （5 平台二进制、addon ZIP、对应源码包、tgz、sealed 清单，
  `correspondingSource` 就位）→ `addon install`：retail 升级路径因旧安装 drift
  被设计内拒绝（`modified`），删除漂移目录后全新安装；classic/titan 此前因
  robocopy 误删归属清单处于 `unmanaged`，同目录删除后全新安装。三者现为
  `managed`（receipt version 2.0.0-dev，commit 82aea78）。此前覆盖式同步
  （交接 §5 方法）今后不可用于已受管安装——队列要求安装与清单逐字节一致。

真机闭环（Retail `12.1.0.69875` / Interface `120100`，单实例，PID 18468）：
- `live instances`：发现已装+运行候选，逐窗口一次性身份标记，回执解码成功，
  输出角色/服务器/输入就绪（识别失败状态如 busy 如实呈现）。
- `live connect`（自动 bootstrap）：唯一匹配自动连接，会话保存。
- `live run`：有界只读探针在真实游戏内执行，返回
  `{"probeStatus":"completed","result":{"addonLoaded":true,"build":120100,...}}`，
  `report.state=verified`、`cleanup=complete`。两轮完整闭环通过。
- 清理路径的重载带 nonce 核验（RuntimeEpoch+1）；**重载后插件回到默认关闭**，
  旧会话等不到 ready（`command.cancelled`，退出 7），必须重新 `live connect`
  bootstrap 建立新会话后运行——此为设计行为，已两轮验证（对应 SKL-13/14）。
- 探针排队要求"干净受管安装"（`delivery/queue.go` 校验 receipt 一致），漂移安装
  在 `load_requested` 被拒（`delivery.invalid_installation`）——安全门按设计工作。
- 二维码几何截图验证：`internal/desktop.CaptureFrames`（CLI 自身 WGC 链路）在
  身份回执显示期间抓帧 2560×1440 PNG；截图证实白底卡片位于**左上角**、卡片
  远小于 480 上限、模块精细（新几何生效）。截图（本机临时目录，含角色画面，
  不入库）`%TEMP%\lychee-frames\frame-1790116978095.png`。回执本体证据链
  （decoded-window-session / decoded-load-readiness / decoded-game-reentry 等
  18 份 capture）已归档并通过核验。
- 真机已知边界：身份回执在聊天输入框获得焦点时按设计隐藏（防误读），CLI 的
  识别在隐藏前完成解码；截帧需在显示窗口期内。
- 未做（如实标注 not_run）：WKB-01..13 工作台逐项 UI 手测（需要游戏内人工
  交互验证，含前后截图）；classic/titan 真机（用户裁决排除）；Forever 全部
  （排除）。

## 上轮验证（2026-09-21）

- 最终代码的 `LYCHEEDEV_REQUIRE_LUA51=1 go test -count=1 ./...` 通过，包含四端
  Lua 协议 fixture 和跨进程 CLI；这是模拟/离线证据，不是真机。
- session 重连、身份变化、无新画面、部分输入保留、禁止覆盖目标，以及完整模拟
  生命周期和报告/清理区分的定向回归通过。
- 跨进程测试改为验证任务级结果、未完成状态、不暴露内部 observation；重跑通过。
  归档引用错配反例通过：不能仅凭阶段记录输出 verified 报告。
- `go vet ./...`、skill quick_validate、版本一致性检查、`git diff --check` 通过。
- 独立 Luna 使用新版 skill 对真实结构的模拟结果做前向测试：正确保留 verified
  报告、标记 cleanup 未完成、不重跑探针，并允许另一个 agent 继续固定数据查询。
  这是决策测试，不是 CLI 或游戏执行。
- Windows amd64 隔离 npm 安装通过，含新 run 参数准入、已安装命令的自有原生
  窗口绑定、已安装命令的完成态恢复，以及插件/skill 安装、升级、恢复与移除。
  报告：`C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-QlvCZM/report.json`；
  包 SHA-256：`32421a0af6d5fbdb2bc9918600f5ed759bc1bc0f8fc86f533f224fd17b34c952`。
  此为单宿主开发包，不是五平台正式包或已安装 CLI 的完整真实游戏运行验收。

## 真机环境侦察（2026-09-23，只读）

- 本机 WoW 根 `D:\Game\World of Warcraft`，客户端目录 `_retail_ / _classic_ /
  _classic_titan_ / _beta_ / _anniversary_ / _classic_era_`。`.flavor.info` 实测：
  `_retail_=wow`、`_classic_=wow_classic`、`_classic_titan_=wow_classic_titan`、
  `_beta_=wow_beta`、`_anniversary_=wow_anniversary`、`_classic_era_=wow_classic_era`。
  本机当前没有 Forever（`wow_forever`）安装；`_beta_` 是 beta 测试轨，不能当作 Forever。
- 已安装插件：`_retail_`、`_classic_`、`_classic_titan_` 三处 `Interface/AddOns/Lychee Dev`
  为 `2.0.0-dev`（四 TOC 齐全，`LycheeToolkitDB`，Interface 120100/50504/38002/16001）；
  `_retail_` 另有 `!BugGrabber`（诊断采集源可用）。WoW 根存有
  `LycheeDev-backup-20260912-232327` 备份目录。
- `Interface/AddOns/.lycheedev-window-owners` 为空（无他人在途操作归属）；
  `.lycheedev-locks` 有 3 个 0 字节锁文件（2026-09-23 01:27–01:38，此前会话测试遗留协调标记）。
  部署/测试前仍须按此复查，不覆盖在途归属。
- 观察时 `_retail_\Wow.exe` 正在运行（PID 18468，启动 2026-09-23 01:15:09）。此前
  Computer Use 捕获为黑屏的历史观察不代表当前画面；真机验收前须重新观察实际画面。
  本轮未输入游戏、未部署、未读取账号 SavedVariables 内容（仅列目录名）。
- 画面侦察补充（2026-09-23 第 9 轮）：WoW 窗口为全屏（0,0,2560,1440）且未最小化，
  但被其他工作窗口遮挡；GDI CopyFromScreen 只能取到遮挡窗口，无法观察游戏内容。
  这提示此前"黑屏"也可能是 GDI/PrintWindow 对 D3D 表面的捕获假象，而非游戏实际
  黑屏。真机验收的游戏画面观察必须走 WGC 窗口内容捕获链路（`internal/desktop`
  的 CaptureFrames，即 CLI 自身的捕获路径），不能用屏幕截屏替代。

## 真机与发布

上次只读检测到 Retail 12.1.0.69875 / Interface 120100，安装目录仍为旧 1.2.0
插件。Computer Use 显示黑屏，未确认角色，未覆盖旧插件、任务注册表或
SavedVariables，未输入新协议。继续真机验收前需重新观察，旧观察不证明当前在线。

2026-09-22 重新通过 Computer Use 选定该游戏窗口，原始捕获和激活后捕获仍为
黑屏，只能看到鼠标；可访问树无登录或角色控件。未输入命令、未选角色、未部署。
已询问用户本机画面是否正常；这不阻塞代码工作，但本轮真机验收尚未执行。

四端 Lua fixture 不替代四端真机，自有 Windows 测试窗口不替代游戏输入资格。
2.0.2 发布仍需完成 [发布规范](release-2.0.2.md) 的完整能力、许可、平台、安装和人工验收门槛；本轮非真机工作不替代人工验收。

## 下一条产品链路

继续收敛单一运行状态和首连体验，贯通“选定目标 → 源码/数据研究 → 游戏探针 →
获取结果 → 中断恢复”，随后补齐全部缺项、修订文档并退役旧实现。不以局部协议
修补或重复打包代替产品验收。
