# Source research

Use this branch for pinned API definitions, Lua/XML/TOC implementations,
relationships and source comparisons. Follow the shortest path that answers
the question; the CLI owns Git preparation, mapping and Emmy native-worker/session lifecycle.

## Route source evidence

Use LycheeDev for WoW source/API investigation by default. Discover missing
repository/track choices with `source list` and fix the requested revision with
`source sync` as described in [source-preparation.md](source-preparation.md).
Use `source query` or `source inspect` for the needed evidence, adding refs/context
only when relationships or context matter. Reuse matching pins, returned locations and
retained evidence; this is not a mandatory command sequence. When that evidence
already answers the question, answer directly without rediscovery, repinning,
repeat verification or extra approval.

Empty or insufficient query results call for checking the mode/topic, search
terms, selected version and coverage, using the existing results where possible.
Correct the relevant mismatch or broaden within the intended fixed source when
useful; a query miss alone does not establish missing Toolkit support or justify
web search.

Use web/GitHub or other external sources only when LycheeDev actually lacks the
needed source, version or capability, or the user explicitly requests external
lookup. State the concrete gap, or the requested external purpose, and distinguish
external evidence and its version from the fixed-source evidence. Do not silently
replace the pin. Subsequent reads use tool-returned or otherwise verified paths
and URLs; never guess GitHub raw URLs or file paths.

## Fix the source

Reuse an explicit snapshot or the invoking project's matching source pin. Use
`target show <pin>` when its repository, exact commit or requested revision is
unclear; a data pin alone is not a source reference. Do not let a conflicting
project default retarget the question or overwrite its lock merely to investigate
another source.
Without a matching pin, read [source-preparation.md](source-preparation.md).

An addon release, its repository track and a game client's build are separate
identities. `source sync --product` selects a track advertised for that repository,
not necessarily a game product. A third-party addon can use `main` while its API
environment comes from a separately pinned Retail or Forever source.

## Choose the next evidence

| Question / known input | Action |
| --- | --- |
| Exact file and line | `source inspect --path` with a bounded line range |
| API, event, template or feature name | `source query` with the relevant topic; inspect the returned original location when more detail is needed |
| Several known questions on the same fixed source | `source batch --file` shares snapshot, document and Emmy analysis preparation; retain each item's coverage and cursor |
| Callers, dependencies or implementation across files | [source-relations.md](source-relations.md), using returned symbol IDs |
| Difference between two revisions | `source diff` on two fixed pins from the same repository; interpretation below |
| Addon load order, syntax or client compatibility | [addon-validation.md](addon-validation.md) |
| Secret value or secure taint | [source-value-flow.md](source-value-flow.md) |

For a normal API lookup, the fixed original declaration and its constraints can
be sufficient. Do not require full indexing, relationship traversal or live work
before answering it. Queries prepare their own mapping; `source index` is an
optional warm-up. Use installed `describe --format json` or help for supported
flags, including when source documentation is newer than the installed CLI.

```text
lycheedev source query <term> --snapshot <pin> --mode precise --topic api --limit 50 --format json
lycheedev source inspect --snapshot <pin> --path <returned-path> --line <first-line> --count 80 --format json
```

Choose `api`, `lua`, `xml`, `toc` or `asset` for the question. Broaden with
`exploratory` only when precise results do not answer it. Search hits are candidates,
not proof of a unique definition. Resolve collisions by repository, commit, path
and scope; retain `symbolId` for relations. If evidence still leaves alternatives,
show those choices rather than silently taking the first match.

## Decide whether to continue

Inspect coverage and truncation separately from exit status. An incomplete map or
dynamic edge cannot establish absence. A query's relation list is only a preview;
use `source refs` if callers matter. Continue `nextCursor` only while another page
can change the answer, preserving the command's snapshot, query and budgets.
Query, refs and context cursors are not interchangeable.
Changing the term, symbol, environment or budgets begins a new investigation
without the previous cursor; keep its captures separate from the earlier pages.

For several known symbols, prepare a JSON file with 1-32 requests (at most
64 KiB), then run:

```text
lycheedev source batch --snapshot <pin> --file requests.json --format json
```

```json
{"requests":[
  {"id":"definition","query":{"text":"C_Spell.GetSpellInfo","topic":"api","limit":10}},
  {"id":"callers","refs":{"symbol":"C_Spell.GetSpellInfo","direction":"incoming","limit":20}},
  {"id":"implementation","context":{"symbol":"C_Spell.GetSpellInfo","depth":1,"limit":10,"maxBytes":16384,"maxLines":120}}
]}
```

Each request contains exactly one `query`, `refs` or `context` object. Their
fields are the corresponding query fields in camelCase; `symbolId` can replace
`symbol`, and `cursor` continues that individual item. All items share the
selected snapshot and optional `--environment`. The default worker ends with
the batch; `--session-reuse` opts into the existing cross-command broker.
Use `--static-only` when only structural evidence is needed. Batch defaults
match the source engine: query limit 10, refs/context limit 25, context depth 0,
context maxBytes 16384 and maxLines 80; specify larger context explicitly.
The batch archives one combined capture. A partial or truncated item remains
partial in the aggregate. An item error fails the command and names its position;
earlier answers are not presented as a completed batch.

`source inspect --path` archives the whole file but returns a bounded excerpt.
Report its path, first/last/total lines, exact commit, full-file hash and capture;
capture completion does not imply that every line was read.

`source diff --from <pin-a> --to <pin-b> --limit 50` compares document/declaration
content and declaration sites, not all assets or version metadata. It has no
cursor: retain category totals/truncation, and inspect relevant files when more
detail is needed. A line move alone does not prove an API break; an incomplete
index is rejected, not interpreted as removed declarations.

For preparation failures, use the bounded recovery in
[source-preparation.md](source-preparation.md#recover-without-changing-the-revision).
Finish with the source-supported answer and unresolved conditions. Static source
does not prove that a client loaded it or behaved that way in combat or another build.
Identify what is established, what could change the answer, and the smallest
next check for that uncertainty. Keep the pin, original locations and captures
with the conclusion so a later data/live step can reuse the evidence.

## Secret values and secure taint

Read [source-value-flow.md](source-value-flow.md) for versioned rules, value identity,
guards and unknown edges. If live investigation is already authorized, carry the
remaining hypothesis into [live investigation](live-investigation.md#test-source-hypotheses).
Source-only work does not authorize game input.


## Optional native session reuse

`source refs` and `source context` keep one-shot semantic execution as the
default behavior. Add `--session-reuse` only when several semantic cache misses
will investigate the same fixed source/environment. Complete result-cache hits,
static-only queries and ordinary source searches do not start a broker.

The native session retains authenticated source/environment analysis under the
selected CLI's ownership. Identity changes must rebuild it; active cancellation
or timeout must retire unusable analysis. Requests keep the original deadline
while queued, initializing and querying. A busy or incompatible owner fails
closed; do not kill a PID or override its lease to recover.

Use `lycheedev source session status --home <workspace>` to inspect without
starting it, and `lycheedev source session close --home <workspace>` to release
it before explicit source reclamation. Preserve the command's selected
`--release <release-root>` where supported; it selects verified release tooling,
not a global LuaLS installation. `--static-only` cannot be combined with
`--session-reuse` or `--release` under the fixed Go command contract.

The required Go-compatible bounds are 60 seconds idle, ten minutes lifetime,
and 256 semantic batches per retained analysis worker. At the batch limit the
worker retires; the broker remains available to prepare the next worker. Idle
time pauses during requests, while the lifetime deadline does not reset.
These are migration requirements, not claims that
the current Emmy session meets them; see [candidate boundaries](source-candidate.md).

Reuse remains opt-in. These bounds do not claim measured full-source RSS or
latency; retain the result's coverage and partial reasons. Session failure does
not establish absence of references or calls.
