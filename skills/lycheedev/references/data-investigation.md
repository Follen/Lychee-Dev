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
- Multiple tables or calculations: use parameterized SQL; see
  [tables](data-tables.md) and [query recipes](data-query-recipes.md).

Use installed describe/help for commands and flags. A negative claim needs
complete readable coverage of its exact scope. Empty partial results, missing
keys/partitions and truncated searches cannot establish absence. SQL LIMIT or
one record does not establish whole-table coverage. JSONL completion requires a
legal end frame and exit 0, followed by a separate coverage check.

Continue returned cursors with unchanged pin/source/filters and the correct
cursor type. DB2 search/foreign-key have no cursor; narrow, use a supported bound,
or use stable unique SQL keyset paging. Never invent an after-ID option for them.
For Hotfix continuation, use the archived source/checkpoint rather than reread a
mutable file. Healthy workspace/target resolution does not prove content exists.

On failure, use [data recovery](data-recovery.md). Finish with the answer, pin,
source, record/filter scope and captures; retain unresolved server/runtime claims.
