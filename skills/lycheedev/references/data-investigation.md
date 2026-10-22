# Data investigation

Use static records for IDs, names and relationships, [Hotfix records](data-hotfix.md)
for retained changes, and explicitly selected effective overlays for changed
values. None proves server behavior. Do not open a game for a static-only task.

Reuse the requested snapshot or matching project pin. Preserve product, full
build, region, locale, definitions and local/CDN provenance. Do not silently
rewrite a conflicting pin. See [targets](data-targets.md) for missing identity.

Start with local task search, exact command schema and leaf help. Open one
caller-owned `data agent serve` process and use its typed tasks. Catalog, fields,
semantics, query and the twelve navigate kinds are the formal Data entry path.
Business names require discovery in the actual localized field and disambiguation
by class/rank/relationships, not the first match. Inspect [tables](data-tables.md)
and [recipes](data-query-recipes.md). Use `data agent schema` to discover the
available task inputs and results.

Request IDs increase strictly. Cancellation of outstanding work or a deadline
terminates the isolated worker and invalidates every session handle, including
earlier results. Never automatically replay uncertain work. Close or EOF drains
submitted requests before release. Incomplete frames are not results. Abnormal
exit invalidates the session even if stderr backpressure prevents the best-effort
diagnostic. Data agent sessions require Windows amd64; schema and help are
available without starting a session.

Ordinary command JSONL has its own end frame and successful exit. Agent JSONL
requires a complete matching-ID response and has no ordinary begin/end wrapper.
Normal close and process exit are distinct from result coverage. Empty partial
results, missing keys, truncated searches and bounded queries cannot establish
unrestricted absence. Page offsets belong to one retained result; Hotfix cursors
belong to archived source/checkpoint, not a reread mutable file.

Finish with the answer, pin, source, record/filter scope and committed captures.
Retain unresolved runtime/server questions and explicit unsupported combinations.
Use [recovery](data-recovery.md) on failure rather than changing pins or raising
limits to hide the original fault.
