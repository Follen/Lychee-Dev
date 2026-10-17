# First paired performance acceptance entry

This entry has real Windows execution evidence. The [first ac8 optimized run](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6220085568) completed 20 cold and 20 warm describe pairs with equivalent output but Rust/Go median ratios 1.127/1.126, failing the ≤0.30 target. Static Retail produced no timed samples after seed validation found the subsequently repaired metadata-generation contract. A new fixed-5bbb optimized build and complete paired run are queued; no result is implied before execution. See [handover](../../handover.md) for the current exact candidate and results. Existing dev-profile CI, the historical five-sample/50% script, and prior failures retain their original meanings.

## Selected scope and pass conditions

- B01: real public `describe` invocation, first/repeated invocation only.
- First B02 entries: full fixed Retail `source index` and precise
  `source query GetActionButtonForID --topic lua --limit 10`, each on its own
  independent application-cache-cold home, followed by the identical warm
  command against that engine's own home and pin.
- Preserve the actual route deadlines: `source index/query` use the existing 180-second product default and 210-second outer invocation cap; `source refs/context/batch` use their existing 90-second product budget. Do not relabel index/query as 90-second routes or increase either deadline to obtain a sample.
- Exactly 20 paired iterations per path and cache group; Go/Rust execution
  order alternates. Every output is compared, not only the last sample.
- Each group independently requires median Rust/Go <= 0.30. Reports preserve
  every timing, median, nearest-rank p95, min/max, raw stdout/stderr hashes,
  process receipts, and actual exit/error state. No pooling across paths.
- Missing, timed-out, malformed, non-equivalent or unreaped samples prevent a
  pass. A correctness failure stops that path rather than replacing the sample;
  remaining iterations are visibly missing. Other independent paths continue
  when process ownership is safely resolved. No warm label follows failed cold.
- `passed_selected_paths_full_acceptance_open` never means complete B01–B07,
  complete Source, full F/P qualification, session acceptance, or release-ready.

Both engines must actually run `init` and `source sync` on fresh private seed
homes. Each closed seed is independently authenticated against all 4049 Git
blobs, including all 4036 Lua/XML/TOC documents and 32,269,051 document bytes at
Gethe/wow-ui-source commit `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b`.
The two real pinned selections must agree. Checkpointed SQLite has the fixed
Go/Rust `documents` and `deleted_generations` tables, exactly that pin, no
cached-analysis documents and no deleted generations; WAL/SHM/journal and derived Source
roots are rejected. Only that engine's seed is copied to its independent pair
home, with every copied byte rechecked. No Go metadata is used as Rust admission.

Warm proof records derived file hashes, file IDs, creation and modification
timestamps before and after the actual repeated command. Go's fixed `openIndex`
path must retain its complete snapshot files; Rust must additionally expose its
real public `io.cachedIndex=true` and `parsedDocuments=0`. Missing proof is
`same_home_repeated_reuse_unproved`, not accepted warm reuse. The B01 command
has no application cache and makes no cache-hit claim. This is application-cache
cold/warm: OS file/page caches are not flushed, and no OS-cold claim is made.

Index schema/storage/parser are strict shared public fields, as are every
business count, completion flag and diagnostic. Query comparison keeps all Go
public semantic fields, rows/order, symbol IDs, original text/hash/coordinates,
scores, relations, suggestions, cursor and coverage. Rust-only Emmy provenance
and I/O proof fields are retained raw rather than demanded from Go. Describe
allows only the existing two exact reviewed engine-summary substitutions.

## Timing, metrics and retained evidence

Each CLI runs in its own nested non-breakaway Windows Job, inside one outer Job
owning the controller and every preparation/audit/sample descendant. The shared
owner adds a monotonic timestamp immediately before `CreateProcessW` and after
root/tree reaping and stdout/stderr drain. PowerShell/Add-Type startup is outside
that interval; all actual CLI validation, local reads/writes, worker work and
reaping remain inside. Native owner/observer startup, scheduling and output
drain overhead inside the interval are included without estimated subtraction.
Observer shutdown is outside the interval but still must complete before a
sample can be accepted. Full wrapper and whole-run wall times are also reported.

Sync is recorded as inseparable network plus local preparation. It is never
subtracted as if entirely download time, and this entry makes no local-sync
performance claim. Fixture copying and independent audit are separately
reported setup. All product-side validation remains enabled in timed commands.

Windows memory is the existing identity-deduplicated owned-Job sampled aggregate,
with gaps explicitly reported. Overlapping outer and inner Job counters are
never summed. Shared pages may still be counted in more than one process's
working set; this is not unique physical RSS. CPU and I/O remain `not_measured`.
The runner does not fabricate those metrics or full resource acceptance.

Each pair uses existing scratch admission and sealing. Only successful, reaped
owners are cleaned up. Failed and interrupted fixtures are retained with raw
output, input/command hashes and ownership receipts. Failed outer preparation is
also retained; no process scanning, broad kill, lease changes or cache recovery
is performed.

## Actual preparation and execution

The standalone `.github/workflows/rust-performance.yml` is opt-in only:
`[rust-performance]` in a dedicated branch push, or explicit workflow dispatch.
It is not part of ordinary push acceptance and has not been dispatched for this
change. It prepares both candidates sequentially on one `windows-2025` runner,
runs the native owner tests, builds optimized Rust, prepares a verified complete
Go package, invokes the paired runner and uploads all sealed reports plus
failed owned fixtures. Hosted physical load/thermal/storage limits are recorded
as unknown, not claimed controlled. No user computer or game is required.

The fixed Go checkout is a real clean git checkout at
`3766f9cacb52e93a803a626187e2a1d54a445468`. The existing fixed release's own
`tools/release.mjs dev-package` is run under the owner with Go 1.27.1, CGO=0,
trimpath and VCS stamps. The resulting complete private package is required:
a bare executable is rejected. Both checkout identity and `go version -m`
must prove the exact commit and `vcs.modified=false`. Every resource is checked
against release.json, and every LuaLS runtime file against the official pinned
3.19.1 archive (4,453,980 bytes; SHA-256
`fdb9a59108cf62517813c97fa5549b0e16d1ef0688306bac728b08434db7e4cd`).
The preparer does not publish or modify the baseline source. `GOCACHE`,
`GOMODCACHE` and `GOTMPDIR` are private directories in its registered owner
scratch; `GOENV=off` ignores user Go configuration and `GOTOOLCHAIN=local`
prevents implicit toolchain downloads. Dependency downloads still use the
inherited process proxy settings or Go's official defaults and remain part of
separately recorded preparation.

After dependencies and official toolchains are available, the same executable
entries can be used directly from the candidate root on Windows:

```text
node tools/rust-build.mjs .tmp/paired-rust-artifact release --one-shot
node tools/rust-performance-prepare-go.mjs <clean-fixed-Go-checkout> <official-LuaLS.zip> <installed-npm-cli.js> .tmp/paired-go-artifact
node tools/rust-performance-paired.mjs .tmp/paired-rust-artifact/artifact.json <clean-fixed-Go-checkout> .tmp/paired-go-artifact/dev-npm-stage <official-LuaLS.zip> <machine-conditions.json>
```

Existing `buildRust` release or symbols records are accepted; dev artifacts are
rejected. Current source digest, binary hash, 3.5.0 beta configuration and
unmodified public Emmy identity must match. Never compare the dev correctness
build to optimized Go. Clear compiler override flags before the optimized build.
The workflow writes machine conditions automatically; direct invocation accepts
an explicit JSON object with `machine`, `powerAndThermal`, `competingLoad`,
`disk`, `operator` strings. Unknown conditions should be stated honestly.

## Checks and remaining work

```text
node --test tests/rust-performance-paired/runner.test.mjs tools/rust-owned-process.test.mjs
```

Node tests cover comparison, missing/failed intermediate samples, independent
homes and actual warm lineage, 30% boundaries, high-resolution timing, retained
workers, strict package paths/Go identity and seed contamination/checkpointing.
The real Windows descendant-clock test is skipped on other platforms and must
pass before a Windows benchmark starts. Synthetic tests are not product passes.

Parser/selection/project/persistence, local sync stages, remaining Source
queries/refs/context/flow/batch, retained sessions/close/invalidation/error
recovery, Data and B04–B07 remain separate required acceptance work.

## One-shot describe startup attribution

`--startup-diagnostics` is a separate observation mode of this existing entry.
It performs exactly one public `describe --home <private-home> --format json`
invocation per engine, with the same sealed optimized Rust artifact and fixed
Go package/source checks, full result comparison, owned Jobs, output capture,
memory observation and final handle/identity verification. It does not run the
20-pair acceptance or acquire/sync/index the Retail corpus. Preparation still
verifies toolchains, power policy, packages, source digest and the original
Emmy identity. A diagnostic observation is not a speedup or performance pass.

Keep the product checkout/source bytes matching the original sealed Rust
artifact. A newly reviewed runner stage may be located separately; invoke its
entry from that unchanged product checkout as the current directory:

```powershell
node <diagnostic-runner-stage>/tools/rust-performance-paired.mjs <optimized-artifact.json> <clean-fixed-Go-checkout> <Go-dev-npm-stage> <pinned-LuaLS.zip> <machine-conditions.json> --startup-diagnostics
```

Use the same five verified input paths from the existing paired run. Deploy the
runner from one new, reviewed commit containing this mode and the then-current
metadata/oracle checks. Do not combine historical 7db dependency copies with
later production edits. The review patch changes only the files listed in its
manifest; it is not permission to overwrite unrelated dependencies.

Create an empty runner directory and archive inside the local coordinator's
owned scratch. From a checkout containing the new reviewed commit, export these
exact runtime dependencies together. Use an archive file rather than a binary
pipeline through Windows PowerShell 5.1:

```powershell
$runnerCommit = "<new-reviewed-commit-containing-startup-diagnostics>"
$runner = "<empty-owned-diagnostic-runner-directory>"
$archive = "<owned-scratch>/startup-runner.tar"
$files = @(
  "tools/rust-performance-paired.mjs",
  "tests/rust-performance-paired/run.mjs",
  "tests/rust-performance-paired/contract.mjs",
  "tests/rust-performance-paired/inputs.mjs",
  "tests/rust-source-full-corpus/assertions.mjs",
  "tests/rust-source-full-corpus/retail-oracle.json",
  "tests/rust-parity/describe-contract.mjs",
  "tests/rust-parity/oracle.mjs",
  "tools/rust-build.mjs",
  "tools/rust-disk.mjs",
  "tools/rust-emmy-identity.mjs",
  "tools/rust-owned-process.mjs",
  "tools/rust-owned-process.ps1",
  "tools/rust-owned-memory.mjs",
  "tools/rust-build-auxiliary.json",
  "tools/zip.mjs",
  "release/rust-disk-policy.json"
)
git archive --format=tar "--output=$archive" $runnerCommit -- $files
if ($LASTEXITCODE -ne 0) { throw "runner archive failed" }
tar -xf $archive -C $runner
if ($LASTEXITCODE -ne 0) { throw "runner extraction failed" }
```

Retain the selected commit, archive SHA-256 and exported file hashes. Then use
the unchanged product checkout matching the ac8 artifact as cwd for the
invocation above. Do not overwrite that product checkout's tools: they
participate in its `sourceDigest`. The diagnostic configuration and outer report
record both the product root and executing runner root; all executing runner
inputs are hashed before/after and retained independently from the original
artifact identity. A new runner never becomes the old artifact's source
identity. Normal acceptance retains its original single-root behavior.

Optional `proof.diagnostics` records six additional monotonic timestamps:
CreateProcess return, durable root-identity write begin/end, observer setup end,
and ResumeThread before/after. The original start/root-exit/tree-reap clock and
all fsync, validation, output, ownership and cleanup behavior remain in place.
The durable-write phase includes opening, writing, `Flush(true)` and closing
that sidecar; it is not a claim that the filesystem flush alone took that time.
Observer setup includes construction/startup; its concurrent samples can also
consume time after ResumeThread. No phase is subtracted from `elapsed()`.

After the unchanged timing end and before closing the original process handle,
GetProcessTimes observes root kernel/user CPU. This is root-only CPU, not owner,
observer or descendant CPU. `preResumeMillis` and
`resumeThroughReapAndDrainMillis` partition the original whole interval;
the latter includes scheduling, loader/runtime, product work, cleanup and
output drain, and is not pure CPU or a measured `main()` interval. The nested
`phases` expose non-overlapping intervals. Missing diagnostics remain compatible
with ordinary timing receipts; explicitly requested invalid or missing
observations fail the diagnostic report without changing a business exit.

Successful observation has `state=startup_diagnostics_observed`,
`performanceAcceptanceEvaluated=false`, `passedSelectedPaths=false` and no
performance groups. Failed/mismatched/unreaped observations retain evidence;
there are no replacement samples. [5bbb Windows observation tests and the first diagnostic](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6220553599) have executed: 61 tests passed, and the separately authenticated ac8 product ran once per engine with original totals Go48.3713ms/Rust50.8590ms. The diagnostic preserved all original timing and ownership facts. One pair is not a stable performance estimate and does not change the failed ac8 20-pair verdict or prove a newer product's speed.

## Single cold index diagnostic

Append `--cold-index-diagnostics` instead of `--startup-diagnostics` to the same
five-argument owned entry when a single cold index observation is required.
The flags are mutually exclusive. This mode authenticates both optimized
artifacts and full fixed Retail inputs, performs each engine's real init/sync,
then clones those independent cold seeds into new homes and invokes each
engine's original `source index` once. It does not read or modify an earlier
failed pair's homes. Preparation is retained separately; no product work is
subtracted from the original command clock. Original 180-second index budget
and 210-second outer invocation bound remain unchanged.

All public index fields remain compared. A mismatch produces
`cold_index_diagnostics_failed`, exit 1, and retains the failed pair's full
outputs, timing, memory and scratch. It does not run a warm sample or replace
the failed pair. An equivalent observation is `cold_index_diagnostics_observed`;
both states have `performanceAcceptanceEvaluated=false`,
`passedSelectedPaths=false`, and no acceptance groups. One pair cannot establish
the 20-pair performance criterion, including when used to diagnose an
optimization while a known semantic difference is still open.

Like startup diagnostics, this mode may use the separately exported, authenticated
17-file runner described above against an unchanged source checkout matching a
previously sealed optimized artifact. Retain both identities; do not overwrite
that checkout's tools or relabel the artifact as the new runner's product.
Ordinary invocation without either flag still runs all original cases with
20 cold and 20 warm pairs per case and the unchanged strict acceptance gate.

## Fixed bbfe/ab4 three-pair Source diagnostics

This separate Windows-only entry compares the two existing sealed Rust EXEs in
exactly three adjacent cold pairs: **bbfe/ab4, ab4/bbfe, bbfe/ab4**. There is no
pair-count option, rebuild, warm phase, Go/LuaLS preparation or acceptance gate.
The existing 20-pair entry is unchanged. Node fixture tests do not establish a
Windows Job or product performance result; the six real CLI calls remain
**NOT_RUN** until the separately authorized Windows run produces its receipts.

Use a separately exported reviewed runner, and supply two authenticated, clean
source checkouts at these fixed commits. Each product and its Git identity
checks use that candidate's own `sourceRoot` as cwd. The original artifact
records and binaries stay at their existing paths under `artifactRoot`; both
artifact roots may be the same historical root. In particular, the old artifact
record's historical `sourceRoot` is provenance, not an instruction to rewrite
that record or pretend the current checkout is its original source. A matching
source checkout must independently satisfy HEAD, clean status, complete
`sourceDigest`, upstream Emmy identity and the fixed 31-phase source SHA-256.
Do not point either candidate at a newer or dirty tree.

Configuration schema (replace only absolute path placeholders; hashes and
commits are fixed and independently enforced by the runner):

```json
{
  "schema": "lycheedev.source-candidate-diagnostics-config.v1",
  "ownerRoot": "C:/actual/original-rust-root",
  "baseline": {
    "sourceRoot": "C:/actual/clean-bbfe-source",
    "artifactRoot": "C:/actual/original-rust-root",
    "artifact": "C:/actual/original-rust-root/.tmp/pr14-bbfe882-release-artifact/artifact.json",
    "commit": "bbfe88265ee431b9ad925ab86dc52561742dd5ee",
    "artifactSHA256": "43429c85cac9e0a3bb130c5de4d939e19a8219dca81001d426ae9a1c9ebd0d4e",
    "binarySHA256": "f25d5fbe4e43eb0ae2f28e310f28e4de51ab6be701d2919f94729132c76a39e6",
    "sourceTreeSHA256": "76bea4b27798356c9f5194f56c97c3a1292b214166b84c875dfc0bb75dfc7b34"
  },
  "candidate": {
    "sourceRoot": "C:/actual/clean-ab4-source",
    "artifactRoot": "C:/actual/original-rust-root",
    "artifact": "C:/actual/original-rust-root/.tmp/pr14-ab4fc03-release-artifact/artifact.json",
    "commit": "ab4fc03605a08b544476742d07a3af08e4cc7ce9",
    "artifactSHA256": "280c42204696a00d2a8e134347c726282c818e6353083c22a7d6737cf17a8450",
    "binarySHA256": "3b124671486c476e55fabf8358d737ebe541005111585837f6ce03a609f95e47",
    "sourceTreeSHA256": "9b45e843f8f4759e729a5574a6af4d5bca6a37bbfefaea9b7f6c03c6e65d71cc"
  }
}
```

The two original artifact records are each 1511 bytes. Preserve their original
bytes, source/binary paths and any matching PDB; do not fabricate replacement
records or copy a binary into an invented artifact owner. Missing or mismatched
inputs stop the run. The existing machine-conditions JSON requires nonempty
`machine`, `powerAndThermal`, `competingLoad`, `disk` and `operator` disclosures,
including explicit unknowns. Invoke the exported entry with just two arguments:

```powershell
node "$runner/tools/rust-source-candidate-diagnostics.mjs" "$candidateConfig" "$machineConditions"
```

Export these **18 files**, retaining the selected reviewed runner commit,
archive SHA-256 and per-file hashes. Use the same `git archive` / extraction
procedure above with this list, without overwriting either product checkout:

```text
tools/rust-source-candidate-diagnostics.mjs
tests/rust-performance-paired/source-candidates.mjs
tests/rust-performance-paired/read-fact-delta.mjs
tests/rust-performance-paired/inputs.mjs
tests/rust-performance-paired/contract.mjs
tests/rust-source-full-corpus/assertions.mjs
tests/rust-source-full-corpus/retail-oracle.json
tests/rust-parity/describe-contract.mjs
tests/rust-parity/oracle.mjs
tools/rust-build.mjs
tools/rust-disk.mjs
tools/rust-emmy-identity.mjs
tools/rust-owned-process.mjs
tools/rust-owned-process.ps1
tools/rust-owned-memory.mjs
tools/rust-build-auxiliary.json
tools/zip.mjs
release/rust-disk-policy.json
```

Both source checkouts must already have their authenticated public-upstream
Emmy materialization. The runner does not prepare or rebuild it. Configuration,
runner bytes and both artifacts are independently recorded and rechecked.
Each candidate performs its own real init/sync at the fixed Retail commit
`31c7f7b9cc79e56c986b365c06a6afbcf3c9177b`; original Git blobs, all 4049 files and
4036 documents, manifests and seed metadata are authenticated before measurement.
Six distinct homes are cloned from the candidates' independently authenticated
sync-only seeds. This is application-cache cold, with no OS-cache-flush claim.
Both homes exist before the adjacent calls. Only saving their evidence occurs
between them; corpus, Facts and source identity scans occur outside that pair.

Each original `source index` keeps the 180000 ms product budget, 210000 ms owned
invocation limit and 30000 ms orphan bound. No stack override is allowed.
The outer owner has a fixed **3600000 ms / 60-minute total bound**: two init
limits, two sync limits and six index limits account for 2160 seconds; two
power-policy observations can consume another 420 seconds, leaving 1020 seconds
for Git authentication, identities, complete Facts comparison and cleanup.
This is a total cap, not a guarantee that every individual worst-case timeout
fits. Exceeding it fails and retains evidence; it never extends the bound or
replaces a sample. Native CreateProcess-through-reap/output-drain timing stays
whole. Memory observations remain sampled lower bounds.

After both calls, the same run must prove all of the following before pair
cleanup:

- Successful original owner/native timing proof, no forced termination,
  deadline, output-limit, proof error or uncertain handle retirement.
- Complete public Rust result equality, removing only `io.elapsedMs`; unchanged
  180000 ms deadline, 4036 parsed documents, zero cached Facts, and `cachedIndex`
  either absent or false. No new/shared result fields are projected away.
- Exactly the fixed 31 Source phases and fixed timing descriptions, strict UTF-8
  compact JSON, no duplicate keys/BOM/extra stderr/lossy numbers. Read, parse,
  serialize and publish calls each equal 4036; worker wait equals consumed;
  sequential + consumed - abandoned equals 4036. Worker counts are not pinned
  to one CPU count. Phase durations overlap and must not be summed/subtracted.
- Every manifest document's own current v10 cache envelope, input hash and Facts
  hash. All fields in declarations, relationships, loads, headers and diagnostics
  match, including absent/extra fields, row order and duplicate multiplicity.
  Files are bounded to 32 MiB and must be regular non-symlink files. Comparison
  reads one document pair at a time, retaining hashes/totals rather than all
  4036 Facts bodies. Declaration/relation/diagnostic totals bind back to the
  public summaries. Historical bodies are never substituted for this run.
- Late artifact, source and runner identity checks before cleanup.

Failures retain raw outputs, profiles, owner proof and pair homes, stop further
pairs and never replace the failed sample. Uncertain retirement blocks the
paired launch as well. Successful pairs seal their full comparison manifest
before deleting owned scratch. The outer report is successful only after all
three pairs and final integrity/power checks pass. Its state is
`source_candidates_observed` or `source_candidates_failed`, always with
`performanceAcceptanceEvaluated=false` and `fullB01B07Acceptance=false`.
Three diagnostic ratios do not establish the formal 20-pair verdict.

Cloud-safe syntax/fixture verification:

```sh
node --check tools/rust-source-candidate-diagnostics.mjs
node --check tests/rust-performance-paired/source-candidates.mjs
node --test tests/rust-performance-paired/source-candidates.test.mjs tests/rust-performance-paired/read-fact-delta.test.mjs
```

## Current Source Facts packs: explicit `v11-pack1` reader

The grouped-cache candidate retains the complete public `source-v1` Facts
contract and the v11 per-document envelope. Its whole-index fingerprint is
`rust-index-v13` plus `facts-pack-v1`; the existing v6 index container gains an
optional `documents[].factsLocator`. Historical layouts and results keep their
original identities. In particular, the bbfe/ab4 candidate runner above remains
fixed to its original v10 per-file artifacts and per-document phase counts;
it cannot be pointed at this candidate by changing only a revision string.

A cold index streams up to 64 ordered eligible Git documents into each private
cache stage. The group identity includes the ordered paths, Git blob identities,
byte sizes, parser/dependency versions and Facts layout, but not the snapshot
commit. Every record contains the unchanged complete envelope, including all
five arrays, their original ordering, duplicates and every field. The cache file
starts with `LYCFPK01`; each record has a 68-byte length/key/SHA-256 header and its
UTF-8 JSON envelope. A locator names the group hash, header offset, body length
and body SHA-256. The `.json` cache suffix is historical; a group body is framed
binary data and must not be parsed as one JSON document.

Both group and single-envelope limits remain 32 MiB. If the next complete record
will not fit, the complete prefix is atomically committed and that record plus
the rest of the cohort use the existing per-file cache. An envelope exceeding
32 MiB retains `codebase.fact_budget`. No aggregate pack body is materialized.
The only persistent new per-document storage is the fixed-field locator. Its
reserved document capacity is admitted at 256 bytes per slot, plus 16 KiB for
one cohort directory, indices and owner state, within the original 256 MiB
retained/speculative admission. The 4 MiB Facts/32 MiB parser reservations,
2 MiB worker stack and eight-slot queue are unchanged. These are requested
storage/admission bounds, not physical RSS claims.

The normal full-index, persisted query projection and lazy XML/TOC load paths
retain their locators. Lazy reads use one opened file and validate the magic,
header offset/range, v11 record key, length, record SHA and complete envelope.
They read only the requested body, not a 32 MiB pack for every document. A
missing, truncated or corrupt record falls back to the exact authenticated Git
input and the existing complete parser/per-file cache. A full rebuild can repair
one corrupt record and backfill earlier valid records into a replacement group
without duplicating a group in memory. Same-key publishers remain lockless;
only complete atomic replacements become visible, while an existing reader
retains its opened snapshot. Existing directory/ancestry checks remain enabled.

An internal cache checkpoint now occurs at a completed cohort or committed
size-limited prefix. A failure before that checkpoint can discard up to 63
already-completed, rebuildable records from the current private group. Earlier
committed groups remain reusable. A whole index is never committed while its
locators reference an unpublished stage; Unknown publication is terminal and
retains its original owner/error state. Public document order and error order
are unchanged.

Cross-snapshot costs must be reported alongside cold results:

- With the same ordered membership and one changed document, only that cohort's
  key changes. At most 63 additional unchanged documents need rebuilding when
  no per-file fallback exists; unchanged groups and the old snapshot still hit.
- Inserting, deleting or renaming an eligible document can shift every later
  64-document boundary. The worst case rebuilds every subsequent document.
  There is no global 63-document bound and no hidden global locator catalog.
- Same-snapshot complete-index hits retain the usual index/query reuse. Per-file
  v11 caches from a prior candidate remain usable, even if no pack is created.

### Full same-run Facts comparison

Obtain the Rust index blob from that run's `source/rust-index/<identity>`
manifest. Its expected SHA-256 is the authenticated manifest's `blob.sha256`;
the corresponding exact bytes are under `cache/objects/<first-two>/<rest>`.
Retain that manifest and blob with the original run evidence. Do not select an
arbitrary recent object or discover a group by scanning directory contents.
Use the existing fixed-Go records and manifest without rewriting them:

```powershell
node tests/rust-performance-paired/read-fact-delta.mjs $goHome $rustHome $goRecords v11-pack1 $rustIndexBlob $expectedRustIndexSHA256
```

The pack reader requires both extra arguments. It verifies the index hash,
parser/snapshot identity and the full path/input-hash set against the Go records,
then reads every document through its authenticated locator or v11 per-file
fallback. A pack record's hash is reported as `rustFactRecordSHA256`; the reader
does not relabel that as a hash of the entire pack. Wrong index hashes, missing
or duplicate inputs, out-of-range locators, header key/length/hash mismatches,
invalid UTF-8 and invalid full envelopes fail the comparison. All five arrays
remain strict, including order and multiplicity. The default remains historical
`v6`; explicit `v10` and `v11` retain their per-file meanings.

The same 31 profile phase names remain available, but cache calls reflect real
operations. Stage/seal/rename/close count actual groups or overflow per-file
publications. `facts_publish` counts the individual coordinator publication
segments; it no longer has a one-call-per-document invariant. `facts_read`
includes group opening/directory/closing and per-record lookup. Do not reuse
historical runner assertions that force these calls to equal 4036. No reduction
in elapsed time is established by operation counts alone.

### Executable storage/cross-snapshot regressions

These commands are for the normal Windows test owner and fixed reviewed source
checkout. Rust execution is **not_run** for this change until the owner records
actual results; cloud rustfmt parsing does not establish compilation.

```powershell
node tools/rust-cargo.mjs --owner source-pack-core -- test -p lycheedev-core --lib --locked --offline cache::source_facts::streaming_tests -- --nocapture
node tools/rust-cargo.mjs --owner source-pack-contract -- test -p lycheedev-source --lib --locked --offline index::facts_cache::tests -- --nocapture
node tools/rust-cargo.mjs --owner source-pack-admission -- test -p lycheedev-source --lib --locked --offline index::facts_pipeline::retained_cohort_metadata_reduces_real_worker_admission_without_changing_facts -- --exact --nocapture
node tools/rust-cargo.mjs --owner source-pack-cache -- test -p lycheedev-source --lib --locked --offline index::facts_cache_tests -- --nocapture
```

The cold/warm test
`complete_cohorts_reduce_publications_and_preserve_all_five_fact_arrays` uses 65
mixed Lua/generated Lua/XML/TOC documents and asserts two real stage/rename/close
operations, all arrays and 65 warm hits. The single-change test
`unchanged_cohort_reuses_across_snapshots_and_changed_cohort_rebuild_is_bounded`
asserts 64 parsed/one hit, then 65 hits for the original snapshot. The insertion
and deletion test
`insertion_and_deletion_can_rebuild_all_shifted_cohorts_but_old_snapshots_still_hit`
uses an original 128-document snapshot, asserts 129 parsed/zero hits after a
front insertion and 127 parsed/zero hits after deleting its first original
member, then 128 original-snapshot hits. These are real Git/cache operations,
with full Facts comparisons, not synthesized timing results. Formal performance
observations must include cold, unchanged, single-change and insertion/deletion
cases rather than presenting only a cold improvement.

The remaining pack tests cover one-record repair/backfill; persisted query and
lazy-load fallback; framing/hash/range corruption; failed partial-group cleanup;
original cancellation/deadline; the 32 MiB prefix/per-file boundary; oversized
single envelopes; and fixed metadata admission. Core stream tests exercise the
new owner API directly: pre-commit invisibility, bounded ranges, same-key threads
and two processes with held old readers, exact size boundary, partial-write
cancellation, primary/close/control errors, late Published stop and terminal
Unknown ownership. Their subprocess helper is inert without its test-only
environment, with bounded waits and owned kill/reap cleanup.

Cloud-only static checks for this batch:

```sh
node --check tests/rust-performance-paired/read-fact-delta.mjs
node --test tests/rust-performance-paired/read-fact-delta.test.mjs tests/rust-performance-paired/source-candidates.test.mjs tests/rust-parity/source-facts-generated/fixture.test.mjs
```

The current combined fixture run passes 64/64 (reader 27, historical-candidate
runner 32, generated fixture structure 5). This verifies Node fixtures and
historical-reader compatibility, not Rust execution or performance acceptance.
