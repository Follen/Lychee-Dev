# Hotfix records and effective values

Collect Hotfix records from the requested source using installed help. Local
cache reads archive the original bytes and verify the build against the pin;
retain `source.id`, derived snapshot and page captures. Contextual product/region/
locale are not authenticated by the cache alone. Physical records can repeat,
contain signed pushes, unknown status or empty payloads. Empty/invalid payloads
are not zero-filled decoded rows. Latest selects the largest matching push batch,
including ties/invalidations; it is not the last row per ID.

To query effective values, use the schema's explicit effective-table request with
ordered Hotfix captures at the same Data pin. Each input is the complete,
untruncated `hotfix-cache` CAP returned as `source.id`, not a records-page or scan
summary CAP. Compare with static values at that pin; collecting a cache alone
does not change static queries.

Local continuation reads the archived source with unchanged filters/nextIndex;
bounded scans instead resume their saved checkpoint. Do not reread a mutable cache
or substitute an index for a scan cursor. Remote cursors bind the full original
query, including search. Changing/removing search or any other filter starts a
new query without the old cursor and restarts its coverage accounting.

Wago and Raidbots are separate providers with distinct bounds; use neither as an
implicit fallback. Search selects candidates, not confirmed facts. Read
`fullQueryComplete` as well as page completion. Only complete coverage of the exact
scope supports a negative finding, and even complete cache/provider coverage is
not every server Hotfix.
