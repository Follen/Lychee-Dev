# Rust 迁移交接：唯一当前事实来源

更新：2026-10-16 10:43 UTC。分支 `rust-refactor`，持续交付到[草稿 PR #14](https://github.com/Follen/Lychee-Dev/pull/14)。最新已核远端批次：`ec503c9a1734e29d67dd5c8ffe6f17caab5336f2`。实现进行中，尚未完成全量验收。

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

## 2. 当前实现与未闭环事项

- **默认业务链已接入，仍在修真实集中失败**：Data 的本地/CDN读取、Prepared/cache、SQL、导航、asset、Hotfix及独立控制/预算/checked收尾已接；Go323自适应内存与紧凑记录已组合。当前收口是实际SQL资源/部分结果合同、HTTP边界的实际Windows验证及失败测试，不能按功能清单称成熟或验收完成。
- **Emmy生产已零上游源码补丁**：固定原版848文件由自有语义/诊断层接入；完整等价仍须集中验证。8a3历史参考在两个16384边界case输出单文件超过16MiB，输出assert在析构中再次触发并abort。已按原每段上限分段保留全部trace，接入受控外层收尾；Node23项及外壳3项通过，Rust分段/异常peer及两侧完整比较待重跑；不得删除原448输入、覆盖证据或边界检查。
- **Source短预算隔离已保存**：私有analysis child保warm状态，沿原请求绝对deadline与整个子树aggregate 2GiB，owner无同步可选sidecar写；失败确认回收且未知不重放。8a3的28项生命周期unit已通过，实际1秒/warm idle及完整Retail后续组因前步耗尽job未执行。原90秒/2GiB不扩张。
- **剩余性能验收**：此前e8完整Retail四路径均耗尽原90秒；API worker虽已写回复，caller未完成。其失败run source进程集合采样峰WS2370.62MiB/private2502.21MiB，有采样间隙且含多进程；retained warm/steady未测。后续准备/身份索引/释放优化不能在新完整结果出现前称改善；尚无同机Go对照或70%目标证明。历史CLI由dev配置构建，不能作为release性能对比；新配对入口使用固定Go323完整包与优化Rust候选，先覆盖describe和静态Retail index/query，仍未执行。
- **HTTP有限边界后继已组合**：原始Host、非UTF8/空authority/未知proxy scheme的错误阶段与固定请求行投影已接；TLS保Go精确验证名，IP通过公开配置禁SNI且仍严格验证。13新增Rust正在当前集中批验证；后继已接5个私有CA TLS loopback观察与同次封存Rust输出的固定Go1.27.1对照，Node13/13通过。实际TLS后继尚未执行，默认Windows系统根验证仍另有边界，未宣称多尾点等价。
- **本次集中候选**：SQL保原合法深度63/拒绝64边界，移出递归帧的大型AST临时值、move嵌套Query代替clone、迭代展开UNION；3新增测试及原崩溃用例尚未运行，栈问题不能先标解决。五项SQL旧fixture按Go323改正并保负例。Data九个失败的五类fixture已按规范路径、真实body长度提示、必需provenance、CAS generation与typed cleanup原因修正；新增2回归待跑。
- **当前修复状态**：149d90已保存adapter集合类型编译修复。8a3另一个all-target失败为Lua integration测试仍访问移除的Script.source字段，已保存改为source_bytes()，89f61b9的all-target实际编译通过。Core非法历史大行fixture与并发临时目录碰撞、SQL及Data对应修复均已保存，运行复验仍在进行；bundle原目录计数漏了初始化既有的`.lycheedev-locks`，已改为严格对照操作前完整目录名集合。

## 3. 最近集中验证的真实边界

- [89f61b9 run38880882451](https://github.com/Follen/Lychee-Dev/actions/runs/38880882451)：10:38核实static/context成功，product仍运行；all-target编译、CLI构建、真实describe、静态Retail及22–75各组已通过，76 adapter组失败（详细封存诊断待取），77固定oracle失败且已受控返回，78–96各组成功、97安装组运行中。SQL深度、分段trace、完整Retail与后续产品组尚不能报通过。

- [8a3 run38854701834](https://github.com/Follen/Lychee-Dev/actions/runs/38854701834)：context/static成功，product取消；final产物122 owners/2210 sealed日志逐SHA一致。CLI构建、真实describe、静态Retail、正式Lua/桥、slot维护17/移除8/修复25、Source准备47/research65（另2ignored）及生命周期28等组实际通过。all-target旧Script字段与adapter旧集合类型失败。历史参考cargo真实异常退出；outer在06:29报错后直至08:08取消，后续组未执行，不能当成用户取消或完整产品通过。最新149修复不在该run。
- [fbba run38850381033](https://github.com/Follen/Lychee-Dev/actions/runs/38850381033)：context/static成功，product失败；191 owners/1171 sealed日志逐SHA一致。CLI/Source多数目标受旧cleanup Result类型编译阻断（现已修）。独立Data remote284等组真实通过；另有SQL断言及IN测试栈溢出、Hotfix、asset、Core等真实失败，不能全部归为编译连锁。
- 两次产物ZIP SHA分别为 `1b7404f8430c2daa048f9f1d1cf99bf982bd06cfc602c33f861d673e75846d7e`（8a3）与 `0e6074a9bcedd905ec60877a8242c0555916642b134ffa28633e465eda281bd7`（fbba）。测试完成与外层回收分开记：参考cargo已回收，不等于取消的outer/preparation owners全部确认回收。
- 本地Node/静态检查可执行；原Rust构建owner尚无合法完整回收证明，保留lease，不新开cache/channel绕行。用户已选择继续Windows CI。独立A/B/C历史31×3/450trace/6outcomes通过只适用于其原冻结输入；当前变更仍须受影响验证。
- 原始批次、owner/hash和失败记录见[历史证据](docs/toolkit/rust-migration-evidence-2026-10-15.md)。PR正文更新曾两次被拒，仍暂停该动作；已授权源码推送继续，未发布或合并。

- 本机ec503实际复验已确认SQL三个0xc00000fd，合法scalar深度63最后停在parse；TLS两多尾点SNI与固定Go不等价；keys redirect fixture与local_query scratch_bytes另有失败。编译已过不代表这些运行问题解决。本机负责测试与PR证据，修复仍在云端进行。

## 4. 进入最终四阶段前的有限收口

1. 修复当前编译、测试输出/收尾与已确认SQL/Data/Core失败，保真实错误、预算、部分结果和提交事实；完成HTTP剩余Go既有边界，不扩成新数据库或通用网络栈。
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

用户指定现有本机线程接力，双方只经同一草稿PR同步源码与结果。先拉取本节标题上方已确认commit，保存自己的未提交修改，读AGENTS与本文件；不要把另一环境的历史lease复制或清除。当前89f61b9集中CI继续，后续普通push不自动排重复长CI。

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
