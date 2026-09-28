# Live investigation

Jump to [connect](#select-and-connect), [execute and close](#complete-an-investigation),
[input recovery](#input-and-recovery), [probe design](#design-a-discriminating-probe),
[bounded probes](#write-bounded-probes), [source hypotheses](#test-source-hypotheses),
or [resume/reload](#recover-without-replay).

For authorized running-client work, the CLI owns native memory reads, 64 input
slots, process/actor identity, persistence and recovery. No wowdump or QR reader
is needed. Use the same invoking-project directory for every call; `.lycheedev/live`
holds connection journals, nonce/ticket lineage and exact result artifacts.

The in-game Automation page retains the latest 100 native operation records per
character, with up to 32 KiB of UTF-8 report preview each. Reload/logout saves
them through SavedVariables; a client crash may lose unsaved previews. The
project journal remains the durable evidence/recovery source. An empty or cleared
UI history never authorizes replay. Old running/prepared display records become
interrupted after runtime replacement; completed reports survive release and
reload. Earlier operations that never wrote UI history are not backfilled.

## Select and connect

Use `live instances --passive` for window inventory without input. Connect directly
when installation/PID are known; never choose a different character to bypass a
busy or unavailable target. The selected process creation identity and actor remain
fixed throughout recovery. One process has one logical connection and one active
CLI driver; a foreign project's durable claim is not an expired timeout.

If several candidates match and the task does not choose one, show their known
product/build, character/realm and PID (unknown identity stays unknown), then ask
which to connect. Do not pick the first process. When the user explicitly asks
to test both instances, retain both fixed targets and proceed within that scope;
there is no remaining selection question.

```text
lycheedev live connect --project <project-directory> --installation <client> --pid <pid> --character <name> --realm <realm> --wait-seconds 120 --format json
```

Retain `result.session` (CON), identity, project and journal path. An optional
`--snapshot` constrains the exact data build; source/data pinning is independent of
connection storage. A fresh bind, not a cached address or an actor descriptor,
authorizes work. `--no-cache` disables disposable address hints; invalid hints
already fall back to a fresh scan. Read [startup](live-startup.md) when activation
is pending or an installation was just updated.

## Complete an investigation

```text
lycheedev live execute --project <project-directory> --session <CON-id> --file <probe.lua> --request <stable-key> --budget-seconds 30 --wait-seconds 120 --policy observation --format json
```

Use `--probe <immutable-PRB-revision>` instead of a file for registered source.
`--budget-seconds` bounds the addon probe (1..120); `--wait-seconds` bounds this
CLI invocation (1..600, default 120), including scans and recovery. A longer host
wait does not extend probe execution. The CLI performs prepare/commit, strict
HEAD/BODY association, fresh confirmation, durable result storage and release.

Choose `observation` only when repeating the whole probe after a confirmed new
runtime is safe. Use the default `opaque` for state-changing or uncertain code.
A retry keeps the logical operation/request and records a new attempt/ticket;
it is not evidence that the interrupted attempt did nothing.

Completion requires `complete: true`, `reportState: verified` and `cleanup: complete`.
Inspect `report.ok`, `report.error`, truncation and assertions separately. A Lua
failure or probe timeout is a verified business outcome and still needs release.
A native complete result needs no ACK/finish/hide command. Original result bytes
remain in the project even after the game exits. Repeat the same request to read
it without executing again, including after later operations on that connection.

Cleanup callback failures retain the verified report and trigger a journaled
reload; a fresh binding then records `cleanupMethod: runtime_destroyed`. The
original `resourcesReleased: false` is preserved. It is distinct from a normal
release acknowledgement. Journal segments rotate only at idle boundaries and
remain linked under the connection's `.jsonl.history` directory; preserve these
alongside the active log. A missing segment must not be worked around by issuing
a new request key.

Loading a slot does not require a reload for each probe. The CLI reserves control
capacity and reloads at a quiescent boundary when needed. That reload can discard
scene state: observation probes should reconstruct their own prerequisites, or
state why the original transient scene cannot be reproduced. The input shield is
released before business code. Ordinary queries, event listeners and async waits
do not lock input: the click-through Lychee says `Agent 运行中` / `Agent running`.
Only an actual input-protection phase says `Agent 接管中` / `Agent in control`.
Neither label is proof of result retrieval.

```text
lycheedev live disconnect <CON-id> --project <project-directory> --wait-seconds 120 --format json
```

Disconnect at the end of the authorized investigation unless continued use is
needed. It verifies unbind before releasing host ownership, or uses OS proof that
the exact process lifetime ended; see [process exit](live-startup.md#ownership-and-recovery).
A repeated completed
close repairs any interrupted host retirement without new game input.
For interrupted opaque work, `closed: true` does not imply a known business
outcome: preserve `operationState: execution_unknown`, `complete: false` and the
missing report. Do not turn successful connection cleanup into task success.

## Input and recovery

The coordinator owns every input attempt. It observes current memory input
state, journals the intended action, then rechecks before sending. With an ordinary
focused editor it sends one Esc and observes again; Esc may close/cancel that UI.
Combat, secret/unavailable observations and stale samples are not permission to
send Esc. Held system keys wait for release. No foreground input or color-patch
protocol is required. Native invocation uses fixed Ctrl+Alt+F12; agents never
send it themselves or edit the slot files.

Connected reload uses the same readiness checks. It sends no Esc when input is
already clear, and re-observes after each necessary Esc. The explicit installation
fallback also uses this path when the addon advertises telemetry; see
[startup](live-startup.md) for first-install and older-runtime limits.

Input evidence is distinct from execution: `not_sent` proves zero messages were
queued; `submitted` proves only queue submission; `uncertain` (or an intent with
no outcome) does not permit replay. These facts drive CLI recovery. Do not
reinterpret them into manual retry instructions. Use the same CON/project and
`live resume`; inspect `waiting` when present. Recheck after a changed condition
within the authorized task; do not repeat indefinitely against an absent process,
foreign ownership or an unresolved opaque execution outcome.

`waiting: shared_publication` is bounded contention inside one installation,
not a lost connection. Resume the same CON/request; keep its nonce and do not
delete a slot reservation. Another driver may release its short lock normally,
but an unresolved reservation needs its original owner's recovery. A combat or
focus wait holds that slot only, not the installation's publication lock.

`complete` applies to the current command. A connected runtime does not complete
a pending disconnect, and a previous probe report does not complete reload.
The bouncing Lychee shows activity only. Ordinary probes do not block input;
short input protection says `Agent 接管中`, ordinary execution says `Agent 运行中`.
Neither animation nor a reload hint proves an accepted request.

The whole `.lycheedev/live` directory is the recovery unit. Keep connection logs,
linked history segments, content-addressed `connections/artifacts`, target metadata
and result files together. New journals reference exact code/result bytes by digest;
older inline snapshots remain readable. Missing or corrupt referenced content is
a recovery error, never a reason to generate another request key.

## Register reusable source

```text
lycheedev live probe put --name <name> --file <probe.lua> --format json
```

Names are mutable selectors; retain the returned immutable PRB revision. Use it
with native execute. Old queue-oriented atomic commands belong to legacy records
and are not the transport for CON connections.
## Design a discriminating probe

Start from the question and two or more plausible explanations. Pin the exact
client source commit and establish how it corresponds to the verified game build;
also pin the target addon's revision and record whether it is known to match the
loaded runtime. Inspect the relevant API definition, call sites, event order,
preconditions and secret/protected boundaries in
[source-research.md](source-research.md). A current branch or newest tag is not
evidence for the running build. When exact source is unavailable, identify the
gap and, within the live authorization, use a minimal capability observation
before relying on the uncertain interface.

Choose the smallest experiment that separates the hypotheses. The supported
Lua probe can inspect state, collect event timing, exercise a target addon's own
controller or callback where authorized, check object lifetime, sample bounded
performance data, or verify a business invariant. Source and task determine the
actions, fields, sample limit and assertions; the bridge does not impose a UI
template. For an interactive behavior, distinguish calling a handler from real
mouse or keyboard dispatch. A programmatic callback cannot prove hit testing,
cursor routing or protected hardware input.

Specify what must be true before each action, what output proves or refutes each
hypothesis, and what remains untested. Rebuild scene state after load when needed.
Finish observation and probe-owned cleanup before reporting. Results, logs and
assertions use a bounded memory report; the CLI verifies and durably saves its
exact bytes before releasing runtime storage. An expected assertion failure should use a clear
failure state instead of wrapping an exception as success. A verified report
proves retrieval and integrity, not that its assertions passed.

If evidence calls for a different experiment, finish or recover the old
operation first, then create a new revision and request key. An uncertain
accepted input or missing receipt is not grounds to rerun the same Lua. Keep
deadline, user cancellation, unresolved execution, probe error, assertion
failure and pending display cleanup distinct in the finding.

## Write bounded probes

For deep secret-value or secure-taint diagnosis, use the hypothesis workflow
below to choose what a probe should observe; the existing operation lifecycle
still applies.

Use Lua 5.1 and inspect only what answers the question. Bound collection sizes,
tree depth, samples and output. Synchronous Lua cannot be preempted by the host;
never rely on a host timeout as the probe's only bound.

For event- or callback-based work, the chunk receives one API as `...`:

```lua
local probe = ...
assert(probe:Async(30)) -- integer seconds, 1..120
local frame = CreateFrame("Frame")
assert(probe:OnCleanup(function() frame:UnregisterAllEvents() end))
frame:RegisterEvent("PLAYER_REGEN_ENABLED")
frame:SetScript("OnEvent", assert(probe:Callback(function()
  probe:Finish({ observed = true })
end)))
```

Use `probe:Fail("stable_reason")` for an expected failed result,
`probe:Log(...)` for bounded diagnostic lines, and `probe:IsCancelled()` before
expensive callback work. Async probes have a mandatory bounded timer, at most
16 cleanup callbacks, 100 log entries and 32 KiB of logs. Completion is
single-use. Wrap every asynchronous entry with `probe:Callback` so late callbacks
cannot enter user code after termination and exceptions become failed reports.
Register resource cleanup before activating each timer/frame. Cleanup runs on
completion, failure or timeout; cleanup failures remain pending
and prevent a false successful acknowledgement. Do
not create permanent hooks or mutate Blizzard-owned APIs.
Choose the load budget to cover the probe's own async deadline and cleanup; a
longer host wait cannot extend the declared execution budget.

### Scene prerequisites and optional input protection

For scene-dependent probes, declare a small, side-effect-free predicate with
`probe:Guard(check, events, reason)`. It must return the non-secret boolean `true`.
The runtime checks it immediately, on the listed events, before each wrapped
callback, and before successful completion. A false, unavailable, secret or
throwing predicate fails the probe with the supplied reason. At most eight
guards and eight distinct events are supported; no polling is added. Choose
events that actually cover the target or UI changes relevant to the experiment.
Preserve that failed report, then reconstruct the scene or propose a revised
experiment according to the operation's observation/opaque policy. A scene
failure is not permission to automatically replay an effect.

Do not acquire input protection for ordinary data reads, listeners or waiting.
Only an explicitly exclusive UI experiment should call
`assert(probe:ProtectInput(seconds))`. It permits one protection phase per probe,
with an Agent-chosen duration no longer than the remaining probe budget; there
is no separate five-second cap. Use
`probe:ReleaseInput()` immediately after the exclusive actions; the remainder
of an async probe runs normally. Completion and errors release before user
cleanup. The hard deadline, combat, leaving the world or Ctrl+Alt+[ release the
shield and fail the active probe rather than silently continuing unprotected.
Use `probe:IsInputProtected()` to observe this probe's lease. Late callbacks and
old lease timers cannot resume a finished probe or release a newer lease.

This is an in-game input shield, not an OS-wide lock, protection from disconnects,
or a source of secure hardware-action authority. Keep checking scene prerequisites
even while protected. Use bounded Lua: a blocked synchronous Lua callback cannot
be preempted by a Lua timer. Slot loading protects only its short dispatch burst
(two-second safety deadline) and releases before business entry. Fixed reload
retains the host's serialized, bounded input transaction; it must work even when
the addon is not loaded, so an in-game takeover indicator is not guaranteed for
that fallback. Never hold a game shield while scanning memory or awaiting reload.

Combat lockdown and secret values are trust boundaries. Check them before
comparison, formatting or branching. Record unavailable and truncated values
explicitly instead of substituting defaults.

## Test source hypotheses

Use this workflow when runtime investigation is in the user's scope and source
analysis leaves a question about actual execution. Carry the source commits,
file locations, suspected value path and unresolved condition from
[source-research.md](source-research.md#secret-values-and-secure-taint). Bind the
observations to the verified session, client build and observed addon version;
record whether the researched revision is known to match the loaded addon.
An installed file or repository commit alone does not prove that match.

Start with existing verified reports and, when useful, the bounded error snapshot
described in [error-diagnosis.md](error-diagnosis.md). State the competing
explanations and the observation that could distinguish them before writing a
probe. For example, check whether the relevant value is marked secret at an
observable boundary, or whether the prerequisite event/state occurred. A probe
must answer that specific question; the live bridge does not automatically
recover arbitrary locals or a complete historical taint chain.

Use only supported, safe observations for the verified client. For secret
values, report the secrecy marker or an explicit unavailable state, not the raw
value. Do not compare, format, serialize or coerce the value to extract it, or
re-execute a known forbidden operation just to reproduce its error. Do not add
hooks to protected/Blizzard objects or replace their functions to observe flow.
If an internal boundary cannot be observed safely, retain the gap and identify
the additional diagnostic capability or reproduction context needed.

Give each check a bounded observation window, sample/output budget and cleanup.
Prefer event-driven observation when the question depends on an event. Complete
the loaded operation, interpret and archive its verified report, then finish
that exact operation before starting the next check. Within the existing task
authorization, continue these steps without separate confirmation for each one.

Feed the observation back into the suspected source path: identify which
explanation it supports, contradicts or leaves open. Run another check only if
it can resolve a remaining material distinction; do not repeat an unchanged
probe after a timeout or absent event. A normal observation outside the failing
conditions does not establish that the failing path is safe. If a repair and
deployment are in scope, repeat the relevant bounded check against the verified
updated runtime and retain both results.

Deliver the supported path, runtime conditions and capture IDs together with
any unresolved links. Distinguish a verified probe result from a demonstrated
root cause and from a verified repair. Stop when the question is answered or
the next step requires unavailable evidence, capability or user action; finish
or recover every outstanding operation under the lifecycle above. An unresolved
cause and completed display cleanup are separate outcomes.

## Recover without replay

```text
lycheedev live status <CON-id> --project <project-directory> --format json
lycheedev live resume <CON-id> --project <project-directory> --wait-seconds 120 --format json
```

Status reads persisted evidence. Resume acquires the same connection's driver
lease, reconciles original nonces and continues within the caller's wait budget.
Exit 6 retains usable results and unfinished cleanup; continue the same task.
Do not replace a request, edit its files, clear ownership or repeat raw input to
force progress. A cached hit never bypasses identity/checksum/fresh-confirmation
validation. Incomplete scan coverage is not proof that no result exists.

After user reload, a newly discovered descriptor is only a candidate. The CLI
binds it with a fresh nonce before retiring the old runtime's reservations.
Repeatable observations can resume in a new attempt, bounded to three attempts.
For opaque execution after a possible commit, `execution_unknown` remains unknown;
inspect external postconditions using an authorized independent observation
instead of repeating the effect. A fresh runtime may prove old runtime resources
are gone without proving that the old script never changed persistent state.

A partial final journal write can be repaired under the owning driver lease only
when the entire preceding chain validates; the original bytes are retained.
Corrupt complete events, missing journals, foreign ownership, incompatible files
or an unavailable client require diagnosis. Never recreate an empty ledger to
turn uncertainty into a new operation. Report the exact blocker and retained IDs
if no safe progress remains; do not label that handoff complete.

For an intentional idle reload, use a stable request key:

```text
lycheedev live reload --project <project-directory> --session <CON-id> --request <reload-key> --wait-seconds 120 --format json
```

The single RGB patch is only a reload observation hint. The new runtime and fresh
bind are still required. The patch expires after 45 seconds or stops on wake.
An interrupted reload resumes observation and does not blindly resend `/reload`.
