# Rust migration checklist

Authority for decisions, status and evidence: [handover.md](handover.md). This checklist gives execution order only. The first delivery is documentation; full implementation is now authorized. Unchecked work does not authorize game input, publication or main merge. Existing Rust code and historical passing tests do not check these boxes automatically.

## 0. Documentation scope

- [x] Freeze Go v3.2.0 at 3057a9666ab078270b63c7f06de748069b0d1f4b and record Rust starting HEAD.
- [x] Define reuse/adapt/replace strategy, full parity and EmmyLua direct-library exception.
- [x] Define ≤30% controllable-time performance target and full functional/protocol matrices.
- [x] Replace README in both languages; use one DESIGN.md without case-only duplicate.
- [x] Keep only handover.md; mark old guidance/evidence historical and preserve prior README/UI text.
- [x] Put every user/game/specific-interactive-machine test at the end of handover.

- [x] Push the reviewed documentation to rust-refactor and create/reuse its own draft PR (#14); subsequent authorized code batches update that same PR promptly, without waiting for all validation or opening one PR per batch.

## Current module work queue (derived from handover; no percentage claims)

This is an execution view, not a second evidence authority. A component being implemented or independently source-reviewed does not check its complete parity box. Exact run/commit/hash evidence and limitations remain in handover.md.

### Implemented components with some native evidence

- [ ] Foundation: retain reproducible public Emmy reconstruction and official Windows build; finish independent corresponding-source rebuilds and complete clean package acceptance.
- [ ] Source: retain fixed full Retail static import/query and real multi-CLI session evidence; validate newly repaired XML owner plans and source batch on the next exact candidate. Complete full Retail native analysis/refs/context/flow, real quota/idle/lifetime, cancellation, identity changes, readable source locations and independent Source evaluator.
- [ ] Data: retain installation/catalog/relations and named Hotfix components; validate corrected numeric archive/field budgets, new capture dependencies and scan checkpoint/resume on the exact Windows candidate. Validate the reviewed effective SQL overlay/reference candidate on Windows; finish SQL meta/CSV/EXPLAIN/output/resource/deadline gaps, remote provider, remaining DB2/CASC/CDN/assets boundaries and all fixed-Go test mapping.
- [ ] Delivery: retain formal 500-slot publication, journal and maintenance/rollback components; finish integrated runtime receipt/retirement, old200 upgrade, disabled/empty-map, shared-installation and complete package/install smoke on the final candidate.
- [ ] Skill/CLI: retain Go-based payload, real packaged CLI task runner and command registration fixes; close actual describe differences and complete all arguments/errors/JSONL/business output equivalence. Public number formatting must distinguish typed floats from raw JSON tokens. Re-run the independent actual Skill flow after the current semantic fixes.

### Implemented or in progress, still not the default production path

- [ ] Formal Live: finish trusted EXE qualification plus runtime RIP/Lua reader, original budget/control, structured firstReadFailure and checked close. Review fixed-Go Unicode path identity rather than Windows Ordinal assumptions.
- [ ] Formal input: execute the reviewed full-identity two-key engine and owned-window tests; integrate fresh INPUT authorization, publication lease held across Invoke, Reload release ordering and 100ms exchange spacing.
- [ ] Native backend: connect formal reader/input/delivery to Backend and RuntimeRetirementBackend, exact receipt/trace/cleanup, original deadlines and two fresh retirement samples.
- [ ] Default Live lifecycle: connect CON admission/durable journal/history to Driver, public status/resume/execute/report/disconnect and result projection; unknown outcome must never replay business input.
- [ ] Activation/doctor: implement prepared/waiting_manual_reload/runtime_selected/cancelled flow, Bootstrap cross-checks, read-only fresh-input doctor and explicit fix/reload fallback without hidden replay or unsafe activation.
- [ ] Replace old default Live/Transport only after formal integration and synthetic process/window failure/restart tests cover the public route.

### Native execution and corpus acceptance still required

- [ ] Obtain terminal Windows evidence for each new frozen candidate; verify selected Rust test groups execute nonzero matching tests and preserve failures. Early describe failure must not short-circuit unrelated product groups.
- [ ] Execute the newly ported 214-case fixed-Go formal Lua/addon matrix and finish the three retained Go host bridges plus remaining bootstrap/input_mailbox/focus boundaries. Current same-named legacy Lua tests are not evidence for Mailbox v1.
- [ ] Migrate valuable legacy native_e2e installation/transaction/unknown-result tests to the formal protocol before removing their extra addon/TOC consumers.
- [ ] Audit dynamic TOC, build/include, tool, package, feature and test consumers before removing 21 extra addon files, optical/WGC/LuaLS/old protocol branches or superseded guidance.
- [ ] Finish corresponding-source, npm install/upgrade/uninstall/restore, official addon-only package closure and exact final candidate validation.
- [ ] Run independent Source and Skill quality acceptance against actual CLI outputs; do not replace semantic evidence with checker-test counts.

### Performance and repository-wide convergence remain unmeasured/open

- [ ] Establish same-machine fixed-Go and Rust equivalent outputs for every B01–B07 path; independent cold/warm >=20 repeats, raw timings and excluded waits. Current isolated timings do not prove Rust <=30% of Go.
- [ ] Complete all F01–F13/P01–P14 functional rows before the four sequential whole-repository audit loops. Per-batch independent reviews do not count as Stage 1–4 convergence.
- [ ] Keep user/game cooperation H01–H09 at handover's end and unfilled until real execution; do not hold unrelated automated work for them.

### Current validation cadence and new Source priorities

- [ ] Run local real pure Rust/Node/Lua regressions first within valid owned channels; preserve results and promptly push each meaningful batch. Every push runs fast checks; explicit `[rust-native]` batch commits run full Windows/Emmy acceptance. Do not treat skipped native jobs as green product validation.
- [ ] Finish Source process terminal proof and legacy luac caller retention, checked Windows process/job close and Closure I/O/control boundaries. An error with uncertain resource retirement must not trigger parent scratch deletion.
- [ ] Record actual full-Retail cold/warm/native/session synchronized process-group WorkingSet and PrivateBytes, including workers, steady samples, sampling gaps and failed-run status; existing peakRSS is not_measured.
- [ ] Introduce narrow owned `lycheedev-emmy` adapter crate after default CLI integration: dependency direction Source→adapter→pinned upstream, no back-reference/Graph copy/framework expansion. First move authenticated environment DTO/type conversion/domain provenance and reporting; preserve necessary recursive/session/unknown/cardinality/checker hooks.
- [ ] Map all 29 current patch paths to production/example/test consumers and regression evidence. Validate API Type public-upstream alternative, file diagnostic opt-in and object reconstruction before reducing hooks; keep 35 existing engine regressions and complete fixed-Retail acceptance.
- [ ] Document and test each upstream-version upgrade: fixed commit/archive/license, patch conflicts/replacements, cache identity, reproducibility, owned-adapter and full-Retail outputs/resources. No claim of zero-maintenance fork or measured performance gains from file movement.

### Newly ordered implementation work (2026-10-15)

- [x] Push reviewed formal activation/doctor/repair and default CON CLI wiring in b83a4d3/da546758. This checks delivery of code, not runtime equivalence; legacy OP/BTP and remaining cleanup cases stay open.
- [ ] Finish Source process terminal ownership and both ordered/legacy luac caller retention; independently review, locally test pure portions, then run automated Windows process fixtures.
- [ ] Publish independently reviewed local pure Rust runner; continue exact production module tests locally. Keep frequent persistence pushes separate from explicit `[rust-native]` batch acceptance.
- [ ] Collect actual full Retail memory by phase using synchronized authenticated Job membership; report current WorkingSet/PrivateBytes sampled group peaks and steady observations, sampling gaps and failed/incomplete operations separately. Historical `not_measured` remains unchanged.
- [ ] Create owned Emmy adapter crate with one-way Source→adapter→Emmy dependencies and real diagnostics/API-type consumers, then move domain DTO/projection/provenance/reporting out of upstream patches in small reviewed batches. Preserve the entire enhanced XML/Lua design per explicit user decision; label it separately from Go-equivalent behavior.
- [ ] Map each remaining Emmy patch to consumers and tests; preserve 35 contextual regressions/full Retail acceptance. Pin upgrades, check official public API replacements, review uncertainty/identity/cardinality boundaries and remove only proven-redundant hooks. Do not count a wrapper-only crate as completed decoupling.
- [ ] Complete SQL meta exact Unicode EqualFold before independent review; logical resources admission (including 16×1MiB Go80MiB/Rust144MiB false rejection), paired CSV transaction and EXPLAIN remain separate required gaps.

## 1. Reproducible foundation (no user interaction)

- [ ] Pin Go release executable/source hashes; export complete describe/help/CLI/Skill/addon oracle.
- [ ] Inventory each current Rust implementation against F01–F13: retain/adapt/replace/missing/unverified with source and test evidence.
- [ ] Use the verified fixed public Emmy upstream as the recovery base; compare consumed APIs, port minimal necessary patches as a NEW dependency with explicit provenance/license/tests rather than pretending to reproduce missing private bytes. Reconcile old custody assumptions honestly.
- [ ] Validate clean build, locked toolchain/dependencies and two independent offline corresponding-source rebuilds within owned cache limits.

## 2. Full business parity (offline first)

- [ ] Differential CLI/parser/envelopes/JSONL/exit codes, selection/products/pins/projects/evidence and budgets.
- [ ] Keep native Emmy worker/session; close full Source/validation/batch/flow/cache/crash/deadline/cleanup gaps without reducing inputs or checks.
- [ ] Import and analyze the entire fixed Retail31c7f7b corpus (all Lua/XML/TOC and inline maps), run representative file/symbol/line/source queries, refs/context/flow, cold/warm/persistent session and resource cleanup against Go; small fixtures and historical Retail19 are not acceptance.
- [ ] Complete Data DB2/SQL effective/table/scan Hotfix/providers/CASC/CDN/assets/relations and error paths.
- [ ] Reuse the released addon; reconcile installation/500-slot/upgrade/rollback/modified-file protection.
- [ ] Reuse the fixed Go3.2.0 Skill and launcher workflow; change only necessary Emmy/install/diagnostic details and actual implemented command/flag contracts.
- [ ] Enforce no unsafe in owned business/cross-platform crates with suitable forbid/deny; inventory unavoidable Windows FFI/memory boundaries, use minimal safe wrappers with per-site SAFETY proofs and pointer/lifetime/thread tests; independently audit, no blanket module exemptions or performance shortcut. Report dependency unsafe separately.

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

- [ ] Stage 1: architecture, all business bugs, Skill and all code-related paths converge; independently check idiomatic Rust ownership/borrowing, meaningful enums/type-state, Result/RAII and bounded safe concurrency, without line-by-line Go translation or needless abstraction.
- [ ] Assign a fresh independent Source evaluator: real representative CLI queries, Chinese/large/empty/error/refs/context results, correct readable file/symbol/line/source evidence.
- [ ] Assign a separate fresh independent Skill evaluator: actually follow its workflow, discover entrypoints/use correct arguments, keep steps concise and recovery current, prefer pinned Source over ad hoc GitHub raw. Record real CLI evidence and regressions, not implementer self-approval or aesthetic contract changes.
- [ ] Stage 2: test sufficiency/meaningful coverage and tests without independent value converge; add regressions/remove proven redundancy, then measure coverage.
- [ ] Map fixed Go tests to Rust equivalents, explicit non-applicability reasons or uncovered gaps; preserve recovery/unknown-result-no-replay/transaction/cancellation/protocol/empty-map/slot-migration boundaries without copying obsolete or redundant fixtures merely to inflate counts.
- [ ] Stage 3: dedicated dead-code audit including dynamic/platform/feature/tool/package consumers converges after cleanup and regression.
- [ ] Stage 4: combined whole-repository audit across all three dimensions converges after fixes, verification and independent re-review.
- [ ] Push every authorized batch promptly to the same draft PR and update handover; never transfer a clean review of an old tree to a changed tree.

## 6. Actual cooperation last

- [ ] Execute only the final handover H01–H09 cases that require user/client/interactive machine cooperation after earlier independent gates; record missing environments as not_run.
- [ ] Complete real Live timing with external waits separated; no unmeasured performance guarantee.
- [ ] Review every F/P/B/H row with exact candidate evidence; preserve failed/partial/not_run rather than claiming complete.
- [ ] Obtain separate authorization for release/npm tag changes/main merge; this documentation task authorizes none of them.
