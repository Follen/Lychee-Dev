# Rust 迁移：当前交接

更新时间：2026-10-21。此文件只保留当前状态；架构和合同见 [DESIGN](DESIGN.md)、[业务覆盖](docs/architecture/agent-task-business-coverage.md)、[领域迁移](docs/architecture/domain-navigation-migration.md)、[整体实现清单](docs/architecture/reconstruction-whole-plan-ledger.md)。原始失败和历史报告保持原身份，不重标为当前通过。

## 目标、来源与执行边界

- 沿用 `rust-refactor` / [草稿 PR #14](https://github.com/Follen/Lychee-Dev/pull/14)。已接受 [GitHub 交接](docs/handoff/2026-10-21/HANDOFF.md)和[统一验证队列](docs/handoff/2026-10-21/VALIDATION-QUEUE.md)，交接实现基线 `cfdefd46a11ec8dce470d296b424a3eaa1372b8b`；原交接包的 NOT_RUN 是冻结时事实，后续结果以本页为准。
- 顺序：Source/Data 业务质量不低于 `f811fc7f75fc206158cecb01dd0eb5d39e88b35f` → 完整实现和 correctness → 同任务 Rust 可控耗时 ≤ 固定 Go 的30% → 最终五循环。Go参考固定 v3.2.3 peeled `3766f9cacb52e93a803a626187e2a1d54a445468`；不跟随其他分支版本变化。
- 当前产品为3.5.0 beta配置。真实 Skill anchor `75cd762e31df0dfd614c8687f9fd3fdb26b6a7f7`、25文件 tree `c1768f92d55b454cafb202eb6c2835af1ecc2811`。Agent是正式Data入口，migration-oracle仅作旧业务对照，Release不启用。七产品、双Forever身份、Mailbox v1、未知输入不重放、精确数字/coverage/CAP合同不变。
- 用户要求全部隔离测试，不写入现有 `~/.agents`。Skill/npm安装仅在项目隔离目录；测试用户HOME/USERPROFILE/CODEX_HOME、AppData、temp和npm配置/cache均由当前owner持有。编译器及离线依赖缓存保持既有位置，不重新安装工具链。`runCargo`新增显式isolateUser选项，后续native队列启用；已核现有delivery/CLI Skill写入用例均显式临时target，未走真实用户发现。
- 本机主代理独占 native 队列和此文件，开发与固定提交测试副本分开；其余代理只在分配文件内工作。保留 main 的独立任务改动及 `.worktrees/rust-refactor`。无游戏、合并或发布授权。
- 全部 Rust 调用经官方 owned runner，600秒/组、默认stack、locked/offline/j1、no one-shot，默认20GiB构建cache/5GiB scratch。Data产品容量另为十进制5GB。已完成一次获批 incremental 清理6,819,246,069字节，112 manifest/2135引用及其余cache前后核验、双lease释放；不因此扩大删除范围。

## 已实现与实际验证

- 固定测试副本及已推送分支为 `b6cf60d731bfd97a7b11276da82323209729a3ee`，tree `0c5350739715b5aaabe753e30fa4dd27f08d06ae`。该版本全部9 crate默认及migration-oracle两套native check通过，真实用户目录隔离已在owner记录核验。
- CORE-FIXTURE-01：853原例839通过/14失败；14个迁移后锁序、CAS、recency/durable peak旧夹具已修，受影响130项在 `1764e19c8c` 逐名全部通过，产品未改。
- b6cf Core其余6个integration目标65项全部通过，完整native退出和资源回收已核；CLI其余integration合计18通过/5失败/3 ignored，5失败的实际原因及当前修复见下表。
- DATA-RUNTIME-01 / DATA-CORRELATION-01：执行流schema、eager布尔错误、公有ID流内缺失证明、安全等值关联键已修并推送；Engine165项在 `8819608054` 全部通过，新增多键MAX和CAST短路回归已通过。
- Data library当前1529项，1528 active、1 ignored。逐名对账1528 active均有真实pass，缺名0、仅fail未pass为0；最后通过来源分别为1764的524项、bc77的2项、8819的165项、f29c的811项、e0c6的26项。这是跨版本覆盖，不是e0整套单次通过。原整组600秒timeout及所有旧失败保留；nested child排除，输出混排的解析遗漏只作独立只读核定，不重跑或改原结果。
- DATA-REMOTE-FIXTURE-01：最后4个remote夹具已修并实跑通过；准确使用durable peak、共享catalog序列化，取消后两库不存在且readonly返回record_missing。取消身份、未发布pin/CAP和私有容量边界保留。
- CLI-SOCKET-01：accepted socket恢复blocking后，原4项Windows真实supervisor测试全部通过。EOF半关闭、stdout背压、整epoch取消、私有入口拒绝已实跑。
- CAP-TRANSPORT-01：Data实际Session query/capture和Navigate自动CAP在stdout flush后observer失败的2项新回归通过，核唯一CAP、原body/manifest和query handle；全部Agent Session消费者22项及12条Navigate业务路径同组通过。b6cf CLI bin184项、supervisor6项全部通过，含真实BrokenPipe保留committed CAP、父进程终态统一失效标记及排队deadline。精确non-yield CPU poll及真实private receipt断线仍无实测。
- Data integration原e0已执行43目标中的40个非空目标：275通过/9失败/21 ignored；其余3个目标仅含ignored，不计业务pass。b6cf针对9失败复验，DB2六项、hotfix一项及content Root修复一项通过；selected修复例暴露真实DATA-LOCAL-ORDER-01，尚待修复后复验。Agent native parity2项已在e0通过。b6cf CLI/Core队列现已结束并完整回收；失败组保留原报告。
- Node原399项393通过/3失败/3权限skip；旧Skill差异清单和metadata布局3失败修后对应43项全部通过。后继Source exporter/corpus工具5套84通过/1权限skip；本批真实Skill入口及用户环境隔离的4套Node68项全部通过。重叠套件不累加为独立覆盖，mock/静态检查不代替Windows业务。

## 当前修正与负责人

| ID | 状态与边界 | 负责人 / 下一步 |
| --- | --- | --- |
| CLI-TERMINAL-01 / CLI-FIXTURE-01 | b6cf bin184及supervisor6全通过，原19个bin旧合同/种子问题及BrokenPipe终态已闭合 | 主代理已实跑 |
| DATA-INTEGRATION-01 / DATA-HOTFIX-FIXTURE-01 | DB2六项、Root重建和hotfix共享归属八项实际复验通过；selected重建另归下面真实问题 | 主代理已实跑；旧报告不改写 |
| DATA-LOCAL-ORDER-01 | selected缺失body仍有合法alias/CAP/keep时，提前发布配置provenance触发严格库存检查，阻断精确修复。local_query.rs及正式Agent使用的sql_reader.rs均已延后一次配置发布，保留原关闭和严格拒绝边界；新增真实Session缺体恢复/坏体拒绝回归 | data_engine已交回；主代理已核diff，待native；local_query方案经core_metadata限定只读复核 |
| CLI-PUBLIC-FIXTURE-01 | b6cf实际四失败分别为prune旧85计数、normal exit旧help/解析器文案两例、source inspect空值旧文案。3个integration文件已更新当前71目录、分组help与clap结构；原退出码、无home副作用、输出完整性及prune行为不变 | core_evidence/主代理已修改，待native |
| DATA-NUMERIC-01 | b6cf numeric page实际records.result_type；Arrow NullArray的is_null恒false，结果scalar match遗漏Null。已补ScalarValue::Null及原scalar边界回归，整数/负零/其他类型拒绝不变 | data_engine已交回，主代理已核，待public page/JSON/CSV/CAP实跑 |
| BUILD-RUNNER-01 | Release、Source full-corpus/fresh场景统一600秒，fresh exporter默认no-oneShot和原20/5GiB；isolateUser令用户目录归owner scratch，本批补齐buildRust透传、Release CLI及三exporter准备显式启用。原prepare身份规则保留；19项对应Node/mock通过。实际Release/exporter未跑 | core_metadata/core_evidence已交回，主代理已核 |
| SOURCE-INSTALLED-01 | npm smoke在隔离prefix执行真实global布局安装；--installed运行固定25文件Skill launcher，使用npm生成的prefix入口。旧local安装报告仍直接进包并标not_run，staged模式保留；未造shim/改Skill。Node及隔离回归通过，实际npm/Source业务未跑 | 主代理整合完成 |
| COVERAGE-01 | 9 crate共150个lib/bin/test声明；补漏emmy-adapter/binding-contracts/delivery共26目标待执行。binding lib合法0-case、两个harness=false helper不计业务pass；ignored单列 | 主代理；core_metadata已交回Data逐名汇总 |

## 下一步

1. 保存本批Data产品和CLI夹具修复，保持唯一native队列，不在运行中切换副本或并发Cargo。
2. 集中保存本批修正并固定新提交，执行两配置check及实际受影响用例；已通过且无新影响的CLI184/supervisor6和Data组不重复运行。
3. 完成尚未执行的Data/CLI/Core integration、Windows、Source、Live和补漏26目标。Lua5.1可用；外部oracle/真实数据、symlink权限等ignored不算通过。自有隐藏窗口/子进程的离线Windows测试不等于真实游戏测试。
4. correctness和资源回收通过后构建生产Release，实际npm安装匹配25文件Skill，执行no-home发现、installed Source及Agent Data。三项exporter/importer显式准备，不在测量内隐式构建；旧SQL私有metadata fixture不能拿来测新Agent。
5. 再执行现有owned paired协议：完整任务、20对、不扣helper成本，CSV仅correctness；历史单pair、诊断profile和旧产物均不是当前性能验收。当前无≤0.30或全产品矩阵通过结论。
6. 最后五循环依序为架构/业务/Skill、覆盖有效性与冗余、死代码、联合全仓、Rust原生度。需要真实配合的项目始终留在最后，不从历史计划推导操作授权。

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
