# Addon validation

Use this workflow for TOC/XML/Lua load closure, interface compatibility,
static addon checks, and cross-client validation.

## Validate the real closure

Select the exact pinned source and the addon input. Validate the named relative
TOC and ordered Lua/XML closure; the CLI prepares any missing source mapping:

```text
lycheedev source validate --path <addon-root> --toc <relative-toc-file> --snapshot <pin> --format json
```

For a fixed multi-client check, provide a matrix config:

```text
lycheedev source validate --matrix <config.json> --format json
```

The minimum config has one target; add a target for each selected client:

```json
{
  "path": "./addon",
  "targets": [
    {
      "id": "classic",
      "toc": "Example.toc",
      "product": "classic",
      "ref": "<40-character-source-commit>"
    }
  ]
}
```

Replace the example path, TOC and commit with the actual inputs. `path` resolves
relative to the config file; `toc` is relative to that addon root. Each target
requires a unique `id`, `toc`, `product` and `ref`; the optional source field defaults to
`wow-ui-source`. Use the exact 40-character commit for a fixed comparison, or an
explicit full `refs/tags/...` / `refs/heads/...` ref whose resolved commit is
retained. A branch ref selects its current commit at preparation time, not a
historical version. Unknown fields are rejected.

Otherwise validate one pinned closure as above. Check `describe` for command
options; it does not provide the matrix JSON schema. Keep each client result separate, including
interface/build identity, missing files, parser issues, unresolved dynamic
edges, and representative locations. Do not validate a recursive directory
scan when the question is about the release TOC.

`load.loadValid` covers paths, ordered references and syntax. `staticValid` adds
the checks listed in `checks`; source-name presence is not binding-identity or
argument-type validation. Missing source names may be addon-local or dynamic and
remain unresolved, rather than being reported as nonexistent game APIs.

`complete: false` means coverage is still incomplete even when the command exits
successfully. Inspect `unresolved`, `interfaceStatus`, `checks` and `notChecked`.
An actual static failure exits nonzero while preserving the result and capture.
Interface matching currently uses only the Toolkit's exact project-approved
commit baselines; an unknown commit is unresolved, not guessed from its version.

Treat `staticValid: true` as “the reported checks found no error.” It is not
proof of in-game loading, taint safety, combat behavior, performance, or
visual correctness. Those require the appropriate live or visual evidence and
must be reported as untested when absent.

Use `--semantic` only when LuaLS diagnostics will answer the question. The CLI
verifies the bundled runtime before use; a missing tool is a capability gap,
not a reason to replace the fixed source or skip the ordinary static result.
For a third-party addon source, pass a fixed client API source with
`--environment <client-pin>`; the toolkit does not infer Retail from the addon
folder or product name. This flag applies to a single pinned validation; a
multi-client semantic matrix uses each target's own pinned client API source.
If a target is a third-party repository without that environment, report the
capability gap instead of applying one client's definitions to every target.
Semantic results include registered captures for the
checked inputs, generated definitions, configuration and raw report. Verify
their IDs when preserving or comparing evidence. LuaLS diagnostics and a clean
static check do not prove secret-value safety or secure execution taint.

Identify a client from its `.flavor.info` and `version.txt` metadata and build,
never from its directory name. Missing or conflicting identity stays unresolved.
Do not add unsupported client branches or hand-maintain a second product/build
catalog in this workflow.
