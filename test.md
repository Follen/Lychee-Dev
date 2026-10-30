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
| [Core](crates/lycheedev-core/src/) | 身份/选择、control（不可变 deadline + TrackedCancellation 世系）、公共精确 JSON、存储发布、CAP/依赖、缓存与 Data 计量、当前 Mailbox 数据合同。私有预声明发布复用已认证摘要的回归覆盖新发、复用、同长损坏正文、长度与maximum；三类证据键仍从删除历史保留归属。 | 共享机制；旧 schema 迁移、historical reader 与旧工作区布局断言已删除。 |
| [Data](crates/lycheedev-data/src/) | Agent Session/Engine/DataFusion、generation、元数据/DBD、CASC/BLTE/WDC、Hotfix、assets、keys、remote 与资源占用。HTTP body/gzip/取消/收费测试直接走正式collect_admitted，不保留仅测试消费的两层异步包装。 | 当前查询、输入和生命周期；旧 local_query/relational/navigate/migration-oracle 测试随旧实现删除；旧 Engine 固定快照/provider 资格层（engine/source、snapshot_cache、provider Fixed*）及内部测试已随旧实现退役，当前路径为 source_demand/session_engine/pull_provider/provider::runtime。 |
| [Source](crates/lycheedev-source/src/) | 原始字节与 Facts/Index、编码告警（N11 已覆盖）、TOC/XML、catalog/environment、luac、native 会话、查询/引用/关系/差异、固定源码与缓存。XML Facts覆盖Script与handler的声明、关系、实体换行原始位置及冷/热缓存一致性。 | 当前源码调查；不保留 LuaLS、旧 evaluator/JSON cache 合同。 |
| [Emmy adapter](crates/lycheedev-emmy-adapter/src/) | config/projection/session 与上下文适配；selected initializer回归覆盖参数/回调数量、返回控制流，以及Lua非尾标量、尾多返回、括号、DocTuple、开放必需前缀、Union包、Never和环境API返回槽。返回type/count与signature复用同一包形状规则，保留nominal carrier。 | 自有适配合同与实际比较的coverage归属；不替代实际 Release Source 任务，也不修改上游语义引擎。 |
| [Live](crates/lycheedev-live/src/) | mailbox（含 lookup 读路径）、passive、probe、runtime_root/runtime_binding 与 SessionOwner 恢复。 | 当前协议、读取和状态；合成内存不证明实际客户端 layout。 |
| [Delivery](crates/lycheedev-delivery/src/) | 获取、受管 addon/槽事务、安装更新、移除与 npm 交接。 | 当前交付事务；CLI 冻结安装/更新选择与 launcher 环境，库测试直接构造 RequestContext/Request；原维护租约与恢复仍在 Delivery。 |
| [Windows](crates/lycheedev-windows/src/) | 进程、Job、IPC、文件、内存、取消和输入句柄。 | 操作系统边界；不把这些测试计为游戏验证。 |

## 3. Rust 独立集成目标

执行入口为构建阶段生成的对应测试程序；依赖真实 CLI、luac、固定源码或安装的目标必须明确提供这些输入。含 ignored 的文件不能整份记为通过；准备 helper、被父测试调用的 peer 和业务测试分开记录。

### CLI：用户实际调用链

| 入口 | 验证的业务或边界 | 处置 |
| --- | --- | --- |
| [data_sql_public_numbers.rs](crates/lycheedev-cli/tests/data_sql_public_numbers.rs) | 实际 Agent 查询、精确数值、空数组、Page、JSON/CSV、CAP 和 Explain/Analyze；没有旧 SQL 执行入口。 | 保留（名字待改） |
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
| [table_decode.rs](crates/lycheedev-data/tests/table_decode.rs)、[wdc_palette.rs](crates/lycheedev-data/tests/wdc_palette.rs) | 受支持 WDC header、palette、copy ID、关系与 null、布局顺序和解码限制。 | 保留 |
| [hotfix_tables.rs](crates/lycheedev-data/tests/hotfix_tables.rs)、[hotfix_scan.rs](crates/lycheedev-data/tests/hotfix_scan.rs) | Hotfix 字段/数组/关系/Unicode、物理行状态、不可变分页/筛选、证据与引用释放。 | 保留 |
| [cache_keys.rs](crates/lycheedev-data/tests/cache_keys.rs)、[content_budget.rs](crates/lycheedev-data/tests/content_budget.rs) | 解密钥匙/缺钥、缓存认证、冷/热读取额度和取消；不单独证明 Data 全盘 5 GB 总账。 | 保留 |
| [assets_business.rs](crates/lycheedev-data/tests/assets_business.rs)、[image_export_business.rs](crates/lycheedev-data/tests/image_export_business.rs) | 资源搜索、raw/demux/image 导出、像素/通道、CAP、已有目标冲突和文件发布。 | 保留 |
| [agent_fixture_import.rs](crates/lycheedev-data/tests/agent_fixture_import.rs)、[agent_fixture_prepare.rs](crates/lycheedev-data/tests/agent_fixture_prepare.rs)、[skill_data_fixture.rs](crates/lycheedev-data/tests/skill_data_fixture.rs) | 导入 Agent 输入、构造当前 catalog 三表/16,384 行精确数值 WDC 和准备 partial 数据；均不预热查询，不计为查询通过。 | 辅助 |

### Source：固定源码与实际 native 分析（21 个目标）

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

### Emmy adapter 与共享类型（19 个目标）

这些目标直接验证当前适配接口。涉及自有表达式编排时必须仍以原版 Emmy 为语义来源，不能以测试存在为理由保留第二套 evaluator。

| 入口 | 验证的业务或边界 | 处置 |
| --- | --- | --- |
| [contracts.rs](crates/lycheedev-binding-contracts/tests/contracts.rs) | 默认值、元数据、返回值/receiver 类型、absent/unavailable 与对象上下文身份。 | 保留 |
| [public_ingestion_contract.rs](crates/lycheedev-emmy-adapter/tests/public_ingestion_contract.rs)、[declaration_ingestion_contract.rs](crates/lycheedev-emmy-adapter/tests/declaration_ingestion_contract.rs)、[declaration_contract.rs](crates/lycheedev-emmy-adapter/tests/declaration_contract.rs)、[declaration_rendering_contract.rs](crates/lycheedev-emmy-adapter/tests/declaration_rendering_contract.rs)、[callback_contract.rs](crates/lycheedev-emmy-adapter/tests/callback_contract.rs) | 输入字节/文本、Unicode 位置、批次增删、取消、声明载入与 unknown/coverage。 | 保留 |
| [lychee_context.rs](crates/lycheedev-emmy-adapter/tests/lychee_context.rs)、[lychee_checker_guards.rs](crates/lycheedev-emmy-adapter/tests/lychee_checker_guards.rs)、[lychee_cardinality.rs](crates/lycheedev-emmy-adapter/tests/lychee_cardinality.rs) | 上下文隔离、未知值与可空/非空callback签名精度；colon/dot/保存方法别名的receiver位置及proof，显式typed self和普通receiver在零/非零显式参数及显式泛型number/table时均核类型与精确范围；固定参数尾包逐槽检查、开放尾的已知前缀与partial、非尾/括号标量化。固定输入使用tests/fixtures/source-native。 | 保留 |
| [selected_query_contract.rs](crates/lycheedev-emmy-adapter/tests/selected_query_contract.rs)、[selected_projector_contract.rs](crates/lycheedev-emmy-adapter/tests/selected_projector_contract.rs)、[query_backend_contract.rs](crates/lycheedev-emmy-adapter/tests/query_backend_contract.rs) | 当前查询与投影、值来源、循环/取消、复用及完成范围。 | 保留 |
| [object_member_contract.rs](crates/lycheedev-emmy-adapter/tests/object_member_contract.rs)、[object_carrier_contract.rs](crates/lycheedev-emmy-adapter/tests/object_carrier_contract.rs)、[dense_tuple_contract.rs](crates/lycheedev-emmy-adapter/tests/dense_tuple_contract.rs) | 对象身份/成员顺序、嵌套值、数组/tuple 与缺失项。 | 保留 |
| [function_call_contract.rs](crates/lycheedev-emmy-adapter/tests/function_call_contract.rs)、[expression_plan_contract.rs](crates/lycheedev-emmy-adapter/tests/expression_plan_contract.rs)、[expression_execution_contract.rs](crates/lycheedev-emmy-adapter/tests/expression_execution_contract.rs)、[binary_core_contract.rs](crates/lycheedev-emmy-adapter/tests/binary_core_contract.rs) | 调用、表达式顺序、二元运算、短路/未知值和控制中断。 | 保留 |

### Core、Delivery、Live 与 Windows（20 个目标）

| 入口 | 验证的业务或边界 | 处置 |
| --- | --- | --- |
| [mailbox_contract.rs](crates/lycheedev-core/tests/mailbox_contract.rs) | 当前 Mailbox JSON、Unicode/code bytes 和精确数字；属现有协议，测试名待改。 | 保留/改名 |
| [capture_dependency_contract.rs](crates/lycheedev-core/tests/capture_dependency_contract.rs)、[capture_query_references.rs](crates/lycheedev-core/tests/capture_query_references.rs) | CAP 依赖、查询引用、身份与生命周期。`capture_dependency_contract` 由原固定 Go 3.2.0 oracle 文件退役重命名而来：保留 6 个当前 typed 合同用例（依赖发布/顺序身份/非引用 payload/显式依赖权威/demux frames typed 依赖/并发删除互斥/ledger 回滚），旧 Go wire 用例（raw 顺序/重复/null、raw timestamp escape、legacy locator/payload typed 字段、Go UTF-8 替换/规范化）已删除；typed manifest payload 引用保护由 core dependencies 单测 + data assets_business 覆盖。 | 保留当前 typed |
| [budgeted_capture.rs](crates/lycheedev-core/tests/budgeted_capture.rs)、[bounded_evidence.rs](crates/lycheedev-core/tests/bounded_evidence.rs)、[durability.rs](crates/lycheedev-core/tests/durability.rs) | CAP 大结果/预算/取消/发布失败、原子替换、CAS、锁与实际持久化。 | 保留 |
| [transactions.rs](crates/lycheedev-delivery/tests/transactions.rs)、[combined_addon.rs](crates/lycheedev-delivery/tests/combined_addon.rs) | 受管安装幂等、addon/槽组合更新、事务中断和恢复、不接管未受管文件。 | 保留 |
| [slot_maintenance.rs](crates/lycheedev-delivery/tests/slot_maintenance.rs)、[release_control.rs](crates/lycheedev-delivery/tests/release_control.rs) | 当前 500 槽、预约/unknown 状态和 release 清单读取边界。 | 保留 |
| [slot_reset_tests.rs](crates/lycheedev-delivery/src/slot_reset_tests.rs)、[slot_publication_tests.rs](crates/lycheedev-delivery/src/slot_publication_tests.rs) | 缺失/损坏清单与任意原槽字节的归档初始化、中断续走、完成重试保留后续预约；已证明结束的原交易精确退休，拒绝旧代与迁移清单，保留文件读取失败与预约冲突。 | 保留；隔离磁盘输入 |
| [doctor_repair_tests.rs](crates/lycheedev-live/src/mailbox/doctor_repair_tests.rs)、[doctor_close.rs](crates/lycheedev-live/src/mailbox/doctor_close.rs)、[native repair tests](crates/lycheedev-live/src/mailbox/native_backend/tests/repair.rs) | 原CON关闭、独立重载、报告与cleanup分离、归档选择恢复及共享占用；发送前刷新INPUT、退休等待新鲜样本和损坏池修复保留原字节。 | 保留；离线输入不证明游戏行为 |
| [slot_reset_end.rs](crates/lycheedev-live/src/mailbox/connection/tests/slot_reset_end.rs) | 正式持久化CON与磁盘pending载荷核对进程结束后的普通退休与hard-reset；重置前保留原字节、原报告、未知输入和预算。 | 保留；隔离磁盘与进程观察fixture |
| [protocol_upgrade_activation_tests.rs](crates/lycheedev-live/src/mailbox/protocol_upgrade_activation_tests.rs)、[protocol_upgrade.rs](crates/lycheedev-live/src/mailbox/native_backend/tests/protocol_upgrade.rs) | 旧schema的实际双键能力才允许自动重载；三键配置要求手动reload，原激活预算保持，旧INPUT不授权Invoke或替代当前新runtime证明。 | 保留；合成Mailbox输入 |
| [passive_windows.rs](crates/lycheedev-live/tests/passive_windows.rs) | Windows 自有窗口的可见性枚举与取消原因；不操作或证明游戏。 | 保留 |
| [formal_lua_bridge.rs](crates/lycheedev-live/tests/formal_lua_bridge.rs) | 当前 addon Lua writer 与 Rust Driver/journal 的双向结果、精确浮点、恢复与不重放；已改接当前 addon/受管 Lua 5.1 admission，移除冻结 Go 来源。 | 保留 |
| [runtime_module.rs](crates/lycheedev-windows/tests/runtime_module.rs)、[native.rs](crates/lycheedev-windows/tests/native.rs)、[destination.rs](crates/lycheedev-windows/tests/destination.rs) | OS 模块/内存/文件边界、精确进程身份、资源计量、原子替换、Job 退出与目标发布。 | 保留 |
| [process_lifetime.rs](crates/lycheedev-windows/tests/process_lifetime.rs)、[native_source_retirement.rs](crates/lycheedev-windows/tests/native_source_retirement.rs)、[detached.rs](crates/lycheedev-windows/tests/detached.rs) | PID 复用/创建时间、持有生命周期、Source owner 退出/回收、管道不继承及失败启动。 | 保留 |
| [mailbox_input_fixture.rs](crates/lycheedev-windows/tests/mailbox_input_fixture.rs) | 自有窗口上的实际双绑定、取消后按键释放、新鲜目标检查与不抢焦点/不写聊天；不替代 H03。 | 保留 |
| [guarded_spawn_fixture.rs](crates/lycheedev-windows/tests/guarded_spawn_fixture.rs) | harness=false 的受控子进程程序，只供父测试使用。 | 辅助 |
| [guarded_spawn.rs](crates/lycheedev-windows/src/guarded_spawn.rs)、[guarded_spawn_tests.rs](crates/lycheedev-windows/src/guarded_spawn_tests.rs) | 目标进程PSS线程快照拒绝多线程，保留取消/deadline；真实子进程核恢复执行前Job归属、参数/环境/stdio、后代进程及失败回收。后两项需显式helper路径、哈希与自有临时根；已按该入口实际运行通过，不涉及游戏。 | 保留 |

## 4. Node 工具、安装与 CI 测试

这些测试多数验证工具编排或模拟输入。即使通过，也不能据此宣布 Rust 原生业务、真实 npm 安装或性能通过。当前 CI 入口见 [ci.yml](.github/workflows/ci.yml) 与 `tools/ci-*.mjs`。

### 构建、包与当前工具（保留）

[rust-agent-client.test.mjs](tools/rust-agent-client.test.mjs) 验证实际 Classic Map 的 CASC nameHash 与 WDC keyId 精确保留 u64，并保持行数、预算和请求身份的安全整数要求；这属于测试消费者的协议回归，不代替实际 Data 查询。

| 入口 | 验证的业务或边界 |
| --- | --- |
| [version.test.mjs](tools/version.test.mjs)、[addon-package.test.mjs](tools/addon-package.test.mjs)、[tar.test.mjs](tools/tar.test.mjs) | 版本源同步、addon TOC/资源打包与归档字节/CJK 路径；addon 打包已去掉旧 Go 用例。 |
| [skill-contract.test.mjs](tools/skill-contract.test.mjs)、[skill-launcher.test.mjs](tools/skill-launcher.test.mjs)、[rust-package-smoke.test.mjs](tools/rust-package-smoke.test.mjs) | Skill 合同/链接、Windows npm 启动、隔离安装 smoke 编排；需补极简使用面与 N09 真实安装。 |
| [workflow-structure.test.mjs](tools/workflow-structure.test.mjs) | 新 CI 四阶段、artifact 消费、无隐式重建、release 复用同一验证内容；所有 action 固定提交，包含带名称步骤的 uses；三个validate失败后仍上传报告且保持job失败。实际PowerShell正反例核对rustc多行版本输出；npm审计消费v1报告内的CLI版本。本地检查不代替Actions运行。 |
| [ci-test-programs.test.mjs](tools/ci-test-programs.test.mjs) | 预编译程序与support复用；普通程序或Lua bridge未回收时保留原失败报告并停止后续启动，已回收的业务失败仍允许后续测试。 |
| [rust-build.test.mjs](tools/rust-build.test.mjs)、[rust-disk.test.mjs](tools/rust-disk.test.mjs) | owned 构建、60分钟编译期限与独立的依赖查询期限、产物/工具链、build cache/scratch 额度（50 GiB/5 GiB）；运行时锁文件不误报源码变化，实际源码编辑仍拒绝；不是产品 Data 容量验证。 |
| [rust-owned-process.test.mjs](tools/rust-owned-process.test.mjs)、[rust-owned-process-atomic-job.test.mjs](tools/rust-owned-process-atomic-job.test.mjs)、[rust-local-owned-process.test.mjs](tools/rust-local-owned-process.test.mjs)、[rust-owned-recovery.test.mjs](tools/rust-owned-recovery.test.mjs)、[rust-owned-memory.test.mjs](tools/rust-owned-memory.test.mjs) | Job/进程创建、启动记录、I/O、取消、资源回收与测量。 |
| [rust-unsafe-boundaries.test.mjs](tools/rust-unsafe-boundaries.test.mjs)、[rust-license-inventory.test.mjs](tools/rust-license-inventory.test.mjs)、[rust-emmy-identity.test.mjs](tools/rust-emmy-identity.test.mjs)、[luac.test.mjs](tools/luac.test.mjs) | FFI 边界、许可证、固定 Emmy 上游身份与 luac 打包；CI 复用已有原生产物。 |
| [rust-formal-lua.test.mjs](tools/rust-formal-lua.test.mjs)、[rust-formal-lua-bridge.test.mjs](tools/rust-formal-lua-bridge.test.mjs) | Lua 桥接 runner 准备/结果判断；已改接当前 addon 与受管 Lua 5.1，无旧 rust-parity 依赖。 |
| [rust-source-pack-evidence.test.mjs](tools/rust-source-pack-evidence.test.mjs) | Source 导出/结果比较辅助逻辑；辅助，不计业务通过。 |
| `tools/ci-{changes,docs,test-programs,artifacts,release}.mjs` | CI 变更分类、文档检查、测试程序构建/执行、产物 manifest seal/verify、候选封存；纯工具逻辑，需一次真实 CI 才能证明产物复用生效（N12）。 |

### Source/Data/Live 业务 runner

| 入口 | 验证的业务或边界 | 处置 |
| --- | --- | --- |
| [session-ipc.test.mjs](tests/rust-source-native-acceptance/session-ipc.test.mjs)、[runner.test.mjs](tests/rust-source-native-acceptance/runner.test.mjs)、[diagnostics.test.mjs](tests/rust-source-native-acceptance/diagnostics.test.mjs) | 实际 Source native IPC、诊断与会话验收的工具编排；复用当前 Release。 | 改接 |
| [runner.test.mjs](tests/rust-source-full-corpus/runner.test.mjs) | 完整固定源码 corpus 的准备/执行/结果读取；全量业务用当前 Source 产物实际运行。 | 改接 |
| rust-skill-usage 旧迁移对账 runner | 已彻底删除（task-13，累计 48 文件）。`tests/rust-source-native-acceptance` 改用自包含 `native-contract.mjs`（native 证据断言）与 `runner-inputs.mjs`（固定 GO_CONTRACT/NATIVE_CALLSITE、当前 package/Skill 校验、json/inventory helper），不再读取 workflow/baseline 迁移输入；perf source-pack RUNNER_FILES 改为 `native-contract.mjs`。 | 退役（自包含改接） |
| [prepare.test.mjs](tests/rust-data-local-current/prepare.test.mjs) | 为实际本地数据测试生成固定独立预期；可以读取仓库外参考产物，不构建/嵌入 Go，不算当前查询通过。 | 辅助/改接 |

### 性能工具（已改接外部参考模型）

用户已接受当前实测性能，性能工具和历史测量保留，不再为原20对及≤50%目标补跑。后续使用仍须比较相同完整业务结果；性能工具不能把生产Rust调用回退到旧SQL，也不能重新构建候选程序。外部固定Go 3.2.4参考只作为**仓库外显式输入目录**（`.tmp/paired-go-3.2.4-artifact/`：artifact.json + dev-npm-stage + tgz，忽略不提交），当前Rust候选使用本轮构建出的optimized artifact；工具只做静态字节/身份校验，不在仓库内构建Go、校验`go version -m`/CGO身份或LuaLS payload。

当前`rust-performance-paired.mjs`的实际执行范围仅为B01 describe、B02固定Retail index/query，三个case各有冷/热组，每组20对。已有报告的比值和判定保持原样；用户接受当前性能不改写历史结果。Live继续保留正式业务及实机验证。报告明确`fullB01B07Acceptance=false`；选定路径通过不证明其他Source、Data或Live已验证。

Data/generation 的现有 Agent 成对消费者使用 `agent-prepare-fixtures.mjs` 和同一 receipt replay 合同。默认准备 catalog/content/images，也可显式选择单类或 `--csv-numeric`；四类均走当前 Rust fixture 构造器，不调用旧执行器。数值 WDC/DBD/逐行预期保持原固定哈希，ABI 只记录当前编译观察。容量报告计入持久化外部导出声明，历史未归属临时文件仍明确不在完整覆盖范围内。

| 入口 | 验证的业务或边界 | 处置 |
| --- | --- | --- |
| [runner.test.mjs](tests/rust-performance-paired/runner.test.mjs)、[agent-consumers.test.mjs](tests/rust-performance-paired/agent-consumers.test.mjs) | 成对运行、顺序/完整响应及精确Agent数值；双方业务进程完整生命周期、原始控制器总时保留和退出完整性；复用本轮Release和外部固定参考目录。 | 保留（外部参考模型） |
| [generation-contract.test.mjs](tests/rust-performance-paired/generation-contract.test.mjs)、[generation-evidence.test.mjs](tests/rust-performance-paired/generation-evidence.test.mjs)、[generation-profile.test.mjs](tests/rust-performance-paired/generation-profile.test.mjs) | generation/完整结果/CAP 比较及阶段测量；当前 Go 仅作外部生产端身份记录。 | 保留 |
| [source-pack-scenarios.test.mjs](tests/rust-performance-paired/source-pack-scenarios.test.mjs)、[source-pack-scenarios-postcard-fresh.test.mjs](tests/rust-performance-paired/source-pack-scenarios-postcard-fresh.test.mjs)、[source-pack-custody.test.mjs](tests/rust-performance-paired/source-pack-custody.test.mjs) | 固定 Source 输入、首次/复用场景与输入来源检查；当前二进制格式和正式 Source 入口。旧 retained/continuation 历史证据认证用例已退役（旧 runner closure 不可认证），生产续跑代码是否退役待 REVIEW 决定。 | 改接（部分历史用例退役） |
| [read-index-postcard.test.mjs](tests/rust-performance-paired/read-index-postcard.test.mjs)、[read-fact-delta-postcard.test.mjs](tests/rust-performance-paired/read-fact-delta-postcard.test.mjs)、[owned-proof.mjs](tests/rust-performance-paired/owned-proof.mjs) | 当前二进制 Index/Facts 的比较辅助读取与自有 custody 断言（替代冻结 oracle helper）。 | 辅助 |
| [live-runner.test.mjs](tests/rust-performance-paired/live-runner.test.mjs) | 有界 Live 计时和结果采集；离线工具通过不代表真实 Live 性能。 | 保留；实机见 H08 |
| [tools/rust-performance-paired.mjs](tools/rust-performance-paired.mjs) | 真实20对比较入口：`<Rust artifact.json> <外部 Go 参考目录> <machine-conditions.json>`；需本轮optimized artifact与外部输入。当前候选的describe及Retail index/query冷、热六组各20对已完成，结果一致；Source四组达到原Go 50%目标，describe不设比值门槛。详细结果见handover，`fullB01B07Acceptance=false`；用户已接受当前性能，保留原测量范围与限制。 | 已接受的实测基线 |

`tests/rust-parity/` 本轮已全部删除；旧 oracle/冻结产品/历史发布合同不再作为测试入口，也不要求重新跑绿。`tools/rust-performance-prepare-go.mjs`（仓库内构建 Go 包）已删除。

## 5. 现有 addon 的 Lua 回归

addon 沿用当前实现，以下测试保留。输入来自 [env.lua](tests/addon/env.lua) 与 [wow_globals.lua](tests/lib/wow_globals.lua) 的模拟 WoW 表面，使用真实 Lua 5.1 解释器执行；证明 addon 在给定模拟环境下的行为，不证明实际游戏战斗、焦点或秘密值实现。

入口：
- 单套件：[run.lua](tests/addon/run.lua)，参数 `<t_file.lua> [client] [addon_root]`。
- 聚合（Go-free，CI使用）：`node tests/addon/run-all.mjs <lua-build-report.json> [--file/--client/--addon-root]`，与正式Rust桥接共用`tools/rust-formal-lua.mjs build`产生的Lua 5.1.5报告、解释器和构建回执，按client matrix运行全部`t_*.lua`。

保留套件（35 个 `t_*.lua`）：bootstrap/client_gate/locale/about/shell/visual；fields/inspector/objects/object_stream_errors/object_picker_combat；execute/run/probe_execution；input_state/input_protection/input_mailbox/input_binding_gate/input_attempt/focus_bindings；mailbox/bridge_status/slot_protocol/slot_runtime/slot_enable；serializer/stores/records；events/trace/trace_owner/diagnostics；automation/automation_history/activity。聚合入口还检查 TOC 统一合同、7 个 Interface、50 个 load 与 6 个 Event Catalog。

## 6. fixture、peer 与独立参考

| 位置 | 用途与处置 |
| --- | --- |
| [tests/fixtures/](tests/fixtures/) | 固定 DB2/DBD、源码、Hotfix、协议和其他字节输入；当前消费者：data 的 db2/hotfix-wago 输入、source 的 source-native 输入、live 的 live-journal。provenance 保留 db2 AGPL 归属。输入文件不是测试通过。 |
| [tests/protocol/](tests/protocol/) | 当前 Mailbox/formal bridge 的Lua协议peer/fixture，包括slot_server与receiver_bindings；正式bridge直接加载addon的CaptureWriter，无消费者的旧EncodeSignal测试已删除。 |
| [tools/rust-formal-lua.mjs](tools/rust-formal-lua.mjs) | 当前解释器准备工具；统一构建阶段执行，addon与Rust桥接复用同一canonical报告及随产物携带的原始构建回执，不每组重建。 |
| 各 crate 的 `tests/support/`、`#[ignore]` fixture/export/import helper | 共享断言、种子和原生 peer；与父业务用例一起检查引用。辅助准备成功不单独计业务通过。 |
| [third_party/emmy/](third_party/emmy/) | 固定原版 Emmy 上游 recipe/identity；生成源码在 `.tmp/emmy-reconstruction-v1/upstream`（848 文件，custody 校验）。这是当前 Source 语义引擎输入，不是旧实现。 |
| [release/licenses/catalog.json](release/licenses/catalog.json)、[release/tools/luac.json](release/tools/luac.json) | 当前许可证补充与受管 luac 构建身份；打包/CI 消费者使用。 |

外部独立预期保留其输入与结果含义，不把旧产品的错误当业务规范。Source 编码行为按 N11 的新合同，不能为了与旧参考结果相等继续拒绝合法注释。

## 7. 新架构需要闭合的回归

这些是覆盖要求，不意味着每项都要新建文件。先核对上面的当前测试，已经通过正式路径覆盖的就复用；缺口才增加用例。每项结果进入 handover，未闭合项保留在 todo。

| ID / 待办 | 正式业务入口与输入 | 必须说明的结果与限制 |
| --- | --- | --- |
| N01 / AGENT-01—02 | 已安装 Skill → CLI search/describe/schema/help、data agent schema。 | 常用称呼与 build ID 范围准确；两 Forever beta 身份独立；68项命令的业务示例统一来自 commands.json，现有合同测试逐项核解析后的命令身份及 help/schema/input/describe 一致性；实际业务运行仍按对应固定输入核实；发现不建 home、不联网、不起 worker；产品使用面无历史迁移内容。 |
| N02 / APP-01 | CLI 与 Agent 转换成当前类型化请求，Source/Data/Live 独立会话。 | Source 单测与集成直接构造 SourceContext、固定 selection 和类型化请求，覆盖 query/inspect/diff/prune、单目标与矩阵验证、batch/refs/context、分页、CAP、取消和 native 会话。source_cli_tests 覆盖参数范围与优先级、项目解析、runtime admission，以及接收请求后项目锁或矩阵文件改变时仍使用已固定输入。Live 实例/probe 测试直接使用 LiveContext/LiveRequest；CLI 存入相对路径 probe 后，正式 execute 复用同一 revision/原字节的回归已通过。Data 测试已改接 DataContext、SourceOptions、SessionConfig 和类型化 Hotfix/资产请求；CLI 分别解析会话内存与物理文件上限，终端 transport 留在 CLI。实际进程隔离回归接入 source_native_session_public：Data 断管终止后旧句柄失效、CAP 可读，Source owner/analysis 继续复用；Live 用正式 journal/claim/lease API 核原恢复事实保持，不替代游戏验证。Data 完整库回归已通过。Core 共享入口与 Delivery 改接 RequestContext/业务 enum，CLI admission 负例与原项目选择、证据/缓存、安装恢复回归随同迁移；本批最新 workspace 编译通过，CLI 和 Core 受影响行为回归已通过，Delivery 及实际进程隔离回归均已通过；Live 部分只证明正式持久化记录与租约，不证明游戏行为。 |
| N03 / DATA-01—02 | 正式 Agent 的通用查询与 12 项领域任务，固定完整与 partial 输入。 | 发现/ID/Join/聚合/子查询/Iterate 等真实运算；领域结果/关系/顺序完整；精确整数、浮点、null 与数组；Page/Export/CAP 不重跑，Analyze 一次。agent_session 的 UnionAll/Iterate 用例核固定行序、重复与宽整数，并在关闭 Engine、破坏自有输入且 fresh query 确认失败后验证原句柄仍可分页/导出/CAP；领域测试独立核模型/纹理/geoset 和完整关系顺序。Engine 的冷 Lookup Analyze 实际加载一次，分析后普通查询复用相同精确源数据；Explain 不加载源。`session_recovery_tests` 另覆盖默认512 MiB下16,384行全量数值排序、缓存复用及关闭退款；全量裸列扫描、投影和排序共用有界输入及DataFusion输出批次，表达式错误和filter/project/limit继续保留逐行读取边界。 |
| N04 / DATA-03 | 实际安装数据经 Agent Session → Engine/DataFusion，独立固定预期。 | 本批新包 Retail Map 冷/预备缓存各完整 1,187 行，Classic 285、Classic Era 59、Anniversary 83、Titan 130 行均逐值、顺序、CSV、CAP、来源与关闭通过。两 Forever beta 独立 PIN 各有 73 行可读、8 行缺失；可读全值、缺失 ID/分段/哈希、缓存复用和 CAP `complete=false` 均核对，不能声称完整数据可用。Retail FID 134400 的 inspect/raw/PNG/WebP/CAP 已通过独立原字节及全 RGBA 对照。共享读取、Arrow 批次/稀疏选择、同步源准备、取消/恢复回归另随正式 Data 套件；上述实际覆盖不扩大到所有表/资源。 |
| N05 / SOURCE-01 | 固定真实源码 → 当前 Release/native Emmy/luac/TOC/XML。 | 实际声明、引用、调用、位置、加载与语义覆盖；会话复用和关闭；当前二进制缓存冷/热一致。冷查询发布同一缓存后直接使用完整索引，热查询继续认证并投影；现有回归核完整结果、关系、游标、正文、资产、非UTF8及原字节按需发布，IO记录分别反映实际路径。research 已补接同 site/context 的 WriteVersion；真实 ResearchSession 正负例、原 projection 44 项和读写索引回归通过，新 Release 显式 refs_context 的 3 项也全部通过，包括原先失败的全关系和 128 outgoing 调用。原 semantic complete 断言不变，未知/条件写仍保持 partial。 |
| N06 / LIVE-01 | 当前 addon Lua writer ↔ 正式 Rust Mailbox Driver/SessionOwner。 | JSON/数字/字节与恢复一致；原请求/deadline、未知输入不重放、报告/cleanup 分开；已去掉冻结 Go 测试来源（formal bridge 已改接）。 |
| N07 / DATA-03 | 真实 supervisor/worker，排队、stdout 背压/断管、EOF/取消与CPU执行中取消。 | 现有实际用例覆盖输出阻塞时deadline与取消、排队deadline、EOF排空、epoch句柄失效不重放及已提交CAP保留。新增CPU用例先完成16384行scan，再执行常量字段自连接COUNT，持续排空stdout；观测owned Job内确切worker身份及用户CPU增量后取消，核原15秒deadline内回收、旧句柄失效和不重放；本批实际supervisor七项均已通过。该证据不证明某次DataFusion poll完全不yield。测试helper关闭已退出根句柄后，沿用原deadline等待同一Job归零；超时保留成员身份，强制清理不算通过。 |
| N08 / DATA-04 | 实际 Data 归属文件与共享 blob、generation、活跃结果、CAP、staging/spill/export。 | 十进制 5 GB 的 Data 归属口径及峰值、共享一次计费、冷缓存回收、活跃/CAP 保护。外部 raw/图片/CSV/demux 发布前持久化额度；output/scoped_tests 用实际子进程退出验证写入中遗留与发布后未结算均继续占额，并保留用户文件；正常连续发布释放额度。Core external_export 与 Windows managed 声明覆盖精确物理边界、Unknown/清理失败、撤销失败与原句柄关闭。inventory_tests 核元数据最终盘点、只读范围和quota释放；Windows目录及retirement回归核父目录复用后的原身份与锁定范围；共享blob盘点复用一次打开的根目录，核两份正文计费、取消和关闭错误，以及共享子目录在父句柄关闭后的身份与reparse边界。shared_blobs成对发布回归覆盖第二份的精确峰值、失败后首份与receipt保留、重复内容和超限零增长读取。固定slot扫描回归覆盖大小写、高编号槽位、后续新记录、坏记录保留及精确峰值恢复；新建slot仍走原占用检查。已有/缺失root回归覆盖full/held-parent/shared-home三路原生身份锁定、共享祖先写属性、取消后不创建与checked-close；初始化两个关闭失败仍保留全部原因并释放quota，CAP目录原写锁等待及取消回归保持；Spool占位探测回归保留foreign payload并核staging锁定直到关闭。历史未归属内容仍显式在覆盖范围外；不能用 build scratch 测试代替。 |
| N09 / DELIVERY-01 | 统一 Release/npm 包在隔离 prefix/HOME 安装，包内 Skill/CLI/luac/addon。 | 真正完成 Source/Data/安装任务，路径/版本/许可证与包内容一致；旧 Go install smoke 已删，当前入口 tools/rust-package-smoke + CI validate-package。 |
| N10 / LIVE-CLIENT | 真实客户端和已授权实例操作。 | 按 handover 最后 H01—H08 记录七产品、当前绑定、500 槽、多实例、恢复和所需性能；离线通过不代替本项。 |
| N11 / SOURCE-02 | 含非 UTF-8 注释/字符串的固定 Lua 输入；当前 Source query/inspect/context/validate/closure/native。 | 见 7.1；单文件编码告警不能中断全快照或伪造语义。最终已安装 Release 的实际 Forever 冷/热索引、inspect、同提交 diff、受管 luac、完整 native IPC 告警与会话复用均通过。research-material CAS 回归覆盖注释分析视图、不可转换 token/字符串 gap、篡改及普通 UTF-8 快路径；32 MiB+17 文件的超限冷/热索引回归也已通过。 |
| N12 / CI-01—04 | 实际 workflow、构建产物和下游 runner；结构检查与一次真实 CI 结果。 | 每种必要目标/配置编译一次；测试、Lua bridge、npm smoke、发布准备消费同一轮匹配产物；缺产物明确失败，无隐式重建、旧 Go job 或失效引用。validate-package 使用显式 build 许可证 closure，不再 fetch/metadata；回归核原字节复用、缺失/错误输入不回退生成及 workflow 调用，本批新包的实际显式复用也已通过。当前已有本地结构/静态及单机产物流验证，真实 Actions 与跨 job 路径未运行；只读 cargo -V 仍用于工具链身份核对。 |

### 7.1 N11：Source 编码回归的具体业务

来源为 [main 修复 c3450d9](https://github.com/Follen/Lychee-Dev/commit/c3450d9a32d6ea1334f0209897cc02ab06502ba6)。Rust 当前实现（本批已完成并验证）在 index/original_cache/lua_tree/closure/luac/inspect/context/native 边界统一处理，不能只删一处检查。

- 最小输入在前后各放一个合法声明，中间注释包含 0x92；声明、原位置、AST 数与完整性保持，编码告警有文件、行号和原始零起始字节偏移。
- query、inspect、context 对同一行采用一致 U+FFFD 显示；原归档逐字节及哈希不变，含非法 UTF-8 的 CAP 不标成 UTF-8 文本。覆盖 CRLF、BOM 和多字节字符邻接，防止显示转换造成位置漂移。
- 冷构建、热索引、原文缓存、同提交 diff、addon 加载检查和实际 luac/native 都保持上述事实。当前缓存格式更新会重建，固定 PIN 不变。
- 非法字节位于真正 token/标识符时仍报告语法问题，不能造出声明；字符串中的原始字节值与语义不被 U+FFFD 显示值替换；native 边界等宽分析视图不得输出替换后的字符串值/伪造声明，无法表达的 observation 标 gap/unknown。
- 单文件超限另报路径、实际大小与上限，仍如实影响 coverage，不与编码错误混用。[index.rs](crates/lycheedev-source/src/index.rs) 的 `public_index_reports_oversized_source_without_losing_valid_ast_or_encoding_warning` 已用实际 32 MiB+17 文件验证冷/热完整 index 与查询：超限文件跳过，另外两份合法 Lua 保留 AST/声明，其中非 UTF-8 注释独立告警，coverage 明确不完整。
- 实际输入使用 Forever 1.60.1.70450、源码提交 `172a387e3ab85b26e6640d3058d5ec1e6ccd4c63`：`Interface/AddOns/Blizzard_NewPlayerExperience/Camelot/Blizzard_TutorialData.lua` 第 400 行，0x92 在偏移 12719。最终已安装 Release 已通过包内 Source sync 获取并验证 4,481 文档/AST、0 跳过、0 语法诊断、1 编码告警，以及原字节、U+FFFD、同提交 diff 和受管 luac。公共 native refs 首次/复用均通过，第二次命中同一输入的缓存并只传快照标识，编码告警不变，随后 session 关闭。
- 完整 XML 来源映射的响应回归使用真实提取的实体、换行、合成 wrapper、空映射及 null 原始位置，对照 canonical JSON 字节和输入身份；[raw_response_tests.rs](crates/lycheedev-source/src/native_session/raw_response_tests.rs) 覆盖 typed reply、固定帧上限、owner 原字节转发、公开 Value 解码及同会话第二次查询。该回归不代替上述真实全快照的内存与 IPC 验证。
- 旧参考缺少这个修复时，不以旧结果为正确性标准；不把 Go 源码、旧 JSON cache 或发行版本改动带回 Rust。

## 8. 执行与记录规则

本地按 AGENTS 使用当前 owned runner 与固定任务工作区，不新建并行原生队列。当前 [rust-cargo.mjs](tools/rust-cargo.mjs) 支持 `--owner`、`--one-shot` 和 `-- <cargo arguments>`；需要隔离用户目录的业务 runner 通过 [rust-build.mjs](tools/rust-build.mjs) 的现有 `runCargo(..., {isolateUser: true})` 入口准备产物。单构建通道同时只允许一个 owner；`disk.channel_held` 时等待重试，不直接调用 cargo。

CI 按 DESIGN 的产物流运行：build 阶段一次准备测试程序、Release、luac 和包；validate 下游直接消费已有产物。两个原生 build job 在任何 Cargo fetch/build 前先用 `python third_party/prepare.py --download`（实际路径 `third_party/emmy/prepare.py`）认证并准备固定 Emmy 上游；Windows runner 无 gcc，Lua 5.1 解释器与受管 luac 都由同一固定 MSVC/SDK 身份构建。需要真实依赖的 ignored 测试显式选入，缺依赖记录未运行或环境受阻，不能算 pass；下游缺二进制时不能回退重新构建。

当前默认跳过62个测试实例（Source 38、Data 14、Windows 4、CLI 3、Live 2、Delivery 1）。源码有47处 `#[ignore]`，其中XML宏展开为16个实例，因此不能直接用属性行数统计用例。它们分别需要真实安装/客户端、真实游戏输入、真实固定源码/外部产物或symlink权限，也包含显式输入构造器；CI与默认测试都不把它们当业务通过。逐目标的原因在对应测试内注明。

纯 Node 套件可用 `node --test <明确的套件路径>`，只选当前相关组；不要全量 glob 把已删除/已退役的历史套件再次当验收。Lua 使用第 5 节入口。主路径的原生/包/实机验证按任务风险执行，不为简单文档编辑运行整套产品测试。

结果记录实际输入、被测产物/配置、入口与结论即可。区分通过、未运行、ignored、环境受阻、业务失败；增删、改名、改接测试或改变测试覆盖与运行入口时同批更新本文件，修正 todo，更新 handover 的当前事实，不追加历史通过数量。
