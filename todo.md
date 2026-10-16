<!-- Current candidate: production Emmy recipe is unmodified upstream (848 files, zero patches); owned Source query and 43 diagnostic entries are wired. Historical initial zero-patch compilation failures are retained; current all-targets and focused behavioral results are recorded in handover.md. Complete behavioral/resource parity remains unaccepted. -->
<!-- Preserve historical patched-engine test references separately. Complete known context/result-proof contracts, then run concentrated verification and the final five audit loops. -->
<!-- Current batch: legacy addon production extras are now zero under the fixed oracle; Node 60/60 passed. Current formal Go323 Lua220/bridge9 execution passed on its recorded tree; final five audit loops remain pending. -->
# Rust migration checklist

Authority for decisions, status and evidence: [handover.md](handover.md). This checklist gives execution order only. The first delivery is documentation; full implementation is now authorized. Unchecked work does not authorize game input, publication or main merge. Existing Rust code and historical passing tests do not check these boxes automatically.

## Current implementation rhythm (user decision, 2026-10-15 18:24 UTC)

Complete end-to-end production behavior first, then add the corresponding tests and run them in consolidated batches. Do not block each small implementation slice on a self-imposed freeze, independent review or CI gate. Persist meaningful batches to the same draft PR promptly. Preserve actual safety/ownership rules and single-writer boundaries; monitor existing CI alongside implementation. The five final sequential audit/fix/test/review loops remain mandatory after full implementation.

## Rust-native architecture acceptance (user refinement, 2026-10-16)

- [ ] During Stage 1, assess every Core/Source/Data/Live/CLI/Skill-delivery boundary for concrete ownership/borrowing, avoidable clones, typed states, compact storage, streaming, explicit heap execution and measured safe concurrency. Include cancel/error/drop behavior, actual allocation/stack/IO costs and consumer evidence. Retain already appropriate implementations; do not translate Go mechanically, rewrite everything indiscriminately or add generic frameworks without need. This is a required architectural outcome, not permission to change the fixed Go business contract or interrupt current implementation/testing.

- [ ] Execute and measure the implemented bounded Source scoped-parser candidate (available logical CPU parallelism with useful task count and actual byte-admitted workers, no fixed CPU-count cap, at most 8 ordered slots, actual Git allocation and shared retained admission, one operation-owned pool, full sequential fallback, original 26 new tests passed on 7a; six adaptive successor tests passed on f088), then close full-output/first-error/cancel/peak-memory evidence. Continue justified bounded concurrency rather than only documenting it: start with the actual Source cold-index CPU/I/O pipeline after the measured cache bottleneck, then evaluate Data independent batches and Live's required serialized state boundary. Bound both task count and in-flight bytes; preserve output order, first error, original budget/cancellation and complete verification. Go constrains public behavior, not Rust internal layout.

- [ ] Validate and measure default Data texture parallelism: full palette/BGRA/BC pixels, clipped edges, real PNG/WebP export/captures, logical-CPU/work/headroom selection, no fixed CPU cap, shared-budget races, cancellation, thread failure/panic and complete join/refund. Fourteen new Rust tests passed on f088, including sixteen real participants and default PNG/WebP exports; throughput and memory benefits remain unmeasured. Keep SQL/provider/transaction state serial where required.

### Current whole-repository consumer migrations

- [ ] Core typed pin request: migrate the actual default Data provider, preserve snapshot/project priority, CAS winner and checked-close/cancel order; run compatibility and real consumer tests.
- [ ] Data/CLI typed DB2 stream: move request/event/summary types through admission, row execution and a shared wire renderer; preserve post-prepare field errors, streaming backpressure and capture/cleanup facts; run full wire and failure tests.
- [ ] Live/CLI typed completion: replace invalid optional-state pairs across connection/activation/doctor with three exhaustive variants and consuming cleanup joins; run original JSON/JSONL/text and error-tree regressions.
- [ ] Source engine-host boundary: centralize unchanged Lua51/diagnostic configuration and authenticated-input policy, verify complete config identity; keep engine types local without copying AST/Graph or modifying upstream.
- [ ] Consolidate the four migrations with the retained Core/Data ownership models and legitimate Live-to-Delivery dependency. Then perform the mandatory whole-repository architecture audit, followed by the remaining final audit stages. Declared tests and static checks alone do not close these items.

- [ ] Verify the connected demand-original pipeline: byte-identical borrowed metadata, moved cold text, explicit empty documents, one authenticated Git request stream, bounded real-capacity memo, original cancellation and cleanup, and actual-demand cache publication. The new Git Windows group passed on 8ee; 6ce closed the remaining fallback fixture with all 19 facts-cache tests. 76d full 4036 Facts and single cold index/query outputs matched Go; complete repeated performance and remaining lifecycle acceptance are still required. Preserve full 4036/52 comparisons and measure index→precise/broad plus independent cold/warm query, since publication work moves to consumption rather than disappearing.

- [ ] Validate the implemented retained ownership fix: streaming digest, one immutable Graph control tree shared across direction-specific analysis and retained answers, full-value expansion after scope Graph release, compact declaration lookups and reused authenticated XML originals. All 18 new tests passed inside 76d native72/research76 (two original ignores), but optimized full retained still fails. The authenticated owner worker now reports 0xc00000fd while receiving/handling the child reply. Replace unnecessary owner-side payload DOM materialization with complete-validated bounded raw-frame ownership, then test default-stack valid/deep/malformed/truncated/error/cancel/drop paths and the complete request/warm lifecycle under the original 2 GiB/90-second limits. Keep d75 second-scope failure and all prior EOF evidence; syntax/Node success is not runtime memory acceptance.

- [x] Execute the independent 32-case public XML lexical diagnostic supplement and unchanged 52-case corpus against fixed Go1.27.1/Go323: e8d complete Facts/error/order comparisons all matched; Rust15/0/0 and Node18/0/0 passed, including private cache invalidation. This closes this fixed input set only; preserve XML tolerances and do not tighten unverified cases by specification. The independently seeded full 4036 v10 optimized output comparison also passed on e8d (PR comment 6226520186); retained and repeated performance were not rerun for that XML-only batch.

## 0. Documentation scope

- [x] Freeze Go v3.2.0 at 3057a9666ab078270b63c7f06de748069b0d1f4b and record Rust starting HEAD.
- [x] Define reuse/adapt/replace strategy, full parity and EmmyLua direct-library exception.
- [x] Define ≤30% controllable-time performance target and full functional/protocol matrices.
- [x] Replace README in both languages; use one DESIGN.md without case-only duplicate.
- [x] Keep only handover.md; mark old guidance/evidence historical and preserve prior README/UI text.
- [x] Put every user/game/specific-interactive-machine test at the end of handover.

- [x] Push the reviewed documentation to rust-refactor and create/reuse its own draft PR (#14); subsequent authorized code batches update that same PR promptly, without waiting for all validation or opening one PR per batch.

## Hard requirement: unmodified Emmy upstream

- [x] Inventory and replace the historical production hook consumers in the owned implementation; the current production recipe restores the exact 848-file upstream tree with no patches. This checks source integration, not behavioral acceptance.
- [ ] Execute the complete composed owned query/flow/call/member/type/diagnostic chain and original enhanced regressions, including all 448 selected-query comparisons. Keep historical S1–S8 failures and exact patched reference traces; current production consumers are connected, with actual scoped comparison results recorded in handover.md; full-corpus acceptance remains open.
- [ ] Validate that the implemented owned-crate replacements preserve the complete XML/Lua behavior through the fixed unmodified public API. No minimal-hook final exemption remains; a zero-patch recipe alone does not complete acceptance.
- [ ] Prove each replacement with full output/diagnostic/coverage/cancellation/resource differentials, then exact-lock native and full Retail cold/warm/session acceptance. Keep prior failures and transitional patches honestly identified until removed.

## Current finite closure queue (2026-10-16 19:28 UTC)

- [x] Prove normal public Facts full fields/order against fixed Go323: e843's real 4,036-document comparison has zero differing files and exact totals. This does not close malformed diagnostics, native analysis or performance.
- [x] Recheck all 52 unchanged malformed inputs against the original sealed Go report after7f's six-observation correction: actual52/52 matched. Retain e843's46 equal/6 different result and complete diagnostic/relationship ordering. Single-addon/matrix validation and native Lua5.1 remain distinct contracts.
- [ ] Complete controlled rebuildable Facts/original-byte cache publication, including missing/corrupt-cache recovery from authenticated pinned Git and real durable capture consumers. e843's measured cold index45.104s versusGo9.554s spends15.613s publishing Facts and21.874s publishing originals; these are measured aggregate wall phases, not proof that fsync alone costs that time. Preserve final index/manifest durability and all content/identity/control checks, then measure the changed complete chain.
- [ ] Revalidate affected B01/B02 cases on the corrected optimized candidate. Fixed5bbb completed: only warm query20 pairs met ≤0.30; describe/cold query failed, index output differed. Fixed408 single cold index remained non-equivalent at40.696s versus8.840s, without statistical improvement proof. Preserve all original results; do not rerun unchanged groups or count diagnostics as acceptance.
- [ ] Resolve the retained session-cold-refs EOF, still present in optimized024 at query_execute_operations, using actual process/Job and memory evidence. The authenticated 024 exit code was observed only after supervisor termination, so it is not proof of the original failure; near-limit commit without a limit notification is not proof of OOM. Ordinary full-Retail refs/context/API and static checks passed this candidate; the four retained successors lack their required first owner. Keep original90-second/2GiB bounds and exact4,036-document/XML/API/source-map input. Quota/idle have passed; the old408 real600-second lifetime run failed with exit5 at normal expiry. The ed6 fix passed8 actual unit tests; e843 real600-second lifetime completed600047ms/exit0, while retained cold native still failed.

- [ ] Reconcile current zero-patch, Data/SQL/Core/TLS and formal-Lua evidence with their exact candidate/input/test mappings, then validate only genuinely uncovered or changed consumers. ac8 Data/TLS/adapter/context, 7db shared-budget regressions, 7d Core/Metadata and 7c Go323 Lua220/bridge9 have real results. The narrow ac8 Emmy current-contract check passed while historical raw comparison remains failed; neither proves full Retail or final audit convergence.
- [ ] Keep complete B01–B07 performance, final-candidate package/install/upgrade/uninstall/restore and two fresh offline corresponding-source rebuilds open. The paired runner covers describe and static index/query only; remaining semantic/session/Data/Live-local/delivery paths and required metrics need their own equivalent-work evidence. Do not substitute historical 50%/five-sample runners or assembly-only reports.
- [ ] After the finite production failures close, begin Stage 1 with independent actual Source, Skill and Data CLI usage, then the five sequential audit/fix/actual-verify/independent-review loops. Existing SQL/provider/Emmy/source-slice reviews remain specialist reviews, not Stage 1–5 results. Keep H01–H09 last and not_run, and do not wait on game cooperation to advance independent work.

The dated checklists below preserve historical obligations. Their unchecked “implement”, “untested” and “not_run” wording does not override current handover evidence and does not require rerunning unchanged passing groups. Current execution and remaining acceptance are governed by handover.md.

## Current tagged increment: Go 3.2.3

- [x] Pin v3.2.3 tag `7a24532cf0fe294faeb96e8224797b6fe69386d5` to `3766f9cacb52e93a803a626187e2a1d54a445468`. Keep 320/321/322 source, tests and results unchanged; Rust stays 3.5.0 beta.
- [ ] Implement G323-01–G323-06 in the [increment matrix](docs/toolkit/rust-go-3.2.3-increment.md): admission capacity, adaptive Windows memory/scratch, shared consumers, compact records, IN selection and Skill/oracle integration.
- [ ] Validate the implemented adaptive/admission policy and connected local/CDN/effective/asset default consumers with exact memoryBudget output; source integration alone does not complete G323-01–03.
- [ ] Map fixed Go323 tests and run exact-candidate local/Windows/independent Data usage checks. Prior fixed-cost Rust candidates remain historical evidence, not new adaptive acceptance.

- [ ] Execute compact-record R2 (11 new Rust tests plus existing WDC regressions); verify actual Go323 rows, resource outputs and memory, not just static capacity arithmetic.
- [ ] Execute default remote asset export and existing local image/output regressions, including committed-error CLI context; finish local adaptive consumers and legacy inner close boundaries.

## Cumulative prior increment: Go 3.2.2

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

- [ ] Execute the reviewed v8 incoming-caller identity/cursor regressions and real Driver/formal-Lua nine-case bridge on the exact composed candidate; retain unresolved-owner and version-overlay boundaries.

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
- [ ] Verify the implemented retirement of 21 extra addon files and migrated current consumers in final package/Lua tests; independently inspect remaining optical/WGC/LuaLS/old-protocol dynamic consumers before any further removal.
- [ ] Finish corresponding-source, npm install/upgrade/uninstall/restore, official addon-only package closure and exact final candidate validation.
- [ ] Run independent Source and Skill quality acceptance against actual CLI outputs; do not replace semantic evidence with checker-test counts.

### Performance and repository-wide convergence remain unmeasured/open

- [ ] Establish same-machine fixed-Go and Rust equivalent outputs for every B01–B07 path; independent cold/warm >=20 repeats, raw timings and excluded waits. Current isolated timings do not prove Rust <=30% of Go. The optimized paired entry has actual Windows validation. ac8 describe cold/warm 20 pairs were equivalent but measured ratios 1.127/1.126, failing ≤0.30; static Retail had zero samples after seed rejection. The seed contract is repaired and verified; a fresh fixed-5bbb build and full paired run are queued. B03–B07 and full semantic performance remain open.
- [ ] Complete all F01–F13/P01–P14 functional rows before the five sequential whole-repository audit loops. Per-batch independent reviews do not count as Stage 1–5 convergence.
- [ ] Keep user/game cooperation H01–H09 at handover's end and unfilled until real execution; do not hold unrelated automated work for them.

### Current validation cadence and new Source priorities

- [ ] Apply the latest complete upstream-patch removal requirement: evaluate each remaining Emmy patch against the independent unpatched public-API matrix, prefer owned XML/business/adapter implementations, and replace every remaining upstream hook with equivalent owned behavior and consumer/regression evidence; no final hook exemption remains. Preserve all approved enhanced semantics; no automatic large rewrite, dynamic plugin framework or zero-maintenance promise. Session/epoch/source-map/ref provenance are not automatically engine-only responsibilities.


- [ ] Run local real pure Rust/Node/Lua regressions first within valid owned channels; preserve results and promptly push each meaningful batch. Every push runs fast checks; explicit `[rust-native]` batch commits run full Windows/Emmy acceptance. Do not treat skipped native jobs as green product validation.
- [ ] Finish Source process terminal proof and legacy luac caller retention, checked Windows process/job close and Closure I/O/control boundaries. An error with uncertain resource retirement must not trigger parent scratch deletion.
- [ ] Record actual full-Retail cold/warm/native/session synchronized process-group WorkingSet and PrivateBytes, including workers, steady samples, sampling gaps and failed-run status; existing peakRSS is not_measured.
- [ ] Validate and incrementally extend the implemented `lycheedev-emmy-adapter` and `lycheedev-binding-contracts` crates: dependency direction Source→adapter→pinned upstream, no back-reference/Graph copy/framework expansion. First move authenticated environment DTO/type conversion/domain provenance and reporting; replace the transitional recursive/session/unknown/cardinality/checker hooks without losing their contracts.
- [ ] Validate the current zero-patch 848-file production recipe, owned API Type/file diagnostics/object reconstruction and all original enhanced contracts. Preserve historical patched-engine references solely for independent test comparison; original 31-path and later 26-path inventories describe historical states.
- [ ] Document and test each upstream-version upgrade: fixed commit/archive/license, patch conflicts/replacements, cache identity, reproducibility, owned-adapter and full-Retail outputs/resources. No claim of zero-maintenance fork or measured performance gains from file movement.

### Newly ordered implementation work (2026-10-15)

- [x] Push reviewed formal activation/doctor/repair and default CON CLI wiring in b83a4d3/da546758. This checks delivery of code, not runtime equivalence; legacy OP/BTP and remaining cleanup cases stay open.
- [ ] Finish Source process terminal ownership and both ordered/legacy luac caller retention; independently review, locally test pure portions, then run automated Windows process fixtures.
- [ ] Retain the published independently reviewed local pure Rust runner and actual 21+5 results; continue exact production module tests locally. Keep frequent persistence pushes separate from explicit `[rust-native]` batch acceptance.
- [ ] Collect actual full Retail memory by phase using synchronized authenticated Job membership; report current WorkingSet/PrivateBytes sampled group peaks and steady observations, sampling gaps and failed/incomplete operations separately. Historical `not_measured` remains unchanged.
- [ ] Extend the implemented owned Emmy adapter with one-way Source→adapter→Emmy dependencies and real diagnostics/API-type consumers, then move domain DTO/projection/provenance/reporting out of upstream patches in small reviewed batches. Preserve the entire enhanced XML/Lua design per explicit user decision; label it separately from Go-equivalent behavior.
- [ ] Map each remaining Emmy patch to consumers and tests; preserve 35 contextual regressions/full Retail acceptance. Pin upgrades, check official public API replacements, review uncertainty/identity/cardinality boundaries and remove each hook after its owned replacement proves equivalent; all must be removed for acceptance. Do not count a wrapper-only crate as completed decoupling.
- [ ] Complete SQL meta exact Unicode EqualFold before independent review; logical resources admission (including 16×1MiB Go80MiB/Rust144MiB false rejection), paired CSV R4 is independently reviewed but native NOT_RUN; EXPLAIN/TopK and actual ANALYZE provider callbacks are implemented; execution-order, group/index/result logical accounting is now integrated through R25.8. Concentrated native tests and remaining parser/binder first-error parity are pending.

## 1. Reproducible foundation (no user interaction)

- [ ] Pin Go release executable/source hashes; export complete describe/help/CLI/Skill/addon oracle.
- [ ] Inventory each current Rust implementation against F01–F13: retain/adapt/replace/missing/unverified with source and test evidence.
- [ ] Use the verified fixed public Emmy upstream as the recovery base; compare consumed APIs, retain the identified transitional reconstruction with explicit provenance/license/tests only until the complete unmodified-upstream migration replaces its hooks; never pretend new bytes reproduce missing private bytes. Reconcile old custody assumptions honestly.
- [ ] Validate clean build, locked toolchain/dependencies and two independent offline corresponding-source rebuilds within owned cache limits.

## 2. Full business parity (offline first)

- [ ] Differential CLI/parser/envelopes/JSONL/exit codes, selection/products/pins/projects/evidence and budgets.
- [ ] Keep native Emmy worker/session; close full Source/validation/batch/flow/cache/crash/deadline/cleanup gaps without reducing inputs or checks.
- [ ] Import and analyze the entire fixed Retail31c7f7b corpus (all Lua/XML/TOC and inline maps), run representative file/symbol/line/source queries, refs/context/flow, cold/warm/persistent session and resource cleanup against Go; small fixtures and historical Retail19 are not acceptance.
- [ ] Complete Data DB2/SQL effective/table/scan Hotfix/providers/CASC/CDN/assets/relations and error paths.
- [ ] Reuse the released addon; reconcile installation/500-slot/upgrade/rollback/modified-file protection.
- [ ] Reuse the fixed Go3.2.3 Skill and launcher workflow; change only necessary Emmy/install/diagnostic details and actual implemented command/flag contracts.
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

## 5. Five sequential audit loops after full implementation

Each stage repeats independent audit → actionable findings → fixes → verification → independent re-audit until no new actionable finding remains on the fixed reviewed tree. Only then advance. Record scope, commit, findings and evidence; never claim mathematical zero bugs. Human/game-dependent exceptions stay unverified in handover’s final section while independent work continues.

- [ ] Stage 1: architecture, all business bugs, Skill and all code-related paths converge; independently check idiomatic Rust ownership/borrowing, meaningful enums/type-state, Result/RAII and bounded safe concurrency, without line-by-line Go translation or needless abstraction.
- [ ] Assign a fresh independent Source evaluator: real representative CLI queries, Chinese/large/empty/error/refs/context results, correct readable file/symbol/line/source evidence.
- [ ] Assign a separate fresh independent Skill evaluator: actually follow its workflow, discover entrypoints/use correct arguments, keep steps concise and recovery current, prefer pinned Source over ad hoc GitHub raw. Record real CLI evidence and regressions, not implementer self-approval or aesthetic contract changes.
- [ ] Assign a fresh independent Data evaluator: real query/version provenance, partial results, export and failure/recovery tasks; fix, actually verify and re-audit usage quality with the Source and Skill evaluations before Stage 1 convergence.
- [ ] Stage 2: test sufficiency/meaningful coverage and tests without independent value converge; add regressions/remove proven redundancy, then measure coverage.
- [ ] Map fixed Go tests to Rust equivalents, explicit non-applicability reasons or uncovered gaps; preserve recovery/unknown-result-no-replay/transaction/cancellation/protocol/empty-map/slot-migration boundaries without copying obsolete or redundant fixtures merely to inflate counts.
- [ ] Stage 3: dedicated dead-code audit including dynamic/platform/feature/tool/package consumers converges after cleanup and regression.
- [ ] Stage 4: combined whole-repository audit across all three dimensions converges after fixes, verification and independent re-review.
- [ ] Stage 5 (added by the user on 2026-10-16): whole-repository Rust-native design converges. Check crate direction, ownership/borrowing, typed states, allocations/clones, streaming, bounded CPU/task/byte admission and backpressure, cancellation/error/RAII and unsafe. Require concrete structural and measured evidence; preserve useful existing code, do not count superficial syntax or gratuitous rewrites. Fix, actually verify and independently re-audit this stage as well.
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
- [ ] Validate the integrated default target/local/CDN reader and DB2/SQL/navigation/assets consumers against fixed Go identity, cache, transport and resource contracts. Named/direct/file target and project lifecycle are now connected; current doctor/cache closure and remaining SQL parser/binder error ordering are still being completed. Historical R4.3 HTTP 49 passes prove only that earlier scope. Complete runtime acceptance of the actual current default routes.
- [ ] Connect existing partial-encrypted BLTE/DB2 primitives through production consumers, retaining missing-span and partial-capture provenance instead of rejecting every absent key.

## Owned contracts and slot preflight checkpoint

- [x] Run exact new binding-contracts integration tests locally (5/5) and retain independent existing pure group (21/21).
- [ ] Execute new recipe adapter24/engine36 and real slot preflight/Core/Windows/combined regressions; static review is not native acceptance.
- [ ] Keep uncertain publication residues and no-replay behavior; limited sharing preflight is not general residue recovery.

## Independent actual-use quality loops

- [ ] Skill: independent agent completes real CLI workflows; verify entry discovery, correct arguments, concise steps and faithful recovery without bypasses.
- [ ] Source: independent actual fixed full-corpus queries prove accurate/readable results, file/symbol/line/source access, Chinese/large/empty/error and refs/context contracts.
- [ ] Data: independent actual queries/Hotfix/SQL/export tasks prove values, product/build/pin provenance, partial/coverage/error reliability, cold/warm behavior, cancellation and no false successful captures.
- [ ] For each, independently use/review → fix → actually validate → independently review again until the frozen tree has no new actionable finding; candidate checks during implementation do not replace the five sequential final audit stages. User-assisted exceptions stay at the end of handover, with no fabricated result.

## Public API and remaining runtime closure

- [x] Independently compare public declaration rendering against the old renderer: 27+27 local focused-lock tests and 450 complete traces equal; retain the original Linux path failure and identical reviewed portability accommodation.
- [ ] Validate the restored unmodified 848-file Emmy tree plus owned semantic consumers on the exact product lock, Windows, and complete Source workflows. Source patch removal alone does not establish full behavior/resource equivalence.
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
- [ ] Validate the now-integrated default remote workspace admission, scoped provider/read sessions and single-pass decoded publication with new/inherited reader tests. Preserve every counter, ownership and independent-cancellation assertion; finish any remaining controlled lifetime gaps identified in concentrated execution.
- [ ] Close the local gzip candidate's one stalled-header deadline test failure before full acceptance; all 233 fixed-Go vectors matched, but full Rust result is 71/72. Do not relabel the old 60/61 or 152/157 results.
- [ ] Diagnose Windows 500-slot fixture timeouts with actual concurrency and single-operation timing; cargo compilation jobs do not bound test threads. Do not extend production deadlines or remove durable writes/checks to obtain green tests.

- [ ] Verify real kernel Data admission R11.1 (17 declarations), then connect the full remote session and all actual encoded/decoded consumers; an implemented admission API alone does not establish aggregate download control.
- [ ] Retain the fixed Go addon ZIP closure (55 business +2 license resources) and independently passing new local regressions; resolve package version consistency in the authorized migration/package plan without pretending the remaining 41/42 result is green. Finish recursive payload consumers and old scenario mapping before deleting legacy files.
- [ ] Fix independently confirmed native incoming relation caller/reference-expression field ambiguity with exact source owner, anonymous/top-level uncertainty and cursor identity regression coverage; do not deduplicate proof rows or silently change existing pagination. Preserve full source-map evidence required by the enhanced contract while evaluating a separately compatible compact presentation.

- [x] Synchronize the approved Rust 3.5.0/beta source and package configuration; exact Go 3.2.2 overlays and independent local package checks preserve historical evidence. No publication.
- [ ] Retain exact-b16 successful Windows all-target/CLI builds, including the duplicate-helper repair; finish its actual IPC and product tests and compile later composed 3.5.0 changes independently. Local Node checks do not replace Windows.
- [ ] Execute nineteen Core supplemental budget regressions and connect all five reviewed seams to actual Data consumers; separately complete controlled streaming consumption and checked Directory lifetime.
- [ ] Execute R12.3 encoded-span regressions and existing range/installed consumers; verify retained peaks and zero-copy ownership with real composed product bytes before claiming end-to-end Data parity.

## Immediate default Data integration order

- [ ] Validate the integrated direct and named local/remote `dispatch_target` routes. Configuration-only paged lookup, exact create-only pin-reference/output behavior and fixed-Go local preparation order are now implemented with 17 new Rust regressions; execution remains NOT_RUN. Preserve original control/Budget, committed captures and named provenance semantics.
- [ ] Validate the now-wired controlled target list/add/show/remove routes (17 new Rust declarations), including all pages, exact-case precedence, historical references, CAS generations and partial 64-reference removal. Complete target resolve --file with its own Go decoding contract; do not reuse the stricter project-only duplicate/depth rules.
- [ ] Validate the frozen decoded-alias Session candidate after its Core optional-blob dependency closes directory-walk cancellation review. Missing body alone may be a cache miss; encoded integrity, metadata mismatch, cancellation and cleanup failures remain errors.
- [ ] Connect the same scoped provider to `local_query::read_batches` / `read_files_with_keys`, then DB2/SQL/navigation. Keep planning/definitions/execution/capture within real admission ownership; EXPLAIN/schema-only and metadata-only must not trigger content scans. Do not replay callbacks on retry.
- [ ] Transfer retained body ownership without cloning complete remote buffers; preserve exact release accounting and actual source provenance. Connect streamed asset inspect/export/demux through the owned verified-blob lifetime, rather than materializing large assets.
- [ ] Run real CLI usage quality and cold/warm/cache-failure paths on these default consumers; internal Session APIs alone do not close the remote Data acceptance rows.

### 2026-10-15 15:04 当前增量

- [ ] R13c2 decoded alias与R13d2共享body retained所有权已静态独审、精确组合；9+6新增Rust回归待Windows。R17.1默认远端SQL/provider已接并独审，6项消费者测试未运行；外层Metadata owner及definitions控制另片，不能标整个Data闭合。
- [ ] b16正式Lua220项通过；完整Source/IPC在release读取被硬编码next阻断，当前3.5.0 beta共享渠道策略修复已独审，须实跑公开入口与全Retail后再测Graph/XML收益。
- [ ] b16 normal control及Mailbox三项fixture合同修正已独审，原4/1和340/2结果保留，新候选待运行。

- [ ] 本地Emmy三方A31及B部分通过，但namespace回收EACCES未验证，保留symbols lease；C/资源与当前GNU未运行。完成独立Windows验证方案审核，不绕本地未回收owner。

- [ ] definitions控制R3已静态独审并集成：原invocation/独立Budget、HTTP和解析/发布贯通；24新增Rust待真实运行。把SQL/DB2实际调用改接此接口，保外层Metadata与Core隐藏Directory close缺口，不能只凭API存在标完成。

- [ ] R18.3已接默认远端DB2 record/page/schema/search/FK及SQL外层Metadata生命周期，15新增/11继承Rust待运行；已修outer/provider预算首因与重复stop，Store/Fragments/target内部同类错误身份修复已审并组合（7新/2修），保真实120秒target上限；R19 navigation 6项消费者待运行，继续normalizer/lock同步边界及capture内部checked-close后再验完整typed取消。

- [ ] Verify reviewed Source context R2b (9 new tests), including return-reserve edge fallback and real nonempty XML mixin tail; preserve the 90-second budget and measure the actual full-corpus result before claiming speed or memory gains.
- [ ] Verify R20.2 scoped remote JSONL (9 new tests): bounded records, terminal sink failures, committed capture identity and compound cleanup errors. Complete inherited capture-internal close/control and unknown-outcome handling separately.
- [ ] Repair the 479 actual failures: same-digest concurrent blob publisher must wait under the original control, Session consumers must not expose raw cancellation tokens, and IPC/Driver fixture startup must compile/load before claiming behavioral tests ran.

- [ ] Validate Core source-single-pass staged publication (23 new tests) and scoped Session verified body (7 new tests), then connect actual asset/export/demux consumers. Preserve CKey-before-publication, original controls, known/unknown publication facts, callback-at-most-once and checked cleanup; extra bounded disk copy is an explicit cost.
- [ ] Close repeated-Session Encoding/Root/Memo reuse and one-time rootStats/rootReported parity against fixed Go323; independent cold/warm query tests do not establish same-session equivalence. Finish existing workspace-open control and external Destination consuming-close gaps.

### Current composed consumer validation (2026-10-15 17:32 UTC)

- [ ] Execute R24.1 real admitted adaptive SQL/DB2/raw/asset inspect/stream/navigation consumers and exact public memoryBudget output (22 new Rust NOT_RUN); keep fixed-budget historical identities.
- [ ] Execute repaired default effective SQL with genuine complete exact-pin cache captures and all invalid-input negatives; no production identity relaxation.
- [ ] Recompile all targets after explicit Core blob-wait child test path fix; resolve Windows three-way tool identity setup before claiming any contract execution.
- [ ] Complete actual owned Function/call successor and rerun all 448 S3 comparisons; the oracle correction is diagnostic, not a passing replacement.

- [ ] 本批验证：XML child index 13 例、indexed IN 23 例、owned Function/call 11 例及原 448 比较；三方官方 Python 身份边界 Node 31 已过，Windows 尚待。默认local/CDN/effective的lazy provider与IN子查询现已组合，97新增/40继承Rust未跑；深层UNION尾callback作用域与local decoded256MiB旧上限已修源码，新增22＋3回归未跑；自动project-pin已接原控制/预算且10新回归未跑，不据源码勾选G323-05或零补丁完成。

- [ ] Execute owned Object/carrier and authenticated owned Data span/source regressions on their exact composed tree; continue complete flow/typecheck/diagnostic consumers. Actual Prepared/Session and lazy SQL consumers are now wired; saved implementation does not close zero-patch or cache acceptance.

- [ ] 集中验证历史capture/Hotfix/effective SQL原控制与独立Budget读取链（8新回归）、默认SQL lexer/parser/lowering控制（17新回归）；公共wildcard hook停止在parse后、lowering/provider前返回。继续UNION后缀/复合首错、evidence keep/bundle/remove与Source默认存储闭环，不把语法检查算Rust通过。

- [ ] 集中执行默认evidence keep/bundle 12项新回归与owned return/union 4项管线测试；保原版普通语义及历史差异完整证据。recoverable remove与Source存储闭环后继已保存待测，已发布/unknown输出不能重放或伪撤回。

- [ ] 集中执行Source默认存储闭环25新回归：Metadata generation/单次intent消费、原scope缓存发布、manifest CAS与checked cleanup。4唯一目标已接；90秒、2GiB与不自动prune边界不变，实际Windows会话/资源尚未验收。

- [ ] 集中验证统一SQL retrieval入口21新回归与c5真实失败修复；保UNION分支CTE声明/执行scope差别，验证已实现的Pratt混合深度首错与三段非法字段分类。e8适配层41过2失败，修真实expression execution scope后保原断言重跑；workspace编译仍以真实结果为准。

- [ ] 统一重跑evidence remove15新回归、owned scope/selected checker19新回归及c5 SQL/Data修正fixture；原e8 2panic和vctip forced收尾分开验，不能只修断言就称owner正常。其余实现限已确认Go323/43checker差距，不扩SQL能力。

- [ ] 有限收口：Data assets准入/CDN已接待测，listfile/search共享transport已接待测，Wago/Raidbots控制后继也已组合待测；SQL非法字段首错/RHS深度2项和Emmy selected实际函数来源/docs统一1组已保存待测。完成默认链并集中修闭实际失败后进入第一轮架构/功能/Skill/Source/Data质量审计，不扩通用SQL能力。

- [ ] SQL最后2入口（7新回归）与Emmy实际函数来源完整消费者（17新回归）已组合，转集中native编译/旧完整差分/43checker/实际Source测试；静态扫完不算运行通过。Data三组默认入口已写齐，集中回归并修闭真实合同差异；保留共享URL兼容差异不得冒称已闭合。

- [ ] 集中验证默认local/CDN export与demux新15项及CLI两项：同query准入、单遍输出、目录租约、source/frame/manifest引用事务、完整wire与published/unknown错误事实；保既有全部asset回归，不以静态解析当运行通过。

- [ ] 集中执行listfile/search默认链17新回归：原120秒/五请求/gzip/停止、真实loopback取消、单Metadata raw/generation CAS/checked关闭及Go重复/null/time/depth；保持共享编码点URL拒绝差异可见并在完整合同验收中解决。

- [ ] 集中验证Hotfix/共享CSV15新回归及CLI完整envelope：Wago同owner receipt/CAS、独立预算在途取消、Raidbots受控原文件/解码、query/CSV已提交1..2 ID、data→manifest发布/unknown与checked多因；raw URI和合法异常offset比较/晚期marshal差异仍需修复，不能作为永久豁免。
