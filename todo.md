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

## Current tagged increment: Go 3.2.2

- [x] Pin v3.2.2 to `0bec60e4406f5d5dc155ee25cef44f9a710e37e8`; preserve both prior oracles and all results. Tag/main checked, Release API assets not verified. See [increment matrix](docs/toolkit/rust-go-3.2.2-increment.md).
- [ ] Implement GUID-scoped managed-slot enable/verify/save without LoadAddOn or slot consumption; retain foreign-addon and partial-failure behavior.
- [ ] Propagate slotReloadRequired through fresh observation, input-profile equality, native pre-send authorization, original-transaction reload/recovery and Continue progress; no unknown replay or deadline reset.
- [ ] Add read-only live.slot_activation doctor and fixed startup Skill guidance.
- [ ] Build separate 3.2.2 oracle/Lua fixtures and real recovery regressions, including replacement runtimes and submitted/uncertain/missing crashes. Old 214 Lua or 3.2.1 results are not 3.2.2 evidence.
- [ ] Complete G322-01–G322-06 with independent automated checks before the user-assisted cases at the end of handover.

## Cumulative prior increment: Go 3.2.1

- [x] Pin the prior target to released 10b561db5706702cd42e047eaeed4ccb5e825351; retain 3057a966 as historical evidence. See [increment matrix](docs/toolkit/rust-go-3.2.1-increment.md).
- [ ] Implement and verify Retail 12.1.5/120105 plus 12.1.0 compatibility across ClientGate, TOCs, runtime/slot catalogs and product/build selection.
- [ ] Implement and verify all-key release plus 500 ms keyboard-activity quiet time, fresh INPUT, chord release, no replay and original deadlines; retain timestamp only, no keystroke history.
- [ ] Synchronize fixed 3.2.1 Skill/protocol guidance and rebuild current oracle/test mappings; do not rename old passing evidence.
- [ ] Complete G321-01–G321-06 automatically first; user/game checks remain at handover’s end.

## Current module work queue (derived from handover; no percentage claims)

This is an execution view, not a second evidence authority. A component being implemented or independently source-reviewed does not check its complete parity box. Exact run/commit/hash evidence and limitations remain in handover.md.

### Implemented components with some native evidence

- [ ] Foundation: retain reproducible public Emmy reconstruction and official Windows build; finish independent corresponding-source rebuilds and complete clean package acceptance.
- [ ] Source: retain fixed full Retail static import/query and real multi-CLI session evidence; validate newly repaired XML owner plans and source batch on the next exact candidate. Complete full Retail native analysis/refs/context/flow, real quota/idle/lifetime, cancellation, identity changes, readable source locations and independent Source evaluator.
- [ ] Data: retain installation/catalog/relations and named Hotfix components; validate corrected numeric archive/field budgets, new capture dependencies and scan checkpoint/resume on the exact Windows candidate. Validate the reviewed effective SQL overlay/reference candidate on Windows; validate reviewed CSV pair publication and finish SQL meta/EXPLAIN/output/resource/deadline gaps, remote provider, remaining DB2/CASC/CDN/assets boundaries and all fixed-Go test mapping.
- [ ] Delivery: retain formal 500-slot publication, journal and maintenance/rollback components; finish integrated runtime receipt/retirement, old200 upgrade, disabled/empty-map, shared-installation and complete package/install smoke on the final candidate.
- [ ] Skill/CLI: retain Go-based payload, real packaged CLI task runner and command registration fixes; close actual describe differences and complete all arguments/errors/JSONL/business output equivalence. Public number formatting must distinguish typed floats from raw JSON tokens. Re-run the independent actual Skill flow after the current semantic fixes.

### Default production integration implemented; exact-candidate validation remains open

- [ ] Formal Live/runtime/input are on the default CON route via formal_live → ConnectionService, rather than an unconnected prototype. Validate trusted EXE/RIP reader, Unicode identity, fresh INPUT authorization and real public command integration on the final candidate.
- [ ] Validate the implemented formal Backend/RuntimeRetirementBackend, durable journal, original deadlines, exact receipt/trace/cleanup and two fresh retirement samples; unknown outcomes must never replay business input.
- [ ] Validate default connect/status/resume/execute/report/disconnect and activation/doctor state/error projections, including manual reload and explicit fix boundaries. Do not describe these already-connected routes as still awaiting first implementation.
- [ ] Complete synthetic process/window failure and restart coverage, then audit remaining old Transport/LuaLS/optical consumers before deletion. Real game-dependent validation remains last.

### Native execution and corpus acceptance still required

- [ ] Obtain terminal Windows evidence for each new frozen candidate; verify selected Rust test groups execute nonzero matching tests and preserve failures. Early describe failure must not short-circuit unrelated product groups.
- [ ] Retain the actual historical 363-run 214/214 fixed-Go formal Lua/addon result; rerun on the final candidate and finish the three retained Go host bridges plus remaining bootstrap/input_mailbox/focus boundaries. Current same-named legacy Lua tests are not evidence for Mailbox v1.
- [ ] Migrate valuable legacy native_e2e installation/transaction/unknown-result tests to the formal protocol before removing their extra addon/TOC consumers.
- [ ] Audit dynamic TOC, build/include, tool, package, feature and test consumers before removing 21 extra addon files, optical/WGC/LuaLS/old protocol branches or superseded guidance.
- [ ] Finish corresponding-source, npm install/upgrade/uninstall/restore, official addon-only package closure and exact final candidate validation.
- [ ] Run independent Source and Skill quality acceptance against actual CLI outputs; do not replace semantic evidence with checker-test counts.

### Performance and repository-wide convergence remain unmeasured/open

- [ ] Establish same-machine fixed-Go and Rust equivalent outputs for every B01–B07 path; independent cold/warm >=20 repeats, raw timings and excluded waits. Current isolated timings do not prove Rust <=30% of Go.
- [ ] Complete all F01–F13/P01–P14 functional rows before the four sequential whole-repository audit loops. Per-batch independent reviews do not count as Stage 1–4 convergence.
- [ ] Keep user/game cooperation H01–H09 at handover's end and unfilled until real execution; do not hold unrelated automated work for them.

### Current validation cadence and new Source priorities

- [ ] Apply the latest upstream-minimization direction: evaluate each remaining Emmy patch against the independent unpatched public-API matrix, prefer owned XML/business/adapter implementations, and retain only demonstrated necessary minimal hooks with consumer/regression evidence. Preserve all approved enhanced semantics; no automatic large rewrite, dynamic plugin framework or zero-maintenance promise. Session/epoch/source-map/ref provenance are not automatically engine-only responsibilities.


- [ ] Run local real pure Rust/Node/Lua regressions first within valid owned channels; preserve results and promptly push each meaningful batch. Every push runs fast checks; explicit `[rust-native]` batch commits run full Windows/Emmy acceptance. Do not treat skipped native jobs as green product validation.
- [ ] Finish Source process terminal proof and legacy luac caller retention, checked Windows process/job close and Closure I/O/control boundaries. An error with uncertain resource retirement must not trigger parent scratch deletion.
- [ ] Record actual full-Retail cold/warm/native/session synchronized process-group WorkingSet and PrivateBytes, including workers, steady samples, sampling gaps and failed-run status; existing peakRSS is not_measured.
- [ ] Validate and incrementally extend the implemented `lycheedev-emmy-adapter` and `lycheedev-binding-contracts` crates: dependency direction Source→adapter→pinned upstream, no back-reference/Graph copy/framework expansion. First move authenticated environment DTO/type conversion/domain provenance and reporting; preserve necessary recursive/session/unknown/cardinality/checker hooks.
- [ ] Map all 29 current patch paths to production/example/test consumers and regression evidence. Validate API Type public-upstream alternative, file diagnostic opt-in and object reconstruction before reducing hooks; keep 35 existing engine regressions and complete fixed-Retail acceptance.
- [ ] Document and test each upstream-version upgrade: fixed commit/archive/license, patch conflicts/replacements, cache identity, reproducibility, owned-adapter and full-Retail outputs/resources. No claim of zero-maintenance fork or measured performance gains from file movement.

### Newly ordered implementation work (2026-10-15)

- [x] Push reviewed formal activation/doctor/repair and default CON CLI wiring in b83a4d3/da546758. This checks delivery of code, not runtime equivalence; legacy OP/BTP and remaining cleanup cases stay open.
- [ ] Finish Source process terminal ownership and both ordered/legacy luac caller retention; independently review, locally test pure portions, then run automated Windows process fixtures.
- [ ] Retain the published independently reviewed local pure Rust runner and actual 21+5 results; continue exact production module tests locally. Keep frequent persistence pushes separate from explicit `[rust-native]` batch acceptance.
- [ ] Collect actual full Retail memory by phase using synchronized authenticated Job membership; report current WorkingSet/PrivateBytes sampled group peaks and steady observations, sampling gaps and failed/incomplete operations separately. Historical `not_measured` remains unchanged.
- [ ] Extend the implemented owned Emmy adapter with one-way Source→adapter→Emmy dependencies and real diagnostics/API-type consumers, then move domain DTO/projection/provenance/reporting out of upstream patches in small reviewed batches. Preserve the entire enhanced XML/Lua design per explicit user decision; label it separately from Go-equivalent behavior.
- [ ] Map each remaining Emmy patch to consumers and tests; preserve 35 contextual regressions/full Retail acceptance. Pin upgrades, check official public API replacements, review uncertainty/identity/cardinality boundaries and remove only proven-redundant hooks. Do not count a wrapper-only crate as completed decoupling.
- [ ] Complete SQL meta exact Unicode EqualFold before independent review; logical resources admission (including 16×1MiB Go80MiB/Rust144MiB false rejection), paired CSV R4 is independently reviewed but native NOT_RUN; EXPLAIN/TopK R3 is implemented and independently reviewed, with 31 new tests NOT_RUN; ANALYZE/provider callback and resource gaps remain.

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
- [ ] Reuse the fixed Go3.2.1 Skill and launcher workflow; change only necessary Emmy/install/diagnostic details and actual implemented command/flag contracts.
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

## Source implementation checkpoint (2026-10-15)

- [x] Implement and independently review owned callback projection, original-line index, and parent-budget/phase observation candidate. Preserve full XML/Lua enhancement.
- [x] Execute 21 exact production pure tests locally, plus 11 runner tests; record exact receipts.
- [ ] Compile the composed Source candidate and run all adapter/worker-control/native preparation/process integration regressions.
- [ ] Re-run 4036-document native queries/Skill/session on the fixed candidate; inspect phase timings before further optimization.
- [ ] Measure cold/warm/semantic/session process-group working set and private bytes; preserve historical not_measured values and the later measured 79ef/62d sampled observations separately; retained warm/steady remain unmeasured.
- [ ] Close legacy OP/session read-only and cancel/abandon compatibility, CSV/EXPLAIN/provider-resource gaps, then complete all four audit stages.

## Native recovery batch checkpoint (2026-10-15)

- [x] Confirm 79ef real Windows all-target and sealed CLI build; actual describe still fails on missing cancel/abandon.
- [ ] Execute reviewed CSV R4 (18 regressions plus 1 helper), Core lock-error classification, SQL required-FROM and Source root-retirement R2 on the exact composed candidate.
- [ ] Resolve slot publication residue safely: neither OS error names nor expected bytes authorize deletion; parent staging needs ACL/permission/ancestry proof before use.
- [ ] Validate native lexical path repair and removal fixture uniqueness; keep original failed run evidence.
- [ ] Retain independently reviewed binding-contracts extraction and its complete stage custody, plus actual 5/5 crate tests; execute new adapter/engine regressions.
- [ ] Obtain actual full Retail cold/warm/native/session memory and phase timing; sampler implementation and successful compilation are not measurements.

## Historical read R2 checkpoint

- [ ] Execute reviewed OP/SESSION/signal/report/capture read chain and shared workspace admission on Windows; 97 new declarations remain NOT_RUN.
- [ ] Close residual Go JSON/corrupt-DEFLATE diagnostics and deferred timestamp marshal semantics.
- [ ] Validate implemented prepared-only cancel and explicit-owner abandon with multi-document generation CAS (44 new tests NOT_RUN), then verify actual 84-command describe.

## Confirmed remaining business implementations (2026-10-15)

- [ ] Validate default Wago and supplied Raidbots Hotfix providers now committed in 62d53b2; complete Wago cancellable shared HTTP integration and actual provider/error/cache tests. Local dbcache table/scan remain dispatched; implementation is not complete provider acceptance.
- [ ] Implement remote target resolution and shared CDN read backend for DB2/SQL/navigation/assets, preserving fixed Go identity, cache, transport and resource contracts. R4.3 HTTP has 49 actual local passes; R6.1 gzip/redirect successor is source-reviewed but not yet runtime-validated, cache/target candidates still require repair and independent review. The final CLI consumers and complete CDN reader remain open.
- [ ] Connect existing partial-encrypted BLTE/DB2 primitives through production consumers, retaining missing-span and partial-capture provenance instead of rejecting every absent key.

## Owned contracts and slot preflight checkpoint

- [x] Run exact new binding-contracts integration tests locally (5/5) and retain independent existing pure group (21/21).
- [ ] Execute new recipe adapter24/engine36 and real slot preflight/Core/Windows/combined regressions; static review is not native acceptance.
- [ ] Keep uncertain publication residues and no-replay behavior; limited sharing preflight is not general residue recovery.

## Independent actual-use quality loops

- [ ] Skill: independent agent completes real CLI workflows; verify entry discovery, correct arguments, concise steps and faithful recovery without bypasses.
- [ ] Source: independent actual fixed full-corpus queries prove accurate/readable results, file/symbol/line/source access, Chinese/large/empty/error and refs/context contracts.
- [ ] Data: independent actual queries/Hotfix/SQL/export tasks prove values, product/build/pin provenance, partial/coverage/error reliability, cold/warm behavior, cancellation and no false successful captures.
- [ ] For each, independently use/review → fix → actually validate → independently review again until the frozen tree has no new actionable finding; candidate checks during implementation do not replace the four sequential final audit stages. User-assisted exceptions stay at the end of handover, with no fabricated result.

## Public API and remaining runtime closure

- [x] Independently compare public declaration rendering against the old renderer: 27+27 local focused-lock tests and 450 complete traces equal; retain the original Linux path failure and identical reviewed portability accommodation.
- [ ] Validate this reduced Emmy patch on the exact product lock, Windows, and complete Source workflows; do not infer full zero-patch equivalence.
- [ ] Execute 11 new independent Metadata budget/owner-cleanup tests and validate real Remote cache/target consumers.
- [ ] Repair the actual gzip deflate-versus-limit difference and execute all 157 differential vectors plus all Rust regressions without fail-fast hiding later cases.
- [ ] Diagnose full-Retail memory by concrete structure counts and ownership lifetimes; compare phase-matched sampled working set/private bytes after any optimization without dropping coverage or results.

- [ ] Execute four Graph ownership-move differential regressions and full-Retail memory measurements on a tree containing the moves; 62d53b2/ce6dfe4 are earlier candidates, with no measured benefit attributable to this change.

## Current remote integration follow-through

- [ ] Validate integrated fragment R9.1 (17 declared tests) and Wago R3/header seam against the exact product lock on Windows; declarations/static review are not runtime acceptance.
- [ ] Complete encoded/index/decoded/default Data consumers plus real workspace download-worker admission; preserve scoped refundable ownership.
- [ ] Resolve retained-session cold worker/pipe failure and both-direction context duplicate work with full output/coverage/budget equivalence; retain the exact 62d failure evidence.
- [ ] Measure Graph ownership moves on full Retail under the same sampled working-set/private-byte protocol; 62d excludes them and proves no reduction.

### Latest integrated candidate and remaining execution gates (2026-10-15)

- [ ] Execute cumulative Go 3.2.2 R4 slot activation/recovery regressions (21 declarations), current 220-case formal Lua suite and actual Skill/CLI against the exact next Windows commit. Explicit budget-source identity, nil-budget and historical Go journal compatibility are implemented and independently reviewed, not yet runtime-accepted.
- [ ] Validate the corrected shared 90-second Source refs/context/batch total budget with actual public CLI calls. The old 182-second refs success does not satisfy this contract. Measure request-local XML catalog reuse, conservative relation filtering and Graph ownership moves on the full corpus; retain every result/check and report failures.
- [ ] Complete remote workspace admission and default provider consumers after the reviewed R10.1 encoded-reader integration. Run new/inherited reader tests and preserve all counter, ownership and independent-cancellation evidence.
- [ ] Close the local gzip candidate's one stalled-header deadline test failure before full acceptance; all 233 fixed-Go vectors matched, but full Rust result is 71/72. Do not relabel the old 60/61 or 152/157 results.
- [ ] Diagnose Windows 500-slot fixture timeouts with actual concurrency and single-operation timing; cargo compilation jobs do not bound test threads. Do not extend production deadlines or remove durable writes/checks to obtain green tests.
