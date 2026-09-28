# Classic input policy and Automation history — 2026-09-28

Development candidate on `codex/nonce-memory-transport`, version 2.5.1. No
publication or release acceptance is implied. Actual target: Classic **50504**,
build **5.5.4.69934**, PID **28276**, **次年雪 — 祈福**. This is a separate
client from the earlier Titan 38002 acceptance run.

## Input policy

Ordinary and asynchronous probes run without input protection. Each slot key
transaction releases its own short lease before business dispatch. An explicitly
exclusive probe can acquire one protection phase, with no separate five-second
cap; its duration must fit the remaining immutable probe execution budget.
Errors, cancellation, combat/world interruption, lease deadline and completion
release protection. Stale timers cannot release a newer lease. Scene guards
check before execution/callbacks/completion and on the declared events.

Protected activity says **Agent 接管中**; normal activity says **Agent 运行中**.
The fallback reload sequence is host-serialized and identity checked but can run
before the addon is active, so it does not promise a game-side input shield.

- `.tmp/channel-live/classic-input-activation/baseline.json`: 20 public CLI
  calls passed, including slot capacity reload, old request reuse, syntax error,
  cleanup failure recovery, diagnostics and repeated disconnect.
- `.tmp/channel-live/classic-input-policy/input-policy.json`: 12 steps passed.
  Requested 12-second protection, released at 8 seconds, then ran unprotected
  until completion. WGC phase 1/3 images show the two labels. Deadline/error/scene
  guard fixtures failed as intended and released resources.
- `.tmp/channel-live/classic-reload-hint/evidence/`: standalone 40-second reload
  observation captured two RGB transitions. The production eight-second hint
  window recorded zero in earlier slow-loading runs; fresh runtime/actor binding
  still succeeded. RGB is an observation hint, never identity proof.

## Automation regression and repair

The new SlotProtocol never notified AutomationView, which still discovered only
the legacy ProbeQueue/ProbeRunner/ReportStore records. The production runtime
regression originally failed with `memory probe missing from Automation history`.
The CLI's project reports were present; the in-game page had no native records.

`AutomationHistory` now receives prepared/running/reported/released observations
through SlotRuntime's adapter. It stores at most 100 records per character in
`LycheeToolkitBridgeDB.automationHistory`, with a 32 KiB UTF-8 preview per report.
Identity includes runtime, ticket, nonce, build and actor. Release retains the
preview. Reload marks unfinished old-runtime records interrupted; a retained
report without confirmed release is not falsely promoted to acknowledged.
History is display-only: it cannot execute a native task or redisplay a QR.
Clear persists deletion and skips active records. It creates no frames/events or
timers; future/invalid schemas are left untouched. The project journal owns
durability and recovery. SavedVariables alone cannot promise crash durability or
restore old previews that were never recorded.

Offline coverage includes the real slot adapter, report release, fresh-module
rehydration, interrupted tasks, failed reports, pending-record protection, count
and byte limits, UTF-8 boundary truncation, clear/reload, character isolation and
invalid/future schemas. `tests/channel-live/automation-history-baseline.mjs`
provides repeatable real-client acceptance with explicit target and project.

First live result: `.tmp/channel-live/classic-automation-regression/automation-history.json`,
9 steps passed. Success and failure reports survived actual reload with identical
IDs and bytes, changed runtime, disabled native replay and WGC confirmation.
The final UTF-8 boundary candidate was installed through managed delivery and
repeated all 9 steps successfully in
`.tmp/channel-live/classic-automation-final/automation-history.json`. Its reload
WGC image is under `after-reload/evidence/20260928T030122.992759800-capture/`.
Both test connections were explicitly disconnected; the workbench remains open
on the retained report for inspection. Build/vet, the mandatory full Lua 5.1
suite (`.tmp/channel-live/automation-history-final-go.txt`), version consistency
and the 87-command/202-reference Skill contract all passed.

Remaining limits: no client-crash persistence claim, no automatic historical UI
backfill, no full relogin/shared-installation acceptance in this run. The earlier
unrelated transport acceptance gaps remain open.
