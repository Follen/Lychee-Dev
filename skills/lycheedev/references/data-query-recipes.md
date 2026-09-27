# SQL investigation recipes

Use these with the existing `data sql` command, a fixed snapshot and an explicit
installation or CDN source. Inspect the selected table's schema before adapting
fields or relationships. Recipes are query documents, not another execution
engine. Bind values with parameters instead of interpolating user text.

## Find a record

```json
{"sql":"SELECT ID FROM SpellMisc WHERE ID=:id","parameters":{"id":1}}
```

An exact ID equality can use the shared DB2 identity index. Missing-key tables
remain partial even when the query returns one row or no rows. Use direct DB2
lookup when the distinction between an encrypted ID and absence is the question.

## Discover a flag and query it

First find its pinned definition:

```json
{"sql":"SELECT Field,Name,Value,ConditionField,ConditionValue FROM meta.SpellMisc WHERE Name=:name","parameters":{"name":"NEGATIVE_1"}}
```

Then use the returned field and integer mask. For a build whose mapping names
`Attributes[0]`, the query shape is:

```json
{"sql":"SELECT ID,\"Attributes[0]\" FROM SpellMisc WHERE HAS_FLAG(\"Attributes[0]\",:mask) ORDER BY ID LIMIT 50","parameters":{"mask":67108864}}
```

The mask above is an example for that mapping, not a timeless API constant.
`HAS_FLAG(value,mask)` tests all mask bits; `BIT_AND(value,mask)` returns the
exact integer intersection. NULL stays unknown. Raw numeric fields are retained.
An empty metadata lookup means no matching metadata was found; it does not prove
a flag is unset. Blank names and unknown bits remain uninterpreted. Conditional
mappings carry their condition explicitly; apply it to the relevant row before
using that meaning. Metadata does not establish runtime memory offsets.

## Compare static and explicitly overlaid values

Archive the selected DBCache first. Put its returned raw cache capture ID(s) in
the query document's `hotfix` array; a page or scan-manifest capture is not a raw
cache. Use `effective.TableName` explicitly:

```json
{"sql":"SELECT s.ID,s.\"Attributes[0]\" AS before_value,e.\"Attributes[0]\" AS after_value,e.__hotfix_source FROM SpellMisc s LEFT JOIN effective.SpellMisc e ON s.ID=e.ID WHERE s.ID=:id","parameters":{"id":1},"hotfix":["<raw-cache-capture>"]}
```

All selected captures must match the pinned data context. Merge order is signed
push ascending, then the order in `hotfix`, then physical record index. Valid
payloads replace/add; invalid records delete, with the documented cached-key
exception. Unknown statuses are refused. Inspect `effectiveSources` for the
policy and capture identities; overlaid rows also carry `__hotfix_source`,
`__hotfix_push`, and `__hotfix_index`. Static queries keep their original values.
The derived query capture is reproducible local-cache evidence, not proof of
complete server or running-client state. Base missing-key coverage remains
partial conservatively, even if a Hotfix supplies a particular missing row.

## Diagnose expensive or invalid queries

Prefix the query with `EXPLAIN` to bind and inspect its shape without scanning
rows, or `EXPLAIN ANALYZE` to execute and collect scan/lookup/work measurements.
Preparation may still need source metadata. Query `timings` separates definition
preparation, table preparation and execution; decoded bytes are not downloaded
bytes, and charged bytes are not peak process memory. Unknown-column errors
include bounded candidate names. Correct the schema reference before retrying.

Start selective joins with a filtered subquery when useful. Hash equality joins
and bounded Top-K preserve duplicate rows, NULL rules and stable ordering.
Projection reduces materialized fields while retaining unused-field validation.
LIMIT bounds output; it does not promise that all source validation stops early.
