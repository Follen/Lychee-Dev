# Static tables and SQL

Read for schema, names, relationships, paging and SQL. Installed help owns syntax,
input modes and flags; inspect actual schema before choosing fields.

A name search may match NPC abilities, ranks or translated aliases. Resolve it
with the requested class/rank and relationships. Prefer a domain command for the
relationship it owns; use DB2/SQL for other supported tables. Field names and DBD
annotations are evidence, not proof of every inferred meaning.

Direct pages use ascending logical IDs, including copies and zero. Keep the
returned cursor, pin and table; the final suffix is not the whole table. Search
and foreign-key do not share that paging interface. A missing single record is
an error, not a successful empty table. Do not build an agent-side decoded cache
or bypass source verification to accelerate paging.

Missing encryption keys can leave readable partitions. Preserve complete/partial,
unavailablePartitions and source partialContent/missing ranges. Placeholder bytes
are not decoded fields; rowCount can describe only readable logical rows.
Changing local/CDN source does not supply a missing key. A partial aggregate,
CSV or zero-row query is not a full-build answer. Raw asset export requires a
complete verified file.

## SQL interpretation

Use named parameters for user values. SQL LIMIT controls rows; the command has
no CLI limit flag. Array fields are quoted scalar columns such as `"Field[0]"`.
Keep returned ordered columns/positional rows and exact integers; do not round
64-bit IDs/masks through an imprecise representation.

Unqualified and static tables keep pinned base values. Metadata names/flags live
in meta.Table; an effective.Table overlay requires explicitly selected raw cache
captures. See [query recipes](data-query-recipes.md) for those request shapes.
Query completion covers its requested filters/LIMIT, not unrestricted table
coverage. Source partial state also applies to CSV and JSONL exports.

EXPLAIN binds without scanning rows but can still prepare metadata/table bytes;
EXPLAIN ANALYZE executes. Decoded/downloaded bytes, logical charges and peak RSS
are different measures. LIMIT/projection need not reduce shared preparation or
unused-field validation. Prefer one bounded relationship query over simultaneous
queries intended to force more downloads; the CLI owns workspace admission.

Read returned memoryBudget and structured limits. Adaptive Windows memory limits
use available headroom and concurrent-query capacity after admission; they are
not allocated eagerly. Per-file byte limits and timeout do not raise the shared
resource ceiling. For missing content, pressure, budget or SQL faults, use
[data recovery](data-recovery.md) without changing the pin.
