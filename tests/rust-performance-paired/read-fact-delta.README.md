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
