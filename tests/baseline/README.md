# Functional regression baseline

Run this suite after changing the CLI, addon, source/data modules or skill.
It checks the current working tree, including uncommitted changes. It does not
publish, install an addon or replace the release acceptance matrix.

## Offline baseline

From the repository root on Windows amd64:

```powershell
node tools/baseline.mjs --lua51 "C:/path/to/lua.exe" --luals-archive "C:/path/to/pinned-luals.zip" --luals-runtime "C:/path/to/payload/tool/luals"
```

Lua must be **5.1**. Build it with `node tests/tools/build-lua.mjs` if needed.
The LuaLS archive and runtime must match `release/tools/luals.json`; use the runtime
from a development or release package. The corresponding environment variables
are `LYCHEEDEV_LUA51`, `LYCHEEDEV_LUALS_ARCHIVE` and
`LYCHEEDEV_LUALS_TEST_RUNTIME`; when set, the invocation is simply:

```powershell
node tools/baseline.mjs
```

The runner executes `go build ./...`, `go vet ./...`, uncached full Go tests
with mandatory Lua suites, all Node tool/npm tests, version consistency and
the skill command contract. It also verifies that the source did not change
during the run. Missing/skipped required tests, missing real LuaLS integration
or skipped Node tests prevent a passing baseline. Other optional-environment
and helper-process Go skips are listed separately in `checks.GO.skipped` (the
`checks` array entry whose ID is `GO`); they do not count as passed tests.
Raw output preserves every test result, including tests
outside the minimum case catalog.

| IDs | Required behavior |
| --- | --- |
| BASE-01–02 | CLI flag admission, passive discovery, fixed targets and concurrent resolution |
| BASE-03–04 | Exact source worktrees, dirty detection, leases and semantic cache identity |
| BASE-05–06 | Hotfix pagination, missing-key integrity and asset evidence |
| BASE-07 | First managed install, modified-file refusal and upgrade recovery |
| BASE-08–11 | Receiver focus release, hardware confirmation, input intent, bounded wake and reload beacon |
| BASE-12–14 | Async ownership, character SV, verified reports, ACK/display cleanup and no replay after interruption |
| BASE-15–16 | Workspace/window/character isolation and process crash recovery |
| BASE-17 | Workbench pages, readable shared controls, layout, locale and disabled-cost fixtures |
| BASE-18 | Evidence corruption detection and workspace isolation |
| BASE-19–20 | SQL partial coverage, hotfix provenance, JSONL completion, aggregation and subqueries |
| LUALS | Real LuaLS process: API references and definitions |

The exact required test names live in [catalog.mjs](catalog.mjs). Removing or
renaming a required test without updating this contract fails the baseline.
Four-profile fixtures include Forever as an unverified compatibility path;
they are not evidence of running any real client.

## Real-client baseline

Use a clean managed addon matching the CLI and an existing connected session.
The selected character must be logged in and out of combat. This suite can
reload the game for loading and SavedVariables persistence, briefly opens all
workbench pages and closes its workbench during cleanup. It does not change
shortcut settings. Run it when no other task needs that client.

```powershell
node tools/live-baseline.mjs --cli "C:/path/to/lycheedev.exe" --session "SESSION-..."
```

Optional: `--home <workspace>`, `--account <account>`, `--out <new-directory>`.
The session determines the window, character and build; the runner does not
automatically select or connect a different client. The old `--connect` string,
named temporary probe and `--skip-hide` flow have been removed. Connect through
the production CLI first; complete cleanup is mandatory.

| ID | Pass criteria |
| --- | --- |
| LIVE-01 | Inline sum 1…10 = 55; receiver inactive and optical overlay absent at business entry; verified report, complete cleanup and display-clear capture |
| LIVE-02 | Repeat the exact request, then resume its completed operation; same operation ID, body/receipt digest and clear capture |
| LIVE-03 | Deliberate `BASELINE_EXPECTED_FAILURE` yields business failure/exit 5 **and** verified report with complete cleanup |
| LIVE-04 | 55-second async probe visits eight main pages and Settings; receiver and transport QR remain absent at each callback; displayed shortcuts match current settings; final cleanup succeeds |

Live probes are versioned in [probes/](probes/), copied into each run's evidence
directory before use, and assigned immutable request keys and source hashes.

The optional [UI activity probe](probes/ui_activity.lua) exercises event selection,
Diagnostics, the running Automation state, report/details switching, fixed
read-only shortcuts, and a persistent activity indicator with input released.
Run it with `live execute --session <session> --file tests/baseline/probes/ui_activity.lua
--request <unique-key> --budget-seconds 75 --format json` and capture the same
window through WGC for visual review. It changes the visible workbench for 56
seconds and closes it on cleanup. Its assertions do not prove physical mouse,
IME or OS shortcut behavior; those remain REAL-03 acceptance.

The runner stops at the first failure/pending result. It never retries unknown
input, auto-abandons an operation or continues with a new request after a
blocked case. The CLI owns journal recovery, not the test runner.

For interruption, inspect `report.json` and the matching `*.command.json`,
`*.stdout` and `*.stderr`. Recover the recorded `OP-...` or `BTP-...` through
`lycheedev live status <id>` / `lycheedev live resume <id>` using the original
workspace. If the process died before returning an ID, the saved execute
arguments preserve the same request key. Do not create another baseline run
until the original ownership has been resolved. A new run uses new request keys.

LIVE-02 verifies evidence reuse through the public CLI. The no-input and
immutable-journal guarantees are additionally tested by the offline lifecycle
fixtures; the live runner does not infer zero keyboard input from elapsed time.

## Evidence and remaining acceptance

Each run creates a fresh `.tmp/baseline-<mode>-<timestamp>/` containing:

- `report.json`: source commit/tree digest, check outcomes, durations, raw
  command paths, live request/operation IDs and CLI report/clear evidence.
- `summary.md`: readable result table.
- `*.command.json` written before process launch; `*.process.json` plus raw
  stdout/stderr; copied immutable live probes.

An existing output directory is refused. Exit **0** means this mode's selected
checks passed, **1** failed, **2** blocked/incomplete. `passed` is never a claim
that the whole client matrix or visual acceptance passed.

REAL-01…09 remain `not_run` in generated reports: WGC visual inspection,
locale/viewport/scale, physical input/IME, RGB timing, first-install/relogin/
combat, multiple real windows/builds, Classic, Titan and long-running stress.
Record actual WGC captures, client identity and reviewer observations in the
acceptance document before changing those statuses. Functional page activation
alone does not prove font readability, unobstructed pixels or rounded corners.
Production scope remains Retail 120100, Classic 50504 and Titan 38002; Forever
16001 is excluded from acceptance.

The runner's own fault tests run with `node --test tools/baseline.test.mjs` and
are also discovered by the existing CI Node test glob.
