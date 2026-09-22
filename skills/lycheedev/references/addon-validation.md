# Addon validation

Use this workflow for TOC/XML/Lua load closure, interface compatibility,
static addon checks, and cross-client validation.

## Validate the real closure

Select the exact pinned source and the addon input. Prepare its source index,
then validate the named relative TOC and ordered Lua/XML closure:

```text
lycheedev source index --snapshot <pin> --format json
lycheedev source validate --path <addon-root> --toc <relative-toc-file> --snapshot <pin> --format json
```

Check `describe` before use. Matrix-file validation is not implemented yet; keep
each explicit TOC/snapshot invocation separate rather than inventing a matrix
flag. Keep each client result
separate, including interface/build identity, missing files, parser issues,
unresolved dynamic edges, and representative locations. Do not validate a
recursive directory scan when the question is about the release TOC.

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

When a client directory is reused by a test track, identify the actual product
from its own metadata and build before using the directory name as a fallback.
Do not add unsupported client branches or hand-maintain a second product/build
catalog in this workflow.
