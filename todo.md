# Rust migration checklist

Authority for decisions, status and evidence: [handover.md](handover.md). This checklist gives execution order only. The first delivery is documentation; full implementation is now authorized. Unchecked work does not authorize game input, publication or main merge. Existing Rust code and historical passing tests do not check these boxes automatically.

## Current implementation rhythm (user decision, 2026-10-15 18:24 UTC)

Complete end-to-end production behavior first, then add the corresponding tests and run them in consolidated batches. Do not block each small implementation slice on a self-imposed freeze, independent review or CI gate. Persist meaningful batches to the same draft PR promptly. Preserve actual safety/ownership rules and single-writer boundaries; monitor existing CI alongside implementation. The four final sequential audit/fix/test/review loops remain mandatory after full implementation.

## 0. Documentation scope

- [x] Freeze Go v3.2.0 at 3057a9666ab078270b63c7f06de748069b0d1f4b and record Rust starting HEAD.
- [x] Define reuse/adapt/replace strategy, full parity and EmmyLua direct-library exception.
- [x] Define ≤30% controllable-time performance target and full functional/protocol matrices.
- [x] Replace README in both languages; use one DESIGN.md without case-only duplicate.
- [x] Keep only handover.md; mark old guidance/evidence historical and preserve prior README/UI text.
- [x] Put every user/game/specific-interactive-machine test at the end of handover.

- [x] Push the reviewed documentation to rust-refactor and create/reuse its own draft PR (#14); subsequent authorized code batches update that same PR promptly, without waiting for all validation or opening one PR per batch.

## Hard requirement: unmodified Emmy upstream

- [ ] Inventory every remaining production hook and actual consumer, including shared ingestion, contextual inference, identity/cardinality and diagnostic receipts.
- [ ] Execute the composed owned-session S1–S8 regressions (72 new Rust tests; 498 S2 atomic passed; S3 successor regression remains failing, with all 448 comparisons retained), implement recursive backends, and connect real consumers before removing upstream hooks; the source-only slices are independently reviewed, not production equivalence.
- [ ] Replace all upstream source changes with owned-crate behavior through the fixed unmodified public API, preserving the complete approved XML/Lua design. No minimal-hook final exemption remains.
- [ ] Prove each replacement with full output/diagnostic/coverage/cancellation/resource differentials, then exact-lock native and full Retail cold/warm/session acceptance. Keep prior failures and transitional patches honestly identified until removed.

## Current consolidated validation queue

- [ ] Validate the integrated public-miniz gzip transport with the current product lock, prefix drains, cancellation/accounting and all 233 fixed vectors; preserve the earlier 71/72 harness failure and denied rerun.
- [ ] Complete retained Source session startup/recovery. Exact bc2 ordinary full-Retail refs/context/API now have successful runs; subsequent optimizations and the performance target are still unverified.
- [ ] Finish the complete owned Emmy query/diagnostic/default-consumer chain and remove the remaining upstream source changes before final zero-patch acceptance.

## Current tagged increment: Go 3.2.3

- [x] Pin v3.2.3 tag `7a24532cf0fe294faeb96e8224797b6fe69386d5` to `3766f9cacb52e93a803a626187e2a1d54a445468`. Keep 320/321/322 source, tests and results unchanged; Rust stays 3.5.0 beta.
- [ ] Implement G323-01–G323-06 in the [increment matrix](docs/toolkit/rust-go-3.2.3-increment.md): admission capacity, adaptive Windows memory/scratch, shared consumers, compact records, IN selection and Skill/oracle integration.
- [ ] Execute the reviewed R23.1 adaptive/admission foundation (26 new Rust tests), connect real default query consumers and exact memoryBudget output; foundation composition alone does not complete G323-01–03.
- [ ] Map fixed Go323 tests and run exact-candidate local/Windows/independent Data usage checks. Current fixed-cost Rust candidates are prior implementation evidence, not new adaptive acceptance.

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
- [ ] Audit dynamic TOC, build/include, tool, package, feature and test consumers before removing 21 extra addon files, optical/WGC/LuaLS/old protocol branches or superseded guidance.
- [ ] Finish corresponding-source, npm install/upgrade/uninstall/restore, official addon-only package closure and exact final candidate validation.
- [ ] Run independent Source and Skill quality acceptance against actual CLI outputs; do not replace semantic evidence with checker-test counts.

### Performance and repository-wide convergence remain unmeasured/open

- [ ] Establish same-machine fixed-Go and Rust equivalent outputs for every B01–B07 path; independent cold/warm >=20 repeats, raw timings and excluded waits. Current isolated timings do not prove Rust <=30% of Go.
- [ ] Complete all F01–F13/P01–P14 functional rows before the four sequential whole-repository audit loops. Per-batch independent reviews do not count as Stage 1–4 convergence.
- [ ] Keep user/game cooperation H01–H09 at handover's end and unfilled until real execution; do not hold unrelated automated work for them.

### Current validation cadence and new Source priorities

- [ ] Apply the latest complete upstream-patch removal requirement: evaluate each remaining Emmy patch against the independent unpatched public-API matrix, prefer owned XML/business/adapter implementations, and replace every remaining upstream hook with equivalent owned behavior and consumer/regression evidence; no final hook exemption remains. Preserve all approved enhanced semantics; no automatic large rewrite, dynamic plugin framework or zero-maintenance promise. Session/epoch/source-map/ref provenance are not automatically engine-only responsibilities.


- [ ] Run local real pure Rust/Node/Lua regressions first within valid owned channels; preserve results and promptly push each meaningful batch. Every push runs fast checks; explicit `[rust-native]` batch commits run full Windows/Emmy acceptance. Do not treat skipped native jobs as green product validation.
- [ ] Finish Source process terminal proof and legacy luac caller retention, checked Windows process/job close and Closure I/O/control boundaries. An error with uncertain resource retirement must not trigger parent scratch deletion.
- [ ] Record actual full-Retail cold/warm/native/session synchronized process-group WorkingSet and PrivateBytes, including workers, steady samples, sampling gaps and failed-run status; existing peakRSS is not_measured.
- [ ] Validate and incrementally extend the implemented `lycheedev-emmy-adapter` and `lycheedev-binding-contracts` crates: dependency direction Source→adapter→pinned upstream, no back-reference/Graph copy/framework expansion. First move authenticated environment DTO/type conversion/domain provenance and reporting; replace the transitional recursive/session/unknown/cardinality/checker hooks without losing their contracts.
- [ ] Maintain the current 31-path patch inventory (26 source, two manifest/lock, three test paths) against production/example/test consumers and regression evidence; the earlier 29-path count is historical. Validate API Type public-upstream alternative, file diagnostic opt-in and object reconstruction before reducing hooks; keep 35 existing engine regressions and complete fixed-Retail acceptance.
- [ ] Document and test each upstream-version upgrade: fixed commit/archive/license, patch conflicts/replacements, cache identity, reproducibility, owned-adapter and full-Retail outputs/resources. No claim of zero-maintenance fork or measured performance gains from file movement.

### Newly ordered implementation work (2026-10-15)

- [x] Push reviewed formal activation/doctor/repair and default CON CLI wiring in b83a4d3/da546758. This checks delivery of code, not runtime equivalence; legacy OP/BTP and remaining cleanup cases stay open.
- [ ] Finish Source process terminal ownership and both ordered/legacy luac caller retention; independently review, locally test pure portions, then run automated Windows process fixtures.
- [ ] Retain the published independently reviewed local pure Rust runner and actual 21+5 results; continue exact production module tests locally. Keep frequent persistence pushes separate from explicit `[rust-native]` batch acceptance.
- [ ] Collect actual full Retail memory by phase using synchronized authenticated Job membership; report current WorkingSet/PrivateBytes sampled group peaks and steady observations, sampling gaps and failed/incomplete operations separately. Historical `not_measured` remains unchanged.
- [ ] Extend the implemented owned Emmy adapter with one-way Source→adapter→Emmy dependencies and real diagnostics/API-type consumers, then move domain DTO/projection/provenance/reporting out of upstream patches in small reviewed batches. Preserve the entire enhanced XML/Lua design per explicit user decision; label it separately from Go-equivalent behavior.
- [ ] Map each remaining Emmy patch to consumers and tests; preserve 35 contextual regressions/full Retail acceptance. Pin upgrades, check official public API replacements, review uncertainty/identity/cardinality boundaries and remove each hook after its owned replacement proves equivalent; all must be removed for acceptance. Do not count a wrapper-only crate as completed decoupling.
- [ ] Complete SQL meta exact Unicode EqualFold before independent review; logical resources admission (including 16×1MiB Go80MiB/Rust144MiB false rejection), paired CSV R4 is independently reviewed but native NOT_RUN; EXPLAIN/TopK R3 is implemented and independently reviewed, with 31 new tests NOT_RUN; ANALYZE/provider callback and resource gaps remain.

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

## 5. Four sequential audit loops after full implementation

Each stage repeats independent audit → actionable findings → fixes → verification → independent re-audit until no new actionable finding remains on the fixed reviewed tree. Only then advance. Record scope, commit, findings and evidence; never claim mathematical zero bugs. Human/game-dependent exceptions stay unverified in handover’s final section while independent work continues.

- [ ] Stage 1: architecture, all business bugs, Skill and all code-related paths converge; independently check idiomatic Rust ownership/borrowing, meaningful enums/type-state, Result/RAII and bounded safe concurrency, without line-by-line Go translation or needless abstraction.
- [ ] Assign a fresh independent Source evaluator: real representative CLI queries, Chinese/large/empty/error/refs/context results, correct readable file/symbol/line/source evidence.
- [ ] Assign a separate fresh independent Skill evaluator: actually follow its workflow, discover entrypoints/use correct arguments, keep steps concise and recovery current, prefer pinned Source over ad hoc GitHub raw. Record real CLI evidence and regressions, not implementer self-approval or aesthetic contract changes.
- [ ] Assign a fresh independent Data evaluator: real query/version provenance, partial results, export and failure/recovery tasks; fix, actually verify and re-audit usage quality with the Source and Skill evaluations before Stage 1 convergence.
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

- [ ] Verify real kernel Data admission R11.1 (17 declarations), then connect the full remote session and all actual encoded/decoded consumers; an implemented admission API alone does not establish aggregate download control.
- [ ] Retain the fixed Go addon ZIP closure (55 business +2 license resources) and independently passing new local regressions; resolve package version consistency in the authorized migration/package plan without pretending the remaining 41/42 result is green. Finish recursive payload consumers and old scenario mapping before deleting legacy files.
- [ ] Fix independently confirmed native incoming relation caller/reference-expression field ambiguity with exact source owner, anonymous/top-level uncertainty and cursor identity regression coverage; do not deduplicate proof rows or silently change existing pagination. Preserve full source-map evidence required by the enhanced contract while evaluating a separately compatible compact presentation.

- [x] Synchronize the approved Rust 3.5.0/beta source and package configuration; exact Go 3.2.2 overlays and independent local package checks preserve historical evidence. No publication.
- [ ] Retain exact-b16 successful Windows all-target/CLI builds, including the duplicate-helper repair; finish its actual IPC and product tests and compile later composed 3.5.0 changes independently. Local Node checks do not replace Windows.
- [ ] Execute nineteen Core supplemental budget regressions and connect all five reviewed seams to actual Data consumers; separately complete controlled streaming consumption and checked Directory lifetime.
- [ ] Execute R12.3 encoded-span regressions and existing range/installed consumers; verify retained peaks and zero-copy ownership with real composed product bytes before claiming end-to-end Data parity.

## Immediate default Data integration order

- [ ] Validate the integrated direct remote `dispatch_target` route with its seven actual CLI regressions; finish named targets through controlled configuration-only lookup and exact pin-reference/output behavior. Preserve local routing, original control/Budget, capture and provenance.
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

- [ ] 本批验证：XML child index 13 例、indexed IN 23 例、owned Function/call 11 例及原 448 比较；三方官方 Python 身份边界 Node 31 已过，Windows 尚待。IN 目前仅静态 eager provider，lazy/effective 继续；不据此勾选 G323-05 或零补丁完成。

- [ ] Execute owned Object/carrier and authenticated owned Data span/source regressions on their exact composed tree; continue member/typecheck and actual Prepared/Session consumers. The newly saved ownership pieces do not close zero-patch or cache acceptance.
