# Source 独立业务验收矩阵草案

状态：**验收规格已列出，完整执行器尚未实现，完整新接口验收仍为 `not_run`**。机器可读源为 [source-business-matrix.json](source-business-matrix.json)。本次只整理文档，没有编译、运行 CLI、启动探针或操作游戏。

矩阵覆盖冻结 Go 3.1.2 的 12 条公开 Source 命令、G01–G16 业务 golden 和全部 36 个冻结 flow 输入。它不决定引擎选型，不授权迁移默认链路，也不把现有局部通过扩展为整体通过。

## 判定依据与执行方式

公开参数以冻结 describe 为准；业务正确性以原始源码、固定环境和明确预期为准。Go 原始失败、partial、重复 finding 和合法 `goto` identifier 的误拒绝均保留在 `frozenExpected`，不能为了让差分变绿而改掉；新实现还必须通过单列的 `requiredBusiness`。

JSON 每个命令含业务问题、固定输入要求、实际 argv 模板、预期、补充 profiles、unknown 含义及下一执行入口。每个 golden 含已哈希的文件或内嵌精确 UTF-8 字节、具体断言、当前证据和缺口。每个 flow 含全部原文、规则引用、target、budget、完整旧预期和独立业务预期。`nextExecution` 中 `runner_not_implemented` 是待补工作，不能当作已运行。

执行器需要先验证所有文件 SHA256 与字节数，再原样物化内嵌输入。不得转换换行、删 BOM、重新格式化 XML，或从待测分析器生成预期。argv 数组按独立参数传递；`${...}` 必须来自封存 manifest。输出目录必须全新，失败证据不得覆盖。

所有事实比较应检查完整集合、顺序（有语义顺序时）、原始范围、身份与 unknown 原因。计数只是附加不变量。支持范围内可确定的 local、alias、table member、固定 Mixin 和有限控制流不得靠永久 partial/unknown 通过。动态行为的预期 unknown 必须能指出实际缺少的事实。

## 固定输入与现有证据

- Go：commit `9d3d3598b8bac7efabf0b937df30508075eaa841`，EXE SHA256 `5738418756ee57c536518ff71223f2e7843ac3495a846b4f063d3104d83aae87`，89 条总命令。
- Retail：commit `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b`；4036 个文档 = 2551 Lua + 1091 XML + 394 TOC；原始文档 32269051 字节。资产查询还需要完整 Git tree/asset manifest，不能用该文档计数代替资产覆盖。
- Retail manifest SHA256：`7c2ba06f96d302621f9f9f0f505e0f66fd978e1509325119ba769ba511b1934a`。
- 所有证据报告、fixture、固定合同的路径、字节数和 SHA256 均在 JSON；后续源码改变不会自动继承旧二进制的通过结论。

| 证据 | 已实际证明 | 未证明 |
| --- | --- | --- |
| Native 193 + 独立 1365 | 54 个固定 typed-call site、selected value、native 类型/诊断及 session overlay refs | 全部结构成员 epoch、公开 raw resolver/reference index、完整 flow |
| Primitive 86 + 独立 1241 | 固定复制/覆盖/调用/effect profile 的图事实 | 完整 WoW 语义或 16 个 golden 全闭环 |
| General golden 94 个业务断言 | 固定词法、alias、范围等局部业务预期 | 原始引擎仍有 1 个 Lua51 label 兼容失败；完整 Source 未通过 |
| XML09 104 + 30 实际 luac | XML 原文提取、字节映射、词法/声明语法及小 fixture 编译 | Lua binding/type/flow 与 XML 统一接入 |
| Retail XML07 | 1091 XML 全量提取；1033 Binding、1025 body、77 ModifiedClick | Commentator profile/native 行为仍 unresolved |
| Full syntax 6569 | 2551 Lua + 4018 XML compiler source 实际 luac 编译；全输入哈希核验 | load phase、名称/类型绑定、flow 与性能对比 |
| 3 cycles / 9 uncached queries | 同进程全量构建、查询、释放；该有限序列资源回收 | 跨 CLI、持久 cache、所有阶段取消与长期稳定性 |

Commentator 原始 `</Binding>>` 的多余 `>` 是合法 XML 字符数据。完整 text `[1132,1136)` 与诊断 `[1132,1133)` 必须同时保留。正确状态是完整提取并带 unresolved profile diagnostic；不能丢弃 57 个 body，也不能把这个通过的预期诊断解释成 profile 全绿。

## 12 条公开合同

下面每条仍为新接口 `not_run`。全部原始 flags 已从冻结 describe 保留，未发明 `source defs/search` 等命令。

| 命令 | 业务问题 | 关键 profiles | Golden |
| --- | --- | --- | --- |
| `source list` | What sources exist and is this exact pin prepared without causing work? | absent pin; explicit project lock; nearest-parent lock; invalid pin; data-only pin | G13, G14, G15 |
| `source sync` | Which immutable commit does the requested source ref select? | default/full ref; commit/tag; ambiguous ref; offline object; missing/corrupt object; cancel | G13, G14, G15 |
| `source inspect` | Which original bytes or declaration support the result? | exclusive selectors; Unicode/BOM/CRLF; line/count bounds; missing file; case collision; escape path | G01, G07, G10, G13 |
| `source index` | Was every selected original file mapped with truthful diagnostics? | cold; valid cache; corrupt cache rebuild; engine/rule/config/API invalidation; cancel preparation | G06, G07, G13, G14, G15 |
| `source query` | Where does the requested text or declaration occur and why did it match? | default/api/lua/xml/toc/asset topics; exact/prefix/body/path; empty result; cursor | G01, G07, G08, G10, G14 |
| `source refs` | Which binding is read, written, captured or called and by whom? | incoming/outgoing/both; ambiguous name; foreign snapshot ID; saved function value; cursor; static-only conflicts | G01, G02, G03, G04, G05, G10, G14 |
| `source context` | Which definitions, original snippets, load evidence and bounded flow explain this symbol? | bytes 1/1048576; lines 1/2000; depth 0/4; long Unicode line; cursor; static/session/release conflicts | G01, G04, G05, G07, G08, G09, G10, G14 |
| `source session status` | What owned analysis resource exists without starting one? | absent; ready; busy; retiring; dead; identity mismatch; other home | G13, G15 |
| `source session close` | Were only this verified owner resources retired? | active request; already absent; invalid owner; cancel close; reopen | G15 |
| `source diff` | Which fixed files/declarations changed, moved or became ambiguous? | same commit; line-only move; duplicate declaration; signature change; syntax error; environment mismatch; limit default/1/200 | G12, G13 |
| `source prune` | Which unused derived resources can be reclaimed without losing fixed evidence or user bytes? | target met/unmet; active; dirty; orphan; foreign Git binding; interrupt | G13, G15 |
| `source validate` | Does the selected addon closure pass each fixed client syntax and semantic profile? | single path/toc/snapshot; strict matrix; Retail/Classic/Titan/Forever fixture identities; missing include; cycle; duplicate; LoD; API gaps | G06, G07, G08, G09, G11, G14 |

公开兼容性待定项必须显式解决：旧 `static-only` 返回 static candidate、没有 targetId 且 complete=false；新引擎身份不能装进虚构的 LuaLS 字段；新增精确 byte ranges 不得偷偷改旧 column 编码。所有查询共用同一身份与坐标投影。

## G01–G16 完整业务 golden

各行现有证据只证明其声明范围。完整 golden 状态均为 `not_run_as_complete_new_interface_golden`。

| ID | 能力与固定输入 | 必须得到的结果 | 现有局部证据 |
| --- | --- | --- | --- |
| G01 | Lexical scope and real references; scope.lua, writes-redeclaration.lua | outer captured initializer and outer return value bind file-local value; inner_read binds block-local value; comments/strings only text hits, never semantic refs/calls; parameters/upvalues/loop variables have distinct declaration IDs; local x=x initializer reads enclosing binding before new local begins; function-value reads are references but not calls | generic |
| G02 | Saved aliases and assignments; aliases.lua, alias-copied-value.lua, alias-copy-chain.lua, alias-name-nil-slot.lua, alias-function-write.lua, parallel-assignment.lua | first alias() selects original; after alias=replacement second selects replacement; before retains original; source-cell overwrite never changes a copied function value; parallel RHS reads pre-assignment cells; missing return slots become nil; parenthesized call truncates multireturn; last expression expands; definitions/types/candidates/diagnostics/inverse refs select identical value | generic, primitive, native |
| G03 | Table identity and methods; member.lua, colon-call.lua, mixins.lua | t.x and t["x"] select same static field on same object; alias retains object identity; t={} creates new epoch; t:run(a) supplies self; t.run(t,a) explicit equivalent; same field name on another object stays distinct; reads before/after overwrite differ; saved local retains old function; native member_infos/cache keys carry object/write epoch | generic, primitive |
| G04 | Load occurrences and execution phase; global-a.lua, global-b.lua, future-a.lua, future-b.lua | top-level read before later global write cannot select future definition; deferred call without phase unknown; explicit after-load phase may select later write; file locals never leak across files; repeated TOC/XML includes create separate ordered occurrences; load order/phase changes graph/query/cache identity with unchanged bytes | primitive, native |
| G05 | Mixin writes and shared semantic facts; wow-v7-contract.json, profile.lua, functions.lua, mixins.lua, shadow.lua, effect-mutation.lua, conditional-call.lua, context-native-contract.json | Mixin(dst,A,B) later source overrides prior fields in fixed profile; CreateFromMixins creates new object; copied function keeps saved value identity; local shadowed Mixin ordinary call, no intrinsic edge; XML mixin needs owner/load proof and same value graph; definitions/references/native types/candidates/argument-return diagnostics/flow use one binding; public raw resolver/reference index cannot bypass context; foreign models/caches carry context identity | primitive, native, coverage |
| G06 | Lua 5.1 authority; bom-unicode.lua, luac-compatibility.mjs | long comments/strings do not create phantom code; varargs/BOM/CRLF preserve original ranges; bundled luac5.1.5 via stdin, no source execution; raw-label parser acceptance corrected or authority-gated before binding certification | generic, syntax, xmlSyntax |
| G07 | XML declarations and byte maps; xml-golden-204.xml, xml-bindings-394.xml, xml-bindings-root-text.xml, xml-empty-bom.xml, xml-empty-control.xml, xml-handler-catalog.xml, xml-newline-contract.xml, xml-lexical-valid.xml, xml-declaration-valid.xml, xml-split.xml, xml-retail-UI.xsd, xml-companion-manifest.json | four fixed XSD handler catalogs with owner/type provenance; entities/CDATA/comment gaps/CRLF/BOM segmented; split token covering not byte-identical; CR/LF and opposite pairs match llex; entity CR preserved; synthetic wrapper anchor explicit; luac line-only column unknown; XML declaration/attribute grammar and retained malformed negatives transactional; Bindings attributes/duplicates/empty entries are metadata not Lua globals; inspect/definitions/refs/native type/flow reuse same source map | xml, xmlRetail, syntax, xmlSyntax |
| G08 | API environment and gap dependencies; environment-cases.json, environment-go.json, environment_parity.rs, flow_parity.rs | namespace differs from display group; parameters/returns/event slots 1-based ordered; nilability/types/enums/raw conditional secret metadata retain source ranges; duplicate conflict creates gap, never silent last-write selection; different client pins can give same API different signature; missing metadata propagates only along actual dependency closure; independent local/text facts remain complete despite unrelated API gaps; documentation tables parsed as data, never executed | not_run |
| G09 | All frozen flow cases; frozen-go.json, flow_parity.rs | legacy all-field equality separate from independent business correctness; known alias/shadow/static fields/finite control flow cannot stay permanently partial; guard protects same value and dominated branch; reassignment invalidates it; wrong/shadowed/negated guard and conditional metadata distinctions; Mixin/XML source/use steps use same source maps; secret-value flow distinct from secure execution taint | not_run |
| G10 | Pagination and output budgets; JSON 内嵌精确字节 | complete ordered page unions identical; duplicate names retain separate declaration IDs/ranges; Unicode long-line cursor advances within line by byte offset; cursor binds snapshot/query/options/client/engine/rules/order/page size; cross-command/identity cursor rejects, not restarts page one; page_complete/hasMore/truncated/inputCoverage/analysisComplete independent; one-record/scalar-too-small budget explicitly refused, no forever cursor | not_run |
| G11 | Load closure and client matrices; xml-ordered.xml, matrix-cases.json, loads_real.rs, matrix_parity.rs | TOC/Include/Script order and duplicate occurrences exact; dependency/OptionalDeps/LoD context explicit; missing include/escape/symlink/cycle diagnosed with source and cause; unloaded bad Lua excluded from execution closure, retained for repository search; matrix strict JSON and independent environment identities | xmlRetail |
| G12 | Diff and incomplete parse; diff_prune.rs | same bytes -> no file/declaration changes; line move distinct from signature break; duplicate declarations preserved; parse gap cannot prove deletion; independent category limits/true totals; no legacy diff cursor; cross-snapshot matching explicit | not_run |
| G13 | Integrity, config and caches; refs_context.rs | one-byte source/CAP/manifest/cache tamper rejected before facts trusted; engine/rule/config/API/map/luac identity invalidates dependent cache/cursor; cold/hit/distinct-query miss/forced derived-cache rebuild facts equal; data-only config schema and hash pinned; Lua config/require/os.execute/endless loop remain unexecuted bytes; Git hooks/filters not executed reading source; symlink/reparse escape and case collision explicit | not_run |
| G14 | Full Retail repeated business queries; 固定 profile/manifest 与受控状态 | all 4036 documents authenticated, none omitted; 2551 Lua and 1091 XML admitted; 4018 XML compiler sources separately counted; CallbackRegistry incoming plus distinct-symbol outgoing/context/flow/validate; cold -> same-session distinct uncached queries -> cross-CLI reuse -> hit -> forced derived-cache invalidation -> reopen; full facts compared with original bytes and fixed golden expectations; input/page scope cannot shrink for resource/time budget | retail, xmlRetail, syntax, xmlSyntax, cycles |
| G15 | Fault recovery and ownership; 固定 profile/manifest 与受控状态 | one total budget includes queue/preparation/serialization/cleanup; reserve return time; failure classification based on evidence; EOF is not OOM proof; after every fault unchanged normal golden and required full query; same-home owners respect leases; homes isolated; close -> close -> reopen -> prune preserves input/facts/captures; process/tree/handle/lease terminal states separately verified; durable result with lost notification recovered by same request ID, not replayed as original | deadline, memory, afterDeadline, afterMemory, cycles |
| G16 | Distribution and performance; 固定 profile/manifest 与受控状态 | Windows amd64 sealed EXE/PDB/source/lockfile/licenses/toolchain/luac identity; isolated offline installation no PATH/DLL assumptions; corresponding-source rebuild identity; luac stdin no bytecode; no investigated-code execution; each required comparable scenario alternates Go/Rust >=20 samples with same input/cache/output validation/optimized builds; Rust median <=0.5 valid Go median; report p95/CPU/memory/IO/retirement; failed/partial Go baseline preserved, never treated as fast success | not_run |

G07 中可直接机器比较的原始范围包括：204 字节 fixture 的 `Run` 为 `[92,95)`、`[123,126)`、`[159,162)`；394 字节 Binding fixture 首个 `Run` 由 `[75,76)` + `[76,82)` + `[91,92)` 组成。它们是分段映射，不能伪装为逐字节相同。所有未来 definition/ref/type/flow 必须复用相同映射。

G14 不得将全量测试缩小为某个 addon 或重用同一缓存结果。要求完整 manifest 的 cold 构建、同会话多个不同真实查询、跨 CLI 复用、cache hit、强制派生 cache 失效后重建，以及关闭再开。CallbackRegistry incoming 之外还要不同 symbol 的 outgoing、context、flow 和 validate。

## 36 个 flow 输入与独立判定

输入与旧输出逐字段保存在 JSON，来自固定 `crates/lycheedev-source/src/flow/fixtures/frozen-go.json`。`supported_complete` 要求相应有界能力真正完成；`expected_unknown` 只允许表中明确的缺口。旧 `flow_parity` 测试仍调用旧实现，不能证明新事实层通过。

| ID | 原 fixture | 独立业务预期 | 完成要求 | 旧边界（原样保留） |
| --- | --- | --- | --- | --- |
| F01 | `direct` | one_source_to_use | supported_complete | none |
| F02 | `slots` | only_second_result_reaches_use | supported_complete | none |
| F03 | `crossfile` | cross_file_parameter_return_chain | supported_complete | none |
| F04 | `last-return` | expanded_second_argument | supported_complete | none |
| F05 | `method` | method_parameter_return_chain | supported_complete | none |
| F06 | `guard` | no_secret_use_on_guarded_path | supported_complete | unknown_side_effect |
| F07 | `wrong-guard` | x_reaches_use_y_guard_irrelevant | supported_complete | unknown_side_effect |
| F08 | `reassign` | new_value_reaches_use | supported_complete | unknown_side_effect |
| F09 | `shadow-guard` | ordinary_local_function_not_intrinsic_guard | supported_complete | shadowed_guard |
| F10 | `allowed` | no_violation_for_allow_rule | supported_complete | none |
| F11 | `alias` | alias_source_reaches_use | supported_complete | shadowed_call, unknown_argument_constraint |
| F12 | `ambiguous` | unknown_which_wrap | expected_unknown | ambiguous_call, unknown_argument_constraint |
| F13 | `block-function` | no_binding_to_expired_local | expected_unknown | ambiguous_call, unknown_argument_constraint |
| F14 | `block-local` | outer_x_reaches_use | supported_complete | none |
| F15 | `branch-local` | one_reachable_use_no_duplicate_branch_finding | supported_complete | none |
| F16 | `shadow-sink` | local_sink_not_global_api | supported_complete | none |
| F17 | `param-sink` | unknown_parameter_callee_not_global_api | expected_unknown | shadowed_call |
| F18 | `side-effect` | guarded_unescaped_local_preserved_unknown_global_callee_effect_separate | expected_unknown | unresolved_call, unknown_side_effect, unknown_argument_constraint, unknown_side_effect |
| F19 | `condition-effect` | require_alias_reachability_for_field_invalidation_global_effect_separate | expected_unknown | unresolved_call, unknown_side_effect, unknown_argument_constraint, unknown_side_effect, unknown_argument_constraint, unknown_side_effect |
| F20 | `table-shadow` | outer_object_field_reaches_use | supported_complete | unknown_side_effect |
| F21 | `table-reassign` | fresh_table_field_nil_not_old_value | supported_complete | unknown_argument_constraint |
| F22 | `arithmetic` | unknown_operation_rule_not_safe | expected_unknown | unknown_operation_constraint |
| F23 | `comparison` | unknown_operation_condition_rule | expected_unknown | unknown_operation_constraint, unsupported_condition, unresolved_call |
| F24 | `concat` | unknown_operation_rule_not_safe | expected_unknown | unknown_operation_constraint |
| F25 | `index` | unknown_operation_and_dynamic_key | expected_unknown | unknown_operation_constraint, dynamic_read |
| F26 | `recursion` | recursive_boundary_no_fabricated_return | expected_unknown | recursive_call, unknown_argument_constraint |
| F27 | `loop` | reachable_secret_use_and_nontermination_boundary | expected_unknown | unsupported_loop |
| F28 | `repeat` | one_reachable_secret_use | supported_complete | unsupported_loop |
| F29 | `numeric` | two_finite_iterations_one_source_location | supported_complete | unsupported_loop |
| F30 | `generic` | zero_iterations_only_with_proved_builtin_pairs | conditional_unknown | unsupported_loop, unknown_argument_constraint |
| F31 | `negated-guard` | no_secret_use_on_guarded_branch | supported_complete | unknown_side_effect |
| F32 | `utf8-crlf` | source_line_2_use_line_3_original_bytes | supported_complete | none |
| F33 | `comment-string` | only_real_use_of_zero_no_secret_finding | supported_complete | none |
| F34 | `goto-identifier` | legal_identifier_source_to_use | supported_complete | parse_error |
| F35 | `path-budget` | visible_path_budget_and_truncated | expected_budget | path_budget |
| F36 | `same-line` | ambiguous_until_symbol_or_exact_range_selected | expected_unknown | ambiguous_location, target_unresolved |

重点差异：

- `alias` 必须识别保存的 Source 函数；合法 `goto` 局部变量必须按 Lua 5.1 接受。原 Go 拒绝保留作历史事实。
- `branch-local` 的常量真分支不能制造两条相同来源的执行路径；`repeat until true` 只执行一次；固定 `for i=1,2` 的两次执行可以用同一 source-site 记录携带 multiplicity 表达，不能靠重复 finding 冒充路径。
- `side-effect` / `condition-effect` 不能仅凭未知函数调用就虚构对未逃逸 local/table 的写入。需要区分可达 alias/effect、guarded value 与可能被改写的 global callee。
- `generic` 原输入没有固定 `pairs` builtin 身份，保留这一未知；另一个明确关闭环境的标准库 profile 才能证明空表没有迭代。
- `same-line` 原 line-only target 确实无法在 A/B 中唯一选择。保留 unknown，同时增加按精确 symbol/range 选 A 的 query，要求得到实际 Source → Use。
- `path-budget` 要求可见预算边界和 truncated，不能返回空成功。循环/动态边界不等于未发现违规，也不等于已证明安全。

## 输入身份、资源、cache 与恢复

不可变身份至少包含每文件原字节/路径/hash、源 commit/tree、有序 TOC/XML occurrence、执行 phase、客户端/API/dependency pin、数据配置、engine/features、规则、Lua 方言、XML map/wrapper 与 luac 身份。页面 cursor 与结果缓存都依赖实际影响结论的这些字段。

input coverage、analysis completion、output pagination/truncation 与 certainty 分开记录。页大小不能缩小分析输入；无关 API gap 不能让完全可解析 local 或全字节搜索永远 partial。缺失 API 也不能被当作不存在证据。

当前原型受控 Job 是 2 GiB / 全请求 90 秒；full compiler worker 为 85 秒，留下回收时间。这不把旧 LuaLS 1.75 GiB 比较场景变成同配额。每个正式性能场景必须冻结相同资源配置，不能增加预算后沿用旧比较结论。管理/index 尚未核定的预算要补冻结，不能编造。

故障矩阵覆盖 queued、preparation、analysis、publish、pagination、cleanup 的取消、deadline、内存拒绝、崩溃、协议、cache、存储失败和 owner 中断。每次之后必须执行未变的正常 golden/全量所需 query；root/tree/handles/lease 必须分别核验收尾。EOF 不等于 OOM，收到取消才是 cancel，持久结果通知丢失不能靠重新执行冒充恢复。

G16 要求 Windows amd64 离线隔离安装、对应源码/依赖/许可证、真实产物身份与无 PATH/DLL 假设。每个可比较的必需场景同机同输入/输出核验/cache 状态、优化构建，交替 Go/Rust 各至少 20 次，目标 Rust median ≤ Go 的 50%，并报告 p95/CPU/内存/IO/释放。Go 必需场景失败时保留失败与不可比较状态，不能拿失败耗时作为更快基线，也不能自动豁免目标。

## 下一步执行入口与关闭条件

现有受控入口可复用，但只证明局部范围：

- XML：`node tests/rust-source-engine/xml-owned-probe.mjs ${FRESH_OUTPUT} ${GOVERNED_BUILD_RECEIPT}`。
- 全量 syntax：`node tests/rust-source-engine/full-syntax-owned.mjs ${FRESH_OUTPUT} ${SEALED_XML_ARTIFACT_DIR} all`。
- 同进程资源 cycles：`node tests/rust-source-engine/owned-resource-probe.mjs ${FRESH_OUTPUT} cycles ${SEALED_LIBRARY_ARTIFACT_JSON}`。
- 旧 flow regression：`node tools/rust-cargo.mjs --owner source-business-flow -- test --offline --locked -p lycheedev-source --test flow_parity`。这不是新 Source 业务验收。

必须补上跨新 `prepare/query/manage` seam 的完整执行器与公开命令投影，再逐项填 actual、证据 hash、原始范围对照和终态。native structural member epoch、public raw resolver/reference index、完整 flow、TOC/LoD integration、API gap dependencies、跨 CLI/cache、全量 context/resource 和发行/性能仍是未关闭的门槛。

只有所有必需 profiles 和 G01–G16 的明确预期都实际通过，动态 unknown/预算负例按原输入正确验证，且 36 个 flow 的业务与兼容差异都记录清楚，才能讨论完整 Source 验收。当前 bounded native/primitive/XML 的通过不能替代这些结果。
