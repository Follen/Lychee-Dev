# Data target preparation

Read when no matching fixed data pin exists, or when comparing local and remote delivery identities.

Start from the requested product and business entity, not a guessed archive path.
Reuse the invoking project's fixed data pin when it matches the requested scope.
`target list` lists saved named targets, not supported products or available data;
an empty list does not mean Forever is unsupported. `target available --region
<region> --format json` discovers advertised releases. Inspect each product's
rows and error separately; a successful listing can contain a failed product.
`totalRows` counts matching verified rows before the output bound; `truncated`
means the displayed list is incomplete. A reused-slot product/build mismatch
requires updating verified product metadata, not treating another beta as Forever.
Use region/locale from the request or established project context, not the language
of the conversation. The CLI maps `--product forever` to its publication endpoint;
do not substitute the endpoint name for the product. Resolve a matching target
when no fixed data pin exists. Ask only for identity choices still ambiguous.

Prepare a local target from a game root or selected client directory. When the
request names a product, pass it as an identity constraint; do not construct
CASC keys or a DataPin by hand:

```text
lycheedev target resolve --installation <game-root-or-client> --product <product> --region <region> --locale <locale> --format json
```

When a named target has already been configured, resolve that fixed
configuration with `lycheedev target resolve --target <name> --format json`.
Use `lycheedev target show <name>` to inspect the named configuration, or
`lycheedev target show <PIN-id>` to inspect a resolved selection. Showing a
target reads it; it does not resolve or refresh it. Use `target add` to create
or explicitly replace a named configuration.

Region and locale are explicit choices; use those requested for the task.
The CLI reads product/build/config identities from the installation and resolves
WoWDBDefs `refs/heads/master` once to an exact commit. Use `--definitions` for a
specific 40-hex commit or full branch/tag ref. `--offline` resolves a symbolic
ref only from its existing workspace record; an explicit commit needs no
network. Table definitions are prepared on first query, not by target resolve.

The result is a fixed pin ID. Carry it between calls and agents. `--from <pin>`
adds data to a source-only pin without modifying the parent; if it already has
data, the existing definition commit is retained by default and conflicting
fixed identities fail. Do not resolve a fixed target again as latest.

The target evidence records the observed client/catalog and archived config
bytes. It does not prove that every table or locale is present, or that the
game is running. `target resolve --file` remains available for already-resolved
identities, but cannot be mixed with preparation flags.

For a remote product, use the same resolver without an installation path:

```text
lycheedev target resolve --product retail --region <region> --locale <locale> --format json
```

It records the selected region's advertised build, verified build/CDN configs,
and exact definition commit. `--build <full-build>` constrains that observation;
it does not search historical builds or fall back to a neighboring one. Online
failures do not silently reuse old catalogs. `--offline` requires a previously
recorded catalog pair and configurations in this workspace; preserve the returned
`context.observedAt` and do not call an offline observation the current server
state. An exact `--definitions` commit alone does not supply remote catalogs.

Remote target preparation does not download game archives or prove content
availability. DB2/SQL/asset queries accept either `--installation <directory>`
or `--cdn` against the same pin; do not combine them. In target resolution,
`--installation` chooses local selection; its optional `--product` filters observed
identity and cannot override it. Without installation, `--product` selects remote
preparation. `--file` and `--target` remain separate selection modes.

A game root enumerates existing immediate client directories, not catalog rows
alone. One verified match is selected directly; multiple matches return
`context.installationSelection` with candidates. Choose the intended client path
when product alone is insufficient. Metadata conflicts are listed as issues;
do not edit flavor/version files or discard conflicting evidence to force selection.
The explicit client path never selects a sibling. Local missing/mismatched clients
never switch to remote preparation. CLI errors distinguish missing installation,
ambiguous selection, product mismatch and conflicting metadata.

A locally resolved pin retains the installation's CDNConfig; selecting region
`us` does not replace it with the US service's configuration. If CDN preparation
or archive lookup fails, compare with a separately resolved remote target using
the **same product, region, locale, full build and definition commit**. Preserve
the original project lock. Compare both BuildConfig and CDNConfig: even an
identical BuildConfig may be delivered by a different archive inventory. Use
the separate remote snapshot explicitly and label its provenance; do not mutate
the original pin or pass off the two configurations as identical.

The selected product/region's version-service `cdns` manifest is the authority
for CDN hosts and paths. The CLI refreshes a stale delivery route once on
availability failure without changing the pin or consulting latest versions.
Do not hand-build alternate CDN URLs. Offline queries never refresh routes.
