# SQL investigation recipes

Use these only with actual pinned schemas and the selected local/CDN source.
They are query documents for `data sql`, not another execution engine.

## Parameters and paging

```json
{"sql":"SELECT ID FROM SpellMisc WHERE ID=:id","parameters":{"id":1}}
```

Parameters must exactly match named values and preserve scalar JSON types and
integer precision. For a bounded listing, order by a stable unique key; continue
using the last key with unchanged source/filters. A join with repeated IDs needs
a unique tuple, otherwise ties can be skipped. Aggregates over missing-key tables
describe the readable subset. Direct DB2 lookup distinguishes unavailable IDs
from absence where SQL cannot.

## Pinned flags

```json
{"sql":"SELECT Field,Name,Value,ConditionField,ConditionValue FROM meta.SpellMisc WHERE Name=:name","parameters":{"name":"NEGATIVE_1"}}
```

Use the returned field/mask and row conditions, not remembered constants.
HAS_FLAG tests all mask bits; BIT_AND returns the integer intersection. NULL
remains unknown. Missing metadata, blank names and unknown bits do not prove a
flag unset. Conditional mappings require their condition to match the row.
Metadata does not establish process-memory offsets.

## Explicit Hotfix overlay

```json
{"sql":"SELECT ID FROM effective.SpellMisc WHERE ID=:id","parameters":{"id":1},"hotfix":["<raw-cache-capture>"]}
```

Archive the selected DBCache first. The ordered hotfix array contains raw cache
captures, not page/scan manifests or provider summaries. Up to 16 captures and
128 MiB combined raw bytes share the pinned data context. Merge order is signed
push ascending, then array order, then physical index; keep that order with the
result. Valid payloads replace/add, invalid records delete subject to the
cached-key exception, and unknown statuses fail. Inspect effectiveSources and
__hotfix_source/__hotfix_push/__hotfix_index for actual provenance.

Static queries remain unchanged. Base missing-key coverage stays partial even
if an overlay supplies one row. Query effective membership directly for additions;
a left join starting from static membership cannot find added-only IDs. Include
the joined ID when distinguishing a deletion from a surviving NULL value.
This is reproducible local-cache evidence, not complete server state.

Use EXPLAIN for shape and EXPLAIN ANALYZE for measured execution. Retain timings
and source coverage; logical charges are not peak memory. Correct unknown fields
from schema/error candidates rather than repeat an unchanged query.
