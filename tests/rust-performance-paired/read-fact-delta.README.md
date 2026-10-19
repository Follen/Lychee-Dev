# Read existing source fact differences

This diagnostic reads a retained failed pair. It does not run either CLI, rebuild
an index, change a cache, adjust the acceptance comparison, or erase a failure.
Wait until the original paired query work is terminal before reading its homes.
Keep its original commit, artifacts, raw outputs and failed report unchanged.

From the original failed pair's Go home, enumerate only
`source/v1/facts/snapshots/*/manifest.json`. Read each `summary` and require exactly
one match for the original `repository`, `product`, `commit` and `parser`. Use
`records.jsonl` next to that manifest. Do not choose the newest file or the first
match. Zero or multiple exact matches are a blocker to report.

```sh
node --check tests/rust-performance-paired/read-fact-delta.mjs
node --test tests/rust-performance-paired/read-fact-delta.test.mjs
node tests/rust-performance-paired/read-fact-delta.mjs "GO_HOME" "RUST_HOME" "GO_RECORDS_JSONL" > "NEW_EVIDENCE_DIR/static-fact-delta.json"
```

The caller chooses a new evidence destination. The reader itself writes only to
stdout. It authenticates Go records against their manifest and both fact caches
against their input identity and stored fact digest. It supports the fixed Go
`lycheedev.source-file-facts.v2` and historical Rust
`lycheedev.source-facts.rust.v6` (the unchanged default) or explicit `v7` through
`v11` below; unsupported or missing inputs fail
rather than triggering a product rebuild. Go digest verification retains its
original JSON bytes, including HTML/U+2028/U+2029 escaping.

The report contains all per-category and per-extension totals, every differing
file's five-section counts, and the first ten differing sections' original rows,
source hashes and fact-file hashes. It reports duplicate multiplicity and array
order separately. It does not omit differing fields or normalize line, scope,
name, category, confidence or signature values. Samples are explicitly bounded;
the original caches remain the full evidence. Compare Rust totals to the retained
Rust command's summary separately; this reader does not locate or authenticate
the Rust aggregate index and cannot establish complete Go/Rust acceptance.

Tests use distinct `os.tmpdir()` fixtures. A successful test removes only its own
fixture; a failed test prints and retains that fixture for inspection.


For a newly built public-facts v7 candidate, pass an explicit fourth argument:

```sh
node tests/rust-performance-paired/read-fact-delta.mjs GO_HOME RUST_HOME GO_RECORDS_JSONL v7 > static-fact-delta-v7.json
```

The default remains `v6` for the original failed 5bbb pair. `v7` selects the
`facts-layout-v7-go323-public` / `generator-v6` identity and authenticates the
`lycheedev.source-facts.rust.v7` envelope. The reader never mixes layouts or
chooses whichever cache happens to exist. Both modes compare every field and
array position; neither changes the original paired-run verdict.

For the subsequent malformed-input contract candidate, explicitly select `v8`:

```sh
node tests/rust-performance-paired/read-fact-delta.mjs GO_HOME RUST_HOME GO_RECORDS_JSONL v8 > static-fact-delta-v8.json
```

This authenticates `facts-layout-v8-go323-malformed` and
`lycheedev.source-facts.rust.v8`. The generator remains v6; changed lexical/XML
diagnostics invalidate the complete index/query identity and per-file facts
cache. Historical v6 and v7 caches remain readable only with their own explicit
identity. No earlier result is relabeled, and malformed-input differential
checks are separate from the normal full-Retail comparison.

The correction based on the first actual 52-case Go observation uses explicit
`v9`, `facts-layout-v9-go323-observed`, and
`lycheedev.source-facts.rust.v9`. Its whole-index/query fingerprint changes too.
Use v8 only for the original e843 candidate and v9 for the subsequent corrected
candidate; no automatic discovery or historical relabeling is performed.

For the XML lexical-diagnostic candidate beginning at `6d26bd6`, explicitly
select `v10` against that candidate's terminal Rust home:

```sh
node tests/rust-performance-paired/read-fact-delta.mjs GO_HOME RUST_HOME GO_RECORDS_JSONL v10 > NEW_EVIDENCE_DIR/static-fact-delta-v10.json
```

This authenticates the exact `facts-layout-v10-go323-xml-lexical` /
`generator-v6` key and `lycheedev.source-facts.rust.v10` envelope. That candidate's
whole-index/query fingerprint is v11; the argument here selects the per-file
Facts layout, so it is `v10`, not `v11`. Use a new evidence destination and
retain the authenticated product, runner and input identities separately.
Historical v6–v9 caches and prior outputs remain unchanged and require their
own layout; neither auto-discovery nor schema relabeling is permitted. The
default remains v6. All existing digest, complete field/array-order comparison
and diagnostic-only limitations above remain in force.


The generated-parenthesis/public-operator cache migration after `8cab848` uses
explicit per-file `v11`:

```sh
node tests/rust-performance-paired/read-fact-delta.mjs GO_HOME NEW_RUST_HOME GO_RECORDS_JSONL v11 > NEW_EVIDENCE_DIR/static-fact-delta-v11.json
```

This selects `facts-layout-v11-go323-generated-origins`, `generator-v6`, and
`lycheedev.source-facts.rust.v11`. The whole-index/query fingerprint is v12;
the serialized index container remains v6 because its shape is unchanged.
The public `source-v1` parser revision is unchanged. This invalidates valid old
caches for restored parenthesized documentation/parameters, accurate outer-origin
and table-body-end spans, and source-order public operator diagnostics.

The migration tests write authenticated v10 envelopes containing the two actual
9a public observations, retain their old paths and bytes, verify current-path
schema rejection/rebuild, and require subsequent current warm hits. A valid old
whole-index container is reconstructed from those sealed rows with recomputed
symbol ordinals/IDs, content digest and old manifest identity. The integration
regression requires readiness to remain false until a current rebuild, then
checks complete-index and query-cache warm reuse under the new fingerprint.
These fixtures are reconstructed test caches, not claimed captures of old cache
files. These two migration regressions subsequently passed in the full afbeb cache26/0/0 group; see PR14 comment 6232978472. This does not validate subsequent pack storage.

The original full-4036 comparison at 8cab used v10 and stays v10. Existing paired
bbfe/ab4 product identities still explicitly use v10. The reader is one of the
runner-hashed inputs, so its additive v11 support changes the current runner
input digest; preserve the earlier exact runner bytes/receipts rather than
claiming their identity is unchanged. Historical v6–v10 layouts,
source/fixture bytes, reports, digest checks and comparison order remain intact;
the reader default stays v6. Never auto-discover a layout or relabel a prior
report. The new explicit reader support does not itself rerun either product or
prove cache migration in Rust.


The cohort-pack successor is selected explicitly as `v11-pack1`. Its command
requires this run's Rust index blob and authenticated manifest SHA in addition
to the original inputs; see the [full pack reader and storage contract](README.md#full-same-run-facts-comparison). Historical per-file modes remain unchanged.

The stable-path successor is selected explicitly as `v11-pack2`, with the same
required same-run index file and SHA arguments. Its production group-key domain
is `facts-pack-v2-path-radix512`; the whole-index fingerprint is `rust-index-v14`.
It partitions authenticated paths into deterministic binary-radix leaves of at
most 512 eligible documents, then emits at most 64 documents per pack. The wire
magic `LYCFPK01`, locator and complete v11 envelopes deliberately remain unchanged.

The requested reader layout is recorded in the report, but the bare pack/index
bytes cannot identify which grouping algorithm created them. Bind pack2 results
to the exact candidate source/artifact as well as the authenticated index SHA;
selecting pack2 does not authenticate or relabel a historical pack1 result. The
original pack1/per-file readers and all old reports stay unchanged. The new
reader performs the same complete five-array, input, range, header and digest
checks; it does not discover groups by scanning cache directories.

## Explicit v13 Postcard comparison bridge

`v13-postcard` is the new private `facts-layout-v13-postcard-1.1.3` layout,
with `rust-index-v17`, the unchanged `LYCFPK01` container, and `LYCFCT13`
record framing. Node authenticates the outer frame, payload hash, pack record
key/range/hash, full container, index and custody. It does **not** decode
Postcard. Only the same-source Rust lib-test executable calls the shared
production decoder and serializes the complete public Facts arrays.

Historical readers, CLI argument modes, v6 default, historical runner file
sets and v11 resume paths remain unchanged. Direct `readRustFact` rejects v13;
the explicit `readFactDelta` v13 option requires a request-local authenticated
export token. A caller-supplied reader, copied token or standalone JSONL file
cannot substitute for the owned exporter invocation.

The executable first slice is:

```sh
node tests/rust-performance-paired/source-pack-scenarios-postcard.mjs CONFIG.json
```

This Windows-owned entry **compares existing outputs**, rather than rerunning
Go or Rust indexing. It uses an existing independently produced Go v3.2.3
custody reference from an authenticated `source-pack-scenarios.v1` observation,
and a terminal v13 Rust home. It verifies the exact persisted pin and v17 index
manifest through the existing read-only metadata snapshot. There are zero
product CLI invocations and one test-exporter invocation. No old sample is
relabeled; this command makes no performance verdict. Exporter wrapper wall
is separately recorded in `export-invocation.json` and the result's
`exporterTiming`, never folded into or subtracted from old product wall time.
This entry continues to report `sixScenarios: "not_run"`. The explicit fresh
six-scenario entry below performs new product invocations instead.

The exact configuration fields are:

```json
{
  "schema": "lycheedev.source-postcard-comparison-config.v1",
  "layout": "v13-postcard",
  "ownerRoot": "ABSOLUTE_MANAGED_OWNER_ROOT",
  "candidate": {
    "sourceRoot": "ABSOLUTE_SOURCE_ROOT",
    "artifactRoot": "ABSOLUTE_ARTIFACT_OWNER_ROOT",
    "artifact": "ABSOLUTE_SEALED_CLI_ARTIFACT_JSON",
    "commit": "EXACT_40_HEX_COMMIT",
    "artifactSHA256": "EXACT_64_HEX_SHA",
    "binarySHA256": "EXACT_64_HEX_SHA",
    "sourceTreeSHA256": "EXACT_64_HEX_SHA"
  },
  "exporter": {
    "artifact": "ABSOLUTE_SEALED_EXPORTER_ARTIFACT_JSON",
    "artifactSHA256": "EXACT_64_HEX_SHA",
    "binarySHA256": "EXACT_64_HEX_SHA",
    "sourceTreeSHA256": "SAME_CANDIDATE_SOURCE_SHA"
  },
  "rustHome": "ABSOLUTE_TERMINAL_RUST_HOME",
  "pin": { "id": "EXACT_PIN_ID", "source": "USE_THE_COMPLETE_ORIGINAL_PIN_SOURCE_OBJECT_AND_REMAINING_PIN_FIELDS" },
  "go": {
    "reference": { "directory": "ABSOLUTE_CUSTODY_DIRECTORY", "catalog": "ABSOLUTE_GO_CUSTODY_JSON", "catalogSHA256": "EXACT_64_HEX_SHA" },
    "observation": "ABSOLUTE_ORIGINAL_SOURCE_PACK_SCENARIOS_REPORT",
    "observationSHA256": "EXACT_64_HEX_SHA",
    "packageRoot": "ABSOLUTE_FIXED_GO323_PACKAGE",
    "luaLSArchive": "ABSOLUTE_FIXED_LUALS_ARCHIVE",
    "packageSHA256": "EXACT_AUTHENTICATED_PACKAGE_SHA"
  }
}
```

The placeholder `pin` above must be replaced by the **complete original pin
object**, not merely those two illustrative fields. `go.reference` must equal
an original scenario's recorded `facts.goCustody`; its archived bytes are
authenticated again and matched to the actual full-array comparison sources.
The existing Go package verifier authenticates the fixed v3.2.3 producer.

Prepare the exporter outside this comparison with the new, narrowly scoped
`prepareFreshPostcardExporter` helper and a validated fresh configuration
(shown below). This prepares only the exporter, with zero product CLI calls:

```js
const prepared = prepareFreshPostcardExporter(validatePostcardFreshRequest(config));
// prepared.request.exporter contains the resulting authenticated artifact IDs.
```

The fixed command is `cargo test --locked --offline -j 1 -p lycheedev-source
--lib --no-run --message-format=json-render-diagnostics`. Reuse controlled
targets, preserve the created artifact record and supply its actual hashes.
The adapter checks the complete current source digest against the candidate
and exporter, exact build recipe, same-invocation record, owned successful
build logs, Cargo's unique Source lib-test artifact, copied executable hash,
and the observed invoked executable identity before accepting exported data.
The helper also owns the required before/after compiler observations; calling
the old bare `sealPostcardExporter` callback without this sealed binding is
insufficient. General `runCargo` and historical runners remain unchanged.

One bounded manifest contains all documents, ordered exactly as the index.
The exporter is invoked as `--exact
facts::binary::export_tests::export_binary_facts_manifest --ignored --nocapture`.
Its manifest is at most 8 MiB, each original container/record at most 32 MiB,
summed record bytes at most 128 MiB, and JSONL output at most 128 MiB. Complete
Facts arrays, row fields, order and duplicate multiplicity are compared against
Go with the existing reader. Unsupported unsafe JSON integers fail rather than
silently rounding. A successful process, one matching summary, exact output
hash/length, complete ordered document rows and all final custody checks are
required. Failure/cancellation retains partial output and scratch evidence;
it cannot produce a successful export token.

The potentially 128 MiB JSONL is written only to owned scratch, never stdout.
Binary containers and output use the existing `splitIndexEvidence` 8 MiB parts
under the unchanged 16 MiB per-evidence-file ceiling. Existing container,
total custody and index bounds are preserved. A successful result archives
the exact input manifest, all referenced binary containers, raw output parts,
index evidence, build/process identities and comparison report before cleanup.

Focused offline verification (no Rust compilation or product execution):

```sh
node --test tests/rust-performance-paired/read-fact-delta.test.mjs tests/rust-performance-paired/read-fact-delta-postcard.test.mjs tests/rust-performance-paired/source-candidates.test.mjs tests/rust-performance-paired/source-pack-scenarios.test.mjs tests/rust-performance-paired/source-pack-custody.test.mjs tests/rust-performance-paired/source-pack-resume.test.mjs tests/rust-performance-paired/source-pack-scenarios-postcard-fresh.test.mjs
```

The v13 tests use the frozen Rust empty-wire golden and a clearly labeled
synthetic process boundary for rich full-array rows. They test authentication,
wrong source/binary/output identity, missing/extra/duplicate/order changes,
changed pack/index/manifest, incomplete/cancelled/limited processes, exact
custody and unchanged historical flows. Their passing result does not claim
the real Rust exporter ran or that Windows/full-corpus/performance acceptance
has passed.

## Explicit fresh v13 six-scenario execution

```sh
node tests/rust-performance-paired/source-pack-scenarios-postcard-fresh.mjs CONFIG.json CONDITIONS.json
```

This Windows-owned entry runs **12 new measured `source index` calls**, one
Go/Rust pair for each existing case: cold, same-home warm, single-content
change, root insert, first-file delete and cross-prefix rename. The four changed
pairs use independently prepared healthy-base clones. It also performs the
existing four untimed product preparation calls (`init`/`source sync` for each
engine), fixed Go metadata verification, corpus audits and power observations.
There are no imported measurements, historical continuations or new budget
exceptions. The source checkout must be clean at the configured exact commit;
the optimized CLI artifact must already exist and match its complete source
digest. The Go checkout/package remains the independently authenticated fixed
v3.2.3 oracle. The runner never builds a CLI or silently replaces the candidate.

Use this exact fresh configuration shape, with actual absolute paths and
recorded hashes rather than placeholders:

```json
{
  "schema": "lycheedev.source-postcard-fresh-config.v1",
  "layout": "v13-postcard",
  "ownerRoot": "ABSOLUTE_MANAGED_OWNER_ROOT",
  "candidate": {
    "sourceRoot": "ABSOLUTE_CLEAN_CANDIDATE_CHECKOUT",
    "artifactRoot": "ABSOLUTE_CLI_ARTIFACT_OWNER_ROOT",
    "artifact": "ABSOLUTE_SEALED_OPTIMIZED_CLI_ARTIFACT_JSON",
    "commit": "EXACT_40_HEX_COMMIT",
    "artifactSHA256": "EXACT_64_HEX_SHA",
    "binarySHA256": "EXACT_64_HEX_SHA",
    "sourceTreeSHA256": "EXACT_64_HEX_SHA"
  },
  "go": {
    "sourceRoot": "ABSOLUTE_CLEAN_GO323_CHECKOUT",
    "packageRoot": "ABSOLUTE_FIXED_GO323_PACKAGE",
    "luaLSArchive": "ABSOLUTE_FIXED_LUALS_ARCHIVE",
    "packageSHA256": "EXACT_AUTHENTICATED_PACKAGE_SHA"
  },
  "exporter": {
    "mode": "build",
    "artifactDirectory": "ABSOLUTE_NEW_DIRECTORY_WITHIN_OWNER_ROOT"
  }
}
```

`CONDITIONS.json` supplies nonempty strings for `machine`, `powerAndThermal`,
`competingLoad`, `disk` and `operator`; disclose unknowns explicitly. Exporter
`mode: "build"` invokes the fixed `runCargo` recipe above **once before any
measured call**, through its existing owned build channel, then seals the
lib-test artifact and authenticates it against the candidate source. The
directory must be new and its parent must already exist. Compilation is
offline, locked, single-job and one-shot with the existing deadline and disk
policy. It does not fetch missing dependencies or retry a failed build.

Before and after that build, a separate bounded owner invokes `rustc -Vv`
through `ownedSpawnSync`, in the same source-root working directory and with
the same environment snapshot. `RUSTC`, compiler-wrapper and corresponding
Cargo overrides are rejected. Cargo's existing target-directory, one-shot and
Emmy identity additions are explicitly accounted for in its effective
environment hash; an environment hash alone is not compiler evidence.
`release/tools/rust.json` currently pins **rustc 1.99.0** and
`x86_64-pc-windows-msvc`, not a moving `stable` label. Both bounded observations
must agree on the actual release, host, 40-hex commit, raw output and observed
invoked executable bytes. The small record and terminal observation-owner
receipt are SHA-bound into the exporter artifact and revalidated on reuse.
Missing, altered, incomplete or unreaped observations cannot produce an
accepted exporter. The observed executable can be a rustup shim: this is not
authentication of every compiler library, nor an actual Cargo-version
observation. The manifest's separate Cargo version is not relabeled as one.

To reuse an existing same-source exporter, replace only `exporter` with:

```json
{
  "mode": "existing",
  "artifact": "ABSOLUTE_SEALED_EXPORTER_ARTIFACT_JSON_WITHIN_OWNER_ROOT",
  "artifactSHA256": "EXACT_64_HEX_SHA",
  "binarySHA256": "EXACT_64_HEX_SHA",
  "sourceTreeSHA256": "SAME_CANDIDATE_SOURCE_SHA"
}
```

The outer owner launches a dedicated inner controller under the existing
Windows process-tree supervisor. The controller reuses the original timing,
memory observation, phase checks, candidate/package checks, case construction,
scratch ownership and disk admission. Each pair is measured first, then gets
one separately owned exporter invocation, one all-documents manifest and full
five-array comparison against that pair's retained Go bytes. Source, exporter,
runner and product identities are checked again at their original boundaries.

Each comparison archives all referenced binary containers, the exact raw
export input manifest, complete JSONL output, Go Facts, complete index parts,
metadata/index manifest, exporter build/invocation identity and full comparison.
Existing 8 MiB parts keep even output above 16 MiB within the unchanged per-file
ceiling; the 32/128/512 MiB record/output/custody limits remain unchanged. All
six archived comparisons and their owner receipts are reauthenticated before
healthy-base cleanup and again by the outer owner. A failed comparison stops
later scenarios and retains failed ownership; an unreaped process prevents
cleanup and further launches. Partial output is archived only after reaping.

Product `controllableMillis` remains the original native process interval.
`exporterPreparation.wrapperWallMillis`, each `exporterTiming`, and each
`validationWallMillis` separately describe build/authentication, exporter and
post-measurement comparison costs. These are never added to or subtracted from
product wall time. Each row retains the Rust/Go ratio; one six-case observation
does not replace the required independent repeats or broader B01–B07 acceptance.

Success returns only report/receipt paths; full Facts never go to stdout.
`postcard-owned-report.json` records actual measured index/exporter invocation
counts, preparation and outer process status. Its referenced `report.json`
holds the six pairs, product timings, full Facts custody and remaining/not-run
calls on failure. This wiring is locally tested with synthetic process results,
including an 18 MiB JSONL archive; real Rust exporter, full-corpus Windows
execution and performance acceptance remain `not_run` until actually executed.
