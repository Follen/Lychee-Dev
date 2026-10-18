# Stage 1 — architecture, functionality, bugs and Skill

Started 2026-10-17 21:40 UTC. Production baseline: `b4d8b22a666648ee0fe18a3f2656a84c586e5bd3`; documentation baseline: `7753776964af6b84a0130e1afaf46bb65497dd8b`. Behavioral oracle: Go 3.2.3 `3766f9cacb52e93a803a626187e2a1d54a445468`.

Three independent read-only reviews covered architecture/public consumers, Source/Data quality, and actual Skill workflows. Findings below are source-confirmed, not newly executed failures. A code candidate is not a closed finding: each needs exact-tree offline verification, independent re-review, and another Stage 1 pass. Stage 2 has not started. Performance remains open and does not block this correctness audit.

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
| COR-4 | init/archive publication chain lacks original control beyond entry checks | Confirmed, subsequent repair pending | Cancellation during actual planning/staging/moves/initialize; completed archive facts and checked cleanup retained |
| DAT-1 | auras/summons local consumers bypass admission/adaptive query budget | Code candidate, offline pending | Local/CDN both hold original admission through capture; true wait/cancel/release; memoryBudget and logical resources |
| DAT-2 | Budget errors omit used/requested/effective limit | Code candidate, offline pending | Five dimensions, fixed and fresh-pressure limits, unchanged ledger after rejection, public envelope |
| DAT-3 | Ordinary SQL parser errors lose structured location and original diagnostic | Code candidate, offline pending | Typed token location, UTF-8/CRLF/EOF/trailing tokens, no capture before failed compilation |
| DAT-4 | Missing-key partial table support is disconnected from default local/CDN consumers | Confirmed, implementing | One clear/one encrypted WDC section; readable Value42 through DB2/SQL/navigation/stream/CSV/captures; raw assets strict; no full-CKey alias for partial bytes; key recovery |
| SRC-1 | inspect routes file paths as symbol/asset searches instead of real inspection | Confirmed, implementing | Exact/fuzzy documents and assets, shared limit, six-line excerpt, authenticated bytes and asset metadata/capture |
| SRC-2 | context --flow recursively expands XML/TOC without total work bound/control | Confirmed, implementing | Explicit work stack, repeated-load order, active-cycle behavior, empty-leaf DAG limit, cancellation and deep-chain default stack |
| SRC-3 | Asset path case folding is ASCII-only | Confirmed, implementing | Unicode/slash/ASCII cold and warm lookup; current cache identities/metadata rebuild |

The first bounded candidate contains Delivery, Probe, Core prune/init-mode handling, Data admission and diagnostic repairs. Independent re-review confirmed the installation exclusion now spans recovery revalidation through mutation; successful acquired owners use explicit close and retain committed facts. Compound `workspace.busy` cleanup errors are no longer skipped. A further diagnostic review found sqlparser quoted identifiers can have empty AST spans: CAST type and qualified-name rejection now use the original token location, with the original 20 cases plus one qualified-name regression.

Still open within DLV-1: InstallationGuard acquisition failures can drop the maintenance owner implicitly; default addon remove also retains two implicit Drop paths. COR-4 remains open. These are subsequent scoped repairs, not claimed fixed by entry checks or the current successful-install cleanup changes.

First candidate verification: 23 new Delivery tests, 11 Probe tests, 15 Core/CLI-init tests, 7 admission tests and 8 diagnostic test entries (including the shared 21-input location matrix) are written, all Rust NOT_RUN. Two existing Node files passed 10/10; rustfmt/diff checks and independent static review are not execution evidence. Source repairs and complete missing-key partial consumers remain a separate code candidate and are excluded from this bounded upload.

## Real Skill acceptance still open

The fixed Go 3.2.3 Data query workflow requires the actual packaged Skill route: describe/help, target, DB2 schema, parameterized IN query, CSV export, both capture manifests and `evidence verify`. Use the authenticated HouseDecor fixture, exact row/column/order/duplicate semantics and actual lookup metrics. The partial fixture must contain a readable section, not only an entirely encrypted file.

The recovery workflow requires actual bounded process/Job memory pressure reflected in native headroom, a normal budget rejection with complete diagnostic values, full retirement, and one same-pin retry after pressure removal. A mocked probe is supplemental only; an OS kill or timeout does not prove budget recovery. The root owns the Windows test queue. No game or input operation is part of these offline tasks.

## Coverage limitations and non-findings

The first pass did not exhaust every Core management consumer, SQL operator, Hotfix layout, codec, native engine/session branch, or Windows process/unsafe site. Current Windows process helpers lacking per-site SAFETY explanations are an audit obligation; this alone does not prove memory unsafety. The selected formal Live intent/publication/input/receipt path produced no new confirmed unknown-execution replay defect in this pass.

The existing parentArray semantic partial gap is not promoted into a newly required feature without a public contract. Preserve its truthful coverage. Original Emmy remains unmodified. Historical passing tests, partial results, performance misses and runtime failures retain their original identities.

Exit condition: repair and actually verify confirmed defects, independently review the repaired tree, repeat Stage 1 review until no new actionable finding remains. This is not a mathematical guarantee of no bugs and does not complete the remaining four audit stages.
