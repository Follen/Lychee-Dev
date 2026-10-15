# Rust migration design

Current decision/status authority: [handover.md](handover.md). This file specifies the target, not completion. Keep this exact uppercase filename; do not add `Design.md` alongside it on case-insensitive Windows filesystems. The former workbench-only design is preserved in [history](docs/history/workbench-ui-design-2026-09-28.md).

## 1. Contract and migration strategy

Go tagged baseline `0bec60e4406f5d5dc155ee25cef44f9a710e37e8` (v3.2.2) defines current observable behavior. Prior v3.2.1 `10b561db5706702cd42e047eaeed4ccb5e825351` and v3.2.0 `3057a9666ab078270b63c7f06de748069b0d1f4b` remain historical test oracles; no existing result is relabeled. Compare each Rust module against this immutable source and classify retain / adapt / replace / missing / unverified. The Rust snapshot is an implementation starting point, not a second behavioral oracle. Tag/main identity is verified; release assets remain unverified.

The allowed implementation differences are Rust host code and EmmyLua direct-library analysis replacing LuaLS. Preserve equivalent Source business semantics and honest engine/coverage provenance; engine-specific implementation metadata must accurately name EmmyLua, never impersonate LuaLS. No other compatibility exception is silently grandfathered from old plans. Any unavoidable business difference requires a separately approved decision before acceptance.

### Independent candidate package identity

The user selected Rust package version **3.5.0** and the **beta** npm distribution configuration, while Go **3.2.2** remains the behavior oracle. The version is not `3.5.0-beta.1`. Version synchronization covers nine owned crates, addon and npm metadata, owned dependency pins and reconstruction custody. Exact full-file release-field overlays permit only the approved Runtime/TOC version literals and restore the original Go bytes for comparison; historical oracle artifacts retain their identities. This configuration does not publish, tag, alter external channels or move `latest` from its retained 2.5.1 value.

### Tagged 3.2.2 managed-slot activation and recovery

Follow the [3.2.2 increment matrix](docs/toolkit/rust-go-3.2.2-increment.md) cumulatively with 3.2.1. For the current character GUID, enable only authenticated managed slot addons, verify changed entries and save addon settings; never load a slot or consume capacity merely to enable it. Preparation failure must not advertise ready. The input profile carries `slotReloadRequired`; compare it even when the timestamp is unchanged. Fresh pre-bind/Invoke observations journal the reload intent under the original CON, deadline, request and recovery transaction. A slot action is blocked while reload is required, but reload itself remains allowed. Repeated replacement runtimes resume the same operation; submitted/uncertain or missing-receipt outcomes must never be replayed. Continue handles recovery progress without resetting its budget. Doctor slot-activation observations are read-only. Keep the 500 ms keyboard-quiet contract and typed cleanup/error evidence intact.

### Released 3.2.1 increment and input ownership

Follow [the fixed increment matrix](docs/toolkit/rust-go-3.2.1-increment.md), not moving main. Retail 12.1.5 / interface 120105 remains compatible with 12.1.0; ClientGate, TOCs, runtime/slot catalogs and selection validate the exact product/build pairing. Do not infer Data support from the broader runtime product list.

Before sending input, all keyboard virtual keys (VK 0x03–0xFE, excluding mouse/reserved VK 0x04–0x07) must be released and keyboard activity quiet for at least 500 ms; then revalidate fresh INPUT and identity. The minimal WH_KEYBOARD_LL hook records the last activity time only, never text or a key history. Let an already-started chord finish and release its owned keys before waiting; cancellation releases only keys pressed by that attempt. Submitted/uncertain operations observe the original receipt without retransmission; every wait retains the original deadline. Offline state-machine tests and owned Windows fixtures precede user/game validation. Existing Rust tests do not prove this new increment.

### Idiomatic Rust, equivalent external behavior

Do not translate Go line by line. Preserve the external contract while using Rust ownership/borrowing to reduce unnecessary copying, enums/type-state where they prevent invalid transitions, Result for explicit errors, RAII for owned resources and bounded safe concurrency where measurements justify it. Prefer clear concrete abstractions over gratuitous generic frameworks. Review lifecycle/cancellation/drop ordering and thread ownership as architecture, not style-only concerns. These choices do not relax unsafe boundaries or replace measured performance evidence.

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

### Rust safety boundary

Owned business and cross-platform Rust code defaults to no unsafe, enforced with suitable crate-level forbid/deny. Unavoidable Windows FFI/memory operations live behind minimal safe wrappers with per-site SAFETY arguments, pointer validity, ownership/lifetime/thread requirements and focused tests plus independent audit. Do not exempt entire modules indiscriminately or introduce unsafe to chase the performance target. Third-party dependencies require a separate inventory; this policy is not a claim that the dependency graph contains no unsafe. Reuse the fixed Go3.2.2 Skill workflow, changing only necessary Emmy/install/diagnostic details.

## 3. Source: preserve and finish the direct-library path

Keep `emmylua_code_analysis` / `emmylua_parser` linked into the native worker. CLI/worker/session ownership remains explicit: bounded requests, fixed inputs, deadlines, cache identity/invalidation, crash/cancellation containment, cleanup and resource release. The worker may be a mode of the same executable; a subprocess is not proof of LuaLS or LSP. Never execute studied Lua or repository configuration.

The verified public upstream archive may be used to reconstruct a new equivalent dependency, with minimal reviewed patches for required APIs. Pin upstream revision/archive hashes, new patch inventory, licenses and corresponding source; never label new bytes as the missing historical fork or forge its custody hash. A version string alone is insufficient. Clean rebuild and offline corresponding-source checks must precede product claims. Full fixed Retail corpus analysis and continuous cold/warm/session queries are required in addition to focused fixtures.

Separate engine internals from business facts: query/definitions/refs/calls/context/flow, XML/TOC closure, batch ordering and errors, static-only fallback, complete/coverage/truncated/cursor and uncertainty must match the release's business contract. Revisit existing long-running Retail failures without silently widening deadlines, quotas or reducing source closure. Legacy LuaLS feature code is a removal candidate only after feature/call/package audits; luac 5.1 validation and tree-sitter structural work have separate responsibilities and are not automatically dead.

### Owned Emmy adapter crate and upstream maintenance

The narrow owned `lycheedev-emmy-adapter` crate is implemented; extend its existing boundary incrementally. This is an implementation boundary, not a new Source protocol or a claim that an upstream wrapper removes all patches. Dependency direction is `source -> lycheedev-emmy-adapter -> pinned emmylua_code_analysis/parser`; the adapter must not depend back on Source Graph, CLI, Live or Windows. Keep XML/TOC interpretation, Graph construction, request/session ownership and file authentication in Source. Pass small borrowed, immutable descriptors or concrete engine types across the boundary; do not copy Graph or introduce a generic provider framework just to move files. Own code forbids unsafe.

The 2026-10-15 independent audit of patch `f174b3c70bd236a619906c7eb3598ef950db7009424dd1b144a9e22d91012a5f` found 29 upstream paths, +4105/-226 lines: 2248 added production lines and 1857 added lines in three files containing 35 tests. This is a measured patch inventory, not a measured performance result. This is the historical pre-extraction inventory; the later binding-contracts slice below records the delivered reduction.

| Boundary / audited paths | Direction and required proof |
| --- | --- |
| `binding_context.rs` Environment DTOs; `semantic_model/binding.rs` descriptor-to-type projection and domain provenance | First externalization candidates for the owned adapter. Preserve authenticated slots, aliases, receiver/unknown rules and budgets using public engine constructors; avoid a circular Source dependency. |
| BindingSession references/report DTOs | Production ordinary refs/definition/flow already use Source Graph/Syntax. Map true production, example and test consumers; move reporting/query DTOs without dropping useful coverage. |
| `lib.rs`, `semantic_db/mod.rs`, `vfs/mod.rs` shared Arc batch ingestion | Separate generic upstream extension, not WoW business logic. Preserve shared allocation, per-file cancellation and single rebuild; ordinary public String ingestion is not allocation-equivalent. |
| `semantic_model/mod.rs`, `access.rs`, `infer/vm.rs`, `types.rs`, `flow.rs` | Retain a minimal recursive/cross-file context hook. An outer API wrapper cannot intercept internal child inference or model_for, nor safely replace unavailable with raw fallback. |
| `complex.rs`, `member.rs`, `type_check/mod.rs`, `infer/function_solver.rs`, reconstruction/render paths | Preserve session token/epoch/object identity and incomplete-shape rejection. Structural side-map keys cannot safely replace contextual identity. |
| signature/cardinality and five enabled diagnostic checker paths | Preserve zero versus unknown returns, exact output arity, no raw-token fallback, and honest executed/skipped coverage. Filtering final diagnostics is not equivalent. |
| assignment/generic-constraint checkers | Default configuration does not enable them, but file-level diagnostic opt-in can. Establish that contract before calling them dead. |

Production contextual entrypoints are `native_semantic.rs` diagnostic/coverage and `native_research/plan.rs` authenticated API type inference. Experiment with upstream public declaration/signature/doc-type APIs for the latter only after matching aliases, methods/receiver, nilable, unavailable and zero/multiple/MayReturnNothing outputs. Do not return a descriptor fallback while labeling it successful engine inference. Keep the existing 35 engine regressions and add owned-adapter tests; moving tests out of the patch must not reduce coverage.

For every upstream update: pin the new commit/archive/license, retain the prior recipe, map each remaining patch to a real consumer and regression, separate clean application from semantic acceptance, and record conflicts or upstream replacement APIs. Remove a patch only when equivalent upstream functionality is verified. Rebuild the candidate and run the 35 semantic guards, owned-adapter fixtures, full fixed Retail indexing/query/session/diagnostics and relevant resource checks before changing the production pin. Preserve old failures, cache identity changes and reproducibility/corresponding-source evidence. A wrapper reduces coupling; it does not guarantee conflict-free upgrades.

### Minimize upstream source changes (latest user direction, 2026-10-15)

Prefer the unmodified pinned Emmy public APIs and owned XML/business crates whenever they preserve the complete enhanced contract. Audit every remaining patch separately: public-API replacement, owned-layer extraction, or smallest necessary engine hook with a documented consumer and regression. A historical implementation or passing patched-engine test alone does not prove that an upstream change is necessary. Treat session/epoch management, source maps and refs provenance as candidates for owned-layer implementation rather than automatically classifying them as engine hooks.

Use the independent unpatched-engine capability matrix and isolated small examples before removing any hook. Thin virtual Lua projection must not silently weaken nil/callable, parameters/returns/generic constraints, cross-file self/identity or executed-versus-skipped coverage. Replacing the inference/checker semantics with an owned implementation would be a substantial rewrite, not a free plugin boundary. No dynamic plugin framework or new language server is implied by this direction. Keep the current complete design, move small proven slices, and compare actual full-Retail output and resources after each change. Public API compatibility still needs upgrade tests; neither zero patches nor painless upgrades is promised.

### Owned Emmy adapter and retained enhancement contract (2026-10-15)

The released Go Source engine starts official LuaLS 3.19.1 through LSP; fixed `internal/codebase/semantic.go` explicitly leaves XML inline scripts and injected frame globals NotChecked. The Rust contextual XML/Lua design is therefore an additional capability, not proof of Go parity. The user explicitly chose to retain the complete design while reducing coupling to Emmy. Preserve its observable behavior and uncertainty guarantees; do not remove enhancements solely because Go lacks them.

Introduce a focused owned `lycheedev-emmy-adapter` crate. Source owns Graph, XML/TOC ingestion, source/session orchestration and business contracts; the adapter owns engine configuration/session entrypoints and neutral diagnostics/type projections. Dependency direction is Source → adapter → pinned Emmy. Do not make the adapter depend on Source, duplicate Graph, or create a generic plugin framework. A first slice must move real production diagnostics/API-type consumers, not merely re-export Emmy symbols. Domain DTOs, environment conversion, provenance and reporting can then move out of the upstream patch in independently reviewed increments.

The initial public reconstruction modified 29 upstream paths (+4105/-226 lines, including 1857 test lines); those counts describe that historical recipe. Subsequent callback guards and owned extraction have separate exact manifests. Retain necessary internal hooks until equivalent replacements are demonstrated: recursive/cross-file context propagation, session/epoch object identity, managed-unavailable versus absent/not-managed, return cardinality and real diagnostic completion. An outer query wrapper or filtering final diagnostics cannot replace these hooks. The shared-Arc VFS ingestion/cancellation extension is a separate generic optimization, not a measured performance result. Two default-disabled checkers can still be activated by file-level diagnostic directives; absence from defaults is not consumer-proof for deletion.

For each upstream upgrade: pin the exact new revision/archive/license; inventory each patch against its production consumer, enhanced or Go contract, replacement public API and regression; port only still-required hooks; preserve the 35 contextual regressions and full fixed Retail queries/diagnostics/session acceptance; compare outputs/uncertainty, sampled memory and cold/warm elapsed time; independently review remaining patches and remove only hooks proven redundant by upstream APIs. Keep the old recipe and failures. Crate extraction alone does not eliminate upstream maintenance or prove successful upgrades.

### Engine-independent binding contracts

`lycheedev-binding-contracts` owns the existing engine-neutral decision/object-view/field-presence/shape-enumeration/environment records. Its dependency direction is contracts → serde/serde_json, Emmy → contracts, adapter → Emmy + contracts, Source → adapter. Compatibility re-exports retain type identity and serialization; no copied Graph, cycle or engine clone is introduced. Engine-specific FileId/LuaSyntaxId/function types, session tokens and inference hooks remain in the minimal upstream patch where moving them would manufacture coupling in the wrong direction.

The first reviewed slice moves 135 existing contract lines, reducing added patch lines from 4193 to 4071 (122 net). These are physical patch counts, not a claim that all remaining hooks are removable. All prior enhanced semantics remain. The new five integration tests passed locally through the controlled exact-source runner; the 36 engine regressions passed for exact ea364 in its independent recipe job, while the current 24 adapter regressions and full product acceptance still require their exact candidate execution. Owned-file identity inventories, both lock graphs, standalone/full-source staging, source custody and unsafe guards cover the new transitive crate. Future upgrades review these boundaries and retain or remove each hook only against its real consumer and regression.

## 4. Live: implement the complete released state machine

The canonical specification is [Go 3.2.1 docs/live-protocol.md](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/docs/live-protocol.md), alongside its Lua/Go implementation and tests at that commit. Protocol literal: `Lycheedev mailbox protocol v1`. Import/port complete cases into a Rust/Lua differential fixture suite; do not substitute the branch's old RUN/RECEIVED exact-COMMIT design.

Read-only memory lookup uses current mapped runtime instructions and RIP-derived roots, guarded current Lua paths and publication validation. Observation-only results do not authorize keys. Input requires exact target/actor/runtime/owner/fence, fresh INPUT, actual two bindings and revalidation immediately before sending. Native input is bounded PostMessage to the fixed HWND; no Esc, Enter, chat typing, focus stealing or blind fallback.

Persist intent before input, preserve submitted/uncertain outcomes, exact request/ticket/report identity and original deadlines through resume. Reports, business success and cleanup are distinct. Cleanup recovery, upgrade compatibility, BootstrapMailbox diagnostics, missing outcomes, publication races and multi-instance shared-install locking are acceptance requirements, not later optional hardening. Full case matrix lives in handover.

## 5. Performance design and evidence

Optimize reuse, indexing, allocation, bounded parallelism and native worker/session costs only after equivalent output and validation are established. The per-path target is controllable Rust elapsed ≤ 0.30 × Go. Define paired phase boundaries before measurement. Network downloads and uncontrollable external waiting are excluded from the ratio but remain visible in raw wall-clock evidence; local parsing, validation, persistence, cleanup and controllable scheduling remain included.

Record independent cold and warm repetitions, interleave Go/Rust on the same machine and inputs, and preserve raw timings, median, p95, CPU, peak memory and I/O. No cross-path average can hide a failing path. Incomplete, failed or unmeasured paths are not passes. The plan's exact collection/report protocol is in handover.

## 6. Cleanup and delivery

Inventory dead code/doc candidates; prove absence of production/feature/test/package consumers, identify replacement coverage, then remove in reviewable batches. Preserve immutable historical evidence in git/history and label superseded documents. Do not delete caches/evidence/user data opportunistically. Follow existing owned-process/disk admission tooling.

Windows amd64 Rust executable, the same addon, generated CLI/Skill references, launcher, licenses and corresponding source must refer to one sealed candidate. Test install/upgrade/rollback/modified-file protection without games first. The first commit remains documentation-only; full implementation is subsequently authorized. Each batch updates the same draft PR. After implementation, execute handover’s four serial audit/fix/verify/review loops to convergence, recording fixed-tree evidence and human-dependent exceptions. Release/publication and main merge require separate authorization.
