# Data target preparation

Read when a matching data pin is missing or local/remote delivery identities differ.
Resolve identity; let the first requested query test content availability. Do not
prepare a whole archive or refresh a matching pin for a narrow lookup.

`target list` lists saved names, not supported products. `target available` lists
advertised releases; inspect per-product errors and truncation even on success.
Use requested/contextual region and locale, not conversation language. Use
`forever-asia-beta` or `forever-cn-beta` for their separate publication endpoints;
legacy `forever` retains the Asia endpoint remotely and is ambiguous locally when
both clients exist. Do not substitute launcher keys for product names. Equal
builds do not make these products' configurations interchangeable.

Use the CLI resolver for a selected local installation, named target or remote
product; discover exact syntax through help. A local product selector constrains
observed identity; it cannot override metadata. A game root enumerates actual
clients, while an explicit client path never selects a sibling. Ask only when
returned candidates leave the task's target ambiguous. Missing/conflicting local
identity does not authorize remote fallback or edits to flavor/version files.
A client folder may share its parent's `.build.info` and `Data/` legitimately.

Keep the returned fixed pin and definition commit. Showing a target reads it;
resolving observes its configured selection. A build constraint does not search
historical builds or choose a neighbor. An offline catalog is a recorded
observation, not current server state. Exact definitions alone do not provide
catalogs or game content. Resolution does not prove tables/locales available
or that a game is running; table preparation happens on demand.

Local and CDN query sources are explicit and do not silently replace each other.
A local pin retains the installation's CDNConfig; choosing a region does not
substitute that region's delivery configuration. To diagnose a CDN mismatch,
resolve a separate remote target with the same product, region, locale, full
build and definitions. Compare both BuildConfig and CDNConfig, label provenance
and leave the original pin/project lock unchanged. The same BuildConfig can have
a different archive inventory.

The selected product/region's version-service cdns manifest owns CDN hosts and
paths. The CLI handles bounded route refresh; do not construct alternate CDN
URLs, change builds or bypass offline restrictions to make a query succeed.
