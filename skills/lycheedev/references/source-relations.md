# Source relationships and bounded context

Read when callers, cross-file implementation, loading context or semantic
resolution can change the answer. Simple declaration lookup can stop earlier.

## Select a symbol, then follow its evidence

Use `symbolId` from a pinned query. `source inspect --symbol <qualified-name>`
finds candidates; `--target-path <path>` inspects file/asset metadata. Same spelling
does not join declarations across scopes, files or repositories. A bare `--symbol`
is suitable only when unambiguous; otherwise inspect candidates and choose by
their original locations and the question.

```text
lycheedev source refs --snapshot <pin> --symbol-id <id> --direction incoming --limit 50 --format json
lycheedev source context --snapshot <pin> --symbol-id <id> --limit 50 --max-lines 120 --format json
```

`refs` reports incoming/outgoing/both relations. `context` combines the definition,
bounded source excerpts, relations and available loading evidence. Retain relation
state, original locations and coverage: confirmed semantic resolution, static
candidates and unknown edges have different evidentiary strength. Generated API
declarations describe a contract; they are not necessarily implementation bodies.

The CLI attempts its verified bundled LuaLS when needed. For a third-party addon,
add `--environment <client-pin>` when client API context matters. Preserve both
source and environment identities; a Retail environment is not a substitute for
Forever. `--static-only` deliberately limits evidence to structural analysis.
Missing runtime/environment can leave usable source evidence with semantic
coverage partial. Neither static inference nor LuaLS types prove runtime behavior.

Inspect `environmentManifest` and coverage to confirm which client definitions
were actually used. When the research source itself is `wow-ui-source`, that
pin supplies its own environment; `--environment` supplies the separate client
pin for third-party source, rather than retargeting a Blizzard source pin.

## Continue only the required scope

Query results contain a bounded relation preview. Use refs, not repeated query
pages, to investigate its edges. Each command owns its own `nextCursor`; retain
snapshot, symbol/query, environment, analysis options and budgets on continuation.
An ending page does not erase missing earlier pages or unresolved dynamic edges.
To change direction, depth, flow mode or excerpt budgets, start without the old
cursor. Add `--session-reuse` only for repeated semantic work as described in
[source-research.md](source-research.md#optional-luals-session-reuse); it does not
make a cursor portable or strengthen a relation's evidence state.

For context, `--max-lines`, `--max-bytes`, `--limit` and `--depth` bound different
parts of the result; inspect its truncation and continuation metadata. Follow a
cursor or request the relevant original-file range when the omitted material
matters. Do not increase every bound just to make an incomplete flag disappear.

Mapping completeness describes syntax extraction, not a validated load closure.
When actual TOC/XML order or multi-client compatibility matters, use
[addon-validation.md](addon-validation.md). Keep dynamic/conditional loading
unresolved when source cannot determine it.

For a concrete value/operation question, use
[source-value-flow.md](source-value-flow.md); ordinary relation traversal alone
does not establish a secret-value propagation path.

For a relation crossing repositories, use its target repository, commit and
original location to obtain the matching fixed source before following it.
An environment declaration or same-named symbol is not evidence that an addon
dependency was loaded. Leave missing dependency and runtime binding edges open.
