# Agent Live r4 实施记录

状态：实施中，未发布，尚未完成架构与真机验收。用户已批准执行[方案](agent-live-architecture-2026-09-27.md)，包括 UI 修订。单人实施，子代理保持停止；不将旧工作区改动、离线测试或局部完成视为整套重构已经交付。

## 已完成的实施批次

### 执行器：结果封存与受管资源

- Finish/Fail 在受管调用栈内先记录候选结果，调用栈退出后才封存；同栈后续异常覆盖候选成功。
- 新增有界 Callback 包装，在用户函数入口前检查任务与会话，捕获异步异常；终结后的旧受管回调不会再次进入用户代码。旧的非受管回调仍不属于可撤销保证。
- 清理先于报告封存，逐项继续并记录有界错误；清理未确认时拒绝 ACK。工作区仍需完成宿主完整任务对这些事实的解释与恢复。
- Async 依据执行起点扣减剩余预算，不因延迟调用延长截止。
- 队列 Busy/Reset 检查执行器活动资源；Session 更换不抹去未决执行，未决旧运行态需保留观察或经过验证的 reload。
- 新增正式回归覆盖 A11–A13；既有 session-change 场景移入独立环境并增加阻止重连清除占用的断言，没有删除覆盖。

验证：Go build/vet 通过；`LYCHEEDEV_REQUIRE_LUA51=1 go test ./tests/protocol -count=1` 通过。四个 profile 是模拟环境，未据此声明真机通过。

### 宿主准入：任务正文先于共享占用

- 本地意图、快照引用、请求索引先原子提交，共享 claim 随后以完整临时文件原子发布。
- claim 关联不可变意图摘要；只允许准备阶段、正确工作区和规范化安装范围恢复缺失的发布。未知提交后的缺失占用不自动重建。
- 驱动取得及每次检查核验实际 claim；不再增加一份易失真的 admitted 状态字段。正式推进仍需现有运行锁和新鲜输入权限。
- 真实子进程分别在 claim 发布前/后直接退出，重开后保持同一 OP、同一请求索引，能够恢复；其他 workspace 不可接管。

验证：journal 全套与 Live 包全套通过，Live 用时约104秒；元数据关闭导致意图失败时不会留下共享 claim。其余 G4 安装维护、首连及输入边界仍待实施与验证。

### UI：独立设置页与统一保存

- 设置页从 Window 中提取为 Pages/Settings；保留侧栏和既有八页面。
- 使用人可读组合键、直接键盘录入、统一保存；绑定整组校验和应用，失败回滚，未保存草稿与外部有效值分开。
- 减少动态效果写入账号设置并在后续打开时生效；中英文成对更新。
- 小窗口采用纵向布局及滚动，恢复默认、冲突反馈与焦点释放都有交互回归。

验证：Go build/vet 通过；四 profile 工作台测试通过；整组快捷键应用失败回滚的协议测试通过。真机视觉和输入未验收。

## 真实客户端观察

### 完整任务、存储和恢复（后续批次）

- 新 `live execute` 接收一个 Lua 文件或固定 PRB、session、request 和 1..120 秒预算。游戏代码、目标、账号、报告 scope 固定后，以同一个 journal 的 `finished` 目标完成装载、执行、报告、ACK、队列退休和显示清理；没有再包一套主状态机。旧原子命令不改变原有完成目标。
- `nextAction` 返回同一 OP 的 resume。报告已核验但清理 pending 保持可用；业务失败且完整收尾返回 exit 5。CLI 契约、生成命令表和 skill 已接线，skill 默认用 execute，异步示例使用受管 Callback。
- 游戏新增 Investigation 小型事实表：16 槽、每槽 2 KiB，仅完成槽可回收；运行前预留报告的一条记录及 8 KiB 最小失败空间。循环/秘密/不支持值编码失败转为有界失败报告，清理异常不因此变成功。
- 新 nonce 相关 observe/finish 在原 owner 下查询同一 request、代码摘要、角色、session 和 runtime。Checkpoint 证据可恢复被隐藏的旧业务 QR，并存为历史身份锚点；后续输入仍需新鲜 ready。光学清理须先有肯定终结回执，再有同一 WGC 流中两个较新的有效无符号画面，缺任一则保留 owner。
- `LycheeToolkitBridgeDB` 改为 SavedVariablesPerCharacter，放报告、重入与终局事实；账号 `LycheeToolkitDB` 保留设置和工作台历史，旧账号报告不搬运。宿主固定 `character-v1` 路径，不回退到账号文件；旧操作未声明 scope 时仍按其原合同读取。
- `live instances --passive` 不捕获、不输入、不声称角色就绪。稳定 request 摘要忽略窗口标题、读取方式和瞬时 readiness，保留实际路径、PID/启动时间/HWND、build、GUID；重复请求先找本地冻结账号再判断摘要。
- 首连 BTP 与 OP 共用安装范围的窗口准入和执行锁；安装维护在存在任何未决共享 owner 时明确拒绝。两个工作区竞争同一窗口、不同窗口独立以及维护锁竞争都有自动化覆盖。
- 原生 probe 操作在准入、投递和文件推进前检查可见的其他 WoW 进程。不同安装跳过；同安装要求两个当前 WGC 观察确认不同角色分区，同角色或无法确认的 peer 拒绝。此守卫不向其他任务输入，不把历史 session 当新鲜身份；它是协作环境下的保守检查，不能阻止游戏外部在检查之后手动换角色或启动新进程。
- UI 设置改为分组和整组保存；自动化报告直接展示，技术详情折叠，列宽适配 800 单位画布，失败报告不再着成功色。Receiver 去掉伪人工输入字段，保留状态、截止和退出键；420×176 圆角面板。

验证：完整纵向测试覆盖正常完成、清屏中断恢复、报告 QR 隐藏、loaded/flush/ACK readiness 隐藏，断言 run/ACK 各一次、恢复不重发；角色存储/失败报告/容量/设置交互协议测试通过。Receiver 旧高度断言已同步；全仓强制 Lua 随后通过。当前真实客户端验证见下节，不能据此声称全部可靠性矩阵已通过。

2026-09-27 20:40、21:44（本地）：只读 WGC 捕获 Retail 12.1.0.69933 / 120100，PID 61784；0条输入。证据位于工作区 `.tmp/agent-live-r4-before-20260927/`、`.tmp/agent-live-r4-current-20260927/`。工作台未显示，此图只证明目标与捕获环境，不能当作工作台 before 图；用户提供的此前界面图仍作为视觉问题依据。此时尚未部署，后续按受管安装更新。

### Retail 实际执行与视觉复验（晚间）

目标固定为 Retail 12.1.0.69933 / 120100、PID 61784、同一 HWND/进程启动身份与角色；候选为未发布的本地开发包，未 overlay 安装。最终 addon 来自 `.tmp/agent-live-r4-dev6-20260927/dev-npm-stage`；后续 dev7 仅修 CLI 参数与恢复建议，addon 字节不变。

| 实际操作 | 结果与证据 |
| --- | --- |
| `OP-f08c1d918cd85f87d9ea032a0b8e3e89` | 测试脚本自身使用不存在的全局 probe，正确返回业务失败/exit5；报告 verified、cleanup complete、display cleared。保留 `.tmp/agent-live-r4-smoke2.json`，不计为业务测试成功 |
| `OP-b4b2649b7c19c28903e7a09d1b30c193` | 修正脚本后9页面调用通过，角色报告与完整收尾通过，`.tmp/agent-live-r4-ui-result.json` |
| `OP-6d5da5cd4b115b9ad7c7e254816e98fd` | 修正异步等待 QR 后完整成功；第一轮连续 WGC 发现标题及历史列表层级错误，`.tmp/agent-live-r4-ui5-result.json`、`.tmp/agent-live-r4-ui-visual5/` |
| `OP-9bb0a806f8d5599876ee3cd2d72e43a8` | 第二轮55秒异步任务，入口、每次切页及 Finish 前均断言 Receiver 不活动、ReceiptView.Current 为空；9页通过，`unobstructed=true`；report verified、cleanup complete、display cleared。`.tmp/agent-live-r4-ui6-result.json`、`.tmp/agent-live-r4-ui-visual6/` |
| 同 key execute 与同 OP resume | 约0.078s / 0.063s 返回同一已归档结果；两次调用前后全部 bootstrap/work 文档及 generation 摘要不变，`.tmp/agent-live-r4-repeat.json` |
| `OP-fba843325ce6d4d43efe7ee62dffffa4` | 正常 reload 通过；只读 WGC 观察 RGB 连续轮换及未唤醒时到期消失，`.tmp/agent-live-r4-activate5.json`、`.tmp/agent-live-r4-reload-visual5/frames.jsonl` |

最终成功报告正文 capture：`CAP-7869892e6d3cbe0aeee3a340d898548421a08f61b19e75f4226e7ee7c0e23170`；回执 `CAP-85812007ee880f5fcdad8c0d242b1ccd0fecd874b6953188c417ced108b19add`；显示清理 `CAP-4c74019ad355a8331cac0f8a5b30cc0ff31971eaf3ee1c13aaa9b47cabb74017`。报告实际写入角色 `SavedVariables/Lychee Dev.lua` 的 LycheeToolkitBridgeDB，没有读取或迁移旧用户数据。

实际发现并修复：

- BugGrabber 的 ADDON_ACTION_BLOCKED 指向 Receiver.OnTextChanged→Reentry.Reload。intent-v2 将完整 LDC1 校验与按键分派分开，最终 Enter 才进入业务；正式服装载/flush reload 现已通过。原生 fixture 断言完整文本后仅一次 Enter down/up，归属丢失时不发送最终按键。
- 原先只关闭 Receiver 仍不够：异步 Dispatch 返回 loaded 信号，Controls 会再绘制 QR。现在异步返回本地接受事实，不绘制等待信号；终局发布保留。四 profile 反例先失败、修复后通过。
- 标题纹理属于外层 frame，被侧栏子 frame 覆盖；改为侧栏自己的纹理。历史列表从外层 frame 迁入 pageCanvas，不再遮挡导航。快捷键按钮增加与字段一致的圆角底面。
- `--passive` 契约/handler 已有，但布尔解析白名单漏接。删除重复白名单，按契约判断布尔参数；真机被动枚举返回 unobserved，不采集、不输入、不声称角色就绪。
- 已有终局编码失败、Resume 本身明确拒绝的任务不再给出无条件 resume 的 nextAction。普通证据/清理 pending 仍指向同一 OP。

可审阅截图：`ui-visual5/082404.png` 为修复前运行页遮挡导航，`ui-visual6/089384.png` 为修复后；`ui-visual6/125529.png` 为设置页；`ui-visual6/082717.png` 为异步执行初始场景，没有接收框或握手 QR。上述路径均位于 `.tmp/agent-live-r4-` 前缀目录。RGB 在 ui-visual6 的 055530→055815 两帧中随接收器唤醒消失；无唤醒到期由 reload-visual5 独立记录。画面上的其他插件和游戏提示不属于桥遮挡保证。

Go build/vet、四 profile 协议与工作台、16/16 Node release 测试（含固定 LuaLS archive）、skill quick_validate、85命令契约与版本检查通过。最终 `LYCHEEDEV_REQUIRE_LUA51=1 go test -count=1 ./...` exit0，日志 `.tmp/agent-live-r4-final-go2.log`；其中 Live 包141.269s。最终安装检查 managed，57个 addon 文件与仓库及 dev7 包 SHA-256 一致，证据 `.tmp/agent-live-r4-addon-final.json`。包为 developmentOnly，未提交、未打 tag、未发布 npm。

## 尚未完成

- G1 固定成本合同已同步且有冷启动/登录/战斗/冲突离线计数；首次安装与禁用/重新启用的完整真机组合仍 not_run。G2 Retail 成功及业务失败链已通过；G3/G4 的多角色、多实例与完整竞争/崩溃矩阵尚未完成。
- 首连 prepared 的 claim 发布中断目前保守停留 pending，需要明确 abandon，不自动重发；清理失败后重入、最坏身份长度/容量等边界继续审计。已补 loaded 回执丢失恢复，不将成功纵向链充当全部故障组合覆盖。
- 多写入者真实多进程与混合版本验收；当前检查不代表可原子冻结外部客户端状态。
- Retail 中文接收面板、9页与两轮 WGC 复验已执行；英文、小视口/缩放、真实键盘设置录入、战斗和 relogin 场景仍 not_run。
- 三支持端矩阵中，Classic/Titan 此轮未在线且未执行真机；Forever 不计入验收。真实多进程、10,000调度种子、长跑与正式发行门禁仍 not_run。

没有 commit/tag/npm 发布；不能将本记录当作候选发布通过报告。

按用户“不保留旧备份”的要求，验收后尝试删除本任务 `.tmp/agent-live-r4-install-transaction*-20260927` 六个安装归档；命令已校验限定工作区路径，但自动审批审查以 `blocked by policy` 拒绝执行，未给出更具体原因。归档仍保留，没有绕过拒绝；当前 clean managed 安装不受影响。
