# Rust 迁移交接：唯一当前事实来源

更新：2026-10-14 UTC。分支：`rust-refactor`。本次首批提交仅文档，用户随后已授权按本计划全量实施；产品迁移尚未完成。

## 1. 当前决定与不可漂移基线

- 唯一验收目标：以正式 **Go 3.2.0** 为行为基线，实现 Rust 宿主并保留 **EmmyLua 直接分析库/native-worker/session** 替代 LuaLS。除此之外，全部业务、同一 addon、CLI、Skill、完整 Mailbox 协议、错误恢复、诊断和输入合同对齐正式版本。
- Go tag `v3.2.0` 的 tag object：`93849f43f34d8ff1d6f4539c5a3496ab941ef229`；peeled commit：**`3057a9666ab078270b63c7f06de748069b0d1f4b`**。本次通过远端 tag 查询及本地 fetch 核实。所有下文 Go 文件引用均固定于该 commit，不跟随 main。
- Rust 调查起点：`5443314c536e925801d3f0360110db946181d1b3`。它是复用基础，不是完成声明。按模块差分决定保留/替换/补齐，不从零丢弃已有成果，不沿用不符目标的旧协议。
- unsafe约束：自有业务/跨平台Rust代码默认禁止unsafe，用适当crate级forbid/deny守住；确有必要的Windows FFI/内存边界必须最小化安全封装、逐处SAFETY理由和指针/生命周期/线程约束与测试，独立审查，不整模块无限豁免，不为性能目标放开unsafe。第三方依赖另列审计，不能宣称整个依赖图无unsafe。
- 内部设计应体现Rust特点，不逐行翻译Go：合理所有权/借用减少复制，enum/类型状态、Result、RAII和有界安全并发按实际收益使用，避免过度抽象/炫技。对外合同不变，unsafe边界不放松，性能仍实测；纳入独立架构验收。
- Skill尽量复用固定Go3.2.0版本；仅Emmy引擎、安装和真实诊断等必要差异修改，不因换语言重造工作流。
- 性能：可控耗时减少至少 70%，即 **Rust ≤ Go 30%**，排除下载与不可控外部等待；同机同数据、输出与校验等价，cold/warm 分开独立重复，逐路径记录。未测不能保证达标。
- 所有需要用户、实际游戏或特定本机互动配合的测试放在本文最后；离线与可自动执行的 Windows CI 先做。Windows 专属测试不自动等于游戏测试。
- 首批交付仅文档；随后已获准全量实施本计划。仍不操作游戏、不发布、不合 main，不修改 Go PR #13 或其他项目。只保留本文件，不创建 `handoff` 文件或第二份交接事实来源。
- 交付约定：为Rust分支创建独立草稿PR；之后每批获准代码改动及时推送并更新同一草稿PR，不等全部验证完才提交，也不每批新建PR。先查已有PR避免重复。后续实现授权不等于游戏操作、发布或合并授权。

[DESIGN.md](DESIGN.md)解释目标结构，[todo.md](todo.md)给执行顺序，两者从本文件派生；更新状态与证据首先改这里。旧 `docs/toolkit/` 和历史 README/UI 设计是原受测范围的记录，不得覆盖本节。历史“Go3.1.2替换”“Rust≤Go50%”“重新设计四槽exact-COMMIT”与旧输入合同已被本次目标替代；历史测试通过仍只证明原始候选。

## 2. 固定 Go 合同索引

| 合同 | 固定来源 | 使用方式 |
| --- | --- | --- |
| 产品/架构/业务 | [Design.md](https://github.com/Follen/Lychee-Dev/blob/3057a9666ab078270b63c7f06de748069b0d1f4b/Design.md) | 提取模块和产品合同，不复制其运行状态为 Rust 状态 |
| 全量命令、参数、输出 | [command_contract.go](https://github.com/Follen/Lychee-Dev/blob/3057a9666ab078270b63c7f06de748069b0d1f4b/internal/command/command_contract.go) | 固定 release binary 的 describe/help/正常及错误 golden；不能沿用旧89命令数量 |
| 协议全貌 | [docs/live-protocol.md](https://github.com/Follen/Lychee-Dev/blob/3057a9666ab078270b63c7f06de748069b0d1f4b/docs/live-protocol.md) | 全部8节与实现/测试共同组成验收，不只移植定位 |
| 协议实现 | [channel](https://github.com/Follen/Lychee-Dev/blob/3057a9666ab078270b63c7f06de748069b0d1f4b/internal/live/channel)、[memory](https://github.com/Follen/Lychee-Dev/blob/3057a9666ab078270b63c7f06de748069b0d1f4b/internal/live/memory)、[bridge](https://github.com/Follen/Lychee-Dev/blob/3057a9666ab078270b63c7f06de748069b0d1f4b/internal/bridge)、[desktop](https://github.com/Follen/Lychee-Dev/blob/3057a9666ab078270b63c7f06de748069b0d1f4b/internal/desktop)、[addon Bridge](https://github.com/Follen/Lychee-Dev/blob/3057a9666ab078270b63c7f06de748069b0d1f4b/addon/Bridge) | 原样保留语义及边界，差分正常/失败/中断 |
| Agent 工作流 | [Skill](https://github.com/Follen/Lychee-Dev/blob/3057a9666ab078270b63c7f06de748069b0d1f4b/skills/lycheedev/SKILL.md) | 比对公开业务步骤、命令/flag和错误恢复；只改真实引擎/构建说明 |
| 开发/发行/许可 | [development](https://github.com/Follen/Lychee-Dev/blob/3057a9666ab078270b63c7f06de748069b0d1f4b/docs/development.md)、[licensing](https://github.com/Follen/Lychee-Dev/blob/3057a9666ab078270b63c7f06de748069b0d1f4b/docs/licensing.md)、[release](https://github.com/Follen/Lychee-Dev/blob/3057a9666ab078270b63c7f06de748069b0d1f4b/release) | 固定包身份、对应源码、清单和自动验证 |
| Go 原始验收范围 | [todo](https://github.com/Follen/Lychee-Dev/blob/3057a9666ab078270b63c7f06de748069b0d1f4b/todo.md)、[handover](https://github.com/Follen/Lychee-Dev/blob/3057a9666ab078270b63c7f06de748069b0d1f4b/handover.md) | 参考已知限制与故障，不把 Go 实机通过转记成 Rust 通过 |

Go 基线若需要变更，必须明确记录新目标、精确 commit、原因和差分范围，再重建受影响 oracle。不能因远端 main 前进而自动滚动。

## 3. 现状、证据与缺口（本次源码只读调查）

状态口径：`implemented`=有代码；`historical-pass/fail`=旧记录、非本次重跑；`unverified`=当前等价证据未齐；`blocked`=明确前置缺口；`not_run`=本轮未运行。任何一项都不能替代最终验收。

| 模块 | 已观察的实现/历史证据 | 当前缺口与下一步 |
| --- | --- | --- |
| workspace | 根 Cargo.toml 声明 cli/core/data/delivery/live/source/windows 七crate | 按Go3.2逐业务核对；提交落后不等于每个语义未实现 |
| Source引擎 | `crates/lycheedev-source/Cargo.toml` 默认emmy-prototype，真实链接emmylua_code_analysis/parser；`src/native/host.rs`、`host_runtime.rs`、`session.rs`等已有worker/session实现 | 保留直接库架构，差分业务并关闭生命周期/资源/完整输入问题；不能称其仍只是LuaLS壳 |
| Source复建 | 同manifest依赖被忽略的`.tmp/rust-source-binding-context-01/upstream`修改版；版本=0.30.0 | tracked checkout缺原修改版完整字节；已核验公开upstream基底，可按用户新授权移植最小必要补丁并作为新来源重新证明等价/复建；不声称恢复了旧字节或用户本地丢失 |
| Source完整分析 | [历史host/runtime进度](docs/toolkit/rust-host-and-runtime-progress-2026-10-12.md)记载多轮Retail失败和分项通过；最后Retail19只记录正在运行 | `unverified`；无Retail19成功结论，不据历史unit通过宣称完整cold/warm/query/context/close通过 |
| Source旧链 | feature条件下旧LuaLS链尚在；luac5.1与tree-sitter有独立验证/结构职责 | 审核调用、feature、测试与包消费者后再删真正死代码；不按名称一刀切 |
| Live默认路径 | `crates/lycheedev-live/src/native.rs`无匹配RootRule返回live.locator_not_finalized；`runtime_observation.rs`明确ObservationOnly | 新runtime/RIP观察尚未构成默认可输入连接；默认接线与freshness/identity/授权完整闭环待迁移 |
| Live旧协议 | 历史Rust native exact-COMMIT/four-slot与Input4/Input5叙述、旧WGC/optical路径残留 | 按固定Go完整v1统一，不保留两套输入合同、不靠观察成功授权执行；逐消费者清理旧路径 |
| Delivery/容量 | 已有200→500磁盘迁移实现及历史测试 | 不等于旧200槽运行时升级、空公开map、DISABLED诊断与Go3.2 R29语义已等价；须分别验证 |
| Data | `crates/lycheedev-data`已有CASC/DB2/SQL/Hotfix/assets等实现和分项历史测试；历史[rust-data-remaining](docs/toolkit/rust-data-remaining-2026-10-08.md)列剩余项 | table/scan Hotfix、remote provider、SQL effective等仍有unsupported/未闭环范围；按具体命令与数据重验，不能把整个Data写成空白或完成 |
| README/规范 | 旧README仍描述Go/LuaLS，旧DESIGN为工作台UI规范，AGENTS有旧目标优先级 | 本次替换入口并保留历史原文；不会通过文档改动假装产品已迁移 |

本次未运行cargo/go/Lua产品测试、Windows CI、性能基准或游戏测试。原因是任务为文档，且默认Emmy依赖复建缺口明确。文档静态检查结果随提交核验记录；历史测试次数不冒充本次运行。

## 4. 分阶段迁移与退出条件

1. **冻结oracle和盘点。** 下载/构建对应Go发布产物并核对commit/sha256；导出describe/help、默认值、输出schema、退出码、Skill和addon清单。建立逐命令矩阵，每项链接源码/fixture/结果。差分不得改变main或Go工作树。
2. **复建Rust依赖。** 优先实际获取已验证公开Emmy upstream固定commit，核验archive/tree/许可证，按所需API/语义差分决定直接复用或实施最小可review补丁。用户已明确允许这条新依赖重建路线；不能把原私有目录缺失作为全任务长期阻塞，也不能把新补丁伪称历史原字节还原。保留新来源/补丁/测试身份，更新旧custody假设而不伪造原manifest哈希。固定工具链、Cargo.lock和对应源码后，隔离两套fresh Cargo home顺序离线重建，遵守配额/所有权。
3. **保留基础并先自动差分。** core/selection/project/evidence/delivery、parser/CLI/Skill、Source、Data分别建立正常/错误/恢复golden。逐模块决定保留、适配、替换或补齐；保留首次失败和修复后结果。
4. **完整Live协议迁移。** 先导入固定Go Lua/协议fixture与模型故障集，再接runtime/RIP默认host、输入、日志和恢复。复用同一addon，先离线再Windows自有子进程/合成窗口，不操作游戏。
5. **清理与一致性。** 删除经证实无消费者的旧LuaLS/WGC/光学/旧协议或其他死代码，修正已失效文档入口；每批有引用审计、替代覆盖和受影响回归。历史证据保留，不清用户数据或未登记缓存。
6. **候选/性能/自动Windows。** 同一封存Windows amd64候选验证CLI/addon/Skill/包闭包、安装升级卸载、错误恢复、离线对应源码和性能矩阵。需游戏采样的性能分项转到最后专节，不阻止此前自动工作。
7. **最后实际配合。** 仅在上述前置可独立完成项闭环后，运行最后一节清单；记录同一候选身份、真实环境和结果。缺客户端/用户未就绪保持not_run。
8. **交付评审。** 全部必需矩阵逐项有证据，未通过项明确保留；文档交付不等于Rust产品完成。发布、npm标签变更、主分支合并另行授权。

## 5. 全量功能验收矩阵

下列全部为迁移验收要求，当前统一 `unverified` / 本次 `not_run`。最终应扩展为固定Go命令表的逐命令行；本表不是用工作流概览替代全量覆盖。每行记录Go/Rust commit与binary hash、输入pin/hash、命令、输出/退出码diff、耗时、日志与结论。

| ID | 必需路径 | 正常/边界/错误及恢复证据 |
| --- | --- | --- |
| F01 | describe/help/全部公开命令与flag | 名称、别名、默认值、选择优先级、无效/冲突参数、版本输出、退出0/2–8；注册不算实现 |
| F02 | JSON/JSONL/CSV与证据 | lycheedev.result.v1字段、数值精度、分页cursor/截断/覆盖；JSONL唯一合法end且exit0才完整，后置失败不发成功end；归档/校验/bundle |
| F03 | 产品/安装/项目/pin | retail、classic、classic-era、titan、forever-asia-beta、forever-cn-beta、anniversary；元数据身份、地区/locale/build/config固定，共享forever源码不混地区数据 |
| F04 | Source同步/索引/search/query/definitions | 固定Git/CAS/DBD/API环境、XML/TOC加载顺序、exact/prefix/index/body优先级、空/缺失/损坏/大输入、不可执行研究源码 |
| F05 | Source refs/calls/context/flow/diff/validation | Lua/XML映射、调用条件和unknown/secret、动态绑定、luac语法、load closure、API版本矩阵；coverage/complete/truncated/uncertainty各自真实 |
| F06 | Source batch/session | 固定Go批文件64KiB、1–32项、90秒整批合同；顺序、部分失败、跨请求仅显式session-reuse、缓存失效、取消/超时/crash、后续正常请求、关闭资源 |
| F07 | DB2/schema/table/scan/SQL/export | 全格式/字段类型/关系/过滤/排序/分页，SQL effective，缺定义/密钥/部分解码、JSONL后置复核、原始及派生内容完整性 |
| F08 | Hotfix | local DBCache/Wago/Raidbots等固定Go支持入口，table/scan与provider路径、build/locale过滤、cursor和fullQueryComplete、窗口完整不冒充全服务端覆盖 |
| F09 | CASC/CDN/资产/关系 | 本地EKey完整验证/桶/代次；远端精确build/地区，不跨产品回退；并发与总预算；listfile、BLP2 PNG/WebP、VP9 AVI、spell/item/creature/encounter/housing路径 |
| F10 | 安装/升级/卸载/打包 | 同addon清单与500槽、修改文件保护、版本/receipt、归档/失败恢复、共享目录锁、npm启动器、Skill命令生成/合同和许可；不能把元数据数量说成已启用槽 |
| F11 | Live全业务 | bind/execute/status/resume/reload/disconnect/doctor及兼容入口；P01–P14逐项通过 |
| F12 | bounded资源/清理 | 固定deadline、取消、内存/磁盘/队列/线程/进程所有权、锁与中断、原证据保全；超时不能通过重试刷新 |
| F13 | 死代码/文档清理 | feature矩阵、静态引用、工具/包/测试引用、替代fixture；删除后全套自动回归和包清单无回归 |

## 6. 完整 Mailbox v1 逐项验收矩阵

以固定Go协议全8节和实现为准。下面的精确数值/行为来自该发布合同，不使用旧Rust协议推导。所有行当前为 `unverified`，本次 `not_run`。

| ID | 合同 | 必须覆盖的差分/故障 |
| --- | --- | --- |
| P01 | 字面schema与记录 | `Lycheedev mailbox protocol v1`统一protocol/schema/transport/槽TOC；HEAD/BODY/receipt/INPUT/Bootstrap kind=6跨语言编码、尺寸、数值、损坏/未知版本拒绝 |
| P02 | 当前根/路径/发布 | runtime指令/RIP→Lua state→globals→LycheeDevInternal→Mailbox；不私有堆暴扫/磁盘固定地址/旧地址回退。Forever短读前缀、按页补读、不跨缺口拼接、按根去重、多有效根歧义、scanComplete与成功读取分开 |
| P03 | 身份与公开空map | installation/PID/创建时间/HWND/product/build及GUID/runtime/owner/fence固定；receipts/bodies各有schema元数据，nonce/ticket32位小写hex，精确缺键仍缺失，私有map/Snapshot不混入元数据 |
| P04 | 两正式绑定 | wake Ctrl+Alt+F12→F10、reload F11→F9默认择空；支持Ctrl+Alt+F1–F12且两键不同；读取实际绑定，不覆盖玩家/override；缺任一报input_binding_unavailable；reset只补未设置 |
| P05 | INPUT授权 | sample年龄≤500ms、领先≤100ms；上次输入后样本须晚100ms；escapeLimit=0；发键前同发布/身份/路由/键位复读；回调加载槽前再次检查。战斗/加载/未登录/secret不可读拒绝 |
| P06 | 焦点与物理输入 | 安全EditBox只透传两实际组合键，原handler/其他键/文字/光标/焦点保持且恢复；密码/secure/protected/forbidden拒绝接管；INPUT不依附UIParent。固定HWND PostMessage，不抢前台，最小化拒绝；物理键按下等待、不强制释放；零Esc/Enter/聊天/盲发 |
| P07 | bootstrap/doctor | 最小启动核先加载；失败仍有BootstrapMailbox/手动reload；静态bootstrap不授权输入；默认doctor只读，逐state/detail/nextStep，与顶层doctor共用。500元数据≠500启用；inputAttempt诊断非执行回执；DISABLED保留slot_addon_disabled |
| P08 | 首装/升级/旧200槽 | doctor --fix同目标复用受管安装/原CON/锁；不抢他项目。无可验证二键INPUT→manual_reload_required，不盲打/reload；未发现500槽要求重启，不循环reload。已知有效旧200身份仅升级判断；二键旧名按实际INPUT核验，三键手动。旧业务信封/receipt/BODY不获兼容授权 |
| P09 | 完整交易与容量 | bind精确回执；execute固定request/ticket/源码/预算/策略；prepare→challenge→commit完整目标；只有执行键commit进入业务；HEAD/BODY精确核验→confirm→release。一般四槽、每runtime500已加载槽不复用，不足完整操作时持久reload→重绑同请求不延预算；禁止换成旧Rust RUN/RECEIVED状态机 |
| P10 | 结果与cleanup | reportState/report.ok/cleanup分离；有限binary64可往返，secret/非有限拒绝；同完成request只读原结果。release已发送无精确回执先观察，5秒后在原期限持久一次cleanup reload；不重发release/业务，核验旧runtime退役才记runtime_destroyed，与released区分 |
| P11 | 日志与未知输入 | intent先落盘；not_sent/submitted/uncertain准确，submitted不等于执行。缺outcome/时间仍不重发；原receipt恢复后用当前INPUT uptime建一次持久上界，再等>100ms样本；反复resume不移动上界/不延期限 |
| P12 | 读取恢复/诊断 | 发布路径改变丢快照按既定调度重读；已提交reload等替代runtime时临时空根/旧字符串布局可继续原观察，普通布局/协议/配方/身份错误保留。发键复核共用错误trace；只有明确not_sent且0消息可再准备；firstReadFailure仅原读取逻辑路径/长度/准入/guard，无额外扫描、地址/payload/nonce泄露或猜测 |
| P13 | 生命周期/策略 | status只读、resume原project/CON/request串行；wait-seconds只限当前调用。初bind未确认关闭意图可同CON新鲜INPUT reload清旧runtime，不绑定替代/重发bind。opaque保留未知副作用；显式observation仅验证runtime更换后限次新ticket，原不确定事实不抹去；手动等待未发键不消耗后续连接预算，未知历史预算不重置 |
| P14 | 多实例/共享安装 | 每进程独立runtime/CON/owner/fence，短发布锁+持久预约，等焦点/执行不持锁；一次wake最多本runtime一封，外部槽不授权。共享磁盘更新不代表各进程已激活；doctor --fix只恢复所选进程；七产品目录/Compat与独立两beta身份完整 |

## 7. 性能验收矩阵与采集合同

所有路径当前 `not_run`。先证明F/P正确性，再判断性能。每个必需场景Go/Rust交替各至少20个有效样本；cold/warm各自独立20组，明确如何冷却/预热及缓存身份。失败/超时样本保留并使正确性门槛失败，不丢弃后只统计成功快样本。固定CPU/电源/负载/机器、工具链、binary hash、输入和输出哈希；随机或交替顺序减少系统偏差。

判定：每路径每cold/warm组的可控耗时中位数 Rust/Go ≤0.30；同时披露p95、全样本、CPU、峰值内存、I/O、总wall time及分阶段排除项，不能用总平均掩盖未达标路径。基线近零/低于计时分辨率则批量重复等价工作扩大测量窗口，仍保留原工作量。不得用删功能/校验、少数据、提前终止、漏cleanup、扩大预算制造达标。

| ID | 路径与必须包含的本地阶段 | 外部等待处理 |
| --- | --- | --- |
| B01 | CLI启动/describe/parser、选择与项目pin、持久化 | 无下载；独立测量启动成本 |
| B02 | Source同步后本地解包/索引/完整cold分析/query/refs/context/flow/batch | 下载单列，Git/CAS相同；worker启动/解析/校验/关闭含在内 |
| B03 | Source warm/session复用、缓存失效、异常后下一请求与close | 分开冷暖，不把cold初始化挪到计时外后称cold达标 |
| B04 | CASC本地定位/DB2 decode/table/scan/Hotfix/SQL effective、关系与资产转换/export | 原数据完全相同；网络download不计比值，本地hash/解码/验证/写盘必须计入 |
| B05 | 远端provider已捕获输入的本地业务与并发调度 | 固定输入replay测可控段；真实网络总时长另外报告，不把服务响应变快归功Rust |
| B06 | Live定位/受控读取/协议decode、日志/回执/结果/cleanup编排 | 离线fixture和Windows自有进程先做；游戏等待与必要输入节奏分段记录，真实游戏取样放最后 |
| B07 | addon安装/500槽清单、升级/恢复、包启动/证据archive/verify | 固定同磁盘数据和状态；下载单列，校验/清理保留 |

证据记录最少字段：caseId、phase、cacheMode、Go/Rust commit和sha256、machine、input hashes、iteration/order、start/end、controllableMillis、excludedDownloadMillis、excludedExternalWaitMillis、wallMillis、resultEquivalent、exit、CPU/RSS/IO、logs、结论。每条排除时间有可审计phase边界，不笼统减去慢阶段。存在缺功能/校验不等价即不得比较为通过。

## 8. 文档与代码清理规则

- 本次保存旧README与旧工作台DESIGN到`docs/history/`；原证据与历史失败不删除，旧toolkit Markdown顶部统一标记历史地位。
- 后续清理逐项记录：路径、生产/feature/测试/打包消费者、替代实现、删除理由、受影响测试、结果。无消费者证据不充分则保留并标待核查。
- 优先审查旧LuaLS禁用feature链、旧WGC/optical、旧协议/输入版本、重复生成文档；不预判luac/tree-sitter全删。不删除Go基线oracle和仍用于差分的fixture。
- 当前文档不继承旧“latest固定2.5.1”等发布目标，也不授权变更任何标签；未来发布合同另核当前用户授权。

## 9. 全量实现后的四阶段串行循环审计

用户要求的执行顺序是下列四阶段，每阶段均循环，并非各审一次。阶段内执行：独立审计 → 记录可行动finding → 修复 → 受影响及必要全套验证 → 独立复审。当前阶段收敛后才进入下一阶段；每批改动及时推同一Rust草稿PR，更新本文件的证据与剩余项。

1. **架构、全部业务bug、Skill及全部代码相关问题。** 包括自有unsafe清单、crate禁用策略、必要Windows安全边界和依赖unsafe范围的独立审计。 以固定Go业务/协议合同为准，覆盖错误恢复、诊断、输入、平台/打包路径，不能只看编译或单元通过。
2. **测试覆盖充分性与冗余测试。** 固定Go3.2.0测试也是行为场景oracle：建立Go测试→Rust对应/不适用理由/未覆盖映射，保留错误恢复、未知结果不重放、事务、取消、协议、空map、槽迁移等有效边界；不机械复制冗余平台夹具或旧死实现测试，不以数量堆覆盖。 检查关键分支、失败/恢复/状态组合及真实断言质量，补有效回归；只有证明无独立价值才删重复测试。修后实测覆盖，保留分母、排除项、平台范围与报告；覆盖率数字本身不是充分性证明。
3. **死代码专项。** 核查真实消费者、动态入口、feature/平台条件、CLI/Skill/生成器/打包与工具调用，确认后清理并回归，重复直到无新可行动项。
4. **前三维度联合全仓审计。** 架构/业务/Skill、测试充分性/冗余、死代码一起看，循环修复和验证，直到当前固定树无新可行动finding。

每轮登记stage/round、被审commit或tree、独立审视范围、finding、修复commit、验证命令/产物/结果和复审结论。修复后旧树的无问题结论不能转给新树。停止条件是已覆盖约定范围、已知可行动finding闭环且独立复审未产生新可行动项，有证据支持；不宣称数学意义零bug。需要用户/实机确认的例外只登记到本文最后，保持未验证，不等待它们才推进其他可独立工作，也不将其算通过。发现跨阶段新问题时补修与受影响复审，不为遵守形式顺序掩盖已知缺陷。

### Source全仓固定真实输入验收

不得仅用小fixture验收。复用历史Retail分支固定commit `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b` 的完整Gethe/wow-ui-source输入：历史记录为4036文档（2551 Lua、1091 XML、394 TOC；32,269,051B），完整XML处理还记录4018 inline maps/sourceFiles6569。重新获取后核对真实清单和内容，不把历史计数当新扫描结果。完整导入/索引/分析后实际执行代表性查询、正确文件/符号/行/源码定位、refs/context/flow、cold/warm、持续session、关闭及资源释放，与固定Go同输入差分。保留全量加载闭包/映射和原预算；旧Retail08部分连续成功与Retail19“运行中”均不能替代当前候选全仓通过。若用户另指定具体分支，另固定其精确commit并新增全仓矩阵，不隐式换输入。

### 独立Source与Skill实际任务质量验收

Source与Skill分别安排新的独立代理，不能由实现者自评通过，也不能只靠静态文档审查。先完成可运行CLI的自动准备；使用固定Go3.2.0合同和真实CLI任务证据发现问题、改进实现与回归，不为审美改变既有业务合同。

- **Source独立验收**：代表性真实查询须准确、易读，能够可靠拿到正确文件、符号、行号及源码；覆盖中文、大结果、无结果、错误、refs/context输出、分页/截断/coverage/uncertainty。记录输入pin、完整命令、预期、原始输出、定位证据、问题与复测；不将实现者的解释当成CLI输出证据。
- **Skill独立验收**：由新代理实际按Skill完成调查，检查入口发现、参数正确、步骤简洁、恢复边界和无旧协议；Source优先使用工具内固定版本来源，不随意改搜GitHub raw代替既定source pin。遇阻应按真实错误/恢复合同处理，不绕过约束。记录实际调用轨迹、失败点、修改和独立复用结果。
- 两条验收纳入第一阶段架构/业务/Skill循环，并在后续测试充分性与综合审计中复查；CLI不可运行或依赖缺失时记blocked，不以模拟回答判通过。需要真实游戏或用户配合的部分仅移至最后清单，其余独立运行。

## 10. 本次交付与验证记录

本次范围：README双语、DESIGN、todo、handover、AGENTS优先级与旧文档历史标记。没有产品代码/配置/依赖/发行修改。Go tag与Rust调查HEAD已核对；文档链接、大小写冲突、唯一handover、diff范围和空白检查在提交前执行。首批文档提交`69827626bff23649898527c7a39200bf1dc5d93c`已推送，独立[草稿PR #14](https://github.com/Follen/Lychee-Dev/pull/14)已创建，后续批次复用该PR；PR同时包含旧Rust snapshot，不能将其全部差异归成本次新实现。产品测试全部not_run，性能未测，真实客户端未接触。

后续只读调查已核实公开Emmy upstream `08b0873962e83bb8307e9a81052e3f41578fbc78` archive SHA256 `ae529a06b551dbb301744bea2b58ba94cb3e30340f63a8c83f9335be9086fdb7`，848文件/8,382,174B与历史基底一致。这只证明原公开基底，不证明缺失修改版或原custody字节可复原；新移植需独立身份和验证。

### 首批CLI实现（非完整等价）

固定Go源码独立复核后，CLI main.rs补describe formats/development、命令自身help旗标、format预扫描及token/route错误优先级、空值/重复/不支持参数文案，加入crate级forbid(unsafe_code)。新增4组单元测试，尚未执行：受控cargo入口在编译前被缺失的旧source-identity.json阻断。Rust1.99 rustfmt语法解析及diff空白检查通过，不等同编译/测试通过。未新增缺失命令或声称handler等价；typed numeric完整错误优先级另待差分。公开upstream新依赖身份接入后必须回跑本批测试。

工具链已找到并复用Rust/Cargo1.99.0 Linux；未新增安装。固定Retail源码已实际下载，PAX commit匹配31c7f7b，4036 Lua/XML/TOC及32,269,051B均已重验；这只是输入准备，不是Source分析成功。

### 固定Go oracle与自动Windows入口

新增`tests/rust-parity/`：固定84命令、7产品、55 addon路径/字节hash和359个Go测试文件清单/hash；只静态读取固定Git对象，不执行仓库Lua/Go或配置。18项runner自身测试及fixture认证由作者和协调者分别实际运行通过，fixture SHA256为`78e04b069c1760da67caf7dc192fda93b51b56df5769c479c530554496bb363c`。这不代表产品等价或359个Go测试已映射/运行。真实diff仍退出1，差异不allowlist。共享工作区的产品7/7记录含尚未提交的另一批修改，已单独保全，不充当tests-only commit通过。

`.github/workflows/rust-parity.yml`新增仅rust-refactor push的Windows自动静态合同检查，contents:read，无游戏/发布/密钥；即使PR合并引用不可用仍能触发。对每个精确commit保留差分artifact，候选不等价时保持失败。Windows Rust编译/运行任务待新Emmy准备及custody接入后追加，不能用这个Node作业代替产品测试。

### 七产品与Source-family迁移批次

七个正式delivery product、准确Source别名/已固定Interface基线已写入，Forever共享Source可用于各自区域Data/Changes，但区域Source不可借另一地区，Data/Changes不可混区。项目锁按Data→Changes→Source确定产品。元数据分开API family与实际区域slot；runtime_binding同步最小身份适配，仍要求exact slot/build/recipe/证据，未接默认输入。已有磁盘安装消费者只调整直接受影响测试和注释，未借本批宣称完整安装或Live等价。

协调者独立核对固定Go products/pins合同并运行静态oracle：产品7/7精确字段对齐，整体仍FAIL（命令/flags/addon等缺口仍在）。新增正反向pin、持久化、七目录/schema、元数据和ABI隔离fixture；rustfmt与diff检查通过，Rust运行测试仍因旧custody/Windows条件blocked、未运行。Source-family行为的静态报告保持unverified，不能拿catalog7/7当行为通过。

### unsafe策略首批落地

core/data/delivery/live/source五个库根已加forbid(unsafe_code)，连同前批CLI共六个自有业务crate根守卫。Source单元fixture唯一Win32 FFI调用移至Windows filesystem最小有界安全wrapper，新增ABI/缓冲区生命周期与独占线程说明及Windows单元测试。静态检查区分注释/字符串、unsafe代码/属性/extern/impl和汇编，六项检查器自测由协调者独立重跑通过，当前扫描记193个unsafe构造+1处汇编宏、分布30个Windows边界/fixture文件；这些仍待逐处安全审计，不因inventory检查通过获豁免，第三方依赖也未审计。

WindowsCI新增同静态守卫/自测。rustfmt解析和diff检查通过；产品编译、wrapper的Windows运行测试仍未运行，受旧依赖身份/平台前置阻塞。未新增业务unsafe，不放松任何existing输入边界。

### 命令元数据/参数arity与Go测试映射

82个共有命令的元数据/顺序/flag已按固定Go对齐，7个Go不存在的旧公开入口移出commands.json，保留内部handler及有独立历史价值的生命周期测试，不假装它们已是新协议验收。live doctor/source batch仍缺；refs/context/session及auras/summons等有派发但完成证据不足的入口未擅自扩大宣传。115个flag改为明确arity，修复enum值/说明文字启发式问题。协调者审阅新的contract.rs并更新受完整hash保护的静态adapter，新增Rust Unicode scalar解析回归，24项Node oracle/CLI元数据测试实际全部通过。六项新Rust合同测试未运行。

真实静态diff仍FAIL：命令/flag/arity为84对82，仅缺2、共有项差异0；实际advertised为75，缺9、无额外；addon仍60路径不同。旧Skill七入口仍待正式Skill迁移，不把当前包/Skill说成完整可用。

`tests/rust-parity/go-test-map.json`新增首批22行证据映射：33个固定Go函数/18个hash固定文件→26个Rust测试引用，2 mapped-unrun、17 partial、3 uncovered，另有6组恢复/事务/取消/空map/槽迁移优先项。全为未运行/部分/未覆盖，无behavioral-pass，行数不是覆盖率。它是记录文件hash的审查快照，之后代码改变需重审。该映射实际找出metadata version-only/优先级差异，后续修复正在按Go准确合同处理，不能把Rust额外严格行为自行定义为兼容例外。

### 安装身份优先级与Source query默认值修复

固定Go selection与records是两层合同：先按首个非空flavor文件、version、受catalog支持的已知目录解析身份，再由安装消费者检查同canonical product/实际slot的catalog build冲突。本批分开实现两层，修复version-only安装、首文件优先级、区域slot、parent-only launcher catalog及canonical ProductCode/CatalogProduct/identitySource；新增30个文件场景、20个纯解析/安装边界场景及序列化/计量断言。仍有明确后续缺口：catalog BuildKey/CDNKey和product token校验未对齐；不宣称完整安装元数据等价。测试仅rustfmt解析/diff检查通过，产品Rust测试未运行（受控构建正在被新Emmy依赖测试占用；未抢占lease）。

source query默认limit由10修为固定Go的50，显式1..200边界保留；新增默认/显式/非法参数回归，未执行Rust测试。此项不代表Source查询结果质量或全仓分析通过。

公开Emmy新补丁已实际通过一次库级cargo check，不等于旧私有修改版复原或产品测试通过。新依赖身份/对应源码工具正接受独立审查；发现并正在修复空目录影响include_dir!但未计入identity、recipe runner失败状态及完整recipe前后验证、准备遍历有界性等问题。全部修复及回归封存前不得宣称可复建验收通过。

### 安装catalog校验后续修复（独立静态复核）

补齐固定Go records的Product token（非空、≤128、小写字母/数字/下划线）、Build/CDN Key（32位小写hex），inactive/未知产品行也先验证；保留有换行的空catalog。独立审查指出原错误码不等价后，格式/UTF8/NUL统一records.invalid_metadata/4，列/行/字节上限为records.metadata_limit/3，并增加精确断言。更新core与Live两个受影响fixture，包括CN API family为forever而区域slot仍wow_cn_beta。独立审核已复看源码，rustfmt/diff通过，Rust测试仍未运行；24项Node合同/oracle测试及unsafe守卫重新运行通过。

审核同时发现另批消费者缺口，已列入继续实施：Delivery尚直接inspect精确client，缺Go完整game-root有界Discover/Select；addon status直接序列化内部安装结构，额外公开displayName/channel/metadataBranch/interfaceKnown。应共享Go形状DTO并保留内部身份字段，不能把旧passive固定目录扫描或Data旧flavor parser当完整实现。

### 公开Emmy最小可复建快照与独立Windows产品CI

新依赖recipe `third_party/emmy/` 固定公开基底08b0873962e83bb8307e9a81052e3f41578fbc78、archive SHA256 ae529a06b551dbb301744bea2b58ba94cb3e30340f63a8c83f9335be9086fdb7；851文件/8,423,416B的新patched tree为1c4aef41afa70a1edec0f5714902e6221e75cb204d5b8e090fd017ce982962a0，明确historicalPrivatePatchRecovered=false。Cargo路径/构建receipt/对应源码与license工具切到新身份，不伪造旧私有manifest。recipe和完整支持脚本闭包前后认证；文件/字节/路径/目录集合有界，空目录也不能暗改include_dir!输入；patch用有界精确上下文应用，无运行upstream配置。准备采用独立owner scratch与超时/中断证据，保持现有cargo渠道/lease规则。

作者36项Python准备回归通过，真实新目录archive重建证据75cd3bdb-2728-4203-b004-c254ce0710de；集成与旧runner的24项Node检查由独立审核者实际重跑通过。真实对应源码stage内prepare --check和check.mjs --verify均exit0，源/目标tree及recipe一致，证据27480fa0-28e1-4a9b-bdb2-07596c607ad7；recipe hash 523081f23ae404e9a5945974b46d5143d981a141821e5613b4f6d20d47c8179e。发现的空目录、Windows大小写overlap、准备无界、runner假成功/丢日志问题已修复并复审。以上是来源/准备/封存证据，非Windows产品编译或双fresh-home离线重建验收。

冻结最小API快照曾通过库级cargo check（328a3d4b-4bba-4baf-855d-6dd15d7ab827）；上游--lib冷测试编译超过600s，真实失败与回收证据2eb8c586-2d74-4fff-b479-90dde9bd4043，未运行契约测试。更多binding context语义审计修复仍在隔离next-stage，本批不宣称等价恢复。将先用实际Windows产品编译暴露API集成错误，再持续补齐语义与公开API独立integration回归；不能让工具准备无限推迟产品验证。

Windows新增独立windows-product-foundation job，无needs静态差分门槛：官方Rust1.99、固定recipe下载/校验、Node/Python自身测试、受控locked fetch、workspace all-target check、client metadata/CLI contract/Source query参数unit fixtures。各执行组在依赖准备成功后独立继续，保留真实失败和每次receipt/log；不运行游戏、真实输入或整个Live integration。已做YAML/命令路径/筛选/权限独立静态复核，实际hosted结果等待此批精确commit；不得把配置存在叫测试通过。

### Windows首次执行与准备器平台修复

精确commit2f42f1e的run38618178216/job116038552475实际安装Rust1.99 MSVC成功，但Python3.12.10准备器在首次读取files.json时误报emmy.file_changed；Cargo未开始，产物测试均未运行。该失败发生在scratch创建前，完整job日志保留，不能说已有构建receipt或产品通过。独立比对CPython源码确认Windows path stat的ctime=创建时间与fstat ChangeTime不同，path stat还按文件名补可执行位；DirEntry.stat则可能提供零dev/ino/nlink。

本次最小修复只规范化Windows跨接口比较中的ctime/权限表示；path/path及handle/handle仍比较完整字段，跨接口保留真实dev/ino/类型/大小/mtime/nlink/attributes/birthtime，差异诊断有界。目录扫描改用fresh path.lstat。作者42项完整Linux准备测试与新增immutable .exe/utime单项通过（共43），独立审核者6项平台stat模拟回归通过；实际Windows重跑待该修复commit。修复文件从隔离stage发布，正在运行的旧Linux库check保持原recipe不被改动；两者证据不可混用。

### 共享安装选择层、公开DTO与诊断合同

新增core installation_selection：固定Go的一层game-root/精确client发现与选择，先有界枚举最多257项判256上限再读候选元数据；marker/knownfolder+exe、排序、partial observedProductCode、missing/ambiguous/product-mismatch/conflict及诊断context均按Go。Delivery三个安装消费者接同一受控selector，公开client DTO仅Go字段；Live按自己的Go固定目录发现语义共享DTO和完整folders，不擅自扩大扫描。统一纯UNC显示转换，IO仍使用原extended路径。

独立复核实际发现并修复三项：全命令安装错误的query/selection stage投影；issues.reason的Go原文、bare clientText sentinel和test-track判定优先级；Live按输入scope去重，保留Go root与唯一knownchild的两条观察。新增原因/observed黄金表、clientText边界、公开envelope与DTO/取消/范围fixture。CLI场景补足Go要求的release/output参数，并实际补齐Delivery必需参数/resume前置检查，避免不合法参数误走安装IO。测试映射附加固定Go测试文件和当前Rust文件hash，不改历史执行状态。

本批Rust测试仍not_run，rustfmt语法解析和scoped diff检查通过，已做独立静态审查；Windows CI将增加限定selection/Delivery/假窗口passive/公开error envelope测试。当前aa86ef86的真实Windows执行只包含之前快照：公开Emmy准备、Node测试和locked fetch通过，Python fixture仍有失败待日志调查，workspace实际编译进行中；不能将它归为本批产品通过。CI不再自动取消正在执行的旧批次，新批排队，保留进程回收与终态证据。

### 首次真实Windows产品单测结果与剩余构建门槛

aa86ef86/run38618929588 artifact11922486642（ZIP SHA256 ffbc206b02ef5413747aa6e41001f5831c0a939945243d00302468da674f561c）已下载逐命令核验：metadata17/17、CLI合同6/6、Source默认limit1/1实际通过，三次Cargo exit0；这些仅属于aa86快照，不是后续07df安装选择或全Source验收。默认产品crate已编译至CLI并运行这些单测。all-target Cargo exit101，Source历史example有ObjectOrigin类型与binding_decision缺方法两项错误，待适配。

四组受控命令仍整体FAIL：宿主VS Enterprise14.51.36231的vctip.exe不符合旧Community14.50精确helper策略，owner按未知残留进程强制回收，均tree/root/handles已回收但不得把强制回收当完整受控成功。保持门槛，新增独立只读Windows工具身份观察job，先采集exact path/hash/bytes/version/有效签名供审核；不会按路径名或自动发现就放行helper。

Python准备43测试中1失败/1平台条件skip：即时同尺寸写入并不保证Windows可观测时间戳变化。本次仅改fixture为真实内容写入加显式不同mtime并断言确已变化，另加真实长度变化case；生产guard不放松。44项Linux准备回归通过，实际Windows待重跑。该test文件从隔离stage发布；云exec服务断连导致正在运行的独立Emmy测试无可信终态，普通lease/证据保留，不根据新PID namespace中看不见进程就清锁。后续新语义的真实运行安排独立Windows host，不借更换本地target绕lease。

### Emmy 上下文扩展与 Windows 第二次真实结果

新公开上游补丁候选 tree 1b8a8da7209996539016f40ad1ce4ea10a0e859ddb2664658db3084fb2e74d99，853文件，提供35个独立integration回归（context20/cardinality9/checker guards6），修补model/session绑定、冻结对象身份、成员与多返回值不确定性，以及公开binding_decision API。历史example改为正确区分Lua/XML ObjectOrigin。已报告的局部问题经过独立静态复看，源码准备认证通过；精确候选编译和35测试仍NOT_RUN，新增独立Windows job执行。managed多返回参数、完整return-flow、环境结构/callback/nominal receiver以及泛型/require/诊断传播仍有未覆盖或不可用边界，不宣称Source语义收敛或全仓通过。

固定07df35f3/run38620576235产物11921949297，ZIP SHA256 996210d0be2ce8b47dd06979923137baef9e1662d6f6733435ef11f3b9f7eedd：metadata18、CLI安装envelope3、CLI合同6、Source参数1实际断言通过；CLI合同组受控命令也完整成功。selection9/10、Delivery1/2、passive10/11，各失败为普通slot公开DTO错误期待8字段；固定Go对空catalogProduct使用omitempty，实际7字段正确。本批将三处改为精确7键集合断言，不修改产品输出或减小校验。其余受控组仍有旧helper强制回收FAIL，不能仅凭内部测试成功判整组成功。

只读官方Windows身份run38621626563确认Enterprise vctip.exe SHA256 41fe46f8a0b41f6dd94c68c85fba20a10aacb662a9ef04784a632e37f3692371、514528B、14.51.36260.0、Microsoft有效签名。已独立审查的最小策略支持保留旧Community精确版本并加入该精确版本；未知残留子进程仍强制失败，新增真实Windows生命周期/未知进程拒绝回归。实际新helper回收结果待新CI，不能预报通过。

云失联测试9da7dda2的支持文件曾被短暂改写后恢复，因此已标记external-invalidation；即使后来有成功终态也不可验收该运行。原证据/lease保留，所有新源码与支持候选在隔离stage冻结后按同一提交发布，由新Windows host验证，不清除旧lease或假定进程已回收。

## 11. 最后执行：需要实际配合的测试清单

**本节必须始终是handover的最后一节。** 只有前述可独立完成的实现、离线差分、自动Windows测试、封存候选和可自动性能检查就绪后再约实际配合。当前全部 `not_run`；以下“结果”故意留空待真实执行填写。每次先固定candidate commit/exe/package/addon hash、产品/build、所选安装/PID/创建时间/HWND及授权操作；缺客户端或用户未就绪留缺口，不缩范围。普通Windows API单元/自有子进程/合成窗口CI不在本节。

| ID | 配合/前提 | 步骤 | 预期证据 | 结果（实测后填） |
| --- | --- | --- | --- | --- |
| H01 | 用户指定真实安装和角色；七产品逐一可用 | Retail、Classic、Classic Era、Titan、Forever Asia Beta、Forever CN Beta、Anniversary独立identify/connect/execute/report/cleanup/disconnect | 同candidate各产品身份、完整精确receipt和清理；两beta不按同build合并；不能以Source/Data成功代替Live | |
| H02 | 选定未安装或旧200槽真实客户端，用户可手动reload/重启 | 首装/升级后doctor；无可验证INPUT时手动reload；未发现500槽时重启；原CON resume | 未盲发输入、原目标/CON不变，旧200仅升级识别，500发现/启用/加载事实分别记录 | |
| H03 | 用户允许安全编辑框/聊天焦点和键位冲突场景 | 测两实际绑定、已有绑定保留、聊天/普通EditBox透传、secure/protected拒绝、UIParent隐藏、最小化和物理键按住 | 不改文字/光标/焦点，不抢前台，不Esc/Enter；恢复handler，INPUT/回调/精确receipt各自证据 | |
| H04 | 同安装双实例、跨build并行及混合多实例就绪 | 分别连接/执行/关闭，交错共享槽发布和选定实例reload/doctor --fix；至少Retail双实例，其他产品依声明范围补齐 | 独立身份/owner/fence/CON，短锁预约不串线，其他实例不被授权/升级结果冒认 | |
| H05 | 用户允许真实容量与reload/relogin验证 | 连续运行触发500槽完整操作容量边界，先日志后reload/重绑同请求；reload/relogin前后验证 | 旧槽不复用、预算不延长、目标固定、exact报告与清理，运行时退役明确 | |
| H06 | 用户同意可控故障与恢复窗口 | bind无回执关闭、submitted/uncertain中断、发布替换、release无回执5秒cleanup reload、退出/重开CLI、opaque与observation区别 | 无未知业务重发；原request/ticket/期限/attempt保留；报告可用和cleanup分离，退役后才runtime_destroyed | |
| H07 | 所选角色可临时设置槽禁用并恢复 | doctor读取真实DISABLED诊断；新无owner runtime读取不存在receipt/空BODY；启动模块失败诊断的安全受控场景 | slot_addon_disabled、inputAttempt不冒充执行；公开map元数据与缺键正确；bootstrap不授输入；保存恢复步骤与结果 | |
| H08 | 同机同数据Go/Rust正式候选与用户允许的只读/有界Live采样 | 按B06独立cold/warm交替重复，真实游戏等待分段；保持同业务/输入/校验 | 原始计时/输出等价、excluded阶段与总wall时间，未达0.30照实失败，不影响前面可自动工作 | |
| H09 | 若既有“十分钟空格”人工活动仍由用户确认需要 | 先确认当时任务目的和安全场景，再由用户实际配合；不把历史计划当持续输入授权 | 人工行为与工具输入分开标记，明确时长/目标/结果，不冒充CLI输入或协议通过 | |
