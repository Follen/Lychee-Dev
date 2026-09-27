# Data capabilities and SQL improvements — 2026-09-27

Local implementation, not an npm release. This closes the recommended work in
the [DBCD audit](dbcd-capability-audit-2026-09-27.md) while retaining the shared
Go reader, SQL engine, immutable evidence and honest incomplete-source state.

## Delivered

| Area | Implementation | Verification |
| --- | --- | --- |
| Encrypted IDs | Bounded WDC4+ ID lists with key/partition attribution; absence is conclusive only with complete metadata | Duplicate/count bounds; encrypted/absent/unknown cases; real Spell and SpellMisc |
| Full Hotfix scan | `--scan`, bounded pages, per-page captures and checkpoints, fixed-filter resume; errors retain the last saved checkpoint | Changed file/filter, cancellation and replay tests; six-table run below |
| Enum/flags | `meta.Table` at pinned DBD commit/build; raw values retained, build conditions checked and conditional mappings exposed | Mapping/build/path bounds, unnamed/negative/64-bit values; actual NEGATIVE_1 metadata |
| Effective SQL | Explicit `effective.Table` with query JSON `hotfix` captures; deterministic multi-capture merge; add/replace/delete and cached-key exception | Synthetic repeated IDs, signed pushes, capture precedence, deletes, unchanged base, joins and provenance; actual SpellMisc aggregate |
| Reader comparison | Independent pinned DBCD adapter and lossless JSON comparator | All readable ID digests, encrypted ID sets and first 200 rows of two real tables |
| ID/projection | Optional shared-source identity lookup and selected-column materialization | Lookup visits at most one row; partial remains partial; unused corrupt fields still fail |
| Equality JOIN | Bounded numeric-aware hash index for eligible bare equalities; fallback when ineligible or index allowance exhausted | Duplicates, mixed exact numbers, NULL, LEFT JOIN, type errors, linear work budget and memory fallback |
| Top-K | Stable bounded heap for non-grouped/non-DISTINCT LIMIT with at most one ordering key | Full-sort comparison including ties/NULL/offset; 20,000 inputs within 32 KiB retained budget; late failures preserved |
| Repeated preparation | Complete decoded content reuse keyed by encoded bytes, CKey, decoder version and key-source identity | Repeated complete/partial source queries; explicit changed private key must not reuse previous plaintext |
| Diagnostics | Definition/preparation/execution timings, decoded bytes, cache hit/reused bytes, lookup counts, bounded unknown-column candidates | EXPLAIN ANALYZE integration and binding tests |
| Agent workflow | Versioned parameter recipes, metadata discovery, explicit overlays, full-scan/resume and coverage guidance | Generated command reference and executable contract checks |

The decoded cache still verifies current encoded source bytes and cached output
digests. It avoids repeated decompression/decryption; it is not a parsed-index
cache, permanent daemon, or promise of zero I/O. Partial decoded tables are not
admitted to the complete cache. Missing-source/key/corruption rules still apply.

## Fixed real-data verification

- Product/context: retail CN zhCN, **12.1.0.69933**.
- DBD commit: `e989e99e6f5f97c57b2e138d4d28b16066b4ee9c`.
- DBCD commit: `e732093f8864240fc5884bd1bba6b02f3dfc0d56`.
- Data pin: `PIN-844e56206c15b8a81da4ca386208008d855bf69b077af8b0cbcbd6adeb14657a`.
- Raw cache: `CAP-879b6f4d55cf4e9c01f21bbb843cb9f23e654cbb4768a9c39c6f82efd9885f60`.
- Cache SHA-256: `2088263cccb0e13dc6d3fd48cd24416a8436406aeba0ca94e656769e9953338f`.

The new CLI scan produced:

| Table | Matched | Decoded | No payload | Pages |
| --- | ---: | ---: | ---: | ---: |
| SpellMisc | 858 | 836 | 22 | 5 |
| SpellEffect | 2,016 | 1,859 | 157 | 11 |
| Spell | 650 | 567 | 83 | 4 |
| SpellName | 579 | 567 | 12 | 3 |
| SpellAuraOptions | 70 | 56 | 14 | 1 |
| SpellCategories | 59 | 48 | 11 | 1 |
| Total | **4,232** | **3,933** | **299** | **25** |

All six scans returned `complete=true`, `truncated=false` for their selected
cache filters. A record with no payload is not a decoding failure. Local run
artifacts: `.tmp/hotfix-scan-<Table>.json`; immutable page and scan captures are
in the current workspace. These artifacts are local evidence, not committed
fixtures or server-wide coverage.

DBCD and Lychee agree on **417,635** readable SpellMisc IDs and **414,027**
readable Spell IDs. Both complete readable-ID digests and all encrypted-ID
sets match; each table's first **200** readable rows match field by field.
The Lychee adapter decoded every readable row. Field comparison was sampled,
not a claim of full-row-value comparison. Both tables still lack 7 key sections
covering 55 logical IDs each. See [adapter instructions](../../tests/dbcd/README.md).

`effective.SpellMisc` from the selected cache returns COUNT **417,637** versus
the static **417,635**. The final patch map contains 832 additions/replacements
and 22 deletions; these are final IDs, not physical change-record counts.
The query correctly remains `complete=false` because base encrypted coverage
is still incomplete. No source installation or running game was modified.

## Performance observations

Single-iteration Go microbenchmarks on the same development machine; these are
illustrative measurements, not release thresholds or universal speedups.
Inputs are deterministic `generatedSource` fixtures; `B/op` is total allocated
bytes reported by Go, not peak RSS.

| Query | Before | After | B/op before → after |
| --- | ---: | ---: | ---: |
| Equality join, 1,000 rows per side | 170.58 ms | 0.66 ms | 384,146,824 → 786,304 |
| ORDER BY, top 20 of 20,000 | 20.54 ms | 5.02 ms | 46,265,560 → 3,538,424 |
| ID equality, 20,000-row scan-only fixture | 4.84 ms | 3.01 ms | 7,527,280 → 2,402,720 |

The last fixture has no index callback: its improvement is scalar comparison,
not an ID-index benchmark. The actual SpellMisc indexed ID query reported
**one scan call, one row and one lookup** while preserving partial coverage.

A repeated real partial-table query reused the complete CASC root's
67,167,107 decoded bytes (one hit); the partial SpellMisc content was not cached.
Observed preparation was 1,152 ms then 745 ms, but another query ran concurrently:
this is functional cache evidence, not an isolated cold/warm performance claim.
Timing fields are wall-clock observations; very short stages may report zero.

## Verification and boundaries

`go build ./...`, `go vet ./...`, affected Go tests and the complete
`LYCHEEDEV_REQUIRE_LUA51=1 go test -count=1 ./...` suite pass. Tests include
existing SQL recursion/correlation/NULL/budget/export contracts, not just the
new fast paths. DBCD is a developer-only differential oracle; shipping CLI and
npm package acquire no .NET dependency.

The 51 Node tooling/launcher tests, version consistency, generated skill contract
(80 commands, 194 references, no violations), skill quick validation and
`git diff --check` also pass. Cancellation is injected at the page-reader
boundary so checkpoint recovery is deterministic; it does not rely on counting
database-driver context checks.

Explicit boundaries retained from the audit:

- Standalone product DB2 import and historical WDBC/WDB2–6/WDC1 formats await
  concrete use cases; the test adapter's fixture input is not a public command.
- DB2 writing and experimental BDBD are outside the research tool's current
  scope. No historical-format or game-file-writing capability is advertised.
- Differential coverage currently consists of the two real tables above plus
  our existing format fixtures, not the entire upstream format/test matrix.
- Optimizations do not constitute a general cost-based planner. Multiple
  ordering keys, DISTINCT/grouped cases and complex joins keep existing paths.
- Projection saves materialization, not unused-field validation. LIMIT does
  not hide late errors. Missing-key aggregate or empty results stay incomplete.
- Effective views are reproducible query captures, not new named snapshots.
  Field before/after comparison uses ordinary SQL. No implicit cache overlay.
- Stage timings are definition/preparation/execution; there are no separate
  per-download/JOIN/export timers or peak-memory measurements.
- Skill contract/recipe verification is not a new end-to-end interactive
  Agent/game acceptance run. No npm publication, tag or installation is included.
