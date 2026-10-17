# Narrow Data Go 3.2.3 / Rust paired entry

The [30d Windows observation](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6228602282)
verified four paths (16 actual CLI invocations), then failed while removing the
fourth pair's already-sealed scratch. The separate
[d86 runner / 30d product continuation](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6228983589)
then verified the remaining eight paths (32 CLI invocations), with all new
owners retired/reaped. It imported no prior samples and left the original
failure unchanged. The later
[5bc runner / same 30d product full acceptance](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6229610505)
completed all 928 measured CLI invocations and 232 pairs with complete
equivalence and retirement. All eleven performance paths collected 20 cold
and 20 warm pairs; all 22 performance groups failed the ≤0.30 target. Thirteen
memory observations were `not_measured`, and all others retain sampling gaps.
The outer performance failure and earlier EPERM remain intact. The optional
startup diagnostic mode below is new runner-only code until separately run;
it does not revise those results or establish B04 acceptance. Use the
existing [paired ownership/identity/timing contract](README.md); do not use the
historical Go 3.1.2 WDC driver. The Data sibling adds no build or fallback to its
measured entry and does not change the Source runner.

## Exactly what runs

Three existing synthetic fixture families supply twelve public command paths:

1. Small authenticated catalog: `data sql --sql "SELECT ID, Name_lang,
   ModelFileDataID FROM HouseDecor ORDER BY ID"`. All three rows, column/schema
   details, ordering, captures and complete envelopes are checked. This case is
   **correctness only**, including in acceptance mode. It is not SQL throughput.
2. `asset inspect --file-id 501 --max-bytes 17825792`: all 17 MiB of decoded
   byte 120, stored as three authenticated zlib BLTE chunks. Root and payload
   encoded hashes, decoded blobs and source capture are verified in full.
3. `asset export --file-id <701..705> --encoding <png|webp> --mipmap 0
   --channels rgba --output <fresh-owned-file>`: all five existing BLP2 modes
   (palette, BGRA8, BC1, BC2, BC3), each 1031×519 pixels, default codec settings.
   The existing serial decoder exports a complete RGBA oracle per source.
   Both actual encoded outputs must independently decode to every oracle byte.
   Encoded PNG/WebP bytes need not match across engines. Default lossless WebP
   and all original product checks remain enabled. All source, artifact and
   manifest captures, ordered dependency edges and archived bytes are checked.

Each command also receives its authenticated fixture `--snapshot`,
`--installation`, its own `--home`, `--format json --offline`. The descriptor
fixes retail/cn/zhCN, build 12.1.0.69933 and synthetic definition commit `bbbb…`
(40 b's). These are constructed test bytes, not real-client or real-upstream
definition evidence. No game, real installation or remote input is required.

Without flags, each path runs one cold Go/Rust pair and its warm Go/Rust pair.
The result can only be `equivalent_data_observation_only`; it has no accepted
performance ratio. `--acceptance` first verifies one such preflight per path,
then runs exactly 20 fresh cold pairs and 20 matching warm pairs for each of
the eleven performance paths whose preflight passed. Engine order alternates.
Preflight samples are retained separately and never enter those 20 samples.
SQL still runs only its correctness pair. Every sample, not just the last, is
checked. Each path/cache group separately requires median Rust/Go ≤ 0.30.
Missing, failed, malformed, non-equivalent or unreaped samples cannot pass.
New rows also carry a distinct `pairTerminal` result written only after the
pair's final fixture-inventory check. Both cold and warm summaries require its
verified terminal state. If that final check fails, original successful CLI
results and `resultEquivalent=true` remain intact, while the pair and summary
report `failed_final_integrity`; even SQL's only pair or the last of 20 cannot
pass. Missing terminal evidence also fails new runs. Historical continuation
alone permits older rows without this field, after separately authenticating
their original sealed `pair.json` with `valid=true`; it never rewrites them.
Failure stops that path, retains its inputs/evidence and never creates a
replacement or a warm label after failed cold. Independent paths may continue
only once process retirement is proved.

## Required prepared inputs

Run with the frozen product checkout as the current directory on Windows,
under the existing single local test queue. All modes authenticate the runner
from the executing entry's directory, which can be that same checkout or a
separate committed export described below. Product source/artifact checks
still use the current directory. Use committed tools, not a historical dirty
working copy. Do not run additional Cargo jobs, borrow a target directory, remove a
lease or change source bytes while preparing/running this candidate.

- Installed Node 24 (including `node:sqlite`), official Rust 1.99.0 toolchain
  and the already admitted Windows build environment. Dependencies must already
  be available: fixture generation and Rust builds use locked/offline Cargo.
- A sealed optimized Rust `artifact.json` produced by the existing controlled
  release/symbols builder. Its entire current source digest and binary hash must
  match; dev or an old product artifact after changing test/source bytes fails.
  The configured 3.5.0 beta and unmodified public Emmy identity are verified.
- A clean real Go checkout at
  `3766f9cacb52e93a803a626187e2a1d54a445468`, official Go 1.27.1, and its already
  prepared complete optimized `dev-npm-stage`. Bare executables are rejected.
  VCS stamps must prove this commit and `vcs.modified=false`; every packaged
  resource is authenticated. If the package is missing, use the existing
  `tools/rust-performance-prepare-go.mjs` command documented in the main README,
  separately from measurement. Do not rebuild or edit the baseline ad hoc.
- The official pinned LuaLS 3.19.1 Windows archive, 4,453,980 bytes, SHA-256
  `fdb9a59108cf62517813c97fa5549b0e16d1ef0688306bac728b08434db7e4cd`.
- An **already installed compatible `sharp` module**, supplied by absolute path
  to its package directory or entry file, for independent PNG/WebP RGBA decoding.
  This runner never installs or downloads it. It records `sharp.versions`, the
  resolved module and the hashes of its loaded module/native-addon graph before
  and after execution. OS shared libraries are not claimed fully inventoried.
  A missing decoder is a setup failure, never a pixel-check bypass.
- An existing machine-conditions JSON object with nonempty string fields
  `machine`, `powerAndThermal`, `competingLoad`, `disk`, `operator`. State unknown
  conditions honestly. Active power policy is recorded and checked unchanged.

Clear `RUSTFLAGS`, `CARGO_ENCODED_RUSTFLAGS` and `CARGO_TARGET_DIR` before building;
do not override the existing owned runner, quotas or command budgets.

## Exact preparation and commands

These are test tasks for the single local coordinator. The following PowerShell
commands need only the five existing baseline/artifact/environment paths plus
the supplied installed decoder path; no temporary comparator script is needed.

First check the harness and real Windows owner. A Linux/cloud Node pass does
not establish the native owned clock or memory observer:

```powershell
node --test tests/rust-performance-paired/data-runner.test.mjs tests/rust-performance-paired/data-resume.test.mjs tools/rust-owned-process.test.mjs
if ($LASTEXITCODE -ne 0) { throw "Data harness or native owner check failed" }

$prepared = node tests/rust-performance-paired/prepare-data.mjs | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or $prepared.state -ne "prepared_synthetic_inputs") { throw "Data fixture preparation failed; retain its report" }
```

The preparer uses one evidence-retained `beginScratch` owner, invokes the existing
`runCargo` three times **serially**, and returns `fixtures.catalog`,
`fixtures.content`, `fixtures.images`, its sealed report and owner receipt.
It runs exactly these existing ignored tests, with `--locked --offline -j 1`,
`--ignored --exact --nocapture --test-threads=1`:

- `lycheedev-data --test catalog_navigation_parity prepare_catalog_public_fixture`
- `lycheedev-data --test content_budget_parity prepare_compressed_content_public_fixture`
- `lycheedev-data --test image_export_business prepare_large_image_export_public_fixture`

The first two test exporters are unchanged; the third exposes the existing
large-image test inputs and all RGBA oracle bytes. Environment output paths are
set by the preparer inside its fresh owner; content size is exactly 17 MiB.
Each invocation retains the actual command, source/input/environment hashes,
exit/error, native retirement proof and original Cargo receipt. Original Cargo
owners alone manage their channel leases. The fixture parent never declares a
shared cache retired, releases its lease, or cleans partial output on failure.
Successful fixtures also remain retained for both engines and later audit.

After all candidate source bytes are frozen, build once if a matching optimized
artifact is not already present. This remains a separate controlled preparation:

```powershell
node tools/rust-build.mjs .tmp/data-paired-rust-artifact release --one-shot
if ($LASTEXITCODE -ne 0) { throw "Controlled optimized build failed" }

$rustArtifact = ".tmp/data-paired-rust-artifact/artifact.json"
$goSource = "<existing clean Go323 checkout>"
$goPackage = "<existing optimized Go dev-npm-stage>"
$luaLS = "<existing pinned LuaLS archive>"
$conditions = "<existing machine-conditions.json>"
$sharp = "<absolute path to existing node_modules/sharp>"

node tools/rust-data-performance-paired.mjs $rustArtifact $goSource $goPackage $luaLS $conditions $prepared.fixtures.catalog $prepared.fixtures.content $prepared.fixtures.images $sharp
if ($LASTEXITCODE -ne 0) { throw "Data equivalence observation failed; preserve evidence" }
```

For the full mode, use `--acceptance`. Its built-in single-pair correctness
preflight must pass automatically before that path's 20 pairs start. The
separate one-shot command above is optional diagnostics, not another approval
or manual review gate:

```powershell
node tools/rust-data-performance-paired.mjs $rustArtifact $goSource $goPackage $luaLS $conditions $prepared.fixtures.catalog $prepared.fixtures.content $prepared.fixtures.images $sharp --acceptance
if ($LASTEXITCODE -ne 0) { throw "Selected Data acceptance failed; preserve evidence" }
```

The entry prints the owned report path. Its detailed `observation` report links
all preparation, preflight, sampled commands, pair owners and sealed raw output.
The output fixture paths can also be read from the persisted preparation report
in a later shell session. Do not move the fixture trees: descriptors contain
their authenticated absolute installation/home paths.

## Cache, equivalence, timing and resource boundaries

Each engine actually executes `init` on a fresh seed home per fixture. The
controller then imports only the identical authenticated synthetic pin and DBD
documents/blobs into that engine's initialized store. It imports neither a
foreign SQLite image nor a derived cache. This is an explicit fixture boundary:
it does **not** exercise real installation discovery, selection, definition
fetch, `sync` or full preparation performance. Every timed public command still
performs its normal validation. Fixture import is never described as real sync.

Each path/iteration clones only that engine's seed into a separate verified
home. Cold has no decoded-cache entries. Warm repeats the identical command on
its own successful cold home/pin; two actual decoded metadata entries and blobs
must survive unchanged. Both modes hash the entire authenticated encoded Root
and payload. Cold must decode/extract their complete decoded byte total; warm
must report two cache hits, no decode/extraction and their entire reused byte
total. All ten original counters are retained. Full-work counters are checked
against fixture bytes; implementation-specific read/cache counters remain
explicit observations. This is application-cache cold/warm. OS page/file caches
are not flushed, and no OS-cold result is claimed.

The complete public envelope is compared after only named audited differences:
verified CAP IDs/times and codec/manifest hashes/lengths, owned output paths,
validated Rust verification/encoding-I/O/accounting additions, exact cache-work
proofs, and separately recorded timings/resource observations. Unknown fields
are not projected away. CAP hashes include Go323's ordered dependency edges;
manifest contents must reproduce the whole actual result before normalization.
Every image pixel and source blob is independently checked. SQL results retain
all original schema, rows/order, query, warnings, completeness and diagnostics.

Original resource counters are retained in full, not compared as generic equal
numbers: Go and Rust admission accounting differ. Network use must be zero;
the unchanged 100,000,000 decode-work limit is enforced. Tiny SQL requires the
same initial 512 MiB retained / 128 MiB metadata policy, no growth, adequate
available-memory ceiling and admitted usage/peak within budget. Concurrent
query policy remains a compared field. Rust's explicit incomplete-accounting
flags/gaps must match the reviewed contract; they never become a completeness
claim. No runtime budget is increased to obtain a sample.

The existing native monotonic interval starts immediately before CreateProcess
and ends after root/tree reaping and output drain. All product content hashes,
validation, decoding/encoding, output writes, captures and cleanup stay inside.
Fixture construction, cloning and independent audit are recorded separately;
none is subtracted from a CLI's measured time. Native owner/observer overhead
inside the original interval is included. Missing native timing fails; there is
no plain wall-clock substitute. SQL retains its existing 300-second query
deadline. Asset commands receive no new timeout flag: Rust retains its existing
180-second general command deadline; fixed Go323 retains its caller cancellation
context. Both engines have the same 330-second outer owner cap. These original
product deadline differences are disclosed, not relabeled as equal limits or
extended to obtain a sample. The complete controller has
a five-hour ownership cap; missing samples still fail if it is reached.

Memory uses the existing identity-deduplicated sampled owned-Job working set and
private bytes with explicit gaps. Overlapping inner/outer Jobs are never summed;
this is not unique physical RSS. CPU and I/O remain `not_measured`. Successful
reaped pair scratch is cleaned only after capture/encoded-output evidence is
sealed. Failed/unreaped pair and outer scratch are retained. No process scanning,
broad kill, cache fallback or automatic recovery is added.

Even `passed_selected_data_paths_full_acceptance_open` is only these eleven
synthetic performance paths. Representative SQL, raw export, real-client data,
Hotfix/effective/remote paths, full resource accounting, full B04/B01–B07,
sessions and release qualification remain open.

## Bounded continuation after the 30d cleanup failure

The original outer owner `811dac59-a06f-4f11-b0b4-43b0c37bb7d6` remains failed.
Its four completed paths and original timings are retained, never rerun by this
mode or copied into a new sample set. Pair
`ed2701b9-639e-4989-9e49-8626ae7a9db1` was sealed succeeded/reaped but not retired.
`handlesReaped` proves its CLI descendants ended; at cleanup the controller and
its in-process decoder were still alive. The archived evidence contains no
actual open-file-holder, Windows attributes/ACL or decoder-cache observations.
It therefore does not prove which component caused EPERM.

The old controller passed scratch filenames into Sharp. Its documented
[operation cache can keep files open](https://sharp.pixelplumbing.com/api-utility/#cache).
The revised controller gives it only the already size-bounded, SHA-256-verified
artifact Buffer, disables operation/file caching and destroys each completed
pipeline in `finally`. Decode and destroy errors are both preserved. It checks
the encoded Buffer hash again after decoding and writes that exact Buffer to
evidence without a new unbounded copy. All RGBA checks remain. This removes a
file-lifetime risk; it is not a claim to have proved the historical holder or
fixed every possible Windows EPERM. Generic scratch removal is unchanged:
another failure stops, retains evidence and records code/syscall/path/message.
There is no retry, chmod, broad cleanup or acceptance override.

`--resume-unattempted <original-owned-report.json>` is deliberately narrow:

- Only the original full, single-root, non-acceptance observation is accepted;
  no chained continuation and no `--acceptance` combination.
- Every attempted path must form a completed prefix with both cold/warm pairs,
  full equivalence, valid original native timing and reaped CLI ownership.
  Partial, mismatched or failed samples cannot be replaced by this feature.
- Earlier pair owners must be retired and the final owner sealed
  succeeded/reaped with the original EPERM naming it. No old owner is cleaned,
  edited, adopted or relabeled by the continuation.
- Both before and after the new run, authenticate the original configuration,
  reports, every outer/pair sealed raw log and hash index, the unchanged old
  runner files, Rust artifact/source, Go package, complete fixture inventories,
  supplied decoder modules, machine/toolchains and recorded conditions.
  Missing or altered raw stdout/native receipt/image evidence is fatal.
- Only whole unattempted paths get new homes, pairs and owners. In this 30d
  case those are BGRA8, BC1, BC2, BC3 × PNG/WebP. No prior sample is imported.
  Empty groups are explicitly `not_run`, not product correctness failures.
- Successful completion is `equivalent_data_continuation_only`, with original
  failed state and both runner identities retained. It is neither a repaired
  original run, a combined performance result nor a 20-pair acceptance. Later
  full acceptance still uses its own complete automatic preflight and 20 pairs.

Run the reviewed new runner from a separate exported stage while the **current
directory remains the unchanged product checkout matching the 30d artifact**.
Do not overwrite that checkout's tools or rebuild the product merely to run the
new comparator. In particular, generic ownership tools participate in its
source digest. Stage/archive/conditions paths below must be fresh paths inside
the coordinator's already registered test scratch. Preparing that stage is test
coordination in the same single local queue, not an additional build channel.

From a checkout containing the reviewed runner commit, export this exact
runtime and test dependency closure. Use a tar file, not a binary PowerShell 5.1
pipeline; do not copy unrelated dirty files:

```powershell
$reviewedCheckout = "<checkout containing the reviewed new runner commit>"
$runnerCommit = "<reviewed runner commit, including the all-mode stage fix for acceptance>"
$runner = "<fresh empty runner directory in coordinator-owned scratch>"
$archive = "<fresh archive.tar in that same owner>"
$files = @(
  "tools/rust-data-performance-paired.mjs",
  "tests/rust-performance-paired/data-run.mjs",
  "tests/rust-performance-paired/data-contract.mjs",
  "tests/rust-performance-paired/data-inputs.mjs",
  "tests/rust-performance-paired/data-resume.mjs",
  "tests/rust-performance-paired/data-phase-profile.mjs",
  "tests/rust-performance-paired/data-runner.test.mjs",
  "tests/rust-performance-paired/data-resume.test.mjs",
  "tests/rust-performance-paired/data.README.md",
  "tests/rust-performance-paired/prepare-data.mjs",
  "tests/rust-performance-paired/contract.mjs",
  "tests/rust-performance-paired/inputs.mjs",
  "tests/rust-source-full-corpus/assertions.mjs",
  "tests/rust-parity/describe-contract.mjs",
  "tests/rust-parity/oracle.mjs",
  "tools/rust-build.mjs",
  "tools/rust-disk.mjs",
  "tools/rust-emmy-identity.mjs",
  "tools/rust-owned-process.mjs",
  "tools/rust-owned-process.ps1",
  "tools/rust-owned-process.test.mjs",
  "tools/rust-owned-memory.mjs",
  "tools/rust-build-auxiliary.json",
  "tools/zip.mjs",
  "release/rust-disk-policy.json"
)
git -C $reviewedCheckout archive --format=tar "--output=$archive" $runnerCommit -- $files
if ($LASTEXITCODE -ne 0) { throw "Runner export failed" }
tar -xf $archive -C $runner
if ($LASTEXITCODE -ne 0) { throw "Runner extraction failed" }
Get-FileHash -Algorithm SHA256 $archive
```

Retain the commit/archive hash and exported file hashes in the coordinator's
stage receipt. Then run from the unchanged 30d product root. The original sealed
configuration supplies all nine existing inputs; it is not rewritten. Its
conditions are copied as original recorded conditions, while the new report
separately identifies the current runner and rechecks power/toolchains. Use
UTF-8 without BOM for the new conditions copy:

```powershell
Set-Location "<unchanged 30d product checkout>"
$prior = Join-Path (Get-Location) ".tmp/rust-disk/evidence/811dac59-a06f-4f11-b0b4-43b0c37bb7d6/owned-report.json"
$original = Get-Content -Raw (Join-Path (Split-Path $prior) "configuration.json") | ConvertFrom-Json
$conditions = "<fresh conditions.json in coordinator-owned scratch>"
[IO.File]::WriteAllText($conditions, ($original.conditions | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
node --test "$runner/tests/rust-performance-paired/data-runner.test.mjs" "$runner/tests/rust-performance-paired/data-resume.test.mjs" "$runner/tools/rust-owned-process.test.mjs"
if ($LASTEXITCODE -ne 0) { throw "Staged Data/native owner checks failed" }
node "$runner/tools/rust-data-performance-paired.mjs" $original.rustArtifact $original.goSource $original.go.root $original.luaLSArchive $conditions $original.fixtures.catalog $original.fixtures.content $original.fixtures.images $original.sharpModule --resume-unattempted $prior
if ($LASTEXITCODE -ne 0) { throw "Data continuation failed; retain its new and original evidence" }
```

The new report includes `continuation.selectedCases`, `skippedCompletedCases`,
`importedSamples=0`, original evidence/file digests and `priorEvidenceStable`.
It does not mark the old EPERM owner retired or remove its residual files.
Those remain a separate explicit ownership/recovery obligation.

## Fresh full acceptance with the unchanged 30d product

Use a **fresh stage of the reviewed all-mode entry fix**, exported with the
complete 25-file list above, while keeping the product checkout and sealed
artifact unchanged. The d86 entry alone does not support separate-stage
acceptance: it chooses the product checkout's old runner for that mode. The
fixed entry always hashes and launches its own stage, records `runnerRoot`,
and authenticates those same runner files again at completion. Normal
same-checkout execution is unchanged. This requires no product rebuild or
replacement of the old checkout's tools.

The following is a new run with new owners, seeds and samples. The original
configuration supplies the exact absolute artifact, Go package/source, LuaLS,
fixture and decoder paths. Reading it supplies parameters only: acceptance
does not load either previous report's samples, skip any path, repair the old
failure or combine with `--resume-unattempted`. Keep the old evidence and
unretired cleanup obligation intact. Copy the recorded conditions without a
BOM; before execution, ensure this new copy truthfully describes the current
machine/load, updating the new copy if conditions changed. Runtime power and
toolchain checks remain enabled.

```powershell
# $runner is the fresh authenticated 25-file export from the preceding block.
Set-Location "<unchanged 30d product checkout>"
$prior = Join-Path (Get-Location) ".tmp/rust-disk/evidence/811dac59-a06f-4f11-b0b4-43b0c37bb7d6/owned-report.json"
$original = Get-Content -Raw (Join-Path (Split-Path $prior) "configuration.json") | ConvertFrom-Json
$conditions = "<fresh acceptance-conditions.json in coordinator-owned scratch>"
[IO.File]::WriteAllText($conditions, ($original.conditions | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
foreach ($inputPath in @($runner, $original.rustArtifact, $original.goSource, $original.go.root, $original.luaLSArchive, $conditions, $original.fixtures.catalog, $original.fixtures.content, $original.fixtures.images, $original.sharpModule)) {
  if (-not [IO.Path]::IsPathRooted($inputPath)) { throw "Every runner/artifact/input path must be absolute" }
}
node --test "$runner/tests/rust-performance-paired/data-runner.test.mjs" "$runner/tests/rust-performance-paired/data-resume.test.mjs" "$runner/tools/rust-owned-process.test.mjs"
if ($LASTEXITCODE -ne 0) { throw "Staged Data/native owner checks failed" }
node "$runner/tools/rust-data-performance-paired.mjs" $original.rustArtifact $original.goSource $original.go.root $original.luaLSArchive $conditions $original.fixtures.catalog $original.fixtures.content $original.fixtures.images $original.sharpModule --acceptance
if ($LASTEXITCODE -ne 0) { throw "Data acceptance failed; retain complete report and owner evidence" }
```

Successful scheduling comprises twelve fresh preflights, each one cold pair
and one warm pair. For each of the eleven performance paths, a passing
preflight is followed immediately by exactly 20 new cold Go/Rust pairs and 20
matching warm pairs; preflight samples are separate. Tiny SQL runs only its
one cold and one warm correctness pair. That is 232 pair owners, 464 cold/warm
pair rows and 928 measured CLI invocations in total, including 44 performance
preflight CLI invocations, 880 performance sample invocations and four SQL
invocations. There are also 15 owned preparation invocations on the complete
successful route. Every result still receives full validation and comparison.

The fixed five-hour controller cap and 330-second per-owned-command cap are
unchanged. Five hours is an operational ceiling, **not** a guarantee that all
commands can consume 330 seconds: the 928 measured invocations alone would
need 85 hours 4 minutes at that limit. Completion within five hours requires
an average below about 19.4 seconds per measured invocation, with additional
room for preparation, cloning, independent audits, process-owner overhead and
sealing. The observed eight-path continuation's native intervals were about
0.108–0.261 seconds, so they do not suggest a per-command budget problem, but
they neither predict full controller wall time nor guarantee completion. No
deadline is extended and no late or missing sample is filled in.

Actual failure policy is unchanged:

- A failed preflight skips that path's 20 samples. A failed measured iteration
  stops further iterations of that path. A failed cold comparison never gets
  a warm sample or replacement; already reaped engine results are retained.
  Other independent paths may proceed only with process retirement proved.
- Unproved retirement, initial setup/final identity revalidation errors or any
  pair cleanup failure stop the controller. Pair-local fixture/clone errors
  stop that path under the same proved-retirement rule above. The five-hour
  outer deadline stops its owned tree; partial
  reports remain failed or incomplete and ownership evidence is retained.
- A median ratio above 0.30 is reported after collection, not used to stop or
  cherry-pick samples. Every cold/warm performance group must independently
  pass; SQL has no performance ratio. Missing memory samples remain explicit
  gaps, not zero or a full-memory acceptance claim. Even a successful elapsed
  result remains `passed_selected_data_paths_full_acceptance_open` for these
  synthetic paths, with full B04 and full resource acceptance still open.

## Optional startup attribution on the same 30d product

`--startup-diagnostics` is an independent observation mode, mutually exclusive
with `--acceptance`, `--resume-unattempted` and `--data-phase-profile`. It runs the existing twelve
paths once cold and once warm: twelve fresh pair owners, 24 pair rows and 48
measured CLI invocations, plus the unchanged 15 preparation invocations. It
does not import, replace or add fields to any of the old 928 samples. The
ordinary mode and acceptance counts/thresholds remain unchanged.

Only measured CLI invocations enable the existing native owner's
`startupDiagnostics` option. Every sample keeps its original full
`controllableMillis` and raw proof, and additionally records `startup` using
the shared strict `startupBreakdown` validator. Its timestamps attribute
CreateProcess, pre-resume root-identity durability, observer setup, Resume,
root-exit observation and final tree/drain. Root kernel/user CPU comes from
that original process handle; it is not descendant/owner CPU or complete
resource accounting. Resume-through-reap still includes scheduling, loader,
runtime, product work and cleanup; it is not pure product CPU.

Missing, malformed or failed diagnostic proof records `startupFailure` and
`failed_diagnostics`; it never invents fields or substitutes wall time. Actual
CLI exits, native timing, reaping, captures and product `resultEquivalent`
remain independently checked and retained. A diagnostically failed pair is
sealed/retained with its original process retirement, and the overall run
fails even when its product results are equivalent. Nothing is subtracted
from the elapsed acceptance interval, and no diagnostic subinterval can pass
the ≤0.30 target. A successful diagnostic run is still only
`equivalent_data_observation_only`, with `startupDiagnostics=true` and
`performanceAcceptanceEvaluated=false`.

Use the same 25-file Git export above from the reviewed **new diagnostic
runner commit**, in fresh coordinator-owned stage, not the old 5bc runner.
Keep the current directory at the unchanged 30d product checkout and reuse
its already sealed release. Use the full acceptance's authenticated original
configuration only for absolute input paths; a new conditions copy describes
this diagnostic run truthfully. No Cargo, product release or package rebuild
is needed:

```powershell
Set-Location "<unchanged 30d product checkout>"
$original = Get-Content -Raw ".tmp/rust-disk/evidence/5cd2078b-11f3-46be-9827-02d96767bb6b/configuration.json" | ConvertFrom-Json
$conditions = "<fresh diagnostic-conditions.json in coordinator-owned scratch>"
[IO.File]::WriteAllText($conditions, ($original.conditions | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
# Review this new conditions copy for the current diagnostic machine/load.
foreach ($inputPath in @($runner, $original.rustArtifact, $original.goSource, $original.go.root, $original.luaLSArchive, $conditions, $original.fixtures.catalog, $original.fixtures.content, $original.fixtures.images, $original.sharpModule)) {
  if (-not [IO.Path]::IsPathRooted($inputPath)) { throw "Every runner/artifact/input path must be absolute" }
}
node --test "$runner/tests/rust-performance-paired/data-runner.test.mjs" "$runner/tests/rust-performance-paired/data-resume.test.mjs" "$runner/tools/rust-owned-process.test.mjs"
if ($LASTEXITCODE -ne 0) { throw "Staged Data/native owner checks failed" }
node "$runner/tools/rust-data-performance-paired.mjs" $original.rustArtifact $original.goSource $original.go.root $original.luaLSArchive $conditions $original.fixtures.catalog $original.fixtures.content $original.fixtures.images $original.sharpModule --startup-diagnostics
if ($LASTEXITCODE -ne 0) { throw "Data diagnostics failed; retain product and diagnostic evidence separately" }
```

The old 5bc Data runner did not enable this native option; its 928 samples
cannot retrospectively supply pre-resume attribution. Existing root-exit to
tree/drain ticks can still be analyzed as recorded. This new one-pair mode
isolates the next observation need without a new product build or another
928-invocation acceptance run. It does not isolate BLTE, texture decoding,
PNG/WebP, metadata or file-publication costs inside the product; those require
separately justified product phase observation if startup evidence points
there. The separate opt-in Data phase mode below supplies that narrower product
observation with a new matching artifact. All existing command budgets and the
five-hour outer cap remain.


## Optional Data business-phase profile with a matching new product

`--data-phase-profile` is a fresh diagnostic observation mode. It is mutually
exclusive with `--acceptance`, `--resume-unattempted` and the standalone
`--startup-diagnostics` flag. It automatically enables the same startup
attribution described above for both measured engines. It runs all twelve
existing paths once cold and once warm: twelve fresh pair owners, 24 pair rows
and 48 measured CLI invocations, plus the unchanged 15 preparation invocations.
Preparation is not a sample. There is no performance acceptance, sample import,
replacement, shortcut or extra product run to obtain a missing profile.

This mode requires a **new sealed optimized Rust artifact built from the exact
product source containing the Data recorder and its actual consumers**. The
unchanged 30d product cannot emit `lycheedev.data-query-phases.v1`; running this
mode against it would fail the required diagnostic proof. Startup-only mode
can still use 30d as documented above. A runner-only export cannot add product
instrumentation. Source digest, complete packaged resources and binary identity
must all match the new artifact through the existing preparation/verification
workflow. This measurement entry performs no build. Reuse only authenticated
unchanged Go323, fixture and decoder inputs; preserve all older failures and
samples under their original product/runner identities.

Only a measured Rust invocation receives `LYCHEEDEV_DATA_PHASE_PROFILE=1`.
The runner removes inherited case variants of that variable from every other
invocation, including Go, `init`, toolchain/identity preparation, ordinary
measurement, acceptance and startup-only measurement. Their stderr-empty
contract stays unchanged. Profiled Rust stderr must contain exactly one compact
JSON record, optionally followed by one line ending, with the exact v1 fields,
fixed ordered eleven phases and nonnegative safe integer call/time values.
Missing, duplicate, malformed, oversized or unknown stderr is rejected. For
these known successful fixtures, admission, prepare, payload, source capture
and source close must each have at least one call; image exports require all
eleven phases. Failed invocations may legitimately report zero calls where
work was never reached. This requirement is not applied to unknown paths.

Each Rust sample retains its original raw stderr logs, `stderrSHA256`, byte
count, full process proof, exit/error and original `controllableMillis`.
The parsed record is stored separately as `dataPhaseProfile`; failures are
`dataPhaseProfileFailure`. Both engines retain the independent `startup` or
`startupFailure`. The profile has no invented invocation identity: its link to
the exact sample is the original stderr hash/log, owned process proof and
matched product artifact recorded with that sample/run. Diagnostic parsing
never replaces product output validation
or a product failure. Equivalent successful products with missing diagnostic
proof remain `valid=true` and `resultEquivalent=true`, while their pair and
overall diagnostic run fail (`failed_diagnostics`) and retain evidence. When
product and diagnostic checks both fail, both original findings remain; failed
cold products still do not obtain a warm sample. A success is only
`equivalent_data_observation_only`, with `dataPhaseProfile=true`,
`startupDiagnostics=true` and `performanceAcceptanceEvaluated=false`.

The actual recorder observes these fixed business scopes:

- `admission`: existing workspace/provider admission and pin checks in that
  scope, after earlier request validation where the product already places it.
- `prepare_inclusive`: local target/CASC/Encoding setup, full Root
  authentication/cache handling, Root materialization and checked close,
  build/CDN evidence and RootIndex construction where that consumer uses it.
  The legacy inspect reader prepares keys before this span. This is not metadata-only.
- `payload_inclusive`: selected lookup, full encoded hash, decoded-cache
  handling or decode/extraction, body materialization and reading metadata.
  The legacy inspect reader's batched Root lookup precedes its per-file span.
- `texture_decode`: verified BLP texture decode; `pixel_transform_and_hash`:
  channel transformation and RGBA digest; `encode`: the existing PNG/WebP
  encoder call, including its writes and in-call error handling.
- `source_capture_metadata_inclusive`, `artifact_capture_metadata_inclusive`
  and `manifest_capture_metadata_inclusive`: the respective existing capture
  calls, including metadata construction/publication and blob work. Source
  capture is the SQL query capture or asset source capture, as appropriate.
  They do not independently time metadata open/write/close.
- `output`: bounded external export-file publication: copy/stage/seal/publish
  and its inner checked close. Final destination close occurs after the request
  budget is dropped; it and CLI JSON render/flush are outside this phase.
- `source_close`: explicit outer checked blob, reader and admission-scope
  cleanup, including repeated and potentially nested close scopes. Internal
  closes stay inclusive in prepare/payload/capture rather than being split out.

Every phase reports calls and elapsed microseconds, including observed scopes
that return errors. Unused phases are present with zero calls/time. The one
record is emitted when the final shared query-budget recorder is dropped;
safety-only views and borrowed spans do not create duplicate output owners.
Its top-level `elapsedMicros` is that budget's diagnostic lifetime, not the full
CLI interval. The scopes can nest and overlap, including existing worker
activity and repeated cleanup. Do not add them into a supposed total, subtract
them from the original interval, call them CPU time or use them to pass the
≤0.30 requirement. They do not cover every operation; notably there is no
separate SQL parse/bind/definitions/row-execution phase or metadata-internal
split; SQL observes its shared admission, physical prepare/payload, query
capture and close. The exact
boundaries are the instrumented consumers, not a promise of exhaustive cost
accounting. All command limits, authentication and content/pixel/capture checks
remain unchanged.

Export the current reviewed 25-file runner/test closure above, then run against
the matching **new** product checkout and artifact, with fresh owned evidence.
The nine existing inputs have the same meanings; no global environment change
is needed:

```powershell
$phaseProductRoot = "<new product checkout matching the sealed profiled artifact>"
$phaseRustArtifact = "<absolute matching optimized artifact.json>"
$conditions = "<fresh truthful phase-diagnostic conditions.json>"
# $runner, $goSource, $goPackage, $luaLS, $prepared.fixtures and $sharp
# are the reviewed absolute runner and authenticated prepared input paths.
Set-Location $phaseProductRoot
node --test "$runner/tests/rust-performance-paired/data-runner.test.mjs" "$runner/tests/rust-performance-paired/data-resume.test.mjs" "$runner/tools/rust-owned-process.test.mjs"
if ($LASTEXITCODE -ne 0) { throw "Staged Data/native owner checks failed" }
node "$runner/tools/rust-data-performance-paired.mjs" $phaseRustArtifact $goSource $goPackage $luaLS $conditions $prepared.fixtures.catalog $prepared.fixtures.content $prepared.fixtures.images $sharp --data-phase-profile
if ($LASTEXITCODE -ne 0) { throw "Data phase diagnostics failed; preserve independent product and diagnostic evidence" }
```

Cloud Node tests exercise the real entry/controller with synthetic product and
native-owner boundaries, including all 48 measured calls, environment isolation,
strict stderr parsing and preservation of simultaneous failures. They are not
Windows execution, product phase measurements or a speedup claim. The source
checkout also checks the parser's phase names against the Rust recorder; that
source-only check is explicitly skipped in a runner export without Rust source.

### Incremental-Z product detail records

New matching products emit `lycheedev.data-query-phases.v2`: the original eleven
`phases`, total elapsed and measurement boundaries remain, with a separate exact
ten-entry `details` vector. The reader accepts historical v1 and v2 strictly by
schema; it never invents absent v1 details. The detail spans identify encoded
identity/read/MD5, Z inflation, whole CKey hashing, decoded-cache open/lookup/verify,
stream publication and cache commit. They nest in existing business phases and
may overlap; do not add them to the parent or deduct them from CLI time. Use the
same `--data-phase-profile` command with a newly matched artifact. No change to
the old 928 samples, acceptance thresholds, fixtures, deadlines or owner rules.

### Publisher diagnostic scopes (v3)

New matching products emit `lycheedev.data-query-phases.v3`, retaining the same
11 business phases and the same 10 v2 details, plus a separate `publisher`
vector of 10 fixed spans: `source_to_spool_inclusive`,
`spool_to_shard_inclusive`, `final_verify_inclusive`, `stage_create`,
`stage_seal`, `digest_lock`, `shard_open`, `rename`, `winner_open`, and
`checked_close`. This is the existing verified publisher path; it does not
remove a read, hash, durable transaction, control check or checked cleanup.
The recorder remains off by default without extra clock reads.

These spans are nested diagnostic wall times, not a new additive business
ledger. `winner_open` ends when `open_winner` returns its file; `final_verify_inclusive` starts later in `verify_opened`, so these two are disjoint on successful fresh and reused publication paths (the earlier documentation incorrectly called them nested); error-path closes in
those scopes remain included there. `checked_close` covers explicit publisher
closes only; a delivered blob's later close belongs to the original
`source_close` phase. Initial parent opening and work between spans remain in
the measured remainder. Never add parent/child values or subtract them from
original native CLI time.

Use the unchanged `--data-phase-profile` entry with a newly matched product.
The reader strictly accepts v1, v2 and v3, preserving absent historical vectors
rather than inventing observations. The new reader has actually accepted all
24 original ab4 v1 stderr records and all 24 original 8cab v2 records unchanged;
that compatibility check is not a v3 Windows measurement. The new Core/Data Rust regressions passed on the fixed 5d4 tree (PR14 comment 6233193157). Matching b3d v3 diagnostic measurements completed: 12 paths, 24 cold/warm rows and 48 CLI calls were equivalent with valid timing and complete process cleanup (PR14 comments 6233589545 and 6233633639). This is one pair per path, not 20-pair performance acceptance; 46 memory observations succeeded and two have sampling gaps.
