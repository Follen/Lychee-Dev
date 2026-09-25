# Data investigation

Use this workflow for static game tables, read-only SQL, semantic records,
independent change records, and reproducible data comparisons.

## Fix the data identity

Prepare a local target from the selected client directory; do not construct
CASC keys or a DataPin by hand:

```text
lycheedev target resolve --installation <client-directory> --region cn --locale zhCN --format json
```

When a named target has already been configured, resolve that fixed
configuration with `lycheedev target resolve --target <name> --format json`.
Use `lycheedev target show <name>` to inspect the named configuration, or
`lycheedev target show <PIN-id>` to inspect a resolved selection. Showing a
target reads it; it does not resolve or refresh it. Use `target add` to create
or explicitly replace a named configuration.

Region and locale are explicit choices; use those requested for the task.
The CLI reads product/build/config identities from the installation and resolves
WoWDBDefs `refs/heads/master` once to an exact commit. Use `--definitions` for a
specific 40-hex commit or full branch/tag ref. `--offline` resolves a symbolic
ref only from its existing workspace record; an explicit commit needs no
network. Table definitions are prepared on first query, not by target resolve.

The result is a fixed pin ID. Carry it between calls and agents. `--from <pin>`
adds data to a source-only pin without modifying the parent; if it already has
data, the existing definition commit is retained by default and conflicting
fixed identities fail. Do not resolve a fixed target again as latest.

The target evidence records the observed client/catalog and archived config
bytes. It does not prove that every table or locale is present, or that the
game is running. `target resolve --file` remains available for already-resolved
identities, but cannot be mixed with preparation flags.

For a remote product, use the same resolver without an installation path:

```text
lycheedev target resolve --product retail --region cn --locale zhCN --format json
```

It records the selected region's advertised build, verified build/CDN configs,
and exact definition commit. `--build <full-build>` constrains that observation;
it does not search historical builds or fall back to a neighboring one. Online
failures do not silently reuse old catalogs. `--offline` requires a previously
recorded catalog pair and configurations in this workspace; preserve the returned
`context.observedAt` and do not call an offline observation the current server
state. An exact `--definitions` commit alone does not supply remote catalogs.

Remote target preparation does not download game archives or prove content
availability. DB2/SQL/asset queries accept either `--installation <directory>`
or `--cdn` against the same pin; do not combine them. In target resolution,
`--installation`, `--product`, and `--file` are alternative
selection modes, not overrides of each other.

## Implemented DB2 reading

For a known table and record ID in an explicitly selected local installation:

```text
lycheedev data db2 --snapshot <pin> --installation <game-root> --table <name> --id <id> --format json
```

For the same query from CDN, replace the installation selector with `--cdn`.
Add `--offline` to prohibit all network access and reuse only this workspace's
verified configuration, fragment and definition caches. Missing cache data fails;
the command does not fall back to a local installation or change the pin.
The initial archive lookup can be slow when the provider lacks a group index.

`game-root` contains `.build.info` and `Data/`; the same selected client directory
used in target resolution is also accepted and checked against the pin. The command selects the DB2
FileDataID from the pinned WoWDBDefs manifest, verifies the table hash in the
actual file, and binds its exact layout hash to the definition. It returns
`row`, `schema`, `layoutHash`, raw-source blob references and a JSON capture.
The capture is complete for that one requested record, not for the whole table.
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
checks its selected source. Encrypted-file key provisioning is not available
from this command yet.

## Implemented static SQL

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

Only static tables are available. Qualified `static.Table` bypasses CTE
names; no Hotfix overlay or implicit source fallback occurs. Query output contains `query`,
`result` (ordered `columns` and positional `rows`, or `plan` for EXPLAIN), and
`sources` with each prepared table's provenance. Verify the returned capture
with `evidence verify <capture-id>`. Complete means the requested query result,
including any SQL LIMIT, not an unrestricted whole-table export.

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

## Local Hotfix records

Use `data hotfix` with an explicit `--source` for independent records, not for
an effective static table. Local caches use `--source dbcache`:

```text
lycheedev data hotfix --source dbcache --snapshot <data-pin> --dbcache <DBCache.bin> --limit 50 --format json
```

The command archives the original bytes, checks the cache build number against
the data pin, and returns a derived `snapshot` containing the cache digest and
capture time. Keep `source.id`, that snapshot, and each result capture. It does
not search old tool directories, read SavedVariables, contact a provider, or
change static DB2 queries. Product, locale and region are selected context;
the file alone does not authenticate them. A record's numeric `region`, when
present in its format, is preserved separately.

For named fields, add `--table <name>` and optionally `--id <record-id>`:

```text
lycheedev data hotfix --source dbcache --snapshot <data-pin> --dbcache <DBCache.bin> --table ItemSparse --record <id> --format json
```

The command uses the data pin's exact WoWDBDefs commit and build. It returns
`sources`, the selected `definition`, and each valid payload's `fields` alongside
`payloadHex`. `--offline` prohibits definition downloads. Missing or ambiguous
build definitions, colliding table hashes, invalid UTF-8, trailing/truncated
payload and inline ID mismatch fail; no neighboring build is substituted.
Schema metadata uses lowerCamelCase; record field names preserve DBD spelling.

For raw inspection without definitions, omit `--table` and optionally filter
with `--table-hash <eight hex digits>`. The two selectors cannot be combined.
Obtain hashes from verified metadata, not a guessed table name. Matching
physical records remain independent, including repeated IDs, negative pushes,
unknown statuses and empty payloads. Named-table entries use `decodeState`:
`decoded`, `no_payload`, or `not_valid`; only `decoded` has fields. An empty
or invalidated record is not a decoded row of zero values.

Use `--latest` when the question asks for the largest push batch within the
selected table/record filters. `page.selectedPush` is chosen before pagination;
all records in that batch remain, including ties, repeats and invalidations.
It is neither one latest row per ID nor the last physical entry. Omit it when
the investigation requires all retained pushes.

If `page.truncated` is true, continue the immutable source with the returned
`page.nextIndex` and the same filters:

```text
lycheedev data hotfix --source dbcache --snapshot <derived-pin> --from <source.id> --after-index <page.nextIndex> --limit 50 --format json
```

`--after-index` cannot read a live file: it requires the archived source so a
client cache update cannot shift pagination. Physical order is not push order.
`page.matched` counts all matching records (or the whole selected latest batch),
not only this page. Preserve `--latest` and all table/ID filters when continuing.
An ending suffix is not a complete whole-query result. Even `page.complete`
only describes the selected local-cache query, never all server Hotfixes.

Default input budget is 128 MiB (maximum 512 MiB), with 1–200 returned records
and at most 8 MiB of raw payload per page. Decoded fields have an 8 MiB JSON
page budget; each record allows 1 MiB of text and 65,536 field elements. Corrupt tails invalidate the query
even when the first page would otherwise fit. Version/build/identity errors
must not be worked around by silently changing the selected build or cache.
Wago is supported for remote Hotfix records using explicit product, build,
region and locale context; `--offline` uses its verified cached pages only.
Raidbots is supported as a separate source for a supplied `DBCache.bin` less
than 30 days old. Keep provider and coverage explicit: neither source is an
effective static table overlay, and an incomplete result cannot establish
absence outside its reported coverage.

Treat Wago text search as candidate acceleration only. Establish a fact from
the returned physical records after applying the exact product, full build,
region, locale, table/hash, record, push and status filters relevant to the
question. A zero-candidate page is not proof of absence unless the returned
coverage is complete for that exact scope. Raidbots never acts as an implicit
Wago fallback.

## Schema and change boundaries

Never map a semantic word such as “name”, “model”, or “description” to a field
by guess. Inspect the resolved schema or use a domain command that owns that
relationship, then select the returned fields. Use named query parameters for
user values rather than string interpolation.

Hotfix/change records have separate semantics from static DB2 data. Do not use
a static query to imply a change record, or silently merge a change into a
static result. Preserve provider, build, filters, page/coverage, raw status,
ordering, and whether the result is complete. A zero candidate page or an empty
success is not by itself proof of absence when coverage is incomplete.

For schema, cache or preparation errors, investigate the cause while retaining
the same explicit identity. Never switch product, region, build or locale just
to make a query return rows.

If Wago returns zero candidates while `coverage.complete=false`, the result is
unresolved rather than absence. Continue with the returned `nextCursor` or page
continuation using the same product, build, region, locale, table, record,
status and push identity. If `--search` was the only narrowing filter, remove
that filter for a wider query while keeping the identity and provider fixed.
If the budget is exhausted or coverage remains incomplete, report the result as
inconclusive and preserve the coverage metadata; only a complete scan of the
exact scope can support saying that no record was found.

Product aliases and region/locale availability belong to the CLI resolver.
Carry the canonical values it returns; do not infer a product from the install
folder, a CDN slot, or an old tool's alias table.
