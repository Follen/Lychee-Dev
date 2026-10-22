# Source selection and recovery

Read when a source pin is missing or preparation fails.

## Select the repository revision

`source list` discovers repository keys and supported tracks, not release tags.
Use each repository's own track map; a game product is not every addon's track,
and a source track does not establish installation compatibility.

For `wow-ui-source`, the seven delivery products map to these Git branches:

| Product | Branch |
| --- | --- |
| `retail` | `live` |
| `classic` | `classic` |
| `classic-era` | `classic_era` |
| `titan` | `classic_titan` |
| `forever-asia-beta`, `forever-cn-beta` | `forever` |
| `anniversary` | `classic_anniversary` |

The `_classic_beta_` client folder does not map to the `classic_beta` track; both
Forever betas use the family-wide `forever` track. A family-wide `forever`
Source pin can accompany either beta's Data pin; regional Source pins retain
their exact delivery identity. Data build configurations and client runtime
verification remain separate even when the UI source commit is shared.

Reuse an initialized Toolkit home for preparation and later reads; a pin ID does
not select a different home. Initialize a new home only when needed. `source sync`
returns a fixed PinnedSet. An exact ref is a 40-hex commit or full branch/tag ref;
omission observes the track's current branch only when the request allows it.
For a named release, resolve its actual ref from repository evidence. If the CLI
lacks tag discovery, use verified repository metadata under the
[source routing rules](source-research.md#route-source-evidence), then sync that
resolved ref. Never guess a tag or replace an unavailable release with a branch.

Retain repository, exact commit, requested version/ref and parser identity. A tag
is a traceable label, not immutable evidence. Keep third-party addon and client
API pins separate; an environment adds API context without replacing addon source.

## Recover without changing the revision

Keep the original selection, structured error and captures. Queries prepare
needed mappings; raw inspection reads fixed Git content. Indexing is optional.
Correct the named cause: catalog/ref selection, missing prerequisite, integrity
failure, semantic environment or resource limit. Do not rewrite managed worktrees,
delete caches blindly or treat unreadable/unparsed source as absent.

Use readable original source when semantic coverage is partial, retaining its
limits. A changed query scope needs a new cursor. Narrow only when that still
answers the question. Allow at most one unchanged transient retry within budget;
further retries require a changed cause. Stop the blocked branch with its exact
revision and prerequisite, while independent useful research may continue.

Source reclamation is a separate requested task through `source prune`, not a
repair prerequisite or the ordinary cache-prune operation.
