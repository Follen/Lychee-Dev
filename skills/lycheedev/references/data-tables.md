# Static tables and SQL

Read for name-to-ID discovery, schema, domain relationships, paging or SQL.

## Find the requested records

For a business name rather than a known ID, first inspect the relevant name table's
schema, then search its actual localized field. For example, after pinning the
requested Forever build with enUS data:

```text
lycheedev data db2 schema SpellName --snapshot <pin> --cdn --format json
lycheedev data db2 search SpellName --snapshot <pin> --cdn --field Name_lang --query Claw --limit 50 --format json
```

Use `--installation` instead of `--cdn` for the selected local source. A substring
search can match other abilities, NPC spells and multiple ranks; never choose the
first row merely because its name matches. Inspect candidate relationships and
the requested class/rank context. A translated name is a search hypothesis, not
proof of identity. For a verified spell ID, use `data spell info` for its related
spell closure or `data db2 foreign-key SpellEffect --field SpellID --value <id>
--limit 50` with the same snapshot/source to inspect effects. Read the actual
schema before naming coefficient fields. Retain truncation and missing-section
coverage; a zero-result partial search cannot establish absence.

For a known table and record ID in an explicitly selected local installation:

```text
lycheedev data db2 --snapshot <pin> --installation <game-root> --table <name> --id <id> --format json
```

For the same query from CDN, replace the installation selector with `--cdn`.
Add `--offline` to prohibit all network access and reuse only this workspace's
verified configuration, fragment and definition caches. Missing cache data fails;
the command does not fall back to a local installation or change the pin.
The initial archive lookup can be slow when the provider lacks a group index.
Static `data` queries have one total time budget across preparation, downloads,
recovery and execution: 300 seconds by default. Use `--timeout-seconds <1-3600>`
to set it explicitly. A timeout returns `command.deadline` (exit 5), not an empty
successful result. Verified cached fragments remain reusable on a later query;
retry retains the same pin. This option does not apply to `data hotfix`, whose
scan/request budgets and saved checkpoints are documented in [data-hotfix.md](data-hotfix.md).

`game-root` contains `.build.info` and `Data/`; the same selected client directory
used in target resolution is also accepted and checked against the pin. The command selects the DB2
FileDataID from the pinned WoWDBDefs manifest, verifies the table hash in the
actual file, and binds its exact layout hash to the definition. It returns
`row`, `schema`, `layoutHash`, raw-source blob references and a JSON capture.
The capture concerns that requested record; inspect its coverage fields before
claiming completion. It does not establish whole-table coverage.
Record ID zero is allowed. A missing record is an error, not an empty full-table
query. Field verification/foreign-key metadata in `schema` is retained evidence,
not proof that every inferred DBD field meaning is correct.

Omit `--id` to read a bounded page (default 50, maximum 200 rows):

```text
lycheedev data db2 --snapshot <pin> --installation <game-root> --table <name> --limit 50 --format json
lycheedev data db2 --snapshot <same-pin> --installation <same-root> --table <same-name> --limit 50 --after-id <page.next> --offline --format json
```

Pages use ascending logical IDs, including copies. Omitted `--after-id` includes
ID zero; an explicit cursor excludes that ID. Continue only when `page.more`
is true, carrying `page.next` with the same snapshot and table. `--id` cannot
be combined with `--limit` or `--after-id`. Empty pages are valid. Preserve
each page's capture and cursor; a suffix ending at the last record is still not
the whole table. Captures mark full-table completion only when a first page
contains all rows. `truncated` denotes more rows after this page; false does
not imply that earlier rows were included.

On a cache miss, the command reads only manifest/DBD files at the DataPin's
exact definition commit from WoWDBDefs; it does not fetch a newer branch. Local
mode never fetches CDN game data. Add `--offline` to prohibit downloads; missing or corrupt
cached definitions fail explicitly. Cache bytes are rehashed before use. No
game input or game installation writes occur. Hotfix records are not overlaid.

The file limit is `--max-bytes` (default 128 MiB, maximum 512 MiB, applied to
both encoded and decoded file sizes). Parser budgets additionally bound metadata
to 64 MiB, indexed physical/copy rows to one million, columns/partitions to 4096,
and decoded row text to 1 MiB. A page's serialized row array is capped at 8 MiB;
any row failure or budget overflow returns no partial page. Page selection uses
bounded extra memory, but each CLI invocation currently reopens the table and
checks its selected source. Within one query, the CLI reuses verified table
views and storage metadata under the same fixed source; separate invocations
revalidate cached bytes and source identity. Do not build an agent-side decoded
table cache or bypass verification to make pagination faster.
Encrypted BLTE chunks use a pinned public TACT key
snapshot, fetched only when needed and cached with a verified digest. Offline
uses only cached keys. Supply `--key-file <WoW.txt|keys.json>` to use an explicit
text or JSON key set instead; private keys stay in request memory.

DB2 reads keep readable encrypted/non-encrypted sections when other sections
lack keys. Inspect `complete`, `partial`, `unavailablePartitions` (direct DB2)
or `tables[].unavailablePartitions` (schema/domain results). File provenance
separates `partialContent` plus missing chunk ranges from full CKey-verified
`content`; placeholder bytes never become decoded fields. A schema's `rowCount`
is the readable logical count, not the total number of records in a partial table.
Report missing keys and row coverage with the answer. Do not describe a partial
search, empty result, SQL aggregate or CSV as a full-build answer. Changing from
installation to CDN does not supply a missing key. Raw asset exports still
require the complete verified file.

## Implemented static SQL

Prefer a single bounded SQL query for a relationship that needs several tables;
its CLI-owned session shares preparation without changing pin or source. Avoid
issuing many simultaneous queries to force parallel downloads. Heavy queries
share workspace admission (at most two ordinary queries; one when download
workers is configured to 1), and waiting can consume the command's deadline.

For parameterized lookups, flags, explicit Hotfix overlays and query diagnostics,
read [data-query-recipes.md](data-query-recipes.md). The `meta` catalog exposes
pinned enum/flag definitions; `effective` requires explicitly selected raw cache
captures in the query JSON. Unqualified and `static` tables retain static values.

Supply SQL as text, from a `.sql` or `.json` file, or from stdin. Exactly one
input mode is accepted. For JSON requests, use a UTF-8 file (at most 1 MiB):

```json
{"sql":"SELECT ID, Filename FROM ChrClasses WHERE ID=:id","parameters":{"id":1}}
```

```text
lycheedev data sql --snapshot <pin> --installation <game-root> --file <query.json> --format json
lycheedev data sql --snapshot <pin> --cdn --sql 'SELECT ID FROM ChrClasses WHERE ID=:id' --param id=1 --format json
lycheedev data sql --snapshot <pin> --installation <game-root> --stdin --encoding csv --output <new-file.csv> --format json
```

Repeat `--param <name=scalar>` for named SQL parameters; values must be scalar
JSON values. JSON request parameters must exactly match SQL parameters, and
integer tokens retain 64-bit precision. Duplicate or unknown request fields
fail. `--encoding csv` requires `--output <file>` and writes a CSV export with
its capture/manifest. Use SQL LIMIT/OFFSET, not CLI `--limit`. `--offline` and
`--max-bytes` have the same cache and per-file meanings as DB2 reading. Replace
`--installation` with `--cdn` for explicitly selected remote content.

Unqualified tables and qualified `static.Table` read pinned static values;
`static.Table` also bypasses CTE names. A Hotfix overlay is used only when the
query document explicitly names raw cache captures and queries `effective.Table`
as described in [data-query-recipes.md](data-query-recipes.md). There is no
implicit overlay or source fallback. Query output contains `query`,
`result` (ordered `columns` and positional `rows`, or `plan` for EXPLAIN), and
`sources` with each prepared table's provenance. Verify the returned capture
with `evidence verify <capture-id>`. Complete means the requested query result,
including any SQL LIMIT, not an unrestricted whole-table export. Missing-key
sections propagate `complete: false` and `partial: true` to SQL; source tables
retain missing ranges/partitions. CSV captures and manifests also retain
`complete: false`. JSONL end frames report partial coverage explicitly; a legal
end frame and exit 0 alone do not prove all encrypted sections were readable.

Supported paths include joins, grouping, ordinary CTEs, UNION ALL, correlated
expression subqueries (also in GROUP BY and aggregate inputs), and
one-seed/one-member recursive CTEs. Complex recursive forms still fail explicitly.
DB2 array fields become quoted scalar column names such as `"Field[0]"`.
Text functions require text; conversions and integer overflow fail explicitly.
EXPLAIN binds sources without scanning rows; it may still prepare table
bytes and definitions. EXPLAIN ANALYZE executes and reports scan counts/work/
charged bytes; charged bytes are not process peak memory.

Budgets: at most 32 physical tables, 512 MiB cumulative raw table content,
10 million execution work units, 64 MiB execution accounting and 16 MiB JSON
evidence. Budget failures yield no partial query result. Narrow the query when
appropriate to the request; do not change the pinned build or imply full
coverage from a narrower successful query.

These SQL limits sit alongside shared query-resource limits across preparation
and reading: 512 MiB retained/scratch reservation, 128 MiB metadata, 100 million
decode/work units, 4,096 network requests and 1 GiB response-body bytes.
Failed candidates consume work/network budget; a new table does not reset it.
`records.query_resource_budget` (exit 3) identifies this shared limit, distinct
from `query.budget_exceeded` in the SQL executor or `command.deadline`.
The returned resource counts are logical charges, not wire bytes, peak heap or
RSS. `--max-bytes` is a per-file bound, not a knob for raising these shared limits;
narrowing output alone may not reduce source preparation costs.

SQL errors use `error.stage: query` with `query.invalid_syntax` or
`query.unresolved_binding` (exit 2), `query.unsupported_expression` or
`query.budget_exceeded` (exit 3), and type/range/cardinality faults (exit 4).
Syntax faults include `error.location` with byte offset and line/column.
Correct the request or report the limitation; repeating an unchanged query
does not resolve these failures. Source/cache/I/O faults retain their own codes.

Domain commands are implemented for spells (`data spell info`, `data spell auras`,
`data spell summons`), items (`data item get`, `data item models`,
`data item geosets`, `data item textures`), creatures (`data creature display`,
`data creature model`), journal encounters (`data encounter get`), and house
decor (`data decor list`, `data decor get`). Prefer the domain command when it owns
the relationship needed to answer the question; use SQL/DB2 for other supported
tables. A static table result is not a substitute for requested Hotfix data.
Preserve source, table/record, locale, filters, row counts and capture hashes.
