# Current Go 3.2.4 / Rust Agent generation pairs

The current Go reference is `29bdbdaff5f064ec212ff2064380b1ccac652eda`.
Current generation measurements use typed Agent query/Page/Capture with explicit
`data cache prepare`; the current entry accepts a request JSON:

```text
node tests/rust-performance-paired/agent-prepare-fixtures.mjs --csv-numeric
node tools/rust-data-generation-paired.mjs --prepare-importer
node tools/rust-data-generation-paired.mjs <request.json>
node tools/rust-data-generation-paired.mjs <request.json> --acceptance
```

Use schema `lycheedev.paired-agent-request-inputs.recovered.v1`, `mode: "generation"`,
absolute `artifact`, `goSource`, `goPackage`, `luaLSArchive`, `importerReceipt` paths,
and the same five machine-condition strings as the [Data entry](data.README.md).
`fixtures` has exactly `numeric`, whose fields are `directory`, `preparation`,
`preparationSHA256` from the fresh `csv-numeric` preparation. `$goPackage` denotes
the authenticated Go 3.2.4 `dev-npm-stage`; old Go 3.2.3 packages cannot substitute.
See [the current owner](agent-paired-owned.mjs) for the exact request contract.

Preparation uses the ordinary existing target, default stack, locked/offline
`-j 1`, fixed 600 seconds, no one-shot and isolated user state. Reuse a matching
optimized artifact; otherwise prepare it explicitly with
`node tools/rust-build.mjs <new-artifact-directory> release` before measurement.
The current acceptance mode preserves twenty pairs and full result/CAP replay.

## Historical static SQL generation contract

The following Go 3.2.3 product/configuration descriptions and reports retain
their original identity. Their old multi-argument commands belong to that
historical collector, not the current Agent entry above.

This is a separate opt-in lane, not a change to historical B04 reports or to
`rust-data-performance-paired.mjs`. It invokes the real default CLI's
`data cache prepare NumericSample` and ordinary `data sql`. It never invents a
DataFusion CLI route. Optional DataFusion qualification is not measured here.

## Scope and clocks

The input is the existing formally produced, authenticated 16,384-row
NumericSample CASC/WDC fixture (`prepare-data.mjs --csv-numeric`). The producer
report, original owner/sealed logs, Cargo receipt, numeric ABI source/lockfile,
pin, definitions, CASC identities, full WDC cells and inventory are checked
before use and again after samples. No new toy loop substitutes for the CLI.
All 16,384 ordered rows and five columns are checked against independent expected
values, not only count/hash. All stable public fields remain compared.

Four distinct homes per pair are independently cloned from each engine's own
successful `init` plus identical authenticated fixture documents. Application
cache state is defined below; OS caches are not flushed.

| Task | Rust | Fixed Go 3.2.3 | Interpretation |
| --- | --- | --- | --- |
| fresh-wdc | first SQL, no generation | first SQL in independent cold home | cold WDC task |
| prepare-inclusive-first | explicit prepare + first SQL in another fresh home | first SQL in another fresh home | complete first-result task, includes conversion/authentication/publication |
| first-after-prepare | first SQL after timed prepare | same Go first SQL | conditional diagnostic only, never cold acceptance |
| subsequent-hit | next SQL on own prepared home | next SQL on own successful query home | repeated task with actual own cache state |

The explicit prepare time is also reported on each pair. No prep/import cost is
moved out of the prepare-inclusive cold claim. Every CLI sample uses the existing
native monotonic CreateProcess-to-tree-reap/output-drain clock, with original
process safety, output, cancellation and memory observation limits. There is no
time subtraction. Decoder/CAP/fixture audits are outside product elapsed but
never remove product authentication or output work from it. Sampled owned Job
working set/private bytes are not unique physical RSS; CPU/IO are not measured.

`--observation` runs exactly one alternating-order pair (seven CLI samples),
without evaluating the performance target. `--acceptance` runs one separate
correctness preflight then twenty fresh alternating-order pairs. All three
accepted task ratios must meet Rust median / Go median <= 0.30. The conditional
first-after-prepare ratio never grants acceptance. Neither mode grants full B04,
all Data quality, real-client, whole-disk-accounting or five-loop audit acceptance.

## Exactness and evidence

The reader retains integers beyond 2^53 as bigint, including signed/u64 and
nested arrays. Decimal/exponent tokens cannot silently round: decimal identity
must match the finite Number's canonical decimal identity or the reader rejects
it. Wide integral exponent forms retain exact decimal bigint values. No JSON
numbers are converted into strings. Negative tests include adjacent wide values,
u64 max, arrays, rounded decimal tokens and nonfinite/underflow forms.

The measured NumericSample task contains exactly representable f32 fractions;
it does NOT itself demonstrate actual CLI u64/array/partial behavior. Those are
separate required native quality tests below. This distinction is intentional.

Each SQL result requires complete source coverage, no truncation, full source
CKey/EKey/hash checks and original resource/memory budgets. Exact signed CAP
identity, metadata database entry and full result blob are verified and archived;
CAPs are rechecked after later commands. Prepared reads require the exact public
`typedGenerationCache` hit, same generation/object identity, no publication, and
unchanged fully hashed IPC bytes/manifest before and after queries. The manifest
is independently bound to fixture source/definitions/pin and prepare fields.
Full manifest and IPC bytes survive successful scratch cleanup in evidence.

The fresh prepare envelope validates source/coverage, batch/row/order identity,
original memory/work budgets and decimal 5,000,000,000-byte Data disk quota.
Incomplete legacy disk attribution remains explicitly false, not upgraded by the
harness. Every native sample proof is revalidated by the outer reader; hand-edited
elapsed values or unclosed owners/readers cannot pass. Failed/unresolved work
retains its owned scratch and logs. Only succeeded, reaped owners are reclaimed.

## Inputs

Create a fresh JSON configuration using verified absolute paths and hashes:

```json
{
  "schema": "lycheedev.data-generation-config.v1",
  "ownerRoot": "<matching product checkout / owner root>",
  "artifact": "<existing matching optimized artifact.json>",
  "goSource": "<clean fixed Go 3.2.3 checkout>",
  "goPackage": "<authenticated Go 3.2.3 npm stage>",
  "luaLSArchive": "<already retained pinned LuaLS archive>",
  "fixturePreparation": {
    "report": "<sealed numeric fixture-preparation.json>",
    "reportSHA256": "<actual SHA256 of report>",
    "sourceTreeSHA256": "<producer report inputs.sourceTree>"
  }
}
```

The Go checkout, executable metadata and package must identify exactly
`3766f9cacb52e93a803a626187e2a1d54a445468`. The optimized Rust artifact must match
current product bytes and the default offline build recipe, same invocation,
full unmodified upstream Emmy identity and beta 3.5.0 configuration. Runner and
product roots may differ: all executing helper bytes are bound separately.
Machine conditions JSON requires nonempty `machine`, `powerAndThermal`,
`competingLoad`, `disk`, `operator`. Changed power scheme rejects completion.
No downloads, build, fixture creation, package installation or game operations
are hidden in the performance entry.

## Exact native queue, coordinator only

Freeze the final reviewed harness commit; keep product/source/artifact identities
separate. Use the existing single parent-owned Windows queue. Do not operate the
user's PC except through its authorized test task. No commands here have been
executed merely because they are listed.

1. `node --test tests/rust-performance-paired/generation-contract.test.mjs tests/rust-performance-paired/generation-evidence.test.mjs tests/rust-performance-paired/data-runner.test.mjs tests/rust-performance-paired/data-csv-diagnostics.test.mjs`
2. Run the existing implementation-quality queue before performance. Required
   generation subset (exact current libtest names/counts must be retained):
   `node tools/rust-cargo.mjs --owner generation-quality -- test --locked --offline -j 1 -p lycheedev-data --lib local_query::generation_tests:: -- --test-threads=1`
   This includes point/u64+CSV/arrays, scan/filter/aggregate/self-join, partial CAP,
   absent/missing/corrupt generation, cancellation/close/failure and budgets.
   Generation producer/retirement full group:
   `node tools/rust-cargo.mjs --owner generation-lifecycle -- test --locked --offline -j 1 -p lycheedev-data --lib generation:: -- --test-threads=1`
   The parent's full default/qualification all-targets and remaining quality
   queue remains authoritative; this subset does not replace it.
3. Reuse an already matching optimized artifact where available. Otherwise, only
   through the approved queue: `node tools/rust-build.mjs <new-owned-artifact-dir> release`.
   Preserve EXE/record and same-invocation PDB when using symbols.
4. Reuse the original immutable numeric fixture preparation report. Only when
   genuinely absent, explicitly produce a new separately identified fixture:
   `node tests/rust-performance-paired/prepare-data.mjs --csv-numeric`.
   Never overwrite/relabel a historical fixture or report.
5. First diagnostic: `node tools/rust-data-generation-paired.mjs <config.json> <conditions.json> --observation`.
   Read the full report, all seven proofs/results, final original owner receipt
   and outer exit. A printout alone is not retirement or acceptance.
6. Only after complete quality/observation gates and parent authorization:
   `node tools/rust-data-generation-paired.mjs <config.json> <conditions.json> --acceptance`.
   Retain failures and all pair results. Missing/incomplete groups are not passes.

Cloud Node contracts and static syntax are developer verification only. Native
harness execution, actual quality gates and performance remain NOT_RUN until the
coordinator records exact fixed-SHA Windows evidence. No runtime/performance
claim follows from these files or the synthetic input alone.

## Explicit new-product profiled observation (v2)

Historical v1 configuration and its observation/acceptance modes retain the
original `automaticEvictionEnabled: false` prepare contract. Historical product
7bd4216c28b45761f27239e1d74536af71b11a9b, its complete/failed runs and their timings
are never relabeled, merged, or changed to satisfy a later product policy.

The new diagnostic lane is deliberately separate:

`node tools/rust-data-generation-paired.mjs <new-product-config.json> <conditions.json> --profile-observation`

Use all v1 path/fixture fields, change schema to
`lycheedev.data-generation-config.v2`, and add exactly:

```json
"product": {
  "commit": "<exact frozen new product commit, 40 lowercase hex>",
  "sourceTreeSHA256": "<matching artifact inputHashes.sourceTree>",
  "binarySHA256": "<matching optimized artifact binarySHA256>",
  "automaticEvictionEnabled": true
}
```

This mode verifies the actual clean product checkout HEAD and source/EXE identities
before work and at final integrity checks. It rejects the historical 7bd commit
and source bytes. The new true flag is required, not optional: the original 5GB,
all remaining accounting gaps, complete prepare context including timeoutSeconds
300, resources/memory, exact rows/CAP/IPC and owner lifecycle remain checked.
V2 cannot use `--acceptance` or ordinary `--observation`; v1 cannot enable this
profile mode. This initial new-product diagnostic runs exactly one seven-command
pair and never grants performance acceptance, even when its raw ratio is small.

Only the four measured Rust invocations receive both
`LYCHEEDEV_DATA_PHASE_PROFILE=1` and `LYCHEEDEV_DATA_CLI_PROFILE=1`. Both variables
are stripped case-insensitively for Go, init/preparation helpers and ordinary
measurements. Each profiled Rust stderr must contain exactly two compact records:
Data v5 first, CLI v1 second. Unknown stderr, missing records, failed rendering,
wrong commands, missing successful SQL/import scopes or a claimed fresh-WDC hit
reject the observation. Subsequent generation samples must have actual nonzero
open/authenticate/directory/batch/cursor scopes as well as public hit evidence.

Complete stderr bytes are preserved and hash-bound; the outer completion reader
reparses both records rather than trusting their summary. Output schemas/state
names identify a profiled observation separately from historical runs. All
existing native elapsed clocks and explicit prepare-inclusive cost remain intact.
Nested Data scopes overlap and are not additive. CLI timing begins at main entry,
excludes pre-main loader/OS work and its own emission, and dispatch includes Data
profile emission. The external elapsed remainder is unattributed, not precisely
startup or CPU time. Instrumented ratios are diagnostics, not an uninstrumented
performance pass. No diagnostic phase is subtracted from accepted elapsed time.

After the reviewed instrumentation product is frozen, use its actual artifact
hashes in this new configuration, reuse the immutable authenticated numeric
fixture, then dispatch only this one-pair mode through the parent's existing
native test queue. Do not substitute the currently running/historical product or
change its receipts. Native execution remains NOT_RUN until its new evidence is
recorded; cloud parser tests are not execution of the Rust timers.

### Native owner startup attribution in the profiled lane

The same existing owner startup diagnostics are now enabled for all seven measured
invocations (three Go and four Rust) in `--profile-observation` only. Initialization
and other unmeasured helpers remain unchanged. No public flag or environment
variable is added; ordinary observation and acceptance do not enable this feature.
Each successful sample retains the original native diagnostics and a
`startupBreakdown` summary, which final completion independently recomputes and
compares. A missing, malformed, or changed receipt/summary rejects completion.

`rootExitTicks` is the owner's observed root-exit time, not the exact OS exit
instant. Resume-to-observed-exit includes scheduling, loader/runtime, product work
and observer latency; it is neither pure product CPU nor evidence of a fixed
50ms floor. Root CPU comes from the original process handle only. All original
CreateProcess-through-reap/drain elapsed time remains included, with no subtraction.
Earlier profile receipts without startup diagnostics remain historical evidence,
not complete evidence for this upgraded observation. Rerun one full seven-sample
pair after freezing the combined product/tooling artifact; do not merge runs.
