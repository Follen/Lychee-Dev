# Live probe design and implementation

Read this when writing a probe or testing a source hypothesis. Use the
[live workflow](live-investigation.md) for target selection, delivery and closure.

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

