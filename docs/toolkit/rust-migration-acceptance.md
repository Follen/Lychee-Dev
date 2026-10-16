# Rust migration acceptance contracts

Derived acceptance detail, not a second status ledger. [handover](../../handover.md) is the sole current decision/status source; [todo](../../todo.md) orders the work. Historical evidence stays separately pinned. The original full functionality, protocol, performance and audit matrices are retained here; current behavior oracle is Go3.2.3.

## 2. 当前 Go 3.2.3 合同索引（历史证据仍各自固定）

| 合同 | 固定来源 | 使用方式 |
| --- | --- | --- |
| 产品/架构/业务 | [Design.md](https://github.com/Follen/Lychee-Dev/blob/3766f9cacb52e93a803a626187e2a1d54a445468/Design.md) | 提取模块和产品合同，不复制其运行状态为 Rust 状态 |
| 全量命令、参数、输出 | [command_contract.go](https://github.com/Follen/Lychee-Dev/blob/3766f9cacb52e93a803a626187e2a1d54a445468/internal/command/command_contract.go) | 固定 tag 对应候选 binary 的 describe/help/正常及错误 golden；不能沿用旧89命令数量 |
| 协议全貌 | [docs/live-protocol.md](https://github.com/Follen/Lychee-Dev/blob/3766f9cacb52e93a803a626187e2a1d54a445468/docs/live-protocol.md) | 全部8节与实现/测试共同组成验收，不只移植定位 |
| 协议实现 | [channel](https://github.com/Follen/Lychee-Dev/blob/3766f9cacb52e93a803a626187e2a1d54a445468/internal/live/channel)、[memory](https://github.com/Follen/Lychee-Dev/blob/3766f9cacb52e93a803a626187e2a1d54a445468/internal/live/memory)、[bridge](https://github.com/Follen/Lychee-Dev/blob/3766f9cacb52e93a803a626187e2a1d54a445468/internal/bridge)、[desktop](https://github.com/Follen/Lychee-Dev/blob/3766f9cacb52e93a803a626187e2a1d54a445468/internal/desktop)、[addon Bridge](https://github.com/Follen/Lychee-Dev/blob/3766f9cacb52e93a803a626187e2a1d54a445468/addon/Bridge) | 原样保留语义及边界，差分正常/失败/中断 |
| Agent 工作流 | [Skill](https://github.com/Follen/Lychee-Dev/blob/3766f9cacb52e93a803a626187e2a1d54a445468/skills/lycheedev/SKILL.md) | 比对公开业务步骤、命令/flag和错误恢复；只改真实引擎/构建说明 |
| 开发/发行/许可 | [development](https://github.com/Follen/Lychee-Dev/blob/3766f9cacb52e93a803a626187e2a1d54a445468/docs/development.md)、[licensing](https://github.com/Follen/Lychee-Dev/blob/3766f9cacb52e93a803a626187e2a1d54a445468/docs/licensing.md)、[release](https://github.com/Follen/Lychee-Dev/blob/3766f9cacb52e93a803a626187e2a1d54a445468/release) | 固定包身份、对应源码、清单和自动验证 |
| Go 原始验收范围 | [todo](https://github.com/Follen/Lychee-Dev/blob/3766f9cacb52e93a803a626187e2a1d54a445468/todo.md)、[handover](https://github.com/Follen/Lychee-Dev/blob/3766f9cacb52e93a803a626187e2a1d54a445468/handover.md) | 参考已知限制与故障，不把 Go 实机通过转记成 Rust 通过 |

Go 基线若需要变更，必须明确记录新目标、精确 commit、原因和差分范围，再重建受影响 oracle。不能因远端 main 前进而自动滚动。

## 4. 分阶段迁移与退出条件

1. **冻结oracle和盘点。** 下载/构建对应Go发布产物并核对commit/sha256；导出describe/help、默认值、输出schema、退出码、Skill和addon清单。建立逐命令矩阵，每项链接源码/fixture/结果。差分不得改变main或Go工作树。
2. **复建Rust依赖。** 优先实际获取已验证公开Emmy upstream固定commit，核验archive/tree/许可证，按所需API/语义差分优先直接复用；此前最小可review补丁只属过渡复建，按2026-10-15最新硬验收最终必须全部移入自有crate并还原未修改上游。用户已明确允许这条新依赖重建路线；不能把原私有目录缺失作为全任务长期阻塞，也不能把新补丁伪称历史原字节还原。保留新来源/补丁/测试身份，更新旧custody假设而不伪造原manifest哈希。固定工具链、Cargo.lock和对应源码后，隔离两套fresh Cargo home顺序离线重建，遵守配额/所有权。
3. **保留基础并先自动差分。** core/selection/project/evidence/delivery、parser/CLI/Skill、Source、Data分别建立正常/错误/恢复golden。逐模块决定保留、适配、替换或补齐；保留首次失败和修复后结果。
4. **完整Live协议迁移。** 先导入固定Go Lua/协议fixture与模型故障集，再接runtime/RIP默认host、输入、日志和恢复。复用同一addon，先离线再Windows自有子进程/合成窗口，不操作游戏。
5. **清理与一致性。** 删除经证实无消费者的旧LuaLS/WGC/光学/旧协议或其他死代码，修正已失效文档入口；每批有引用审计、替代覆盖和受影响回归。历史证据保留，不清用户数据或未登记缓存。
6. **候选/性能/自动Windows。** 同一封存Windows amd64候选验证CLI/addon/Skill/包闭包、安装升级卸载、错误恢复、离线对应源码和性能矩阵。需游戏采样的性能分项转到最后专节，不阻止此前自动工作。
7. **最后实际配合。** 仅在上述前置可独立完成项闭环后，运行最后一节清单；记录同一候选身份、真实环境和结果。缺客户端/用户未就绪保持not_run。
8. **交付评审。** 全部必需矩阵逐项有证据，未通过项明确保留；文档交付不等于Rust产品完成。发布、npm标签变更、主分支合并另行授权。

## 5. 全量功能验收矩阵

下列为迁移验收要求，不是全行当前执行状态；已执行候选、通过/失败和未覆盖项以[当前交接](../../handover.md)的精确证据为准。最终应扩展为固定Go命令表的逐命令行；本表不是用工作流概览替代全量覆盖。每行记录Go/Rust commit与binary hash、输入pin/hash、命令、输出/退出码diff、耗时、日志与结论。

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

以固定Go协议全8节和实现为准。下面的精确数值/行为来自该发布合同，不使用旧Rust协议推导。各行仍须逐项证据闭合；正式自动化子集已有实测，当前状态见[交接](../../handover.md)，不能由子集通过推定整行或实机通过。

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

各路径保留独立实测状态：ac8的B01-describe已测且未达标，B02静态子集尚无有效样本，其余路径不得据此推定通过；最新证据见[交接](../../handover.md)。先证明F/P正确性，再判断性能。每个必需场景Go/Rust交替各至少20个有效样本；cold/warm各自独立20组，明确如何冷却/预热及缓存身份。失败/超时样本保留并使正确性门槛失败，不丢弃后只统计成功快样本。固定CPU/电源/负载/机器、工具链、binary hash、输入和输出哈希；随机或交替顺序减少系统偏差。

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
2. **测试覆盖充分性与冗余测试。** 固定Go3.2.3测试是当前行为场景oracle，既有Go3.2.0映射作为历史基底保留：建立Go测试→Rust对应/不适用理由/未覆盖映射，保留错误恢复、未知结果不重放、事务、取消、协议、空map、槽迁移等有效边界；不机械复制冗余平台夹具或旧死实现测试，不以数量堆覆盖。 检查关键分支、失败/恢复/状态组合及真实断言质量，补有效回归；只有证明无独立价值才删重复测试。修后实测覆盖，保留分母、排除项、平台范围与报告；覆盖率数字本身不是充分性证明。
3. **死代码专项。** 核查真实消费者、动态入口、feature/平台条件、CLI/Skill/生成器/打包与工具调用，确认后清理并回归，重复直到无新可行动项。
4. **前三维度联合全仓审计。** 架构/业务/Skill、测试充分性/冗余、死代码一起看，循环修复和验证，直到当前固定树无新可行动finding。

每轮登记stage/round、被审commit或tree、独立审视范围、finding、修复commit、验证命令/产物/结果和复审结论。修复后旧树的无问题结论不能转给新树。停止条件是已覆盖约定范围、已知可行动finding闭环且独立复审未产生新可行动项，有证据支持；不宣称数学意义零bug。需要用户/实机确认的例外只登记到本文最后，保持未验证，不等待它们才推进其他可独立工作，也不将其算通过。发现跨阶段新问题时补修与受影响复审，不为遵守形式顺序掩盖已知缺陷。

### Source全仓固定真实输入验收

不得仅用小fixture验收。复用历史Retail分支固定commit `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b` 的完整Gethe/wow-ui-source输入：历史记录为4036文档（2551 Lua、1091 XML、394 TOC；32,269,051B），完整XML处理还记录4018 inline maps/sourceFiles6569。重新获取后核对真实清单和内容，不把历史计数当新扫描结果。完整导入/索引/分析后实际执行代表性查询、正确文件/符号/行/源码定位、refs/context/flow、cold/warm、持续session、关闭及资源释放，与固定Go同输入差分。保留全量加载闭包/映射和原预算；旧Retail08部分连续成功与Retail19“运行中”均不能替代当前候选全仓通过。若用户另指定具体分支，另固定其精确commit并新增全仓矩阵，不隐式换输入。

### 独立Source、Skill与Data实际任务质量循环验收

Source、Skill与Data分别安排新的独立代理，不能由实现者自评通过，也不能只靠静态文档审查。先完成可运行CLI的自动准备；使用当前固定Go3.2.3合同和真实CLI任务证据发现问题、改进实现与回归，不为审美改变既有业务合同。

- **Source独立验收**：代表性真实查询须准确、易读，能够可靠拿到正确文件、符号、行号及源码；覆盖中文、大结果、无结果、错误、refs/context输出、分页/截断/coverage/uncertainty。记录输入pin、完整命令、预期、原始输出、定位证据、问题与复测；不将实现者的解释当成CLI输出证据。
- **Skill独立验收**：由新代理实际按Skill完成调查，检查入口发现、参数正确、步骤简洁、恢复边界和无旧协议；Source优先使用工具内固定版本来源，不随意改搜GitHub raw代替既定source pin。遇阻应按真实错误/恢复合同处理，不绕过约束。记录实际调用轨迹、失败点、修改和独立复用结果。
- **Data独立验收**：由未实现该片的代理实际执行固定产品/build/pin的查询、表/记录/SQL、Hotfix及CSV/资产导出任务，核结果与版本来源、provenance链、partial/coverage/无结果及错误恢复是否可靠可读；覆盖取消、预算、无效输入、缓存冷热和失败后不产生伪成功capture。使用固定Go相同任务作为差分证据，保原始CLI输出和产物身份，不能以fixture数量或静态review替代真实使用。
- 三条验收各自执行“独立使用/审查→修复→实际验证→独立复审”，直到当前冻结树无新可行动finding。实现期可先验证已就绪候选，不能据此跳过全量完成后的四阶段串行循环。
- 三条验收纳入第一阶段架构/业务/Skill循环，并在后续测试充分性与综合审计中复查；CLI不可运行或依赖缺失时记blocked，不以模拟回答判通过。需要真实游戏或用户配合的部分仅移至最后清单，其余独立运行。

