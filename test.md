# 测试用途与当前回归

业务和架构以 [DESIGN.md](DESIGN.md) 为准，待做事项在 [todo.md](todo.md)，实际运行结论在 [handover.md](handover.md)。本文件解释测试为什么存在、调用什么、证明什么、哪些待改接或待补，不是运行日志。

本批已完成旧实现测试、冻结对账套件和旧文档账本的删除（见 handover）；下表只列当前工作树存在的测试入口。**“保留”表示用途属于当前产品，不表示当前工作树已全部编译或通过**；实际运行结果只记在 handover。

## 1. 取舍与证明范围

| 标记 | 含义 |
| --- | --- |
| 保留 | 验证当前业务、当前格式或真实外部边界；去掉文件内仅约束旧实现的断言。 |
| 改接 | 有效业务仍需要，但 runner、fixture 来源、命名或调用入口仍依赖历史路径；接到当前正式产品后才能纳入回归。 |
| 辅助 | fixture 生成、子进程 peer、测量/比较工具；通过只证明辅助逻辑，不能计为产品业务通过。 |
| 待补 | 当前架构需要但尚未有对应回归，见第 7 节。 |

判断依据是实际调用路径。名字带 SQL/Go 的 Rust 代码可能是当前 Mailbox 协议或正式查询路径；反之文件名带 Rust 也不能证明它没调用旧执行器。旧测试的业务预期可以重述为当前入口回归，但不复制旧实现、不保留旧源码 oracle、不用 cfg(test) 恢复生产中已删除的路径。mock/静态检查、自有窗口、固定文件和真实客户端分别说明，不能互相冒充。

## 2. Rust 模块内测试

随代码保留，按当前职责筛选；不要为了保持历史断言把已退役模块重新接回来。

| 模块 | 用途与主要分组 | 能证明的范围 |
| --- | --- | --- |
| [CLI](crates/lycheedev-cli/src/) | contract/agent_cli/discovery：解析、发现、schema/help/describe、产品称呼及commands.json声明的游戏进程/输入效应，包含统一doctor的--fix条件与--hard-reset依赖；data_agent/transport：帧、取消通道与保留提交事实的标准流输出；formal_live：原连接与输入编排；init/project/target/doctor：工作区、选择与诊断。 | 入口合同与应用编排；mock 不证明原生进程、游戏或已安装包。 |
| [Core](crates/lycheedev-core/src/) | 身份/选择、control（不可变 deadline + TrackedCancellation 世系）、公共精确 JSON、存储发布、CAP/依赖、缓存与 Data 计量、当前 Mailbox 数据合同。进程回归用自有根进程与无stdio后代核同一Job、原期限及checked回收；不以根退出代替后代回收。私有预声明发布复用已认证摘要的回归覆盖新发、复用、同长损坏正文、长度与maximum。 | 共享机制；旧 schema 迁移、historical reader 与旧工作区布局断言已删除。 |
| [Data](crates/lycheedev-data/src/) | Agent Session/Engine/DataFusion、generation、元数据/DBD、CASC/BLTE/WDC、Hotfix、assets、keys、remote 与资源占用。HTTP body/gzip/取消/收费测试直接走正式collect_admitted，不保留仅测试消费的两层异步包装。 | 当前查询、输入和生命周期；旧 local_query/relational/navigate/migration-oracle 测试随旧实现删除；旧 Engine 固定快照/provider 资格层（engine/source、snapshot_cache、provider Fixed*）及内部测试已随旧实现退役，当前路径为 source_demand/session_engine/pull_provider/provider::runtime。 |
| [Source](crates/lycheedev-source/src/) | 原始字节与 Facts/Index、编码告警（N11 已覆盖）、TOC/XML、catalog/environment、luac、native 会话、查询/引用/关系/差异、固定源码与缓存。XML Facts覆盖Script与handler的声明、关系、实体换行原始位置及冷/热缓存一致性。 | 当前源码调查；不保留 LuaLS、旧 evaluator/JSON cache 合同。 |
| [Emmy adapter](crates/lycheedev-emmy-adapter/src/) | config/projection/session 与上下文适配；selected initializer回归覆盖参数/回调数量、返回控制流，以及Lua非尾标量、尾多返回、括号、DocTuple、开放必需前缀、Union包、Never和环境API返回槽。返回type/count与signature复用同一包形状规则，保留nominal carrier。 | 自有适配合同与实际比较的coverage归属；不替代实际 Release Source 任务，也不修改上游语义引擎。 |
| [Live](crates/lycheedev-live/src/) | mailbox（含 lookup 读路径）、passive、probe、runtime_root/runtime_binding 与 SessionOwner 恢复。 | 当前协议、读取和状态；合成内存不证明实际客户端 layout。 |
| [Delivery](crates/lycheedev-delivery/src/) | 获取、受管 addon/槽事务、安装更新、移除与 npm 交接。release/checked_close_tests将当前release/npm-channel.json原字节送入release与update两条入口，核相同渠道合同、其他合法preserveLatest、畸形字段及原runtime/tag/version负例。 | 当前交付事务；CLI 冻结安装/更新选择与 launcher 环境，库测试直接构造 RequestContext/Request；原维护租约与恢复仍在 Delivery。 |
| [Windows](crates/lycheedev-windows/src/) | 进程、Job、IPC、文件、内存、取消和输入句柄。 | 操作系统边界；不把这些测试计为游戏验证。 |

Data新增业务见[navigation_business_tests.rs](crates/lycheedev-data/src/agent_session/navigation_business_tests.rs)：经Session执行并核自动CAP的完整法术闭包/光环/召唤、遭遇树、生物显示与资源、浮点字段；用真正partial表与完整空表区分unavailable和not_found。旧SQL前端测试的有效数值断言迁到[typed plan](crates/lycheedev-data/src/engine/plan/tests.rs)及[值边界](crates/lycheedev-data/src/engine/values.rs)，覆盖精确SUM/AVG、负零、除零、分组、双分区状态合并、Decimal schema、HashJoin和COUNT，不保留仅SQL消费的planner胶水或引擎选型对照。

[schema_tests.rs](crates/lycheedev-data/src/agent_session/schema_tests.rs)核公开wire schema覆盖全部typed Task、真实serde必填/默认/枚举和可解析引用；公开示例经过Session执行查询、分页、CSV、CAP和释放。页大小边界及释放后的句柄失效由实际业务拒绝证明，不只比较CLI嵌入的schema是否相等。doctor无效的no-cache兼容参数已移除，CLI测试核参数拒绝。

[status_control_tests.rs](crates/lycheedev-delivery/src/status_control_tests.rs)走正式status dispatch，observer只停止原control或在实际close后注入错误；真实Windows禁止data-read句柄区分读前大小检查与事后拒绝。取消/期限、IO/checked-close错误不得降格为modified/incomplete成功。

补审回归沿当前正式入口：Data的[Session测试](crates/lycheedev-data/src/agent_session/tests.rs)核Semantics只许可static/空captures且先于元数据读取，以及真实partial Aura来源不能产生否定事实；保完整空来源与已观察正事实对照。CLI的[source选择](crates/lycheedev-cli/src/source_cli_tests.rs)、[Hotfix选择](crates/lycheedev-cli/src/hotfix_remote_tests.rs)、[发现合同](crates/lycheedev-cli/src/discovery.rs)和[Data admission](crates/lycheedev-cli/src/data_cli.rs)分别核固定项目/最近锁/显式snapshot优先、实际输出字段、命令路径语义和显式jsonl；不启动真实游戏。

Source的[batch](crates/lycheedev-source/src/batch_tests.rs)拒绝重复/未知字段、损坏UTF-8、null操作和越界参数；[agent_cli](crates/lycheedev-cli/src/agent_cli.rs)把公开batch/matrix文件示例交真实parser，并核默认值、范围和引用。[asset头](crates/lycheedev-source/src/assets.rs)核PNG/BLP头的维度观察，不充当像素解码验证。native research的既有caller/context/observation测试直接走生产observed/Serialize路径；已移除无消费者的no-observer包装、重复自比和转换指针断言，保原独立预算、XML、foreign graph与取消预期。

[Source会话](crates/lycheedev-source/src/native_session.rs)的初始Hello测试使用实际dispatch和自有pipe核原取消/期限与正常EOF；观察线程测试区分精确scope stop、独立I/O/checked-close、活跃与已完成请求，真实Windows marker读取另由broker测试覆盖。CLI环境/diff选择与Source sync继续同一原control；Core默认fixture入口共用现有受控PIN/CAP读写，不保留另一条未检查关闭的执行路径。[NativeReader](crates/lycheedev-live/src/mailbox/native_reader_tests.rs)用同一真实ledger和独立child停止核record分配失败同时保留额度与取消原因；不读取游戏。

[回复组合](crates/lycheedev-source/src/native_session/raw_response_tests.rs)把真实typed child错误帧、原analysis退休错误和observer错误交同一生产收尾函数，再经正式帧writer/consumer核Core cleanup结构与全部原因；单错精确字节、原Vec地址/容量、任意精确数值及默认栈限制分别保留。真实线程panic用例核回包尝试仍fatal退出，取消/期限/写入失败不恢复warm owner；组合帧超限必须零字节发布。

Windows新回归在真实一次关闭之后注入结果，保启动/工作原因与关闭原因：[Process查询](crates/lycheedev-windows/src/process/checked_query_tests.rs)、[broker](crates/lycheedev-windows/src/broker.rs)、[detached](crates/lycheedev-windows/src/detached_close_tests.rs)、[Job](crates/lycheedev-windows/src/job_close_tests.rs)。[keyboard event](crates/lycheedev-windows/src/mailbox_input/keyboard_activity.rs)的event_only分组只使用自有event/线程，不装全局hook或发送输入；默认helper空跑不能计为业务通过。[destination](crates/lycheedev-windows/tests/destination.rs)的文件、取消、替换、race和parent/reparse断言已迁当前prepare_controlled/publish_reader_controlled，旧发布执行器退役。

Delivery的[npm回归](crates/lycheedev-delivery/src/npm_update.rs)覆盖预取消准备、受控流式SRI、已更新后的journal保存失败及子命令原结构化错误。Live的[Driver回归](crates/lycheedev-live/src/mailbox/driver_tests.rs)使用现有合成backend/journal核input_observed保存失败与传输/取消双错、原意图和恢复不重放；doctor的typed公共原因与旧协议修复许可分别验证。这些离线用例不扩大为实机通过。

新增[默认更新发现](crates/lycheedev-delivery/src/update/tests.rs)用真实metadata核原停止、target前缀/来源过滤、key顺序及原10001行总限额；[doctor归档](crates/lycheedev-live/src/mailbox/doctor_repair_tests.rs)核普通主addon升级在Delivery错误和等待手动reload时都保实际选定路径；[正式安装](crates/lycheedev-delivery/src/formal_repair/tests.rs)用合成client/release与真实deny-delete核main及500槽已经验证完成时，清理错误仍附原Receipt。以上列的是用途，实际红/绿状态以handover当前结果为准。

## 3. Rust 独立集成目标

执行入口为构建阶段生成的对应测试程序；依赖真实 CLI、luac、固定源码或安装的目标必须明确提供这些输入。含 ignored 的文件不能整份记为通过；准备 helper、被父测试调用的 peer 和业务测试分开记录。

### CLI：用户实际调用链

| 入口 | 验证的业务或边界 | 处置 |
| --- | --- | --- |
| [data_agent_public_numbers.rs](crates/lycheedev-cli/tests/data_agent_public_numbers.rs) | 实际Agent查询、精确数值、空数组、Page、JSON/CSV、CAP和Explain/Analyze。 | 保留 |
| [data_diagnostics_public.rs](crates/lycheedev-cli/tests/data_diagnostics_public.rs) / [normal_exit_public.rs](crates/lycheedev-cli/tests/normal_exit_public.rs) | 真实 CLI 的无 home 发现、参数/帧错误、错误输出、退出码与 EOF。 | 保留 |
| [data_agent_supervisor.rs](crates/lycheedev-cli/tests/data_agent_supervisor.rs) | 真实 supervisor：EOF 排空、背压、排队 deadline、取消 epoch、不重放、断管保留已提交 CAP。 | 保留 |
| [data_cache_prune_public.rs](crates/lycheedev-cli/tests/data_cache_prune_public.rs) | 公共 prune 的显式 Data 范围，保留其他对象。 | 保留 |
| [source_inspect_public.rs](crates/lycheedev-cli/tests/source_inspect_public.rs) / [source_flow_public.rs](crates/lycheedev-cli/tests/source_flow_public.rs) | 正式 inspect、项目上下文、文档/资源/片段、flow 和 CAP；N11 的编码告警/显示断言在 source crate（[source_inspect.rs](crates/lycheedev-source/tests/source_inspect.rs)），此入口验证公共 CLI 合同。 | 保留 |
| [source_native_session_public.rs](crates/lycheedev-cli/tests/source_native_session_public.rs) / [source_native_xml_public.rs](crates/lycheedev-cli/tests/source_native_xml_public.rs) | 实际 native 会话的固定快照复用/关闭，XML 原位置、manifest、缓存与公共 CAP；同 root/home 下 Data 实际断管故障后 Source owner/analysis 复用，Live 原 CON/request/report/budget 与 execution lease 保持。Live 此处仅为正式持久化 API 的合成输入，不涉及游戏。 | 保留 |

### Data：输入、查询与输出

| 入口 | 验证的业务或边界 | 处置 |
| --- | --- | --- |
| [agent_current_local.rs](crates/lycheedev-data/tests/agent_current_local.rs)、[local_domain.rs](crates/lycheedev-data/tests/local_domain.rs) | 实际安装的固定数据进入生产 Agent Session/DataFusion；领域报告、缺钥/损坏与安装选择。 | 保留；需真实安装/独立预期 |
| [installed_inputs.rs](crates/lycheedev-data/tests/installed_inputs.rs)、[casc_inputs.rs](crates/lycheedev-data/tests/casc_inputs.rs)、[encoded_ranges.rs](crates/lycheedev-data/tests/encoded_ranges.rs) | 当前安装产品/build/config/Root/Encoding 身份、CASC 范围读取、未访问块与缺钥覆盖。 | 保留 |
| [table/decode_tests.rs](crates/lycheedev-data/src/table/decode_tests.rs)、[table/palette_tests.rs](crates/lycheedev-data/src/table/palette_tests.rs) | 原始WDC header、palette、copy ID、关系、整数/浮点/text/array边界，直接走当前leased-row decoder；Engine投影另核实际访问完整物理行及partial。 | 模块内保留 |
| [hotfix_tables.rs](crates/lycheedev-data/tests/hotfix_tables.rs)、[hotfix_scan.rs](crates/lycheedev-data/tests/hotfix_scan.rs) | Hotfix 字段/数组/关系/Unicode、物理行状态、不可变分页/筛选、证据与引用释放。 | 保留 |
| [keys/input_tests.rs](crates/lycheedev-data/src/keys/input_tests.rs)、[streaming/source_tests.rs](crates/lycheedev-data/src/decoded_cache/streaming/source_tests.rs)、[content_budget.rs](crates/lycheedev-data/tests/content_budget.rs) | 原始密钥与缺钥、nested BLTE、CASC选定索引/extent、流式解码/缓存认证/checked-close；真实asset入口冷暖额度和取消。 | 当前scoped/streaming路径 |
| [assets_business.rs](crates/lycheedev-data/tests/assets_business.rs)、[image_export_business.rs](crates/lycheedev-data/tests/image_export_business.rs) | 资源搜索、raw/demux/image 导出、像素/通道、CAP、已有目标冲突和文件发布。 | 保留 |
| [agent_fixture_import.rs](crates/lycheedev-data/tests/agent_fixture_import.rs)、[agent_fixture_prepare.rs](crates/lycheedev-data/tests/agent_fixture_prepare.rs) | 正式包/Agent控制器使用的独立输入导入与三表/16,384行精确数值WDC构造；准备不等于查询通过。 | 辅助 |

### Source：固定源码与实际 native 分析

| 入口 | 验证的业务或边界 | 处置 |
| --- | --- | --- |
| [source_inspect.rs](crates/lycheedev-source/tests/source_inspect.rs)、[refs_context.rs](crates/lycheedev-source/tests/refs_context.rs) | 文档/资源/符号 inspect，引用上下文、顺序与 cursor；N11 编码显示与通知入口。 | 保留/补 N11 |
| [environment_catalog.rs](crates/lycheedev-source/tests/environment_catalog.rs)、[environment_owners.rs](crates/lycheedev-source/tests/environment_owners.rs)、[flow_rules.rs](crates/lycheedev-source/tests/flow_rules.rs) | 客户端 API/secret 条件、catalog 位置/归属、对象方法与全局规则边界。 | 保留 |
| [environment_real.rs](crates/lycheedev-source/tests/environment_real.rs)、[catalog_native_real.rs](crates/lycheedev-source/tests/catalog_native_real.rs) | 固定客户端 catalog、对象方法与 callback 实际进入 native checker。 | 保留 |
| [luac_real.rs](crates/lycheedev-source/tests/luac_real.rs)、[luac_inline_real.rs](crates/lycheedev-source/tests/luac_inline_real.rs)、[luac_ordered_real.rs](crates/lycheedev-source/tests/luac_ordered_real.rs) | 真实 Lua 5.1 语法、Unicode/goto 标识符、内联原位置、加载顺序与编译子进程回收。 | 保留 |
| [semantic_validate_real.rs](crates/lycheedev-source/tests/semantic_validate_real.rs)、[validate_contract_real.rs](crates/lycheedev-source/tests/validate_contract_real.rs) | 公开 validate 的 catalog/luac/native 语义、inline 位置和 single/matrix 覆盖，不把未分析当完整。 | 保留 |
| [loads_real.rs](crates/lycheedev-source/tests/loads_real.rs)、[native_xml_real.rs](crates/lycheedev-source/tests/native_xml_real.rs) | 固定加载闭包、XML 与当前 index/native 的关联，热索引身份和篡改错误。 | 保留 |
| [native_nominal_real.rs](crates/lycheedev-source/tests/native_nominal_real.rs)、[native_checker_families_real.rs](crates/lycheedev-source/tests/native_checker_families_real.rs)、[native_return_count_real.rs](crates/lycheedev-source/tests/native_return_count_real.rs) | 实际native的对象类型、方法、参数/返回基数和控制流；调用许可与返回观察独立，未知返回保留已知参数及返回前缀，条件调用和未知基数不伪报完成。[invocation.rs](crates/lycheedev-source/src/native/invocation.rs)中的vararg回归经真实Builder/Entry核非尾与括号单值、精确空/单/多值展开、固定参数/self偏移、赋值补nil及未知尾partial。 | 保留 |
| [matrix_config.rs](crates/lycheedev-source/tests/matrix_config.rs)、[matrix_real.rs](crates/lycheedev-source/tests/matrix_real.rs)、[matrix_four_real.rs](crates/lycheedev-source/tests/matrix_four_real.rs) | 严格的 matrix 输入及多个固定产品各自的分析/失败/CAP；不合并客户端身份。 | 保留 |
| [diff_prune.rs](crates/lycheedev-source/tests/diff_prune.rs) | 声明/签名/位置差异、同提交无变化、未完整分析的缺失语义，以及活动源码/证据保护和可回收对象。 | 保留 |

### Emmy adapter 与共享类型

这些目标直接验证当前适配接口。涉及自有表达式编排时必须仍以原版 Emmy 为语义来源，不能以测试存在为理由保留第二套 evaluator。

| 入口 | 验证的业务或边界 | 处置 |
| --- | --- | --- |
| [contracts.rs](crates/lycheedev-binding-contracts/tests/contracts.rs) | 默认值、元数据、返回值/receiver 类型、absent/unavailable 与对象上下文身份。 | 保留 |
| [public_ingestion_contract.rs](crates/lycheedev-emmy-adapter/tests/public_ingestion_contract.rs)、[declaration_ingestion_contract.rs](crates/lycheedev-emmy-adapter/tests/declaration_ingestion_contract.rs)、[declaration_contract.rs](crates/lycheedev-emmy-adapter/tests/declaration_contract.rs)、[declaration_rendering_contract.rs](crates/lycheedev-emmy-adapter/tests/declaration_rendering_contract.rs)、[callback_contract.rs](crates/lycheedev-emmy-adapter/tests/callback_contract.rs) | 输入字节/文本、Unicode 位置、批次增删、取消、声明载入与 unknown/coverage。 | 保留 |
| [lychee_context.rs](crates/lycheedev-emmy-adapter/tests/lychee_context.rs)、[lychee_checker_guards.rs](crates/lycheedev-emmy-adapter/tests/lychee_checker_guards.rs)、[lychee_cardinality.rs](crates/lycheedev-emmy-adapter/tests/lychee_cardinality.rs) | 上下文隔离、未知值与可空/非空callback签名精度；colon/dot/保存方法别名的receiver位置及proof，显式typed self和普通receiver在零/非零显式参数及显式泛型number/table时均核类型与精确范围；固定参数尾包逐槽检查、开放尾的已知前缀与partial、非尾/括号标量化。固定输入使用tests/fixtures/source-native。 | 保留 |
| [selected_query_contract.rs](crates/lycheedev-emmy-adapter/tests/selected_query_contract.rs)、[selected_projector_contract.rs](crates/lycheedev-emmy-adapter/tests/selected_projector_contract.rs)、[query_backend_contract.rs](crates/lycheedev-emmy-adapter/tests/query_backend_contract.rs) | 当前查询与投影、值来源、循环/取消、复用及完成范围。 | 保留 |
| [object_member_contract.rs](crates/lycheedev-emmy-adapter/tests/object_member_contract.rs)、[object_carrier_contract.rs](crates/lycheedev-emmy-adapter/tests/object_carrier_contract.rs)、[dense_tuple_contract.rs](crates/lycheedev-emmy-adapter/tests/dense_tuple_contract.rs) | 对象身份/成员顺序、嵌套值、数组/tuple 与缺失项。 | 保留 |
| [function_call_contract.rs](crates/lycheedev-emmy-adapter/tests/function_call_contract.rs)、[expression_plan_contract.rs](crates/lycheedev-emmy-adapter/tests/expression_plan_contract.rs)、[expression_execution_contract.rs](crates/lycheedev-emmy-adapter/tests/expression_execution_contract.rs)、[binary_core_contract.rs](crates/lycheedev-emmy-adapter/tests/binary_core_contract.rs) | 调用、表达式顺序、二元运算、短路/未知值和控制中断。 | 保留 |

### Core、Delivery、Live 与 Windows

| 入口 | 验证的业务或边界 | 处置 |
| --- | --- | --- |
| [mailbox_contract.rs](crates/lycheedev-core/tests/mailbox_contract.rs) | 当前Mailbox JSON、Unicode/code bytes和精确数字。 | 保留 |
| [capture_dependency_contract.rs](crates/lycheedev-core/tests/capture_dependency_contract.rs)、[capture_query_references.rs](crates/lycheedev-core/tests/capture_query_references.rs) | typed CAP依赖发布、顺序身份、显式引用、非引用payload、demux依赖、并发删除与回滚；实际query引用和生命周期。 | 保留当前合同 |
| [budgeted_capture.rs](crates/lycheedev-core/tests/budgeted_capture.rs)、[bounded_evidence.rs](crates/lycheedev-core/tests/bounded_evidence.rs)、[durability.rs](crates/lycheedev-core/tests/durability.rs) | CAP 大结果/预算/取消/发布失败、原子替换、CAS、锁与实际持久化。 | 保留 |
| [transactions.rs](crates/lycheedev-delivery/tests/transactions.rs)、[combined_addon.rs](crates/lycheedev-delivery/tests/combined_addon.rs) | 受管安装幂等、addon/槽组合更新、事务中断和恢复、不接管未受管文件。 | 保留 |
| [slot_maintenance.rs](crates/lycheedev-delivery/tests/slot_maintenance.rs)、[release_control.rs](crates/lycheedev-delivery/tests/release_control.rs) | 当前 500 槽、预约/unknown 状态和 release 清单读取边界。 | 保留 |
| [slot_reset_tests.rs](crates/lycheedev-delivery/src/slot_reset_tests.rs)、[slot_publication_tests.rs](crates/lycheedev-delivery/src/slot_publication_tests.rs) | 缺失/损坏清单与任意原槽字节的归档初始化、中断续走、完成重试保留后续预约；已证明结束的原交易精确退休，拒绝旧代与迁移清单，保留文件读取失败与预约冲突。 | 保留；隔离磁盘输入 |
| [doctor_repair_tests.rs](crates/lycheedev-live/src/mailbox/doctor_repair_tests.rs)、[doctor_close.rs](crates/lycheedev-live/src/mailbox/doctor_close.rs)、[native repair tests](crates/lycheedev-live/src/mailbox/native_backend/tests/repair.rs) | 原CON关闭、独立重载、报告与cleanup分离、归档选择恢复及共享占用；发送前刷新INPUT、退休等待新鲜样本和损坏池修复保留原字节。 | 保留；离线输入不证明游戏行为 |
| [slot_reset_end.rs](crates/lycheedev-live/src/mailbox/connection/tests/slot_reset_end.rs) | 正式持久化CON与磁盘pending载荷核对进程结束后的普通退休与hard-reset；重置前保留原字节、原报告、未知输入和预算。 | 保留；隔离磁盘与进程观察fixture |
| [protocol_upgrade_activation_tests.rs](crates/lycheedev-live/src/mailbox/protocol_upgrade_activation_tests.rs)、[protocol_upgrade.rs](crates/lycheedev-live/src/mailbox/native_backend/tests/protocol_upgrade.rs) | 旧schema的实际双键能力才允许自动重载；三键配置要求手动reload，原激活预算保持，旧INPUT不授权Invoke或替代当前新runtime证明。Prepared和旧协议手动等待路径在重载前分别拒绝build/product不符，保留原CON、owner、请求与预算且不创建输入日志。 | 保留；合成Mailbox输入 |
| [passive_windows.rs](crates/lycheedev-live/tests/passive_windows.rs) | Windows 自有窗口的可见性枚举与取消原因；不操作或证明游戏。 | 保留 |
| [formal_lua_bridge.rs](crates/lycheedev-live/tests/formal_lua_bridge.rs) | 当前 addon Lua writer 与 Rust Driver/journal 的双向结果、精确浮点、恢复与不重放；已改接当前 addon/受管 Lua 5.1 admission，移除冻结 Go 来源。 | 保留 |
| [runtime_module.rs](crates/lycheedev-windows/tests/runtime_module.rs)、[native.rs](crates/lycheedev-windows/tests/native.rs)、[destination.rs](crates/lycheedev-windows/tests/destination.rs) | OS 模块/内存/文件边界、精确进程身份、资源计量、原子替换、Job 退出与目标发布。 | 保留 |
| [process_lifetime.rs](crates/lycheedev-windows/tests/process_lifetime.rs)、[native_source_retirement.rs](crates/lycheedev-windows/tests/native_source_retirement.rs)、[detached.rs](crates/lycheedev-windows/tests/detached.rs) | PID 复用/创建时间、持有生命周期、Source owner 退出/回收、管道不继承及失败启动。 | 保留 |
| [mailbox_input_fixture.rs](crates/lycheedev-windows/tests/mailbox_input_fixture.rs) | 自有窗口上的实际双绑定、取消后按键释放、新鲜目标检查与不抢焦点/不写聊天；readiness间接安装全局键盘钩子，仅CI干净runner执行，不替代 H03。 | 保留 |
| [guarded_spawn_fixture.rs](crates/lycheedev-windows/tests/guarded_spawn_fixture.rs) | harness=false 的受控子进程程序，只供父测试使用。 | 辅助 |
| [guarded_spawn.rs](crates/lycheedev-windows/src/guarded_spawn.rs)、[guarded_spawn_tests.rs](crates/lycheedev-windows/src/guarded_spawn_tests.rs) | 目标进程PSS线程快照拒绝多线程，保留取消/deadline；真实子进程核恢复执行前Job归属、参数/环境/stdio、后代进程及失败回收。后两项需显式helper路径、哈希与自有临时根；已按该入口实际运行通过，不涉及游戏。 | 保留 |

## 4. Node 工具、安装与 CI 测试

这些测试多数验证工具编排或模拟输入。即使通过，也不能据此宣布 Rust 原生业务、真实 npm 安装或性能通过。当前 CI 入口见 [ci.yml](.github/workflows/ci.yml) 与 `tools/ci-*.mjs`。

### 构建、包与当前工具（保留）

[rust-agent-client.test.mjs](tools/rust-agent-client.test.mjs) 验证实际 Classic Map 的 CASC nameHash 与 WDC keyId 精确保留 u64，并保持行数、预算和请求身份的安全整数要求；这属于测试消费者的协议回归，不代替实际 Data 查询。

| 入口 | 验证的业务或边界 |
| --- | --- |
| [version.test.mjs](tools/version.test.mjs)、[release-channel.test.mjs](tools/release-channel.test.mjs)、[addon-package.test.mjs](tools/addon-package.test.mjs)、[tar.test.mjs](tools/tar.test.mjs) | 版本源同步、单源渠道选择/无未知版本fallback与原TGZ authority审计、addon TOC/资源打包与归档字节/CJK 路径；addon 打包已去掉旧 Go 用例。 |
| [skill-contract.test.mjs](tools/skill-contract.test.mjs)、[skill-launcher.test.mjs](tools/skill-launcher.test.mjs)、[rust-package-smoke.test.mjs](tools/rust-package-smoke.test.mjs) | Skill 合同/链接、嵌套staging父目录及目标排他、Windows npm 启动。package produce一次组装/pack，validate只安装原TGZ；真实归档fixture检查搬运、旧提交/字节篡改、安装CLI和资源、失败报告，不允许消费阶段重建或重打包。N09 的实际安装另行验证。 |
| [workflow-structure.test.mjs](tools/workflow-structure.test.mjs)、[ci-changes.test.mjs](tools/ci-changes.test.mjs) | 每PR完整Release/包验收的依赖图；固定工具先验、Rust/addon独立消费、唯一TGZ、分配置缓存、失败诊断和报告上传。两项文件symlink边界在所属Windows runner显式执行ignored exact用例，复用原测试程序并保留独立报告，环境错误仍失败。实际PowerShell执行工具链拒绝与required汇总的成功/失败/取消/跳过分支。嵌入与打包目录中的Markdown仍触发产品验收；普通文档才跳过。结构测试不代替Actions。 |
| [ci-test-programs.test.mjs](tools/ci-test-programs.test.mjs) | 预编译程序与support复用，默认/显式编译并发、timings传递与非法参数零启动；原生产receipt/日志随产物传递。两组选择共用唯一包映射，汇总检查同一record哈希/提交、计划与实际程序身份、互斥完整覆盖和回收；漏组、重复、篡改及缩小测试参数不能算通过。普通程序或Lua bridge未回收时保留原失败报告并停止后续启动，已回收的业务失败仍允许后续测试。 |
| [rust-build.test.mjs](tools/rust-build.test.mjs)、[rust-disk.test.mjs](tools/rust-disk.test.mjs) | owned 构建、60分钟编译期限与独立的依赖查询期限、并发、产物/工具链、build cache/scratch 额度（50 GiB/5 GiB）。源码摘要包含实际嵌入fixtures；receipt/日志复制保留原字节，本次Cargo timing报告不能用旧文件冒充，记录真实耗时与重建原因；不是产品Data容量验证。 |
| [rust-owned-process.test.mjs](tools/rust-owned-process.test.mjs)、[rust-owned-process-atomic-job.test.mjs](tools/rust-owned-process-atomic-job.test.mjs)、[rust-owned-recovery.test.mjs](tools/rust-owned-recovery.test.mjs)、[rust-owned-memory.test.mjs](tools/rust-owned-memory.test.mjs) | Job/进程创建、启动记录、I/O、取消、资源回收与测量。Cargo helper许可使用固定清单内的精确Rustup启动器哈希，启动与恢复共同验证；未知哈希仍拒绝，Job与root wrapper绑定保持。 |
| [rust-unsafe-boundaries.test.mjs](tools/rust-unsafe-boundaries.test.mjs)、[rust-license-inventory.test.mjs](tools/rust-license-inventory.test.mjs)、[rust-emmy-identity.test.mjs](tools/rust-emmy-identity.test.mjs)、[luac.test.mjs](tools/luac.test.mjs) | FFI 边界、许可证、固定 Emmy 上游身份与 luac 打包；CI 复用已有原生产物。 |
| [lua-tool-supply.test.mjs](tools/lua-tool-supply.test.mjs)、[rust-formal-lua.test.mjs](tools/rust-formal-lua.test.mjs)、[rust-formal-lua-bridge.test.mjs](tools/rust-formal-lua-bridge.test.mjs) | 固定Lua供应包按清单、原生产记录/receipt、源码/许可证/EXE哈希导入，不依赖旧业务HEAD或绝对路径。原记录不改写；bridge仅为本轮执行生成路径映射并保留原摘要。篡改、目录覆盖及身份不符拒绝；当前addon与Mailbox测试继续使用同一解释器。 |
| [ci-release.test.mjs](tools/ci-release.test.mjs)、[ci-package-business.test.mjs](tools/ci-package-business.test.mjs) | 生产/安装/内容审计/业务报告绑定同一TGZ与CLI；候选还要求完整当前addon×client矩阵、同一Lua解释器及成功的addon/Skill报告。声明的哈希/字节/清单与复制后字节均核对，渠道不能改变。包验收和候选共用安装CLI的版本/编译提交/dirty状态校验；同版本旧binary不能冒充本轮产物。missing、not_run、partial、输入变化或未回收不能封存。业务入口复用tiny Source与预编译Data输入，实际查询使用安装包内Release；Node边界测试不代替真实执行。 |
| `tools/ci-{changes,docs,test-programs,artifacts,release}.mjs` | CI 变更分类、文档检查、测试程序构建/执行、产物 manifest seal/verify、候选封存；纯工具逻辑，需一次真实 CI 才能证明产物复用生效（N12）。 |

### Source/Data/Live 业务 runner

| 入口 | 验证的业务或边界 | 处置 |
| --- | --- | --- |
| [session-ipc.test.mjs](tests/rust-source-native-acceptance/session-ipc.test.mjs)、[runner.test.mjs](tests/rust-source-native-acceptance/runner.test.mjs)、[diagnostics.test.mjs](tests/rust-source-native-acceptance/diagnostics.test.mjs) | 实际 Source native IPC、诊断与会话验收的工具编排；复用当前 Release。 | 改接 |
| [runner.test.mjs](tests/rust-source-full-corpus/runner.test.mjs) | 完整固定源码 corpus 的准备/执行/结果读取；全量业务用当前 Source 产物实际运行。 | 改接 |
| [prepare.test.mjs](tests/rust-data-local-current/prepare.test.mjs) | 为实际本地数据测试生成固定独立预期；可以读取仓库外参考产物，不构建/嵌入 Go，不算当前查询通过。 | 辅助/改接 |

### 当前独立输入与有界测量工具

用户已接受性能，不重跑比较。保留当前Agent Data/generation/Live控制器、完整独立输入
及已有结果；旧Source pack格式/exporter/B01—B02执行器已退役。当前用途见
[目录说明](tests/rust-performance-paired/README.md)。

| 入口 | 证明范围 |
| --- | --- |
| [agent-consumers.test.mjs](tests/rust-performance-paired/agent-consumers.test.mjs) | 当前Agent请求、精确数值、完整响应及退出/回收；固定输入认证与原始控制器总时。 |
| [generation-contract.test.mjs](tests/rust-performance-paired/generation-contract.test.mjs)、[generation-evidence.test.mjs](tests/rust-performance-paired/generation-evidence.test.mjs)、[generation-profile.test.mjs](tests/rust-performance-paired/generation-profile.test.mjs) | 当前prepare/generation、完整结果与CAP、各阶段身份和诊断记录。 |
| [live-runner.test.mjs](tests/rust-performance-paired/live-runner.test.mjs) | 离线Mailbox控制器与结果收集；无游戏访问，不能证明实机。 |

正式包继续复用实际消费的agent fixture producer/importer。外部固定参考只做显式输入；
当前仓库不构建Go或保存旧执行器。数据、业务覆盖、测量与实机结果分开说明。

## 5. 现有 addon 的 Lua 回归

addon 沿用当前实现，以下测试保留。输入来自 [env.lua](tests/addon/env.lua) 与 [wow_globals.lua](tests/lib/wow_globals.lua) 的模拟 WoW 表面，使用真实 Lua 5.1 解释器执行；证明 addon 在给定模拟环境下的行为，不证明实际游戏战斗、焦点或秘密值实现。

入口：
- 单套件：[run.lua](tests/addon/run.lua)，参数 `<t_file.lua> [client] [addon_root]`。
- 聚合（Go-free，CI使用）：`node tests/addon/run-all.mjs <lua-build-report.json> [--file/--client/--addon-root]`，与正式Rust桥接共用`tools/lua-tool-supply.mjs import`导入的Lua 5.1.5报告、解释器和原生产回执，按client matrix运行全部`t_*.lua`。

保留套件（37 个 `t_*.lua`）：bootstrap/client_gate/locale/about/shell/visual；fields/inspector/objects/object_stream_errors/object_picker_combat；execute/run/probe_execution；input_state/platform_input_state/input_protection/input_mailbox/input_binding_gate/input_attempt/focus_bindings；mailbox/bridge_status/slot_protocol/slot_runtime/slot_enable；serializer/stores/character_state/records；events/trace/trace_owner/diagnostics；automation/automation_history/activity。绑定套件核有效方案、加载/脱战等待、玩家修改、同步UPDATE_BINDINGS重入、原方案保存与手动重置；History核非空内容、false状态及reload持久化。聚合入口还检查 TOC 统一合同、7 个 Interface、50 个 load 与 6 个 Event Catalog。

## 6. fixture、peer 与独立参考

| 位置 | 用途与处置 |
| --- | --- |
| [tests/fixtures/](tests/fixtures/) | 固定 DB2/DBD、源码、Hotfix、协议和其他字节输入；当前消费者：data 的 db2/hotfix-wago 输入、source 的 source-native 输入、live 的 live-journal。provenance 保留 db2 AGPL 归属。输入文件不是测试通过。 |
| [tests/protocol/](tests/protocol/) | 当前Mailbox/formal bridge的slot_server peer；正式bridge直接加载addon的CaptureWriter。旧receiver_bindings、character_state、input_state的独立业务已迁到当前addon套件，原孤立入口删除。 |
| [tools/rust-formal-lua.mjs](tools/rust-formal-lua.mjs) | 当前解释器准备工具；统一构建阶段执行，addon与Rust桥接复用同一canonical报告及随产物携带的原始构建回执，不每组重建。 |
| 各 crate 的 `tests/support/`、`#[ignore]` fixture/export/import helper | 共享断言、种子和原生 peer；与父业务用例一起检查引用。辅助准备成功不单独计业务通过。 |
| [third_party/emmy/](third_party/emmy/) | 固定原版 Emmy 上游 recipe/identity；生成源码在 `.tmp/emmy-reconstruction-v1/upstream`（848 文件，custody 校验）。这是当前 Source 语义引擎输入，不是旧实现。 |
| [release/licenses/catalog.json](release/licenses/catalog.json)、[release/tools/luac.json](release/tools/luac.json) | 当前许可证补充与受管 luac 构建身份；打包/CI 消费者使用。 |

外部独立预期保留其输入与结果含义，不把旧产品的错误当业务规范。Source 编码行为按 N11 的新合同，不能为了与旧参考结果相等继续拒绝合法注释。

## 7. 当前架构的业务回归范围

这些是当前正式路径的覆盖要求。复用已有独立输入和有效断言，只为确认缺口增加用例。
测试用途留在这里；本批实际执行结果进入handover，未完成项进入todo。

| 方向 | 正式入口与必须验证的事实 |
| --- | --- |
| N01 / Agent自解释 | 安装Skill→search/schema/help/describe。命令示例、必填/条件输入、更新policy与实际typed admission一致；Data请求和响应字段、普通/Agent framing、错误纠正可用；发现不创建home、不联网、不起worker。 |
| N02 / 类型化编排 | CLI/Agent转正式领域请求，固定项目/矩阵/原始probe输入；Source/Data/Live各有独立归属。实际Data断管、CAP保留、Source复用与持久Live恢复互不冒充游戏行为。 |
| N03 / Data查询与领域 | 原始DB2/CASC→Session/Engine/DataFusion；通用plan及全部领域任务、关系/顺序/精确数/数组/null、真实partial与完整空来源；page/export/CAP复用同handle，Explain/Analyze按声明执行。 |
| N04 / 真实数据范围 | 固定原BLTE与独立逐行/像素预期；七产品独立PIN，两Forever保持缺密钥/分区事实。明确synthetic元数据与真实当前安装分别证明什么；旧build不冒充更新后安装。 |
| N05 / Source语义 | 固定源码和环境→实际Emmy/luac/TOC/XML；声明、引用、调用、位置、关系与值流，冷暖认证/按需原文、unknown/partial、会话复用与关闭。 |
| N06 / Mailbox离线 | 当前addon writer↔正式Rust Driver/SessionOwner；精确JSON/字节、原request/deadline、取消、未知输入不重放、report/cleanup分离；formal Lua bridge只证明离线协议。 |
| N07 / 实际worker | 排队、背压、断管、EOF、CPU执行中取消与deadline；同Job全部回收、epoch失效、不重放、已提交CAP保留。单根退出或强制清理不算原正常回收成功。 |
| N08 / 资源与存储 | Data十进制5GB、共享blob一次计费、峰值、活跃/CAP保护、staging/spill/export；实际进程中断、原身份/锁/reparse/checked-close、发布后未知状态与退款。构建scratch不是产品Data总账。 |
| N09 / 正式包 | 唯一TGZ的隔离npm/Skill/addon/luac、同包实际Source/Data与安装恢复；manifest/版本/提交/dirty/许可证/完整资源匹配，验证阶段无重建。 |
| N10 / 实机 | 按handover最后H01—H08，七产品、500槽、绑定、多实例与恢复逐项实机；用户安排前不占用。 |
| N11 / 原始源码编码 | 下述非UTF8与原位置合同，单文件告警不丢全快照或伪造语义。 |
| N12 / CI产物流 | 测试/Release每配置生产一次，两shard覆盖唯一record；Lua/addon/Skill、安装/业务/审计/候选复用原物，漏程序/缺输入/篡改/未回收均失败。 |

### 7.1 Source原始编码

- 合法Lua的非UTF8注释继续保AST、声明、关系和符号位置，单独报告编码告警；展示转换不改变原始字节CAP和位置。
- 真实token/标识符中的非法字节仍是语法问题；字符串值不能由U+FFFD伪造。native等宽分析视图保留gap/unknown。
- 冷暖索引、原文、同提交diff、加载闭包和实际luac/native遵守同一合同；超限与编码错误分开，跳过保持coverage不完整。
- [index.rs](crates/lycheedev-source/src/index.rs)保32MiB+17超限文件与两个合法Lua的独立输入；[raw_response_tests.rs](crates/lycheedev-source/src/native_session/raw_response_tests.rs)核XML实体/换行/wrapper映射、精确wire字节、原始位置和同会话第二次查询。
- 真实Forever固定输入为`172a387e3ab85b26e6640d3058d5ec1e6ccd4c63`，含原0x92注释字节；按完整4,481文档、原字节与独立位置核对，不用旧参考的错误拒绝行为定义当前正确性。

### 7.2 最终安装包的非Live业务验收

验收使用完整CI的唯一TGZ和真实隔离npm安装，不另编CLI、不重打包。原生version、commit、dirty、资源及producer身份逐项绑定；每个完整业务owner在同一Job/原期限内完成并确认回收。实际报告及独立哈希汇总见handover。

| 业务 | 输入、独立预期与证明范围 |
| --- | --- |
| Source调查/验证 | tiny原始Git项目、Forever固定提交及四产品固定API；实际项目选择、query/inspect/native refs/context/batch、UTF8续读、原编码、TOC/XML加载顺序/内联Lua、语法和语义矩阵、CAP/bundle/keep/remove及关闭。静态候选/partial不升级为运行时结论。 |
| Data领域 | 认证原DB2/CASC及固定定义；12种navigate/17独立向量、浮点/数组、自动CAP、三类unavailable与完整空来源，以及同会话错误恢复。 |
| 查询与effective | 精确数值、lookup/batch/related/explain/analyze、同handle分页/CSV/JSON/CAP/release/close；两份原XFTH经正式Hotfix归档后验证新增/替换/删除、push/index/历史顺序、static不变及错误恢复。 |
| Hotfix与demux | 原XFTH归档续读、固定Wago HTML离线重解析/过滤/游标及完整性、AVI原字节/限额/冲突保留。固定HTML不冒充当前远程服务验证。 |
| 七产品Map | 保留真实BLTE全字节及原加密块，用明确synthetic元数据和新独立PIN读取，逐原文SHA/全部行值/顺序/分页/CSV/CAP核对；两个Forever保持73/81与缺key/不可读分区。Retail cold和显式prepare分别证明读取及generation复用。新旧PIN分开，不作为已更新真实安装或Live通过。 |
| 原资源导出 | 134400原BLTE与独立BLP/PNG/RGBA；原始字节、PNG/WebP64×64全像素，以及来源/导出/清单CAP依赖。 |
| 受管交付 | 合成client的main/500槽/Skill安装、幂等、update、真实deny-delete故障、原归档/不传release的resume与重复resume、归档移除及无关文件保留。 |
| npm自更新 | loopback registry只服务同一原TGZ，两次真实prepare/handoff/apply与隔离global-prefix安装；历史字节、新nonce、500槽、完整journal和服务/进程关闭。不发布或修改真实渠道。 |
| 管理与Agent使用 | config/target/cache/keep/doctor只读诊断与离线target修复；独立Agent仅读安装Skill、实际metadata与用户原输入，自主完成Source和三个Data任务，Root另用未透露的oracle核有用答案、精确数值、partial及收尾。doctor Live repair仍留后。 |

## 8. 执行与记录规则

本地按 AGENTS 使用当前 owned runner 与固定任务工作区，不新建并行原生队列。当前 [rust-cargo.mjs](tools/rust-cargo.mjs) 支持 `--owner`、`--one-shot` 和 `-- <cargo arguments>`；需要隔离用户目录的业务 runner 通过 [rust-build.mjs](tools/rust-build.mjs) 的现有 `runCargo(..., {isolateUser: true})` 入口准备产物。单构建通道同时只允许一个 owner；`disk.channel_held` 时等待重试，不直接调用 cargo。

CI 按 DESIGN 的产物流运行：先验证并导入 [Lua供应包](release/tool-supply/lua-5.1.5-windows-amd64/manifest.json)，生成一次addon ZIP，再并行编译测试与Release配置。两个原生job在Cargo fetch/build前用 `python third_party/emmy/prepare.py --download` 认证固定Emmy上游，保留本次timings与原receipt/日志；编译失败也上传owned证据。Rust/addon检查各自尽早运行；正式npm生产入口只pack一次，安装、独立内容审计和候选封存共用该TGZ。npm脚本路径与检查的固定版本一致。包内Source用tiny本地Git输入验证真实native查询，Data用预编译fixture仅构造DB2/CASC输入，再由安装Release完成query/page/CAP/release/close。缺输入、ignored和实机范围独立记录，下游没有构建回退。

Windows guarded-spawn的4个现有真实进程用例需要同批编译并封存的standalone helper。
该harness=false程序仅作为support传给下游，不计业务测试程序。Source-runtime job显式运行
这4项，验证辅助程序哈希并要求实际执行4项；本机也只使用自有helper。显式exact/ignored
过滤若实际执行0项，即使libtest退出0也必须失败；不能把筛选错名或全ignored记绿。

Rust验证使用 `ci-test-programs.mjs run <record> --shard data-delivery` 与 `--shard source-runtime` 两组；命名组不接受额外包/目标或测试过滤参数。包分组由工具中的TEST_SHARDS维护，新增workspace包须进入完整覆盖。release-ready使用 `aggregate <record> --shard-report <第一组报告> --shard-report <第二组报告> --report <汇总报告>` 校验所有程序恰好完成一次，再封存候选；每组失败报告独立保存。

默认ignored项按当前预编译程序的 `--list --ignored` 清点。fixture producer、真实外部输入、慢生命周期与子进程helper分别说明；helper只能由真实父测试带明确角色启动，不计空跑成功。当前数量在新构建验证后更新。

本机不运行全局键盘钩子：排除整个`mailbox_input_fixture`程序，并在`lycheedev_windows`中跳过`passive_monitor_installs_and_closes_without_input`及`class_and_executable_mismatch_block_normal_and_emergency_posts_on_owned_hwnd`。后者虽替换readiness检查，constructor仍安装钩子。纯输入引擎、键盘状态、其余自有身份检查和event_only测试按原入口执行；完整桌面集成由CI保留原期限与业务断言验证。

纯 Node 套件可用 `node --test <明确的套件路径>`，只选当前相关组；不要全量 glob 把已删除/已退役的历史套件再次当验收。Lua 使用第 5 节入口。主路径的原生/包/实机验证按任务风险执行，不为简单文档编辑运行整套产品测试。

结果记录实际输入、被测产物/配置、入口与结论即可。区分通过、未运行、ignored、环境受阻、业务失败；增删、改名、改接测试或改变测试覆盖与运行入口时同批更新本文件，修正 todo，更新 handover 的当前事实，不追加历史通过数量。
