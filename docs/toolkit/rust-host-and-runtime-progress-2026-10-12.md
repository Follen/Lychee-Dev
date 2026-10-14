> Historical reference for the Rust migration (2026-10-14): current decisions, baseline and acceptance status are in [handover.md](../../handover.md). This document preserves its original scope and evidence; conflicting requirements, protocol designs, performance targets and release instructions are superseded. Historical test results are not current-candidate passes.

# 2026-10-12：Host 事实、Beta 身份与运行时扫描

状态：进行中。总任务入口为 [完整目标/验收](rust-refactor-goal-acceptance-2026-10-12.md)。本页仅记录本批受测范围，不代表 Source、Live、Data 全量完成；后续改动需要绑定新的受测产物。

最新 Mailbox 路线已改为参考 Go v3.1.0 的运行时指令特征/RIP 解析动态 Lua 根，再沿 state/globals/Mailbox 读取，见 [总方案 §7](rust-refactor-plan-2026-10-08.md#7-mailbox-运行时指令定位与-lua-路径读取)。本页私有堆 scanner 和 typed Deadline 的通过仅保留为历史原型证据；停止其产品接入、后续扫描验收和堆扫描回退。可复用的进程身份/点读/预算机制须按新路径重新验证，不转移为新定位通过。

## 已执行的分项

| 范围 | 实际结果 | 产物、输入与限制 |
| --- | --- | --- |
| Source host 材料准入 | 26 固定断言、163 外部检查通过；受控进程/树/句柄收尾通过。 | `.tmp/rust-emmy-source-host-admission-fixed-01/report.json`；build receipt `0866b9bb-145a-4efb-b4f0-62ddf620e105`，source `f17d8063a78592aa1fe813f99152c2cb7c15e479cdadbc4c5b29068a3c26c064`，freeze43 `3dff211ff17c495dc549712f042c364ddcc319a85a7c186ba6b5f2914e23757f`。仅准入，不含 host 值执行或 flow。 |
| Core/Data/Live 联合回归 | 106 通过，2 个既有测试 ignored；其中运行时扫描 28 全通过。 | `.tmp/rust-beta-discovery-batch-02/report.json`，receipt `21b04b12-4e63-413b-ba00-526f8258967e`，source `f9d07b484adac1e8225d62667b15fcc571e3286bbc04d602d0ac9c0292889ebe`。Core lib 11、Data lib 26（1 ignored）、installed parity 2（1 ignored）、Live lib 39、scanner 28。不是当前后续源码的全量验收。 |
| Source host 统一执行事实 | 12 cases / 144 断言、301 外部检查、1,376 独立检查通过；12 份原始 Lua 输入经固定 Lua 5.1 stdin 语法检查，不执行源码。 | `.tmp/rust-emmy-source-host-runtime-fixed-02/report.json`，build-04 receipt `4609a74d-93b5-4332-b4f4-04e76f97784d`，source `b88b5d43b1fdf46c2c09036bc8b9416a3563ed16bf62b1406cd2fd8b2be77410`，freeze47 `7468fb5b47ac69b2970460e5c53e1c8b322925a15831246ba8d44293dfcfb87b`。匹配 EXE/PDB、203 输入、完整 source snapshot 和私有 Emmy patch 已保全；进程树/句柄回收通过。显式 entry、未知加载顺序、guard/secret flow 和生产 API 尚未验收。 |
| Windows/Live 当前进程回归 | 96 passed、2 个既有 ignored；包括 20 项新 Windows 适配器/私有内存测试及 28 项 scanner fixtures。 | `.tmp/rust-runtime-windows-batch-01/report.json`，receipt `cb1bd44e-afe6-4188-b1fb-ad372ceeed3d`，同上 source `b88b5d43…`。Live lib 39、scanner 28、runtime_windows 10、Windows lib 12（2 ignored）、private_runtime 7；三个新 process 单测包含在 Windows lib。已封存五套实际 test EXE/PDB、完整源码和日志。仅测试进程/隐藏窗口，不是游戏或生产认证验收。 |
| Beta 客户端识别 | 固定元数据 tests 区分相同 `1.60.1.70338` 的两 product；共享解析贯通 project/target/availability/local Data。 | `forever` / `Forever Asia Beta` → `wow_classic_beta`；`cn-beta` / `Forever CN Beta` → `wow_cn_beta`。Branch 为发布元数据，不证明玩家地区；CN 的 Source interface 仍未知，不套用 Asia 固定 commit。生产连接/Skill/完整升级链仍待验证。 |

Host 准入 build 已封存同次 EXE/PDB、195 固定输入、完整私有 Emmy patch 和完整本地 Rust source snapshot（293 entries / 5,458,191 bytes）。联合回归也封存实际源码与五套同次 test EXE/PDB。内容摘要不能代替这些实际文件。

## 保留的失败与独立审查

首轮联合构建 `.tmp/rust-beta-discovery-batch-01/failure.json` / receipt `69172909-e3ed-4b57-970b-8c815f099633` 因测试 `sum()` 的类型歧义 E0283 失败；完整原输入与日志保留。显式 `sum::<u64>()` 后同批回归通过。

严格 Clippy receipt `e50e26c5-1773-4f09-8a07-73a7c503c2ea` 因新 scanner 的四项 collapsible_if 失败。已做等价条件整理，尚待最新源码严格复验；不得把首次失败改写成通过。

Source host 执行首编译 `.tmp/rust-emmy-source-host-runtime-build-01/failure.json` / receipt `b440019b-8026-4062-a26d-f9bf4a34d6d5` 因 probe 使用不存在的 Graph::type_view 而非既有 Facade::type_view 失败。Windows/Core/Source 库实际已编译；probe 未生成 EXE/PDB。原 source snapshot、freeze44、全输入与 patch、日志保留，调用点已修，待第二次构建。

续验：build-02 实际编译通过（receipt `1e41617a-45a2-4214-a1df-56384cadc38c`）。首运行 fixed-01 的 144 断言、语法及进程回收均通过，但独立检查器错误地要求裸文件名，实际既有 graph 合同为 `index:file`，12 项覆盖标识检查失败。核对 graph 构造后修正检查器为精确带索引序列，原失败保留。freeze47/build-04/fixed-02 全部上述检查通过。build-03 在 Cargo 启动前被原有缓存配额挡下，只保全输入，不算编译失败或通过。

运行时 scanner 的独立只读审查发现并修正：致命读错被吞为零长度、提前命中漏计已搜索前缀、局部截止未传给适配器、BODY fixture 未先核授权长度。追加实际局部到期→同请求回退和到期后身份失败 tests；这些属于上述 28 项已运行范围。

Source host 统一事实只读审查发现：query ID 手拼无效、条件返回把 max 表示宽度当 exact arity、未知调用列表尾被当 scalar、条件返回丢传播来源、捕获 owner closure 被当 captured value。结构 capture references、有限 arity alternatives、open-tail 传播和带条件 return candidate edges 已在本次 12-case 验证，包含四个新增固定反例。原八个 host Lua fixtures 和原 36 flow 输入均未修改。原无 host control/query/native 基线尚待对当前源码复验。

Windows scanner Source adapter 新增有界私有区域查询、一次 RPM 前后映射复核、累计 OS/query/read/byte 预算及固定进程/窗口/完整 Scope。审查发现最后窗口 guard 失败时 caller buffer 已修改，现已延后提交；预算 49 哨兵和其余新 tests 已通过上述受控回归。没有接生产 transport、认证器或键盘路径。Windows 调用内 deadline 目前仍是 fatal error，纯 scanner 的 NearbyBudget 回退证明不能套用于它。同步内核调用不具硬抢占承诺。

随后 typed Deadline 批次已实际通过 `.tmp/rust-runtime-windows-batch-02/report.json`，receipt `f31d6d02-c58a-4440-aa5d-fb9387fb8e72`，source `4ad21f4fe64a5eace56fc3aed81690ac56fa59a28189dbc3c4bfdb1e9dea42a4`：102 passed / 2 ignored（Live lib 40、scanner 33、runtime_windows 10、Windows lib 12 / 2 ignored、private_runtime 7）。Source 错误现明确区分 Fatal/Deadline；仅精确 Windows private_deadline 转换，Request 核对实际有效截止后才允许同请求局部回退，累计预算不重置，重验身份。提前假报 Deadline、全局到期、取消及到期伴随 fatal cause 都有 fixtures。上一段 fatal 分类是先前批次边界，已由此修复；生产认证/transport 未接入，同步系统调用仍不能硬抢占。

显式 Source entry / Known-Unknown load-plan 已完成代码批次，freeze48 `305fd668518dfd268bc1ffab8d54f68b919822ae256a587260b41ef73692180b` 包含 206 输入。原 12-case/144 断言保持，新增 11 execution scenarios / 57 断言，预计合计 201；17 个原始 Lua 输入。尚待实际编译、运行和原无 host 基线回归。unknown order 明确知识边界、零顶层执行，不是所有加载顺序的合并验收。

续验：entry build-01 成功（receipt `bb51ba46-3bff-4d56-a192-14ea4d7597e4`，source `4ad21f4f…`，EXE `7cdc7d4c0c02d38edfe4bd29790efc53ac42b1aceb0526222cae106ac62e94ab`、PDB `88a3f8f4b4574306e0f2c72841a493702dd12880e41cc5c81d40c2514d7e3939`）。fixed-01 的全部 201 断言通过，但独立检查器把已排序输出 JSON 的 Site 当作 typed Rust Site 的原始字段顺序，15 项 candidate ID 核验失败。核对 digest/Serialize 实现后重建 `{file,start,end}` typed Site；原失败保留。freeze49 `687cb0ed781f136d7340e27a893504287bb97623017cd27755f527d556ad3ea2` 仅两份外部 observer 文件变更，逐项核验原 Rust/patch/材料/Lua 编译输入不变，复用同一封存 EXE/PDB；fixed-02 通过 201 断言、417 外部检查、1,400 共用独立检查、214 entry 独立检查和 17 Lua stdin checks，进程/句柄回收通过。复用只适用于明确相同编译输入，未伪造新 build receipt。

独立 Source entry 审查发现预算边界差异：entry 内 65 层 do 或深调用的普通 budget 错误会直接丢整个 graph，而 SourceCall 保留 prefix。条件创建闭包的 entry 也缺少创建路径/存在假设。下一批正在统一边界恢复、保持最新捕获 cell、明确创建谓词和抽象假设，并新增真实静态反例；尚未验证，不因上述 201 通过关闭问题。

续验：上述边界修复已由同次封存 build-01 验证，receipt `7526a5c6-9e32-4f54-96d1-37cd3e1ba468`，source `7d158bb0e93708f61a34aeaacaa562a0f762f21ac10167989f91334f0eba5f43`，compile freeze50。`.tmp/rust-emmy-source-entry-boundary-fixed-02/report.json` 通过原 201 加新增 36 断言、495 外部检查、1,400 共用独立检查、214 entry 独立检查、52 边界独立检查及 22 Lua stdin checks，进程/句柄已回收。fixed-01 的业务断言已通过，但 observer 仍要求旧 ENTRY_RULES v1；保留首失败，freeze51 仅修正外部规则期望与一个空格，核编译输入不变后复用同一 EXE/PDB。普通预算错误保留已完成 prefix，取消/截止和非预算致命错误仍按原错误边界处理；条件 entry 显式记录闭包存在假设并继承创建谓词，不恢复旧 heap。当前无 host control/query/native、完整 flow 和严格 Clippy 仍待复验。

新 Mailbox 路线已冻结 RuntimeModule/RuntimeRootRecipe 第一批：OS 主模块能力与统一请求预算、运行时代码两模板唯一匹配、有符号 RIP 和操作数/映射校验、anchor/root 复核。独立只读审查与 rustfmt 已完成；计划 33 个新增 tests 尚待实际执行。`.tmp/rust-runtime-root-batch-01/failure.json` 是 Cargo 启动前缓存配额拒绝，不算测试失败或通过。生产 BoundMemory/native/channel/Mailbox 仍未接线，未读取游戏进程或发送输入。

续验：`.tmp/rust-runtime-root-batch-02/report.json` 已实际通过，receipt `eed3993b-bfed-469c-acb9-aa6dcaaa760e`，source `7d158bb0…`。Live lib 60、Windows lib 19 / 2 ignored、RuntimeModule 测试进程集成 6，合计 85 passed / 2 ignored；包含全部 33 新 tests（20 RIP recipe、7 budget/mapping、6 OS 集成）。两个 Go 模板的正负位移、ASLR、跨块、原始零/多命中、operand 宽度/权限、Forever 重复 call、保护 header、anchor/root/mapping 变化及取消/截止/累计预算均有执行证据。三套同次匹配 EXE/PDB、完整源码和原日志已封存，进程/句柄终态已回收。这只关闭原型受测范围，不代表 Lua 路径读取、生产认证/输入或任何客户端实机验收通过。

Data 新增 creature display/model 业务批次及真实合成 CASC/DBD/WDC/provider/capture tests，尚未构建。独立只读对照 Go 已发现并修复 Geoset 64-bit 算术回绕、model 缺参数与显式零的错误区分，以及共享 16 MiB capture 超限错误码；新增实际大结果反例测试等待运行。没有将审查或注册记为业务验收通过。

续验：Data 批次实际通过 `.tmp/rust-creature-navigation-batch-01/report.json`，receipt `35e0102d-1b8d-4977-b005-f90f2d9d85ab`，source `4ad21f4f…`。40 passed / 2 ignored：Data lib 27 / 1 ignored、原 item/decor catalog 7 / 1 ignored、新 creature 6。包含实际 65,536 显示的 16 MiB capture 超限返回 `query.budget_exceeded` 且不提交 evidence，以及三表身份、数组/重复纹理、排序/歧义、缺失关系、空模型、参数和取消。三套 matching test EXE/PDB、完整源码及日志已封存。跨 Go/Rust 公开 CLI 对比、实际客户端/CDN、完整资源核算和其余导航业务仍未验收；不把手工 Go 源码对照当执行差分。

## 缓存与后续

配额及两通道不变。所有回收先核终态、精确路径、输入/日志与匹配符号，保留不可重建证据。最近 `.tmp/rust-beta-exact-session-reclaim-01/result.json` 回收 9 个 exact session / 236,645,304 bytes；Root 核对 1,593 个 COFF object 的编译单元到实际 fresh compiler outputs，保留旧 Core sibling，不凭时间或大小认领。历史未确认 CLI/PDB 与其他任务缓存保持保护。

本轮精确回收：8 个有终态归属的旧 sessions / 319,279,872 B；随后按 actual fresh compiler JSON、823 个 COFF object 和 metadata 认领 build-02 的四个 sessions 及冗余 probe EXE/PDB，共 248,635,887 B。build-04 验证后又回收其单个 probe session/冗余 pair，共 57,282,430 B。全部匹配封存 pair、全输入、首失败和旧 sibling 保持。第一次回收生成器因 JavaScript replacement-string 的 `$'` 展开产生 PowerShell parse error，解析前停止；失败 runner/日志保留，改用 replacement callback 后通过，没有执行该失败脚本的删除。

Windows batch-01 精确回收另 432,772,807 B；先核实际 fresh compiler JSON、1,382 COFF objects、metadata、全 source snapshot 与五套 matching test pair；Windows 0.62.2 原 crate hash 对 Cargo.lock，701 个实际 dependency source files 全匹配并保全。PowerShell retirement exit0/handlesReaped，随后 Node 因 result.json 的 UTF-8 BOM 解析失败；已只读核原终态、全部已回收路径不存在并保存 `root-terminal-verification.json`，没有重放删除。失败日志及旧脚本保持原 bytes。

下一步：对当前源码补跑原 control/query/native 基线及严格检查。随后继续显式 entry、未知加载顺序和完整 flow 36-case，运行时指令/RIP 根定位、Lua 路径读取/生产 Mailbox 认证/协议审计与接入，以及 Data/Assets 剩余业务。完整 Retail、跨 CLI/缓存/故障、性能 20 samples、候选包装/升级/Skill 与最终真实客户端矩阵仍未完成。

typed 批次测试完成后，按同样终态/完整源码/匹配符号/归属守卫精确回收 374,656,599 B（9 sessions、5 test aliases、5 library pairs），记录 `.tmp/rust-runtime-windows-batch-reclaim-02/result.json`；BOM 解析已修，Node/PowerShell 终态均完成。该回收不删除历史测试证据，也不改变最新路线选择。

Data batch 精确回收 450,019,277 B（6 sessions、3 matching test aliases、4 library pairs），记录 `.tmp/rust-creature-batch-reclaim-01/result.json`。发现 lib-test 的 `.d` 另引用 53,018 B Unicode JSON，未包含在旧 sourceDigest 范围；已从同次封存 EXE 的唯一嵌入区间 offset 7,361,859 恢复，核 SHA `a25210a164d69810e5d3c6657b5bfb0867168f155c2d8e9130cfc16a54ba8eac` 与封存 include_bytes! 源码位置，保存 supplemental input/proof，不以当前文件默认回填历史。后续 test runner 必须在编译前保全外部 include 输入。

Source boundary build-01 精确回收 258,872,794 B（4 个已知 sessions 与冗余 probe EXE/PDB），记录 `.tmp/rust-source-entry-boundary-build-01-reclaim/result.json`。三套 fresh 库保留，完整原输入、freeze51 observer 输入、首失败及匹配产物稳定。五个可重建 cache 路径依实际 fresh producer 更新保护身份；旧 Source rlib 已被 Cargo 覆盖且无副本，明确记录此事实，未声称保全旧 blob。不可变历史产物及旧 request 未改。

## 运行时 Lua 路径与 Data 联合续验

`.tmp/rust-runtime-data-batch-01/report.json` 实际通过 **258 passed / 13 ignored**，receipt `baa2b166-bd97-4f53-9872-2bfdd0e99aca`，source `b69e510c53b99b88fa0ea07625bd448cf28e44a3f5e2c81b03eb4f5767f79ad3`。26 个受测目标的同次 EXE/PDB、完整源码、原日志和 81 份外部 fixture 输入均已封存，进程/句柄回收通过。ignored 的真实环境及准备测试不算通过。

运行时部分包括 Core lib 16、Live lib 80、Windows lib 22 / 2 ignored、RuntimeModule OS 集成 11、Mailbox lookup 11。新增 33 项覆盖有界私有对象点读、受控 OS image directory、计量客户端 metadata、绑定模块实例的 ABI admission/reader；另外四项 reader 反例验证共享累计预算、guard 分配失败、无效 selector 零路径读取和错误 wire kind。沿同一请求账本读取并复核 root/metadata/命名对象路径，错误保留原因与使用量、使绑定失效；没有堆枚举或自动重试/扫描回退。metadata 读取只涉及 flavor/version/build，未打开或哈希 EXE；每次 std 文件操作保守预留 16 OS units，这是成本预留，不是实测 syscall 计数。生产 ABI catalog 仍空，未知布局明确 unsupported；默认 native/channel、最终协议认证/输入及实机均未接通或验收。

Data 部分包括 lib 31 / 1 ignored 及全部 20 个 integration 目标，含新增 encounter 8 项。动态表需求在同一固定 pin/Encoding/Root/budget 会话完成：只有可达且深度内的 spell references 才需要 SpellName/SpellMisc；树结构、稳定顺序、循环/深度/行/关系限制、orphan、相关图标与两份 capture 有实际 fixture 验证。共用 bounded JSON writer 在复制/增容前检查 16 MiB 与预算，包含转义膨胀反例；集合去重保留发现顺序，结构链/排序计量，避免递归复制整个子树。原 catalog/creature/SQL/CASC/provider/cache/assets/stream/WDC 自动回归同步通过。公开 Go/Rust CLI 差分、CDN/完整资源核算及实机仍未验收。

为该批构建准入，`.tmp/rust-runtime-root-batch-03-reclaim/result.json` 精确回收此前 eed3993b 的 302,102,377 B（6 sessions、14 emitted aliases），保留全部封存输入/产物及三套 Source fresh 库。02 原退休稿未执行且原字节保留；03 依旧实际编译快照及历史输入 proof 核归属，当前源码路径仅是逻辑位置，不要求后续合法修改等于旧字节。

当前 Source 原 control/query/native 三基线运行器已准备，同次编译三个例子；freeze52 `58a5314bc9823ebaa37df4b0d418ff83004726a89f5d0d6486662e1995ef7966` 封存 214 项输入，完整私有 patch 保持 E747。尚未运行该批或当前严格 Clippy。联合测试后共享 cache 为 20,629,793,617 B，下一次固定 2 GiB reserve 准入缺 1,302,440,785 B；只回收归属闭环的可重建输出，未改变配额。

Data 联合批次回收首尝试在删除前失败：`.tmp/rust-runtime-data-batch-01-reclaim/failure-4c31f4ca64554d35859213039231b7dc.json` 明确 `removedBytes=0/removedFiles=0`，retirement exit1/handlesReaped。Windows PowerShell 的默认 Get-Content 解码损坏 UTF-8 请求内的中文 supplemental 文件路径；原请求、脚本、失败和日志全部保留。重试须使用新输出目录并显式 UTF-8 读取 JSON，不重放或改写首失败。

UTF-8 重试已实际完成：`.tmp/rust-runtime-data-batch-02-reclaim/result.json` 精确回收 1,309,300,299 B（31 sessions、26 test alias pairs、6 library pairs），全部封存产物及首失败目录稳定，进程/句柄已回收。构建下一批前另保存六个已知 Source mutable library 原字节，共 84,270,760 B，见 `.tmp/rust-source-baseline-prior-cache-01/proof.json`，不把后续 Cargo 覆盖误写为旧字节仍在。

随后只读审查确认三项运行时接线缺口：父取消惰性传播、metadata pathname 检查与打开之间的目录替换窗口、Unicode 路径额外分配少计。已改为完整 OperationControl checker、计量的 held Directory 链和相对打开、单次预收费 UTF-16 目录转换，新增 11 项反例。公开 Data 审查还确认 parser/错误 stage/项目 context、HTML 转义上限和追加表错误优先级的差异；实现及真实公开差分矩阵已补，未以旧 fixture 通过掩盖这些差异。

修复批次 `.tmp/rust-runtime-data-batch-02/report.json` 实际通过 **293 passed / 14 ignored**，receipt `f05bbd1f-7425-4c6c-84ba-f82e3d16d01e`，source `6d9f0f820753263d27387c16f83c7e90c337eeae5daffe27f128ad0c0544394e`。28 个同次匹配 test EXE/PDB、完整源码与外部输入已封存，进程/句柄已回收。包括 CLI unit 17、Core lib 17、Live lib 83、Windows lib 29 / 2 ignored、destination 5、RuntimeModule 11、Mailbox 11，以及全部 Data 自动回归（lib 32 / 1 ignored、encounter 9 / 1 ignored）。该结果证明修复反例及自动范围，生产 ABI catalog/默认协议通道和实机仍未完成。

已用上述封存 encounter test EXE 实际生成八组固定 CASC/DBD/WDC 公开差分输入，`.tmp/rust-encounter-public-preparation-01/report.json` passed/handlesReaped，suite 为 `.tmp/rust-encounter-public-fixture-01/suite.json`。37 cases × JSON/JSONL（74 域查询）及双 capture/evidence 校验已准备，尚缺当前公开 CLI 的同次构建与实际差分。当前共享 cache 20,863,217,168 B，下一次固定 reserve 准入缺 1,535,864,336 B；已知 fresh scope 正在核验，未扩大配额。

f05 批次 cache 退休已完成，`.tmp/rust-runtime-data-batch-03-reclaim/result.json` 回收 1,542,619,677 B（34 sessions、28 test alias pairs、7 library pairs），全部保护证据稳定，终态进程/句柄回收。

当前 CLI 与三项 Source 例子已在同次 Cargo 构建封存：`.tmp/rust-source-baseline-build-01/artifact.json`，receipt `06624619-be05-4c37-96d7-90af0413d481`，source `6d9f0f82…`，freeze54 `ed329c910d38e036c015fe2d1155df795c31143d4b9975054a543368ee12fef7` / 214 inputs / E747 patch。CLI EXE SHA `655e8ada666d73efec4fdb4a1eeea897e87928a192d09b99e43cd8bc69e0b260`、PDB `cb55ec84580857bf4fe99f734d6bacd9b0c664ea2e7e17f4fda07dbbbe51186f`；其 scope 明确为 enabled emmy-prototype feature，默认 Source 尚未迁移。

`.tmp/rust-source-baseline-run-01/report.json` 实际通过原 control/query/native 三基线：control 4,280 checks、query 178 checks、native 193 assertions，以及 query 496 / native 1,365 独立 checks；三个受控程序均 handlesReaped。完整 flow/secret/实际 API/Retail/性能和当前严格 Clippy 仍未完成。

公开差分首运行 `.tmp/rust-encounter-public-01/report.json` failed，完成 case 数 0：冻结 Go 的第一条 encounter 及 capture 已成功，随后 Go `evidence verify` 返回 capture 本身，而旧 runner 要求 `.result.verified=true`；Rust 公开 CLI 当时也额外包装为 `{capture,verified}`。原失败、18 次执行及输入保持。CLI presentation 已改为先完成内部 blob 校验、再返回原 capture；新 JSON/JSONL 与同长度 blob 篡改反例、完整 verify envelope 检查器已写，尚未构建/执行。必须新 CLI artifact 后重新差分，不能复用上述旧 CLI 或将失败写为通过。

续验：`.tmp/rust-cli-evidence-build-01/report.json` 已通过 CLI 18 项单元测试，receipt `c84e48ca-2ebd-475d-a49c-2605fd296074`。随后公开差分 `.tmp/rust-encounter-public-02/report.json` 实际完成 74 cases，72 通过、两项 `maximum-content-budget` 失败：Rust 将公开的 512 MiB decoded-content 上限错误用作 CASC encoded/metadata 预算配置，小文件也被 transport limit 拒绝。264 次受控运行均回收，首失败保留。

已分离 decoded-content 与 CASC transport/metadata 预算，不增加全局资源配额。`.tmp/rust-cli-content-build-01/report.json` 在同次构建中封存 CLI 和九个 Data 测试目标，实际 **42 passed / 9 ignored**；receipt `fd615dba-aa0b-4c96-994b-173a1443abb6`，source `ec92517c096ee10f279134868fe82b5ec64eebd690e9afe1dc0202eeea4caa9e`。CLI EXE SHA `d2c3f3865b8097c3011f9663ae288a6debdddf5fbe8447348b29a01d71030d58`，matching PDB SHA `ab17cda875d528c37b561075e36184c83ba4d629575086e0b10dcb12e85d61ea`。新增四项测试覆盖冷/热缓存、decoded-size 边界与 BLTE framing、取消/编码源损坏及非法参数。实际 512 MiB 内容的完整解码/物化仍有 256 MiB 内部上限，单列 ignored 的未完成测试，不能由小文件接受 512 MiB 参数推导为通过。

使用该封存 CLI 的 `.tmp/rust-encounter-public-03/report.json` 已实际通过 **74 / 74** JSON/JSONL cases，包括完整 envelope、双 capture 和公开 evidence verify；`.tmp/rust-encounter-public-03/process-proof.json` 的 **268 次**运行均 root/tree/handles/output/identity locks 回收，Root 已独立核对。fixture 差分只证明该矩阵，不关闭其余 Data/Assets、性能或实机门槛。当前源码的严格 Clippy 仍待执行。

Source 066 缓存精确退休 `.tmp/rust-source-baseline-build-02-reclaim/result.json` 已完成 782,393,606 B；首尝试遇见工作树外 wowdump 进程而拒绝，删除量为零，后续守卫仅限定本仓库缓存，未操作外部进程。CLI c84 缓存 `.tmp/rust-cli-evidence-build-01-reclaim/result.json` 已完成 652,452,476 B，完整对应输入和 matching artifacts 保全。fd615 的下一份退休草案 `.tmp/rust-supervision/reclaim-cli-content-build-01.mjs` 尚未执行，须 Root 独立审查；不能重放已完成退休脚本。

后续只读对照修正边界解释：冻结 Go `v3.1.2` 的 `internal/records/reader.go::extractContentWithCoverage` 将所选 encoded/decoded 长度都与 `q.ContentBytes` 比较。因此新增“decoded 恰好等于 max、BLTE encoded 更大仍成功”的 Rust 测试不能作为 Go parity；当前 74-case 公开结果只覆盖其中实际输入。下一批须恢复所选 encoded 长度限制并新增公开冷/热差分。需求并未要求任意 exact 512 MiB 查询无条件成功；512 MiB 参数上限、低层流式解码能力和包含元数据/结果持有成本的全请求资源预算分别验证，原 ignored 硬成功期望是未经证明的推导。保留该批原证据，不通过提高资源预算或改变 oracle 来消除差异。

fd615 精确回收已实际完成：Root 独立审查完整 JS/PowerShell 守卫后执行 `.tmp/rust-supervision/reclaim-cli-content-build-01.mjs --execute`，`.tmp/rust-cli-content-build-01-reclaim/result.json` 的 SHA `7e7bbf97d471c6f1f503bd4079ac78d0c7895db337d15919822b07ee38544308`，回收 **929,678,936 B**。终态 exit0、root/tree/handles 回收、全部保护稳定，50 个指定目标路径均未重现；不扩大配额、不删除冻结 CLI/test EXE/PDB、源码、首失败或公开差分证据。

当前六包 Source/emmy-prototype/all-targets 严格 Clippy 已实际运行，`.tmp/rust-strict-clippy-current-01/report.json` 保留 red，receipt `10732e83-a64f-4122-a5c5-773ca351ff6a`、源 `ec92517c…`；exit101、owned handles 回收、输入保持。首次诊断为 Windows runtime_module 的 deprecated `AtomicU64::fetch_update`，另五项是手写 div_ceil/is_multiple_of。Root 已按固定 Rust 1.99.0 的实际诊断修改该文件并通过 rustfmt --check，新源 `e02f3d2758d261049bf26a811241702c06916d6168aa6a021ad8019b07f4a116`；尚待新的封存严格检查及受影响运行回归，不将 lint 修复写为已通过。

Source flow 修订方案与独立反例分别保存在 `.tmp/source-flow-batch-a-revision-20261012.md`、`.tmp/source-flow-minimal-plan-review-20261012.md`。确认需独立执行表达式/未知调用根，不能仅遍历 sink；条件返回 guard 必须绑定候选 observation，不能比较 unavailable 文本；条件写入须 typed join/checkpoint/path/可缺写入及 field 身份；原 36 例每例 2s 的完整生命周期要独立计时/回收证据。当前仍为方案，未改 Source，实现与完整业务验证未开始。

## Source flow 与资产流式读取实际接通

2026-10-12 新工作规则要求实现优先，后续迭代使用已有受控 Cargo、必要日志及具体失败复现材料，不再逐轮复制全源码或建立额外封存链。保留原证据与既定缓存配额。`b5bfdf53-de77-4df8-b685-e3d46dc81863` 首次实际编译新增 Source flow 和 Data/Core streaming；受影响回归为 164 passed / 2 failed / 8 ignored。两项失败均是已记录的旧 LuaLS broker deadline/observer-cancel 边界，相关 session/control/pipe 本批未修改；失败日志及 matching EXE/PDB 保留，不能据此宣布全 Source 通过。

Source 新 `SourceQueries::flow` 由同一事实图计算来源、槽位、写入/读取、条件候选与 guard 极性。真实运行器和独立 verifier 为 `tests/rust-source-engine/flow-owned.mjs`、`flow-independent.mjs`；原 36 输入、规则及预算保持，增加确切选择及 join/guard 反例。真实入口编译 `42cf73ad-59fe-4f63-bb47-00d0f9dd30bc` 后 `.tmp/rust-source-flow-run-03/report.json` 的 46 requests 实际通过；追加未知 entry 参数 C09 先得到真实 RED（错误的 complete=true），后在原 sink 依赖 trace 修正为有来源的未知 gap。

修正编译 receipt `4925a026-9600-41c5-b623-4b95152aa76b`，source `185c217e97ab0dc7cb7700d3aabfe688984504ab7c1f3fe9b117c3b2c57a85ab`。`.tmp/rust-source-flow-run-04/report.json` 已实际通过 **原 36 + 11 附加 requests** 的业务及原 2s 全生命周期计时，全部 owned processes 回收；最长 owner 1,185.42 ms、worker/drop 31 ms。独立检查覆盖每条真实 proof edge、认证 return/sink 槽位、原文范围、真实 not/paren false-predicate 链、重复发生与预算停止。这里只证明 common-facts flow 请求切片，生产 WoW API、默认 Source 查询/session/validate、完整 Retail 与性能仍未完成。公开 `source context --flow` adapter、真实 pin/TOC/API/luac 与 capture 接线已写，尚待新构建和实际 CLI 集成测试。

Data 的生产 `asset inspect` / raw `asset export` 已切到 CASC/BLTE 有界流式读取、完整 CKey/SHA 核验、持有 verified blob 与既有原子 Destination 写入；表格/SQL 等仍保留明确的 materialized adapter。本批不提高资源上限。实际 17 MiB 多 chunk 冷暖 inspect、完整 blob/capture 和 raw bytes 测试通过，`.tmp/rust-content-streaming-tests-02/report.json` 为 5 passed / 3 ignored。`.tmp/rust-content-boundary-public-01/report.json` 的 **20 个** JSON/JSONL × cold/warm 公开请求全部与冻结 Go 相符，包含 selected encoded/decoded 上限及完整错误 envelope；原 message/stage 差异由实际 CLI 观察发现并修正。

真实 320 MiB / 40 chunks 公开读取见 `.tmp/rust-large-content-public-01/results.json`、`verification.json`。Go 冷 inspect 与 Rust 冷 inspect 的稳定完整 envelope、335,544,320 B 正文及 SHA 相同；Rust 暖 inspect 与 raw export 同样成功，导出全部字节的 SHA 已验证。Go 暖 inspect/raw 则实际返回 `records.query_resource_budget: retained_bytes`。后两项是 bounded blob ownership 避免整块重复物化后的资源行为改进，**不是 Go parity**；默认预算未提高。当前还需新 stream 路径的损坏/取消/失败原子性、其余 Data/Assets、CDN、性能及最终客户端验收。

续验：receipt `b832b2fd-3ef9-4be1-9681-927a048f968d`、source `a9ff973ee779a3210902a9eb871247aae89fc0a7315a8402aeb858e86bf4034a` 实际构建了公开 CLI、flow probe 和受影响集成目标。`.tmp/rust-source-flow-public-01/report.json` exit0 且整树/句柄回收：真实 pinned `source context --flow` 经 TOC/API/luac、worker 和 capture 接通，覆盖嵌套调用、namespace/local alias、未知 entry；未知 API effect 保留明确 gap。`.tmp/rust-source-flow-run-05/report.json` 原 36 + 11 requests 的业务与 2s 生命周期均通过；运行句柄 44704 已观察终态 exit0。

`.tmp/rust-stream-fault-tests-01/report.json` 的 content/evidence 目标实际通过并回收，覆盖流式源损坏、完整 CKey/SHA、终端读错、取消及导出冲突/失败原子性；不代表 CDN 或性能通过。BLTE 第一块后取消的新 lib test 尚未构建。b5、42cf、492 的精确缓存退休均已完成，分别见 `.tmp/rust-source-flow-streaming-tests-01/cache-retired.json`、`.tmp/rust-source-flow-unknown-entry-red-01/cache-retired.json`、`.tmp/rust-disk/evidence/4925a026-9600-41c5-b623-4b95152aa76b/cache-retired.json`，不得重跑。

当前源码已在 b832 后继续修改，不能复用其产物宣称新实现通过：默认 refs/context 的 `native_research` 接线、保留单快照的 `native_session`、独立 native broker scope/精确 owner retirement 尚待新构建及公开验证。Source worker 已修正 BTreeSet 导入和每文件 capture 索引查找并冻结；Root `native_session.rs` 仍是未完成草稿，需先消除 WorkerRequest clone、补齐一次性启动意图与私有 CLI 路由，再证明多 CLI cache hit、显式 close、deadline/cancel 与恢复。旧 LuaLS broker 两项真实失败保留；当前严格 Clippy、完整 Retail/validate、全部业务性能/包装和最终客户端验收仍未完成。

## 原生会话接通与完整 Retail 的实际失败

后续 `native_session` 已接到公开 refs/context 的 `--session-reuse` 及 status/close；一次性启动意图绑定真实父进程、创建身份、当前 CLI 和 same-logon home。首次 0f8b 编译漏传私有 Emmy patch 标识，公开 session/flow 因 `compiled engine patch identity missing` 失败；`runCargo` 现从实际 fork 计算并传入标识，保持原失败日志。7bb 更正后 flow 通过，session 暴露仅 load graph 不含所选函数入口；真实 unique entry/unspecified 参数接线后 ac11 运行到 fault，批量同步准备造成超时。checked batch 现逐文件/阶段检查实际 control，保留一次全库索引并丢弃中断 analysis。

`64c290d4-ec1f-4314-b0b1-9dda91e27b42`，source `af1b4f122f6987f012a0cedb557ef1b05e116e311ad981d261233ea6f96ddcf5`、patch `857c47d66c145a270b8fbc52218d922a466effc66b373f1d7b933650a02529b0`：`.tmp/rust-native-source-session-run-04/report.json` 实际通过公开独立 CLI refs/context 复用、换 pin 后 cache miss、假设 entry 的已知/未知绑定、原 capture、取消、deadline、故障后同 owner 重准备及 close/原进程退出。取消在 68 ms 内返回 7，1 秒 deadline 在 813 ms 内返回 5；整树/句柄回收。`.tmp/rust-source-flow-run-06/report.json` 原 36 + 11 requests 的业务及 2s 生命周期全部通过。这是受测范围，未关闭完整 Source 验收。

固定 Retail `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b` 实际有 2,551 Lua 文件、23,354,176 B；使用已有固定 mirror/home，无游戏输入。`.tmp/rust-native-retail-controller-01/report.json` 与 run-01 原始结果记录冷 refs 约 81.0s 后 EOF，warm 未到，controller/tree 已回收。ff2 新诊断下 controller-02 同样实际失败，冷 refs 约 73.1s；末次阶段为 `emmy_indexed` 25.5s，private commit 1,735,278,592 B、peak 1,759,461,376 B、job limit 2,147,483,648 B，后续 identity 阶段未完成。原始诊断保存 `.tmp/rust-native-retail-controller-02/last-request.json`，SHA `005b2cb6a36281f4cf5ad25ebaaee45f9fef8dae116b9cab1148845c7231ecee`。退出码尚未取得，不能只凭 EOF 声称 OOM。紧凑 `(FileId,start,end)` expression 索引已写，保留 public Site/proof/BOM 语义，尚待新编译和完整 Retail 复验；没有提高预算。

## 原生语义与图片导出新批次

ff2 receipt `ff2c907f-fe06-4d9b-9111-e307c3404dc1`、source `f53620c11a31a8dbd5ef654b81dd2062593db6a11299cb81f4cb702bc985c16c`、patch `3363d345078b6a981c14c21025f8b04fc3a11a8e087738b85bb793208009a88d` 实际构建了 typed environment callable/return slot、库内参数/返回类型投影、native semantic 单 TOC/matrix，以及 PNG/WebP 导出。`.tmp/rust-native-semantic-image-run-01/report.json`：session 与原 raw-assets 通过；image 一个通过一个失败，一个准备 ignored；semantic 实际失败。所有目标整树/句柄回收，不记整批通过。

图片失败为实际字段名 `pixelsSha256` 与 Go `pixelsSHA256` 不同；显式 serde 名已修，像素及三份 capture 断言未删。语义 rawReport 的 `coverage=[]` 与 `native vararg formal shape unavailable` 证明 XML wrapper 未使用的 `...` formal 提前阻断 checker；现允许未使用的变参声明，实际读/展开仍为未知边界，UnitHealth true/{}、错误返回类型、matrix 输入保持。两项修复尚待实际重跑。XML 仍是原行映射，character=null、保留 generated range；仅参数/返回两个诊断族接通，其余族、名义 script-object/继承和精确 XML 字节映射未完成。

Codec 为纯 Rust `png=0.18.1`、`image-webp=0.2.4`，受控 fetch receipt `130ead32-f46c-4b7d-bb94-3cf164733073` 实际加入六个锁定包，未编译业务；后续 build 使用 locked/offline。完整许可证库存、公开 Go/Rust 图片差分、性能及包装尚待验证。encoded bitstream 可不同，像素/source/image/capture 合同仍须比较。Root 增补 asset export 的 Go export-stage 错误映射，未据源码对照宣布公开差分通过。

当前取消中途 BLTE lib test 已实际通过，见 `.tmp/rust-native-source-session-run-01/blte-cancel/stdout.bin`；atomic startup intent 的拒绝保留/并发单消费及相关 durability 为 28 passed、broker 4 passed。b832、0f8b、7bb、ac11、64c、ff2 精确退休均已完成，各实际失败 EXE/PDB 和既有四组测试文件保持；勿重跑已完成退休。Node controlled build runner 六项测试通过；当前 Rust 严格 Clippy、完整业务/性能/候选交付及实机仍未完成。下一编译包含 compact/unused-vararg/pixelsSHA 修复，不将草稿记为通过。

续验 bcd receipt `bcd94186-0562-4c63-badf-c694d88364f0`、source `1a317f1b17ea44e014d63162a553fdc22c43152cec1f7ed94a0a53973ce80a32`、patch `3363d345…` 实际构建成功。`.tmp/rust-native-semantic-image-run-02` 的 image 2 passed / 1 ignored、session passed 且进程回收，`.tmp/rust-source-flow-run-07/report.json` 原 47 requests 业务及生命周期全通过。semantic 仍真实 RED：新 1,648 B rawReport `aaf87c36adf7b0a19dd98d274e087d49ab3479e7a8bfe5ada20eed4b04d65016` 已有调用 permit，但 UnitHealth 第一个参数 `UnitTokenPvPRestrictedForAddOns` 无可投影声明，整调用 unavailable；第二个 bool 参数 `usePredicted` 应独立检查。库内逐参数实际检查及 coverage 已写，未知第一槽/整体 partial 保留，尚待新构建验证。

Retail03 `.tmp/rust-native-retail-controller-03/report.json` 仍失败并回收：冷 refs 约 79.0s 后 EOF，末次诊断 `source_identity_indexed` 32.13s、private/peak 1,881,665,536 B；原始 JSON 存 `controller-03/last-request.json`。紧凑表达式索引让受测查询越过原失败阶段，但后续语法索引未完成，不能宣称完整 Retail 可用。下一修改将内部 syntax 位置、by_site 与引用声明关联改为紧凑文件序号/原字节范围/声明行号，在输出边界恢复原 public Site、声明 ID 与 proof，保持 caps 和功能；尚待编译/47flow/session/Retail。真实 EOF 后只退役已认证 exact CLI owner 的接线与实际 OS 身份/回收反例也待运行。

图片公开业务实际通过 `.tmp/rust-image-export-public-01/path-normalized-report.json`：**86 cases / 172 owned processes**，Root 独立核对数量、每项通过和整树回收，Rust CLI SHA `30a6f56ac5c720149ee9b4e857adc1e5958fc75b5905cf92b65e0cfdaeb1c48d`。六种 BLP2 输入、PNG/WebP、rgba/a/abgr、JSON/JSONL、限额/缺 mip/非法 channel/损坏/不支持格式/冲突/overwrite，完整 RGBA 由独立 Sharp/libvips 解码，source/image 语义及三份 capture/blob/SQLite/manifest 核验。原 report 两项冲突消息仅含各自测试输出路径，保留原 RED并逐项核真实路径/旧文件后重判，未重复已通过 CLI。压缩产物和 manifest 字节各自认证，不声称编码器 bitstream 相同；Sharp 仅验收依赖，产品仍纯 Rust codec。性能、CDN/provider 及最终客户端仍未完成。

bcd 精确退休已完成 `.tmp/rust-native-semantic-image-run-02/cache-retired.json`，保存 Retail03 CLI/PDB及 semantic test/PDB，旧失败、诊断与指定四组测试产物保留；勿重跑。当前新构建包含 syntax compact、逐参数检查、exact owner EOF 退役及 Windows 真实进程反例。保持 full goal active，分项通过不结束任务。

续验 `2060da1a-b343-4603-ba5a-04f58f3e32a8`、source `3ded943e3df2b0873fec1b1583333ecfb17ded1201527902b7c09fad714a8329`、patch `08a8200dbaa64acb7cecccc86385ffcedc7ec2e29c4295265bf6b088f0cc3e97` 实际构建通过。`.tmp/rust-native-semantic-image-run-03/report.json` 的 SDK semantic 和 session 均通过并回收；SDK 实际覆盖 UnitHealth 已知第二槽的 true/{}、未知第一槽的 unavailable、API number vs 声明 string、两 TOC matrix。`.tmp/rust-native-semantic-cli-controller-01/report.json` 的真实 CLI good/bad JSON/JSONL 四例通过，exit0/4、原生诊断及合法帧序列验证；complete=false 未提升。`.tmp/rust-native-owner-retirement-run-01/report.json` 的实际子进程创建身份拒绝与 exact 退役通过；`.tmp/rust-source-flow-run-08/report.json` 47 requests 业务与 2s 生命周期通过。受测进程均已回收。

Retail04 `.tmp/rust-native-retail-controller-04/report.json` 仍失败并回收：cold refs 约 97.1s，末次 `source_identity_indexed` 39.756s、private/peak 1,881,124,864 B，原 JSON 保存 `controller-04/last-request.json`；syntax 阶段未完成。新增 EOF 退役遇见已退出进程的 TerminateProcess access-denied 竞态，致原 IO 错误被 capability3 遮盖；same-held-process signal-before/after-terminate 的修正已写，尚未编译，不声称已修好。退出码仍未取得，未将 EOF 等同 OOM。下一步按完整 Corpus 的实际需要调整语法索引持有/查询投影，继续保留全部输入和公开能力，不涨预算。

后续代码批次：Source worker 提取原型的精确 XML 分段/BOM/CRLF/UTF16 映射；Data worker 实现仍缺的七表 spell info；cache agent 解决 full Retail wrapper 索引资源行为。当前严格 Clippy、完整诊断族/名义继承、89 命令全部等价、性能/包装及最后客户端验收仍未完成；checkpoint latestIteration 对应上述实际产物，当前源码后续编辑不能借旧 binary 声称通过。

## 完整输入逐文件投影、精确 XML 与 spell info 续验

构建 `1f12111a-3ee0-40ed-9a97-3fcb8ab6aced` 实际成功，源码 `eb6fe7df6c9daa8b673aca803b68f1681374757c25b05a19703a3e501dca196a`，Emmy patch `08a8200dbaa64acb7cecccc86385ffcedc7ec2e29c4295265bf6b088f0cc3e97`，普通 CLI SHA `8e66877adbcd892bf6914fbd78cfe19dc8bad65209f94a3461c9d143677b9b75`。新 Source 保留完整输入和声明目录，按文件投影 refs/calls；入向查询仍遍历全输入，不减少覆盖。

`.tmp/rust-file-xml-spell-tests-01/report.json`：spell info 6、encounter 9、catalog 7、creature 6、精确 OS 退役 1、真实 luac inline 1 个实际测试通过；准备夹具的 ignored 单独保留。`.tmp/rust-native-semantic-image-run-04/report.json` 的 SDK semantic 与跨 CLI session 实际通过；已知参数/返回类型、未知类型的诚实 coverage、精确 XML 原字节/UTF16 来源、capture、取消/截止/失败后恢复与退出均覆盖。`.tmp/rust-native-semantic-cli-controller-02/report.json` 的新夹具普通 CLI good/bad JSON/JSONL 四例通过；语义完整性仍为 false，没有提升未知类型或诊断族范围。`.tmp/rust-xml-exact-run-01/report.json` 的 104 项 XML 检查和 30 次真实 luac 编译通过。原 47 个 flow 场景在 `.tmp/rust-source-flow-run-09/report.json` 通过。上述 owned 进程均已回收。

完整 Retail05 仍失败，证据 `.tmp/rust-native-retail-controller-05/report.json`：2,551 Lua 文件冷 outgoing refs 约 110.8s 返回成功，warm context 约 108.1s 成功、同 worker/inputIdentity 且 cacheHit=true；随后 warm incoming refs 返回拒绝访问、exit3。末次 `processing` 诊断 private commit 2,109,108,224 B、峰值 2,144,780,288 B，固定 Job 上限 2,147,483,648 B；保存 `last-request.json`。未取得原始 worker 退出原因，不仅凭接近上限断言 OOM。controller/tree 全部回收，不记完整 Retail 或性能通过。下一批消除 warm 请求的重复全文解码/序列化与保留，并在请求前持有精确退役句柄，避免 EOF 时重新打开已退出进程遮盖原错。

`.tmp/rust-native-query-current-01/report.json` 为保留的实际 RED：原 lexical/loop 35 条断言后，roles 在固定赋值读取 `...` 时被新增开放参数序列边界错误阻断；完整原合同与新增分页未跑完。固定赋值未知值投影、开放 call/return 序列边界保持的修复及独立多槽案例已写，尚待新编译。未改原八个 Lua 夹具。

spell info 普通 CLI Go/Rust 差分 `.tmp/rust-spell-info-public-01/report.json` 共 50 profiles / 156 owned 进程，40 通过、10 失败，均回收。28 个成功查询 profile 的完整七表资料、闭包、关系、capture 和真实 65,537 项边界一致。失败为 CLI 漏纳入既有 catalog 错误映射：table 错误的 exit/code/message 与 schema/depth 的 stage 不同；Root 已写同一映射入口修复，尚待新产物针对原失败及普通成功复验。

对应源码 runner 原先未携带私有 Emmy 依赖且 identity 绑定旧绝对目录。已补完整 upstream/patch/原始 identity manifest 的打包，并允许原 manifest 在新目录按完整原始库存验证。`.tmp/rust-emmy-relocation-01/report.json` 实际验证 848 个原始文件与 850 个 fork 文件迁移后的相同 base/patch 身份；仅证明依赖迁移，不表示两次离线产品重建完成。六项受控 build runner Node 回归通过。严格 Clippy、完整 Source/Live/Data、性能和最终候选/实机仍未完成。

续验 `e6067d69-8fd9-4273-ad13-18e908325693` 实际构建成功，source `36c8d7db35b8a9aab11ed361683651e437bcac453f8eb427021b74cfe5a336aa`、patch `d9095362b790c050bf34aa5ed7f1db807c78460c7524b95dac179562365189de`、CLI SHA `87c2365820dfb9cbefc47c656c16dd0020274c8e0700e848d7abc2f8fbfb2e0d`。warm 请求以当前完整快照流式哈希发送 identity-only 查询；miss 先返回只读缺失，再接完整快照由 worker 自算身份。原 full 格式保留，原取消/截止/换 pin/失败后恢复/close 及 snapshotTransfer 新断言在 `.tmp/rust-native-semantic-image-run-05/report.json` 实际通过，SDK semantic 同时通过。`.tmp/rust-source-flow-run-10/report.json` 原 47 场景通过。

`.tmp/rust-native-query-current-02/report.json` 新正常 probe 实际通过，原 84 断言/178 项外部检查全部保留，新增 17 断言/20 检查覆盖跨文件分页及固定 vararg 赋值，独立 verifier 496 检查和九份真实 luac 语法验证通过；end checked=101、failed=0、complete=true，但 fullSourceAcceptance=false。所有 owner 回收。原 current01 RED 保留。`.tmp/rust-spell-info-public-02/report.json` 原失败十项及普通成功两项合计 12 profiles/28 owned 进程全部通过，完整公开错误 envelope/capture 一致；未重复已通过的大边界，原 public01 的 40/50 记录保留，wholeQueryAccountingComplete=false 等真实资源缺口未隐藏。

新 `environment-ts-v2-script-object-owners` 保留旧方法查询记录，增加实际 ScriptObject 根与来源/owner facts，避免方法误入 native/flow 全局；callbacktype 由库投影为 DocFunction。真实固定 Retail catalog 在 `.tmp/rust-warm-owner-catalog-tests-01/catalog_native_real/` 通过：79 roots、1,438 methods，RegisterEventCallback 同名 global/method、函数/错误回调正负、AbortDrag 非全局及 capture 均实际验证。原 environment parity 三项、luac inline 及两项 exact Windows owner 退役通过。新增 environment_owners 测试一项实际 RED：测试误把无 Secret/Restricted/Protected 注释的普通 callable 视作风险规则；保留原源字节与所有 owner/span 断言，改为符合原 from_facts 合同的空规则，并另加具有真实风险注释的同名规则测试。该测试修正尚未重编译；该报告整体仍为 failed，不转记全绿。

完整 Retail06 `.tmp/rust-native-retail-controller-06/report.json` **首次完成受测连续查询和退出链**：同一 2,551 Lua/23,354,176 B 固定输入，cold outgoing 109.6s、warm context 100.3s、warm incoming 63.7s，warm 同 worker/inputIdentity、cacheHit=true 且 identity_only；显式 close 与 status closed 完成，controller/tree/句柄均回收。最后 worker 诊断峰值提交 1,907,679,232 B，固定上限仍 2,147,483,648 B。三次查询仍诚实 complete=false，有五个未解析关系；没有缩减输入或把未知事实改称完整。这关闭本次受测链的 EOF/资源失败，不关闭完整 Source 或 20 轮 Go/Rust 性能验收。此前 Retail01–05 和其原失败证据全部保留。

1f 精确退休已完成 `.tmp/rust-native-retail-controller-05/cache-retired.json`，1,558,331,290 B；保存 Retail05 与 query-current01 原失败 EXE/PDB。受保护四组测试、未知旧 Emmy cache、源码与全部日志不动，勿重跑已完成退休。下一批已写 item models 七表实现和业务夹具，未编译；Source 正在接参数数量/不可调用值等真实库 checker。严格检查、完整业务、性能、包装与实机验收继续未完成。

下一批首次 build `c79621b1-8f67-4136-bf2c-e5a577a18bf7` 实际编译失败：新增 callback 参数诊断把 i18n `Cow<str>` 传给要求 String 的方法，E0308 原 JSON/stdout/stderr 与终端记录保留。显式转换修复后 `31ed2bb3-3f3f-4292-9000-25c3a4a92aa7` 构建通过，source `4e3045d9cdcc52fa9f52220d91c4a757887abac2d983575165c42c83398cbb8a`、patch `6ee153a94f36febf3ab29491db3de5d89bd1707b35d2e45880445d1a9f1f50fc`、CLI SHA `f15aeb4d85810a0596ee637c20ee89426a5695001e4cd9aecbe666cf6d983419`。e606 精确退休 1,444,787,092 B 已完成，失败 owner fixture 和已验证 Retail06 CLI 已保全；c796 已完成库与其实际使用的完整缓存依赖精确退休 301,275,030 B，两个未绑定 Emmy session 及全部失败日志保留。两个结果分别在 `.tmp/rust-warm-owner-catalog-tests-01/cache-retired.json`、`.tmp/rust-checker-item-models-failed-build-01/cache-retired.json`，不要重跑。

31ed 的 `.tmp/rust-native-semantic-image-run-06/report.json` 原 SDK semantic/session 通过，`.tmp/rust-source-flow-run-11/report.json` 原 47 场景通过，`.tmp/rust-native-query-current-03/report.json` 原 178 加新增 20 项检查、101 内部断言和原独立 496 检查通过。`.tmp/rust-checker-item-models-tests-01/report.json` 的 owner 两项、item models 六项、精确退役两项通过；**整体仍 failed**，新的调用诊断 16 例第一例即因合法空函数 `local function one(x) end` 报 `native body missing`，未转记新诊断族通过。固定 parser 在立即 `end` 时不生成 Block；新的 invocation 空体修复保持正常 Continue 和零返回槽，标量/赋值补 nil、尾调用空序列沿现有 Lua 5.1 规则。额外回归已写，原 16 SDK 源例及旧 fixture 不改，尚待新编译。返回数量诊断仍未启用，不能因空体覆盖记录声称已检查 missing return。

item models `.tmp/rust-item-models-public-01/report.json` **64 profiles / 200 owned 调用全部通过**：36 成功 profile 的完整资料/外观/模型/材质/variant/关系和 capture/blob/SQLite/public verify；28 错误 profile 的 code/message/exit/stage/envelope 一致，七输入 variants、JSON/JSONL、i64 极值原始十进制 token 和真实 65,536/65,537 边界均验证。普通 CLI SHA 绑定上述 31ed，Go 使用冻结 `5738418756ee57c536518ff71223f2e7843ac3495a846b4f063d3104d83aae87`。独立 fixture 准备一次回收；未重复六个 active 测试。该业务通过不关闭 Data/provider/资源/性能总验收。当前下一批继续 Source 空函数与原诊断矩阵，以及缺失 item geosets；最终同一候选、严格检查、完整业务、性能/包装/Skill/实机仍未完成。

续验 `7c200505-6f5e-457f-95ba-f1f2113f4d2b` 构建通过，source `a5c493c13ce2da29fc382e22e305f54c4219b6271d83374fccf207eb877ec65a`、patch `6ee153a94…`，CLI SHA `d64704f2541868842b509d6be16a5f694fd781e56970532dd8cbb42314e9d2cb`。`.tmp/rust-native-query-current-04/report.json` 原 178 加新增 30 检查、109 内部断言、原独立 496 检查和十份真实 luac 语法验证全部通过。`.tmp/rust-native-semantic-image-run-07/report.json` 原 SDK/session 通过；models/geosets 各六项 active 测试通过。

`.tmp/rust-source-flow-run-12/report.json` 的原 47 个业务场景全部通过，但 F01 在多项并行工作时 owner wall 2,044.1ms，超过原 2s（worker 30ms，exit0/reaped）；该首次生命周期 RED 保留，不能把整体报告改为绿。其他场景生命周期通过。空闲环境以同一 binary、原原型/预算仅专项复验一次，`.tmp/rust-source-flow-f01-recheck-01/report.json` F01 owner 594.8ms、worker 16ms、2s 内通过并回收；未增加限额或重跑其余 46 项。

调用诊断矩阵 `.tmp/rust-empty-geosets-tests-01/` 通过前 13 例后，第 14 例 `function_callee` 为 command.deadline。确认测试在 16 次不同输入的独立 validate 间错误复用了一个默认 180s control，而不是函数调用性错误。仅将默认 control 的创建移入每次独立调用，原 16 源例/断言和外部 600s 监督不改。test-only build `df216dce-c06b-4d15-b72d-870535769e19` 实际通过，source `48981ce846f3e131306637cfd8dcf7a33432d66094905ede9a374fa60e0f2570`、相同 Emmy patch；`.tmp/rust-source-call-families-run-03/report.json` 完整 16 例最终全部通过并回收。五个实际 checker codes 为 param-type-mismatch、return-type-mismatch、missing-parameter、redundant-parameter、call-non-callable；含源码/真实 API 必选、可选、变参、回调归属、重新赋值、数/表/函数调用及 unknown/尾返回数量 unavailable。true/false complete 分别按实际记录，不提升为完整 Source。

geosets `.tmp/rust-item-geosets-public-01/report.json` **48 profiles / 152 owned 调用全部通过**，28 成功、20 预期错误均与冻结 Go 完整稳定 envelope/资料/关系/capture/public verify 一致，宽 visibility 与显式 uint32 selector 转换、真实 65,536/65,537 边界和超过 16 MiB 的完整 JSONL file sink 验证，无输出 cap 变更。共享 helper 的 models 代表回归 `.tmp/rust-item-models-public-02/report.json` 四项/16 owned 调用通过，原 64 大矩阵不重跑。所有消费者回收。31ed 与 7c 精确退休结果分别为 `.tmp/rust-checker-item-models-tests-01/cache-retired.json`（1,492,979,569 B）及 `.tmp/rust-empty-geosets-tests-01/cache-retired.json`（1,533,690,908 B）；原失败 family/已验证 CLI 成对保存，positive tests/probes 与指定旧测试保留，未绑定 sessions 不动，勿重跑。

当前 Data item textures 六表实现/六项业务测试/11 组夹具及 66 个公开 profiles 已写并冻结，尚未编译或执行。Source 正在接三类真实 return-count checker（缺显式返回槽、多返回槽及函数末尾缺返回），条件多路径调用仍需诚实 unavailable，不借声明或兄弟 Block 回退。parent Retail 暖请求约 46s 的 worker 之外时间已定位到完整 release inventory、环境重建和全输入构建链，但阶段归因与缓存/性能修复尚未实际测量。严格 Clippy、89 命令全量、Live 默认通道/统一协议/ABI、剩余 Data/Assets/provider、完整性能、对应源码两次重建/升级/Skill及最后实机都未关闭；goal 保持进行中。

## 完整研究材料、返回数量与纹理续验

`b61aa5e4-db41-4764-9c50-87188f97ead9` 构建实际通过，source `2a7e06908abfc9451dae64889d56e2c065d9f693fbad031e4214ebd82dcfd8b0`、Emmy patch `6710bcdf90edac85004e7b897ee36e61aeb9a80ae733c96bc065cc1966e7ab9c`、CLI SHA `f04687ebf3dbb6c618a347a5a1c248b7047944df1d2be5210e89e375eb0455e2`。完整 SnapshotRequest/Environment 材料复用原 CAS 与 Metadata，绑定真实 index CAS digest、源/环境 pin、CLI/引擎/patch/rules/luac/输入预算；命中完整读取并校验源字节、恢复 API observed spans/owner，再由 worker 校验完整 snapshot identity。计时仅在 `LYCHEEDEV_SOURCE_PHASE_PROFILE=1` 时写小型 stderr JSON，不进入输入 key、业务或 capture。

`.tmp/rust-native-query-current-05/report.json` 的原 178 加新增 30 外部检查、109 内部断言、原独立 496 检查和十份真实 luac 语法全部通过；`.tmp/rust-native-semantic-image-run-08/report.json` 原 SDK/session 通过。`.tmp/rust-material-return-textures-tests-01/report.json` 三项 Data 套件各六个 active 测试通过，但原整体 failed 保留：新 return-count 测试在进入 19 例前，用短名 After 查实际 name=C_Timer.After 的记录失败。根据实际已捕获 definitions 材料核对，仅修正 selector；namespace/kind/owner、零 returns 与 MayReturnNothing 证据及原 19 源例/断言不变。

textures `.tmp/rust-item-textures-public-01/report.json` **66 profiles/200 owned 进程全部通过**，34 成功/32 预期错误的完整资料、section/material/texture/关系/capture/blob/SQLite/provenance/public verify 和稳定错误 envelope 与冻结 Go 一致。真实 65,536 relation、65,537 section/texture 行边界及大于 16 MiB 的完整 JSONL file-sink 均实际验证，原输出 cap 不变。models/geosets 代表回归分别 `.tmp/rust-item-models-public-03/report.json`、`.tmp/rust-item-geosets-public-02/report.json`，各四项/16 owned 通过；原大矩阵不重跑。所有消费者退出回收，Data 全量/provider/资源/性能总门槛仍开放。

Retail07 `.tmp/rust-native-retail-controller-07/report.json` 完整 2,551 Lua 固定输入 cold/warm context/incoming/close 链通过并回收，冷 outgoing 98.008s、warm context 54.545s、warm incoming 23.406s，同 worker/inputIdentity、warm identity_only。峰值提交 1,908,310,016 B，固定 2,147,483,648 B 上限未改；三次结果仍诚实 incomplete/五个未解析关系。源材料缓存实际取消重复重建，不能把本次少量诊断外推为 20 轮 Go/Rust 中位性能通过。

stderr phases 实测冷请求 release inventory 1.803s、index 2.299s、environment 2.599s（Git .196s/extract 2.402s）、snapshot 33.839s，其中 load_facts 33.310s（全 Lua blob .359s）、材料写入 2.368s、native analyze 54.187s。warm context 材料完整读 1.210s/准入 .732s，native analyze 16.598s、snippets .245s，但壁钟仍 54.545s。源码确认 context 无条件再做同份 load_facts。后续材料升 v2，存储并移动完整认证 Vec<LoadFact> 给 Reading/context；static/flow 仍原入口，loadEvidence/closure/gaps 和非所选 load facts 均保留，新增 context.load_facts phase，未删功能或缩小输入。

`0a4915b9-ca07-4503-b7a9-65a3d0d7b46d` 实际编译 Source lib test 和修正后的 return-count test，source `9b453d4395b4976ee8b03c13883d5c0e4ca10bcff5957f9e70c545af4172b7fd`、相同 patch。`.tmp/rust-material-cas-tests-01/report.json` 两项实际测试通过：完整源/API owner/span/加载事实/缺口 roundtrip、原 index 源内容准入、CAS 改写拒绝与已清理 miss、准确序列化上限及超一字节不缓存。`.tmp/rust-source-return-count-run-02/report.json` 原 19 个真实 SDK case 完整通过并回收；新增三个原 checker code 为 missing-return-value、redundant-return-value、missing-return，包含尾/非尾/括号调整、重赋值、条件及 unknown 保留。v2/context 新代码尚未编译为普通 CLI，仍需新的完整 Retail/profile 链；其 lib 测试不能冒充 CLI 验收。

Root 补齐受控 Cargo metadata 路径；七项 build runner Node 测试通过，metadata 允许非 manifest 源码并发编辑但拒绝锁文件变化，原配额不变。`.tmp/rust-dependencies-current-02/dependency-closure.json` 的 Windows 锁定 graph 251 项许可证文本库存包含新 codecs 和三项私有 Emmy crate，拥有 metadata receipt `75311a45-81ea-4f9b-8234-62de9e917b2d`。原归档遗漏的五个 MIT 文本从各自 Cargo VCS revision 保存并核 SHA；两个 rust-i18n 子包记录使用官方 v3.1.5 workspace LICENSE（源提交 5624dcb），并明确 Cargo revision a6b110e 上的字节未能验证，不宣称是同提交文本。完整库存与 provenance gap 不等于最终许可政策/对应源码验收通过。

df 精确退休 `.tmp/rust-disk/evidence/df216dce-c06b-4d15-b72d-870535769e19/cache-retired.json`（873,287,328 B）及 b61 `.tmp/rust-material-return-textures-tests-01/cache-retired.json`（1,830,102,433 B）均已完成，全部当前 positive EXE/PDB、保存的历史 CLI/failures、旧四组测试和未绑定 sessions 保留；勿重跑。当前还需普通 CLI 编译/完整 Retail 复验、严格 Clippy、后续 Source 类型/检查族、Live/其余 Data/Assets/provider、全部命令/性能、两次 offline rebuild/升级/Skill及最终客户端。goal 未完成。

### 材料 v2 普通 CLI 续构建（2026-10-13）

首次准入因 cache 20,848,347,915 B 与固定 2 GiB reserve 拒绝，未启动 Cargo。按实际 0a producer 精确退休 `.tmp/rust-material-lib-returns-tests-01/cache-retired.json` 完成 1,527,671,319 B（444 文件/6 唯一 incremental）；16 对 CLI/test/probe 保留，配额未变。

普通构建 `e4a56a6e-53a7-4fbc-b4c7-7d2dadc3cae1` source `d177ae17ba3556de8b7e5c56d896c57a91e37bbac685511559e092890f053ac5` 实际失败 E0609：清理 IndexedCall 时误删了 queries.rs 使用的 id 字段。id 及原 digest 已恢复，仅移除无使用者 closure。原编译 JSON/日志保留，Cargo exit101；受控进程记录显示 VCTIP 未在 drain 内退出并被所属 job 收妥，tree/handles 全部回收，deadlineHit=false，不能将其误记编译超时。重构建准入又因 cache 20,548,093,630 B 拒绝，当前正按本次实际 producer 规划可再生缓存，尚未完成普通 CLI/完整 Retail 复验。

Retail 原 driver 增加原始 AuraUtil TOC loadEvidence 的 path/line/sourceBlob SHA 与字节断言；node --check 通过，实际运行待新 CLI。此批不代表 Source、性能或最终候选通过。

续构建 `4383c8ff-829f-4efe-8dd8-caff950716ee` 实际成功，source `c0bbe817169f524273f5510bfc801cab5547a3fe4d4152726d33368cbe15c491`、patch `6710bcdf90edac85004e7b897ee36e61aeb9a80ae733c96bc065cc1966e7ab9c`、普通 CLI SHA `7d10e391dc618cee54a1f2004d35de2416c20f60033fae989634c73b94b56d0d`。包含 CLI 和当前 query/flow probes，无测试专用降级。`.tmp/rust-native-query-current-06/report.json` 实际原 84+新增25 断言、178+30 外部检查、独立496检查、10份 luac 语法验证全部通过，owned tree/handles 回收。

完整 Retail08 `.tmp/rust-native-retail-controller-08/report.json` 同固定 2,551 Lua/23,354,176 B 成功完成 cold outgoing、warm context、warm incoming、显式 close/status closed；同 worker68204/inputIdentity、warm cacheHit=true。实际 wall 分别95,510.1/23,245.6/24,630.6ms；context 的完整原始 TOC loadEvidence 新断言通过，`source.context.load_facts` 阶段0ms，消除了此前约31s重复解析。完整材料仍认证/读取，native query仍运行，未缩减源或改预算。三次结果仍 complete=false/五个未解析关系；峰值提交1,908,232,192 B，固定2,147,483,648 B上限，原 worker诊断和原始阶段日志保留。仅证明本次连续业务/缓存/关闭，不是 Go/Rust各20样本性能或完整 Source 验收。

e4 精确退休 `.tmp/rust-source-material-v2-cli-failure-01/cache-retired.json` 已完成1,229,884,832 B（554文件/3唯一sessions），全部16对CLI/test/probe和新Emmy/Core/Windows库对保留。执行前因真实foreign wowdump Cargo短暂拒绝，未删/未干预其进程；观察到其退出后才执行。0a/e4退休不重放。下一批 nominal 实例方法与 creature 公开差分继续实施；严格Clippy和其余全量目标仍未完成。

同 4383 probe 的 `.tmp/rust-source-flow-run-13/report.json` 原47业务场景/business/lifecycle全部通过，实际进程回收。此前F01 wall失败及原quiet recheck仍保留，不改历史结果。handoff与当前checkpoint已更新，goal继续active。

### Nominal 实例方法与 creature 顺序续批（2026-10-13）

Source 增加真正 `LuaType::External`，以完整 catalog hash/runtime type/owner 标识 nominal；owned methods 独立绑定，未变成全局函数。仅根据固定 catalog 的真实 self-copy 记录接通 Duration、Curve 两种明确映射，未发明父类或新 UnitToken alias。真实调用使用原字段读写、方法别名与未知效果失效机制，签名 receiver 支持冒号和显式 dot+self。

测试构建 `03f5d7c5-c740-404b-ad54-e44fa9936a4a` 成功，source `236cc7066d0da184466899818d82093d6853eb1d5840c873c9d35fee90dc122b`、patch `f56f79aa19ce64f8342851eb0d81c3a663241b42aeb4b8dde1ee459c42a67fc0`。`.tmp/rust-native-nominal-semantic-run-01/report.json` 实际 nominal18场景与原 semantic/XML SDK 检查通过，所有消费者回收。nominal覆盖连续Copy→Copy→Assign、receiver偏移、普通table/不同nominal/标量错误、缺参/多参、creator/member重写及保存方法；18份业务报告仍 complete=false，不转记完整Source。`.tmp/rust-runtime-open-lib-tests-01/report.json` 实际Live库83 passed，新增RuntimeBoundMemory::open只整合模块/recipe/layout/bind同一budget入口；生产profiles/defaultchannel/完整协议仍未通过。

Creature新增31组认证CASC/DBD/WDC夹具及14 active tests，原17公开场景保留，公开runner总168配置；真实signed64字段支持实际i64回绕验证。`.tmp/rust-creature-fixture-preparation-01/report.json` 已生成完整夹具并回收；未把生成算业务验收。旧4383 CLI对冻结Go的 `.tmp/rust-creature-before-order-fix-01/report.json` 六配置实际RED：首表坏/后表缺，Go table.invalid_format exit5，Rust schema.definition_not_found exit3，12 owned进程回收。已将两个creature入口改为逐表完整打开；正常CLI/旧检查族/return-count/query/flow和creature测试正在受控重编译，完整公开矩阵尚未运行。

4383 精确退休 `.tmp/rust-4383-next-batch-cache/cache-retired.json` 完成1,870,730,683 B；其真实CLI/query/flow三组EXE/PDB先按报告身份保存（201,287,168 B，preserved.json）。03f5 精确退休 `.tmp/rust-nominal-creature-runtime-tests-01/cache-retired.json` 完成2,070,165,279 B，仅额外包含两组已编译未消费lib test，19组已用/必要pairs、夹具和历史日志保留；两次退休均勿重放。goal继续active，全量质量/业务/性能/候选/实机未闭环。

续构建 `336c130d-f9ed-4a47-8898-377e6d3d52dc` 实际成功，source `6e5c791d90cb6728d7f3810ad89537686378891a2b508528e59414c742d04605`、同f56 patch；正常CLI SHA `b530e24ecc482b720a0dc7cf391a4ec0978a1d1c3222bb7b1202cedb044df7f1`。`.tmp/rust-nominal-creature-regression-tests-01/report.json` 实际creature14 active、原checker16/return-count19 SDK场景全部通过，全部owned进程回收。`.tmp/rust-creature-public-01/report.json` 168/168配置、496/496owned调用通过、差异零，包含原17场景、首坏后缺、typed/missing字段、损坏非匹配行、真实signed64回绕、65,536/65,537边界、capture拒绝和公开evidence/capture完整内容；最大保留stdout16,215,545 B，未宣称超过16MiB。资源计账remainingGaps仍如报告所示，非实机/CDN或性能验收。

同336c `.tmp/rust-native-query-current-07/report.json` 原109断言/208外部检查、独立496检查与10份luac验证通过；`.tmp/rust-source-flow-run-14/report.json` 原47/business/lifecycle通过。当前完整Retail09正在实际Root session59011运行，尚未确认warm链/关闭及当前nominal改动下的全量资源。严格Clippy、余下Source类型/XML、Live生产布局/协议、Data资源/provider/Assets及最终全量门槛仍未完成。

完整Retail09已终端成功并回收：同worker65580/inputIdentity、2,551 Lua完整输入，cold88,963.7ms/context22,061.4ms/incoming21,797.1ms；warm cacheHit=true，loadfacts0ms，原TOC证据、显式close/status closed通过。峰值提交1,910,099,968 B，原2,147,483,648 B上限不变。三份结果仍complete=false，未解析关系分别5/5/1，不把新incoming改善记成全量解析完成；仍非20样本公平性能验收。下一严格Clippy需当前336c精确缓存规划，不能在失败/未执行时写通过。

336c 精确退休 `.tmp/rust-336c-clippy-cache/cache-retired.json` 已完成2,028,769,603 B，19个CLI/test/probe受保护。严格workspace lib/bin Clippy `bc41a052-4eca-4862-ad47-d274c8759927` 实际failed/exit101/reaped，Source9项真实lint日志保留。已修7文件：assignment空格、独立if换行、catalog identity initializer、原短路条件合并、native_join组合branch/join元组，以及全crate核查后删除IndexedReference三个未读字段（保留实际使用的IndexedCall.id和role计算locals）。未放宽lint/测试。重跑仍需当前bc41可再生check缓存空间，尚未写严格通过。

bc41精确退休 `.tmp/rust-bc41-clippy-cache/cache-retired.json` 完成722,521,927 B，正产物和历史保留。重跑 `0382f03d-fc41-461f-8e32-eb86cfe99234` Source阶段无lint，Data阶段实际failed两项并reaped：固定4-byte像素分块用as_chunks_mut::<4>().0保持相同完整分块/尾部语义，item models嵌套条件保持原筛选/短路顺序合并。Root已修；再次准入因cache20,149,590,035 B拒绝，尚未启动下一Cargo，当前按0382规划可再生缓存。全严格检查仍未通过，普通336c业务产物仍为lint修复前身份。

0382精确退休 `.tmp/rust-0382-clippy-cache/cache-retired.json` 完成823,803,737 B。后续严格 `35331aff-1091-42e9-ab19-6a17637cda01` Source/Data阶段无lint，Live阶段10项failed/reaped；仅两个Live runtime文件修复：RootFailure.evidence/RuntimeBindingFailure.root装箱保持原cause/stage/usage和JSON形状，PE section固定40-byte分块保持尾部语义。35331精确退休 `.tmp/rust-35331-clippy-cache/cache-retired.json` 完成839,819,132 B。

最终严格workspace **lib/bin** `b3e4fdec-2997-43d2-95b7-37f7d81d7436` 实际succeeded/exit0/reaped，source `d333492defc88c93fe39f60bc13b6a2b1bae59f65797ea900aa80a3d963f09dc`、同f56patch，完整使用`-D warnings`，未压lint/预算/测试。仅证明生产库/程序检查；all-targets/最终候选全质量仍需按目标验收。正常336c CLI与已通过业务报告均为lint修复前身份；下一需重编译受影响LiveLib、query/flow以及Data像素/models代表回归，不能拿旧报告声称新lint修复已实际执行。

已定位下一真实资源缺口：installed.rs解析/读取无预算，local_query.rs仅事后补计，初次和最终重校验都须沿同一budget提前计费；Catalog CaptureWriter已计费，证据gap主要是Core bounded metadata/identity与SQL先to_vec后检查；BLTE预收费Decoder{budget:None}路径遗漏内部控制检查。只是只读审计，未把三个gap改成完成，待实施。

### lint 后业务补验（2026-10-13）

88e26ba3-7b16-438f-9b1a-56aa06b08811实际普通构建成功，与严格b3e4同source d333492defc88c93fe39f60bc13b6a2b1bae59f65797ea900aa80a3d963f09dc/f56 patch；新CLI SHA6146beec12ba44b5fff8379b027bd98d5a1786973ea51b2a3edc8d1426e44ffe。`.tmp/rust-native-query-current-08/report.json` 原109/208与独立496/luac10通过；`.tmp/rust-source-flow-run-15/report.json` 原47/business/lifecycle通过；`.tmp/rust-post-lint-data-tests-01/report.json` image2/models6实际通过，全部进程回收。未重复未受影响的creature168矩阵。Live错误装箱的库测试重编译仍待进行，不能拿旧03f5报告充作本次运行。

b3e4精确退休 `.tmp/rust-b3e4-normal-cache/cache-retired.json` 完成854,165,343 B；普通覆盖前按336c清单保存六组实际EXE/PDB（398,211,072 B，`.tmp/rust-336c-clippy-cache/preserved.json`）。原正常/失败/SDK/fixture证据保留。预算化安装解析确认只有local_query初次resolve_target/末次resolve_local及catalog额外check_source三个调用点；重试/额外重验要真实重新计费，SQL还保留独立plan/order-container gap，未实施前不改质量报告。

Live装箱后的实际测试构建 `5e624d4b-4a4e-4c83-a919-48a65fe3abf1` 成功，同d333/f56；`.tmp/rust-post-lint-live-tests-01/report.json` 83/83库测试通过，进程回收。此前lint后未执行的路径已补验，不扩大为生产RuntimeLayout/defaultChannel/游戏验收。88e精确退休 `.tmp/rust-88e-live-lib-cache/cache-retired.json` 完成2,082,694,214 B，20个消费对与历史保留，勿重跑。

资源续批已开始：安装身份解析实施由Data worker拥有；Root仅修改BLTE Ranges预收费分支让Decoder沿同一budget进行scratch/nested/cipher及循环work计费与控制检查，并在zlib成功读取后检查取消/截止。外层extent预收费仍服务output/copy+digest；没有改变caps或数据。新增128KiB真实framed zlib range的正常字节及低工作预算反例，旧budgetNone路径无法满足该断言。现仅代码已写，尚未编译/运行；其它单次image codec调用仍未关闭，因此不删除uninterruptible gap或声称全资源计账完成。

安装解析续批已编译：b211b5b1-6353-452b-a457-9030d00b9d57 succeeded/source a929211013d84ee1969d2341956dae99f88f71db54acbd10a79c0a3818e2c307/f56。`.tmp/rust-installed-blte-budget-tests-01/report.json` 实际73passed/0failed/9ignored（Data库36、installed4、resource3、content8、image2、models6、creature14），全部消费者回收。真实Zlib128KiB正常字节/低工作预算反例通过；配置parser先承认token容器、临时scratch释放、MD5/文本/字段上限错误顺序、client/common-root原选择、同budget重复校验、取消及capture不变都通过。ignored实机/显式fixture准备不转记通过。

生产初次resolve_target、末次resolve_local及catalog/encounter原有check_source均接同Budget；删除旧事后raw+4096估算。分配前承认、临时reservation释放、逸出结果commit；重试/重验实际再次收费。四生产结果/六公开runner仅移除installed gap，SQL plan/order gap及其他gap保留，wholeQueryAccountingComplete仍false。正常CLI88e尚未重建此资源改动，公开业务/capture回归及当前strict检查待下一batch。5e精确退休 `.tmp/rust-5e-data-test-cache/cache-retired.json` 完成406,558,787 B、21消费对与历史保留，勿重跑。

正常资源CLI `1bd7f9ba-0c10-430b-b764-acd3a479ccc4` 实际构建成功，同a929/f56，CLI SHA7b66937c043d8b1e2bf918e2db635e86adeab5d7c423c59f460b9f970df58bef。公开模型 `.tmp/rust-installed-models-public-01/report.json` 10配置/32owned、creature `.tmp/rust-installed-creature-public-01/report.json` 20/68、content `.tmp/rust-installed-content-public-01/report.json` 20/60 全部通过/回收（合计50配置/160owned）。代表覆盖冷/暖、原错误顺序、真实i64、行/关系边界、完整envelope/capture/metadata/公开verify；保持资源诊断与其它gap真实，不重复原全部大矩阵。当前strict准入因cache21,422,183,763 B与固定2GiB reserve拒绝，未启动Cargo，按1bd7精确可再生cache规划后重跑。

b211精确退休 `.tmp/rust-b211-normal-cache/cache-retired.json` 完成988,253,553 B，包括明确授权、尚未退休且旧sealed记录/current hash匹配的12库文件/两analysis check sessions，以及同fileID build-script别名；仅logical pathname quota记账，不宣称等量physical free。当前CLI覆盖前保存原88e EXE/PDB（仅producer identity，无独立CLI业务报告）于 `.tmp/rust-88e-normal-cli-preserved/compiled/`；其它copy-only只按将来实际覆盖需要执行，未复制未覆盖项。所有旧正常/失败/fixture/测试证据保留；这些退休不可重放。

当前资源source a929/f56的严格workspace lib/bin `8a2fd831-f564-40b6-af77-7d07139aeffd` 实际succeeded/exit0/reaped，完整`-D warnings`，无抑制/改上限。1bd7精确退休 `.tmp/rust-1bd7-strict-cache/cache-retired.json` 完成2,097,237,792 B，28消费对/历史保留，不重放。安装身份缺口已在当前CLI、实际tests与公开代表路径验证；BLTE内部检查已由真实低工作预算反例验证。没有将它扩大为全部资源、all-targets、20样本性能或最终候选通过。后续证据序列化、SQL plan/order容器和其它codec真实gap继续未完成；Source XML有序对象/handler self方案仍只读调查，无实现。goal active。

### Capture 序列化与发布预算（2026-10-13）

Core 新预算入口复用调用方账本：元数据计数、Go HTML/Unicode 转义身份流式 SHA、普通 JSON 写入及公开 Value 在插入前计账；原 schema、字段顺序、1MiB metadata 上限、CAP 身份与错误保持。Metadata 共用事务以 SQLite BLOB 字节比较判断幂等，不再复制已有文档；expected generation 更新和冲突仍保持。materialized body 的 SHA 与实际发布/已有 blob 复核各独立预付每64KiB一个工作单位，正文分配由 producer 承担；held verified 入口不增加正文复制或扫描。取消仍可能留下已发布 blob，不承诺回滚正文。

Data 的 DB2、SpellEffect、SQL、Catalog/encounter、stream、asset inspect/input/raw/image 接入预算入口；普通 JSON 保持普通 serde，Catalog 继续原 Go formatter。行计数和 SQL request SHA 不再先分配临时 JSON Vec。普通与 Catalog 缓冲扩容都先承认新旧缓冲同时存活，成功后释放旧额度。四生产结果与六公开 runner 只移除 evidence serialization allocations gap；SQL plan/order 与单次 codec gap、wholeQueryAccountingComplete=false 保留，hotfix/demux/listfile 等整体计账尚未闭环。

stream 现在先成功提交 manifest capture，再构造全部 Outcome/terminal，最后发送 end；end 后仅返回已构造结果。冻结 Go 原实现先 end 后 capture，因此本次修正其发布失败时的终端顺序；成功业务字节保持。任意 end 写出/flush 失败仍须看退出码，合法 end 加 exit0 才是成功。提交后的取消可留下 capture 并 error/no end；测试只证明受测发布失败/提交前取消没有新 capture。

受控测试构建 `5910ab68-b784-49b6-9250-9b3effc53b5e` succeeded/reaped，source `466572f0da36772d002a9ba16a229ced9aedb68c2094ed37c2b791f69673ece0`、同f56 patch。`.tmp/rust-budgeted-capture-tests-01/report.json` 实际 **120passed/0failed/8ignored**，全部消费者回收：Core库18、Data库38、budgeted_capture4、bounded_evidence3、durability28、content10、local-domain1、catalog7、encounter9、image2。包含固定Go转义CAP黄金、预算/取消/oversize拒绝与旧blob保留、Metadata幂等/冲突、SQL与DB2 record/page实际CASC/DBD/WDC合成fixture的行7/完整capture精确字节及固定SQL request SHA、stream真实发布顺序/失败/取消。不是实机或全部provider验收。

8a2精确退休 `.tmp/rust-8a2-evidence-tests-cache/cache-retired.json` 完成519targets/982,328,091 B；当前CLI与全部28消费对保留，勿重放。新构建覆盖前保存14files/7对旧测试产物246,160,896 B，清单及实际复制校验在 `.tmp/rust-capture-prebuild-preserve-01/preserved.json`。当前正常CLI1bd7与严格8a2仍为本批改动前身份；正常CLI公开差分和新严格检查继续待办。整套goal active。

当前普通 CLI 构建 `5db75cbf-8496-4068-a993-e3aeb9dfd053` succeeded/reaped，同4665/f56，封存 `.tmp/rust-capture-current-cli-01/artifact.json` / CLI SHA `33daabe4cb12dcd4be9945a164fc6146977ef59c7d66d794553294219d51aaf4`。该 buildRust 调用没有 compiler-artifact JSON；实际 stdout 为空、stderr 编译文本，不能伪称拥有 JSON 产物清单。5910精确退休 `.tmp/rust-5910-normal-cache/cache-retired.json` 完成339items/1,081,482,019 B；1bd7 CLI/PDB覆盖前按实际models/creature报告保存，见 `preserved-cli.json`。不得重放退休。

公开 Catalog `.tmp/rust-capture-catalog-public-01/report.json` 8格式配置/30owned，encounter `.tmp/rust-capture-encounter-public-01/report.json` 8/56，完整保留envelope/捕获身份/正文/SQLite/公开verify均passed/reaped。`.tmp/rust-capture-public-run-01/report.json` 初次SQL/DB2/stream专门差分保留失败：已完成6格式配置，其中SQL literal与record的4配置passed，参数SQL的JSON/JSONL两配置locator不等（Go ac2dfa…，Rust bcec4e…）；源于Go DataQuery的sql→parameters字段顺序与Rust map排序。另runner错误假定Item存在两行，在第一个Go page处停止；该fixture实际只有ID7，page完整且more=false。38owned全部回收；不把observer误判或未运行page/stream转记产品失败或通过。下一批修正locator有序Go转义SHA及observer事实后新CLI复验，首轮证据保持。当前source开始该修正后，4665CLI与120tests只证明本轮修正前身份。

参数SQL修正完成：借用sql与非空parameters的有序Serialize结构体计算Go转义SHA，不复制/生成中间JSON正文；catalog原formatter移到capture.rs共用，已有capture正文编码保持。literal/参数/HTML/U2028/U2029/i64/u64三固定golden通过。observer依实际存档更正Item单行page完整性，stream用ItemSparse两行证明完整及截断，不删除截断业务。新增参数SQL真实fixture dispatch/capture精确字节和固定locator通过。

当前构建 `4c5c8ad1-9322-4c73-a9f8-b29541053398` oneShot受控普通通道 succeeded/reaped，source `e366ab5a02732436dbfad4497f2045e5ac8cc5e67c0ffe687ddb66a7ef39b3bb`/同f56。实际compiler JSON包含正常CLI与5个test harness；`.tmp/rust-sql-go-locator-tests-01/report.json` **66passed/0failed/6ignored**（Data库39、content10、catalog7、encounter9、Source真实CLI flow1），全部消费者回收；Source原ignored用既有验证release root实际执行，保持原2秒wall、完整input/unknown effects和capture提交断言，不扩展为全Source实机/性能通过。

新CLI/同次PDB已封存 `.tmp/rust-capture-current-cli-02/artifact.json`，CLI SHA `be2099b09e9870ccb492728f63342d1e9b51bde28b2f2516d150e9f42c773916`，PDB `f185e771a0da1ca95a8455dcf1408a2b5a6c2c4d15700f5b6da2a03300d50a17`。`.tmp/rust-capture-public-run-02/report.json` SQL literal/参数、DB2 record/page、完整/截断stream共10格式比较passed；60owned回收，独立新home只导入pin/定义缓存，完整legacy结果/SQL locator与capture身份/blob/SQLite/公开verify通过。首次失败保持。当前严格workspace lib/bin尚待执行，整套goal未完成。

5db精确退休 `.tmp/rust-5db-next-cache/cache-retired.json` 完成709items/2,240,773,333 B；已知路径当前hash与原sealed清单/唯一session核对，明确该构建无JSON artifact记录，不虚构owner或按mtime选目录。新oneShot构建缓存增加保持20GiB总上限内，不提高既定配额。原5db PDB经PE CodeView GUID/age核对后保存到其immutable CLI旁；历史消费对均保留，退休不可重放。

当前source e366/f56的严格workspace **lib/bin** `086fea29-6329-4823-8379-0d5b4bb98be3` 实际succeeded/exit0/reaped，完整`-D warnings`，无抑制或预算调整。4c精确退休 `.tmp/rust-4c-strict-cache/cache-retired.json` 完成702files/1,824,749,026 B、无session；所有80消费文件和当前Data/Live库保留。少量SDK可重建库的边界为旧sealed实际artifact路径加当前精确SHA，不声称历史SHA相等，亦未重放旧删除清单；此前已归档的旧测试原件最终也全部保留。当前无Root构建或消费者进程。受测capture/SQL批次完成，不扩大为all-targets、全部provider资源、全Source或最终候选通过，goal active。

下一Live ABI目录的只读事实已收敛并由Root核对四profile文件SHA：Retail69933 `1afe07213bb3979384c5939062df458ea9f26ef5d1b3e98a8cff5fa382f56406`；Classic70032 `3c7cf9920b59e62c8ab62ab296c47df50a73c8666010d38d2712650e0269d6db`；Titan69874 `98e5bc40a701845af2552e13545d0800d7fd2bc86ae07b8d80e03b5e00a9ef7d`；Forever70124 `2c85b30a93ab8b4f0da884f928a02e4bf8cc051c7b67071d8c41ce15432a4db9`（路径均位于对应research目录）。四份历史Lua ABI布局相同；前三端用Retail recipe，Forever用Forever recipe。Forever历史`.flavor.info=wow_classic_beta`支持Asia映射，无CN `wow_cn_beta`证据；不能由同build继承。profile文件本身保留candidate标签，完整实际read/reader/验收确认仍须引用旁边历史记录，不能单独用Go数值偏移或RIP命中发出生产能力。当前PROFILES仍空，普通native Services、WindowsChannel、Session runtime identity的生产接线尚未实现；只读结果不是Rust500槽/实机通过。完整运行时代码扫描也不能被ABI目录绕过。Source XML有序对象/handler-self依然待实施。

### XML、被动实例和运行时 ABI 续批（2026-10-13，实施/验证中）

Root已在runtime_binding.rs录入上述四个精确历史ABI及其profile SHA，并引用旁边实际read/acceptance记录；目录单测核对存档布局、根路径、product/channel/build/recipe，未知build/CN不继承。它仍不是默认新协议输入服务；旧native targets/open/moduleSha256/optical接线继续待替换。不能把源码目录已录入说成实机通过。

`live instances` 已写独立passive模块、Services.instances入口和CLI/lib接线，恢复Go installations/candidates DTO，默认与显式--passive一致；独立窗口保留，--installation仅追加安装扫描根，元数据失败保留候选，读取占用记录但不创建home/锁。新的Windows枚举使用PROCESS_QUERY_LIMITED_INFORMATION和路径/创建时间/窗口信息，避开Process.main_module、EXE字节/hash、进程内存、Mailbox、WGC和按键。8个单元/2个ownedWindows/CLI封装用例尚待运行。

Source首批 XML 实现已写：共享v8 extractor、原TOC/Include/XML顺序、虚拟/固有定义与独立对象、实际Lua mixin字段版本、handler self、XML原字节来源，以及同一query/checker入口。首入口是source validate --semantic；16个真实SDK风险用例和公共query断言尚待运行，默认refs/context的Snapshot/cache聚合输入仍待接通。Root审查修正了把20k file-load上限误作action上限的问题，使用原load/include/XML nesting/construction/object预算；字段失效时不再声称复制过期source write。所有原Lua文件与2GiB边界保留，不猜Frame继承/nativeMixin/生命周期。

首次联合编译 `9c0fa702-070c-42f4-a22d-c00b8b64ae33` actualexit101，Windows IsWindow参数需Some(HWND)，Root已修；再编译 `77052a43-5745-46b9-b4c0-c874b1cc47c8` Windows通过、Source三编译错误（两Arc<Program>序列化、XmlOccurrence fingerprint match缺分支），修正中。两次另有VCTIP非reviewed auxiliary在Cargo已退出后的drain forced清理；所有句柄回收、deadlineHit=false，不将其误称超时或覆盖实际编译错误。770 source64da/私有Emmy patch `ea944114e1dda1918dec3505a47f071d704d7fc17060bf870b2b37925fb97a01`；当前正常CLI仍be209旧批身份。

086精确退休 `.tmp/rust-086-next-tests-cache/cache-retired.json` 完成571items/1,122,027,703 B；9c精确退休 `.tmp/rust-9c-retry-cache/cache-retired.json` 完成336files/692,074,277 B。全部80消费文件/immutable CLI/PDB/旧失败日志保留，禁止重放。

### eff5 首次执行及 XML 修复待验证

受控 oneShot producer `eff5f3a6-b0d6-46fa-a095-438eb456dfb0` 已 succeeded/reaped，source `f108230895610acfe1d7664f8198fd163f93cc68e5412d0262254641d3868457`、Emmy patch `ea944114e1dda1918dec3505a47f071d704d7fc17060bf870b2b37925fb97a01`。实际 Cargo JSON 包含正常 CLI 和测试产物；正常 CLI/同次 PDB 已封存 `.tmp/rust-xml-passive-cli-01/artifact.json`，CLI `6f86e8ea97aff31b37d67b0057d0ba8d878c3db5a81af68e36ab220ada7ea8c6`。该产物早于下面的 XML 修复，不能作为修复后的身份。

`.tmp/rust-xml-passive-tests-01/report.json` 保留首轮实际结果：Live lib 92、CLI lib 20、owned passive Windows 2 全部通过；Source lib 57 passed/4 failed/2 ignored，真实 XML SDK 外层用例 failed，所有进程回收。Source 两个旧 session deadline 用例仍失败；它们不是本批改动，也不将完整 Source lib 标成 green。另两个新失败是 generated footer 原行映射和测试重复添加 BOM，现已修复：复用现有 closing-tag anchor，保留原期望行与 synthetic raw=null；golden 直接使用原有单 BOM 字节并保留双 BOM 拒绝。SDK 失败来自负例对正常 public validation error 误用 unwrap；fixture 现在保留完整报告并验证 exact error code/exit、valid、semantic、syntax 和原全部断言，未放宽生产 validator。修复尚待新编译/执行。

`.tmp/rust-xml-original-type-tests-01/report.json` 的 nominal、return-count、checker-families 三套真实分析外层测试均 passed/exit0/reaped，原具体用例报告保留，范围绑定 eff5 源码。`.tmp/rust-xml-current-flow-tests-01/report.json` 中 semantic_validate_real 和 source_flow_public 也已 passed/reaped，原 2 秒 flow wall 不变；同批 lifecycle 尚在执行，未提前算通过。默认 refs/context 的 XML Snapshot/cache 输入、生产 Live 默认接线及完整 goal 仍未完成。

当前 eff5 lifecycle 消费者也已 passed/exit0/reaped，见 `.tmp/rust-xml-current-flow-tests-01/report.json`；实际新版 driver 对应 `.tmp/rust-supervision/native-revised-lifecycle-82b1f2b10746ca6dd045832c8438014a/result.json`，`maximumCodeFailures=[]`、`finalProtocolPassed=false`。两/四槽 MAX_CODE 完整生命周期分别 544/560 ms，原 750 ms 窗口与 code/close/clear 各一次断言保持；实际 initializer/native bit/Lua DLL 输入 SHA 前后稳定。本次证据关闭当前源码原型受测范围，不关闭独立审计、P1/clock/B1/500 allocator/OS carrier 或生产接线。

被动公开差分 `.tmp/rust-passive-public-run-02/report.json` 四次 owned JSON 调用 exit0/reaped，原始 `exact=false` 继续保留。仅在观察到精确 `forever/wow_classic_beta/us/1.60.1.70338`、原 flavor/build 元数据不变且无 version.txt 的条件下，逐字段记录已批准的 productCode 与更准确 identitySource 差异；所有其他 envelope 字段、顺序、窗口身份、状态和退出精确相等。Go、Rust 默认/显式 passive 各自 exact=true，四个 home 始终未创建，首次 run-01 报告未改写。本轮 CLI hash 仅引用既有报告，明确未重算；没有读取 EXE 内容、内存或发送输入。范围是当前 Asia 单窗口 metadata-only，CN、多实例、JSONL、连接、性能和实机业务仍未验收。

eff5 精确退休 `.tmp/rust-eff5-xml-retry-cache/cache-retired.json` 已在全部消费者终态回收后完成 726 files/2,173,668,789 logical B；无 session，所有保留的 runnable/归档副本/immutable CLI+PDB/输入/原报告保持。不得重放。当前新编译只验证 XML 映射与负例 fixture 修复，默认 XML Snapshot/query/cache 的完整接通随后继续。

XML 修复重编译 `97082021-4ceb-4951-8054-aeb8b332a3cf` source `234877441871bc84f97138c9f33d84a9246a3d71fb1596d3806c99ed9dd8827b`/同 ea944 已 succeeded/reaped；`.tmp/rust-xml-remediation-unit-01/report.json` XML lib 9 passed/0 failed/reaped。实际 JSON 只包含 CLI 测试 harness，没有新正常 CLI，当前封存正常入口仍是 eff5 的 6f86，不能声称 2348 正常 CLI 已验证。

`.tmp/rust-xml-remediation-sdk-01/report.json` 保留整批外层 watchdog 真失败：180000 ms、exit8、deadlineHit=true、forced owned tree termination、root/tree/handles 已回收。原序列前 13 个 case 已完成断言，第 14 个 empty_registration 准备中被终止；矩阵 failed，不将中间报告当作全部通过。16 个 cold public validation 原先合在一个 Rust test，误共用单次操作的外层截止。现在拆为 16 个独立命名 ignored tests，共用原完整 case/断言主体；`.tmp/rust-xml-sdk-matrix.mjs` 核对完整 listing，并按原每个 owned 进程 180000 ms 顺序执行，保持完整 API 输入、原业务 control/2GiB 和全部正负例/query/cursor 断言。没有增大单请求预算或减少案例。新矩阵尚待编译和实际执行。

### 默认 XML 查询与运行时 Identity 观察批次

共享 `SnapshotRequest → ResearchSession → research_relations` 已接入原 XML/TOC、统一 inline extractor/maps/program、各清单独立图与 exact XML self entry；materials schema v3 与研究/XML规则参与身份，缓存准入核对原始 XML/TOC 索引字节，既有跨 CLI/会话缓存包含原文。generated sites 投影回原 XML/UTF16/segments，context 继续读取原文 snippet。facts 索引的 inline 声明/关系/诊断也改用同一 decoded body/source-map，而非按 decoded 行直接平移，修复实体换行选择器；相关索引/文件缓存 revision 提高。保留完整 Lua 输入、原预算与 load issues，不猜 native UI 类型或执行事实。

新增内部 `lycheedev_live::observe_runtime(window, control)`：同一预算贯穿 OS module、指令/RIP 根、命名 Identity、客户端 description 与 DTO 解码。交叉核对 record runtime、identity.v3、本地 product/build/release 和 inventory，去除 root/state/opaque capability 地址，结果标为 observation_only；失败保留原因/阶段/覆盖/预算并释放 reader。未改变 passive 或旧 Target/slots 账本，未接旧 v4/光学输入；四槽生产推进器和认证接线仍待完成。

受控 producer `98b9fb4e-2e09-484f-98c7-591765003bb0` source `7bc1adf1994ac249dcbc0777b0a58993473c3a64fee01fa7f569b649382af9a0`/同 ea944 succeeded/reaped。正常 CLI/同次 PDB 已 sealed `.tmp/rust-xml-runtime-cli-01/artifact.json`，CLI `50a63dacd0754c96a8f9bc3791e9e23872381d220318a9c9f13edfe788e7035f`、PDB `87cbe71e6c727a131b981d82314309f1b02f2e72c5d3ff0b8987481dfd154d1b`。Live 100（含新增观察 8）、CLI 20、facts 5、xml_inline 4、research_material 2 均实际 passed/reaped，报告位于 `.tmp/rust-xml-runtime-{lib,facts,mapping,material}-01/`。

`.tmp/rust-xml-runtime-public-01/report.json` 保留新默认 XML CLI 首红及原 Lua session/flow 两项 passed/reaped。新例在对象 outgoing 投影失败：带 BOM 的结构索引错误地把 DemoFrame 放在 line1，选不出实际 line2 的 XML 对象。最小原 pin blob 复现证明共享 extractor tag span 正确；fixture 的工作区 CRLF 被 Git autocrlf 转成 pin LF，不能拿工作区 bytes 指控原 span。修复仅 facts/index/public fixture：结构行用 borrowed tag 原字节，空标签用真实 Event::Empty，错误位置换算原 bytes；fixture 固定 `* -text` 后保留真实 CRLF，并断言 Frame line2/start9，新增 BOM/LF/CRLF/缩进/空 handler 网格回归。诊断临时 rustc 链接旧库得到真实 RED，但未经过标准受控 producer，仅保留诊断范围；修复仍待统一构建验证。

`.tmp/rust-xml-sdk-independent-01/report.json` complete=true，原 16 个 cold XML SDK case 各独立 180000 ms、全部 passed/exit0/reaped，全部原 public valid/semantic/syntax、对象/self/字段版本/原文/query/cursor 断言保留；首整批 timeout 不改写。范围绑定 7bc。closure/validate 会消费 facts::xml，最新结构修复属于实际影响，下一批需在新源码继续完整 SDK 与首红公共测试，不转移旧通过为最新候选事实。

9708 copy-only `.tmp/rust-9708-next-tests-cache/copied.json` 保存当前必要 4 对/8 文件/237,299,712 B；exact retirement report 同目录已 complete，700 files/1,545,048,594 logical B，禁止重放。已核 eff5 Source/CLI/XML 三对 cache bytes 在 9708 重编译时被覆盖，限定 .tmp 搜索未发现副本；旧失败报告、日志、源 SHA 与 immutable 6f86 仍保持，不声称旧二进制保有。新的实际覆写前保存以必要当前消费者为限，未全量复制源码或全部历史缓存。

原 tag 修复受控 producer `3096ea44-57b2-4b63-ab8b-5eb2ab024d2e` source `ec82ea4a808426410b9af1dfb167bf52ea6149576f75e549a434dc7e3a2cf2a5`/同 ea944 succeeded/reaped。`.tmp/rust-xml-original-tag-facts-01/report.json` 6 passed，原首红 `.tmp/rust-xml-original-tag-public-01/report.json` 真实默认 CLI XML/缓存/跨 CLI/会话/失效/实体声明测试 passed，全部回收。正常 CLI/同次 PDB sealed `.tmp/rust-xml-original-tag-cli-01/artifact.json`，CLI `4e1c905ed929d2a1a3636235bdfa6c5a40c0da133975d69676097eb4ce2938e7`、PDB `a25166ac604dff650e3be3e4315f6c1a759fb8da99907020a52ba8acd87d4a77`。原首红报告与其消费者对保留；当前 `.tmp/rust-xml-sdk-independent-02/` 16-case矩阵仍在执行，不提前标通过。

完整 Retail 下一入口复用 `.tmp/rust-retail-controller-current.mjs → .tmp/rust-native-source-session-01/retail.mjs` 与原固定 pin/home/release。只读核实原输入 4036 文档（2551 Lua、1091 XML、394 TOC；32,269,051 B），新默认路径还要保留 4018 inline scripts/sourceMaps；观察器现在要求 sourceFiles=6569/maps=4018，不再仅断言 >=2000。旧09 peak为 1,910,099,968 **bytes**（约1.779 GiB），2 GiB limit保持；其 AuraUtil query旧图为空/缺[Family]，故不能当完整 XML loaded semantics 证据。当前 full-input 与加载 XML/多TOC query验证仍待实际执行，不增配额、不复制全数据。

98b9 copy-only report保存4对/8文件/233,135,104 B；exact retirement report完成703files/1,706,219,644 logical B，含已完整sealed的 normal EXE cache 副本。Live100、原session/flow消费者保留，所有immutable正常CLI/PDB/报告不变；禁止重放。

当前 `.tmp/rust-xml-sdk-independent-02/report.json` complete=true、16/16 passed/exit0/reaped，全部原案例/断言/API输入在 ec82 scope 下通过。随后完整 Retail `.tmp/rust-native-retail-controller-10/report.json` 真失败：producer3096、CLI4e1c前后hash稳定，outer exit1/root/tree/handles回收，deadlineHit=false/forced=false。冷请求 worker62560/current诊断返回 `source.native_invalid_input: source identity serialization byte budget`；peak334,557,184 B，原2GiB limit未变，不是OOM或观察超时。身份入口原上限65,536 bytes/8192 nodes/depth32，新增完整 XML/TOC path/hash 字典超出这个合同。当前修复方向是在完整原文/映射保留与认证不变的条件下，用canonical streaming SHA及规则/计数作compact身份，禁止升cap、剪输入或用通过的小fixture替代完整Retail。首失败stdout/stderr/phases/last-request保留；full input cold/warm/close与加载XML/多TOC尚未通过。

全量身份精简仅改 native_research.rs/xml.rs：两个 canonical JSON 流式 SHA覆盖全部原 XML/TOC path/content hashes 与全部 mapping manifests，各 hash header 包含 program/map rules；身份对象保存 digest/计数/规则/材料入口，完整原文、生成脚本和 sourceMaps仍保留。逐项 serializer 检查同一 OperationControl，无整组Value/Vec或serializedcopy，原65,536/8192/depth32/2GiB不变。真实1091XML/394TOC/4018maps规模测试证明旧身份超cap、新身份<2048 bytes，并检查原字节/路径、XML hash、单段map、规则变化失效和canonical摘要。受控 `dc52c59b-60f5-4130-b595-ab169d091fde` source `9f2813e64b198271cd1771df2846529a851ca34ca5dbc92906ba2b8d706b4405`/同ea944 succeeded/reaped；`.tmp/rust-xml-compact-identity-unit-01/report.json` 2 passed，原default XML公共用例同名public目录1 passed，全部回收。

当前 CLI/PDB已sealed `.tmp/rust-xml-compact-identity-cli-01/artifact.json`，CLI `76eba36e5da5ee14918586596fd1f6923bc9303f99d89e46f370612d53307686`、PDB `5278cc6d9b713df8466e7433529b0b634a584ddafaf991b6844f1d7a33d9d6fc`。完整 Retail controller11正在执行（Root exec47318，已确认CLI50712），当前不标通过或把旧last-request当新阶段。3096 copy-only保存三对6files159,225,344B，退休report703files1,706,184,461logicalB complete；SDK16/Live100/原session-flow及全部immutable产物保留，禁止重放。严格检查、加载XML/多TOC/fullquality与完整goal仍未完成。

完整 Retail controller11现已 terminal1/root/tree/handles回收，activeProcesses=0、deadlineHit=false、forced=false，正常CLI76eba前后hash稳定。实际当前worker65820诊断到 **querying**，error=null，peak_job_memory_used=2,096,656,384 B/原limit2,147,483,648 B；CLI返回 `source.native_session_unavailable`、Windows“拒绝访问。(0x80070005)”。身份65KB失败未再出现，但本轮仍failed，未执行warm/context/close成功断言；观察器finally已执行source session close，整棵owned进程终态回收。接近memory上限不是OOM证明，需从真实OS/IPC错误路径/子进程记录核实，不能把新失败归因旧09或模型推测。完整输入/原预算保持，Sourceworker只读诊断中，当前无Root活动producer/consumer。

### Retail 查询原错误与共享数据续批

诊断批仅改 native_session.rs/native_research.rs：read_frame 原失败保留，cleanup/worker退出观察分别记录；deadline/cancel主码保留原错误详情，原50ms/absolute deadline与unknown退出状态不变，无重发或重试。复用现有last-request增加query_inputs_renewed/build_scope/scope_built/build_entry/project/projected/assemble_reply/reply_assembled。受控 producer `00c8591f-fd72-4908-ad96-1bceef3f1f5b` source `9340ef167ba358406bf2d667602f6c0b50542304b4ce086f27dd8ea1d2812c47`/同ea944 succeeded/reaped；`.tmp/rust-source-retail-cause-unit-01/report.json` 真实截断帧+cleanup拒绝回归1 passed，public目录原真实session取消/deadline与XML两外层case passed/reaped。当前normalCLI/PDB sealed `.tmp/rust-source-retail-cause-cli-01/artifact.json`，CLI `d1eb9f9388ebd9cae72d811bddb02eb737e9080b725020b8fe05c231286ebb6a`、PDB `5cb77d5a653ade4c703b4d8ad11a27bcf703144c3340fdc86e43d22700d106e2`。

`.tmp/rust-native-retail-controller-12/report.json` terminal1/root/tree/handles回收、CLIhash稳定、无watchdog/forced cleanup。新实际worker62388诊断 **query_build_scope**，private commit/peak=2,096,107,520B/原2GiB；主error现在正确保留 `external.io: failed to fill whole buffer`，details清理失败ACCESS_DENIED、workerExitCode=null、退出50ms观察截止分别保留。不能由null退出/接近cap捏造OOM结论。明确已完成prepare，失败在构造scope前后的路径，未走成功warm/close断言。

独立只读核算Retail11实际原文/结构：Lua23,354,176B、analysis23,354,167B；XML/TOC8,914,875B；9930API声明JSON7,854,480B，实际4898globals+41methods；每套binding descriptors JSON3,048,984B/158,798Value/28,028object/7583array。这是结构/序列化量而非堆实测。现有Arc正文经to_string再进入Emmy Arc产生第二份；SourceInputs.clone深拷贝host/identity，graph admission的json!又拷贝。Dataworker已获授权完整sharing批：共享正文贯穿EmmyVFS、immutable admission/host descriptors/完整typed identity与GraphSnapshot共享，requestcontrol/steps/owner/executionview独立；完整JSON字段/原键序/hash与全输入/查询事实保持，原String入口和原budgets不变。不删API/不剪root scopes；当前实施中、尚未编译。Sourceworker只读审查兼容与owner/cursor隔离，严格检查/fullRetail/加载XML/multiTOC及完整goal保持未完成。

dc52 copy-only四对8files184,355,328B已核保存；exact retirement703files1,705,923,455logicalB complete，报告在 `.tmp/rust-dc52-native-session-retry-cache/`，禁止重放。当前Root无producer/consumer进程，不将之前CLI44280/worker62388当活跃等待。

### Source 共享批与 session 真实失败修复（2026-10-13）

共享正文、SourceCorpus/host descriptor 与完整 identity/Graph evidence 的批次已编译成功：producer `7e33a6c4-a699-452b-baf9-3ef95602c88f`，source `a39ebbf2b28d74a76afad804d87446a683ddecd9ba479aa4359e362c41908b14`，private Emmy patch `f5a8e176ab1ee00c8c37eb788b00ee59f9d751e49d5777a9e71d98368199faf0`。首 e57 实际 exit101 的三处字段引用已修复；辅助进程回收与编译错误分别记录，未称超时。normal CLI/PDB 已保全于 `.tmp/rust-source-shared-corpus-cli-01/artifact.json`，CLI `ae10eb9839802696ddfe86f54a114101a3e1bf9e0ad6a76d8aec57c95d2009aa`，PDB `2a6a524c388f08b92bb591280b05793068e8602522eba5b42475e7ad85335dd7`。e57 精确退役回执已 complete（626 files / 1,293,506,219 B），不得重放。

全 Source lib `.tmp/rust-source-shared-corpus-unit-01/report.json` 为 65 passed / 4 failed / 2 ignored，全部回收。两个新 sharing 用例在原 host callable admission 处失败，fixture 缺少 `sourceExecution:false`；仅补齐该字段，原生产校验与全部 JSON/共享/隔离断言保持。另两项旧 RED 在真实管道复现：session observer 将自身 deadline 错设为取消；expired business 的 typed reply 被已过期写控制拒绝成为 EOF。`session.rs` 的最小修复已冻结：只根据实际 EOF/peer input 判取消，business 与 reply 均在原 total/owner deadline 内，最多预留 min(500 ms, remaining/4) 回复时间，不重开业务；新增取消 typed reply 与过期 transport 不发字节两项真实管道回归。修复尚未编译或实际验证。

CLI `.tmp/rust-source-shared-corpus-cli-unit-02/report.json` 20 passed / reaped；unit-01 曾因错误产物名未启动测试，保留该启动失败。原公开 session/XML/flow 已分别 passed，后续 nominal/return/checker/semantic 与完整 SDK16 正在独立受控消费者中运行，最终结果见同名前缀 public-01/sdk-01 reports。这些记录不表示全 Source、完整 Retail、性能或最终候选已通过；旧失败不被覆盖。

上述 7e33 public-01 最终 7 项原公开/类型外层 case 全 passed/reaped，sdk-01 完整 16 案例全部 passed/reaped。随后两处 session 生产修复与 sharing fixture 补齐在 producer `1df49bc9-08b8-4432-a222-1c934120b0ee` / source `904cf4b6ce24efe1c516fa74f1a7c4a26c40f62eaad55de4d63e6079410e1bc0` / 同 f5 patch 编译成功并回收。`.tmp/rust-source-session-reply-unit-01/report.json` 全 Source lib **71 passed / 0 failed / 2 ignored**，CLI 20 passed；旧四红全部清零，包含三项共享正文/完整 JSON/执行身份/owner/control/VFS 回归和新增两项真实管道回复边界。两个独占原生 commit 资源测试继续 ignored，不视为已跑。`.tmp/rust-source-session-reply-public-01/report.json` 原公开 native session/cancel/deadline case 通过回收。

当前 normal CLI/PDB 保存 `.tmp/rust-source-session-reply-cli-01/artifact.json`，CLI `989b05f2b4495d0fdcb24169f01042567c73dd5eb31929026429e60656a18e8e`、PDB `59f625c71e31a3dd6832d38cb4a23a601284633fe4aa4d7c80582575a886ae8a`。7e33 当前 10 对 / 20 files / 592,318,464 B 测试字节在覆盖前已保存；随后精确 698 files / 1,753,874,875 B retirement complete，不可重放，全部 f5 Emmy 与旧 positive/immutable 原件保留。原完整输入、2 GiB/时间/响应限制不变；完整 Retail controller13 正在此 1df 候选上执行，尚未计为通过。加载 XML/多 TOC runner 已对齐实际 pin 路径/行号和 Source 输出合同，未执行；完整 Source、当前 strict、公平性能及其余目标仍未完成。

### 完整 Retail 第 13 轮：回复组装与重复事实解析

`.tmp/rust-native-retail-controller-13/report.json` 已 terminal1 / root/tree/handles 回收，当前 CLI 989b 前后 hash 稳定，deadlineHit=false / forced=false。新 worker44548 越过此前失败的 `query_build_scope`，实际诊断到 **query_assemble_reply**（59,345 ms），private commit 2,091,376,640 B、peak 2,109,304,832 B，原 2,147,483,648 B 限额保持。主错误 `external.io: failed to fill whole buffer` 保留，cleanup ACCESS_DENIED 与未知 exit 分开记录；本轮不证明 OOM，也未执行 warm/close 成功断言。controller finally 已收尾，CIM 确认 worker/CLI/controller 均退出。

明确存在的复制是 `query_observed` 的最终 json! 再序列化 owned rows/scopes/gaps/entry_facts，以及已生成的完整 `Vec<Value>` sourceMaps；正在用 owned Value/Array 移交保持全部字段/字节语义，禁止减少 4018 映射、输入或查询事实。另本轮 profile `source.snapshot.load_facts=52,028 ms`，source_blobs=466 ms。只读源码确认 load_facts 又做一次 tree/batch blobs，1091 XML 每次完整事实解析（含 schema 与全部 inline Lua），随后只使用 loads；还绕开现有 v6 完整事实缓存并逐项发布/校验 1485 CAS 原文。没有分段实测，不把 52 秒全部归因任何单项。下一批复用既有认证事实缓存及原控制/原文校验，不削事实、不提升预算。新修复尚未编译，完整 Retail/加载 XML/多 TOC/性能仍未通过。

### 最大代码新鲜窗口失败：实际原型修复范围

原P3 MAX_CODE两/四槽observation_expired（约6135/6204ms）保持。新的最小实际Lua51 `commands.decode` 循环 `.tmp/rust-native-crypto-original-01/report.json` 在完整262144代码/524322 wire上SHA正确但CPU5985ms；SHA-only6081ms定位主体为32bit纯Lua热点，不是减少载荷或延长750ms窗口。crypto.lua现在可使用捕获的实际native bit word操作，原arithmetic fallback与SHA/HMAC caps保留；signed word直接进入同位运算或模2^32加法，不逐轮多余归一化。实际native循环336ms，14组独立Node SHA/HMAC golden在portable/native两端通过（高位、二进制、padding、长key和原limits），各进程回收。

`.tmp/rust-native-crypto-lifecycle-01/report.json` 使用原封存且五个Rust协议源SHA不变的历史compiled driver395554…，运行当前Lua receiver与有SHA身份的实际Lua51/native bit provider。完整原positive/fault矩阵及MAX_CODE/MAX_BODY通过，two/four最大代码完整生命周期558/549ms，codeCalls/closeEffects/bodyClears各1。只证明该原型/provider范围；arithmetic backend的大载荷性能仍失败，不把它隐去；finalProtocolPassed=false，P1/cold binding、clock/B1、500 allocator、真实OS carrier/审计/产品/实机仍未通过。正式原型runner新增显式--native-bit并记录provider/initializer身份，未引入产品DLL依赖或降低载荷/窗口。当前联合源码对应的新driver尚待编译/执行。

### 回复移动与加载缓存已验证，完整 Retail 仍有语义缺口

producer `65bbe0cd-0aad-49d4-8da7-2844729ea161` / source `031cadb5a78e983160c18be2aa50382e35049f11196fd47175d31e08992f7aa1` / 同 f5 Emmy patch 编译成功并回收。`.tmp/rust-source-reply-move-unit-01/report.json` Source **74 passed / 0 failed / 3 ignored**、CLI 20 passed；公开 XML/session/flow 三项 `.tmp/rust-source-reply-move-public-01/report.json` 全 passed/reaped。显式执行的 full-map 测试 `.tmp/rust-source-reply-move-full-maps-01/report.json` 保留完整 4018 maps / 12041 segments，旧 eager 与 move JSON 字节完全一致；实际 compact JSON 3,834,525 B、172,656 nodes、44,159 objects。这是编码/结构计数，不是 Rust 堆量测或 OOM 证明。另两个独占 native quota 测试仍未执行。normal CLI/PDB 保存于 `.tmp/rust-source-reply-move-cli-01/artifact.json`，CLI `31219747c4c52d137079ecb02e59fe0f7a734c6f31a00f0cccf040769d0b1308`、PDB `a8c73f875df3e2d7965a448cf4823fb01af8020324f4579ec0a6311e4023e7c3`。

完整 Retail14 `.tmp/rust-native-retail-controller-14/report.json` terminal1/reaped，无 watchdog/forced cleanup；cold CLI 实际 exit0、全部 maps 返回，EOF 未复现。load_facts=4,041 ms，相比第13轮52,028 ms，未改输入、原 CAS 或限额。worker35052 到 replying，peak 2,119,921,664 B/原2GiB。验证仍失败：所有 graph scope 因 object mutation budget exceeded 被丢弃，analysis 缺少 sourceFiles，loadScopes 为空；另三个 [Family] load_file_missing。不得将 native 冷回复成功等同完整查询/缓存/性能通过。

下一批修复分别处理真实原因：已记录预算 boundary 后同一 observation_failure 被 host_finish 再抛，需只消耗已转交 latch 并保留 partial 前缀；每次未知调用遍历 namespace 的重复无变化版本，需保留完整逐调用 footprint/source/context/proof，再复用语义未变对象版本，中途字段写必须再次失效，原4096上限不变。这是 unknownEffectsProfile 证据表示升级，不声称旧日志同构。Family/Game/TOC 条件使用共享加载上下文；固定源码证明 Mainline standard 的 [Game] 为 Standard，不能猜为 Mainline，显式替代 TOC 独立；raw LoadFact/CAS/v6缓存保留，未知条件显式 gap。casefold路径索引与sources表按admission共享，避免每次引用扫描全表。此批尚未编译/实际验证。

完整 Retail runner 增加实际 loadScopes、graphControl 和 selected entryEvidence 检查，防止仅 syntax fallback 或预算 prefix 被计为完整分析。全量源、4018映射、原2GiB/时间/响应限额及后续 loaded XML/multiTOC 验收保持。全目标仍 active/incomplete。

### Family/加载上下文与完整调用影响证据批

producer `8d305159-840f-4fbe-8849-a3aa675efbae` / source `faa5c6194d77b08c37d04ef64c3a5532c439350d5f0c7495fb888b0370d105b5` / 同 f5 Emmy patch 已编译成功并回收。原七文件批保持全部输入、预算和旧 raw LoadFact/CAS；Program/parent walk_load 共用加载上下文、条件和一次构建的 casefold 路径索引，worker 所有 Program 共享原始 sources 表。固定源码支持的 Mainline standard `[Game]` 为 Standard；未知 subtype/mode/环境/locale 不猜。替代 TOC 的 family/mode/假设进入输出与身份，xml_load/research rules 已更新。

unknownEffectsProfile v3 保留每次未知调用的完整 footprint/source/context/观察前后/proof，仅复用没有字段后写的同一 unknown 状态；普通 unknown_object、checkpoint/restore/join 与 4096 真变化边界保持。`.tmp/rust-source-family-effects-unit-01/report.json` Source **82 passed / 0 failed / 4 ignored**、CLI 20 passed/reaped，包括五项真实调用影响/独立重放回归。`.tmp/rust-source-family-fixed-retail-01/report.json` 显式执行的固定原文闭包测试 passed/reaped：FrameXMLUtil 与 DeprecatedSpecialization 正确展开条件/Family/Standard，两个加载入口全部 Lua occurrences 顺序一致，原 AuraUtil 第15行/1944B/CAS SHA 保持。两个独占 quota 测试及未在此批重跑的完整 map ignored 范围仍不冒充通过。

normal CLI/PDB 保存在 `.tmp/rust-source-family-effects-cli-01/artifact.json`，CLI `8f0c3917ff4e8ea9f753b3107988e5e4b0094b667cc8bb2168e87286db263d6d`、PDB `8db685df5b91a7f3bd3ea3f0895ff00063897ad1fdb2fcf28206d52eb34879c8`。完整 SDK16 与公开 XML/session/flow 正在执行，尚未计为全通过。65bb 覆盖前20文件593,602,048B保全及698文件1,754,721,680B精确退役回执均 COMPLETE，禁止重放。完整 Retail15、加载 XML/multiTOC、current strict/完整质量/性能及其他目标仍未完成。

上述 sdk-01 最终 complete=true/16原案例全部 passed/reaped，public-01 原 XML/session/flow 三项全部 passed/reaped。随后 `.tmp/rust-native-retail-controller-15/report.json` 实际失败：terminal1/allreaped、binary前后稳定、deadlineHit=false/forced=false；worker59588 到 **query_project**（58,430ms，peak2,123,919,360B/原2GiB），cold native_analyze165,806ms 耗尽原调用总 deadline，主码 command.deadline，原 broker.deadline 单独保留，ownerRetired=true。没有完成 warm/context/close 成功断言；不据此捏造 OOM，也不增加时限。

只读定位明确的投影成本：每条语法 reference 在比较 mapped path 之前，origins.within 经 utf16_range 对两个 offset 分别从文件头扫描；6559 个源文件中无关普通 Lua 也承担该成本。Function target 对同函数反复做完整位置扫描，Object target 反复遍历全部 writes。修复方向为保持 owners 优先和精确映射/UTF16，用保守路径门过滤不可能的 outgoing 文件/位置，并每 graph/symbol 计算一次完整目标值集合；incoming/both 全遍历和所有输入/事实/顺序继续保留。新 CPU 修复尚未编译/运行，原失败记录不被覆盖。

### 2026-10-13 单代理续作：Source 投影与执行状态计数

投影优化已完成，旧 eager 路径与新路径在四类目标 × incoming/outgoing/both 下 JSON 一致，包含 BOM、CRLF、Unicode、跨 XML 继承和未加载文件引用。producer `3c11474c-bc19-434a-a02d-d3e4cd2aef82` 的 `.tmp/rust-source-projection-unit-01/report.json` 为 Source83/CLI20 passed，public-01 原 XML/session/flow 三项通过。Retail16 已越过投影超时，cold native_analyze 98,524ms，但入口仍因 `native checkpoint slot budget exceeded` 未执行；整体失败。

快照改为共享未变对象、保存 cell delta，完整序列化/恢复和 4096 存储槽上限保留。合流将严格互补的谓词化简，原分支边与合流输入仍保留；新增真值表、状态重放及真实存储耗尽测试。Retail17（producer43ed20f0）越过快照限制后，在 ItemUtil.lua 第257个函数声明处碰到原型 closure 总量上限，整体仍失败。中途进度曾误把 cold exit0 说成通过，随后已纠正；以完整控制器断言为准。

研究模式现在区分源码声明/静态模板与动态实例：声明受既有输入、AST、公共图和总工作预算约束；函数调用、factory/重复创建仍使用原256实例/帧和捕获预算。producer `5eef51c0-42e2-47e0-8048-0a40015fa347` 的 `.tmp/rust-source-checkpoint-unit-03/report.json` 为 Source88/CLI20 passed，public-01 原三项通过。Retail18 又在公共读写关系投影的原型4096计数处失败；不计为完成。

`57b3de51-8af5-41a1-b203-c03cb261eedf` 完成公共图派生关系与新增调用推导的计数分离；输入/AST/工作、2GiB worker、180秒 worker / 190秒 CLI、32MiB响应均保持。source tree `b7d8341d4e11fecd771566951811c248b3da0e8b844f0335c78d02d20ca864ba`，Emmy patch `f5a8e176ab1ee00c8c37eb788b00ee59f9d751e49d5777a9e71d98368199faf0`。`.tmp/rust-source-checkpoint-unit-04/report.json` Source89/CLI20 passed，4 ignored 仍未运行；正常 CLI/PDB 位于 `.tmp/rust-source-checkpoint-cli-04/artifact.json`。Retail19 正在运行，尚无通过结论。此前失败、产物及其原受测范围保留，不转移为当前候选的验收。

全量 Retail 连续查询、加载 XML/多TOC、Source 最终质量/稳定性/性能、Live最终协议生产接线、Data剩余业务、最终候选及五客户端验收仍未完成。未启动子代理、未操作游戏、未提交或发布。
