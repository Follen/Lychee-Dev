# Source selection and recovery

Read when the requested source is not pinned or source preparation fails.

## Select the repository revision

Use `source list` to discover repository keys and their supported tracks. It does
not list tags; missing version text there does not mean the release is absent.
Read the repository's own track map: do not apply a game product to every addon
repository. A source track is not proof of installation compatibility.

Initialize a new Toolkit home with `lycheedev init` before syncing; an empty
directory alone is not a workspace. Reuse an initialized `--home` when available.
Use that same home for preparation and subsequent pin reads; a pin ID does not
select a different workspace. Pass `--home <workspace>` when working outside its
normal discovery context.

```text
lycheedev source list --format json
lycheedev source sync --source <catalog-key> --product <repository-track> --ref <commit-or-full-ref> --format json
```

`source sync` returns the immutable PinnedSet directly. `--ref` accepts a 40-hex
commit or full `refs/heads/...` / `refs/tags/...` ref. Omission observes the selected
track's current branch; use that only when the request allows current source.
For a named release without an exact ref, establish its tag/commit from repository
evidence. Ask only if that mapping remains unavailable or ambiguous. Never guess
a tag, silently use latest, or replace an unavailable exact revision with a branch.

Carry the returned fixed pin and exact commit between calls. Preserve requested
version/tag, repository identity and parser revision; a tag is a traceable label,
not immutable evidence. Separate third-party addon pins from client API pins;
`--environment <client-pin>` on refs/context supplies API context without changing
the addon revision. Folder names and branch labels do not prove a client's build.

## Recover without changing the revision

Keep the original command, pin/ref and structured error. The CLI prepares mappings
when the operation needs them; raw file inspection reads fixed Git content directly.
`source list --snapshot <pin>` reports readiness;
`source index --snapshot <pin>` is optional, not a mandatory repair sequence.

| Condition | Next action |
| --- | --- |
| Unknown repository/track or invalid ref | Check catalog and exact ref syntax; correct the selection without substituting the requested version |
| Missing Git, source objects or preparation failure | Address the named prerequisite and retry the same fixed source; do not manually rewrite managed worktrees |
| Integrity failure | Preserve evidence and diagnose changed/corrupt bytes; do not bypass verification or delete caches blindly |
| Syntax diagnostics / partial map | Inspect readable original files and retain the gaps; unparsed does not mean absent |
| Missing LuaLS or client environment | Continue original-source research with partial semantic coverage; see [source-relations.md](source-relations.md) |
| Invalid cursor | Restore the originating command and scope to continue, or start without a cursor for the changed scope; do not merge those page sets |
| Resource budget | Narrow to the part needed by the question; retain omitted scope, not a claim of complete investigation |

Allow at most one unchanged retry for a transient failure within the task budget.
Further retries require a changed cause. If still blocked, stop that branch and
report its exact revision, error, useful captures and missing prerequisite;
independent useful source inspection may continue. Do not copy data-only timeout
or offline flags onto source commands; consult installed help for their surface.

Source disk reclamation is separate from research. Only when requested, use
`source prune --target-bytes <n>`: it removes idle derived worktrees and preserves
pins, cached facts and captures. It is not the ordinary `cache prune` command.
