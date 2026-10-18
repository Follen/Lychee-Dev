# Stage 1 — architecture, functionality, bugs and Skill

Started 2026-10-17 21:40 UTC. Production baseline: `b4d8b22a666648ee0fe18a3f2656a84c586e5bd3`; documentation baseline: `7753776964af6b84a0130e1afaf46bb65497dd8b`. Behavioral oracle: Go 3.2.3 `3766f9cacb52e93a803a626187e2a1d54a445468`.

Three independent read-only reviews covered architecture/public consumers, Source/Data quality, and actual Skill workflows. Findings below distinguish source-confirmed defects from exact-tree Windows results. A code candidate is not a closed finding: each needs exact-tree offline verification, independent re-review, and another Stage 1 pass. Stage 2 has not started. Performance remains open and does not block this correctness audit.

## Confirmed findings and repair ownership

| ID | Actual consumer and defect | Current status | Required closure evidence |
|---|---|---|---|
| DLV-1 | Default install/remove/update loses original cancellation before publication/moves | Code candidate, offline pending | Cancel during reads/staging/moves; no new publication after stop; already committed facts and cleanup errors preserved |
| DLV-2 | Default update re-installs remembered absent targets | Code candidate, offline pending | Remember/remove/default update stays absent; explicit selection follows intended contract |
| DLV-3 | New release bypasses an old release's pending replacement/cleanup | Code candidate, offline pending | Original-release resume, conflicting pending records, old-moved and cleanup recovery |
| DLV-4 | Receipt equality incorrectly depends on resource array order | Code candidate, offline pending | Reordered equivalent resources accepted; duplicate path, missing resource and different content rejected |
| PRB-1 | Probe list applies limit before filtering tombstones | Code candidate, offline pending | Keyset pages return the requested number of active names; include-removed order and all-removed termination |
| PRB-2 | Probe put separately commits revision/name; put/list/remove miss key identity | Code candidate, offline pending | Invalid selector leaves no new revision; either CAS failure rolls back both changes; mismatched keys rejected |
| PRB-3 | Probe stat then unbounded path read can publish invalid or oversized code; CRUD loses control | Code candidate, offline pending | Same-handle bounded growth/truncation/replacement; cancellation before commit; checked file/metadata/lease close |
| COR-1 | Cache prune scans/deletes without original control | Code candidate, offline pending | Cancellation during scan/lease/object/staging boundaries; retain actual partial deletion facts |
| COR-2 | Cache prune omits staging bytes from before/after/reclaimed | Code candidate, offline pending | Staging-only 7/0/7, no-uncommitted 7/7/0, mixed and dry-run |
| COR-3 | init ignores resume, rejects standalone plan and lacks mode exclusivity | Code candidate, offline pending | Missing journal causes no writes; planned/moved resume; independent read-only plan; mutually exclusive modes |
| COR-4 | init/archive publication chain lacks original control beyond entry checks | Saved e974 candidate, fresh review passed; offline pending | Cancellation during actual planning/staging/moves/initialize; completed archive facts and checked cleanup retained |
| DAT-1 | auras/summons local consumers bypass admission/adaptive query budget | Code candidate, offline pending | Local/CDN both hold original admission through capture; true wait/cancel/release; memoryBudget and logical resources |
| DAT-2 | Budget errors omit used/requested/effective limit | Code candidate, offline pending | Five dimensions, fixed and fresh-pressure limits, unchanged ledger after rejection, public envelope |
| DAT-3 | Ordinary SQL parser errors lose structured location and original diagnostic | Code candidate, offline pending | Typed token location, UTF-8/CRLF/EOF/trailing tokens, no capture before failed compilation |
| DAT-4 | Missing-key partial table support is disconnected from default local/CDN consumers | Rebuilt candidate independently reviewed; offline pending | One clear/one encrypted WDC section; readable Value42 through DB2/SQL/navigation/stream/CSV/captures; raw assets strict; no full-CKey alias for partial bytes; key recovery |
| SRC-1 | inspect routes file paths as symbol/asset searches instead of real inspection | Rebuilt candidate independently reviewed; offline pending | Exact/fuzzy documents and assets, shared limit, six-line excerpt, authenticated bytes and asset metadata/capture |
| SRC-2 | context --flow recursively expands XML/TOC without total work bound/control | Rebuilt candidate independently reviewed; offline pending | Explicit work stack, repeated-load order, active-cycle behavior, empty-leaf DAG limit, cancellation and deep-chain default stack |
| SRC-3 | Asset path case folding is ASCII-only | Rebuilt candidate independently reviewed; offline pending | Unicode/slash/ASCII cold and warm lookup; current cache identities/metadata rebuild |

## Current exact-tree evidence and reconstructed candidates

First39 commit `6a93a859d80efd6d757fcaa8b9e85be6285a734d`: all-target compilation passed; Windows groups 567 passed / 2 failed / 0 ignored. All64 new entries executed:62 passed/2 failed. Delivery128, Core cache59, Live probes12, Data navigate16/resource41/relational302/syntax3, CLI syntax1 and BLTE1 passed; CLI init4 passed/2 failed. [Actual failure evidence](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6244909468). All11 owners retired with no deadline; the CLI init native receipt retains forced=true, knownAuxiliaryCleanup=false and deadlineHit=false; the auxiliary classification is unknown. The211-file archive was locally verified; its final public delivery is not established by this document.

The init path test assumed a literal slash spelling; its successor retains mixed-slash input and asserts the platform-composed exact path. The identity test exposed fixed-nine-digit creation versus canonical reopening of the same instant. COR-5 below records the production formatting difference; no identity recreation or corruption is established. The successor asserts complete decoded identity, original marker bytes and repeat-resume equality, with3 deterministic/real-initializer/historical-marker tests. NOT_RUN.

Second10 commit `e9749e9d851cfefa9a02b4b8865aa5cbba3f80ef`: all10 approved blobs/full bytes verified. DLV-1 cleanup four files match their original independently reviewed bytes; COR-4 four files were reconstructed and freshly reviewed. Their12+8 new Rust tests remain NOT_RUN, including one Unix-only entry. No runtime finding is closed by these static reviews.

Source9+reader3 is freshly reviewed: five production/test files exactly recovered, four reconstructed;37 Rust tests NOT_RUN. Node247/247 passed for the current reader. Data partial30 is freshly reviewed with10 new tests NOT_RUN; it preserves base missing-row behavior for SQL/navigation and direct-DB2-specific unavailable-ID errors, and repairs the local full-alias/offline-public-key fallback. Capture8 adds6 tests NOT_RUN. Skill10 uses the partial batch's shared factory and has fresh independent Node62/62, but no actual Windows launcher/exporter acceptance. These candidates never inherit pre-reset test results.

Additional confirmed findings:

| ID | Consumer and defect | Status | Closure evidence |
|---|---|---|---|
| DAT-5 | DB2/navigation/catalog/encounter drop already committed capture IDs when later session/scope cleanup or the next commit fails | Separate candidate independently reviewed; offline pending | Actual capture followed by original cancellation/typed close diagnostic; exact ID/body/hash remains readable; original errors retained, no replay; Rust-only tracking stays safety-only |
| COR-5 | New workspace identity uses fixed nine fractional digits while reopen returns Go canonical time spelling | Separate candidate independently reviewed; offline pending | Same instant/clock/ID derivation, canonical new persistent and returned identity; historical marker bytes unchanged; zero/seven/nine digit regression |

DAT-5 tests inject a typed error after the actual metadata close; encounter cancellation is before the second commit's precheck. They do not prove an arbitrary native-close failure or interruption during the second commit. Previously confirmed findings remain open until affected execution and a further independent Stage1 review establish closure.

## Real Skill acceptance still open

The fixed Go 3.2.3 Data query workflow requires the actual packaged Skill route: describe/help, target, DB2 schema, parameterized IN query, CSV export, both capture manifests and `evidence verify`. Use the authenticated HouseDecor fixture, exact row/column/order/duplicate semantics and actual lookup metrics. The partial fixture must contain a readable section, not only an entirely encrypted file.

The recovery workflow requires actual bounded process/Job memory pressure reflected in native headroom, a normal budget rejection with complete diagnostic values, full retirement, and one same-pin retry after pressure removal. A mocked probe is supplemental only; an OS kill or timeout does not prove budget recovery. The root owns the Windows test queue. No game or input operation is part of these offline tasks.

## Coverage limitations and non-findings

The first pass did not exhaust every Core management consumer, SQL operator, Hotfix layout, codec, native engine/session branch, or Windows process/unsafe site. Current Windows process helpers lacking per-site SAFETY explanations are an audit obligation; this alone does not prove memory unsafety. The selected formal Live intent/publication/input/receipt path produced no new confirmed unknown-execution replay defect in this pass.

The existing parentArray semantic partial gap is not promoted into a newly required feature without a public contract. Preserve its truthful coverage. Original Emmy remains unmodified. Historical passing tests, partial results, performance misses and runtime failures retain their original identities.

Exit condition: repair and actually verify confirmed defects, independently review the repaired tree, repeat Stage 1 review until no new actionable finding remains. This is not a mathematical guarantee of no bugs and does not complete the remaining four audit stages.

## Second independent consumer review — cbff, 2026-10-18 04:15 UTC

The verified60-file payload (58 changed files;2 identical to the parent) is published as `cbff043e8cfe54cf30909c61dde94ce287736fb0`; all60 Git blobs and1,578,501 bytes match the frozen payload. Its Windows queue is pending results. The preceding e974 run actually finished23 passed/4 failed/0 ignored; all19 Windows additions ran (15 passed/4 failed), while the Unix-only addition was not compiled. The four initializer/archive failures all compare nine-digit creation against canonical trailing-zero removal. The successor changes creation before persistence and returning the same identity; runtime closure is still pending. See [e974 final result](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6245217858). All5 owners retired; removal retains successful native/wrapper exits with forced=true, knownAuxiliaryCleanup=true and deadline=false. Its91-file archive is locally sealed only. The older6a93 [211-file public delivery](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6245133188) is now verified; earlier freeze-time statements above remain historical.

Three new reviewers, independent of the respective writers, checked current Source output/cache/flow, Data integrity/capture and installed Skill consumers. No new confirmed Source or Skill defect was found. This does not replace the37 Source Rust tests or actual v15/Windows Skill acceptance. The reader is scoped to fresh and explicit historical continuations, not arbitrary mixed-version cache directories; exhaustive Unicode-table equivalence is unproven.

| ID | Confirmed consumer defect | Status | Required closure |
|---|---|---|---|
| DAT-6 | A readable WDC row points to a complete NUL-terminated string, but bulk suffix reading crosses a later missing-key span and wrongly rejects the readable value | Six-file narrow repair independently reviewed;6 new Rust tests NOT_RUN | Same-block/cross-block NUL versus missing span, missing-before-NUL refusal, absent NUL, boundaries, cancellation/budget, real local/CDN consumers |

DAT-6 brings confirmed findings to21. Two further observations remain non-blocking candidates: empty decoded N chunks on the partial route may be a permissible broader format interpretation; encounter's second post-commit close error retains its capture identity in the nested primary error although the outer completed list contains only the first ID. Neither is currently established as lost evidence or a public-contract defect. Do not silently change acceptance to mimic Go's internal implementation.

## cbff partial execution update — 2026-10-18 04:28 UTC

All-targets passed. Workspace33/0 (including3 new canonical tests), archive4/0 and CLI init6/0 now verify the timestamp/path successors against the earlier failures; retain original failed runs. Source lib667 passed/4 ignored, source_inspect13 passed/2 failed, and CLI inspect6/0 passed. [Exact Source evidence](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6245595382).

The asset recovery failure is a real SRC-1 implementation gap: its64MiB request maximum was rejected by the generic32MiB source-entry admission before reading a24-byte PNG. A narrow successor uses an asset-specific entry with the pre-existing64MiB bound and unchanged pinned commit/tree/regular-leaf/framed SHA-1 checks; the document32MiB limit stays intact. Two new tests and strengthened original recovery/corruption tests are Rust NOT_RUN. The other failure is an incorrect test expectation: cold full-index Rust IO omits cachedIndex, whereas warm sets true. The corrected assertion requires absence plus exact cold/warm work counts and complete coverage/capture equality; Go does not expose those Rust IO diagnostics.

The failed Source owner retired; forced=true, knownAuxiliaryCleanup=false and deadline=false remain recorded. Data/Node/Skill have no reported terminal result yet. DAT-6 is saved separately as c9cd3e729c6b4b0b33eeac81a5adbcac1d8b9ccd; its6 new Rust tests remain pending. No new confirmed finding count is added for these SRC-1 follow-up failures. Stage1 remains open.

## cbff completed preflight — 2026-10-18 04:34 UTC

[Final result](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6245678877):1881 passed/9 failed/5 ignored across7 Rust invocations;56 new formal functions ran once each,47 passed/9 failed, with the ignored exporter not started. Data lib1152/7/1 stopped before WDC/catalog integration targets. Source2 failures retain their original evidence; the independently reviewed successor is22da4290f352daf10981568d9eee8ee3c1f2bcfd, based on DAT-6 c9cd, with runtime verification pending.

The [seven Data failures](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6245645368) identify test setup gaps:4 BLTE partial leaves and1 table availability leaf use an unadmitted query budget;2 shared local/CDN consumer assertions submit an empty required search query. The three-file test-only correction is independently reviewed: fixed controlled default limits and original control for leaf tests, with valid query42 and its returned-query assertion for the real consumers. Production admission and all complete/partial rows, capture and recovery assertions remain intact. No new tests are added; the7 existing failed entries are NOT_RUN for this revision. No additional confirmed production finding is claimed from these errors alone.

Original Node253 passed/54 failed/2 skipped remains historical: the test coordinator leaked its external scratch override into a child environment whose existing policy forbids it. A separate corrected-environment run with unchanged argv/tools/budgets passed307/0/2;2 Windows file-symlink creation EPERM cases remain unexecuted. All9 owners sealed/retired/reaped; source/data forced and unknown auxiliary flags remain unchanged. The164-file archive is locally sealed only. Release/assembly/exporter/real Skill remain NOT_RUN. Stage1 remains open.
