# First paired performance acceptance entry

This entry has real Windows execution evidence. The [first ac8 optimized run](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6220085568) completed 20 cold and 20 warm describe pairs with equivalent output but Rust/Go median ratios 1.127/1.126, failing the ≤0.30 target. Static Retail produced no timed samples after seed validation found the subsequently repaired metadata-generation contract. A new fixed-5bbb optimized build and complete paired run are queued; no result is implied before execution. See [handover](../../handover.md) for the current exact candidate and results. Existing dev-profile CI, the historical five-sample/50% script, and prior failures retain their original meanings.

## Selected scope and pass conditions

- B01: real public `describe` invocation, first/repeated invocation only.
- First B02 entries: full fixed Retail `source index` and precise
  `source query GetActionButtonForID --topic lua --limit 10`, each on its own
  independent application-cache-cold home, followed by the identical warm
  command against that engine's own home and pin.
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
