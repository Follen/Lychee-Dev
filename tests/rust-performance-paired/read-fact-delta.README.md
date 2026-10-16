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
`lycheedev.source-facts.rust.v6` (the default) or explicit `v7` below; unsupported or missing inputs fail
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
