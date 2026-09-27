# Source research

Use this workflow for versioned UI/API definitions, Lua/XML/TOC source, symbol
relationships, source diffs, and compatibility questions.

## Pin and query

Resolve or receive a `SourcePin` containing the repository identity, exact commit,
parser revision, and any traceable tag. A tag is a label, not the
immutable evidence. Keep the exact commit in the report. If the requested exact
source cannot be resolved, stop with an actionable unresolved result; do not
silently substitute the newest snapshot.

Check `describe --format json` for the installed command surface. Select the
repository and track from the question, then carry the returned fixed snapshot.
Run `lycheedev init` once for a new workspace before `source sync` (or use an
already initialized `--home`); a fresh directory alone is not a workspace.
Query, inspection, relations and context prepare the needed internal mapping
for that same snapshot; `source index` is an optional explicit warm-up:

```text
lycheedev source list --format json
lycheedev source sync --source <catalog-key> --product <track> --ref <commit-or-full-ref> --format json
lycheedev source index --snapshot <pin> --format json
lycheedev source query <term> --snapshot <pin> --mode precise --topic api --limit 50 --format json
lycheedev source inspect --snapshot <pin> --path <repository-relative-file> --line <first-line> --count <line-count> --format json
lycheedev source inspect --snapshot <pin> --symbol <qualified-name> --format json
lycheedev source inspect --snapshot <pin> --target-path <path> --format json
lycheedev source refs --snapshot <pin> --symbol-id <id-from-query> --direction both --limit 50 --format json
lycheedev source context --snapshot <pin> --symbol-id <id-from-query> --limit 50 --max-lines 120 --format json
lycheedev source diff --from <pin-a> --to <pin-b> --limit 50 --format json
lycheedev source validate --matrix <config.json> --format json
```

`source sync` returns the immutable PinnedSet directly. Its `--ref` accepts a
40-character commit or a full `refs/heads/...` / `refs/tags/...` reference; omitting
it selects the catalog's product branch at preparation time. Do not omit it when
the user supplied an exact version. A source query product is not evidence that
the addon supports installation on that client.

`source list` lists repositories and product branches, not tags. When the user
supplies an exact commit or full ref, pass it directly to `source sync`. When
only an addon version is supplied, establish its exact tag or commit from public
repository evidence, or ask for the missing reference when that evidence is
unavailable or ambiguous. Do not guess a tag or treat its absence from
`source list` as proof that it does not exist. Retain the requested version/tag
and resolved commit; never fall back to a product branch or newer tag. If automatic
preparation fails, inspect the reported gap and retry only the same fixed snapshot
after resolving it.

`source inspect` reads only the prepared exact commit, without network access.
It archives the **whole original file**, while `result.text` contains only the
requested lines (at most 2,000). The capture being complete does not mean the
excerpt contains the whole file. Preserve `context.snapshot`, the exact commit,
path, first/last/total line counts, full-file hash and capture ID. Git must be
available for preparation and reads; this workflow does not invoke the old CLI.

`source index` warms the rebuildable file mapping by analyzing Lua/XML/TOC
without executing them. A successful command can still have `complete: false`:
retain the diagnostic count and sample, rather than treating an unparsed file
as empty. Mapping completeness covers syntax extraction, not load-closure
validation or runtime behavior. `source query`
supports `precise` and `exploratory` modes and the `api`, `lua`, `xml`, `toc` and
asset topics. Precise mode prioritizes stronger matches; exploratory mode
broadens matching, so treat results as candidates and verify them in their
pinned locations. Calls are inferred, dynamic calls remain unresolved, and
generated API definitions are separately categorized. Keep query truncation
visible; inspect returned source locations to obtain original-file evidence.
`source query --cursor` pages matching results; its relation list is a bounded
preview. If relation evidence can change the answer, use `source refs` with the
selected `symbolId` and page that command's own cursor.
`source inspect --symbol` finds symbol candidates; `--target-path` inspects file
or asset metadata. Use returned stable symbol IDs for relation and context
requests. If names collide, inspect each candidate's repository, commit, path
and scope; do not join them by spelling alone.

`source refs` returns references/calls for one symbol. `source context` combines
its definition, bounded code, direct relations and available loading evidence.
Both accept `--symbol-id` from query or `--symbol` for an unambiguous name, plus
`--cursor` for the next page. Keep the same snapshot, symbol and budgets when
using a cursor. Relations identify confirmed semantic resolution, static
candidates and unknown edges separately. The CLI attempts the verified bundled
LuaLS when needed; `--static-only` chooses structural evidence alone. For a
third-party repository, pass `--environment <client-pin>` when a client API
environment is relevant. A missing runtime or environment leaves semantic
coverage partial; continue original-source research and state that limit.

When explicitly reclaiming source disk space, `source prune --target-bytes <n>`
removes only idle derived worktrees. It preserves fixed pins, cached facts and
capture evidence; the ordinary `cache prune` remains a separate operation.

`source diff` requires two explicitly fixed snapshots from the same
repository. It compares file and declaration content and
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
verify the exact definition and follow relations only when the question needs
them. A sufficient original declaration can answer a simple API question.
Preserve excerpt, path, line, relation state, coverage and truncation metadata.
Follow `nextCursor` only while another page can change the answer; never treat
the first truncated page as the whole set.

Source facts are not runtime proof. A definition or relationship can establish
what the pinned source contains, not that a live client loaded it or that the
API behaves identically under combat, secret values, or another build.

## Compatibility and closure

When checking an addon, route to [addon-validation.md](addon-validation.md) so
the ordered TOC closure and client matrix are checked as a unit. Do not infer a
client identity from a directory name or a branch label when resolved metadata
and build evidence are available.

Investigate preparation failures without silently changing the source version.
Keep dynamic or conditional relationships explicitly unresolved.

## Secret values and secure taint

For these questions, start from the reported operation, value or source location.
Pin both the addon revision and the relevant client API source; inspect the exact
generated declarations and their conditions. Preserve metadata such as
`SecretReturns`, `SecretPayloads` and `SecretArguments` as written. Do not assume
Forever follows Retail rules or lacks secret values because it is an older client.
Known `SecretArguments` modes such as `AllowedWhenTainted` and
`AllowedWhenUntainted` describe conditional permission, not a static proof of
the caller's taint state. Keep the raw mode in the finding and classify the
path as possible until runtime conditions are observed.
An API marker can establish a possible source; it alone does not prove that the
reported call produced a secret value or that the consuming operation forbids it.

Use the installed capabilities reported by `describe --format json`. Query and
inspect can support a source-based explanation; inferred calls and LuaLS types
alone are not a verified value-flow analysis. Request `source context --flow`
only for a concrete value/operation question. Interpret returned steps,
conditions, rule identity and unknown boundaries; do not claim a proven
propagation path when the result has only static candidates or incomplete coverage.

Trace the relevant value through assignments, table fields and function arguments
or returns, preserving each step's original location. Confirm symbol identity and
loading context before crossing files. A guard applies only to the checked value
on the relevant branch before reassignment; merely calling `issecretvalue` does
not establish that later operations are safe. Check the consuming operation's
versioned restrictions before reporting a violation. Mark dynamic calls, unknown
aliases, missing dependencies and truncation as gaps rather than inventing edges.

Keep secret-value propagation separate from secure execution taint. A stack made
entirely of Blizzard frames is not sufficient to identify who caused the taint;
static hook/write candidates need further evidence. Source-only research does
not authorize game input. For deep investigation with live work in scope, carry
the fixed source locations, suspected value path, conditions and missing links
into [the live hypothesis workflow](live-investigation.md#test-source-hypotheses).
Continue with existing error evidence and a bounded check that distinguishes the
remaining explanations; do not stop at an unresolved static path when live can
answer the next question. Feed observed conditions back into the source analysis.

Finish with the supported path and restriction, a minimal correction when
justified, and any conditions still requiring verification. Distinguish observed
runtime evidence, source deductions and unresolved hypotheses. Do not treat no
findings as proof of safety, or propose coercion/default substitution as a generic
way to remove secrecy. When the path cannot be established, state the missing
link and the evidence needed to resolve it.
