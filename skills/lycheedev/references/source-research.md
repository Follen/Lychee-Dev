# Source research

Use this workflow for versioned UI/API definitions, Lua/XML/TOC source, symbol
relationships, source diffs, and compatibility questions.

## Pin and query

Resolve or receive a `SourcePin` containing the source identity, exact commit,
parser/index revision, and any traceable tag. A tag is a label, not the
immutable evidence. Keep the exact commit in the report. If the requested exact
source cannot be resolved, stop with an actionable unresolved result; do not
silently substitute the newest snapshot.

Check `describe --format json` first. The current implementation supports
catalog discovery, explicit source preparation, syntax indexing, topic and
evidence-tier queries, indexed symbol or target-path inspection, pinned file
reads and static addon validation:

```text
lycheedev source list --format json
lycheedev source sync --source <catalog-key> --product <track> --ref <commit-or-full-ref> --format json
lycheedev source index --snapshot <pin> --format json
lycheedev source query <term> --snapshot <pin> --mode precise --topic api --limit 50 --format json
lycheedev source inspect --snapshot <pin> --path <repository-relative-file> --line <first-line> --count <line-count> --format json
lycheedev source inspect --snapshot <pin> --symbol <qualified-name> --format json
lycheedev source inspect --snapshot <pin> --target-path <path> --format json
lycheedev source diff --from <pin-a> --to <pin-b> --limit 50 --format json
lycheedev source validate --matrix <config.json> --format json
```

`source sync` returns the immutable PinnedSet directly. Its `--ref` accepts a
40-character commit or a full `refs/heads/...` / `refs/tags/...` reference; omitting
it selects the catalog's product branch at preparation time. Do not omit it when
the user supplied an exact version. A source query product is not evidence that
the addon supports installation on that client.

`source inspect` reads only the prepared exact commit, without network access.
It archives the **whole original file**, while `result.text` contains only the
requested lines (at most 2,000). The capture being complete does not mean the
excerpt contains the whole file. Preserve `context.snapshot`, the exact commit,
path, first/last/total line counts, full-file hash and capture ID. Git must be
available for preparation and reads; this workflow does not invoke the old CLI.

`source index` analyzes Lua/XML/TOC without executing them. A successful command
can still have `complete: false`: retain the diagnostic count and sample, rather
than treating an unparsed file as empty. Index completeness covers syntax
extraction, not load-closure validation or runtime behavior. `source query`
supports `precise` and `exploratory` modes and the `api`, `lua`, `xml`, `toc` and
asset topics. Precise mode prioritizes stronger matches; exploratory mode
broadens matching, so treat results as candidates and verify them in their
pinned locations. Calls are inferred, dynamic calls remain unresolved, and
generated API definitions are separately categorized. Keep query truncation
visible; inspect returned source locations to obtain original-file evidence.
`source inspect --symbol` searches indexed symbols; `--target-path` inspects
indexed file or asset metadata. Both require an index.

`source diff` requires two explicitly fixed, complete indexes from the same
repository and parser revision. It compares indexed Lua/XML/TOC content and
declaration sites, not every repository asset or version metadata file. Duplicate
declarations remain separate sites; a line-only move is a source-location change,
not proof of an API break. `--limit` applies separately to document and declaration
change groups; totals and `truncated` describe the full comparison. The capture
records both snapshot IDs and both commits. An incomplete index is rejected
rather than making an unparsed declaration appear removed.

For load/syntax/source-name checks and fixed multi-client validation, use the
addon-validation reference. A matrix validates only the clients and pinned
inputs declared in its config; preserve unresolved coverage and per-client
results.

Choose the narrowest stable symbol, path, event, template, TOC field, or API
name in the question. Use a broader query only to discover candidates, then
verify the result with the exact definition and returned location. Preserve the
excerpt, path, line, relationship confidence, and match/truncation metadata.

Source facts are not runtime proof. A definition or relationship can establish
what the pinned source contains, not that a live client loaded it or that the
API behaves identically under combat, secret values, or another build.

## Compatibility and closure

When checking an addon, route to [addon-validation.md](addon-validation.md) so
the ordered TOC closure and client matrix are checked as a unit. Do not infer a
client identity from a directory name or a branch label when resolved metadata
and build evidence are available.

If the pinned source is present but its index is missing, build that index and
retry with the same pin. Investigate preparation failures without silently
changing the source version. Keep dynamic or conditional relationships
explicitly unresolved.
