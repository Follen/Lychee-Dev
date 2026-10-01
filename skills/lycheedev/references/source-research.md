# Source research

Use this branch for pinned API definitions, Lua/XML/TOC implementations,
relationships and source comparisons. Follow the shortest path that answers
the question; the CLI owns Git preparation, mapping and LuaLS lifecycle.

## Fix the source

Reuse an explicit snapshot or the invoking project's matching source pin. Check
repository, exact commit and requested revision with `target show <pin>`; a data
pin alone is not a source reference. Do not let a conflicting project default
retarget the question or overwrite its lock merely to investigate another source.
Without a matching pin, read [source-preparation.md](source-preparation.md).

An addon release, its repository track and a game client's build are separate
identities. `source sync --product` selects a track advertised for that repository,
not necessarily a game product. A third-party addon can use `main` while its API
environment comes from a separately pinned Retail or Forever source.

## Choose the next evidence

| Question / known input | Action |
| --- | --- |
| Exact file and line | `source inspect --path` with a bounded line range |
| API, event, template or feature name | `source query` with the relevant topic, then inspect the returned original location |
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


## Optional LuaLS session reuse

`source refs` and `source context` keep one-shot semantic execution as the
default behavior. Add `--session-reuse` only when several semantic cache misses
will investigate the same fixed source/environment. Complete result-cache hits,
static-only queries and ordinary source searches do not start a broker.

The optional broker retains one verified worktree lease and one guarded LuaLS
worker for that home. Identity changes rebuild the worker; an active cancelled
or timed-out query retires it. Requests keep the original deadline while queued,
initializing and querying. A busy or incompatible owner fails closed; do not
kill a PID or override its lease to recover.

Use `lycheedev source session status --home <workspace>` to inspect without
starting it, and `lycheedev source session close --home <workspace>` to release
it before explicit source reclamation. Add the same `--release <release-root>`
when the investigation uses a separately verified release. The broker also exits
after 60 seconds idle, ten minutes lifetime, or 256 semantic RPC batches.
`--static-only` cannot be combined with `--session-reuse` or `--release`.

Reuse remains opt-in. These bounds do not claim measured full-source RSS or
latency; retain the result's coverage and partial reasons. Session failure does
not establish absence of references or calls.
