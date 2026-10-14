> Historical reference for the Rust migration (2026-10-14): current decisions, baseline and acceptance status are in [handover.md](../../handover.md). This document preserves its original scope and evidence; conflicting requirements, protocol designs, performance targets and release instructions are superseded. Historical test results are not current-candidate passes.

# Source/Data next contracts — read-only preparation

This plan does not modify compilation inputs. It follows native quota run05;
that scoped pass does not close Source lifecycle, AV causality or performance.
Implementation waits for the root-granted window after Foundation's Assets checks.

## Shared reader profile

Keep the existing Data `read_files_budgeted(inv, pin, ids, &Arc<Budget>)` path and
defaults. Add one explicit shared profile entry for callers that have already
admitted their public contract, returning the existing `File { bytes, reading }`.
Assets supplies its selected content maximum; it continues to own exactly one
body and passes the same Budget, deadline and OperationControl. No alternate
provider, replacement body or new caller budget is introduced.

The profile separates encoded/decoded selected-content ceilings from Encoding/
Root/configuration metadata ceilings. The frozen Go `FileQuery` contract bounds
encoded and decoded sizes under the corresponding content/metadata allowance.
Parsing `--max-bytes=512MiB` alone does not prove a 512MiB workload fits the whole
query: Go and Rust both default to retained512MiB/metadata128MiB/network1GiB/
4096 requests/decode-work100000000. Preserve those whole-query limits and honest
refusal; test at least one compressed content file above256MiB that fits them.

| Existing boundary | Required narrow treatment |
| --- | --- |
| local_query.rs parses max≤256MiB | Profile validates Assets≤512MiB and preserves the Data default/ceiling. |
| casc.rs open_local_reader/open_local encoded≤256MiB | Use selected profile ceilings; keep index64MiB, bounded reads/chunks, directory limits and existing caller control. |
| decoded_cache.rs decoded≤256MiB | Admit selected decoded size through the same profile; retain full CKey check and current encoded-source rehash before decoded cache reuse. |
| blte.rs full decode decoded≤256MiB | Carry selected content ceiling while keeping chunk≤64MiB, chunks≤65536 and depth≤16. |
| decoded_cache reserves max*2 and commits it permanently | Separate actual returned body ownership from bounded temporary decoder scratch, releasing scratch only when dropped; preserve conservative accounting and failed-attempt work. Widening hard limits alone cannot fix this boundary. |
| local_query root/build/cdn/content publication and decoded-cache publication | Forward the original OperationControl to Core publish_blob_bounded. No fresh control/deadline or publication outside the admitted operation. |

The current local content path also materializes the entire encoded object.
Large-profile support needs the existing ByteSource/range-reader seam to borrow
bounded chunks instead of keeping an unnecessary second complete body. The
public File/reading shape stays stable. This is an internal resource correction,
not a budget increase or weakened validation.

Before implementation, freeze matching Go fixtures for: unchanged Data defaults;
256MiB boundary and a compressible >256MiB content object; caller-budget refusal;
encoded versus decoded maxima; missing/encrypted keys and same-source lazy retry;
corrupt EKey/chunk/CKey/cache; cancellation during read/decode/publication; and
no complete publication/capture after a rejected or partial body. Large fixtures
must be generated under the owner channel and used in exclusive intervals.

## Explicit CDN source

Current Rust local_query refuses --cdn or missing installation. Existing
availability/definitions/key HTTP helpers are not a CASC CDN implementation.
The authoritative frozen Go source chain is reader.go/file_source.go/cdn_files.go
and its range/configuration/archive helpers. Reuse one prepared source abstraction
for local and explicitly selected CDN reads, behind the existing shared reader.

Required behavior before public parity is claimed:

- Bind product/region/full build/BuildConfig/CDNConfig/locale/definition commit
  to the selected immutable DataPin; local and CDN choices stay explicit.
- Authenticate regional distribution and build/CDN configurations, then support
  standalone objects and archive/index locations. Root and content need whole
  decoded CKey verification; Encoding claims remain framed EKey/requested-page
  checks rather than an invented whole Encoding CKey scan.
- Validate exact HTTP ranges, total sizes, status/headers/identity encoding and
  bounded response bodies. Share network/decode/metadata budgets and the existing
  deadline/control; failed mirrors and refreshes still consume work.
- Preserve cache fragment identity by route path/object/offset/length; cache
  fragments do not become complete-content evidence. Offline missing data fails
  explicitly and never causes network access. Cache corruption remains an error.
- Match frozen Go's bounded mirror behavior and at most one justified route
  refresh; no local/CDN fallback or retry on invalid data/resource/cancel errors.
- Keep TLS verification and redirects explicit. Test transport injection may
  validate protocol logic without changing the machine trust store; it cannot
  stand in for public platform-TLS evidence.

Start with fixed Go differentials for offline authenticated CDN fixtures and
unit/transport cases: exact206, bad Content-Range/length/total, disallowed200,
truncated range, malformed configuration, wrong product/build, archive missing,
uncertain mirror failure versus definite404, one refresh, cache poisoning, key
retry, cancel/deadline and aggregate budget exhaustion. Then run the same sealed
public CLI on actual pinned HTTPS/CDN input. CDN remains unavailable until that
full source chain is implemented and its scope verified.

## Public Source lifecycle and causality

Use one fresh sealed CLI/PDB plus unchanged1307 pinned resources and the complete
Retail input manifest. The last normal run is sol03 on b748/source411d; do not
automatically transfer it to a later candidate. Native run05 uses a lib-test
artifact and does not exercise LuaLS/public CLI.

Freeze a public lifecycle matrix before running it:

1. Absent/status, full-source request, same-session independent request, exact
   held broker and worker identities, actual batches and complete coverage.
   Full Retail's partial environment catalog currently makes semantic cacheHit
   false; prove worker/index reuse without relabelling incomplete cache admission.
2. Bound in-flight caller cancellation to its exact process/control signal;
   verify typed cancellation or explicitly unknown interruption, worker retirement,
   subsequent authenticated status, and fresh reopen. Keep hard caller termination
   separate from graceful cancellation and never infer replay/success.
3. Authenticated close, idempotent close, reopen with new held identities and
   fixed input, then prune with verified leases/handles and preserved source pin.
   Finish with no broker/worker/runtime/input lease leak and no repeated resource
   accumulation. Synthetic pipe fixtures cannot prove production leases.
4. Authenticated complete small catalog exercises genuine semantic cache hits;
   full Retail remains unchanged for normal supported load. Report these scopes
   separately, including unknown relation/capability gaps.

The historical stage11 AV, later owner allocation abort and1.75GiB normal second
request failure retain their original evidence. One repaired normal two-batch
pass and native quota/tree cleanup are insufficient for causal closure. Follow
new failures with the exact artifact/symbols and preserve first failure records.
Repeat stability and the fixed20-sample Go/Rust performance matrix remain separate
required work, with no source pruning/query reduction/budget changes to pass.
