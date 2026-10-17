# Narrow Data Go 3.2.3 / Rust paired entry

This is candidate benchmark wiring, not a measured result. No product execution,
speedup or B04 acceptance is established by the synthetic Node tests. Use the
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
Failure stops that path, retains its inputs/evidence and never creates a
replacement or a warm label after failed cold. Independent paths may continue
only once process retirement is proved.

## Required prepared inputs

Run from the frozen, reviewed candidate checkout on Windows, under the existing
single local test queue. Use committed tools, not a historical dirty working
copy. Do not run additional Cargo jobs, borrow a target directory, remove a
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
node --test tests/rust-performance-paired/data-runner.test.mjs tools/rust-owned-process.test.mjs
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
