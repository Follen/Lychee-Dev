# Typed tables and metadata

Use `lycheedev cli search "data"`, exact `schema data agent serve`, then
`data agent schema`. Metadata discovery does not open a workspace or worker.
Installed schemas are authoritative; do not invent a SQL/DB2 command outside
the declared Data tasks.

Within one pinned `data agent serve` session, catalog lists the exact definition
manifest, not installed availability. Fields/discover resolve authenticated DBD
schema and physical layout. Preserve sourceDefinition relationships and types,
sourceLayout, sourceReading and sourceVerification. Arrow field names alone do
not prove a foreign-key relationship. Semantics returns the original DBD flags,
enums and condition metadata for static tables with no Hotfix captures; it does
not execute a DB2 row query or imply that every condition applies to every row.

Query accepts typed plans. Scan/lookup/filter/project/join/aggregate/sort/limit,
union_all, explicit bounded iterate/delta, and advertised query-valued expressions
run in the unified engine. Nonrecursive CTEs can be represented by nested plans.
This is not SQL syntax compatibility or an unrestricted recursive interpreter.
Use only schema-supported combinations; dynamic query-valued CASE/COALESCE and
correlated limits/order/union have explicit unsupported boundaries. Do not use a
specific Spell/Encounter navigator as proof of arbitrary recursive-query support.

Keep ordered columns, positional rows, nested arrays and exact 64-bit integers.
An array is a typed nested value; indexing is an explicit expression, not a
quoted `Field[0]` SQL identifier. Use a suitable exact-number decoder, never
round IDs or masks through JavaScript Number. Filter user values as typed literals.

Query returns an owner-backed handle. Page and JSON/CSV export use that handle,
not a rerun or a mutable source cursor. Export returns bounded text/rows, not an
arbitrary filesystem write. Release unused handles. Expiry, close, worker exit,
cancellation and deadline invalidate the appropriate session ownership; never
reuse a handle in another process. Repeated source access may reuse authenticated
Arrow/ID/session caches; cache reports are observations, not permission to skip
verification. Finite-demand paths must not force a full-table cache build.

Missing keys preserve readable partitions, missing ranges and incomplete
coverage. Placeholder bytes are not fields. Empty results, partial aggregates,
CSV and a limited result cannot establish full-build absence. Raw asset export
requires complete verified bytes. Complete means the requested scope, not every
possible row in the build.

Explain without analyze expands the native plan without running its rows, though
metadata/source preparation may still be required. Analyze executes once. Read
analysisScope: prerequisite materialization may precede root native metrics.
Request elapsed time and admitted-work ledgers cover the stated request stages;
root metrics are not automatically all-node metrics. Ledger bytes are not RSS,
OS peak or complete native allocator accounting. Formal Windows worker protection
is external to the Engine's logical limits. See [recovery](data-recovery.md).
