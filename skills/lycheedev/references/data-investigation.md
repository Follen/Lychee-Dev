# Data investigation

Use static records for IDs, names and relationships; [Hotfix records](data-hotfix.md)
for retained changes; an explicit [effective SQL overlay](data-query-recipes.md)
for values after selected caches. None alone proves server behavior. Keep a
static baseline for an authorized live measurement; do not open a game for a
static-only task.

Reuse an explicit snapshot or the project's matching data pin. Inspect product,
full build, region, locale and definitions; a conflicting default must not answer
a different question or prompt a lock rewrite. For missing identity, read
[data targets](data-targets.md). Keep local/CDN provenance fixed and label separate
pins when delivery configurations differ.

Choose the smallest query that can settle the claim:
- Verified ID: use the domain relationship command when it owns that relationship,
  otherwise inspect schema and query the ID/foreign key.
- Business name: search the actual localized schema field, then resolve candidate
  class/rank/relationships. Do not pick the first name or ask for a discoverable ID.
- Repeated or multi-table tasks: inspect `data agent schema` and use a single
  supported `data agent serve` worker with typed lookup/filter/aggregate/join
  plans. Keep the same snapshot and result handle across pages; release handles
  when finished. This worker accepts typed plans only. The older SQL command
  remains a separate migration path, not evidence that all consumers are unified.
- If the installed version does not advertise the Agent worker, use its existing
  supported commands and parameterized SQL; never invent flags or claim session
  reuse. See [tables](data-tables.md) and [query recipes](data-query-recipes.md).

The worker is a bounded, caller-owned process, not an installed daemon. Request
IDs increase strictly. Cancelling outstanding work or exceeding a deadline
terminates the isolated worker and invalidates every session handle, including
results from earlier requests. Never automatically replay an uncertain request.
Close or stdin EOF drains submitted requests and then releases the session;
retain the needed pages/evidence first. Incomplete output frames are not results,
and an abnormal process exit invalidates the session even when a blocked stderr
pipe prevents delivery of the best-effort supervisor diagnostic. Never infer
completion of pending work from missing error output. Check the installed schema for platform memory
protection and configurable limits before opening a worker.

Use installed describe/help for commands and flags. A negative claim needs
complete readable coverage of its exact scope. Empty partial results, missing
keys/partitions and truncated searches cannot establish absence. SQL LIMIT or
one record does not establish whole-table coverage. Ordinary command JSONL
requires a legal end frame and exit 0. Agent JSONL instead requires a complete
response frame with the matching request ID; it has no ordinary begin/end wrapper.
Normal session exit or a close response is separate from each result's coverage.
Inspect coverage independently in both protocols.

Continue returned cursors with unchanged pin/source/filters and the correct
cursor type. DB2 search/foreign-key have no cursor; narrow, use a supported bound,
or use stable unique SQL keyset paging. Never invent an after-ID option for them.
For Hotfix continuation, use the archived source/checkpoint rather than reread a
mutable file. Healthy workspace/target resolution does not prove content exists.

On failure, use [data recovery](data-recovery.md). Finish with the answer, pin,
source, record/filter scope and captures; retain unresolved server/runtime claims.
