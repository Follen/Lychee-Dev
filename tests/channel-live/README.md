# Native memory / slot acceptance workbench

Windows amd64, native Go reader and real main-addon protocol. No external memory
tool is needed. This workbench is excluded from release builds. It supplies
controlled fault injection, WGC captures and scan benchmarks; ordinary acceptance
uses the public `live connect/execute/resume/reload/bugs/disconnect` CLI. Keep the
remaining disaster-recovery and multi-client matrix separate from scoped results.

```powershell
go build -tags lycheedev_channel_lab -o .tmp/channel-live/host.exe ./tests/channel-live
.tmp/channel-live/host.exe --mode discover --installation 'D:/Game/World of Warcraft/_retail_' --pid <observed-pid> --project <test-project> --timeout 60
```

Use an explicitly selected live PID and a dedicated test project. `connect`
creates a durable project journal and installation-wide connection claim.
`run --file tests/channel-live/fixtures/normal.lua --budget 10` executes an
observation fixture. `resume` continues the same journal; a completed operation
is read-only. `disconnect` verifies unbind before retiring the host claim.
Every mode has an overall `--timeout` (1–300 seconds); `--cache=false` disables
all address/region hints. Fixtures cover ordinary results, async completion,
expected errors, timeout and a 90 KB UTF-8/HTML/newline payload.

`capture` saves a real WGC image. `reload` requires an unowned installation and
records each fixed-input step, then two transitions of the single RGB patch.
The patch is only a hint: follow with native discovery and fresh binding.
`install --release <development-package>` uses managed delivery. Temporary
upgrade recovery material is not a permanent backup.

The `repair-bootstrap`, `reload-bootstrap`, `retry-bootstrap` and
`recover-bootstrap` modes are narrowly scoped development recovery for the
first unpublished template's dependency failure. They require an unresolved
bind and no business operation. Retry additionally verifies that every other
slot is inert, retaining the publication lease through input. They do not
authorize general replay of unknown input or a business request.

`scan-benchmark` performs full one-worker and eight-worker native scans against
the same live process. Compare coverage as well as elapsed time; region churn
can prevent complete coverage. Avoid running other benchmarks concurrently.

```powershell
node tests/channel-live/summarize.mjs <test-project>
```

This produces `summary.json` from retained evidence, without exposing payloads.
The original `evidence/*`, `.lycheedev/live/connections/*.jsonl` and immutable
`.lycheedev/live/results/*.json` remain the source evidence. Results stored as
`resultBytes` preserve original bytes using base64. A result artifact records
verification; its connection journal independently records confirmed release.
Do not edit these files to force a passing state.

## Public CLI baseline

### Automatic optimization first round

Once a matching clean managed candidate is installed, the owner only needs to
log one accepted client into a safe, noncombat character. The agent can run:

```powershell
node tests/channel-live/optimization-baseline.mjs --cli '<candidate.exe>' --project '<new-evidence-project>'
```

The runner uses passive discovery and selects only one unoccupied, supported
process. If more than one client is open, supply both `--installation <client>`
and `--pid <pid>`; it never selects a replacement target. Process creation time
is preserved as an exact decimal string. Candidate deployment and rollback use
managed delivery outside this runner; it refuses modified/incomplete/pending
addon files and never overlay-copies them.

The fixed first round contains cache-off connect, five normal requests with
hints and five without, a large HEAD/BODY result on each path, an exact historical
request readback, explicit reload with verified runtime replacement, a normal
request after reload, disconnect and read-only repeated disconnect. It uses only
checked-in read-only probes, copies/hashes them, checks actor identity after each
command, and verifies business results independently of transport completion.
All input, reconnect and cleanup decisions belong to the public CLI.

Every CLI command has a saved intent before launch, raw output and outcome.
`optimization.json` includes CLI/probe digests, installation receipt, process/actor,
durations and additive `observation` metrics. It requires actual RPM and mapping
queries plus real read bytes; unavailable counters never become synthetic zeroes.
Metrics are **invocation-scoped**, not durable operation totals across resume.
Nearby misses/timeouts are diagnostic and may precede a successful full fallback.
Warm/cache-off labels describe scheduling paths, not OS/network coldness or a
measured speedup. The collected samples are not an RSS or p95 claim.

The evidence directory must be new. Pending, cancellation, malformed output or
unexpected results stop the runner and preserve the original CON/project/request.
It never sends a second uncertain request, auto-abandons or disconnects uncertain
work in a `finally` block. Recover using the original public status/resume flow
before any new run; do not restart this runner with a new project to escape a
blocker. Exit 0 means only this first-round scope passed; exit 2 is incomplete.
Re-login, combat, dual instances, other clients, physical IME and subjective
experience remain separate `not_run` entries. CI imports only the runner's fault
tests and never executes this real-client entrypoint.

After installing and activating a matching managed candidate, run from the repo:

```powershell
node tests/channel-live/public-baseline.mjs .tmp/channel-live/lycheedev.exe .tmp/channel-live/new-baseline 'D:/Game/World of Warcraft/_retail_' <observed-pid>
```

The project must be dedicated to this run. `baseline.json` is created exclusively;
the runner refuses to overwrite a prior run. It records each argv, exit status,
duration and original result. Assertions cover cache-off connection, 47
normal tasks (46 per fresh runtime, then automatic rollover on task 47), history rotation and read-only old
request replay, compile failure, cleanup failure with verified reload, diagnostic
completeness and repeated disconnect. Failure retains the exact connection for
resume; the runner does not abandon or reset it.

After resolving the recorded dependency, append `--resume` with the same project,
installation and PID to continue an incomplete public baseline. It validates the
original CON and reads completed request keys without executing them again.
Exhausted budgets, closed connections and unresolved external blockers stop the
runner. Resumed reports and attachments receive unique names; the original
failure remains intact.

For controlled interruption, `interrupt-observation` requires a running observation
and the exact owning project/target. `interrupt-opaque-fixture` additionally
requires the checked-in read-only `async_reload.lua` bytes under opaque policy;
it tests unknown-effect handling without executing a real state-changing effect.
Both retain the pre-interruption state, WGC image and input steps. The optional
`-reload-hint=false` omits RGB sampling for fault injection; the returned input
receipt does not prove reload. Public resume must verify a new runtime/binding.
Default hint sampling remains available for the separate beacon check.
An opaque operation may close with `closed=true` and `complete=false`: ownership
has ended while its business outcome remains unknown. Keep these facts separate.

## Two-client interruption baseline

```powershell
node tests/channel-live/cross-client-baseline.mjs .tmp/channel-live/lycheedev.exe .tmp/channel-live/host.exe <new-project-root> <targets.json>
```

`targets.json` contains exactly two objects with `name` (lowercase letters),
absolute `installation`, and the currently observed integer `pid`. Both clients
must already have the matching managed addon and 200-slot pool activated. The
runner connects each, submits two async probes concurrently, and checks actor
isolation and input release. It then checks cache-off large payloads, probe
timeouts, interrupted observation continuation, and opaque interruption without
replay. Opaque tests use the exact read-only fixture, never a real side effect.
It saves WGC running/closed images, each CLI envelope, and an exclusive
`report.json`; failures retain the original connection for diagnosis/resume.
Separate installation paths test separate pools, not two clients sharing one
physical pool. Do not promote this scoped result to full multi-instance acceptance.

After a collection failure, `--resume` as the final runner argument reuses the
original report's fixed CONs and verifies target identity. Closed connections
are skipped; completed request keys are read back without re-execution. It saves
a new timestamped report and envelope files without overwriting failed evidence.
It does not replace public recovery for an unresolved interruption. All runners
decode stdout/stderr as UTF-8 streams, preserving multibyte characters split
across OS pipe reads.

## Input protection and labels

```powershell
node tests/channel-live/input-policy-baseline.mjs .tmp/channel-live/lycheedev.exe .tmp/channel-live/host.exe <new-project> <installation> <pid>
```

Use a clean managed candidate with `InputProtection` and the current probe API.
The fixture explicitly requests 12 seconds of protection, releases at 8 seconds,
and continues unprotected until completion. Four timed native WGC captures allow
inspection of `Agent 接管中` and `Agent 运行中`; screenshots are observations,
not an automated visual verdict. Assertions also cover protection deadline,
runtime error, scene-guard failure, cleanup and a subsequent unprotected probe.
`input-policy.json` and the original envelopes are retained; a failed run does
not abandon the connection. Protection has no separate fixed duration cap: it
must fit the probe's remaining execution budget.

## Automation history across reload

```powershell
node tests/channel-live/automation-history-baseline.mjs .tmp/channel-live/lycheedev.exe .tmp/channel-live/host.exe <new-project> <installation> <pid>
```

The managed addon must include `AutomationHistory`. The runner executes a
successful Unicode result and an expected error, opens the real Automation page,
verifies the retained reports and disabled replay button, then reloads and checks
the same record IDs and report bytes under a different runtime. It saves native
WGC images before/after reload and disconnects its exact session. Existing history
is preserved. Failure retains the connection and original evidence. The game-side
history is a bounded display cache in per-character SavedVariables, not the
project journal; crash durability and automatic backfill are not claimed.
The failure remains visibly labelled as an execution failure while its detail
retains acknowledgement. When the list overflows, the same inspection checks
the native slider's endpoints and narrow thumb dimensions before restoring the
top; it does not claim a physical mouse-drag test.

## Workbench pages and focus

```powershell
node tests/channel-live/focus-baseline.mjs .tmp/channel-live/lycheedev.exe <new-project> <installation> <pid>
```

The matching native candidate uses Ctrl+Alt+F12 with memory input preflight.
Connected reload and telemetry-capable explicit fallback use the same preflight.
`node tests/channel-live/reload-readiness-baseline.mjs <cli> <new-project> <installation> <pid>`
verifies no Escape when ready, cache-disabled reload, fresh runtime binding and
read-only retry of the same reload request. It reads the input journal and fails
if the observed path silently used the fixed bootstrap burst.
This fixture creates its own single-line and multiline EditBoxes. Each releases
focus only on its third Esc. Completion must observe exactly three Esc presses,
released focus and unchanged text/caret, without replaying the probe. It never
modifies an existing user editor. A 50-second watchdog clears only the fixture;
expiry fails acceptance rather than hiding an input-routing defect. No arbitrary
third-party keyboard interceptor is claimed covered by these two cases.

```powershell
node tests/channel-live/workbench-visual.mjs .tmp/channel-live/lycheedev.exe .tmp/channel-live/host.exe <new-project> <installation> <pid> after
```

The runner opens all eight pages, checks that page activation does not capture
keyboard focus, and verifies removal of Settings and native-channel QR notice
controls. It captures each page through WGC and disconnects the exact session.
`workbench.json` preserves commands and outcomes; images still require visual
inspection. The optional `before` mode captures an older candidate and explicitly
clears page-created focus, without asserting the new UI contract. It cannot turn
an incomplete run into a passed baseline.

The development-only `release-editor` host mode is limited to a pending
`runner`, `objects` or `trace` visual observation at `confirm_ready`. Under the
original project/window lock it sends Escape to release the old editor, then
wakes the already-published exact read-only confirmation slot. It never republishes
prepare/commit or replays business code; evidence is retained before input. Use
normal `live resume` afterward. A disappeared PID stays a failed recovery, never
an excuse to select another client or edit the journal.

## Two processes sharing one installation

```powershell
node tests/channel-live/shared-installation-baseline.mjs <cli> <lab-host> <discovery-report.json> <new-evidence-directory>
```

The source report must contain exactly two explicitly authorized, still-owned
connections with `name`, `installation`, `pid`, `project`, `session`, `guid` and
`product`. Both must share the same installation. It reuses their original
projects and writes new immutable envelopes in the new evidence directory.
Keep both actors out of combat for the reload/input phases; pending is a retained
recovery obligation, not a passed case.

The runner aligns runtimes with public reload, injects a six-second publication
lease hold with no game input, checks one-second pending on both original
requests, resumes them, executes 47 paired actor-tagged requests through
capacity reload, and reloads one process while the other's async probe finishes.
It closes both connections only after all assertions pass. Failure retains the
connections and reports for public recovery; never delete their journals or slot
reservations. The `hold-publication` host mode checks exact target and owner,
holds only the OS publication lease, and changes no payload or connection log.

The shared-installation and cross-client runners recognize temporary publication
contention from structured continuation data. They wait for the exact reservation
to change, or for evidence of short-lock progress, before resuming the same CON
within the original budget. Every attempt is retained. Explicit pending and
unknown-outcome assertions remain strict; these runners cannot clear a foreign
owner or renew an exhausted request.

After a complete run, `node tests/channel-live/shared-installation-audit.mjs <report.json>`
reads the current and rotated journals, verifies the original prepare nonce and
absence of repeated submitted/uncertain effects, confirms actual capacity reload
intents, and checks all 200 slots have no pending reservation and no window claims
remain. It writes a separate immutable `journal-audit.json`. The read-only audit also accepts historical 64-slot reports with 13 paired commands; new runners require 200 slots and 47 paired commands.
