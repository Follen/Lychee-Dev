# Rust 迁移交接：唯一当前事实来源

更新：2026-10-16 11:26 UTC。分支 `rust-refactor`，持续交付到[草稿 PR #14](https://github.com/Follen/Lychee-Dev/pull/14)。最新已核远端批次：`3336d8240f5a32cbf3ec4d5e910cc316ea746d0a`。实现进行中，尚未完成全量验收。

## 1. 当前决定与不可漂移基线

- 唯一验收目标：以固定 **Go 3.2.3** tag 源码为当前行为基线，实现 Rust 宿主并保留 **EmmyLua 直接分析库/native-worker/session** 替代 LuaLS。除此之外，全部业务、同一 addon、CLI、Skill、完整 Mailbox 协议、错误恢复、诊断和输入合同对齐正式版本。
- 当前 Go tag `v3.2.3`：tag object `7a24532cf0fe294faeb96e8224797b6fe69386d5`，peeled commit **`3766f9cacb52e93a803a626187e2a1d54a445468`**；2026-10-15远端tag/main与本地精确对象已核。322→323为2提交、41文件；业务在该正式提交，另一个提交为322发布文档。新增自适应Data内存/准入容量、紧凑记录、IN选择及Data Skill，详见[323增量矩阵](docs/toolkit/rust-go-3.2.3-increment.md)。新增验收PENDING；未验证发行二进制/资产。
- 历史 Go tag `v3.2.2`：tag object `97890b267a620a166889db0934e57a826d72a872`，peeled commit **`0bec60e4406f5d5dc155ee25cef44f9a710e37e8`**；2026-10-15 精确远端对象与本地 fetch 已核，检查时 main 同 commit。Release API 当时返回 404，不能声称发行二进制/资产已验证。业务增量 `969fe480fdb38eb038401044c3327b04ccaaee81`，详见[3.2.2 增量矩阵](docs/toolkit/rust-go-3.2.2-increment.md)。
- 前 v3.2.1 tag object `637584b8b976966c3863c1889e9ff86fe55493d4` / commit **`10b561db5706702cd42e047eaeed4ccb5e825351`** 和 v3.2.0 tag object `93849f43f34d8ff1d6f4539c5a3496ab941ef229` / commit **`3057a9666ab078270b63c7f06de748069b0d1f4b`** 的源码、fixture、二进制、测试/失败证据均保留原身份，不能重标为3.2.2通过。未完成的[3.2.1 累积增量](docs/toolkit/rust-go-3.2.1-increment.md)仍须完成。
- 2026-10-15 用户最新版本决定：未发布 Rust 候选源码/包版本 **3.5.0**，发行配置渠道 **beta**（版本与 npm dist-tag 分开，不自动改成 `3.5.0-beta.1`）。Go 行为 oracle 现为上述 3.2.3；只获准同步源码和配置，未执行发布、tag 或 npm 渠道变更。版本同步已完成源码集成及本地包合同检查；精确 b16a 候选已通过 Windows all-target 编译和 CLI 构建，后续提交及完整产品验证仍按各自证据记录。
- 2026-10-15 13:39 用户明确要求“全量替换”：最终验收必须移除全部 Emmy 上游源码补丁，使用未修改的固定上游公开 API，自有 crate 承接完整 XML/Lua 增强、类型/诊断/来源/取消与资源合同。此前“尽可能减少、必要时保留最小 hook”的最终豁免已被此要求取代；已有补丁仅属过渡实现，不能作为交付完成。逐 production hook→实际消费者→自有替代→差分/全仓证据列迁移表并分批实施，不另写整个 LuaLS、不削功能或假造 checker coverage。现有原版5/5小样及 public rendering27+27/450trace证据只覆盖各自范围，不能宣称全量零补丁已完成。
- Rust 调查起点：`5443314c536e925801d3f0360110db946181d1b3`。它是复用基础，不是完成声明。按模块差分决定保留/替换/补齐，不从零丢弃已有成果，不沿用不符目标的旧协议。
- unsafe约束：自有业务/跨平台Rust代码默认禁止unsafe，用适当crate级forbid/deny守住；确有必要的Windows FFI/内存边界必须最小化安全封装、逐处SAFETY理由和指针/生命周期/线程约束与测试，独立审查，不整模块无限豁免，不为性能目标放开unsafe。第三方依赖另列审计，不能宣称整个依赖图无unsafe。
- 内部设计应体现Rust特点，不逐行翻译Go：合理所有权/借用减少复制，enum/类型状态、Result、RAII和有界安全并发按实际收益使用，避免过度抽象/炫技。对外合同不变，unsafe边界不放松，性能仍实测；纳入独立架构验收。
- Skill尽量复用固定Go3.2.3版本；仅Emmy引擎、安装和真实诊断等必要差异修改，不因换语言重造工作流。
- 性能：可控耗时减少至少 70%，即 **Rust ≤ Go 30%**，排除下载与不可控外部等待；同机同数据、输出与校验等价，cold/warm 分开独立重复，逐路径记录。未测不能保证达标。
- 所有需要用户、实际游戏或特定本机互动配合的测试放在本文最后；离线与可自动执行的 Windows CI 先做。Windows 专属测试不自动等于游戏测试。
- 首批交付仅文档；随后已获准全量实施本计划。仍不操作游戏、不发布、不合 main，不修改 Go PR #13 或其他项目。只保留本文件，不创建 `handoff` 文件或第二份交接事实来源。
- 交付约定：为Rust分支创建独立草稿PR；之后每批获准代码改动及时推送并更新同一草稿PR，不等全部验证完才提交，也不每批新建PR。先查已有PR避免重复。后续实现授权不等于游戏操作、发布或合并授权。

[DESIGN.md](DESIGN.md)解释目标结构，[todo.md](todo.md)给执行顺序，两者从本文件派生；更新状态与证据首先改这里。旧 `docs/toolkit/` 和历史 README/UI 设计是原受测范围的记录，不得覆盖本节。历史“Go3.1.2替换”“Rust≤Go50%”“重新设计四槽exact-COMMIT”与旧输入合同已被本次目标替代；历史测试通过仍只证明原始候选。

- 2026-10-15 18:24 用户调整实施节奏：先按完整端到端功能实现并接入默认生产消费者，再集中补测试和统一验证；取消自行增加的逐小片冻结/独审/CI先行门禁。实现过程中仍及时分批提交推送同一草稿PR，真实安全边界和文件单写协调保留；已有CI旁路诊断，不等待每轮才继续编码。最终四阶段循环审计与完整验收要求不变。

## 2. 当前实现与有限剩余项

- **Data 默认链已接入**：本地/CDN、Prepared/cache、SQL、导航、asset、Hotfix、自适应内存与 checked 收尾均已有实现。当前重点是 SQL 默认线程栈安全：原合法 scalar 深度63在 parse 实际溢出；完整堆任务栈解析/降低、迭代树 clone/equality/drop 已在云端改写，仍在收尾同步 provider callback 的混合嵌套执行和错误析构，尚未保存或实际验证，不能称问题闭合。
- **Emmy 已使用原版848文件，语义验证未完成**：`3336d82`已保存 initializer/provider 重复查询、call quality 多余查询和完整键集合被未知值污染的修复。原固定参考与当前候选的全量类型/provider/coverage差分保留；最新 adapter 另四项失败正在逐项用固定公开 API 合同核对，不能统一改期望或隐藏额外证据。
- **HTTP 后继已保存待 Windows 复验**：`6051063`使用固定 native-tls/schannel 三源38加4删的最小适配，将SNI名与证书校验名分离，保原验证与tokio所有权。完整来源/许可/逆补丁和打包已接，Node20/20通过；真实五项Go对照及取消/错误连接器仍待测试。Emmy零补丁不等于整个依赖树零补丁。
- **Source 正确性与性能仍未验收**：短预算隔离/暖态回收已有实际测试；完整Retail仍在原90秒内未完成。最新 dev 候选准备约72秒，API虽写完回复，最终 capture 仍耗尽总预算。不得增加deadline、减少输入或校验来通过。优化候选同机Go323配对入口已保存，实际结果尚无；先覆盖 describe 与静态Retail index/query，不能外推完整 B01–B07。
- **正式 Lua 已明确切到 Go323**：`4092344`保存113原blob认证、精确Rust3.5 overlay与220/9正式入口，旧320/321/322原清单保留。Node正式63及包/版本115通过；本机缺固定MSVC组件，用户已授权官方必要组件安装，实际220/9仍NOT_RUN。

## 3. 最新实际验证与证据

- [89f61b9 run38880882451](https://github.com/Follen/Lychee-Dev/actions/runs/38880882451)已终态：static/context成功、product失败。final产物211 owners、8194 sealed日志逐SHA匹配，ZIP `1534ce9c1aca3718ec62d2b53b69f46e9f66884f44731c25c8e8b92e49d03c17`。all-target/CLI、Core、slot、正式旧Lua与桥、HTTP及IPC等已执行组通过；adapter、固定oracle、keys、local SQL、relational、Skill、fullRetail失败，不报全绿。
- **89具体失败**：adapter lib50通过4失败，object member6通过1失败；member缺键Nil/Unknown真差异已在3336修，其他四项核固定函数参数/诊断/metatable合同后复测。relational在 scalar63/parse发生 `0xc00000fd`，未超deadline且完整回收；另外8个SQL集成target共43项通过。keys旧fixture与local SQL scratch准入已在1f9修，随后本机定向1+1+15实际全部通过。
- **完整Retail**：4049文件/4036文档及1091 XML/4018映射认证通过；普通refs/context/API和retained冷启动均原deadline失败，后续依赖暖态无前置而未测。API最终 `source.refs.commit_capture` 耗尽预算，不能把worker产出当CLI成功。retained同身份child观测private峰1,995,444,224字节，低于2GiB，未证明OOM；全Source多进程集合private峰2,495,275,008字节且有采样缺口。Skill静态流程通过，唯一原生refs/context同样deadline。此为dev失败记录，无70%速度或内存验收结论。
- **本机ec503**：process47、native_session28、trace storage5通过，父启动故障peer1项单列ignored。固定oracle reference36/36、candidate29通过7失败，双方4137行/38段，完整回收；大边界trace输出修复已实证，语义差异仍保留。公开差异见[PR证据](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6213988386)，3336修复尚待同输入复验。
- **本机4092344当前批**：Root/keys定向17项及CLI dev构建已通过，正式Lua等待缺失的固定MSVC14.51.36231/SDK10.0.26100.0准备，优化配对测量独立准备中。后续普通push不打断固定SHA测试、不自动排重复长CI；本机只测试，云端负责实现修复。
- 历史run、原失败和前版交接正文完整保留在[历史证据](docs/toolkit/rust-migration-evidence-2026-10-15.md)。四阶段最终循环审计尚未开始，实机清单仍在本文最后。

## 4. 进入最终四阶段前的有限收口

1. 结构性修复SQL输入驱动的parse/retrieval/lowering/execute/drop递归与错误释放；不能以Box缩小帧、抬线程栈或降低原深度代替闭合。修复当前编译、测试输出/收尾与已确认SQL/Data/Core失败，保真实错误、预算、部分结果和提交事实；完成HTTP剩余Go既有边界，不扩成新数据库或通用网络栈。
2. 在同草稿PR保存完整修复批，集中跑实际生产链与原全量比较；外层验证必须有界失败并让后续独立组执行，不提高产品deadline。
3. 验证Source短预算/暖态回收与完整Retail输出、诊断、性能/内存；未完成组清楚列NOT_RUN，不以unit或静态Retail替代。
4. 完成上述实现闭环后进入下节四阶段循环。继续以完整功能为单位实施与集中验证，不恢复逐几十行独审门禁。

## 5. 验收与循环审计

完整[F01–F13功能、P01–P14协议、B01–B07性能和Go来源合同](docs/toolkit/rust-migration-acceptance.md)保留；[321增量](docs/toolkit/rust-go-3.2.1-increment.md)、[322增量](docs/toolkit/rust-go-3.2.2-increment.md)与[323增量](docs/toolkit/rust-go-3.2.3-increment.md)是累积要求。代码已实现、静态已审、某树历史通过、当前实际通过必须分开。

- 固定Retail数据为Gethe/wow-ui-source `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b`：4036 Lua/XML/TOC，32,269,051字节。每次核身份后全仓分析/代表查询/源码定位/refs/context/flow/cold-warm/session/释放，不以小fixture代替。
- Source、Skill、Data分别由独立使用者执行真实CLI任务，记录输入、完整输出、正确性/可读性/来源/错误和复测。实现者不自评通过，无法运行时记blocked。
- 全量实现后的四阶段**顺序循环**：①架构/全业务/Skill/全部代码；②有效覆盖与无独立价值冗余；③真实消费者确认后的死代码；④前三维度联合全仓。每阶段独审→修复→实际验证→独立复审至无新可行动finding，再进下一阶段。每批源码审不等于最终收敛。
- Go323测试映射为当前差分oracle，Go322映射仍保留，320/321原证据保留；尤其未知结果不重放、事务、取消、空map、槽迁移与错误恢复。记录固定树、覆盖分母、平台排除与停止条件，不保证数学零bug。
- 性能必须同机同数据/结果与校验等价，独立cold/warm逐路径重复；可控耗时Rust≤Go30%，外部等待另列。当前仍未达成验收，不能把缓存预热或删校验当提速。

## 6. 本机线程接力验证入口（2026-10-16）

用户指定现有本机线程接力，双方只经同一草稿PR同步源码与结果。先拉取本节标题上方已确认commit，保存自己的未提交修改，读AGENTS与本文件；不要把另一环境的历史lease复制或清除。89f61b9集中CI已终态；当前本机固定4092344测试，后续普通push不自动排重复长CI。

1. 在本机明确记录OS/架构、Rust1.99.0、Node24、Windows SDK/编译器和可用空间。Windows正式产品命令沿[当前workflow](.github/workflows/rust-parity.yml)的准备步骤：认证原版Emmy、受控`rust-cargo.mjs --owner local-fetch -- fetch --locked`，随后`--owner local-check --one-shot -- check --locked --offline --workspace --all-targets -j 1`。Linux只能报告其真实支持的子集，不假称Windows测试通过；不用裸Cargo绕现有owner/cache控制。
2. 优先复验历史失败：workflow中完整adapter组；固定oracle的`node tests/rust-source-engine/fixed-emmy-oracle/controlled-run.mjs`（保全部输入与分段trace）；Data `relational::`与原joins/CTE/derived/recursive资源组。保持原63/64深度与预算，保存完整panic/退出码及实际句柄回收，不能只改期望。
3. TLS新组按[专用README](tests/rust-parity/tls-name-oracle/README.md)先运行一次真实remote组，再以其精确owner执行Go对照；Go1.27.1与私有测试CA准备沿同workflow，不安装系统信任。随后依workflow检查Core修复与其他失败组，不能跳过尚未跑的组来报全绿。
4. Source真实短预算/暖态回收及完整Retail使用现有sealed CLI、package与native acceptance入口及固定Gethe输入；性能另用[专用优化候选配对入口](tests/rust-performance-paired/README.md)（Node42通过、10个Windows项未在云端运行；实际基准未跑），不拿dev单次结果冒充70%达标。报告commit、命令、通过/失败/跳过、证据位置与结果摘要回同PR；不操作游戏、不发布。最终四阶段审计仍未开始。

## 11. 最后执行：需要实际配合的测试清单

**本节必须始终是handover的最后一节。** 只有前述可独立完成的实现、离线差分、自动Windows测试、封存候选和可自动性能检查就绪后再约实际配合。当前全部 `not_run`；以下“结果”故意留空待真实执行填写。每次先固定candidate commit/exe/package/addon hash、产品/build、所选安装/PID/创建时间/HWND及授权操作；缺客户端或用户未就绪留缺口，不缩范围。普通Windows API单元/自有子进程/合成窗口CI不在本节。

| ID | 配合/前提 | 步骤 | 预期证据 | 结果（实测后填） |
| --- | --- | --- | --- | --- |
| H01 | 用户指定真实安装和角色；七产品逐一可用 | Retail、Classic、Classic Era、Titan、Forever Asia Beta、Forever CN Beta、Anniversary独立identify/connect/execute/report/cleanup/disconnect | 同candidate各产品身份、完整精确receipt和清理；两beta不按同build合并；不能以Source/Data成功代替Live | |
| H02 | 选定未安装或旧200槽真实客户端，用户可手动reload/重启 | 首装/升级后doctor；无可验证INPUT时手动reload；未发现500槽时重启；原CON resume | 未盲发输入、原目标/CON不变，旧200仅升级识别，500发现/启用/加载事实分别记录 | |
| H03 | 用户允许安全编辑框/聊天焦点和键位冲突场景 | 测两实际绑定、已有绑定保留、聊天/普通EditBox透传、secure/protected拒绝、UIParent隐藏、最小化和全部键盘VK按住/释放、500ms静默中再次活动、活动与已开始chord重叠、仅释放本attempt按键 | 不改文字/光标/焦点，不抢前台，不Esc/Enter；恢复handler，500ms后新鲜INPUT、原deadline不延长、submitted/uncertain不重放及精确receipt各自证据 | |
| H04 | 同安装双实例、跨build并行及混合多实例就绪 | 分别连接/执行/关闭，交错共享槽发布和选定实例reload/doctor --fix；至少Retail双实例，其他产品依声明范围补齐 | 独立身份/owner/fence/CON，短锁预约不串线，其他实例不被授权/升级结果冒认 | |
| H05 | 用户允许真实容量与reload/relogin验证 | 连续运行触发500槽完整操作容量边界，先日志后reload/重绑同请求；reload/relogin前后验证 | 旧槽不复用、预算不延长、目标固定、exact报告与清理，运行时退役明确 | |
| H06 | 用户同意可控故障与恢复窗口 | bind无回执关闭、submitted/uncertain中断、发布替换、release无回执5秒cleanup reload、退出/重开CLI、opaque与observation区别 | 无未知业务重发；原request/ticket/期限/attempt保留；报告可用和cleanup分离，退役后才runtime_destroyed | |
| H07 | 用户同意在选定真实角色设置受管槽禁用并恢复 | 按角色GUID启用、复查并保存；doctor只读核slot_activation；有新鲜INPUT时按原CON reload，模拟多次replacement与可控保存失败 | 不LoadAddOn、不消费槽、不动foreign元数据；slotReloadRequired未清不发槽动作，reload自身可执行；原deadline/交易/unknown不重放，失败不报ready；记录恢复步骤 | |
| H08 | 同机同数据Go/Rust正式候选与用户允许的只读/有界Live采样 | 按B06独立cold/warm交替重复，真实游戏等待分段；保持同业务/输入/校验 | 原始计时/输出等价、excluded阶段与总wall时间，未达0.30照实失败，不影响前面可自动工作 | |
| H09 | 若既有“十分钟空格”人工活动仍由用户确认需要 | 先确认当时任务目的和安全场景，再由用户实际配合；不把历史计划当持续输入授权 | 人工行为与工具输入分开标记，明确时长/目标/结果，不冒充CLI输入或协议通过 | |
