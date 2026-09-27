# DBCD capability audit — 2026-09-27

This records the original source comparison and backlog. The recommended P1
work and six optimization areas have since been implemented locally; see the
[implementation and verification ledger](data-improvements-2026-09-27.md) for
the exact delivered scope and remaining boundaries. The original observations
below describe the pre-change baseline, not the current implementation. This
is not a claim that upstream has passed our acceptance matrix. It includes the local,
unreleased DB2 partial-reading changes documented in
[the encryption investigation](db2-encryption-2026-09-27.md).

Upstream inspected: wowdev/DBCD commit
`e732093f8864240fc5884bd1bba6b02f3dfc0d56`. Definition metadata inspected:
wowdev/WoWDBDefs commit `e989e99e6f5f97c57b2e138d4d28b16066b4ee9c`, already
pinned by the retail data investigation. Downloaded source was inspected;
the upstream test suite was not executed.

## Owner requirement: preserve and extend Lychee's strengths

The owner explicitly requires preserving Lychee's existing strengths, especially
SQL, while adopting useful upstream capabilities. The following are acceptance
constraints on this backlog, not newly implemented features.

- **Keep the shared SQL engine and its existing semantics.** Preserve joins,
  grouping/aggregates, NULL and exact integer behavior, CTEs (including bounded
  recursion), correlated subqueries, parameters, ordering and paging. Preserve
  read-only enforcement, cancellation, resource budgets, EXPLAIN without row
  scans and EXPLAIN ANALYZE. Existing acceptance cases DAT-04 and DAT-05 remain
  mandatory. The implementation and executable cases are in
  `internal/records/relational/` and `internal/records/sql.go`.
- **Make new data capabilities usable through SQL.** Explicit effective
  snapshots should become selectable sources for the existing query engine,
  supporting joins and aggregates through the same typed row interface.
  Enum/flags metadata should support SQL interpretation/filtering as well as
  record display, with raw numeric fields preserved. Exact query syntax is a
  future design decision; no proposed syntax is advertised as available.
  Ordinary static queries must retain their current source and result semantics.
- **Keep one data-reading implementation.** DB2 lookup, SQL, domain navigation
  and exports must share the reader, schema and provenance boundaries. Do not
  create a second parser or query engine merely to expose an upstream feature.
  DBCD is a reference and differential-test candidate, not a required production
  runtime dependency under this plan.
- **Preserve reproducibility and honest coverage.** Fixed builds, locales,
  definition commits, source digests, offline behavior and captures remain
  attached to results. Missing-key or otherwise incomplete sources must remain
  incomplete through joins, aggregates and exports, even if the query returns
  one scalar or zero rows. No implicit Hotfix overlay or source substitution.
- **Preserve the complete agent workflow.** Command discovery, structured
  results, bounded streaming, resumable operations and evidence reuse remain
  shared contracts across source research, data, assets and live investigation.
  New data features must be discoverable and covered by the versioned skill.

For each affected feature, run existing SQL acceptance tests plus focused
integration cases through the new source. Compare existing static-query outputs
and evidence before/after; exercise joins, aggregates, cancellation, budgets and
incomplete sources. Measure representative cold/warm queries for runtime and
memory before changing the data path; document any tradeoff rather than claiming
an unmeasured performance improvement. Do not replace this coverage with DBCD
row-decoding comparisons, which cannot validate Lychee's SQL or agent contracts.

## Highest-value gaps

| Priority | Capability | Current Lychee behavior | Proposed acceptance |
| --- | --- | --- | --- |
| P1 | Exact encrypted record IDs | WDC4/5 encrypted-ID metadata is skipped in `internal/records/table/layout.go`. Partial reads expose keys, partitions and counts, but cannot identify every unavailable record. | Preserve bounded ID metadata with key/partition provenance. A requested ID can be classified as readable, encrypted, or absent only when the metadata supports that conclusion; otherwise retain unknown. Test copies, multiple partitions, malformed counts and formats without complete ID metadata. |
| P1 | Complete Hotfix scan orchestration | Paged queries expose continuation, but an agent must own the loop and aggregate coverage. Reading one page does not complete a table investigation. | A bounded, resumable scan/export owns a fixed capture, filters and cursor chain. Distinguish page completion from whole-query coverage; retain checkpoints and counts for decoded/no-payload/failed records. Never infer completion from exit 0 alone or silently switch captures. |
| P1 | Enum/flags interpretation | Generic schema and rows expose numeric values without the upstream enum/flags metadata. | Load metadata at the pinned definition commit, honor build applicability, retain raw values and unknown bits, and distinguish missing mappings from a false flag. Include array-index mappings and explicitly supported conditional mappings. |
| P1 | Explicit static + Hotfix effective view | Static tables and Hotfix captures remain separate by design. SQL reads static tables. `--latest` selects a push batch, not the latest state of each record. | Create an explicitly requested derived snapshot, with base pin, selected captures, ordering/conflict policy, additions/replacements/deletions and row provenance. Preserve the base. Test repeated IDs, negative pushes, no-payload statuses and special cached tables. An effective local snapshot is not proof of complete server or runtime state. |
| P1 | Differential parser regression | Local fixtures and selected real tables are tested; there is no broad pinned-DBCD differential gate. | Compare identical files, definitions and key sets for row IDs, decoded values and unavailable-ID sets. Cover sparse/fixed records, strings, alignment, signed values, palette/common compression, copies and relationships. Treat disagreements as investigations, not automatic proof that either implementation is correct. |

The scan orchestration and differential gate are recommendations for Lychee;
they are not claims that DBCD ships an equivalent agent workflow.

### Why the upstream capabilities matter

DBCD exposes encrypted sections and `GetEncryptedIDs()` through its storage
API. Our existing WDC4+ metadata loop already reaches those IDs and skips their
bytes. Keeping them is a focused improvement to the newly added partial-read
path. This does not decrypt a missing-key section and should not promise exact
ID coverage for every older format. See
[encryption metadata reader](https://github.com/wowdev/DBCD/blob/e732093f8864240fc5884bd1bba6b02f3dfc0d56/DBCD.IO/Readers/BaseEncryptionSupportingReader.cs)
and [storage API](https://github.com/wowdev/DBCD/blob/e732093f8864240fc5884bd1bba6b02f3dfc0d56/DBCD/DBCDStorage.cs).

The pinned WoWDBDefs metadata maps `SpellMisc.Attributes[0..16]` to
`SpellAttributes0..16`, as well as SpellEffect enums and some conditional
field interpretations. DBCD offers enum membership and flag checks through an
optional provider, explicitly marked experimental. This could remove much
manual mask interpretation from aura investigations. It does **not** establish
the relationship between a runtime memory offset and a DB2 field. See
[pinned mappings](https://github.com/wowdev/WoWDBDefs/blob/e989e99e6f5f97c57b2e138d4d28b16066b4ee9c/meta/mapping.dbdm),
[enum provider](https://github.com/wowdev/DBCD/blob/e732093f8864240fc5884bd1bba6b02f3dfc0d56/DBCD/Providers/GithubEnumProvider.cs)
and [experimental API declaration](https://github.com/wowdev/DBCD/blob/e732093f8864240fc5884bd1bba6b02f3dfc0d56/DBCD/DBCD.cs).

DBCD applies Hotfix records in PushId order, with a customizable row processor
and default add/replace/delete behavior. It has special handling for cached
TACTKey-related tables and an ordering-verification TODO. Copying a generic
"invalid means delete" rule would therefore be insufficient. An effective view
would extend Lychee's current explicit-separation contract, not repair an
accidental missing overlay. A field-level before/after diff would be an
additional Lychee feature. See
[HotfixReader](https://github.com/wowdev/DBCD/blob/e732093f8864240fc5884bd1bba6b02f3dfc0d56/DBCD.IO/HotfixReader.cs).

DBCD has useful representative reading tests, but its all-DB2 test immediately
returns with a manual-run comment. Its existence is not evidence of an automated
full-table acceptance gate. See
[ReadingTest](https://github.com/wowdev/DBCD/blob/e732093f8864240fc5884bd1bba6b02f3dfc0d56/DBCD.Tests/ReadingTest.cs).

## Additional differences

| Priority | Upstream capability | Assessment for Lychee |
| --- | --- | --- |
| P2 | Direct filesystem/stream DB2 input | Useful for exported samples and reproducible bug reports. Add explicit artifact ingestion with build, definition commit and digest if pursued. Existing SQL `--file` accepts query text, not arbitrary DB2 input. |
| P2 | Combining Hotfix caches | DBCD supports combining caches and checks build equality. A Lychee version also needs explicit product/locale/context compatibility, capture provenance and conflict rules. More captures do not prove complete server coverage. |
| P3 | WDBC, WDB2–WDB6, WDC1 | DBCD covers these older formats; Lychee currently covers WDC2/1SLC and WDC3–5. Useful for historical data, but today's Classic client is not synonymous with historical DBC formats. |
| P3 | DB2 editing and writing | A separate product direction. Upstream calls writing experimental and does not support writing multiple sections; there is no current need to add game-file mutation to the research workflow. |
| P3 | BDBD binary definitions | Upstream itself describes this as experimental with little or no current advantage. Keep pinned text definitions unless measurements establish a benefit. |

Format coverage, filesystem loading and writing boundaries are described in the
[DBCD README](https://github.com/wowdev/DBCD/blob/e732093f8864240fc5884bd1bba6b02f3dfc0d56/README.md).
Cache combination is implemented in HotfixReader above; the BDBD caveat is in
the DBCD constructor documentation linked above.

## Further optimization opportunities in Lychee

These findings come from the local implementation, not from an upstream feature
checklist. The execution shapes below are confirmed by code inspection; speedup,
peak-memory impact and cold/warm latency have not been benchmarked in this audit.

| Priority | Observation | Proposed optimization and constraints |
| --- | --- | --- |
| P1 | `relational.Source` exposes columns and a full scan callback. `TableSource` flattens every decoded row/column; SQL has no source-level ID lookup or projection request. | Add optional typed lookup and scan-selection capabilities to the shared source interface. Start with exact ID predicates, then safe filtering and selected-column decoding. Reuse DB2 lookup machinery; preserve NULL, integer, copy-row, field-validation and completeness semantics. |
| P1 | `executeSelect` materializes right-hand tables and uses nested-loop joins. | Add bounded hash joins for eligible equality conditions and safe early single-table filtering. Retain a fallback for other conditions, duplicate-key multiplicity, LEFT JOIN unmatched rows, numeric equality and cancellation. Measure representative Spell/SpellMisc/SpellEffect joins before expanding to a general optimizer. |
| P1 | `executeRows` retains projected rows; `finishResults` applies DISTINCT, full stable ordering, then OFFSET/LIMIT. | Consider bounded Top-K for eligible ORDER BY/LIMIT queries, reducing retained rows while still evaluating required input. Evaluate simple LIMIT early termination separately: it can change visibility of late source/expression errors and must not silently weaken the current failure contract or imply full-source validation. |
| P2 | Sources are already prepared and reused within a query, but separate CLI queries reopen/prepare their tables. | Measure cross-invocation cost before adding a bounded derived index or reusable prepared artifact. Key it by exact source, definition, locale, decoder and key-set/coverage identity; a partial read must not poison a later complete read. Preserve offline and explicit-source rules, corruption checks and eviction. A permanent daemon is not a prerequisite. |
| P2 | EXPLAIN ANALYZE reports scan calls/rows, work units and charged bytes. Charged bytes are cumulative accounting, not peak memory. | Add stage timings and actual data-path counters: definition preparation, source/index lookup, download, decode, query, export, cache hits and bytes. Distinguish measured values from estimates and expose budget failures with enough context to choose a narrower query. |
| P2 | Agents have schema discovery and SQL entry points, but still compose investigation-specific queries and interpret coverage manually. | Add versioned, schema-checked parameterized recipes for common investigations and structured diagnostics for unknown fields or missing preparation. Recipes use existing SQL and fixed references; avoid a second domain query engine. Report empty-but-complete separately from empty-with-unavailable-source data. |

Suggested performance cases: one ID from a large table; a selective multi-table
spell join; ORDER BY with a small LIMIT; repeated cold/warm queries; and an
aggregate over a partial table. Record elapsed time, scan/decode counts and
memory with exact input identities. Every optimization must also retain the
existing SQL correctness and evidence acceptance gates described above.

## Recommended implementation order

Apply the preservation constraints above throughout every step.

1. Preserve encrypted IDs and make full-scan coverage a CLI responsibility.
2. Add pinned enum/flags metadata with raw-value preservation.
3. Design and implement explicit effective snapshots and before/after queries.
4. Grow differential regression alongside each parser or merge change.
5. Add sample ingestion and historical formats only for concrete use cases.

The recent six-table Hotfix investigation covered 4,232 physical cache records:
3,933 decoded records and 299 records without payload, over 25 pages. That
resolved the first-page coverage gap for that capture. It did not create an
effective static-plus-Hotfix table or eliminate the missing-key DB2 sections.
