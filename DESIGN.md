# Rust migration design

Current decision/status authority: [handover.md](handover.md). This file specifies the target, not completion. Keep this exact uppercase filename; do not add `Design.md` alongside it on case-insensitive Windows filesystems. The former workbench-only design is preserved in [history](docs/history/workbench-ui-design-2026-09-28.md).

## 1. Contract and migration strategy

Go tagged baseline `3766f9cacb52e93a803a626187e2a1d54a445468` (v3.2.3) defines current observable behavior. Prior v3.2.2 `0bec60e4406f5d5dc155ee25cef44f9a710e37e8`, v3.2.1 `10b561db5706702cd42e047eaeed4ccb5e825351` and v3.2.0 `3057a9666ab078270b63c7f06de748069b0d1f4b` remain historical test oracles; no existing result is relabeled. Compare each Rust module against this immutable source and classify retain / adapt / replace / missing / unverified. The Rust snapshot is an implementation starting point, not a second behavioral oracle. Tag/main identity is verified; release assets remain unverified.

The allowed implementation differences are Rust host code and EmmyLua direct-library analysis replacing LuaLS. Preserve equivalent Source business semantics and honest engine/coverage provenance; engine-specific implementation metadata must accurately name EmmyLua, never impersonate LuaLS. No other compatibility exception is silently grandfathered from old plans. Any unavoidable business difference requires a separately approved decision before acceptance.

### Evidence-backed Rust enhancements

The user explicitly confirmed on 2026-10-17 that demonstrably better Rust results should remain. Fixed Go323 remains the compatibility reference, but a source-proven improvement in accuracy or completeness is retained with an explicit compatibility record and focused regression coverage rather than mechanically restoring a reference limitation. Preserve the original raw comparison and separately check the exact enhanced contract, including all unaffected fields, provider order and coverage. The two necessary successful object call-result receipts are the existing example. Unknown differences, speculative extra evaluation, changed error order and incomplete coverage are not improvements merely because Rust emits more output. Classify and establish each difference before acceptance; this does not relax business safety, budgets or the zero-patch Emmy requirement.

### Independent candidate package identity

The user selected Rust package version **3.5.0** and the **beta** npm distribution configuration, while Go **3.2.3** remains the behavior oracle. The version is not `3.5.0-beta.1`. Version synchronization covers nine owned crates, addon and npm metadata, owned dependency pins and reconstruction custody. Exact full-file release-field overlays permit only the approved Runtime/TOC version literals and restore the original Go bytes for comparison; historical oracle artifacts retain their identities. This configuration does not publish, tag, alter external channels or move `latest` from its retained 2.5.1 value.

### Tagged 3.2.3 adaptive Data memory

Follow the [3.2.3 increment matrix](docs/toolkit/rust-go-3.2.3-increment.md). Acquire the real workspace query capacity before sampling Windows available physical/commit/virtual memory. Fix the query ceiling at half the minimum available amount divided by admitted capacity; initial retained/metadata limits remain capped at 512/128 MiB. At growth boundaries, resample available headroom and round growth within the original ceiling in 64 MiB steps. Temporary scratch participates in retained accounting. The public memoryBudget report describes accounting limits and observed scratch, not RSS or allocated heap.

All query, SQL, reader/session and navigation consumers share this admitted budget. Keep non-memory limits, cancellation and typed cleanup; remove only the superseded cumulative raw SQL cap and per-file RemainingRetained restriction. Use compact record locations and shared alias locations without changing logical record identity. Indexed single-table numeric IN collects and validates candidates before yielding; NULL, duplicates, residual predicates and fallback semantics remain exact. Budget/cancellation failures do not fall back to a scan. Implement in idiomatic Rust and preserve Windows unsafe boundaries; test actual memory behavior rather than assuming a larger accounting ceiling improves performance.

### Tagged 3.2.2 managed-slot activation and recovery

Follow the [3.2.2 increment matrix](docs/toolkit/rust-go-3.2.2-increment.md) cumulatively with 3.2.1. For the current character GUID, enable only authenticated managed slot addons, verify changed entries and save addon settings; never load a slot or consume capacity merely to enable it. Preparation failure must not advertise ready. The input profile carries `slotReloadRequired`; compare it even when the timestamp is unchanged. Fresh pre-bind/Invoke observations journal the reload intent under the original CON, deadline, request and recovery transaction. A slot action is blocked while reload is required, but reload itself remains allowed. Repeated replacement runtimes resume the same operation; submitted/uncertain or missing-receipt outcomes must never be replayed. Continue handles recovery progress without resetting its budget. Doctor slot-activation observations are read-only. Keep the 500 ms keyboard-quiet contract and typed cleanup/error evidence intact.

### Released 3.2.1 increment and input ownership

Follow [the fixed increment matrix](docs/toolkit/rust-go-3.2.1-increment.md), not moving main. Retail 12.1.5 / interface 120105 remains compatible with 12.1.0; ClientGate, TOCs, runtime/slot catalogs and selection validate the exact product/build pairing. Do not infer Data support from the broader runtime product list.

Before sending input, all keyboard virtual keys (VK 0x03–0xFE, excluding mouse/reserved VK 0x04–0x07) must be released and keyboard activity quiet for at least 500 ms; then revalidate fresh INPUT and identity. The minimal WH_KEYBOARD_LL hook records the last activity time only, never text or a key history. Let an already-started chord finish and release its owned keys before waiting; cancellation releases only keys pressed by that attempt. Submitted/uncertain operations observe the original receipt without retransmission; every wait retains the original deadline. Offline state-machine tests and owned Windows fixtures precede user/game validation. Existing Rust tests do not prove this new increment.

### Idiomatic Rust, equivalent external behavior

Do not translate Go line by line. Preserve the external contract while using Rust ownership/borrowing to reduce unnecessary copying, enums/type-state where they prevent invalid transitions, Result for explicit errors, RAII for owned resources and bounded safe concurrency where measurements justify it. Prefer clear concrete abstractions over gratuitous generic frameworks. Review lifecycle/cancellation/drop ordering and thread ownership as architecture, not style-only concerns. These choices do not relax unsafe boundaries or replace measured performance evidence.

The user's 2026-10-16 refinement makes this an explicit all-module acceptance requirement. Stage 1 must assess Core, Source, Data, Live, CLI and Skill/delivery boundaries individually: identify who owns each large allocation and handle, which borrows or moves avoid copies, whether enums prevent invalid states, how streaming and explicit heap tasks bound memory and native stack depth, and where concurrency has measured benefit. Include cancellation, error and destruction paths. Record concrete costs and evidence for retaining or changing each design. Preserve good existing Rust implementations; neither blanket rewrites nor generic frameworks count as progress. Skill is evaluated at its workflow/implementation boundary rather than rewritten into Rust merely for appearance. Fixed Go external behavior, resource limits and independent measured acceptance remain mandatory.

The user further asks for proactive use of Rust concurrency: Go is the functional/result reference, not an internal module-layout or scheduling template. Evaluate bounded parallel parsing/validation and I/O pipelines where independent work exists; preserve deterministic public ordering, first-error behavior, resource admission and prompt cancellation. Measure throughput, latency and peak memory together. Data batch work may benefit from parallel preparation, while Live transaction/input order remains serialized where its contract requires it. Do not force concurrency into shared-state paths or replace a concrete pipeline with an unnecessary generic scheduler.

## 2. Boundaries and ownership

| Rust boundary | Responsibilities | Go reference at fixed release |
| --- | --- | --- |
| cli | Contract table, parsing/help/describe, orchestration, envelopes, JSONL, exit codes | internal/command |
| core | Selection/pins, project state, deadlines, evidence/cache ownership and atomic persistence | internal/selection, evidence, vault |
| source | Snapshots, load closure, static facts, semantic queries, validation and native worker/session lifecycle | internal/codebase, internal/addoncheck |
| data | CASC/CDN, DB2/SQL, Hotfix, relations and asset conversion/export | internal/records |
| live | Journaled connections/requests, exact receipts, reports, cleanup and recovery | internal/live/channel, internal/bridge |
| windows | Runtime instruction/RIP root lookup, guarded Lua reads, target identity and input | internal/live/memory, internal/desktop |
| delivery | Same addon payload, 500 slots, managed receipts, upgrade/uninstall, package and Skill | internal/delivery, addon, skills, packages |

Crate placement is not a new external interface. Shared addon reuse means no alternate Rust-only envelope, schema, binding or state machine. First compare the branch addon bytes with the release; restore equivalent released behavior where divergent, retaining the release's Compat/product catalog boundaries.

### Whole-repository Rust-native implementation plan

Keep the existing business crates and their useful ownership boundaries. The shared rule is a borrowed typed request plus the original operation control, an explicitly owned resource lifetime, and a consuming finish that preserves both primary and cleanup errors. Domain-specific Data logical memory accounting remains separate from Core deadlines and physical owners. Do not hide every business value behind a generic executor or create empty facade crates.

The current implementation work is four complete consumer migrations, followed by their consolidated validation:

1. **Core selection and Data default consumers:** `PinSelectionRequest` borrows home/cwd/snapshot/project; the compatibility Invocation entry constructs it once. The actual Data provider calls the typed resolver directly, while its existing Scope retains the original invocation/control and exact shared budget identity. Preserve immutable pin creation, winner/CAS recovery, lexical paths and first-error/checked-close order.
2. **Data stream through CLI:** parse a typed request in the compatibility boundary, then admit, resolve the pin, prepare and walk rows through a typed event sink. Keep fields validation at its established post-prepare point. The row executor receives concrete projection/filter/limit and budget, borrows one row at a time and uses the synchronous sink as backpressure. One wire renderer preserves JSON/JSONL numbers, keys, flushing and partial-output semantics. Capture and checked finish precede End; failed sinks are never called again. Broader Scope request migration is a later composed change, not an invented temporary Invocation.
3. **Live completion through CLI:** replace optional-result/optional-error combinations with Completed, Failed and ReportedWithFailure. Joining cleanup consumes the previous outcome and moves its report, preserving ordered nested causes. Connection/activation/doctor and CLI project the same public records. Keep single-writer input ordering, fresh authorization after waits, persistent uncertainty and explicit checked close; concurrency must not reorder transactions.
4. **Source engine host:** centralize the Lua 5.1 base configuration and Source's eight-family diagnostic policy, with complete serialized configuration/identity equivalence tests. Public Emmy AST/Analysis/FileId/config types are legitimate within the engine host and adapter ingestion. Owned business records and worker messages do not borrow engine lifetimes. Do not manufacture an AST mirror or copy the Source Graph merely to hide an engine type. Upstream remains unmodified.

CLI remains the compatibility/wire boundary rather than propagating flag maps and untyped frame inspection into new row executors. Delivery and Skill remain thin, with legitimate Live-to-Delivery controlled installation dependencies; removing that dependency would duplicate lifecycle logic. The current libraries are sufficient for these migrations. A new library must have a concrete capability/cost advantage, reviewed source/license/unsafe surface and a maintainable upgrade test, rather than satisfy an abstract preference for either no dependencies or more dependencies.

Source's measured cold-cache work is a separate concrete concurrency opportunity: bounded scoped parsing workers, stable sequence slots, shared in-flight byte admission and ordered coordinator publication. Maintain original content checks, first error, cancellation and output order. Evaluate Data batch parallelism only where independence and useful measured cost exist; preserve required Live serialization. These changes and the whole-repository ownership/dependency review are implementation work. They do not count as completion of the five final audit loops or as performance acceptance.

Source original documents use one demand-owned lifetime rather than eager duplicate persistence. Query metadata serialization borrows the authenticated Index; cold original buffers move into the projected document set, while skipped Facts and unselected symbols are released. Empty bytes are distinct from absent bytes. A search/context request borrows cold buffers or owns validated cache bytes, populates the same disposable cache only when consumed, and shares a lazily opened fixed Git reader. The reader authenticates the pinned commit and each tree edge before the demanded blob, has one in-flight frame, and reuses the existing Child/Job/WorkerSet retirement model. It does not prefetch later documents or replace first consumer errors with later transport errors. Original capacities and a capped tree memo share the existing source-text allowance; bounded frame/cache scratch remains separate. Final Index/capture durability and ancestor-reparse checks remain unchanged. Removing eager document publication shifts work to actual consumers, so cold-index→precise/broad and independent cold/warm query must all be measured; no index-only timing proves full-workflow improvement.

### Bounded offline Live measurement

Measure the real Driver, durable journal, protocol codec and consuming terminal projection through the existing authenticated Go323 Lua peer. A dedicated cfg(test) ignored case uses 27 fresh fixtures: three result-string byte sizes (256/16384/131072), original journal histories (1/255/256), and three repeats. Actual BODY byte length is recorded separately. Real monotonic clocks cover complete sample setup, history construction, execution, probes and checked cleanup; ordered disjoint spans plus remainder reconstruct each wall interval. Synchronous Driver journal/backend spans replace their parent total when explaining costs; nested Lua exchanges and record decodes are not added again. Synthetic protocol clocks are never elapsed evidence.

The benchmark-only entry authenticates the actual optimized library-test executable, fixed fixture/runtime and owned process receipts. It does not require an unused CLI build. The normal nine-case bridge retains its existing interface and authorization. Exact generated Lua allowlisting excludes arbitrary input. Real journal checkpoint archive bytes, all six ordered actions, durable-result-before-release, completed retry without replay, codec/state/public projection equality and original checked-close outcomes remain assertions. Code-provable copy/capacity counts are reported separately from unknown allocator/RSS totals. This diagnostic is not a Go-host speed comparison or game latency, and it cannot close B06 or the final audit loops. See [the bounded runner contract](tests/rust-performance-paired/live.README.md).

### Live authenticated-chain reuse within a journal lease

The execution lease excludes cooperating writers, but it does not prove the log or owner file was not externally changed. Retain the original owner check and complete bounded read with checked close on every append. Only byte-identical content can reuse a previously authenticated event count and chain head. Changed content uses the original complete parser, checksum chain and predecessor conflict checks, preserving corruption and owner-error precedence. Event preparation returns the computed checksum with its bytes instead of reparsing the event just generated. Successful write, durable sync, checked close and final control check must all precede advancing either the head or cache; uncertain writes retain poisoning semantics. Checkpoint, historical/load paths and close explicitly invalidate or rebuild eligible data rather than silently trusting metadata.

Caching is optional and uses the existing 8 MiB checkpoint-size threshold for eligibility; it does not reduce the valid 64 MiB log limit. Cached observable capacity is at most 8 MiB. While that cache remains active, a new raw-read allocation requests at most 8 MiB; mismatched length, failed eligibility or read growth evicts before new allocation/growth. An allocator-returned capacity above eligibility immediately disables retention. Post-allocation eligible raw-pair observations are at most 16 MiB. These are request/capacity facts, not an instantaneous arbitrary-allocator or whole-request/RSS bound; original parsed event/state allocations remain separate. Larger valid logs use the original full path. No new generic budget or allocator framework is introduced.

The measured 0890 baseline places 70.81% of Driver elapsed in 1,215 appends. A cfg(test)-only diagnostic splits that append interval into ten disjoint scopes plus remainder, and records actual hits/replays/read/compare/capacity observations. It activates only after fixture-history construction, preserves old 27-sample reports, and never changes public wire output or adds production clocks. Original same-input full diagnostic and failure regressions must validate any performance claim; the implementation remains linear in bytes read/compared and is not O(1) append.

### Call-local borrowed journal artifacts

Durable snapshot persistence uses an internal borrowed artifact view: valid code bytes and raw result bytes borrow the caller's State for that one call. Invalid code UTF-8 still receives the original one-replacement-per-invalid-byte normalization; its owned buffer moves into the first digest entry without another copy. Raw result bytes are never normalized, including non-UTF8 payloads. The public Snapshot API remains an owned compatibility wrapper. Metadata/INPUT normalization, first digest ownership and sorted publication order are unchanged.

The journal retains no artifact reference across the call. It still verifies the current owner and completely reads/compares each existing artifact with checked close; first publication remains durable before state append. No path, digest name or pointer substitutes for detecting external changes. The 1360 baseline counted 1,647 nonempty temporary artifact vectors and 30,736,260 copied payload bytes across 27 cases; that is a concrete ownership cost, not a whole-allocator total or predicted wall saving. Optional cfg(test) counters report the final append snapshot map only, excluding setup/checkpoint and dropped duplicate normalization buffers. Existing full timing and the original reports remain valid.

### Data digest screening before backend changes

Use the existing Windows example to isolate the authenticated 44-byte Root and 17,825,792-byte payload as separate messages. At 64KiB, payload has 272 updates and Root one; do not turn a profile's aggregate273 into a single synthetic message. Compare current RustCrypto incremental clone-finalize, consuming finalize and fused SHA256/MD5 full state lifecycles with complete digests and retained individual samples. The existing safe CNG whole-slice API remains a separately labeled historical comparator; it does not expose equivalent caller-owned incremental state. No production backend, dependency, FFI or private control contract changes merely to produce a benchmark. Native process wall/CPU, setup/reference checks and per-sample monotonic spans have distinct boundaries. See [the screening contract](tests/rust-performance-paired/md5-benchmark.README.md); any production optimization still requires full Data equivalence and end-to-end measurements.

### Final Rust-native architecture audit

The user added a fifth sequential final stage on 2026-10-16. Preserve the first four stages (architecture/functionality/Skill, coverage and redundancy, dead code, combined audit), then audit the entire repository's Rust-native design. Inspect concrete crate direction, ownership and borrowing, useful typed states, clone/allocation costs, streaming, CPU/task/byte admission and backpressure, cancellation/error/RAII lifetimes, and minimal unsafe boundaries. Repeatedly fix, measure/test and independently re-review until no new actionable finding remains on the reviewed tree. Use existing good designs and mature libraries where justified; superficial syntax changes, needless abstractions, reduced checks and unmeasured concurrency do not establish success.

CPU parallelism is an operation-level policy bounded by available processors and the existing in-flight byte budget. Use process-available logical parallelism rather than a hard-coded CPU count, then constrain actual workers by useful independent tasks and the existing byte allowance. Small work stays inline; one operation-owned team handles multiple tasks. Source's present 32 MiB scratch + 4 MiB Facts + 2 MiB stack per active worker admits at most about six full tasks within 256 MiB even on larger machines; this is an actual resource consequence, not an arbitrary CPU cap. Data's default verified texture decoder uses borrowed disjoint RGBA strips for independent palette/BGRA/BC pixel computation. It selects useful participants from logical CPUs, row/pixel work and an atomic reservation of existing private headroom; no new adaptive growth, input copy or output queue is needed. One scoped team handles multiple rows, joins in image order and refunds only after every worker has ended. Small work and unavailable worker capacity use the complete serial decoder; source authentication, SQL streaming cursors, capture/transaction order and cancellation stay in their original owners. The f088 affected Rust groups passed 40 tests with one original fixture-preparation ignore, including the new Source/Data parallel regressions; performance and memory acceptance remain pending. Neither a CPU count nor task creation proves throughput or memory acceptance.

### Generated Facts: borrowed views and bounded pure tasks

Generated Lua documentation uses borrowed fixed-key views, preserving first-match masking even when the first value is invalid; it does not adopt XML attribute last-value semantics. Explicit depth-first continuations preserve the original seven-section/root order and borrow live ancestor namespaces by index. One signature buffer has explicitly chosen capacity requests rather than vectors of temporary strings. The complete generated pass shares the existing worker guard and ordered fallback; no partial worker Facts can publish. Input, scratch, retained Facts, shared in-flight bytes and default worker stack limits remain unchanged. Transparent parentheses retain the outer source start while descriptor traversal uses the enclosed semantic table. Declaration end positions describe the actual table body ending, rather than reconstructing a line-keyed brace queue; any observed difference from the fixed Go end position is reported and tested explicitly as a compatibility enhancement. This does not waive complete comparison of the original single-line inputs. The 8cab actual observation verifies six cross-line cases whose fixed Go endLine is zero while Rust reports the real table end; all other fields match. These are explicit accurate-position enhancements with raw oracle differences retained, alongside five fully equal boundary cases and all 104 original strict inputs passing. The requested-capacity derivation and two-axis regression inputs are recorded in [the generated fixture contract](tests/rust-parity/source-facts-generated/README.md); they do not claim allocator/RSS bounds.

Owned linear Lua/XML scans use a new 256 byte-work-step observation interval, with entry/exit checks and sticky abandonment. Nested comparisons and expansion copies count their work; allocation charges and AST/event checks remain unthrottled. Explicit checks after scanners finish prevent a short-input final cancellation from escaping via a value computed before Drop. The interval bounds counted scan work, not wall-clock latency or the complete parser. This is a documented scheduling change rather than a claim that earlier per-byte clock calls were required by the public CLI contract.

### Private streaming decode and observable errors

The local and CDN verified publishers may incrementally inflate an already authenticated encoded Z chunk into their private unpublished spool. They retain the original encoded-block MD5, exact decoded size, complete CKey and terminal source EOF verification before publication. A bounded 64 KiB decode buffer replaces the decoded-sized Vec; original per-chunk conservative retained admission remains until the chunk ends, so this does not claim a lower ledger peak. Successful read/work totals and the existing hard limits remain unchanged. Non-Z/nested/encrypted modes and the public eager Decode/unpack keep their original path, including the public contract that a corrupt current chunk writes no bytes to its destination.

Private staging adopts the first actually observed failure. A sink failure stops immediately; it does not drain a corrupt but unread tail or enter the next chunk to reconstruct the old eager error order. Thus corrupt-tail plus early private-write failure can return the sink error where the old eager route found the codec error first; tight work allowance plus malformed tail can also change which failure is reached first. This is an explicit internal scheduling/error-order adaptation, not a claim of identical double-fault timing. No partial spool is published, cancellation uses the original control, and checked cleanup retains real ordered errors. No work-reservation framework, secondary inflate or production publisher callback is introduced to imitate an unpromised private order.

The opt-in Data phase recorder v2 preserves all eleven original business phases and total elapsed boundary, adding a separate nested detail vector for encoded read/hash, inflate, whole-content hash and decoded-cache lookup/verification/publication. Historical v1 reports remain strictly readable as v1; details are not additive with parent phases and are not subtracted from end-to-end timing. Disabled observation adds no clocks.

### Private spool read-ready ownership

A same-command intermediate spool is not a recoverable or published object. A dedicated `PrivateSpool` owns the original staged handle, exposes no publish/replace/Deref/inner-file escape, and transitions once from writes to rewind/read-ready. Reads remain bounded by the existing caller buffer and content length; writes after the transition fail. The private transition does not request `sync_all`. Its handle is copied and SHA-verified into a distinct final-shard stage, whose original durable seal, atomic publication, winner authentication and checked close remain unchanged. Every business capture retains its own durable transaction. No private spool is reused across commands or offered as a crash-recovery checkpoint.

This explicitly changes private error observation: an error observable only from the omitted private-file sync is no longer requested. Actual write/read/seek/final-stage sync/publication/close errors remain failures with checked cleanup. Final-file durability is not a claim about survival of the discarded intermediate. The owner type prevents accidentally applying this optimization to a publishable final stage; no new FFI or unsafe is required.

Opt-in Data profile v4 splits the old combined stage seal into `private_spool_read_ready` and `final_stage_seal`, retaining the eleven business phases and ten decoder/cache details. Historical v1/v2/v3 vectors keep their exact versions. Successful winner-open and final-verification spans are disjoint; failed cleanup may remain inside its observed parent. Publisher details are never added again to business parent times. Disabled profiling performs no clock reads. The old v3 four-seal total of 30.570ms is only an upper envelope for the affected private work, not a promised saving; matching product/end-to-end measurements are required.

### Rust safety boundary

Owned business and cross-platform Rust code defaults to no unsafe, enforced with suitable crate-level forbid/deny. Unavoidable Windows FFI/memory operations live behind minimal safe wrappers with per-site SAFETY arguments, pointer validity, ownership/lifetime/thread requirements and focused tests plus independent audit. Do not exempt entire modules indiscriminately or introduce unsafe to chase the performance target. Third-party dependencies require a separate inventory; this policy is not a claim that the dependency graph contains no unsafe. Reuse the fixed Go3.2.3 Skill workflow, changing only necessary Emmy/install/diagnostic details.

## 3. Source: preserve and finish the direct-library path

Current source candidate: production uses the exact unmodified 848-file upstream tree (tree SHA256 `375d92c5d5e503d77232fabf22a21a82f504c2b38dc5def8fcca1d6ecf06e104`), public batch ingestion, and owned contextual queries plus the complete 43-checker scheduler. The remaining acceptance work is compilation, full historical-reference/clean-candidate semantic comparison, full Retail/session/resource behavior and performance. Historical patched reference workspaces are test-only; no patch is applied to the production engine. The migration inventory below preserves earlier decisions and measured counts, not permission to restore production hooks.

**Current hard acceptance requirement (2026-10-15 13:39): zero changes to Emmy upstream source.** Replace every remaining production hook with owned-crate implementations using the unmodified, fixed upstream public API. Preserve all approved XML/Lua enhancement, type/cardinality/diagnostic/coverage, source identity, cancellation and resource contracts. Earlier permission to retain necessary minimal hooks is superseded as a final-delivery criterion. Existing patches are transitional only. Maintain a complete hook→consumer→owned replacement→differential/full-Retail evidence matrix and complete end-to-end implementations before consolidated validation; do not replace the whole language server or reduce behavior to achieve an empty patch. A demonstrated unresolved API gap remains unfinished work requiring concrete evidence, not an automatic exception.


Keep `emmylua_code_analysis` / `emmylua_parser` linked into the native worker. CLI/worker/session ownership remains explicit: bounded requests, fixed inputs, deadlines, cache identity/invalidation, crash/cancellation containment, cleanup and resource release. The worker may be a mode of the same executable; a subprocess is not proof of LuaLS or LSP. Never execute studied Lua or repository configuration.

Use the verified public upstream archive without source patches. Pin upstream revision/archive and complete tree hashes, licenses and corresponding source; keep the fixed historical patched reference separately identified for tests. Never label new bytes as the missing historical fork or forge its custody hash. A version string alone is insufficient. Clean rebuild and offline corresponding-source checks must precede product claims. Full fixed Retail corpus analysis and continuous cold/warm/session queries are required in addition to focused fixtures.

Separate engine internals from business facts: query/definitions/refs/calls/context/flow, XML/TOC closure, batch ordering and errors, static-only fallback, complete/coverage/truncated/cursor and uncertainty must match the release's business contract. Revisit existing long-running Retail failures without silently widening deadlines, quotas or reducing source closure. Legacy LuaLS feature code is a removal candidate only after feature/call/package audits; luac 5.1 validation and tree-sitter structural work have separate responsibilities and are not automatically dead.

### Owned Emmy adapter crate and upstream maintenance

The narrow owned `lycheedev-emmy-adapter` crate is implemented; extend its existing boundary incrementally. This is an implementation boundary, not a new Source protocol or a claim that an upstream wrapper removes all patches. Dependency direction is `source -> lycheedev-emmy-adapter -> pinned emmylua_code_analysis/parser`; the adapter must not depend back on Source Graph, CLI, Live or Windows. Keep XML/TOC interpretation, Graph construction, request/session ownership and file authentication in Source. Pass small borrowed, immutable descriptors or concrete engine types across the boundary; do not copy Graph or introduce a generic provider framework just to move files. Own code forbids unsafe.

The 2026-10-15 independent audit of patch `f174b3c70bd236a619906c7eb3598ef950db7009424dd1b144a9e22d91012a5f` found 29 upstream paths, +4105/-226 lines: 2248 added production lines and 1857 added lines in three files containing 35 tests. This is a measured patch inventory, not a measured performance result. This is the historical pre-extraction inventory; the later binding-contracts slice below records the delivered reduction.

| Boundary / audited paths | Direction and required proof |
| --- | --- |
| `binding_context.rs` Environment DTOs; `semantic_model/binding.rs` descriptor-to-type projection and domain provenance | First externalization candidates for the owned adapter. Preserve authenticated slots, aliases, receiver/unknown rules and budgets using public engine constructors; avoid a circular Source dependency. |
| BindingSession references/report DTOs | Production ordinary refs/definition/flow already use Source Graph/Syntax. Map true production, example and test consumers; move reporting/query DTOs without dropping useful coverage. |
| `lib.rs`, `semantic_db/mod.rs`, `vfs/mod.rs` shared Arc batch ingestion | Separate generic upstream extension, not WoW business logic. Preserve shared allocation, per-file cancellation and single rebuild; ordinary public String ingestion is not allocation-equivalent. |
| `semantic_model/mod.rs`, `access.rs`, `infer/vm.rs`, `types.rs`, `flow.rs` | Migrate recursive/cross-file contextual inference to owned semantics and public engine queries. The historical hook cannot be retained for final acceptance; prove child inference/model_for and unavailable behavior without raw fallback. |
| `complex.rs`, `member.rs`, `type_check/mod.rs`, `infer/function_solver.rs`, reconstruction/render paths | Preserve session token/epoch/object identity and incomplete-shape rejection. Owned identity mapping must prove the same contextual identity; replacing it with an unverified structural key is insufficient. |
| signature/cardinality and five enabled diagnostic checker paths | Preserve zero versus unknown returns, exact output arity, no raw-token fallback, and honest executed/skipped coverage. Filtering final diagnostics is not equivalent. |
| assignment/generic-constraint checkers | Default configuration does not enable them, but file-level diagnostic opt-in can. Establish that contract before calling them dead. |

Production contextual entrypoints are `native_semantic.rs` diagnostic/coverage and `native_research/plan.rs` authenticated API type inference. Experiment with upstream public declaration/signature/doc-type APIs for the latter only after matching aliases, methods/receiver, nilable, unavailable and zero/multiple/MayReturnNothing outputs. Do not return a descriptor fallback while labeling it successful engine inference. Keep the existing 35 engine regressions and add owned-adapter tests; moving tests out of the patch must not reduce coverage.

For every upstream update: pin the new commit/archive/license, retain the prior recipe, map each remaining patch to a real consumer and regression, separate clean application from semantic acceptance, and record conflicts or upstream replacement APIs. Remove each patch only after equivalent owned/public-API behavior is verified; final acceptance requires no upstream patch. Rebuild the candidate and run the 35 semantic guards, owned-adapter fixtures, full fixed Retail indexing/query/session/diagnostics and relevant resource checks before changing the production pin. Preserve old failures, cache identity changes and reproducibility/corresponding-source evidence. A wrapper reduces coupling; it does not guarantee conflict-free upgrades.

### Transition from minimization to complete upstream-patch removal (2026-10-15)

Prefer the unmodified pinned Emmy public APIs and owned XML/business crates whenever they preserve the complete enhanced contract. Audit every remaining patch separately: public-API replacement or owned-layer implementation, with each remaining transitional hook assigned a documented consumer and replacement regression. A historical implementation or passing patched-engine test alone does not prove that an upstream change is necessary. Treat session/epoch management, source maps and refs provenance as candidates for owned-layer implementation rather than automatically classifying them as engine hooks.

Use the independent unpatched-engine capability matrix and isolated small examples before removing any hook. Thin virtual Lua projection must not silently weaken nil/callable, parameters/returns/generic constraints, cross-file self/identity or executed-versus-skipped coverage. Replacing the inference/checker semantics with an owned implementation would be a substantial rewrite, not a free plugin boundary. No dynamic plugin framework or new language server is implied by this direction. Keep the current complete design, move small proven slices, and compare actual full-Retail output and resources after each change. Public API compatibility still needs upgrade tests; zero upstream patches is now required but has not yet been achieved; painless upgrades are still not promised.

### Retained research evidence ownership

A completed immutable Graph owns one lazy shared control projection; its private facts have no post-construction mutator or mutable dereference, and every constructor starts with an empty cache excluded from serialization. Direction-specific analysis metadata remains separate. References answers, optional retained rows and accumulated scope evidence share the same complete control value until Graphs are released, then expand the unchanged public JSON. Retention and output budgets serialize and count the full shared evidence, not pointer sizes. Hashing streams the same canonical JSON bytes directly into SHA-256. Immutable declaration lookup permutations borrow keys from the original declaration vector; XML documents reuse already-authenticated shared originals only when byte-identical, leaving unrelated owned strings untouched. These ownership changes require complete output and default-budget runtime verification; they do not establish a memory or performance result.

Scope identity uses a borrowed canonical XML Program serializer and a bounded streaming JSON hash instead of materializing an intermediate tree and byte buffer. A Graph retains shared snapshot evidence; its original construction point still validates and counts the entire serialized form and computes the same hash. Only the unused snapshot string is delayed until an actual serialized/explicit reader requests it. Lexical and prepared closure/parameter indexes use exact admitted file coordinates rather than duplicated path-containing strings; preserve the former file:start:end equality and last-write rules, including BOM offsets and public cell/capture IDs. Complete bytes, errors, accounting and repeated-scope ownership regressions are required; allocator commit high-water is not identical to live object ownership.

### Retained owner reply ownership

The authenticated analysis child remains the producer of the complete typed response. The IPC owner validates the complete bounded frame and typed envelope, but does not build, clone, serialize or recursively drop its business `Value` tree. A normal successful response moves one raw frame through the existing single in-flight exchange, join, forwarding and retirement paths. Full JSON grammar/tail, Unicode, numeric semantics and the original nesting admission remain checked; wire/token/process creation/owner identity and request custody are unchanged. The final CLI remains the public result consumer.

Pinned serde private RawValue objects can genuinely occur inside API Event payload metadata. Only those subtrees use an explicit heap-task normalization step to preserve the former owner-then-CLI interpretation, including its second-consumer failures; ordinary producer bytes remain untouched. Arbitrary noncanonical object order that the authenticated child `Value` writer cannot emit is not a new input contract. Error details stay borrowed as raw spans while original primary, retirement failure and terminal evidence are combined. No complete recursive DOM is reconstructed in the owner, including when retirement discards an already-completed exchange.

The existing 32 MiB wire limit is not an RSS guarantee. Ordinary validation has scalar scratch and bounded container state; private-marker normalization can temporarily retain the input frame, a bounded replacement frame and a decoded subtree/key-range table. Error rewriting can hold input plus a bounded output frame. These real allocations share the existing aggregate 2 GiB process-tree limit; no stack size or request deadline is increased. Default-stack and full-Retail validation are still required before claiming the observed owner stack failure is closed.

### Owned Emmy adapter and retained enhancement contract (2026-10-15)

The released Go Source engine starts official LuaLS 3.19.1 through LSP; fixed `internal/codebase/semantic.go` explicitly leaves XML inline scripts and injected frame globals NotChecked. The Rust contextual XML/Lua design is therefore an additional capability, not proof of Go parity. The user explicitly chose to retain the complete design while reducing coupling to Emmy. Preserve its observable behavior and uncertainty guarantees; do not remove enhancements solely because Go lacks them.

The implemented `lycheedev-emmy-adapter` crate owns this boundary. Source owns Graph, XML/TOC ingestion, source/session orchestration and business contracts; the adapter owns engine configuration/session entrypoints and neutral diagnostics/type projections. Dependency direction is Source → adapter → pinned Emmy. Do not make the adapter depend on Source, duplicate Graph, or create a generic plugin framework. Real production diagnostics/API-type consumers are connected to owned queries rather than merely re-exported Emmy symbols. DTOs, environment conversion, provenance and reporting stay in the owned boundary and are validated together in consolidated tests.

The initial public reconstruction modified 29 upstream paths (+4105/-226 lines, including 1857 test lines); those counts describe that historical recipe. Subsequent callback guards and owned extraction have separate exact manifests. That historical transition retained internal hooks for recursive/cross-file context propagation, session/epoch object identity, managed-unavailable versus absent/not-managed, return cardinality and real diagnostic completion. Those responsibilities now belong to the owned candidate and must pass the same semantic acceptance; they do not authorize production patches. An outer query wrapper or filtering final diagnostics cannot replace these hooks. The shared-Arc VFS ingestion/cancellation extension is a separate generic optimization, not a measured performance result. Two default-disabled checkers can still be activated by file-level diagnostic directives; absence from defaults is not consumer-proof for deletion.

For each upstream upgrade: pin the exact unmodified revision/archive/license; inventory public-API and derived-algorithm changes against their actual consumers, enhanced or Go contracts, and regressions. Port owned implementations without adding upstream patches. Preserve the contextual guards and full fixed Retail queries/diagnostics/session acceptance; compare complete outputs, uncertainty, sampled memory and cold/warm elapsed time. Keep the old test reference and failures. An unmodified engine still requires semantic compatibility work and does not guarantee painless upgrades.

### Engine-independent binding contracts

`lycheedev-binding-contracts` owns the existing engine-neutral decision/object-view/field-presence/shape-enumeration/environment records. Its current dependency direction is contracts → serde/serde_json, adapter → unmodified Emmy + contracts, Source → adapter. The upstream engine has no contracts dependency or compatibility re-export. Owned sessions carry the authenticated token/epoch/object and contextual result state; public engine types stay within Source/adapter engine-host integration, while business graphs and worker DTOs own their identities and spans. The following paragraph records the earlier transitional extraction, not the current production dependency graph.

The first reviewed slice moves 135 existing contract lines, reducing added patch lines from 4193 to 4071 (122 net). These are physical patch counts, not a claim that all remaining hooks are removable. All prior enhanced semantics remain. The new five integration tests passed locally through the controlled exact-source runner; the 36 engine regressions passed for exact ea364 in its independent recipe job, while the current 24 adapter regressions and full product acceptance still require their exact candidate execution. Owned-file identity inventories, both lock graphs, standalone/full-source staging, source custody and unsafe guards cover the new transitive crate. Future upgrades review these boundaries and retain or remove each hook only against its real consumer and regression.

## 4. Live: implement the complete released state machine

The canonical specification is [Go 3.2.1 docs/live-protocol.md](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/docs/live-protocol.md), alongside its Lua/Go implementation and tests at that commit. Protocol literal: `Lycheedev mailbox protocol v1`. Import/port complete cases into a Rust/Lua differential fixture suite; do not substitute the branch's old RUN/RECEIVED exact-COMMIT design.

Read-only memory lookup uses current mapped runtime instructions and RIP-derived roots, guarded current Lua paths and publication validation. Observation-only results do not authorize keys. Input requires exact target/actor/runtime/owner/fence, fresh INPUT, actual two bindings and revalidation immediately before sending. Native input is bounded PostMessage to the fixed HWND; no Esc, Enter, chat typing, focus stealing or blind fallback.

Persist intent before input, preserve submitted/uncertain outcomes, exact request/ticket/report identity and original deadlines through resume. Reports, business success and cleanup are distinct. Cleanup recovery, upgrade compatibility, BootstrapMailbox diagnostics, missing outcomes, publication races and multi-instance shared-install locking are acceptance requirements, not later optional hardening. Full case matrix lives in handover.

## 5. Performance design and evidence

Optimize reuse, indexing, allocation, bounded parallelism and native worker/session costs only after equivalent output and validation are established. The per-path target is controllable Rust elapsed ≤ 0.30 × Go. Define paired phase boundaries before measurement. Network downloads and uncontrollable external waiting are excluded from the ratio but remain visible in raw wall-clock evidence; local parsing, validation, persistence, cleanup and controllable scheduling remain included.

Record independent cold and warm repetitions, interleave Go/Rust on the same machine and inputs, and preserve raw timings, median, p95, CPU, peak memory and I/O. No cross-path average can hide a failing path. Incomplete, failed or unmeasured paths are not passes. The plan's exact collection/report protocol is in handover.

### Source, Data and Live end-to-end optimization requirement (2026-10-17)

The user explicitly requires sufficiently detailed cost attribution for all three modules, including algorithms, data structures, architecture and bounded scheduling. The acceptance clock remains the complete observed request/process lifecycle, not a selected inner bucket. For each exact run, present mutually exclusive measured wall-clock intervals plus named unmeasured remainder; keep nested inclusive timings and summed parallel-worker durations separate. Never combine different commits/runs into a synthetic total, subtract framework costs from the agreed metric, or defer required work to the next query/background to manufacture a speedup.

Use real critical-path proportions to choose complete, attributable changes: eliminate repeated traversal/conversion and unnecessary cloning, use compact ownership and suitable lookup structures, specialize bulk operations, and parallelize genuinely independent work under existing memory/cancellation/order constraints. Source publication and sequential parsing are the current measured priorities; Data uses its same-call business phases and native startup proof. Live begins with existing offline protocol/driver/journal and self-owned fixture boundaries; synthetic clocks/input do not establish real game latency. Keep real-game acceptance last and do not operate games. Reuse the existing scoped profilers and add only missing necessary intervals, not a global observability framework. Validate unchanged full outputs/recovery/resource contracts and improved complete walltime and memory; the five final audits remain separate.

## 6. Cleanup and delivery

Inventory dead code/doc candidates; prove absence of production/feature/test/package consumers, identify replacement coverage, then remove in reviewable batches. Preserve immutable historical evidence in git/history and label superseded documents. Do not delete caches/evidence/user data opportunistically. Follow existing owned-process/disk admission tooling.

Windows amd64 Rust executable, the same addon, generated CLI/Skill references, launcher, licenses and corresponding source must refer to one sealed candidate. Test install/upgrade/rollback/modified-file protection without games first. The first commit remains documentation-only; full implementation is subsequently authorized. Each batch updates the same draft PR. After implementation, execute handover’s five serial audit/fix/verify/review loops to convergence, recording fixed-tree evidence and human-dependent exceptions. Release/publication and main merge require separate authorization.

### Rebuildable Facts publication without a standalone lock file (2026-10-17)

The Facts-only candidate uses an exclusively created, privately owned stage and the existing same-directory atomic replacement. Inputs and cache reads were already unlocked; the removed per-key lock serialized complete publication bytes, not parsing. Preserve same-key/different-body last-writer semantics and the existing rare NotPublished equivalence fallback. Originals, whole-index and metadata locking remain independent.

The 64 fixed stage names bound active writers plus unknown/foreign residues. A name collision or Windows sharing violation acquires no ownership and may advance only within that finite set; other failures remain errors. Never reclaim another slot by age or PID. In that original per-file revision, full writes, original cancellation/deadline, checked close, Published/Unknown reporting and per-document successful checkpoints remain. Readers holding the old file continue reading its complete snapshot across replacement. That revision introduced no new cache format, locator, quota or GC policy; the subsequent bounded cohort design below changes only the rebuildable Facts storage granularity. This is atomic filesystem publication without an explicit lock file, not a lock-free/wait-free progress guarantee. Runtime and performance acceptance remain in handover.

The Windows writer-close window requires bounded coordination even without a lock file: an earlier published writable Stage still denies delete sharing until checked close. Preserve that sharing policy so a private stage cannot be moved away from its authenticated parent. Under the normal synchronous Windows filesystem status contract, only exact native STATUS_SHARING_VIOLATION maps to a cache-only SharingBlocked/NotPublished attempt; retain the sealed owner and original absolute control, wait at most 25 ms per retry, then revalidate and try the same handle. All other failures, arbitrary converted error 32 and uncertain completion remain terminal Unknown. Ordinary/durable Stage APIs retain their previous error semantics; this does not authorize general I/O retry or extend any request deadline.


### Bounded rebuildable Facts cohorts (implemented; runtime verification pending)

The next Source-only storage boundary groups at most 64 eligible documents in
source order. A cohort key is derived from their authenticated path/object/size
identities and the Facts format identity. Each pack is at most the original
32 MiB bound. Fixed headers and complete per-document envelopes are streamed
into one private stage; there is no second pack-sized aggregation buffer,
mutable global locator, background writer or new GC subsystem. The final index
stores a fixed-size locator for each packed document. Locator/cohort capacity is
charged alongside existing worker and retained state under the same 256 MiB
admission cap.

Only a confirmed publication may supply locators to the final index. Unknown
publication, cancellation and checked-close failures keep their existing error
meaning. A group that reaches the pack byte bound publishes its completed
prefix and uses the existing per-file path for remaining records; a record
exceeding the original per-file limit still fails with the original budget
error. XML/TOC and other default consumers use authenticated locator ranges,
validate header/key/length/hash and the complete original Facts envelope, then
fall back to authenticated Git bytes and the original parser/per-file repair
when a rebuildable record is missing or corrupt. The index commit and original
source authentication boundaries remain authoritative.

This deliberately changes an internal optimization checkpoint: completed groups
are reusable, but an interrupted unpublished group may need recomputation.
With unchanged eligible membership and order, changing one member may cause
at most 63 otherwise unchanged peers in that cohort to be rebuilt. Inserting
or deleting an eligible document can shift later ordinal cohort boundaries
and invalidate more groups, potentially the remaining corpus; this broader
cross-snapshot cost must be measured and reported, not described as a global
63-document bound. No public command promises a
persistent checkpoint after each parsed document; no source or captured business
data is lost by this change. Record the cache tradeoff explicitly rather than
claiming zero migration cost. Keep the historical per-file readers and evidence;
new diagnostics select the pack layout explicitly and bind the exact index blob
by SHA, never infer a locator from an arbitrary directory scan.

The held-directory verification policy is unchanged. This design reduces the
number of stage/create/replace operations instead of omitting ancestry checks.
Concurrency, malformed/range-corrupt caches, cross-snapshot reuse, partial-group
failure, cancellation, default consumer reconstruction and memory accounting
require formal tests before runtime acceptance or a performance claim.

#### Stable path partitions for incremental cohorts

The first ordinal-pack implementation (`b3d`, pack1) passed full Facts comparison,
but actual insertion/deletion invalidated all 4,037/4,035 documents. Its cold
benefit does not establish incremental performance. The replacement partitions
eligible paths by a deterministic binary radix tree: maximal prefix leaves hold
at most 512 documents, and each leaf is consumed in original order as packs of
at most 64 records. Path terminators participate in the prefix ordering; UTF-8
bytes and original Git order remain authoritative. Content does not choose the
leaf, while the complete path/object/size identities still choose each pack key.
The implementation uses bounded scalar scans and the existing 64-index scratch,
not a materialized trie, global lookup catalog or extra pack body.

For a healthy, usable previous cache with only the stated membership change,
insertion affects at most 513 candidate documents, deletion at most 512 surviving
documents, and rename conservatively at most 1,025 across its removal/addition.
A deletion may collapse several trie ancestors, but the highest newly eligible
ancestor has at most 512 survivors; it does not invalidate an arbitrary suffix.
An unchanged-membership content edit still affects at most one 64-record pack.
These are membership-induced recomputation bounds, not guarantees against
independent eviction, corrupt cache, failed publication, changed eligibility or
first-time format migration. Existing byte-cap overflow/per-file fallback does
not enlarge affected pack membership.

Static planning on the authenticated public 4,036-path corpus predicts 74 cold
packs versus the historical 64. Smaller leaf limits 64/128/256 yield 155/103/87
packs. The observed test's first-path edit, root insertion and first-path deletion
predict 9/1/8 recomputations with the selected partition; these are specific
static results, not general bounds or measured product speed. The extra cold
publications and real change/insert/delete/rename costs require matching-runtime
measurement. Independent complete-trie and default-consumer regressions must
cover skipped assets/oversized documents at exact pack/leaf boundaries as well
as split, merge, cancellation, corrupt groups and concurrent publishers.

Use a new `facts-pack-v2-path-radix512` key domain and `rust-index-v14`
fingerprint to prevent old ordinal index reuse. The pack wire magic, locator
shape and complete v11 envelope stay unchanged. Explicit `v11-pack2` diagnostics
bind the authenticated candidate and exact index SHA; a naked index blob does
not identify which grouping algorithm produced it. Historical `v11-pack1`
readers and raw reports remain available, with their original identities.


#### Cache-aware ordered pure-parse admission

The authenticated 00d/e475 incremental observations still consumed roughly3984
pure-parse worker results despite only1–9 cache misses; main-thread wait occupied
2.06–2.09 seconds. The current owned pipeline uses metadata-only admission to
avoid that work. Within one cohort, a bounded64-bit cursor tracks confirmed
misses; only those enter the existing bounded pure worker queue. A possible
packed/legacy hit, non-worker row or cohort boundary drains prior work and then
uses the original ordered consumer for full content hash, envelope, mode,
retained-budget and encoding checks. No decoded cached Facts are queued or cloned.
Legacy presence readers checked-close immediately; consumption rereads the
current complete file including EOF, so replacement/growth cannot turn a hint
into trusted content. Deferred cache I/O errors bind the absolute document ordinal
and are returned only after that row's original prelude; earlier consumer failures
remain first. Corrupt records retain the original ordered parse fallback.

This changes scheduling only: pack2/v14 bytes and identity remain unchanged.
Cold misses remain parallel inside a cohort; cross-cohort parse/publication overlap
is deliberately narrower. Both cold and incremental native wall must be measured;
zero worker results on complete hits is a structural assertion, not a speed claim.
See [same-row attribution](docs/toolkit/source-pack2-incremental-attribution-2026-10-17.md).

#### Borrowed canonical whole-index persistence

The measured ba0 incremental IndexSerialize span remains465–596ms after eliminating
unused pure-parse work. The default writer now projects borrowed Index fields
straight into the same complete byte Vec, avoiding a second whole-index Value
DOM and its later destruction. Private wire wrappers preserve lexical object keys,
array order/duplicates, existing optional omissions, exact locator hex, and remove
only summary's own io field. Existing Value subtrees stay borrowed; supported
Number constructors/parser and pinned formatter preserve historical numeric bytes.
The hidden unchecked-number constructor is not a supported Index producer.
Public Serialize implementations and cache fingerprint/layout remain unchanged.

Do not stream around admission: encode the complete Vec, then use the original
128MiB publisher cap, content hashing, manifest update, lock and checked-close
sequence. This slice does not change Facts reading or warm index loading. Ten
formal regressions compare full historical to_value/remove-io/to_vec bytes,
including real cold/warm persisted blobs and downstream failure contracts.
Static review is not execution; default-stack Windows tests and matched end-to-end
measurement remain required. A reduction may also affect profile_other because
the old DOM was dropped outside IndexSerialize; judge complete native wall.


#### Authenticated Facts ownership transfer

After the unchanged envelope schema, input SHA and canonical whole-Facts digest
checks, both default loose and packed decode consumers move the existing Value
subtree into typed Facts rather than cloning it. get_mut/map(Value::take) leaves
missing keys absent and supplies the same Null miss value without inserting a key.
Keep both original call-site control checkpoints and fallback paths; do not merge
them into a helper that adds a miss-path check before parsing. Packed outer
header/body verification and checked reader closing remain unchanged.

This removes only the authenticated subtree clone. JSON parsing, canonical hash
serialization, typed vector allocations and full reads remain. The entire
FactsRead span is not a savings estimate. Nine formal regressions retain clone
reference bytes, all five arrays, Unicode/optionals/duplicates, deep unknown fields,
number spellings, missing/null/malformed inputs, cancellation/expiry, real loose
and pack/legacy consumers, and inner corruption with a valid outer checksum.
Cache version/layout and original bytes do not change. Windows execution and
matched end-to-end measurement of this slice are pending.
