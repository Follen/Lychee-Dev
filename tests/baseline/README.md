# Functional regression baseline

Run this suite after changing the CLI, addon, source/data modules or skill.
It checks the current working tree. It does not publish, install an addon or
replace the release acceptance matrix.

## Offline baseline

From the repository root on Windows amd64:

```powershell
node tools/baseline.mjs --lua51 "C:/path/to/lua.exe" --luals-archive "C:/path/to/pinned-luals.zip" --luals-runtime "C:/path/to/payload/tool/luals"
```

Lua must be **5.1**. Build it with `node tests/tools/build-lua.mjs` if needed.
The LuaLS archive and runtime must match `release/tools/luals.json`. Their
environment variables are `LYCHEEDEV_LUA51`, `LYCHEEDEV_LUALS_ARCHIVE` and
`LYCHEEDEV_LUALS_TEST_RUNTIME`; when set, run `node tools/baseline.mjs`.

The runner executes `go build ./...`, `go vet ./...`, uncached full Go tests
with mandatory Lua suites, all Node tool/npm tests, version consistency and
the skill command contract. It also verifies that source did not change during
the run. Missing/skipped required tests, missing real LuaLS integration or
skipped Node tests prevent a passing baseline. Raw output preserves every
test result, including tests outside the minimum case catalog.

| IDs | Required behavior |
| --- | --- |
| BASE-01–02 | Duplex command admission, retired-input rejection, fixed targets and concurrent resolution |
| BASE-03–04 | Exact source worktrees, dirty detection, leases and semantic cache identity |
| BASE-05–06 | Hotfix pagination, missing-key integrity and asset evidence |
| BASE-07 | First managed install, modified-file refusal and upgrade recovery |
| BASE-08 | Duplex wire integrity, durable execution, recovery and bounded retention |
| BASE-09 | Verified migration of idle legacy LoD files and refusal of unresolved pools |
| BASE-15–16 | Read-only installation/window inventory and process crash recovery |
| BASE-17 | Workbench pages, shared controls, layout, locale and disabled-cost fixtures |
| BASE-18–21 | Evidence integrity, SQL semantics and managed update recovery |
| LUALS | Real LuaLS process: API references and definitions |

Exact required test names live in [catalog.mjs](catalog.mjs). Removing or
renaming a required test without updating this contract fails the baseline.
Four-profile fixtures include Forever only as an unverified compatibility
path; they are not evidence of running any real client.

## Real-client qualification

The old OP/session live runner has been retired. Native Duplex real-client
qualification is **unavailable and remains `not_run`** until an owner records a
run using the dedicated real-client harness. Offline Go, Lua and protocol
fixtures do not satisfy this qualification. The supported acceptance clients
are Retail 120100, Classic 50504 and Titan 38002; Forever 16001 is excluded.

Manual real-client evidence must be recorded separately and must identify the
client build, observed result, cleanup state and reviewer. Do not infer
qualification from an offline fixture or an earlier OP/session run.

## Evidence

Each offline run creates a fresh `.tmp/baseline-offline-<timestamp>/` with
`report.json`, `summary.md`, command/process metadata and raw stdout/stderr.
Existing output directories are refused. Exit **0** means this run's selected
checks passed, **1** failed, and **2** blocked/incomplete. `passed` does not
claim whole-client, visual or real-client acceptance.
