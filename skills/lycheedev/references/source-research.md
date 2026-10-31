# Source research

Use pinned definitions, Lua/XML/TOC and relationships to answer the question.
The CLI owns Git preparation, source mapping and analysis worker/session lifetime.
Structural mapping, Lua 5.1 compilation and semantic analysis provide different
evidence. Preserve the returned source/environment identities, locations,
captures and coverage. Never execute studied Lua or repository configuration.

## Route source evidence

Use LycheeDev by default. Reuse matching pins and retained locations; a fixed
original declaration and its constraints can already answer an API question.
Use `source list` / `source sync` only when selection is missing; see
[source preparation](source-preparation.md). Query or inspect the relevant source;
add [relationships](source-relations.md) only when callers or context matter.
No full-index or live prerequisite is imposed on a simple lookup.

A miss calls for checking topic, terms, selected revision and coverage. Broaden
within the fixed source when useful. Use web/GitHub only for an actual missing
source/version/capability or an explicit external-lookup request. State that gap
or purpose and distinguish external provenance. Subsequent reads use verified or
tool-returned paths/URLs. Never guess GitHub raw URLs, file paths or release tags,
or silently replace a pin with latest.

## Choose evidence

Inspect an exact returned file/location; query an API/event/name; use refs/context
for relationships, `source diff` for two fixed revisions of the same repository,
or [validation](addon-validation.md) for load closure. For secret values or taint,
read [value flow](source-value-flow.md). Use installed help for syntax.

Search hits are candidates. Resolve name collisions by repository, commit, path
and scope; keep returned symbol IDs. A data pin is not a source reference. An
addon track and its client API environment are separate pins; neither a folder
nor an installed file proves the version loaded at runtime.

Coverage and truncation survive a successful exit. Relation previews are not all
callers. Continue only pages that can change the answer, with the original scope
and cursor type. A changed term, symbol, environment or budget starts a new page
set. A complete file capture does not mean the returned excerpt included every
line. Diff truncation and line movement do not prove removed or broken APIs.

## Several questions on one pin

`source batch` shares preparation. Its file holds 1–32 items, at most 64 KiB:

```json
{"requests":[{"id":"definition","query":{"text":"C_Spell.GetSpellInfo","topic":"api","limit":10}}]}
```

Each item contains exactly one query, refs or context object; an optional ID
labels the item. Fields use camelCase (`symbolId`, `maxBytes`, `maxLines`); each
item owns its cursor and coverage. All share source/environment. An item failure
is not a completed batch.
Use `lycheedev schema source batch` and its `fileInput` for the complete typed
file schema, defaults and bounds. Unknown or repeated fields and malformed
Unicode are rejected.

## Optional native session reuse

One-shot analysis is the default. Opt into session reuse only for repeated
semantic cache misses on the same source/environment. Static queries and result
cache hits do not require a broker. Inspect or close sessions through the CLI;
do not kill another owner or override leases. Reuse cannot strengthen coverage,
move cursors between scopes or renew deadlines.

For preparation failures, follow [bounded recovery](source-preparation.md#recover-without-changing-the-revision).
Static evidence does not prove the client loaded it or behaved that way. If live
work is already in scope, carry the unresolved conditions into a bounded
[hypothesis check](live-probes.md#test-source-hypotheses) and reconcile both results.
