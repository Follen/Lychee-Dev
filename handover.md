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

### Data 默认安装消费者与真实 CLI 可用性执行入口

Data 两层消费者现共享固定Go安装身份：target需要真实client选择，pin读取先接受有效catalog-only CASC root，再检验canonical product/实际slot/full build一致性；local query/navigation默认读者已接该路径。公开InstalledBuild仅Go五字段，Branch不新增准入限制，配置key/UTF8/integrity错误顺序与encoding-size上限1<<50对齐。临时selector allocation用RAII释放，metadata只累计实际读取字节，保留双次选择更新检查；新增64目录默认预算、真实上限、取消、CASC和区域slot回归。

独立静态复核关闭上述发现，另发现symbolic definitions解析会早于预算检查；本批在dispatch_target入口先检查同inv.control，并加exact/symbolic offline已取消/过期不创建home回归。definitions resolver运行中的持续取消/网络deadline仍是明确后续缺口，不称整个target全程受控。源码语法/scoped diff通过；新增Rust测试NOT_RUN，Windows job将实际跑installed_parity/local_domain_parity与target取消场景。

另加受控构建封存真实CLI exe，以及校验精确artifact后独立owned-process执行version/help/describe并保存原始输出和receipt。此入口用于接续独立Source/Skill真实使用，当前仅配置/静态验证，不是产品运行通过；没有游戏、安装或业务执行。嵌套Emmy job显式RUSTUP_TOOLCHAIN=1.99.0，防upstream更近的stable工具链文件覆盖根目录pin。

### 正式 Mailbox v1 首批可独立测试边界

新增go_mailbox_v1模块，按固定Go实现LYCMEM06/kind1–6有界wire、two-read HEAD/BODY校验、Identity/Receipt、500槽与旧200仅手动激活识别、INPUT readiness/绑定/时钟、可选DISABLED诊断及unknown不重放判定。24项纯fixture与逐项固定Go测试/消费者映射已写入；实际Rust测试NOT_RUN，由Windows独立限定组执行。

独立审核发现并修复三项公开JSON差异：inputAttempt在序列化/日志重读中保留且不恢复host freshness，Receipt零report字段omitempty，旧InputOutcome缺atMillis/messagesQueued为0且unknown时间不授重放。补充旧04/05/07有效checksum wire拒绝、BODY状态/序列边界。新增serde_json raw_value feature用于保留原始诊断字节限额，不新增依赖。rustfmt/scoped diff通过，局部复审无新增阻断；没有将首批谓词称完整协议通过。

该模块尚未替换生产消费者，默认Live仍有locator/input接线缺口。下一批按README逐层替换mailbox decode、native read-only Identity/INPUT与driver；必须保留Selector.Accept次序、二键stale能力证据和joined错误分类。逻辑公开map fixture不等于Lua内存遍历通过；真实游戏/输入仍留最后配合清单。

### 完整 Go Skill 复用与独立声明门修复

恢复固定Go全部22个Skill文件/流程，必要Emmy与候选未实现边界单独标明；打包保留完整references/launcher/agent配置，固定目标与实际candidate describe另存，未实现doctor/batch不能从目标删除。独立审核指出并修复固定commands生成器冲突、整个live/source组缺失仍假绿、Flags行未关联而漏缺参数三项；保留stale回归、整组/根命令/缺flags双入口负例。固定目标生成/检查不调用Go，实际声明检查仍读取指定CLI，二者不能互相代替。

作者与独立审核者分别实跑14项Node：11通过、3Windows条件skip；固定目标84命令/313引用与生成一致检查通过。恢复Go两项Windows launcher路径回归，新增Windows CI实际执行。尚无真实Rust Skill任务质量通过，必须等封存exe实际使用独立验收；本批静态门只证明文档/声明检查能力。session规则按Go生产源码明确：60s空闲（请求中停表）、600s总lifetime、每保留分析worker256semantic batches后仅worker退役，broker可继续准备，非256条CLI请求后broker退出。

### 真实 Windows CLI、Emmy 与基础合同执行结果

固定d27a174/run38624344361的Emmy job116061756336真实编译并运行35/35通过；artifact11923113192 ZIP SHA256 b7845a435a464ad356cefad1323ad93203d020fe74896f95733c2c285026bff3。已下载核receipt41cd5404：tree1b8a8da7、recipe a6cc9d5e25148243e67409442887f98dd31ebe8d81e50682681ba4ed2154e7cb，exit0、root/tree/handles/readers回收、active0；精确核验Enterprise helper由knownAuxiliaryCleanup清理。此证据覆盖35项，不能代替全仓Source/性能。

固定c68d6d85/run38625341832已真实构建42,555,904B Windows CLI，SHA256 2bb446a112671e7576ac6a30b8490ecfdedce56f6e2788fef053821067510b83，version/help/describe实际成功且封存。product artifact11925460902 ZIP SHA256 6464ec42c35ac9b962fd97d1c996c00c2eda8ed03924ab563b052d31bb66b648。metadata18、selection10、Delivery2、passive11、CLI安装3/合同6、Source参数1、Mailbox纯合同24、target取消1共76项通过，各组受控命令成功；这不含未提交Live读路径/doctor/session变更。

all-target仍FAIL：旧example遗漏NativeQueryView::Xml，此批按精确control occurrence处理，不回退任意source view。Data installed14项9过4失败1需真实客户端ignore：3个fixture对Windows TEMP短路径与canonical路径错误比较，1个区域selector错误优先级期望不符合Go；此批仅修fixture，增加诊断/无product/已有pin三层拒绝断言。local_domain_parity被同Cargo前一target失败短路，现拆独立CI组；不得将未跑记通过。失败命令exit101并触严格残留回收仍FAIL，未放松门槛。

### Emmy doctor 和 native session 生命周期有限对齐

doctor改真实内置样本经native worker分析并检查resolved调用/源码行/retired证据；只证明embedded-worker-fixture，不证明luac/session/user-source。独立审查发现删除原发行完整性能力后已恢复release.integrity独立检查，缺/坏资源保留warn，原cancel/deadline不降级。仅doctor summary为必要精确引擎差异，固定Go oracle原文不改。标准执行预算≤5秒，常规故障回收另≤5秒；既有JobGuard附加失败的无界wait仍需后续修复，不能承诺所有OS异常10秒。新增Rust fixtures和隔离scratch home真实doctor CI配置，尚NOT_RUN。

native session本批独立复核：60s idle请求中停表、固定600s lifetime限制在途、5s握手且不延长saved idle、原取消token、空缓存状态idle；新增3个确定性单测，更新两个真实CLI fixture预期。Rust运行NOT_RUN；完整真实session fixture需要已核发行包，本批不会把ignore算通过。256 semantic batch worker退休映射、>90s wire deadline拒绝、坏帧只退handler、精确Go状态JSON仍未齐，继续后续实现。

### 正式 Mailbox v1 生产只读路径接线

现有mailbox reader已委托正式06 codec，真实Lua schema-only map、Bootstrap独立namespace、Confirmation4/INPUT5、BODY精确静态头先验、路径guard之后Accept、正式identity与optional inventory、旧200升级诊断已接入。stale二键样本仅能力证据，decode_input仍要求fresh。25纯合同与19 mailbox_lookup fixture、binding观察fixture尚NOT_RUN，CI新增独立组。

独立审核两次发现guard复核错误可能吞取消/身份变化，分别覆盖原lookup失败及完整record成功后guard变更，均已修复并补拒绝Accept回归；旧release错误断言也按Go joined incompatible修正。当前native::open无RootRule仍locator_not_finalized，有显式旧RootRule则临时migration_incomplete防混协议发送。旧WindowsChannel/四槽driver尚未迁移，不称完整Live可用；下一步按固定Go typed state/journal、写事务、恢复期限拆分，最后替换临时阻断接正式输入。

### 正式 Go addon 资源与 Delivery 模板首批恢复

独立按Git对象逐字节验证：固定Go的55个addon文件/1,035,262B全部恢复一致，正式TOC加载50个Lua且未加载旧Native额外文件，Bindings.xml及3媒体资源均在正式集合。21个旧额外文件仍留待真实消费者迁移，完整库存差分仍失败；不能仅以正式TOC不引用就删掉仍由历史测试/包装工具使用的文件。已列旧协议完整fixture保存与当前有效Go回归分别迁移方案。

Delivery两处安装/更新门槛改正式SlotRuntime，保留release清单/hash验证；六Interface/正式transport/SlotRuntime Loader的1..500全部槽字面模板已与固定Go独立核对。新增4项Node静态合同测试通过并加入CI，其中门槛源码regex仅静态证据；combined_addon实际Rust事务fixture仍NOT_RUN，新增独立Windows组。pool仍v4/modules，正式Go v3/files/marker内升级intent、publish/verify/confirm/retire与旧消费者尚需下一批，Live旧transport会失败关闭，不称端到端安装/执行已齐。

审查另发现静态oracle的Rust lexer把raw Windows路径与合法\0当JSON escape而exit2。本批修raw delimiter匹配和NUL escape，新增6回归；30项Node通过，固定oracle SHA不变。实际报告恢复exit1正常差分：products7/7，缺live doctor/source batch和必要doctor summary差异、21 addon extras如实保留，未verified的arity/advertised不改为pass。

### 固定4036文档全仓实际CLI验收入口与启动握手修复

独立Source验收者新增真实CLI runner：消费封存EXE，在独占scratch home经正常source sync固定Gethe31c7f7b9cc79e56c986b365c06a6afbcf3c9177b；逐Git blob SHA1及SHA256/path/size双digest对独立archive oracle，核全部4049文件及4036文档/32,269,051B。约25个实际CLI案例覆盖derived-cold/warm全仓索引、原文/符号/API位置、中文字面无结果、分页/无结果/错误恢复、static refs/context。独立审核纠正API table行10与Name原文行11区别、call种类/caller/行141，以及超时后not_run口径。11项runner自测、独立真实archive重验及外层故障模拟通过；Windows产品执行待本提交，不能把runner自测当Source通过。

语义/flow/session/Go差分与20次独立cold/warm性能另列后续。普通native研究仍要求已验证release/luac identity，flow/validate会真实执行luac；官方Go3.2 ZIP已核commit3057但不含Rust所需luac，不能将其冒作Rust候选包。正在复用既有官方Lua源码构建/候选打包工具重建精确可复现新身份，保留现有验证而非删掉门槛。

全仓验收者另发现真实native session启动回归：idle状态workerPid0，startup却仍拿它比owner child PID。已改为Status绑定认证Hello.pid，startup再绑定持有child.id，idle/ready形状一致性检查；增加缺/错owner与worker冒充拒绝fixture，以及close→新Helper cold start/new identity/full transfer/实际exit0公测。独立复核关闭；Rust运行NOT_RUN，公测仍需候选release。后续90s和256 semantic批映射继续。

### Windows 产品基础全绿与候选 luac 重建入口

固定4c93953/run38629743958产品job116078018491成功：102项Rust回归通过、2项需真实客户端ignore；all-target check、真实CLI构建、version/help/describe和隔离home Emmy doctor通过。artifact11926168194 ZIP SHA256 e37ffc9699292b71066fb0eefc5ccb8fe1978c81f9f23f7ddac474da028a5fa9；EXE SHA256 e79c5a755c9576e8bea690fd7748fe547dafc53dc2085af58bf5557d42c3e7c1。下载后核17个产品Cargo command exit0及22个owned-process成功receipt，root/tree/handles/readers全回收、active0；另存runner故障注入receipt的预期非零不混入产品结果。真实doctor只证明内置解析/定位及one-shot退休；裸EXE缺发行资源的release.integrity保留warn，不称完整包通过。此证据不覆盖之后8c/f083的新Live/addon/full-corpus。

候选工具现在严格选择明确MSVC14.51.36231/SDK10.0.26100.0，不自动选择latest，清除隐式编译/链接参数；普通cl Job不借Cargo helper豁免。13项Node工具回归经作者和独立复跑通过。新增独立Windows流程从已固定官方Lua5.1.5源码构建两个独立目录，对比完整manifest/EXE/许可证、系统imports，并实际执行7个Lua5.1语法正负案例。实际重建NOT_RUN，结果仅候选，不自动更新旧luac pin。若helper未退出，保留失败receipt并失败，不能删检查过关。

候选包装只复用与当前sourceDigest完全相同的sealed EXE，换luac pin后必须重新编译。assembly-only仍生成受控许可证闭包，但不会将装配称Source/session通过；后续须先独立核双重建证据，再提交新pin、重编产品并执行完整语义/持久会话验收。

### Native session wire 准入与非法顶层字段拒绝

已按固定Go对请求wire期限作有界准入：客户端不发超过90秒的wire期限，服务端拒绝0和超过90秒上界；合法非零但已过期请求保留缓存并按既有admitted规则重置idle。解码失败、wire不符、越界期限及未知顶层字段仅退当前handler，不清缓存或刷新原绝对idle。新增边界/严格解码单测及真实连接EOF后同PID/创建时间/完整缓存状态fixture，独立两次复核关闭未知字段遗漏。

本批仅wire准入，exchange仍受原调用期限（可180秒）约束，不宣称端到端硬90秒或全部Go非法帧都已覆盖。60s实际idle、真实连接fixture、256 semantic batch退休与完整Go状态仍各自待验；Rust运行NOT_RUN，已有生命周期CI组将跑新增单测，完整真实session待重建候选包。

### Mailbox typed state / observation / journal 合同进入编译验收

新增正式channel.v2状态、事务/操作/输入attempt/恢复预算/退役证明DTO、二元事实加错误观察接口，以及只提供租约拥有者接口的journal snapshot/hydrate、结果原字节base64与原始JSON hashchain。22项pure fixture已编写，独立两轮发现并修复joined typed cause丢失、Observation事实与错误不能并存、SlotAvailable→Blocked→Pending及publication cause的精确unwrap/optional-timeout语义。

日志新增固定Go独立标准库生成器与两个完整前驱链黄金（HTML、U+2028/29、原始1.00/-0.0/1e+02）；Python交叉hash核验通过，官方Go1.27.1独立Windows流程会实际生成比对。当前导出进入现有all-target与go_mailbox_v1组，Rust与Go实际运行NOT_RUN。没有durable filesystem writer，尚未接Windows事务/input；B事务和C恢复独立实施中，不称协议端到端已齐。

luac首轮真实Windows run38632703141/artifact11927375965（ZIP SHA256 8082b69a0e80578918c51cc5ccb825b0c9c52919580f782dd81cdc67dd674a53）13工具测试通过，cl自身exit0，但vctip子进程未自然退出，普通Job严格回收并失败。已保留完整证据，第二构建/7例实际检查未运行；将按已验证cl与helper精确身份作最小独立支持，不冒用cargoBuild或将失败产物更新pin。

### 生产只读 Mailbox 与 addon 真实 Windows 结果、cl 最小支持

固定8c9199a/run38630824020产品job116082486009全部成功：157项Rust通过、2个人工条件ignore，其中正式codec25、生产mailbox lookup19、binding观察34、combined addon真实文件事务1。all-target、sealed CLI及真实doctor均通过；EXE SHA256 fce18cb538a3e9745be02c55276c7e76a94700e3aec0730eb994c061c3b849f9。artifact11926507844 ZIP SHA256 25917743512d27e366b2afd19b2141126e6c242b518b50249c18a21782855d24已下载，25个产品owned-process成功receipt逐项核root/tree/handles/readers回收、active0。该树未含typed A/B/C或全仓runner，不把它们算通过。

724edd8的独立Go日志oracle run38633376924/job116090323945实际使用Go1.27.1成功生成并比对；artifact11927796307 ZIP SHA256 57077b2ebaf232b1b855fde0a2317c4dd48fc7a133c353e5fee87302a6e43bee下载后再次核JSON值与黄金一致。此为真实Go黄金证据，Rust journal运行仍待队列。

luac失败后的只读run38633376782核定cl/link精确文件与有效Microsoft签名。新增独立msvcCompile准入与cargoBuild互斥，固定cl/link/helper身份并持文件锁，只接受本次固定C编译shape，拒绝response/替换工具/隐式参数；仅root成功且剩余均为准确已核本Job helper才允许定向退休。原Cargo分支、既有Cargo/helper pins与C# Job/退休核心逐字节不变，未知子进程仍失败。隔离stage纯Node10通过/9Windows skip，独立静态审核关闭；真实cl双重建及新Windows负例尚NOT_RUN。本地冻结support旧字节不用于证明新树测试，通过远端精确候选CI验证。

### 正式 Mailbox 事务驱动 B 进入独立编译验收

新增可注入backend/journal/clock/token的Connect/Prepare/Run/Disconnect状态机、prepare→challenge→commit→HEAD/BODY→confirm→durable原始result→release完整业务链，500槽分配边界、unknown不重放、intent先保存、exact receipt先保存再Consumed、输入事实独立3秒有界flush、release缺回执5秒后保存cleanup reload intent。33项pure fake测试已写，独立静态审核修复preappend取消误poison阻断sticky预算flush、畸形challenge消费/错误顺序，以及A Save按Go先递增进度后检查取消的次序（另2回归）。append已进入后的不确定错误仍强制重新读取日志，不能凭错误码猜无写入。

本批导出进入Windows all-target/现有Mailbox测试组，实际运行NOT_RUN，没有持久化writer/正式Windows输入/生产driver消费者。C恢复仍独立审核，已发现自动reload预算clone丢失Go共享goal的lastObserved/exhausted语义，必须修并回归后才接生产；B恢复入口目前只生成intent，不能称完整恢复通过。Delivery正式publish共享DTO边界待移至core无循环依赖，不复制第二套信封。

### cl 支持第二轮 CI 准备缺口

57fa401/run38634308510在新增Windows runner测试入口因clean checkout尚无.tmp目录而9项ENOENT，编译未开始；不是cl/helper新准入实际失败。修复流程显式创建测试容器目录，并上传故障注入process-test证据。保留该失败历史，下一轮仍需真实全部负例与双编译通过，不将Linux条件skip当Windows通过。

### Lua 编译器候选独立证据通过并固定新身份

0d6b8ab/run38634722358/job116094827790真实双目录重建与7语法案例成功；artifact11927578038 ZIP SHA256 d774fcb89dc970594da26b7a283c9fa20502104c4052e29dba694648c1d91352。独立验收者重新核147文件、完整a/b manifest、86 sealed日志、15进程receipt、精确签名工具身份、两编译root0及helper/全树回收；AMD64仅kernel32.dll且无delay imports，源码COPYRIGHT与LICENSE逐byte一致，3项语法负例exit1为正确拒绝。

据此将release/tools/luac.json整份更新为新可复建候选：EXE SHA256 5aa1605bfea56e6c2961c6b4b0f845a699b43f9b3666dd64f50289c3952d441b、491008B，MSVC14.51.36231/SDK10.0.26100.0；原64cd0d82/487424B旧身份保留Git历史，绝不声称恢复旧字节。这仅同一宿主固定工具链双重建，不等于跨宿主或完整Source通过。

Windows产品CI现将在新pin下重新编译CLI，重建luac并验证精确当前sourceDigest的未发布候选包/许可证闭包，然后真正执行原先ignored的多CLI native session/缓存/关闭/取消/坏帧场景，使用独立保存的fixture目录与owned controller。配置已写，实际候选装配及真实session仍NOT_RUN；其他差分/测试失败不短路这些独立组，不能把包构建等同业务收敛。

### Live / Delivery 共用正式信封边界

将已审的SlotEnvelope、ExpectedEnvelope、token/checksum、ContractError与Lua payload生成器唯一实现抽取至lycheedev_core::mailbox，Live原公开路径全部re-export同类型/函数，原生memory reader与状态机路由留Live。Delivery将依core复用，不形成delivery→live依赖或复制第二种信封。独立复核迁移实现逐字一致、JSON/default/omitempty/Lua转义与边界无变化，7个core加1个Live身份fixture已写，现接独立Windows测试组；运行NOT_RUN。

### 全仓静态 Source 首次真实验收与恢复状态机 C

独立验收71180b3/run38632871866产品job116090164748：全部4049文件/4036文档/32,269,051B逐一SHA256、Git blob SHA1、精确commit/tree及61日志hash一致；25次实际CLI的21验收项通过。证据包含Lua 97/2、API query10/inspect11原文位置、4775候选两页连续无重复、三种exit2错误及恢复、三条静态callsite/context原文和合法UTF8中文无结果。artifact11927883033 ZIP SHA256 41fd9320ed43720d1724b75582e74ed2deff26f405bf9c113018bb2fcea545da，EXE a8d8ffe70fb0790d074f2f2e80b8bfd1b75c580e2ff6cf9e2bccee26893bf80f。cold index87.488s/warm2.607s、首GetAction6.562s/复查1.121s、static refs5.140s/context7.518s为各一次实际命令样本；sync2.153s混合输入准备独列。没有同机Go差分、20次重复或70%性能结论，也不覆盖后续计划/语义/session新代码。

C新增持久预算、reload/continuation、candidate bind→supersede、opaque unknown不重放、输入时间一次恢复、报告与cleanup分离及proof-gated runtime retirement。独立审核修复自动reload与原operation预算分离造成的rollback/exhaustion丢失，并在加载/保存/观察时合并原deadline/maxObserved/sticky事实。取消原因曾在延迟flush后误判，现采用core opt-in真实单调取消时间戳与固定deadline裁决首次原因；祖先/root-to-leaf锁顺序无环、无后台线程，独立flush用scope身份区分。raw AtomicBool保留旧兼容但不能伪称可证明先后，C输入前拒绝该模式。

38项C与5项新core测试、原测试均已写，静态独审关闭所查finding，运行NOT_RUN，现导出并接Windows组。生产console/Invocation仍需tracked取消接线，native backend只读token边界、durable journal writer、正式Delivery发布及默认host输入仍未接通，不能据此称完整Live可用或启动四阶段最终审计。

### Native Source 真实逻辑操作规划基础（尚非执行批次）

新增References/Type/Definition逻辑operation计划与128切批、原始来源hash/精确UTF16位置/请求owner认证；图准备与规划沿同一request预算，不能reset后继续无限工作。独立多轮审查已修普通Lua嵌套caller污染、128之后继续多余扫描、API唯一mapping/短名碰撞、独立environment原文与source混同、XML继承/兄弟owner资格顺序，以及受限scanner空/部分结果伪完整。环境原文独立传输并从明确env pin认证，材料规则v4使旧缓存失效，不绕验证。

11项新计划fixture已编写，独立静态复核所查finding关闭；Windows新增计划及既有native研究准备组实际运行，当前NOT_RUN。输出明确planned_not_executed/executedSemanticBatches:null；尚未接实际操作consumer或256累计退休。XML内命名Lua函数、真实索引符号的virtual-template路径仍需补验，手写end_line fixture不作真实symbol路径证明。当前基础不得冒称全部XML计划/语义完成，也不继承71180b3静态全仓通过。

### 正式槽池与卸载维护状态对齐进入 Windows 验证

Delivery维护改正式v3/files、256KiB manifest、旧64/200→500形状/精确旧TOC、marker内migration.v3/v4恢复、完整文件预检、Consumed/退役保护、冲突exit3及removal.v1。独立多轮审查修复了大小写/重复JSON/null/数组复用造成预约被忽略、取消后仍写、卸载rename覆盖竞争目标、Windows普通/verbatim/UNC身份误拒，以及私有removal资源int64输入顺序。intent链按Go宽松顺序解码，正式安装marker的unknown/duplicate严格守卫未放宽。

必要Windows最小safe封装使用MoveFileExW仅WRITE_THROUGH且不替换目标，保持UTF16生命周期/NUL/绝对路径校验；业务crate仍forbid unsafe。受控atomic writer复用原实现，在写块/sync后最终发布前检查原control。新增真实文件/取消/目标竞争/Go形状恢复fixture及相应独立CI组，Node静态4项与独立复核通过，Rust/Windows运行NOT_RUN。UNC当前仅纯身份fixture，没有真实share测试；checked file-close错误传播仍为后续durable journal所需边界，不把Drop说成已检查close。

旧任意bytes publish明确失败关闭，正式SlotEnvelope publish/verify/confirm/retire下一批复用已抽取core合同；当前maintenance不是完整发送链。21个额外addon文件未删，仍待真实消费者迁移。上述candidate尚未证明全安装/卸载/崩溃恢复等价。

### 独立 Skill 实际工作流验收入口

新增独立验收器先认证固定Go的完整23文件Skill，再经真实prefix发现、包launcher和封存EXE执行固定Retail的代表性API/实现查询、原文定位、静态及普通native refs/context、中文/分页/错误恢复与batch。prefix为明确隔离的解析fixture，不冒称npm安装验证。缺失batch记required失败，仍保留其他实际案例；超时/进程失败后不再启动命令，owned Job证明全树回收后才清scratch，完整日志和身份封存。

独立审查发现并修复两处验收假绿：API正文必须与已认证archive精确10–21行逐byte一致；native不得由空元数据stub或静态同址关系满足，需实际引擎/输入身份/one-shot退休及精确调用点native依据，仍接受诚实partial。17项Node检查器回归已独立复跑通过；这不是Windows Skill实际使用通过。新增Windows真实工作流在候选包成功后独立执行，不被其他组失败短路；当前实际执行NOT_RUN，后续须独立核原始产物。完整Source语义、生命周期、Go性能差分与Data/Live全流程仍另列。

### 持久发布 checked-close 边界

Windows新增consume File所有权的最小close_file安全接口，仅一次CloseHandle且传播错误，不重建或重试已消费handle。core受控atomic writer顺序为sync→原control→checked close→原control→durable replace；关闭失败或关闭期间取消均不发布。独立源码审核已核所有权和首个错误传播，新增独占句柄/duplicate所有权、注入关闭错误与关闭期间取消四项fixture，Windows运行NOT_RUN。错误注入是在正常关闭后返回错误，不冒称实测内核CloseHandle失败；现有遗留Drop未被宣称可检查关闭。durable journal将复用此接口，尚未接生产。

### 0d6 产品真实回归结果与保留失败

0d6b8ab/run38634722276产品job116096315251终态failure；artifact11929632102 ZIP SHA256 329c09ae3c126164b8e064c61e92727520332be40ab8109f1727ea39eec08afc已下载，307份封存日志hash逐一验证。成功组214项Rust通过、失败组另1项通过、2项人工条件ignore；正式Mailbox A+B组82项实际通过，receipt证实root/tree/handles/readers完整回收与active0。all-target、sealed CLI、doctor及全仓21项static验收通过；该树未含C、共享core提取、新维护和新语义计划，不能转借通过。

三组失败照实保留：首个owned-process fixture的PowerShell wrapper在约76秒超时，尚未产生child receipt，不称树已回收；Delivery status在根选择后返回external.io/os2，原日志不足以证明并行fixture碰撞；combined addon在183.40秒耗尽原180秒deadline。旧slot migration无control且全部1500文件重复持久写，ec2已保全量验证/只写变化文件并传原control，尚待精确新树实测；不通过增加产品deadline掩盖。其他独立组没有被这些失败短路，旧失败与未回收fixture保留于artifact。

Delivery测试fixture的PID+timestamp非独占目录存在并发碰撞风险；本批增加进程内序号与create_dir独占、强制相同timestamp的删除隔离回归，并保留原业务断言和分阶段错误定位。独立差分复核仅测试模块修改，3项selection fixture运行NOT_RUN；这是可证明隔离风险修复，不把原os2根因称为已经复现。

### Named Hotfix 本地表解码与受控 ChangesPin

Data dbcache named table现在按固定DBD顺序解码uint64、合法UTF8、有限float、inline/noninline ID、relation/数组；保留decoded/no_payload/not_valid、v1–9布局、完整尾部验证和物理/最新/after-index分页。校验cache/table/region既有ChangesPin身份，选中payload错误先于capture/pin。scan、remote provider、SQL effective和core capture dependency retention仍未完成，provenance不冒称可阻止删除依赖。

独立复审修复三类真实问题：合法大cache不再受默认metadata128MiB错误截断；字段JSON按Go浮点数字与转义计量；ChangesPin贯穿原control到SQLite BEGIN/COMMIT有界重试，取消后rollback而非迟到提交。计量曾把临时集合/scalar Vec累计为保留内存，现按固定Rust1.99节点保守上界及真实生命周期计量，不提高retained上限，补4096字段×200记录/1MiB raw允许页和32×65536 float边界。12项integration、额外typed预算/数字unit及BEGIN/COMMIT锁取消回归已编写，独立静态复审全部所查finding关闭，实际Rust运行NOT_RUN；现分组接Windows CI。外围Metadata::open旧锁等待未改，不声称整个pipeline严格实时取消或Data全量收敛。

### 既有连接 durable journal 与共享执行租约

新增正式channel日志的持久适配器：固定Go共享admission/execution锁命名、完整既有WindowOwner认证、artifacts先于log、sync和checked-close、原始结果bytes、hashchain/replay/checkpoint/retained history、不确定写入poison及readonly查询。既有claim必须已有非空合法log，不能通过丢日志重建链；新建连接仍须未来Book admission的persist-before-claim步骤。

独立审核修复命中历史仍读无关旧段、checkpoint错误加入1024写入门槛、既有claim缺log reseed、LF后额外严格事件大小四处差异。按固定Go保留JSON恰2MiB写成功而reader随后拒绝的上游边界不一致，明确不伪称已修上游；checkpoint直接使用生成hash避免读限失败后索引panic。27测试函数含1子进程helper已编写，覆盖真正子进程无析构退出、锁与持久化故障等；源码复审所查finding关闭，实际Rust/Windows运行NOT_RUN。现导出进入all-target及Mailbox组，不含Book/claim创建退休/default host/物理输入消费者，不称完整Live接通。

### Native Source operation 执行证据与范围绑定

Source逻辑operation现在有Definition/References/Type答案及受封装的scope completion，绑定同一request owner/原文/query/plan digest及逐项index/batch/kind/operation digest；账本借用唯一plan且不接受替换plan finish。真实执行后仍可诚实返回unavailable/partial；graph缺失的静态fallback与API descriptor Type元数据明确executed:false，不给completed批数。API真正Type查询仍未实现，不能用descriptor假装查询完成。

独立审查发现并修复静态占位假计数、仅凭Vec长度可伪造receipt、XML具名子Frame handler错误归父virtual template及两处UTF16错行fixture；fixture控制还修为复用原admitted control，不放宽身份守卫或reset预算。真实XML索引产物覆盖named/unnamed子对象边界，规则v5使旧结果材料缓存失效。新增/修改Rust回归运行NOT_RUN，已有Windows计划/研究组将覆盖；这不是完整语义或性能通过，incoming projection仍有重复工作。

尚未接256生命周期退休。后续必须分清fixedGo admitted/attempted batch计数与completed证据；Go在执行前/请求批次层累计且失败退休，不能以partial的0completed令session永久存活，也不能为限额伪报执行。需要按真实plan预准入、超256先退analysis/要求full snapshot、恰256响应后退analysis，保broker现有idle/lifetime合同及独立实际生命周期验证。

### ec2 完整产品组成功与真实 native session 初验

固定ec2a09b/run38637995018产品job116108268721终态success；artifact11930054873 ZIP SHA256 94a6052d8e411e05e1975292404de13a190d1c92823f02af0401a3294ba05ecc下载后核445份封存日志hash。315项Rust通过、2项ignore，包含Mailbox A/B/C与共享边界121、slot maintenance17、combined/rollback/removal组；sealed EXE SHA256 3f02ef9910372a24c5a6c554434719240d96cfebc0ec2a1a5d0ad80d51931411。全仓static CLI、真实doctor、luac重建及精确source候选/许可证装配均完成。

真实multi-CLI native session集成首次执行成功（29.64s），覆盖同快照复用、关闭退休、取消/期限与坏帧等原fixture；取消响应样本68ms、期限样本818ms仅该场景观测。session owned receipt确认exit0/root/tree/handles/readers完整回收/active0。此证据已由独立验收者重核源码DFS digest、105包资源、445日志及5份公开结果/capture，40Lua+TOC+独立API原Git对象逐一认证；坏信封保持owner/cache、取消/期限后重建恢复、close后新owner均成立，但不是完整4036语义、256新准入、实际Skill使用或性能通过；后续journal/Hotfix/consumer提交不继承该树运行结果。

复核发现CI过滤器native_research::tests实际0测试通过，不能记为XML研究fixtures通过；独立plan组6项确实运行。立即合并为真实native_research::模块过滤器，覆盖plan、projection、load_context、reply_assembly各模块而不重复计数。新过滤器实际执行仍待下一精确树；保留原零测试证据，不改旧结果为通过。

### CI YAML 准入错误修复

复核发现1d0eaec、be99829、0b26b9a的产品workflow在进入runner前即失败：新hotfix::过滤器后的空格使未引号run标量含冒号分隔，被YAML拒绝。这三树产品测试完全未运行，不能把已推送或静态审查当CI通过。现将含该形式的命令改block scalar并用本地YAML解析器实际解析完整三job工作流；新native_research::过滤器也同样保护。等待GitHub精确新树确认调度和实际测试，不掩盖原准入失败。

### 正式 typed slot publication / 消费确认 / 退役

Delivery新增共享core信封的typed API，完整pending manifest→payload→committed manifest三写点、同nonce不可变、实际有界route检查、Consumed确认及独立process/runtime退役。旧任意bytes发布仍失败关闭。短lease先维护锁再正式publication锁，原control贯穿，持至physical burst后应在receipt wait前显式close；持久reservation保护后续等待。cleanup按Go仅依已认证路径/槽池和caller确切证明，不被主addon receipt缺失/内容变动或旧Rust claims错误新增阻塞。

core只抽同namespace lock-only维护lease，旧InstallationGuard注册行为不变；ExclusiveLock消费File后仅checked close一次，不先unlock、失败后不重复关闭。两锁关闭都尝试并保有序错误；原atomic writer逐byte未变。独立新上下文审核六路径，29项Delivery+3项core回归及Go场景映射源码未见本批阻断，实际Rust/Windows测试NOT_RUN，现分组接CI。尚无正式生产driver消费者，调用方仍须在业务失败也显式关闭并保留业务与close错误，不能声称物理input/receipt生命周期已完整接通。

### source batch 正式命令与单请求共享研究

新增固定Go的1–32项严格batch JSON/默认值/已知字段重复null与大小写行为，输入全验证后才准备，同一pin/index/environment和普通native owned worker/session，capture-free refs/context汇合单capture，各item顺序/cursor/coverage保留。query-only不准备引擎；显式reuse走既有session owner；flow仍既有独立逐context分析器，不冒称共享Emmy会话。CLI目录83项按固定Go字段与顺序核对，仅两处批准的分析器说明差异，live doctor仍未齐。

独立复审修复四项：Node目录门禁旧缺失断言、Index正文/符号扫描未贯穿control、默认引擎不可用缺warning/static partial、聚合native结果预算迟到。现受控搜索/分块原文读取可停止；默认未显式release资源准入失败警告降级且不执行坏资源，显式release/取消期限保持失败；先验证原参数避免降级移除reuse后接受冲突。default/reuse逐项计量包含协议封装、超额立即停止后续求值并不capture，不构造第二份全JSON。

独立metadata5+Skill检查器17项通过；14项Rust batch与既有refs_context整合回归实际NOT_RUN，现接Windows。预算fixture只证明局部owner析构/后续不求值，真实进程退休与取消/崩溃/恢复须产品运行证据。新命令让实际Skill runner可按describe执行三项batch，当前仍不能记实际Skill通过。

### tracked console 与只读取消边界 API

Windows新增callback注册guard供CLI连接core TrackedCancellation而不反向依赖core；回调/FFI均锁外执行，Arc保障已取快照的旧回调可安全完成且不串新注册，panic不跨ABI或冒称取消成功。显式注销报告失败，Drop只尝试一次且失败禁止重复注册。core新增只读祖先取消查询，不暴露raw写权限、不代替deadline/first-cause裁决。Process新增完整control检查入口；独立审核发现旧wait_exit退出后检查造成行为漂移，已用共享内部policy恢复Legacy原边界，新入口仍保完整检查与唯一FFI实现。

11项新core/Windows fixture含确定性退出/取消竞态已编写，独立源码复审所查finding闭合，Rust/Windows实际NOT_RUN；现接CI。CLI正常invocation、broker、helper及Source serve仍有旧token接线，当前仅API基础，不声称正常Ctrl+C已经保留取消时间戳；后续接线须继续保原deadline与生命周期，不从raw AtomicBool伪造历史时点。

### API 真正 Type 推导及 session admitted 配额

API Type现在由独立认证environment原文重建definitions.lua和唯一mapping，建立独立Emmy模型，核精确UTF16/site/host来源后调用managed semantic_model_with_context→type_of_expr及bound coverage。source模型不覆盖/替代环境原定义；未绑定为not_executed，实际推导Unknown为executed/unavailable，不把descriptor或unknown称已知function。

生产cached-query采用同request一次plan→admitted/attempted批累计→execute，completed仅证据不决定限额。255+2执行前先退分析；identity-only回snapshot_missing，full输入最多一次同control重建且继承planning steps；恰256及处理错误退analysis，broker原60秒idle/600秒life保留。独立源码复核未发现本批阻断；新增5项配额fixture中3项走实际生产helper，预置254/255边界，不能称已跑256次真实IPC/多CLI。

本批Rust运行NOT_RUN。独立审核指出新semantic_admission_tests不被旧native_session::tests过滤器覆盖，现扩为native_session::整体并用block scalar实际解析YAML。真实完整IPC额度轮转、0-batch helper与来源shadow/full Retail semantic/session实测仍需补，不把上一ec2的小fixture复用为这次通过。

### 正常 CLI 有序取消与 Source 完整 scope 接线

正常 invocation 在预算建立前注册 TrackedCancellation callback，保留参数错误优先级和 version/help 豁免，guard 保持到输出结束。Emmy doctor 使用 ordered child；one-shot Source 现在通过完整 OperationControl 子 scope 等待实际进程，保留父取消、原 deadline 和 first cause，而非只轮询子 AtomicBool。Complete 路径在 spawn 前与收齐事件后检查实际 scope；Legacy 保持原边界。原 Job、管道、退出等待和 terminate/reap 实现共享，未另造清理机制。

独立复审关闭两项 scope 检查遗漏，三文件哈希冻结核验通过；5 CLI + 7 Source 新测试实际 NOT_RUN，已分组接 Windows CI。Node metadata 5/5、diff 检查通过。此批不代表所有 legacy broker/helper/session token 路径已改完。

### 首次真实 Skill 流程终态（6cbd61b 历史快照）

run38639615054/product116122379218 于 2026-10-14 22:19:54 UTC 结束：all-target、独立产品组、完整 Retail 静态 Source、真实多 CLI native session 成功；Skill 实际 27 项中 21 成功、6 失败。失败均为该快照 describe 缺少 refs/context/batch/session status，未跳过或放宽必需命令；因此产品 job 整体失败。19cb15 后库存已有修复，仍须新树真实复跑，不能倒填旧证据。artifact11933365203，ZIP SHA256 76bcee97302414b06b6c6ace682d17c4c7e6ee0169544b0444564590912c01cb 已下载认证；独立 artifact 内容复核待完成。静态差分仍失败，Emmy 组成功；不宣称完整协议/性能或四阶段收敛。

### Capture 依赖与同事务删除保护

新增有序不可变 capture dependencies；同一 SQLite 写事务先核依赖再发布 blob/manifest，空依赖保留既有身份字节。named Hotfix 页引用 source capture，继续页也保留依赖。删除检查用借用当前写事务的原始 manifest reader，保留大小写字段顺序、重复对象/null 合并、原始时间转义及被覆盖的字面 capture 引用，不经 Value 排序后误漏消费者。Go 时间和无效 Unicode 的受控投影保留原始 blob/hash，不改写证据字节。

三轮独立复审关闭时间/Unicode/字段严格度及原始顺序 P1，8文件receipt SHA256 68de154999537fd4638b8d311b652003636084e0e52aba098fc498dc5b5f7fb1。24项新Rust测试全部 NOT_RUN，现分组接 Windows；直接 SQL 原始JSON fixture避免测试本身丢顺序。非capture文档仍用旧Value扫描，其被覆盖字面引用和转义并不等价Go raw扫描；旧64MiB scan限额、Metadata::open等待、archive恢复/幂等错误边界未在本批解决，不能称archive全等价。

### 正式 CON 准入、目标与退休持久化

新增 ConnectionBook create/find_selected/status/acquire/retire_closed 与 OwnerInspection，正式 CON 项目文件和 OP SQLite Book 分离。保留 target→初始 log→claim 顺序；claim 前 orphan只读保留，不补claim或reseed，既有claim缺日志拒绝。完整 ClientWindow/owner、同PID创建时间多HWND排他、256 JSON/1024 entries、admission→try-only execution 锁及Forever Asia旧slot单向兼容按固定Go实现。新增连接target独占stage→sync→checked close→原子no-replace，跨安装同token也不覆盖赢家证据。

独审发现并修复 quoted u64 前导正号误接受、interface:-0误拒绝，以及跨安装同token的target check-then-replace竞态；复审所查finding闭合。29个测试声明含子进程helper及Unix-only项，全部 NOT_RUN，已有go_mailbox_v1整组Windows过滤器覆盖新模块。真实进程exit崩溃切点fixture保留磁盘证据，但未执行不能称crash验证通过；AppendClosed注入也不冒称真实OS close失败。默认CLI仍接旧host/session/runtime，当前仅正式持久化与准入基础，后续须完整接新driver/runtime/input/doctor。

### 6cbd61b 实际 Skill 产物独立复核

独立复核逐字核验112包文件/105resources、25候选Skill文件中全部23固定Go必需文件、6runner输入、52sealedLogs、22实际命令和11capture；EXE SHA256 5bd7f80fc6c90c9e7da4151e37373699bf00d7b458e83022738248185d9ac5be，包93dd15beeb28aa04b86e6718640957027ed0df081fb876b0614256c252b1d643，368输入sourceDigest aad3ba8ea083019c6703c8fdd1657619a3c456618e654d93248012646cbeeb3b。固定archive重算4049文件/4036文档及Git root2368890bcb80a4abc8fe2ced2d0be95dcb5b71c6吻合；API第10–21行和ActionButton第97–108行原文、查询坐标、分页offset0→3及3个exit2错误后同pin查询均有真实证据。

原始21 passed/6 failed保留，但一项成功名称过强：post-semantic-source-read-still-usable仅为末尾API查询；此前所有semantic任务因缺能力未发出，不能称语义后恢复。必须修runner前置条件，历史该项只认可末尾同pin查询。六失败中refs/context/session status五项是已有handler未广告，batch一项当时确实未实现；不是只补广告即可验收。中文字面零结果不证明自然语言理解；部分分页不证明全部结果；public session close未执行，只有outer Windows Job全树回收/active0与scratch retired实证。isolated prefix→Skill launcher→package launcher→EXE已跑，不是npm安装验收。

### Source 真实广告注册修复与早期 CLI smoke

补充纠正19cb的覆盖：其新增batch及完整定义表，但source::commands仍漏refs/context/session status/close四个已有dispatch路由；describe会过滤它们。本批精确补注册并加固定Go全部Source集合/重复检查回归，metadata Node 6/6实际通过，独立源码/CI顺序审核无阻断。新Rust回归 NOT_RUN；不能据注册完整宣称这些业务实际通过。

sealed CLI build与version/help/describe smoke已移到all-target之后、长事务/全仓测试之前，保持独立失败记录、不短路其余组。当前smoke仅验可执行及基本形状；严格实际describe逐项合同差分仍在下一批实现，未提前声称该门槛存在。正在运行的418快照仍含这次广告缺口，其结果保留不回填。

### 默认 Live 生产接线的剩余关键路径

最新独立源码调查确认：CLI仍进入旧host/lifecycle/live-v2日志/memory-slot-v4，不能把正式Driver适配成旧Transport来宣称对齐。正式CON admission/journal与typed publication已具备，但未被默认入口消费。runtime_root仍旧3.1双recipe/exact块读/原pattern第二命中即歧义，runtime_binding旧四build白名单不能代表七产品；Go3.2需要第三ForeverContext/CN recipe、短读前缀+页补读/不跨gap、按有效root去重与当前Lua路径验证、8192次等预算、窄path_changed/reload可恢复分类。该reader批正在实施，输入权仍未授予。

后续生产门槛还包括：同一受锁EXE完整受控hash/PE定位字段对照的不可伪造layout能力；正式Selector/Accept前后identity/control guard；Windows双键40ms节奏、ALT WM_SYSKEYDOWN/context位、额外物理modifier检查、独立有界仅自有按键释放与uptime；Backend真正消费PublicationLease、fresh INPUT和非持久地址witness；Project完整历史请求/unknown不重放/结果投影/精确process-end退休；Activation与Bootstrap/read-only Doctor及fix。不能移除locator_not_finalized错误就称接线完成。runtime reader→输入/backend→Project/Doctor→CLI切换按共享文件单写实施；最终用自有Windows子进程/窗口跑CLI→真实RIP/Lua→slot→chord→receipt/report→新CLI resume，游戏验证继续最后。

### 实际 describe 早期合同差分

新增sealed EXE的独立 --contract smoke：执行真实describe，对固定Go84项命令逐字段/flags顺序/命令顺序比较；rawExactMatch保留全部差异，acceptedContractMatch仅允许已授权的两条完整Emmy摘要替换，其他缺失/多出/metadata变化仍exit1。原stdout/stderr、cli-contract.json、cli-smoke.json在seal前保存；未知回收不cleanup。早期步骤失败不短路后续独立产品组，普通smoke/doctor语义保留。此工具落地前只跑Node检查器，不能称实际EXE差分已跑。

静态arity/advertised适配器已重新独立审查：contract.rs生产前5112字节与固定5d0cac6387c79ab4eb3bbb91279886316fad48bf逐字一致，旧wholefile gate失配来自Rust测试改动；更新精确全文件hash而非放宽识别。全115flag类型匹配，缺live doctor等实际差异仍报告，21addon extras未隐藏。作者49/49 Node检查通过；82/84广告观察属于当时含未提交Data两注册的dirty输入，其candidateSources记录实际hash，不冒称远端f0c的80/84。freeze receipt SHA256 43bd465053392ec8ad3d79e36efd85190fb2df05cd6832f6348351442aed5ae2。 独立复审49/49 Node与额外420逐命令变异拒绝均通过，所查5文件及CI接线无阻断；实际Windows --contract仍NOT_RUN。

### Skill 验收假阳性闭环与 context 真执行门槛

post-semantic检查现在要求两个实际native任务和末尾同pin查询均通过；未执行/失败时not_run并保留原原因，末尾查询独立准确命名。业务state移入observedState，不能覆盖检查状态。固定Go depth0 context返回空relations，保limit5并逐字核ActionButton第97–107行/全文hash/编号正文；refs仍强制真实callsite，不为context错误要求无关关系行。

独立复审进一步发现summary壳可冒充执行、未完成receipt可伪称complete，现context核生产typed字段序计划与operation摘要、索引/kind/target及真实executed references回答，绑定已认证声明[2915,2935)/UTF16 96:9–96:29；从answers重算receipt，并将plan.unavailable及不完整回答强制partial。诚实部分执行即使completed_batches=0仍允许，不混同全部scope完整。四轮限定复审的finding均闭合，最终30/30 Node独立通过；旧6c原报告字节不变，修复后实际Windows尚未运行，不算全仓四阶段审计。

同时补workspace serde_json的raw_value feature：已推admission整数原文解码消费RawValue，该类型在锁定1.0.151中受此feature控制。仅打开已有锁定依赖功能，无版本/锁文件变化；独立核上游本地源码cfg及Cargo单行差异。此前包含admission的新树尚未实际编译，不能称本地编译已验证。

### 完整 Retail native / 真实 IPC 验收接线

新增8文件验收控制器，固定原始archive认证全部4049文件/4036 Lua-XML-TOC文档/32,269,051B，独立XML字节tokenizer重新推导1091XML的4018个映射及坐标/digest，不引用历史计数冒充新运行。完整material原文、独立API Type模型绑定、普通refs/context及跨CLI retained cold/warm/identity改变/reopen均按实际计划与执行回答验；context精确原声明/逐snippet文件hash和编号正文，depth0不误要求relations。只保留同输入已执行但partial的真实证据，不能让未执行summary或伪造complete蒙混。

quota、idle、lifetime分别使用真实IPC连续attempted操作和60/600秒实际生命周期，不预置计数；Windows pipe peer持同管道/进程句柄，核OS PID/创建时间/映像/锁定EXE。每组同已封存EXE/package/source身份，独立owned Job与scratch；nested prerequisite失败/超时停止后续launch，交外层Job证明退役再cleanup。四组上限20/10/2.5/12分钟，合44.5；CI产品job120分钟容纳已有测试，各组互不短路并保留失败证据。

独立审核修复XML自洽漏检、context原文坐标不足、嵌套fatal传播与receipt伪complete问题；从原archive/生产typed序列化/实际scope规则复核，最终8文件/14依赖无漂移，83/83 Node独立通过。receipt SHA256 372773d30187b63cc087196be5a5ce68293f0e6825a7202872d7e0de9496d3ef。Windows/PowerShell/完整native产品运行仍 NOT_RUN，首次由此CI接线执行；这不是Go性能对照或四阶段审计通过。identity变化用隔离本地README后继commit，不能称第二个真实上游版本已验收。

### Hotfix scan 检查点与恢复

新增固定Go scan路由：每次至多1000页、累计10000页，逐页复用named table读取、同原control/deadline与依赖capture；检查点成功持久化后才替换恢复点，错误/取消保留最后可用结果，完成的历史重放仍先核source/pages存在。历史JSON字段大小写/重复对象/null合并与请求身份匹配保留；新增RawValue有符号词法保MaxBytes/Limit中间负值后覆盖、拒溢出/浮点/指数/伪number对象，Push按i32、blob.bytes同Go int64，四处-0合法。此共用修正保留原capture依赖整组测试，不删除旧回归。

scan依赖校验的临时解析reservation在前一对象全部销毁后释放，512MiB活内存上限不变；metadata累计10,621,026,304B/work84,162,904,064推导自10001个至多1MiB文档，峰值25,169,920B，累计收费不清零，其他producer默认记账不变。10001依赖fixture只测账本生命周期，不能称真实大规模磁盘事务/性能已过。

两个既有Data auras/summons handler补广告注册，23 Data/Asset命令集合无重复。9文件两轮独审原数值P2已关闭，28项新Rust回归（15unit/11integration/2registry）均NOT_RUN，现接Windows；rustfmt/diff及11个JSON测试模板静态检查通过。receipt SHA256 4a312dd5fd6dd3c23367e44ecc9844cfcff0f07f28a7222d496113ab52931212。正在跑的418 named-table和typed-budget组已失败，终态日志待取并优先修复，本批不能覆盖或倒填它们。remote provider/SQL effective/完整Data与性能仍未完成。

### 418c2ef Windows 终态：新产品组与明确失败

run38645225374/product116135835948整体失败；artifact11934043350 ZIP SHA256 b61b42d3297dd4d17e6ddd54ef2eceb5b4e200967cc96f66897261705fe2b915已下载，572份sealedLogs逐项hash全部吻合。EXE SHA256 84acfd02cae72af870321a61bd09b3574e7b478e157f63d37922484d141132f2。按各受控Cargo命令结果计445次通过断言执行/4失败/6ignored（非去重覆盖率，失败组内pass也不等于整组通过）。其中source batch14、正式Mailbox/journal148、typed slot publication29、native session12以及SQLite锁取消组实际成功；full Retail静态21检查和真实多CLI小fixture session成功，不冒称完整Retail semantic通过。

明确失败：native research组22pass/2fail/2ignore，两个虚拟模板handler计划期望1实际0；已定位开标签span与完整handler span混用，待修。named Hotfix组11pass/1fail，整数浮点页误触records.hotfix_limit；typed Hotfix组5pass/1fail，Go应编码1却返回1.0，待修共享根因。后两Cargo exit101后仍有vctip子进程，按既有仅成功root允许辅助进程特例策略转强制回收；root/tree/handles/readers均回收、active0，但整组失败，不能抹去forced或放宽helper遮盖产品错误。

Skill此旧树27项原始22pass/5fail：真实batch新通过，refs/context/session status五项仍因未广告而未发出；f0c才补这些注册。旧post-semantic标签限制仍适用，de321才修checker，不回填本次。完整产物复核记录SHA256 f7d3aba9697acad971fdf97eec5e70e32eb6a5139d74cc616e6188fe77b0ae6e；这只是本次日志/命令逐项复核，不是全产品或四阶段收敛。

### 早期稳定诊断产物

真实GitHub job运行中日志下载返回404，因此在早期build/describe步骤后新增独立技术证据快照上传，后续长测继续。仅采用sealedAt且handlesReaped=true的receipt和本owner目录内逐hash吻合的sealedLogs；不读取活跃组日志、不借其他owner、不写原evidence。验证后的Buffer一次物化到全新独立目录，单文件16MiB/累计64MiB/2000文件；原文件不被清理或改写。失败但已回收组保留，未封存组明确skip；校验失败不上传新快照。

独立复审发现并修复跨owner活跃日志借用及输出目录重叠两项边界；8/8 Node独立通过，workflow仅上传成功快照，不扩大权限、不含凭证或用户数据。实际Windows早期artifact尚未运行；完整终态artifact保留原流程，早期快照不代替最终证据。

### Source XML handler 真实失败修复候选

418c2ef 的两个实际失败保留原断言与日志。修复准备期 script → 最近对象的真实声明身份：按原文件 path/hash/开标签 span 匹配，排除顶层 Script、具名/匿名子对象和兄弟对象，保留跨文件模板继承；移除查询期重复 XML 解析。RESEARCH_RULES 升至 v7，使受影响的 material/cache/cursor 身份失效，不沿用旧计划。

三文件独立复审无本批阻断；原两项失败断言不弱化，新增三项回归覆盖同名同线对象、BOM/CRLF/emoji/entity/UTF-16 与取消。独立 Expat 拓扑/坐标检查、rustfmt 解析、diff 检查通过。receipt SHA256 6a420d7e7dea8f063e30d23c2c983dfdfb549a9cae63f00867a540f6dfde55c7。Rust/Windows 新候选测试仍 NOT_RUN，既有 native_research:: CI 将执行；不能把计划正确等同实际语义查询通过。prepare 同步解析取消粒度和同名同线多个 Definition 的上游选择歧义仍待后续处理。

### 正式 Mailbox 双动作 Windows 输入候选

新增仅 Invoke/Reload 的 Ctrl+Alt+F1–F12 输入引擎，按固定Go顺序/40ms间隔发送；ALT按下自身使用系统键消息与正确context。完整私有 MailboxWindow 绑定 PID/创建时间/HWND/EXE/class，正常输入及独立1秒逆序清理都重新核验，拒绝物理修饰键冲突；只清理由本次成功排队的keydown。线程绑定try-only互斥、abandoned拒绝、原control/deadline保留，cleanup错误与原错误合并。

独审发现初版仅PID/创建时间/HWND的身份缺口，已用完整不可反序列化身份封装修复，包含首次keydown后EXE/class变化的回归。Go EqualFold 使用固定官方 Go1.27.1 Unicode17 源和许可证生成1512代表映射；独立核遍全部Unicode码点及12287比较组合，不借Rust或Windows不同折叠语义替代。无业务unsafe，必要Windows FFI逐点说明。freeze SHA256 7d2dbe76e3cb4f4e3cecb91da4d385fe3b81ac96e22396e5d662fa8061df5325，15源码/映射文件独审无本批阻断。

22项Rust测试声明仍 NOT_RUN，CI新增独立engine/identity与自有窗口fixture组，均无需游戏或用户机器。真实跨进程abandoned mutex、游戏输入和完整默认CLI接线未验收；Go交换间100ms、fresh INPUT witness、PublicationLease由后续正式backend统一接入。本批不能称生产Live已可用。

### Hotfix 实际浮点编码失败修复候选

418c2ef 的两个真实失败共享根因：serde_json arbitrary_precision 的 Number 走 write_number_str，旧 Go formatter 仅覆盖 write_f64，导致1.0被计量/归档为1.0而非Go的1。现两个hook共享Go数值编码，fields预算和capture归档使用同一formatter；整数字面量不经f64，保留u64::MAX及2^53+1。通用capture默认CompactFormatter路径不变。归档解析的-0由原decoded fields恢复公开Value符号，合法字段形状只有标量/一层数组。

四文件新独审无可确认P0/P1/P2，原两项失败测试及断言不变；新增8MiB精确允许/+1拒绝、双hook、归档原token/公开精度/负零测试。r2只将新增测试的f64恢复改为RawValue原token.parse，避免Serde无float_roundtrip配置导致测试自身1ULP误差，不改预期位值。独立Python核验编码边界8,388,608/8,388,609及10个数值token通过。receipt SHA256 8db722aa1ec9bbb24971f2a7b34fbc3e3af0727b444f8dab39ab4d50bfbeaf2e；Rust/Windows原失败与新增回归仍NOT_RUN，下一候选执行后才能认定修复。

明确未解决：公开CLI通用JSON序列化仍会把负零输出为-0.0，而固定Go为-0；本批仅保证所改Hotfix归档/计量设计，不宣称完整CLI字节等价。后续必须统一公共数值编码并回归，不能登记为新兼容例外。

### 剩余范围与现行测试迁移

todo新增按“已有组件/未接默认生产路径/待原生实测/性能与全仓审计未完成”排列的模块工作队列，具体证据仍以本handover为准，不用测试数量估算完成百分比。当前默认Live仍未切换正式reader/backend/CON Driver；activation、doctor/reload fallback、完整Data effective/remote和公共JSON来源语义、最终包与同机Go性能对照均须完成。

前置消费者审查已确认额外21 addon文件仍被旧native_e2e、Rust TOC动态Lua测试、递归打包和install-smoke消费，不能直接删簇制造差分绿灯。当前部分同名Lua测试仍断言旧memory-slot-v3/input.v3，缺固定Go的bootstrap/input_mailbox/focus等正式测试；应先移植正式Go测试和仍有独立价值的安装/事务/未知不重放集成边界，再清理旧消费者。此调查不算四阶段第三阶段收敛，正式四阶段尚未开始。

### 正式 runtime reader 与可信 EXE 候选

新增正式 NativeReader：固定Go三类RIP指令模板、稀疏代码prefix与分页恢复、最多16候选/4096原始match，同一LuaReader完成正式selector和外层recipe/layout资格后才接受；只对规定的纯瞬态错误恢复，不重置原ledger/deadline。可信EXE在read-sharing-only锁定句柄上完成完整SHA/PE32+AMD64资格与runtime定位guards，明确checked-close后才签发不可伪造metadata资格；Arc保留metadata，不谎称持有文件锁。

独审五项真实finding已逐项修复并复审：formal 32Gi work限额与构造器一致、postguard仅核实际返回prefix且未读suffix可有界恢复、lookup失败也复核外层recipe/metadata/control并保留firstReadFailure、VirtualQueryEx原错误码结构化透传、非BODY固定16KiB不受无关body.length污染。专用累计1GiB requested/32768 read calls/32Gi work不改变普通默认预算；推导覆盖初始扫描/16候选/首次lookup组合18359次read，后续继续累计不能无限重试，数值推导不是性能通过。

r3另补正常open/bind错误和显式NativeReader finish/close的唯一process owner checked-close，不因取消/预算停止清理，原错误/close/control聚合且不二次close。正式module/image两路径改用输入批固定Go1.27.1 Unicode17 UTF16 SimpleFold，避免WindowsOrdinal与Go特殊轨道不等价；原input算法/表未变，仅crate内共享可见性。r3 19文件freeze SHA256 32ba5e3c00e9adbd01a44ccc2796fd038d601ff964bcf07971d44616956350c1，独立增量复审无新增可行动finding。

仅静态rustfmt/diff/Go模板与映射/unsafe inventory通过，全部本批Rust/Windows实际NOT_RUN。CI新增runtime_root、process::runtime_及自有进程runtime_module三个独立组，已有formal Mailbox/binding组覆盖reader其他回归；实际收据必须确认非零匹配数。此批还没有默认CLI/backend/input/journal整体接线，不称Live可用、实机验收或性能达标。

### 正式 Windows backend 接线组件

新增 NativeBackend 实现正式 Backend/RuntimeRetirementBackend，组合可信NativeReader、typed PublicationLease、完整MailboxWindow及双动作发送，不使用旧Transport。observe_input生成私有一次性fresh witness，Invoke精确发布资格和同一短lease跨整次发送，Reload检查后先checked-close再发送；保留原控制/期限、100ms交换间隔、Go的2秒stale grace、至多15秒退役与间隔1100ms双fresh样本。trace保留256条/10秒flush并精确Go字段；进程关闭和当前操作错误合并。它是可接默认业务的正式适配组件，不等于默认CLI已切换。

独审修复恢复watermark合法10100样本被严格相等误拒、FakeSender首post前guard失败假计非零、trace外层字段大小写；最终7文件/4依赖/2前置freeze及23固定Go源码/test身份复核无新增可行动finding。freeze SHA256 661e797863f4b42de0ee1f70d18d3ea9794ad8d01dae89c1d63427913ccad5e1（冻结时记复审待完，随后独立终态已完成）。34项Rust回归仅声明，rustfmt/TOML静态通过，编译/Windows全部NOT_RUN；现接入已有go_mailbox_v1测试组执行。

明确新发现待修：Delivery PublicationLease在构造过程中取得维护锁后发生错误，及wait_lock成功后遇取消，仍可能仅Drop而不回传checked-close失败。backend只能显式关闭已经返回的lease，无法修未返回资源；此构造cleanup作为独立批处理，不能借本批审核关闭。完整CON/public CLI、activation/doctor与自有进程Lua端到端仍待接。

### 公共JSON显式数字来源候选

新增内部serde(skip) NumberLayout：未知Value默认保留现有token，不把所有decimal猜为float；仅typed生产者声明Binary64，Hotfix schema Float字段/数组明确标注，整数不经f64。Exact(JsonNumber)只接受完整有界JSON数值原token并核当前Value匹配，保留SQL等Go json.Number的-0/指数词法；Indexed用于异构行，SQL消费者另批复审。公共JSON/JSONL/text统一Go HTML不转义及U+2028/2029转义，布局不进archive/cache/journal身份。

独审两P2已修并复审：Object默认required，只有合法Hotfix删除行fields显式optional；借用前缀避免布局/Exact深复制，同一不可变Value/layout及相同formatter先sink预检再直接输出，去掉整帧无限Vec、不自造更小输出cap。序列化失败不触输出；实际IO失败仍可能部分写入并exit5，不承诺OS原子写。两遍CPU成本须实际测量。

最终6文件freeze SHA256 36e947e82c5c6c2e0fcbdd144755c2faee5f8aa5e12752bc78c93fd83c6c725d，嵌入r1性能值漂移已还原，能精确重建旧8c130952收据。独审24唯一测试声明/16固定Go引用核实，rustfmt/diff通过；全部Rust/Windows/性能NOT_RUN，CI独立core/CLI输出组与既有Hotfix组验证。该typed数值入口没有声称所有其他生产者和完整SQL已经对齐。

### PublicationLease 构造失败清理

补齐取得维护锁后在control/path/第二锁/inspect失败，以及wait_lock刚取得锁后取消的显式cleanup。正常错误路径只消费各已取得owner一次，按publication→maintenance checked-close，保留原ToolkitError/details/exit及cleanup原因，不新增公开variant，不丢Reserved元数据，不改变发布/退役算法或授权重放。

两文件独审无新增可行动finding；29原测试保留，新增9项，38声明全部Rust/Windows NOT_RUN。close故障仅helper注入，不冒称真实kernel失败实测。freeze SHA256 6e59929a75ed3af6e2a29582e7bdfebffcd5ae55632eea8e4ff176bf5df46c83，review SHA256 936703e729401c9a810cf65f02993400170832522c8e9c03baf135a90575eb42；既有slot_publication::tests CI会复跑。后续service主addon验证复用仍需核core release/storage其他Drop-only边界，NativeBackend非control错误转文本也不等于端到端details/exit已透传，本批不覆盖这些独立缺口。

### e38851d Windows 完整终态与新失败

run38650383333/job116148400586终态失败；artifact11936886255 ZIP SHA256 40eb97deee89c42e41cae59028b188364df4cd656a000b78e7c99c87ffaed911，107 owner收据的811份日志hash全吻合，复核记录SHA256 b34cb04eaabc43a954317b55ebdd56b8cde957615f0e5dcb7e258942890c481e。EXE SHA256 1c9a14cdbfc7bda6ba59d494d231a46f2744541de7e4fc42eeff03626e951169，sourceDigest 837ad96ba8b08bd31fc2238b4cb575af1d66b9bec6b9e9d0360487c65dfcc276。539通过/5失败/6ignored为各Cargo组断言执行次数，非去重覆盖率。正式Mailbox/admission176、scan11、capture依赖18实际成功；metadata::tests过滤匹配0，不能算取消测试通过。

除后继a8c/cb修复的旧XML/数值失败，还发现旧测试期待scan unsupported但新实现已支持。实际Skill普通refs/context与全Retail refs/context/APItype/持久会话失败：source.native_plan_environment，生成API出现“---@return number in”被Emmy解析为缺右操作数类型表达式；正在修生产emit，保留校验与4036输入。quota/idle/lifetime均在project init被runner非法--home参数拒绝，未进入真实IPC容量或60/600秒生命周期，不能称这些边界已跑。所有这几组outer root/tree/handles/readers已回收active0且未forced；完整失败原证据保留。

### e388 验收入口修正候选

三IPC组的project init现显式不附--home，其他workspace命令仍强制隔离home，helper拒绝对source等命令省略home。metadata过滤改为源码实际controlled_tests，避免0项绿灯；旧--scan unsupported断言替为实际成功dispatch、returned=1与hotfix-scan捕获类型断言，其他非法参数/取消和既有scan回归不减。五文件另一个独立视角复核无阻断，43/43 Node实际通过/0skip，diff检查通过。Windows IPC、metadata与scan Rust新候选均NOT_RUN；本次修的是已证实错误调用/过期预期，不改变CLI准入来迎合runner。

### 原进程生命周期事实接口

新增safe observe_lifetime，以原PID/creation进行最小QUERY_LIMITED_INFORMATION|SYNCHRONIZE查询：只有OpenProcess ERROR_INVALID_PARAMETER为Absent，access denied/查询错误/异常wait不冒充终态；先核creation再Wait(0)，返回Running/Absent/Exited/PidReused。所有取得句柄的正常路径checked-close一次，原OS/control/cleanup原因保留；不依HWND猜退出、不读取游戏内存、不终止外来进程。固定Go UTF16 EqualFold只公开安全函数，算法/表不变，供正式service真实选择使用。

7文件独审无P1/P2阻断，freeze SHA256 1f7db61f6f1624007676673fa547b6aa12b0921dfed1ff1a7d0957994c87d9ec；15unit+4integration+1 ignored自有子进程入口声明对应test-map，均Rust/Windows NOT_RUN。fixture仅启动当前测试程序，以stdin EOF正常退出，fallback只用自身Child句柄；真实权限错误/PID复用竞争/内核close故障仍只有模型覆盖。CI新增两个独立原生组，不能把本接口审核当service owner退休或完整Live验收。

### 固定Go正式Lua验收独立入口

新增独立tests/rust-parity/formal-lua固定夹具闭包，不覆盖仍有消费者的旧v3/v4测试。57个Go原文件逐Git blob/SHA/长度认证（addon38、protocol13、wow_globals1、probes4、testkit1），正式55 addon同样认证。计划实际执行35原Lua断言套件×6profile加4纯Lua协议，共214项；slot_server及3项Go-host桥接明确NOT_RUN，后续移植正式Rust端到端，六profile不冒称七产品实机。

官方Lua5.1.5固定源码及官方MSVC构建独立interpreter，复用既有owned-process/disk归属；build≤10min、run≤20min。普通断言失败独立收集，超时/取消/forced/未知回收或身份漂移停止后续并记not_run，原日志保留。解释器及Rust artifact全部同owner有界sealedLogs、命令/proof/output、sourceRoot/sourceDigest前后核验；逐fixture继续核原Lua/addon/解释器身份，不反复全仓扫描。

三轮独审修复预算检查后仍launch、真实deadlineHit后继续、缺封存源码身份、遗漏其他sealed日志及cargo命令前缀五项P2；最后与真实HEAD producer和418已下载artifact逐形状复核。freeze SHA256 047ccb1723f378bba868adc6fb12d4e1570d06a803aa2cc3dd54671c4a56d93f，63文件/9依赖独审吻合，31/31 Node checker独立通过，仅checker证据。Windows解释器构建、214 Lua实际执行、3host桥均NOT_RUN。

CI新增独立checker/build/run并保留最终artifact，长job从120到150min仅容纳明确新增10+20min组上界，不放大单组预算、不使其他失败短路。README双语同步已经固定公开依赖重建并实际Windows构建的现状，仍不声称全Source/defaultLive/package/performance已验收。

### 完整Retail API模型实际失败修复候选

真实固定Retail ConnectionDocumentation.lua:45 的GetNetStats首个返回标签in，被旧生成器直接写成“---@return number in”；固定Emmy词法将in作为二元类型运算符，缺右项导致整个环境解析失败。现在普通标签字节保持，危险标签移到“# description”，原facts名称/类型/位置/顺序不变；generator升environment-ts-v3-emmy-return-labels使material/session/cursor旧身份失效。保留manifest/facts/VFS精确文本/parse全部拒绝链，诊断只给有限hash/错误数量/偏移，不泄露完整源。

独立审核核全部4036文档、612环境文件、完整原fixture及固定Emmy6个grammar文件；对真实生成定义5159 return与5873 mapping逐项对齐，唯一变化第20911行，预测1,041,400B、SHA256 1d6e1f79c1aea00e873aca50f9cad761990dd15605ee88f15e48a855d00b2cd3。freeze SHA256 f3b543f34ce682f24e6a9aa649fe1eebca812b4a38c67a5803d04030bb1d73e4，本批静态独审无阻断。六新Rust回归仅声明NOT_RUN，现有native_research::会执行；必须在同一新Windows封存EXE重跑完整普通refs/context/APItype、持久cold/warm及真实IPC，静态重放和代表fixture不代替全仓验收。未改XML v7、third_party、预算或裁剪源输入。

### 身份文件与 release 验证的显式资源清理

- 冻结 `b5f98d1ac623b648607f405d4ea51ef723fa57870e5428ec79205a04e9110406`，对应 `tests/rust-parity/release-resource-cleanup-{freeze,test-map}.json`。独立只读复审核实 4 个源码文件、5 个依赖、6 个固定 Go 来源，无新增可行动阻断。
- `storage::locked_sha256` 构造失败显式关闭，成功仍把原 deny-write/delete 句柄交给调用者保留；`storage::finish_file` 先消费关闭一次再检查原 control，单错误保真，多错误以固定 `lycheedev.file-cleanup.v1` 保存 primary/cleanup/control 的完整 ToolkitError。release inventory 与 manifest 的取得句柄后成功、失败和取消路径均接入，原验证体及扫描规则保留。
- 作者 rustfmt、独审身份/映射/固定基线与 diff 检查通过；新增 13 core + 9 release 回归及两独立 Windows CI 组均 **NOT_RUN**。注入关闭结果仅验证模型，不能冒称真实 OS 关闭故障已测。固定 Go 的对应路径仍是 unchecked defer Close，本批为安全强化；下游 service 的组合错误消费仍需其独立验收。
- publication 内部 receipt 的跨 burst 句柄与其其他错误路径不在本批范围；不得为了复用早关闭 helper 缩短身份保护期。正式 Live 默认 CLI、repair/activation 全接线、性能与四阶段审计仍未完成。

### Effective SQL 与引用发布的有界候选

- R4 冻结 `fa076be395dbb23c6607d642eaca6c783556c84b6924e0c952ca29a761afa03e`（`tests/rust-parity/effective-sql-{freeze,test-map}.json`）。独立复审闭合三项资源/顺序 P2 及后续参数转换顺序缺口；13 冻结条目、4 依赖、16 个固定 Go 来源与 32 回归声明核实。R1–R3 原失败快照保留，未用后续结果覆盖。
- 实现显式 Hotfix 来源的 effective 表覆盖、布局/状态/构建/完整性准入、精确 JSON 请求参数和数字来源，以及 SQL 执行后 CASC 身份复核、归档前原子引用校验。Go 顺序为表数/参数名/转义后请求大小准入、pin/DataPin、参数数值转换，随后才准备来源；组合错误回归固定此优先级。
- 大来源共享 CaptureRef，在重复输出分配前检查 16 MiB；decoded map/字符串/数组按实际生命周期归还 retained，累计工作与 metadata 不重置。原 SQL 主解码 100M 工作预算不变；Rust 额外的归档引用重验仅可用 `16×1MiB×8` 有限子预算，临时解析内存 scoped、metadata 累计实际文档字节。这不是声称 Go 原先有相同额外计费。保留 16×750KB 单表、3200列×600条、取消及第17份拒绝等回归。
- 12 Rust 文件静态 rustfmt/身份/差分核验通过；32 Rust 回归、Windows 和固定 Go 运行差分全部 **NOT_RUN**。新增四独立 CI 组覆盖 local_query、scoped资源、effective integration、capture引用；既有 Hotfix/关系/依赖组继续运行。
- 尚未齐：remote/CDN、SQL meta/CSV、resources/resourceAccounting 与 EXPLAIN、完整输出字段排序、部分既有 I/O 的 deadline 贯穿和单次 codec/plan 临时内存计量。此批不代表全 SQL/Data 等价。固定 Go 不把查询引用当永久生命周期依赖，因此只在发布事务中防删除/ABA，不永久禁止后续删除来源。性能未测。

### 正式 CON 应用服务候选与已导出的测试入口

- R2 冻结 `cd3fb4289aa5e39ca9acbcbef491606eb29a9f20a439cccda66a2b898098770b`，`tests/rust-parity/formal-connection-{freeze,test-map}.json` 与 `formal-connection-contract.md` 记录 API/差分/余缺。独立复审闭合原 1 P1＋4 P2，另修真实 IO/close 分类、无效 fixture 与 `Interface/AddOns/Lychee Dev` 路径；14 owner、22 个固定依赖逐项核验。
- ConnectionService 将 CON admission/journal/Driver/NativeBackend/原 PID 生命周期接成 connect/status/resume/execute/history/disconnect；history 在 input 前和锁内重查，unknown 绝不重放。torn-tail 仅在原 claim/执行锁内归档原字节后恢复合法前缀；legacy unfinished unbind 继续原终态退役，不误开新进程。
- 退役观察保留原 ordered control lineage 与最短 deadline，3 个新增回归直接经过 Driver→实际 NativeCore；组合错误保留完整 typed control/close 叶，IO/close 不被抑制为单纯 timeout。安装发现投影沿用原 Discovery，不重扫制造选择竞态。
- 结果与错误同时保留；ProjectResult/RawReport 的 duplicate keys 与数字词法不能转换成 Value 后输出。CLI typed writer/adapter 另批进行。发现 Go native `json.Valid` 允许 string 内非法 UTF-8 字节，而当前 Rust RawValue 边界拒绝，此为待修真实差异，不列作永久允许例外。
- `pub mod connection` 按独审 stage 精确导出，新 44 回归（40 connection、1 error、3 NativeCore）进入现有正式 Mailbox CI 组。全部 Rust/Windows **NOT_RUN**，静态复核不是运行通过。原默认 CLI 尚未替换；activation/doctor/reload、modified repair publication、missing inventory 完整分类和后续资源边界须独立整合。四阶段审计和性能仍未开始。

### 原 CON repair 专用解绑发布与 receipt 清理

- 冻结 `b43757b85729c9ab6becb89d520435a3db3775b8d232d8752a26ff9805b498d8`（`tests/rust-parity/repair-publication-{freeze,test-map}.json`），全新独立复核两源码、11依赖、9个固定 Go 来源，无新增可行动 P1/P2。
- 不可变 RepairPublication 只描述已认证原安装/原 release/consumer 的精确 unbind；同一 publication lease 检查路径/release、原 payload/reservation 与完整 route，不接受替换 envelope。普通 bind/execute 仍要求干净主 addon。此能力补齐 fixed Go CloseForRepair 不依赖 modified main receipt 的解绑条件，不能作为任意输入豁免。
- receipt 第一次成功取得的 File 保留至 burst 结束；重验临时 File 检查关闭，不替换原持有者。全部正常错误路径 checked-close；lease 最后按 receipt/publication lock/maintenance lock 顺序都尝试关闭，不重试不确定 close、不丢组合错误。
- 原38测试及共享 publication 算法字节未变，新增13有效回归进入现有 ci-slot-publication；rustfmt、身份、scoped diff 静态通过，全部新增 Rust/Windows **NOT_RUN**，关闭故障仅注入。Native/doctor 消费接口仍待独立接入，必须证明原 journal 身份、持久 close intent、fresh INPUT 与 same-lease input；完整 repair 尚未验收。

### Typed Live 公开输出编码前提

- 冻结 `1b28361dbbfa53bdec2de5a6464ea9204320f4d6aae18aaea6c0942ebdb85ba6`，`tests/rust-parity/typed-public-output-{freeze,test-map}.json`；全新独审核实5条目、依赖及固定项目/Go标准库/serde来源，无新增可行动 P1/P2。原 Value＋NumberLayout 编码体仅增导出，其余字节保留。
- `write_serializable_line` / `write_pretty_serializable_line` 可直接编码 typed envelope/ProjectResult，无 Value 中转，保留 raw duplicate keys、数字和字符串词法。依据项目实际 `SetEscapeHTML(false)`，raw 的字面 U+2028/U+2029 与 typed string 转义有区别；raw空白压缩/缩进以迭代处理，沿用Go 10,000层上界，不额外给合法报告设更小容量。
- 嵌套非有限浮点及非法 raw 的预检失败不输出该帧；实际 IO 失败仍 external.io，可能部分写入，不声称OS原子。18回归由现有 public_json:: CI组覆盖，静态rustfmt/身份/两个source-derived Python golden通过，全部 Rust/Go/Windows/性能 **NOT_RUN**。
- 此UTF8 writer尚不能修复 native RawReport/State 的非法UTF8字节准入差异；明确另批修原始字节入口。默认CLI typed渲染/JSONL错误伴结果/业务输出仍由正在实施的adapter接入，不能用此单组件通过代替公开CLI验收。

### 5a0e3ed 原生终态与 e047 早期编译失败

- `5a0e3edd228a14dc3a9bb76b5eeb986188158d36` run `38655286434` / product job `116164945294` **failure**；artifact `11938112825` ZIP SHA256 `f827988aa54375630e8ac68a594874ea28304b66317f9bf0035dc9a2fc5a8405`，1467项/170376262展开字节、112owner/851日志hash逐一验证。EXE `ffbd4c3cbef5dcc17face2b9fbeaa314029c2605512684e01a4cdf60c0c53ebc`，source digest `4175d82595cd08af57ff435ed51a1f581c5f7400f5021bd41fe8301afb572be7`；本地核验JSON SHA256 `13dee0aa93d10d4b16bc3577bc53a03f5c2b328983590e9d4c7488f74bb58e33`。
- 新增真实通过：输入unit16、owned-window6、RIP/root30、可信runtime43、owned-memory14、runtime-observation45，日志均非0匹配。总体Cargo断言执行680通过/1失败/6忽略（并非去重覆盖数）。唯一Rust失败是旧scan期待unsupported，后续1f2候选已修但尚未验证；catalog13通过/1忽略却因Cargo退出后vctip未在首次辅助回收时收敛而wrapper forced cleanup，整个组仍失败。其严格helper回收竞态另批修复，不追认原run为通过。
- Skill实际26passed/2failed/1not_run；完整Retail native5passed/7failed，ordinary refs/context/API及session仍为旧API语法问题；quota/idle/lifetime在旧project-init非法home入口失败，实际寿命/额度仍未测。上述树不含231/1f2修复。原失败及已回收资源证据保留。
- 后续 `e047c739a7fe40d4eecc8ada8b5fcb5fd22d2a87` run `38660112968` 的early artifact `11938129355` ZIP SHA256 `65ea398be04c72377689211a1e66b2bcf95388547906a16d5408732f4b7122a8` 提前暴露构建E0599：锁定windows0.62.2没有 `Error::from_win32`。候选改为WAIT_FAILED惰性即时 `GetLastError`→`HRESULT::from_win32`→`Error::from_hresult`，维持非失败零读取与失败一次读取、控制/关闭顺序；独立小diff复核及rustfmt通过，新Windows编译 **NOT_RUN**。e047依赖此crate的后续组未构建成功不能当执行覆盖。

### 辅助进程 drain 竞态窄修复

- 冻结 `b4cbc239ed47460076882c5892fbfaec2bf9fcd11aac6db1cd0d9b861fb6e918`，`tests/rust-parity/owned-process-auxiliary-drain-{freeze,test-map}.json`；正确HEAD工具在独立stage修改，历史冻结本地工具字节未动。全新独审核实4上传文件及5仅依赖文件，范围内无新增阻断。
- orphan drain 到期时，仅根已回收、原hard timeout未到、无输出超限/reader或既有错误，重新调用原严格 helper retirement；随后再次检查Active会计为空再决定强制回收。原身份/成员/创建时间/已认证路径/文件锁/非零root退出与最终checked-close策略不变，不新增允许名单、不重设deadline。
- 独立重跑14 Node通过、9既有Windows跳过；新增12为11个source-bound scheduled/fake-kernel决策回归＋1个严格函数身份回归。7个仅生产源mutation均按行为断言失败。它们不执行C#、Job或真实CloseHandle；可控Windows root→普通child→允许helper定序仍缺，后续构建观察不能冒充该精确覆盖。
- 5a原catalog wrapper仍为失败；首次拒绝退休原因未记录，代码可成立的时序不能升级为历史事实。既有严格退休还含独立有界回收窗口，本补丁不宣称整个wrapper总耗时严格落在原hard timeout内。新Windows执行 **NOT_RUN**。

### Execute 原始字节与 Go 持久副本语义

- R2 冻结 `d989d512a54cb3e7857285ceeed8386d2108f591b51ddd15cbd9553b6dd9bb2a`，`tests/rust-parity/execute-code-bytes-{freeze,test-map}.json` 与 contract。全新原审复核闭合2项P2，14owner、26固定依赖、18回归声明核实；R1原失败快照保留。
- ExecuteRequest 使用 `&[u8]`，CodeBytes 保持原始长度/请求等值与 Lua 逐字节编码；合法 U+FFFD 与非法字节不可提前lossy合并。JSON副本按Go逐非法字节替换，live内存原始bytes不变，恢复后按已恢复字节比较。首次artifact写入可达解码后3×262144，后续读取/已有写仍执行Go512KiB界限；其他result/archive immutable路径保留。
- 修正真实 ResultVerified 写结果时持久归一化 Operation 与 raw live 误比较：全Operation按实际持久表示比较，其他身份/结果不弱化，写同一编码；真实Driver→DurableChannelJournal→standalone结果→ReleaseReady/release intent回归保留raw与结果checksum，11种其他身份差异仍拒绝。另一新fixture改用正确记录publication后停止的backend，未削弱旧recovery-only Peer约束。
- 新core字节组与现有formal Mailbox/共享payload/Delivery组接入CI；静态rustfmt/diff/身份映射通过，18项Rust及Windows **NOT_RUN**。现有core3行/Delivery1行fixture仅API适配，原断言保留。native RawReport非法UTF8准入/公开byte输出仍是下一批必修边界，不能凭此声称全字节合同通过。

### 实现阶段验证节奏（用户2026-10-15修订）

- 持久化推送与慢验收解耦：有意义小批及时push同PR，不等长CI。实现阶段优先本地可运行的真实纯Rust/Node/Lua/差分回归；Windows专属仍由Windows原生验证，不能用Linux或交叉编译冒充。
- 每push保留快静态/合同门槛。需要完整Windows/Emmy验收的已审模块批次在提交消息显式含 `[rust-native]`；可选workflow_dispatch亦保留，但不依赖其是否存在于default branch。快/原生concurrency分开，不让快检查排在长组后。不取消正在运行的关键363d8d4证据。
- 全仓长测、资源会话与性能集中到可用候选阶段；最终完整验收不删除。新本地Rust运行须恢复有效disk/process admission、原channel租约与证据保留，不按死PID/目录年龄夺取旧lease。此前本地Cargo禁止状态仅描述旧lease缺口，后续按实际安全准入更新。

### Activation/Doctor 与受控 Repair Delivery 集成候选

- Activation/Doctor R2 候选 `02474e47b64aa8777144e8b23760a52bac38c156678610c5db28d03f2088f15b`，freeze SHA256 `6a807a33be7ec344a377a69904f20f0c10733082bf79e6aca99a0f131d55dc13`；独立复审关闭三项 P2：reload 只读不恢复 torn journal、未知输入保留 waiting-runtime 投影、index 先于 owner 发布。source/reachable 各321项已核验，七处集成声明逐字纳入。
- Native repair 重新读取原 CON 的 durable closing/未closed/原target-release-owner及精确unbind envelope；同一 publication lease 保留 fresh INPUT、单次 witness、route和burst保护。clean-main confirm/release 仍正常校验完整 inventory；modified-main confirm/release **仍待受精确原operation/result-durable条件约束的能力扩展**，未宣布完整修复完成。
- Delivery R2 freeze `d541a32b475248e5e49ed87b1a4ded2579dc9d433594c3713da1263109d8dd98`；独审与14owner/16deps/38测试映射核验通过。真实流程为有界完整release快照→调用host validator→原client身份复核→window/maintenance/installation锁→不覆盖发布与500槽维护。CLI validator消费者仍待接入。
- 两项P2已修：receipt资源先检查唯一path再按map比较；清理保留已认证原始文件/目录/祖先句柄直到句柄绑定删除与checked close，不按已关闭路径删除替换物。CREATE_NEW临时文件仅原句柄可清除；关闭结果不确定则留存。单一Windows FFI边界已独审，4个snapshot根/每目标64个临时slot的保留上限与容量错误明记，不能以自动删除不确定资源恢复容量。
- 组合以a353固定依赖和已审Delivery R2构建；原Delivery快照core/lib含未携带的并发raw_json声明，因此组合明确采用a353该文件，不伪称原快照完整可编译。旧R1缺失依赖字节/失败stage与全部原证据保留。
- 新40 activation/doctor＋38 repair回归全部 **NOT_RUN**。静态rustfmt与独立源码审查不等于Windows通过；新增三个独立CI组覆盖repair、controlled lock和owned-handle cleanup，activation由既有formal组覆盖。默认CLI还未切换，四阶段最终审计/性能仍未开始。

### 363d8d4 首次修复后构建观察

- run `38661264087` / product `116185104371` 仍在运行。all-target check、sealed CLI build/smoke实际成功；early artifact `11939886461` ZIP SHA256 `edc6efcaa1e562e3bd190cba1688adf39c4da1f55a52d7e6d9fee8a71afd8b1e` 已校验大小、路径和全部index文件hash。构建 source digest `71d0cb27744732d11d85e9e2060e1738d32f92b9c1f4f3e978a9c54e5dda04fa`，check owner `ecde983b-c525-440f-a73c-0e1d292a43ed`、build owner `819f7f56-ab79-4f74-9d93-dc19097295ff` exit0。
- 固定Go Lua解释器build和profiles执行step首次SUCCESS；最终artifact的实际匹配数量/完整日志尚待核验，暂不宣称214项全部通过。真实describe差分仍失败，后续独立组继续执行。此树不包含本节新Activation/Repair候选，也不包含a353的Execute字节候选。

### 正式默认 Live CLI 与 native 原始报告集成

- CLI R3 freeze `6d7fc1eb25703c2d29fb91bd90c5da6a2261aa81171b4dc0cbefe7909f6cd238`，原审复核关闭原6项及R2新P1。默认正式CON/activation/doctor/reload与真实repair validator走新service；probe/instances复用原安全实现，旧非CON OP/BTP仍是真实未齐项，不宣称全部Live完成。
- 参数逐flag错误顺序、PID拒绝+前缀、文件读/close/control原cause、doctor query stage及空nextStep省略对齐。typed renderer不经Value，错误伴结果/JSONL仍保留。validator只做认证release/TOC/XML闭包/固定luac `-p`，不运行addon。
- entered-runner所有Err均保留compiler子目录和validator父目录；owner.json发布前就关闭自动删除能力，atomic临时保留叶及未知lock close在有界typed聚合中传播，避免父Drop抹掉尚未证明回收的资源。8根admission容量记录每根路径/保留原因，不按PID或年龄清理。成功runner worker-join证明和Closure I/O/control仍由后续窄修复处理。
- RawReport R2 freeze `1d5c2c61ac1b2a8f38e004afb5f2c8464a4ab24f5f73bf04b3da0872e9b70d28`，独审核229项。Go兼容迭代字节JSON grammar、四native gates与显式scoped byte formatter保留非法UTF8 string字节/duplicate keys/checksum，普通serde_json遇非法UTF8仍失败；无unsafe/TLS/global缓存。真实archive-only历史fixture强化，不把活跃checkpoint误称archive读取。
- 原CLI R3依赖副本仍缺已审module声明，未篡改该历史freeze。`formal-cli-integration.json` 新组成从完整固定b83a4d3 crate树＋精确CLI R3/Raw R2 owners建立436项source快照，包含实际inspect_controlled及七声明；4 crate＋CLI递归rustfmt解析成功。全树format check发现既有evidence/bounded格式差异，保留日志，不以此改写无关源码；编译及新CLI实际任务仍 **NOT_RUN**。
- 新55 CLI/ordered-validator＋15 raw回归声明不等于已运行。新增三个独立unit组及固定luac构建后显式ignored实测组；本次普通push不触发长原生，完整默认入口/Source生命周期/内存观测可用后集中带 `[rust-native]` 验收。
- 本地快速验证实际发现4个旧测试/hash锁不适配，修为完整84命令、保留7旧路径拒绝，contract生产advertised/arity字节未改；固定Go oracle无改。重跑Node **51/51通过**，保留首次失败及第二次日志。新Linux纯模块工具初次实际执行原始production raw_json **4/4通过**（23.57s symbols编译），owner `9227754a-3a35-456e-8a8c-14acead7be80` 的6份sealed日志已核hash；本地runner独审尚在进行，不冒称全workspace/Windows通过。

### Source 内存观测与 Emmy 补丁边界

- 已保存完整Retail报告均 `performance.peakRSS=not_measured`，无可信实际占用数字；4036文档/32MB源码与runner主机16GB不是运行内存。下一有效原生批添加按phase同步的Job成员WorkingSet/PrivateBytes采样，区分冷索引/暖查询/native语义/常驻会话、controller/CLI/worker及未分类成员；报告采样峰值/稳态/缺口，不叠加各进程各自峰值，不把失败run当完成数据。
- 独立只读Emmy审计确认当前29路径补丁未最小化：生产新增2248行，另1857行为35测试。优先可移回自身adapter的领域DTO/类型投影/provenance/references报告与独立VFS共享输入扩展已列下一架构任务。递归跨文件上下文、session/epoch对象身份、unknown/absent/返回基数及真实checker完成观测仍需某种内核接缝，外层过滤不能等价。两个默认不启用checker仍可能被文件级diagnostic opt-in触发，不视为已证死代码；先映射消费者/合同并保35回归，再小批缩减，当前不声称已删或性能已提升。

### 363d8d4 终态及本地纯 Rust／Source 回收修复（2026-10-15）

- run `38661264087` 已结束：product `116185104371` 失败、Emmy job 成功；不是整体验收通过。最终 artifact `11942796058` ZIP SHA256 `7d2c05de10ec463a72f5a2ce69f1b1e665eb71b81265660d18da38c86fea4b60`，125 owner／2268 sealed log 哈希已核。固定 Go Lua profiles **214/214 实际通过**，owner `6983fdb9-addc-4714-b262-cfde5d8c164f`；三个 Go host bridge 仍未完成。固定4036文档静态组通过，不代替 native 语义。
- Source native refs/context 分别约184/180秒 deadline，冷 refs 的 native_analyze 阶段163865ms。API CallbackType 原始声明存在，但当前 environment_type 不读取 catalog.declarations，TimerCallback 投影 unavailable；单测和完整 Retail 的 known 断言真实失败，需保签名的有界声明投影，不降低断言。Skill 两个 native 任务同样 timeout。实际性能和内存仍未测，未通过运行不算可用性验收。
- quota/idle/lifetime 三个 IPC 组均在 bootstrap-open exit2：缺少 --snapshot，尚未进入真实256请求／60秒idle／600秒lifetime。project init 已真实成功。正在修公共 index→snapshot→refs 合法调用；静态测试不冒称 IPC 通过。
- 三个独立失败为实际 Rust exit101，均非 wrapper timeout：slot fixture 的 Windows verbatim PathBuf 吞掉 ParentDir；Hotfix successful scan 后仍断言零capture；SQL 在内部 Value 上断言原始指数拼写。候选仅改测试：原始非canonical路径与目标先验、scan后完整capture/pin快照不变、public writer和archive精确数字字节。父独立窄审通过，历史24日志哈希保留，三组 Windows rerun **NOT_RUN**。freeze `613a3852500ad096cb91cd5cde02f5d6bc9cf2f24b2cbc9e3c45150a398d96f2`。
- Source process lifetime 候选 freeze `76b8a6eeb5595b204c2fda6e2f6d024576c3694becaf146364b22b3b1d5eee44` 已独审：父线程 try_wait、成功root状态、stdin/stdout/stderr worker完成与join、attached Job清空后才成功；失败按原cause有界回收，仍未证明完成时明确保留。新增14测试，全部23包含helper均未运行。旧attach race及部分std handle Drop边界未在此批假称解决。
- Legacy luac 与 ordered 共用 entered-runner scratch disarm；Err/unwind保留，既有typed cause/receipt与取消边界不变；pre-run与确认成功后保留legacy best-effort清理语义。新增10回归与既有8项均由现有selector覆盖。独审核三owner、process依赖及历史证据通过；Rust/Windows **NOT_RUN**。
- 本地纯模块 runner 已独审闭合4项发现，使用独立symbols admission、bubblewrap私有PID namespace、只读精确production副本、工具链/依赖/配置身份与封存日志；不占用或抢夺旧ordinary lease。覆盖原始production raw_json及Mailbox/CodeBytes，不是全workspace。独审实际重跑 **10/10 Rust通过**（32.81s），owner `63fa515c-5996-4201-9eb1-9edda1210f44`，receipt SHA256 `fcd22660adbbe30d5cfb22477e8d8da2b10b40a8ea6d071d1396c85d58cef876`；11 Node测试通过，17sealed logs/12依赖/110tool身份已核。freeze `f3dec24b0fc722ada17ce4dcb2d501ce273ebb85cc5b2066e49d8dd589b087a6`。此前setup失败证据保留。
- 用户最新明确保留完整XML/Lua增强设计并尽可能解耦Emmy；这部分增强区别于固定Go官方LuaLS/LSP的NotChecked范围。自有adapter首片、内存观测与SQL语义候选另行审核，当前未据此声明补丁已缩减或全部功能通过。最终四阶段收敛审计尚未开始。

### Source 分阶段内存观测、IPC 入口及 SQL 语义候选

- 内存观测 R2 freeze `a1300d2c76e07d8c8ba937b4c6d8e2bd087412b445a2cfe245fa7ebdd1282899` 已独审通过。沿既有受控 Job 读取同步成员 WorkingSet/PrivateBytes，creation FILETIME 保留字符串，区分controller/CLI/worker/未分类；kernel单进程历史peak与采样同一时刻进程组peak分别报告，不直接相加各自峰值。冷index、warm-index、静态暖查询、native及持久session阶段各自保留结果与缺口。
- 集成首次误对工作目录旧experimental wrapper运行造成1项静态失败，原日志保留，不作候选结果；改用完整R2精确stage加独审IPC文件后实际32项：30通过、2 Windows跳过。
- 原R1发现 warm-index 未列required，EPERM留下旧ended marker时即使所有PID见过也可能假报完整。R2同时补required和所有记录phase完整性，4个distinct PID全部observed/unseen0仍不能掩盖marker失效。独审实际95 Node测试：84通过、11 Windows跳过；另4语法检查通过。未采样/失败运行不报sampled_successful_run；shutdown/handle gate不放松。**真实 Windows/Retail 内存仍 not_measured**，等待下一有意义原生批。
- 控制器与旧共享support未混用：memory的两个process wrapper按精确独审stage字节纳入，工作目录旧实验副本不作发布依据。前后recipe/custody身份据新实际字节计算，不能把旧support hash追认当前。
- IPC bootstrap 现先经公共source index注册project pin，验证返回context.snapshot精确等于fixture pin，再refs明确传snapshot与唯一symbol。三个mode共用；父独立复跑Node14/14通过、原29失败证据保留。只证明合法入口生成，真实CLI/Windows IPC仍 **NOT_RUN**。
- SQL语义R2 freeze `fd60c7a5dda8532722b2b64e93c47c962af7eaa0247fd0d332885ddef9920e21` 已独审核12owner/31deps/19Go/5stdlib/61历史文件。实现固定commit的mapping、enum/flags/build condition、精确i64/u64、definition复用、空catalog及static/effective joins/provenance，控制检查贯穿load边界。长s/Kelvin按Go ASCII-origin SimpleFold类匹配；Node源码验证28160比较及删除映射mutation通过，17项Rust回归 **NOT_RUN**。新增supplement保存原freeze漏列但已读核的精确Go build_match.go，不修改历史freeze。
- 资源准入仍有真实差异：16份1MiB语义定义的Go逻辑80MiB可接受，而当前Rust重复charge推到144MiB会拒绝。逻辑预算与Rust保留分配安全账应分别核对，不能抬高Go limit或冒称实际内存。resources输出合同、CSV、EXPLAIN继续开放。

### 自有 Emmy adapter 首片与下一原生批

- 新 `lycheedev-emmy-adapter` 私有持有API declaration分析模型、FileId和BindingSession；Source保留文件/manifest/目标认证与公开结果，adapter返回owned DTO，沿原control/step budget。无反向Source/core/Windows依赖、无Graph复制，crate forbid unsafe。只是首个生产入口外移，当前第三方补丁并未缩减，也未修CallbackType缺口。
- R2 freeze `2676850e0d11683846579ddc1a3b9a494a224943b1c44f7319408e494b347a29` 已独审。scanner要求新crate forbid；build custody分别强制Source optional与adapter非optional的精确Emmy版本/path，执行前后双manifest保护。supportFiles改变会合法改变recipeSHA，固定upstream/patch/tree身份未改。
- 实际25 custody/unsafe Node通过。原build fixture在/tmp仅2.14GiB可用而需4GiB时6项被正确拒绝，原日志保留；同一测试使用workspace临时目录且不改变门槛，7/7通过。新增adapter Rust测试11项（含8MiB/取消边界）**NOT_RUN**，独立native job已接实际integration target，不用0test假绿。
- 原adapter R2依赖副本含另一在途native control修改且缺其子模块，故不以该副本冒充完整组合。新 `adapter-native-integration.json` 从固定a3c10d1完整crate/tool/recipe源＋已审adapter owner建立542文件组合，采用原正式native_research.rs；control R1独审P2指出sidecar writer按路径重开未绑定parent原file，**本批明确排除**并继续修，不将未审修复混入测试树。
- 本批用 `[rust-native]` 触发有意义的真实Windows验收，优先取得cold/warm静态及native阶段内存、默认CLI和adapter11测试结果。全仓native仍可能失败；不等全部功能才测内存，也不把sampler实现等同已测。Source预算传递/phase和callback修复各自继续，不扩大deadline掩盖失败。

### SQL 逻辑资源计费与 modified-main 修复收尾

- SQL资源R2在同一个原子准入中分别维护Go逻辑账与Rust安全账。definition逻辑metadata按4×实际字节及ceil(bytes/65536)decode计费，安全保留仍按8×；executor只预留一次64MiB逻辑额度，其具体执行与发布仍受独立安全账约束。resources在source复核/SQL执行之后、发布前快照。此片限定纯语义/无物理provider；physical/effective逻辑计费仍未全部审齐，不能整体冒称等价或RSS。
- 独审发现cold definition并发publication winner路径少了Go重复read/charge/validation。R2新增Core原子strict create/create_controlled，完全保留旧put/put_controlled同字节幂等；same-byte既有记录也严格conflict，事务退出后复用cached read重收费/rehash/validation，再比较内容。预算或validation原cause优先，取消/锁等待/COMMIT回滚边界保留。五个确定性loopback竞态和两个Core严格创建测试新增，旧batch明确metadata4×/retained8×。14个映射声明全部 **NOT_RUN**；原审复核通过，新freeze与R1历史均保留。
- Modified-main repair现在仅允许精确原CON的confirm/release收尾：每次重新认证owner/target/transaction/original operation/ticket/fence和durable阶段。confirm须report_candidate；release还须读取验证standalone结果artifact，不凭release_ready字符串授予。prepare/commit仍走clean inventory，不给予通用豁免。原fresh INPUT、同publication lease及burst约束保留。
- Finalization freeze `bb57f3757f26253b240708b4443f7375f196f2df690aa88ffbe7360ac4ffc437`，七owner/162deps/102Go来源。独审关闭artifact预算演进、completed历史恢复、Go escaped结果大于2MiB上界、legacy预算初始化四项；17个repair fixture仍 **NOT_RUN**。组合使用固定7489 Windows closure，排除另一路正在修改的file_identity helper，原freeze与snapshot hash均保留。
- 这批SQL五组独立测试入口已接Windows CI：semantic catalog、logical ledger、resource output、definition conflicts、strict metadata create。普通push仅持久化，不替换在跑7489原生证据；新增测试等下一意义批次执行。
- 固定Go3057已刻意退役旧OP/BTP execution/resume，不应恢复旧协议。剩余迁移范围是只读status/session、prepared-only OP cancel、显式OP/BTP abandon及exact owner退役、resume准确exit2退役错误。当前migration_incomplete错误仍需对齐；此前笼统“OP/BTP未齐”不代表必须重建旧执行链。

### 7489 原生批编译阻断（2026-10-15）

- run38670108226的Emmy job116210989364已SUCCESS，最终日志数量尚待artifact核验。product all-target及sealed CLI build均失败；不是采样正在正常进行。early artifact11944491855 ZIP SHA256 `361c8a6af8999558e6423075ff210fc88088b4b81385c0110a290dbd9ef51b59` 及119条index文件hash已核。
- 两个失败均为Delivery formal_repair/publication.rs100 的E0282：闭包result的错误类型在map_err字段访问前无法推断。最小修复为第67行显式 `let result: Result<bool>`，不改变闭包、原cause或retention语义，父独立窄核通过；当前尚未重新编译，不能称编译修复已验证。原错误日志保留。由于无sealed CLI，本批不能提供所需完整Retail内存结果；修后再次原生验证，不以预算推算占用。

### 2026-10-15：编译恢复 checkpoint 与本地 Source 纯逻辑实测

- `ca7830a9` 的原生 workflow 因 plain scalar 中 `definitions:: ` 的冒号未启动产品 job；失败保留，`8a57231e` 改用 block scalar，并实际解析 YAML（3 jobs）后启动 run `38671670245`。后续每次 workflow 修改均须真实解析，不能只用 diff-check。
- `8a57231e` early artifact `11944552995` ZIP SHA256 `0a447562362389eab992f109a4948cfc5a6d2f2204586439f92fd567f5a88a11`，119 个索引文件逐一核 hash。all-target owner `5956a70f-ec82-4608-a4df-b9f755e2e614` 与 CLI owner `bd092958-f245-4efd-a40d-e5227ba9a0ff` 的完整 stderr 均显示一个根因：`ActivationPhase` 的 `#[serde(other)]` 不在最后，令 derive 失败并产生四条级联 trait 错误。前一 Delivery `Result<bool>` 错误不再出现。本次仅移动 Invalid variant 至末位，保留 snake_case、default 与 unknown 映射；修后 Windows 编译仍待新 run，不声明已编译。
- 当前 `8a57231e` Emmy job `116215966932` success；产品独立组继续运行，但 CLI 构建失败使 full Retail/内存测量不能开始。内存数值仍 **not_measured**，不能将主机总内存或预算作实际占用。
- 后续尚未合入的 Source 行索引/有界 stderr 观测候选已实际通过 Linux 本地纯模块 21 项（原 10、行索引 4、观测 7），0 failed/ignored/filtered，编译 22.92 秒、测试 0.06 秒。owner `d2608611-bb54-49d1-a490-d3dec35db9b4`，receipt SHA256 `35cbac7277d2cb5174d0fc8fa155e277a8cde22ca6587709b1288586a46e23b7`，17 sealed logs 核 hash。其先前错误包含真实 process 集成测试、无法在纯模块 crate 编译的失败 owner `9fb69182-7bd9-4cbc-a981-3136fbbbc6f6` 保留；原集成测试完整移动至真实 process::tests，由既有 Windows selector 覆盖，仍 NOT_RUN。这 21 项不是全 Source、Windows、性能或全 Retail 通过。

### 2026-10-15：Source 完整增强保留、owned callback 投影与控制预算候选

- 此批基于 `79ef742f`（activation serde 排序最小修复另行启动原生 run `38672577841`）普通 push 保存，不替换其待运行原生批。`tests/rust-parity/source-quality-integration-snapshot.json` 记录固定已提交工具/Windows/配方闭包与精确候选覆盖；不混入未集成 legacy-read 工作树或旧 support 文件。独立静态审查通过只适用列明冻结，不能当编译通过。
- owned Emmy adapter 补齐 callback 类型投影：原生 `TimerCallback` unknown 的已知原因是已有 catalog declaration 没有进入 environment type。自有层递归解析参数/返回/nilable/cardinality/provenance，歧义、循环、深度与工作预算 fail closed；没有硬编码该类型。保留完整 XML/Lua 增强，Go 原引擎并不提供该增强。引擎内核仅新增 CallableSignature 语义接缝及转换（2 生产文件 6 行），旧 35 回归保留并加 1 项；adapter 24 项对应生产实现，尚未实际运行。重建 patch SHA256 `eff914ec33bd8bf3bb67096bd2136994c71f90b710b5e0032876bc25b8771c86`，tree `828cfad0692b79595e1591649b33013916f8dd8cc5e8230d74c70f0b56a5088b`。冻结 `owned-emmy-callback-freeze.json`，不是已实现全部去耦合或零 patch。
- 原始行号查询由每次 prefix 扫描改为一次逐文件 newline 索引，保持 byte offset、CRLF/LFCR 首字节、BOM/UTF8、EOF/半开区间语义；64MiB 仅可选缓存容量上限，达到上限/分配失败回退原可取消扫描，不删结果、不作为产品内存配额。类型化取消/超时沿公共 Result 保留。冻结 `owned-original-lines-r2-freeze.json`；4 个纯模块测试真实通过，prepare 集成测试新接入独立 native:: CI，尚 NOT_RUN。尚无耗时占比或性能改善实测。
- native worker control v3 使用原总预算内绝对截止时间，扣除启动/传输/解析并预留 500ms，不再父级剩余约164秒却给新worker180秒；父 monotonic deadline 仍是硬边界。私有 deadline exit85+唯一marker 经严格匹配映射原公共 deadline，异常/伪造不覆盖原错误。phase 从子进程 stderr 固定64KiB前缀观测，无子进程路径写入或新增 Windows FFI，锁冲突/poison/容量耗尽永久停止观测、不拼接漏段。best-effort 诊断不保证末事件落盘，亦不证明完整慢因。R1/R2路径sidecar设计已撤回未发布，原失败保留；发布R4冻结 `source-native-control-r4-freeze.json`。
- 本地 exact production 21 项与17sealed logs见 `tests/rust-local-pure/freeze-source-r4.json`，runner Node11项本次真实通过；workflow实际PyYAML解析3jobs及run字段。完整 adapter/native控制/实际process/4036 Retail/Skill/session/内存仍须对应候选实际验证。当前性能目标与四阶段串行审计仍未验收。

### 2026-10-15：历史 BTP 只读与精确退役错误首批

- 此批完成 non-CON resume 在 workspace/native factory 之前返回固定 Go 退役 exit 2；不复活已在 Go3.2.0 退役的 OP/BTP 执行链。BTP status 只读历史记录并保留 missing/corrupt/render 错误，无键盘/截图/native connection/replay能力。
- 复用 Go typed decoder，保留重复字段、null、slice backing 重用、raw invalid UTF8、时间反序列化/序列化边界和10000层JSON深度合同。Core仅新增受原 OperationControl 管理的只读原始字节读取：打开前截止检查、bounded SQLite wait/checked close、1MiB preallocation边界和 TEXT/BLOB byte length。既有 strict create/put 语义未改。
- 冻结 `legacy-read-freeze.json` SHA256 `4e3bd1aa066ad13b9c5fa9190ddfbb839281a0e074df08cfff454edb0b6adac1`，9owner、218依赖、200固定Go文件及8 Go toolchain来源文件经独立审查核 hash，所声明首批无阻断项；16映射测试均 **NOT_RUN**，只作 rustfmt 解析/diff检查。入批前9owner再次核 hash；相对3ea1155的依赖唯一变化是已独审的 activation Invalid variant 顺序修复，业务逻辑未变。
- CI新增独立 history::tests 8项与 metadata::historical_read_tests 4项，另4项由现有 formal_live 模块过滤覆盖；实际PyYAML解析3jobs及run字段。此为测试接线，不是Windows执行通过。
- 仍缺 OP status、历史 SESSION 验证、prepared-only OP cancel、显式 owner OP/BTP abandon、原有 workspace诊断；下一只读两路需要真正的历史 signal/report/capture 身份与证据验证，不能拿现代 mailbox Identity 替代。未来变更两文档必须原子 expected-generation CAS，不能顺序独立put冒充事务。完整历史兼容尚未交付。

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
