# Rust migration design

Current decision/status authority: [handover.md](handover.md). This file specifies the target, not completion. Keep this exact uppercase filename; do not add `Design.md` alongside it on case-insensitive Windows filesystems. The former workbench-only design is preserved in [history](docs/history/workbench-ui-design-2026-09-28.md).

## 1. Contract and migration strategy

Go release `3057a9666ab078270b63c7f06de748069b0d1f4b` (v3.2.0) defines observable behavior. Compare each existing Rust module with that immutable oracle and classify it as retain / adapt / replace / missing / unverified. Retain code only with evidence for the current contract, not its age, number of tests or registration in `describe`. The Rust snapshot is an implementation starting point, not a second behavioral oracle.

The allowed implementation differences are Rust host code and EmmyLua direct-library analysis replacing LuaLS. Preserve equivalent Source business semantics and honest engine/coverage provenance; engine-specific implementation metadata must accurately name EmmyLua, never impersonate LuaLS. No other compatibility exception is silently grandfathered from old plans. Any unavoidable business difference requires a separately approved decision before acceptance.

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

Owned business and cross-platform Rust code defaults to no unsafe, enforced with suitable crate-level forbid/deny. Unavoidable Windows FFI/memory operations live behind minimal safe wrappers with per-site SAFETY arguments, pointer validity, ownership/lifetime/thread requirements and focused tests plus independent audit. Do not exempt entire modules indiscriminately or introduce unsafe to chase the performance target. Third-party dependencies require a separate inventory; this policy is not a claim that the dependency graph contains no unsafe. Reuse the fixed Go3.2.0 Skill workflow, changing only necessary Emmy/install/diagnostic details.

## 3. Source: preserve and finish the direct-library path

Keep `emmylua_code_analysis` / `emmylua_parser` linked into the native worker. CLI/worker/session ownership remains explicit: bounded requests, fixed inputs, deadlines, cache identity/invalidation, crash/cancellation containment, cleanup and resource release. The worker may be a mode of the same executable; a subprocess is not proof of LuaLS or LSP. Never execute studied Lua or repository configuration.

The verified public upstream archive may be used to reconstruct a new equivalent dependency, with minimal reviewed patches for required APIs. Pin upstream revision/archive hashes, new patch inventory, licenses and corresponding source; never label new bytes as the missing historical fork or forge its custody hash. A version string alone is insufficient. Clean rebuild and offline corresponding-source checks must precede product claims. Full fixed Retail corpus analysis and continuous cold/warm/session queries are required in addition to focused fixtures.

Separate engine internals from business facts: query/definitions/refs/calls/context/flow, XML/TOC closure, batch ordering and errors, static-only fallback, complete/coverage/truncated/cursor and uncertainty must match the release's business contract. Revisit existing long-running Retail failures without silently widening deadlines, quotas or reducing source closure. Legacy LuaLS feature code is a removal candidate only after feature/call/package audits; luac 5.1 validation and tree-sitter structural work have separate responsibilities and are not automatically dead.

## 4. Live: implement the complete released state machine

The canonical specification is [Go 3.2.0 docs/live-protocol.md](https://github.com/Follen/Lychee-Dev/blob/3057a9666ab078270b63c7f06de748069b0d1f4b/docs/live-protocol.md), alongside its Lua/Go implementation and tests at that commit. Protocol literal: `Lycheedev mailbox protocol v1`. Import/port complete cases into a Rust/Lua differential fixture suite; do not substitute the branch's old RUN/RECEIVED exact-COMMIT design.

Read-only memory lookup uses current mapped runtime instructions and RIP-derived roots, guarded current Lua paths and publication validation. Observation-only results do not authorize keys. Input requires exact target/actor/runtime/owner/fence, fresh INPUT, actual two bindings and revalidation immediately before sending. Native input is bounded PostMessage to the fixed HWND; no Esc, Enter, chat typing, focus stealing or blind fallback.

Persist intent before input, preserve submitted/uncertain outcomes, exact request/ticket/report identity and original deadlines through resume. Reports, business success and cleanup are distinct. Cleanup recovery, upgrade compatibility, BootstrapMailbox diagnostics, missing outcomes, publication races and multi-instance shared-install locking are acceptance requirements, not later optional hardening. Full case matrix lives in handover.

## 5. Performance design and evidence

Optimize reuse, indexing, allocation, bounded parallelism and native worker/session costs only after equivalent output and validation are established. The per-path target is controllable Rust elapsed ≤ 0.30 × Go. Define paired phase boundaries before measurement. Network downloads and uncontrollable external waiting are excluded from the ratio but remain visible in raw wall-clock evidence; local parsing, validation, persistence, cleanup and controllable scheduling remain included.

Record independent cold and warm repetitions, interleave Go/Rust on the same machine and inputs, and preserve raw timings, median, p95, CPU, peak memory and I/O. No cross-path average can hide a failing path. Incomplete, failed or unmeasured paths are not passes. The plan's exact collection/report protocol is in handover.

## 6. Cleanup and delivery

Inventory dead code/doc candidates; prove absence of production/feature/test/package consumers, identify replacement coverage, then remove in reviewable batches. Preserve immutable historical evidence in git/history and label superseded documents. Do not delete caches/evidence/user data opportunistically. Follow existing owned-process/disk admission tooling.

Windows amd64 Rust executable, the same addon, generated CLI/Skill references, launcher, licenses and corresponding source must refer to one sealed candidate. Test install/upgrade/rollback/modified-file protection without games first. The first commit remains documentation-only; full implementation is subsequently authorized. Each batch updates the same draft PR. After implementation, execute handover’s four serial audit/fix/verify/review loops to convergence, recording fixed-tree evidence and human-dependent exceptions. Release/publication and main merge require separate authorization.
