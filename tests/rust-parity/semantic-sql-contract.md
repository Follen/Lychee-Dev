# Pinned semantic SQL candidate

Oracle: Go v3.2.0 at `3057a9666ab078270b63c7f06de748069b0d1f4b`.
This is the next bounded SQL provider batch after effective SQL R4. R1–R4
snapshots and receipts remain historical and unchanged. All Rust, Go and
Windows execution for this batch is **NOT_RUN**. Static formatting is not a
compile, native execution or differential pass.

## Implemented scope

- Real `meta.Table` provider, with case-insensitive catalog/table lookup and
  canonical table identity from the pinned manifest. A valid ordinary DBD is
  required before semantic resources, including an empty semantic catalog.
- Exact-commit `meta/mapping.dbdm`, `meta/enums/*.dbde`, `meta/flags/*.dbdf`;
  cached resources are hashed and parsed, with no refetch after corruption.
  A missing applicable mapping yields a complete empty table, not knowledge
  that all possible enum values are known.
- Selected-table mapping grammar, ignored unrelated/COLOR lines, optional
  explicit conditions, build applicability, null unnamed values, comments and
  exact signed/unsigned 64-bit values, including the high bit and u64::MAX.
  Conditions are returned as metadata and never implicitly applied to DB2 rows.
- Eight fixed columns: Field, Kind, Definition, Value, Name, Comment,
  ConditionField, ConditionValue. Shared definitions are loaded once per
  metadata source. Go limits remain 4 MiB per text document, 4096 mappings,
  128 distinct enum/flags files, 16 MiB aggregate definition bytes, and 100000
  individual/combined rows. No new metadata-specific hard caps were added.
- `semanticSources` provenance and true metadata rows compose with existing
  static and explicit-effective SQL. Static WDC provenance stays in `sources`;
  semantic bytes do not inflate `decodedSourceBytes`. Original request number
  lexemes, effective capture references, and post-SQL CASC checks are unchanged.
- SQL max-byte argument admission now also runs for metadata-only queries.
  Existing request/table-count/name/escaped-size/pin/parameter conversion
  ordering is retained before source access.
- For this provider, cached blob reads and blob/metadata publication carry the
  original caller control; checks also surround HTTP and parsing steps. HTTP
  reads are bounded and checked before/after each read. Existing transport
  calls, SQLite open/read and a single schema parse remain indivisible calls;
  this batch does not claim fully interruptible I/O.

## Resource evidence and deliberately unfinished work

The Go `resources` object is logical conservative admission accounting, not
RSS or a measured peak. `resource/budget.go` defines retained/metadata/work/
network counters and separate live scratch. The fixed definitions regression
asserts cold/warm `4 × actual bytes` counts exactly. The current Rust shared
loader charges `8 × bytes`; therefore changing the field name alone would not
establish the Go admission or output contract.

`semantic-sql-resource-gap.json` preserves a concrete legal Go input: 16 cached
1 MiB comment-only metadata definitions. Go definition load+parse subtotal is
80 MiB, below the 128 MiB metadata limit; the Rust nominal subtotal is 144 MiB
and it rejects during the fifteenth load. These numbers are source-derived,
not executed measurements. Resource accounting remains an open parity defect.
This batch leaves the existing `resourceAccounting` output unchanged rather
than manufacturing Go counters or partially adding a 64 MiB executor charge.

The new parser/source uses the existing Rust safety ledger before retaining
rows and copied strings. Its counters are honest admissions, but not proof of
complete allocator/RSS accounting or equivalence to Go's logical ledger.
Plan/order-container transient allocations and single-codec-call accounting
remain open. The 100M main decode-work ceiling and effective R4 publication-only
reference budget `16 × 1 MiB × 8` are unchanged.

Also still open:

- CSV is `--encoding csv --output <file>`, not stdout. Fixed Go writes a query
  capture, a CSV capture, then destination-scoped paired files with manifest
  last. Destination transaction/recovery/control must reuse the reviewed shared
  filesystem primitives in a later batch; no placeholder is called complete.
- EXPLAIN/ANALYZE needs real schema-only preflight, plan steps and actual scan,
  work and charged-byte measurements. Go emits null columns/rows plus plan;
  ordinary empty SELECT emits an empty row array. Current Rust rejects EXPLAIN.
- Complete QueryResult field order/archive formatting and resources schema,
  all relational preflight/value/error differences, remote/CDN, and wider
  deadline propagation need their own implementation/differential evidence.
- Performance and the full four-loop acceptance process remain unperformed.

## Regression meaning

The test map records exact Go files and test names, Rust declarations and
unexecuted status. Synthetic integration tests exercise actual metadata caches,
query dispatch, authenticated static/effective data and query captures. The
cancellation-after-validation test cancels a real cached load at that boundary;
it is not a claim that cancellation was injected during an OS read. Normal
pre-cancel/deadline tests and fresh retry are separate. Corruption, missing data,
strict syntax, source-count and combined-row failures assert no query capture.

New CI entrypoints: unit filter `definitions::semantics::tests::` and integration
target `semantic_sql_parity`. Existing `local_query::`, effective SQL, definitions
batch, relational and capture-reference groups must continue to run. The parent
owns CI configuration and publication after independent frozen-tree review.

## R2 narrow Unicode table-filter correction

R1 is preserved as `semantic-sql-r1-freeze.json`,
`semantic-sql-r1-test-map.json`, `semantic-sql-contract-r1.md` and its original
complete snapshot. R2 changes only the new mapping-table comparison and adds
two focused Rust tests. Canonical manifest names remain ASCII. Go 1.27.1
`strings.EqualFold` / `unicode.SimpleFold` admit long-s U+017F into the S/s
class and Kelvin-sign U+212A into K/k. No other non-ASCII scalar joins an ASCII
fold class in the fixed `asciiFold` / `caseOrbit` tables. Dotted/dotless I,
sharp-s expansion, ligatures and fullwidth compatibility forms remain distinct.
The helper allocates no normalized string and preserves full-name length.

`semantic-sql-fold-static.mjs` verifies those exact saved Go source hashes,
traverses all 128 ASCII fold classes and checks the production Rust mapping
arms/guards. This is source-derived Node evidence, not execution of Rust or Go.
The focused parser regression also ensures malformed syntax in a Unicode-folded
selected mapping errors rather than being ignored; integration checks real SQL
rows, unchanged canonical table provenance, and no load of a dotless-I near-match.
The prior resource-admission counterexample, CSV, EXPLAIN and all other open
contracts are unchanged. All 17 Rust regressions remain NOT_RUN.
