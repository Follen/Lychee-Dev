# Live probe design

Read this when writing Lua, not merely reading an existing report or recovering
a connection. Discover exact registration/execution flags from the installed
CLI. A probe name is a mutable selector; retain its immutable PRB revision.

Choose the smallest observation that answers the question. For diagnosis, state
the competing explanations and the observation that separates them. Carry fixed
source locations, client/build and loaded addon revision into the experiment.
Installed files alone do not establish the loaded revision. A handler call does
not prove hardware input, hit testing or cursor routing.

Choose `observation` only when the entire chunk, including setup and cleanup, is
safe to repeat after verified runtime loss. Persistent settings or business
actions normally require `opaque`. Neither permits manual replay of uncertain
input or extension of the original durable deadline.

## Write bounded Lua

Use Lua 5.1. Bound synchronous work, collection sizes, tree depth and output;
a host deadline cannot preempt a blocked Lua call. For asynchronous work the
chunk receives the probe API as `...`:

```lua
local probe = ...
assert(probe:Async(30))
local frame = CreateFrame("Frame")
assert(probe:OnCleanup(function()
  frame:UnregisterAllEvents()
  frame:SetScript("OnEvent", nil)
end))
frame:RegisterEvent("PLAYER_REGEN_ENABLED")
frame:SetScript("OnEvent", assert(probe:Callback(function()
  probe:Finish({ observed = true })
end)))
```

The example observes a future combat-exit event; establish the actual
precondition when the question requires it. `Async` accepts integer seconds
1..120. Use `Fail("stable_reason")` for an expected failed result, `Log(...)` for
bounded diagnostics and `IsCancelled()` before expensive callback work.

Wrap every asynchronous entry with `Callback`. Register cleanup before enabling
each timer/frame; unregister events, detach scripts and cancel owned timers.
Hiding a frame alone leaves callbacks and captured values attached. Completion
is single-use; late callbacks cannot reenter the finished probe. Cleanup runs in
reverse order on completion, failure or timeout. Limits are 16 callbacks,
16 cleanup handlers, 100 log entries and 32 KiB of logs. Choose the execute budget
to cover the probe's own async deadline and cleanup.

Use `Guard(check, events, reason)` for a side-effect-free scene predicate returning
the non-secret boolean true. It runs immediately, on selected events, before
wrapped callbacks and before success. At most eight guards and eight distinct
events are supported. Choose events that cover the relevant scene changes.
A failed guard preserves a failed report; it does not authorize replay.

## Input and trust boundaries

Ordinary reads and listeners need no input shield. An explicitly exclusive UI
experiment may use `ProtectInput(seconds)` for one phase within its remaining
budget, then `ReleaseInput()` immediately after the exclusive action.
`IsInputProtected()` reports the probe's lease. Completion, error, combat,
world exit or cancellation releases protection. It is an in-game shield, not
OS-wide control or secure hardware-action authority. No separate cancel binding
exists. Never hold it while awaiting reload or scanning host state.

Check combat, forbidden/protected objects and secret markers before comparison,
formatting or branching. Report an unavailable value instead of coercing it.
Do not replace Blizzard functions, install permanent diagnostic hooks or write
protected frames to observe a path. Existing error reports can guide the next
check; the bridge does not recover arbitrary historical locals or taint chains.

## Interpret and finish

A verified report proves retrieval and byte integrity; `report.ok` expresses its
business result. Preserve expected failures, missing events and truncation.
Finish the original operation's cleanup and connection closure, retaining useful
reports when recovery is pending. Follow returned status/recovery instructions;
manual startup activation requires one user reload in the selected window.

Feed observations back into the source hypothesis. A normal run outside the
failing conditions does not establish safety there. Run another experiment only
when it distinguishes a remaining material cause, and use a new probe revision
and request after the old operation is finished or recovered.
