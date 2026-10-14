# Rust migration checklist

Authority for decisions, status and evidence: [handover.md](handover.md). This checklist gives execution order only. The first delivery is documentation; full implementation is now authorized. Unchecked work does not authorize game input, publication or main merge. Existing Rust code and historical passing tests do not check these boxes automatically.

## 0. Documentation scope

- [x] Freeze Go v3.2.0 at 3057a9666ab078270b63c7f06de748069b0d1f4b and record Rust starting HEAD.
- [x] Define reuse/adapt/replace strategy, full parity and EmmyLua direct-library exception.
- [x] Define ≤30% controllable-time performance target and full functional/protocol matrices.
- [x] Replace README in both languages; use one DESIGN.md without case-only duplicate.
- [x] Keep only handover.md; mark old guidance/evidence historical and preserve prior README/UI text.
- [x] Put every user/game/specific-interactive-machine test at the end of handover.

- [ ] Push the reviewed documentation to rust-refactor and create/reuse its own draft PR; subsequent authorized code batches update that same PR promptly, without waiting for all validation or opening one PR per batch.

## 1. Reproducible foundation (no user interaction)

- [ ] Pin Go release executable/source hashes; export complete describe/help/CLI/Skill/addon oracle.
- [ ] Inventory each current Rust implementation against F01–F13: retain/adapt/replace/missing/unverified with source and test evidence.
- [ ] Recover and pin exact modified Emmy dependency source, patches and licenses; no moving upstream substitution.
- [ ] Validate clean build, locked toolchain/dependencies and two independent offline corresponding-source rebuilds within owned cache limits.

## 2. Full business parity (offline first)

- [ ] Differential CLI/parser/envelopes/JSONL/exit codes, selection/products/pins/projects/evidence and budgets.
- [ ] Keep native Emmy worker/session; close full Source/validation/batch/flow/cache/crash/deadline/cleanup gaps without reducing inputs or checks.
- [ ] Complete Data DB2/SQL effective/table/scan Hotfix/providers/CASC/CDN/assets/relations and error paths.
- [ ] Reuse the released addon; reconcile installation/500-slot/upgrade/rollback/modified-file protection.
- [ ] Reconcile versioned Skill and launcher with actual implemented command/flag contracts.

## 3. Complete Mailbox protocol (no game needed yet)

- [ ] Port the fixed release's complete Lua/Go contract and fixtures; close P01–P14, not just runtime lookup.
- [ ] Wire runtime/RIP guarded path into the default host; observation cannot authorize input.
- [ ] Unify two-key INPUT, BootstrapMailbox/doctor, old200 upgrade, empty map, DISABLED and firstReadFailure semantics.
- [ ] Model/fault-test intent/outcome/recovery/report/cleanup/deadlines, capacity, multiple instances and shared installation.
- [ ] Run Windows-specific automated tests with owned child processes/synthetic windows; keep game-dependent cases last.

## 4. Proven cleanup and sealed candidate

- [ ] Audit consumers/features/package/test references before removing dead LuaLS/optical/WGC/old-protocol paths or obsolete docs.
- [ ] Preserve history, failure evidence, oracle fixtures and separate luac/tree-sitter responsibilities.
- [ ] Build one Windows amd64 candidate with same addon, Skill, licenses, corresponding source and artifact hashes.
- [ ] Run full offline/automated Windows and install/upgrade/uninstall/restore/package regressions on that exact candidate.
- [ ] Run B01–B07 automatic performance cases: independent cold/warm samples, equivalent output, per-path ≤0.30, raw timing and excluded waits disclosed.

## 5. Four sequential audit loops after full implementation

Each stage repeats independent audit → actionable findings → fixes → verification → independent re-audit until no new actionable finding remains on the fixed reviewed tree. Only then advance. Record scope, commit, findings and evidence; never claim mathematical zero bugs. Human/game-dependent exceptions stay unverified in handover’s final section while independent work continues.

- [ ] Stage 1: architecture, all business bugs, Skill and all code-related paths converge.
- [ ] Stage 2: test sufficiency/meaningful coverage and tests without independent value converge; add regressions/remove proven redundancy, then measure coverage.
- [ ] Stage 3: dedicated dead-code audit including dynamic/platform/feature/tool/package consumers converges after cleanup and regression.
- [ ] Stage 4: combined whole-repository audit across all three dimensions converges after fixes, verification and independent re-review.
- [ ] Push every authorized batch promptly to the same draft PR and update handover; never transfer a clean review of an old tree to a changed tree.

## 6. Actual cooperation last

- [ ] Execute only the final handover H01–H09 cases that require user/client/interactive machine cooperation after earlier independent gates; record missing environments as not_run.
- [ ] Complete real Live timing with external waits separated; no unmeasured performance guarantee.
- [ ] Review every F/P/B/H row with exact candidate evidence; preserve failed/partial/not_run rather than claiming complete.
- [ ] Obtain separate authorization for release/npm tag changes/main merge; this documentation task authorizes none of them.
