# Lycheedev mailbox protocol v1

本协议定义 3.2.0 开发基线中 Go CLI 与游戏内 Lua addon 的 Native Live 行为。
实现位于 `internal/bridge`、`internal/live/channel`、`internal/live/memory` 与 `addon/Bridge`。
协议名称、schema 字段、transport 字段及槽 TOC 中的传输名称均使用同一个字面值：
**Lycheedev mailbox protocol v1**。

## 1. 通道与身份

- CLI → addon：500 个 LoD 槽承载不可变信封，一个正式执行快捷键触发读取。
- addon → CLI：Lua 中的命名 Mailbox，保存身份、输入状态、回执和报告。
- 重载：独立的正式快捷键直接调用 UI reload API，不占用业务槽。
- 启动诊断：独立的 BootstrapMailbox；它说明启动情况，不授权执行输入。

CLI 只读游戏内存。运行时指令特征匹配与 RIP 相对位移解析得到 Lua 根 RVA，
再沿 Lua state → globals → LycheeDevInternal → Mailbox 读取。
每次读取重新核验当前路径和发布对象。禁止私有堆暴扫、旧地址回退、把磁盘 EXE
中的固定地址当成当前根，或把其他构建的 RVA 直接套用到当前进程。

公开的 `Mailbox.receipts` 与 `Mailbox.bodies` 各保留一个 `schema` 元数据键，值为本协议全名。
该字符串键使尚无回执、或最后一个 BODY 已释放时的哈希表仍有正常分配的节点。
nonce/ticket 只能是32位小写十六进制串，不会与该键重合；按精确键查询缺失记录仍返回缺失。
枚举公开 map 时须单独处理此元数据；私有回执集合与 Snapshot 不包含它。

Forever 在主模块 `.text` 的可读区间内匹配指令，不依赖构建 hash 白名单。
短读已返回的前缀仍参与匹配，剩余部分按页有界补读；不跨不可读缺口拼接特征。
全部命中先解析 RIP，再按目标根去重；不同根通过 Lua/Mailbox 路径验证选择，
多个有效根仍报告歧义。根证据保存实际读取字节、缺口、特征命中和候选根数；
存在缺口时 `scanComplete=false`，成功读取 Mailbox 不代表完整扫描或全范围唯一。

目标由 installation、PID、进程创建时间、HWND、构建与产品共同确定；进入世界后
再绑定角色 GUID、runtime、owner、fence。目录名只是位置，产品以实际客户端元数据为准。

## 2. 两个正式快捷键

| 动作 | Bindings.xml 名称 | 默认 | 默认被占用时 |
| --- | --- | --- | --- |
| 执行 CLI 指令 | LYCHEEDEV_WAKE | Ctrl+Alt+F12 | Ctrl+Alt+F10 |
| 重载插件 | LYCHEEDEV_RELOAD | Ctrl+Alt+F11 | Ctrl+Alt+F9 |

动作显示在游戏快捷键设置的 Lychee Dev 分类中。只为尚未设置的动作选择空闲键，
保存当前账号或角色绑定方案；不覆盖现有玩家绑定或其他 addon 的 override。
两个动作使用不同功能键，支持 Ctrl+Alt+F1–F12。Mailbox 发布实际生效键位，
CLI 不硬编码发送默认值。缺少任一有效键时返回 `input_binding_unavailable`。
`/dev receiver reset` 只补齐未设置的动作，不重置玩家已有设置。

## 3. 编辑框与输入就绪

普通 EditBox 可能先截获全局快捷键。bridge 启用时，FocusBindings 保留当前编辑框
原 OnKeyDown，并仅对两个实际绑定的组合键设置 `SetPropagateKeyboardInput(true)`。
其他键保持原处理和传播行为；不读取、改写或发送文字，不移动光标，不清除焦点。
切换焦点、停用 bridge 时恢复原处理函数和传播设置。

聊天框通过暴雪的焦点事件即时接通，其他编辑框复用已有 INPUT 采样；不新增轮询器。
密码、secure text、protected、forbidden 或无法安全观察的编辑框不接管。
战斗、未登录、加载期间和 secret/API 不可读时不授权输入。

当前 INPUT 字段：

| 字段 | 含义 |
| --- | --- |
| schema | Lycheedev mailbox protocol v1 |
| runtime、owner、fence、nextSlot | 当前运行时、占用与槽路由 |
| guid、build | 角色与构建 |
| sampleMillis | 当前宿主 uptime 采样毫秒 |
| inputBlocked、reason | 是否阻塞及具体原因；ready 时为 false 和空字符串 |
| bindings | 实际有效的 wake、reload 两键 |
| escapeLimit | 固定为 0；任何正值不能授权当前输入 |
| inputAttempt | 最近回调诊断，不是执行回执，也不授权重发 |

输入样本年龄不超过 500ms，不领先宿主 uptime 超过 100ms；前一次输入后至少等待
晚于该输入 100ms 的新样本。发键前重新读取同一发布并核对采样、身份、路由与键位。
Lua 执行回调在加载槽前再次检查输入状态；聚焦时还要确认透传仍作用于当前编辑框。

启用 bridge 后的 INPUT 采样框架不依附 UIParent，避免挂机界面隐藏 UIParent 时
连带停止采样。未登录、加载和战斗等限制仍生效；这不保证第三方键盘框架会透传快捷键。

Native CLI 只发送执行或重载组合键。**没有 Esc、Enter、聊天文字或盲发降级路线。**
输入向固定 HWND 发送 PostMessage，不抢 Windows 前台；最小化窗口不能输入。
CLI在首次输入时启动被动键盘监听，并在本次调用结束时关闭。任意键盘按下、重复或抬起
事件均暂停新的自动组合键；所有实体键松开且连续500ms没有键盘事件后，重新读取并核验
INPUT，再在首个key-down前检查一次键盘。按住W移动也会持续等待。监听只保留最近事件时间，
正常放行按键，不记录键名或文字；启动监听后同样先等待500ms。
这些检查不阻塞持锁调用，零消息的等待沿原调度和期限重试。已开始的组合键完成发送和抬键，
取消或错误时仍释放本次已按下的键，不在Ctrl/Alt按下时等待键盘静默，也不释放玩家的实体键。
submitted/uncertain继续读取原回执，不因键盘活动而暂停读取或重发。发送途中仍可能与实体输入
重叠，是否执行以精确插件回执为准。

## 4. 启动、doctor 与首次安装

最小启动核先于工作台和业务模块加载，拥有命名空间、启动事件、正式绑定和独立诊断。
后续模块初始化失败时，BootstrapMailbox 仍可读取，手动重载键仍可用。
启动核不执行探针，不将启动成功视为业务输入许可。注册结束后不保留空闲计时器或 OnUpdate。

BootstrapMailbox 的 `bootstrap` 字段使用二进制记录 kind=6，包含 schema、bootstrap
标识、release、phase（loading/ready/failed）、reason 与绑定注册快照。
CLI 从当前 Lua 路径读取它；这些静态启动事实没有 INPUT 的新鲜度保证。
只有当前 Mailbox.input 可以授权自动按键。

`live doctor` 默认只读：检查目标选择、受管安装、500 槽、窗口占用、内存定位、启动核、
业务运行时、已发现槽数和输入状态。结果含逐项 state、detail、nextStep。
指定 installation/PID 或 CON 时，顶层 `doctor` 复用相同检查，不实现另一套规则。
已发现500份槽元数据不等于槽均已启用或已加载。最新回调加载失败、未分派、异常或拒绝
单列为 `live.input_attempt` 错误；`LoadAddOn` 的 `DISABLED` 保留为 `slot_addon_disabled`。
这条诊断用于修正所选角色的槽启用设置，不是执行回执，也不允许重放已提交输入。

`live doctor --fix` 在同一目标范围内复用现有安装、连接和恢复入口：

| 当前情况 | 处理 |
| --- | --- |
| 缺少或过旧的 clean managed addon | 从当前发行包安装/升级，保留升级归档；开发运行可用 --release 指定 |
| 原项目已有 CON | 恢复原连接及原请求，不新建竞争连接 |
| 其他项目占用 | 报告原占用；不抢占、不删除日志或槽预约 |
| 当前协议已加载且 INPUT ready | 正常绑定或通过独立重载键激活已安装的更新 |
| 改名前的二键运行时，INPUT 新鲜且 escapeLimit=0 | 读取实际 reload 绑定，记入原 CON 后发一次重载键；观察新协议后绑定 |
| 未加载，或旧运行时没有可验证的二键 INPUT | 保留目标和 CON，返回 manual_reload_required；由用户在选定窗口手动 /reload 一次 |
| 客户端尚未发现全部 500 槽 | 明确要求重启选定客户端，不循环 reload |
| 受限焦点、战斗、输入过期或未知执行结果 | 保留原状态，等待或核对原回执；不补发按键 |

首次安装没有 addon 回调时，任何“自动打 /reload”都缺少输入落点证据，因此不自动发送。
协议改名不等于首次安装。已加载的二键运行时仍可提供重载许可；兼容读取只用于这次
升级的 identity/INPUT，不接受旧业务信封、回执或 BODY。新发布和正常业务只使用本协议全名。
旧 schema 名称曾用于三键和二键两个实现，不能单凭名称判定自动重载能力。doctor 与激活
先读取实际 INPUT 键位；三键实现进入手动等待，二键实现仍须取得新鲜 INPUT 才能发键。
结构有效的已知200槽旧身份也进入升级兼容性判断，不因当前500槽合同而提前报无效记录；
旧身份仅用于选择升级方式，不获得当前业务权限。损坏的当前身份仍按协议错误处理。
手动完成后用原 CON resume；CLI 观察新 runtime、验证身份和 inventory 后才继续。
手动等待尚未授权机器输入，不消耗后续连接预算；新运行时被选中时只启动一次连接预算。
已有未知机器输入的历史 activation 保留原预算和不确定事实，不转为“未发送”。

`live reload fallback` 保留为激活命令的兼容入口，行为服从本节；名称不代表盲发许可。

## 5. 连接、执行、结果与结束

1. 选择固定目标，验证受管安装、进程身份和当前运行时。
2. 持久化 CON 与窗口占用，发布 bind 信封；精确回执确认绑定。
3. execute 固定 request、ticket、源码、执行预算与恢复策略。
4. prepare → challenge → commit：Lua 核对 preparedNonce、challenge 与完整目标，
   只有执行快捷键触发的 commit 才进入业务。文本变化不执行代码。
5. 读取精确 HEAD/BODY，confirm 后保留报告，release 清理对应 BODY。
6. disconnect 通过 unbind 或已验证的进程/运行时退役释放原连接占用。

通常每次操作用四个槽；每个 runtime 最多消费 500 个物理槽，已加载槽不重用。
容量不足时，先在日志中记录重载，再发独立 reload 键。新 runtime 重绑后继续同一请求，
不延长业务预算。剩余物理槽不等于仍可容纳一整次操作。

结果区分三件事：报告字节已核实（reportState）、业务成功与否（report.ok）、
资源清理是否完成（cleanup）。报告成功但清理 pending 时保留可用报告并完成后续清理。
同一已完成 request 的重复调用读取原结果，不重新执行源码。报告 JSON 保留有限 binary64
数值的可往返表示；secret 与非有限数值仍明确拒绝，不以舍入后的值冒充原始证据。

报告已核实、release 已尝试发送但没有精确回执时，先继续观察原回执。持续等待
5 秒后，CLI 在原操作期限内持久化一次清理重载，取得新鲜 INPUT 后发送独立重载键。
原 release 和业务不重发，报告、ticket 与尝试次数不变。只有确认旧 runtime 退役后
才能记为 `cleanupMethod: runtime_destroyed`；这与正常 `released` 回执分别记录。

## 6. 日志与恢复

每次输入先记录 intent，再记录 not_sent、submitted 或 uncertain outcome。
submitted 仅证明消息已入队。缺失 outcome 或未知执行结果都不允许重发同一次业务或重载。
`status` 只读原证据；`resume` 在原 project/CON/request 上串行继续。
原输入 outcome 缺失或没有时间时，原回执恢复后，下一交易用当前已核验 INPUT 读取的
宿主 uptime 建立一次持久上界，再等待晚于此上界 100ms 的样本。旧发键前样本不能冒充
输入时间；反复恢复不移动该上界，不重放原输入，也不延长原期限。

读取期间发布路径发生变化时，丢弃该快照，在当前 CLI 调用内按已有调度间隔重读，
不延长持久期限。重载已提交、正在等待替换运行时时，Lua 根暂为空或旧字符串布局暂不可读
也继续原观察流程。普通阶段的布局错误、协议不兼容、根指令配方及进程身份变化仍报告错误。
发键前复核与普通读取共用错误分类和 trace；只有明确 not_sent、0 条消息才允许再次准备输入。
读取重试不构成重发 submitted/uncertain 输入的许可。

`Lookup.firstReadFailure` 可记录一次读取的固定逻辑路径、请求/返回长度、原有区域准入
或系统错误及guard复核结果。它只复用本次已有读取，不增加扫描或输出地址、payload、
nonce等内容；缺少适配器证据的字段保持缺省，不推测底层失败原因。

初始 bind 没收到回执而需要结束连接时，先 disconnect 保留原关闭意图。显式 reload
可沿同一 CON 清理这个尚未确认绑定的连接：仍要求新鲜 INPUT，核实旧 runtime 退役后
关闭原连接，不绑定替代 runtime，不重发 bind，也不重新启动业务。

`--wait-seconds` 只限定本次宿主调用；连接、操作、重载、关闭的持久期限单独保留。
超时、进程重启或换 CLI 不延长已开始的业务预算。
默认 opaque 策略保留未知副作用；显式 observation 策略在确认 runtime 更换后，可在既定上限内
使用新 ticket 重做观察，但保留旧尝试的不确定事实。

历史日志里的旧键位与 Esc 计数仅用于解释已发生的输入。当前驱动不再利用它们发键。
旧 input-v3/hybrid-v2 必须手动激活新 addon；不自动升级旧输入协议。

## 7. 多实例与共享安装

每个进程独立拥有 PID/创建时间/runtime/CON/owner/fence。共享安装的槽发布使用短锁和
持久预约；等待焦点、输入回执和业务执行时不持有发布锁。
一次 wake 最多处理一个属于自身 runtime 的信封。外部槽不能授权本实例业务。
一个实例的 ready、重载成功或角色身份不能为另一个实例提供许可。

安装更新作用于共享磁盘文件；每个运行实例是否加载新版本必须独立验证。
doctor --fix 只对选定进程执行恢复；维护锁和原占用继续约束共享目录升级。

## 8. 支持范围

交付目录和 Addon profile 均包含 Retail、Classic、Classic Era、Titan、两种 Forever beta 与 Anniversary 七产品；不能由 Source/Data 可用推断 Live 实机验收已通过。
产品差异归入 Compat、产品目录和运行时定位配方；两个 Beta 的源码 track 都是 forever，
数据与安装身份仍按地区产品隔离。

离线验收覆盖：禁用与启用、绑定冲突与保存、启动失败诊断、编辑框透传、零 Esc、
未知输入不重放、手动激活与取消、500 槽容量、跨语言记录和共享安装竞争。
实机分别验证输入聚焦、重载、升级/首次激活、执行/清理；Retail 另做双实例竞争。
已完成事实与尚未运行的项目只记录在 [待办与验证范围](../todo.md)，不将计划写成通过。
