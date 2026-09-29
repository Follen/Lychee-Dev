# Hotfix records and continuation

Read for change records, local cache scans or remote Hotfix coverage. For an
explicit effective overlay, use [data-query-recipes.md](data-query-recipes.md).

## Select and archive the source

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

For named fields, add `--table <name>` and optionally `--record <record-id>`:

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

## Bounded scans

For a complete local Hotfix investigation, prefer the CLI-owned bounded scan:

```text
lycheedev data hotfix --source dbcache --scan --snapshot <pin> --from <raw-cache-capture> --table SpellMisc --limit 200 --max-pages 100 --format json
```

The result contains cumulative matched/returned/decoded/no-payload counts and
page capture IDs. If `result.complete` is false, continue with `--cursor` set to
the returned `resume` capture, the returned snapshot, the same raw cache and the
same table/filters/limit. Read page captures for records; do not treat the summary
as their decoded contents. A completed scan covers only that selected cache.
It does not imply an effective overlay or full server coverage. Resume checkpoints
are saved after each successful page; a completed checkpoint can be reread.
If a scan fails after saving pages, its error response retains the last saved
scan result and `resume`. Report the failure and that checkpoint together;
resume after addressing the cause instead of restarting from the mutable file.


## Incomplete remote searches

If Wago returns zero candidates while `coverage.complete=false`, the result is
unresolved rather than absence. Continue with the returned `nextCursor` or page
continuation using the same product, build, region, locale, table, record,
status and push identity. If `--search` was the only narrowing filter, remove
that filter for a wider query while keeping the identity and provider fixed.
If the budget is exhausted or coverage remains incomplete, report the result as
inconclusive and preserve the coverage metadata; only a complete scan of the
exact scope can support saying that no record was found.
