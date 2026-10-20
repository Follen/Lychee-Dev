# Hotfix records and continuation

Read for local caches or remote Hotfix records. Effective table overlays are
explicit [typed requests](data-query-recipes.md), never an implicit consequence of
reading Hotfixes. Use installed help for each source's selectors and bounds.

## Preserve the source

Local DBCache reads archive original bytes, verify the build against the data
pin and return a derived snapshot. Retain source.id, snapshot and captures.
Product/region/locale are selected context; the cache alone does not authenticate
them. Named decoding uses the exact pinned definition, not a neighboring build.
Raw physical records include repeats, signed pushes, unknown statuses and empty
payloads. no_payload/not_valid are not zero-filled decoded rows.

Latest selects the largest push batch within filters, including ties and
invalidations; it is not one latest row per ID or the last physical record.
Physical order is not push order. A completed local cache is not all server
Hotfixes. Corrupt tails are errors even if a first page would fit.

Wago and Raidbots remain distinct sources. Remote records need explicit product,
full build, region and locale even with a parent pin. Raidbots uses a supplied
cache less than 30 days old; it is not an implicit Wago fallback. Wago text search
only accelerates candidate selection. Establish facts from exact record filters
and reported coverage, not a search hit/miss alone. Provider time bounds and
local raw-cache selectors do not have interchangeable meanings.

## Continue without losing coverage

Local page continuation uses the archived source and returned nextIndex with
unchanged filters; it cannot safely reread a mutable cache. A bounded scan uses
its resume checkpoint instead, retaining cumulative counts and page captures.
Read those page captures for records; a summary is not their decoded contents.
If a scan fails after saving pages, retain its failure and checkpoint and resume
after the cause changes. Do not substitute a last-record index for a scan cursor.

Remote cursors belong to their provider and exact query identity. Preserve
product/build/region/locale/table/record/status/push when continuing. If search
was the only narrowing filter, a wider search can omit it without changing those
identities. An incomplete zero-candidate page is unresolved; continue only when
coverage/budget permits a useful answer, otherwise retain the gap.

Read fullQueryComplete, not only complete: a window or final suffix can finish
without covering the whole query. Local pages, scan checkpoints and remote
cursors are different. Only complete coverage of the exact scope can support a
negative finding, and complete cache/provider coverage is still not proof of
all server changes.
