# Source research

Use this branch for pinned API definitions, Lua/XML/TOC implementations,
relationships and source comparisons. Follow the shortest path that answers
the question; the CLI owns Git preparation, mapping and LuaLS lifecycle.

Read the selected CLI's command directory before choosing an advanced route.
The same workflow applies to an installed release and a Rust candidate: use
query/inspect for fixed original evidence; use refs/context, session reuse or
flow only when that directory includes the path and its required flags. A
source guide or candidate guide cannot add a missing capability. Retain useful
static results when navigation is unavailable, and state the remaining gap.

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
| Callers, dependencies or implementation across files | If refs/context are advertised, [source-relations.md](source-relations.md) using returned symbol IDs; otherwise inspect fixed candidate locations and keep binding/coverage unresolved |
| Difference between two revisions | `source diff` on two fixed pins from the same repository; interpretation below |
| Addon load order, syntax or client compatibility | [addon-validation.md](addon-validation.md) |
| Secret value or secure taint | Read original API constraints first; use [source-value-flow.md](source-value-flow.md) only when the required flow profile is available |

For a normal API lookup, the fixed original declaration and its constraints can
be sufficient. Do not require full indexing, relationship traversal or live work
before answering it. Queries prepare their own mapping; `source index` is an
optional warm-up. Use installed `describe --format json` or help for supported
flags, including when source documentation is newer than the installed CLI.

```text
lycheedev source query <term> --snapshot <pin> --mode precise --topic api --limit 10 --format json
lycheedev source inspect --snapshot <pin> --path <returned-path> --line <first-line> --count 30 --format json
```

Choose `api`, `lua`, `xml`, `toc` or `asset` for the question. Broaden with
`exploratory` only when precise results do not answer it. Search hits are candidates,
not proof of a unique definition. Resolve collisions by repository, commit, path
and scope; retain `symbolId` for relations. If evidence still leaves alternatives,
show those choices rather than silently taking the first match.

Use the returned `whyMatch`, `role`, `confidence`, `signature` and exact
`sourceSpan` when available to decide whether a result answers the question.
For an exact API declaration, quote only the relevant contract and inspect more
original lines when its argument or secret-value rules need context. A short
default excerpt is a starting point, not a restriction on reading the source.
Scope and local binding identity matter for aliases and member calls; a matching
string or comment does not establish a call.
For `body_text` or `indexed_text`, `confidence=exact` can mean an exact literal
text match. Read `whyMatch`; this value does not prove lexical or LuaLS binding.

Read `analysis`, `structure`, `syntaxCompilation` and `semantic` as separate
facts. tree-sitter identifies syntax structure; `luac -p` checks Lua 5.1 syntax;
LuaLS definitions/references are semantic observations only when the reported
engine actually ran against the fixed source and client API environment.
Do not describe templates, name matches or static alias inference as LuaLS
results. Preserve parse diagnostics and excluded or unresolved scope.

## Decide whether to continue

Inspect coverage and truncation separately from exit status. An incomplete map or
dynamic edge cannot establish absence. A query's relation list is only a preview;
use refs if callers matter and that command is advertised. A unique exact
declaration may be covered while `result.complete=false` because its attached
relation preview was truncated. Check `matchCoverage` and
`relationPreviewTruncated` separately: definition completeness, candidate-page
completeness and relation completeness are different questions. A declaration
can answer a signature question without establishing all callers.
Continue `nextCursor` only while another page
can change the answer, preserving the command's snapshot, query and budgets.
Query, refs and context cursors are not interchangeable.
Follow the returned next action only when it is listed by the current CLI's
command directory. During the Rust migration, a registered command can still
report a precise unsupported profile or environment; retain that capability
gap and the useful static evidence instead of inventing another call or using
a Go fallback. Candidate tooling is not the installed release's capability.
Changing the term, symbol, environment or budgets begins a new investigation
without the previous cursor; keep its captures separate from the earlier pages.

`source inspect --path` archives the whole file but returns a bounded excerpt.
Report its path, first/last/total lines, exact commit, full-file hash and capture;
capture completion applies to the requested range, not every line of the file.
`result.blob` identifies the original full-file bytes; the CAP blob holds the
inspection result/provenance JSON and can have a different hash and length.
CAP verification establishes that capture's integrity, not LuaLS binding or
runtime behavior. Use first/last/total lines to say which original bytes were
actually read, and expand that range when it matters.

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

Use this section only if the selected directory advertises refs/context with
session reuse and both session status/close paths. Otherwise keep the available
query/inspect workflow; session documentation is not a reason to invoke an
unregistered path.

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
