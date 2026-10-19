# Live probe design and implementation

Read only when new Lua can answer missing evidence. Use the [live workflow](live-investigation.md)
for target binding and closure. Reusable probe names are mutable; retain the
immutable PRB revision. Registration stores source and does not execute it.

## Design a discriminating probe

For a simple state query, inspect only the required fields/APIs. For diagnosis,
identify the remaining explanations and a small observation that separates them.
Match fixed client source/addon revision to the verified runtime; latest source
and installed files alone do not prove what loaded. If an interface is unknown,
retain the gap and use a safe capability observation within live scope.

A supported experiment can inspect state, events, lifetime, a target addon's own
controller, bounded cost or business invariants. A programmatic callback is not
proof of actual hardware dispatch, hit testing or protected input. State relevant
preconditions and what remains untested. Finish observation and owned cleanup
before reporting; assertions that fail must not be wrapped as success.

Choose observation policy only when the whole chunk, including setup/cleanup,
is safely repeatable after verified runtime loss. It starts a new attempt, not
an old Lua stack. Rebuild transient scene state each time. Persistent or business
effects default to opaque. Neither policy allows manual uncertain-input replay
or deadline renewal. Close/recover the old operation before a different experiment.

## Write bounded probes

Use Lua 5.1. Bound synchronous work, collection size, depth, samples and output;
a host timeout cannot preempt synchronous Lua. For callbacks the chunk receives
one API as its vararg:

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

This observes a future event, not prior combat. Verify the relevant preconditions.
Use Fail for an expected failure, Log for bounded diagnostics and IsCancelled
before expensive callback work. Async accepts 1–120 integer seconds; maximums
are 16 wrapped callbacks, 16 cleanup callbacks, 100 log entries and 32 KiB logs.
Wrap every async entry with Callback; register cleanup before activating work.
Cancel owned timers, unregister events and detach scripts; hiding is insufficient.
Cleanup runs in reverse order and completion is single-use. A cleanup failure
can preserve the report while requiring CLI recovery/runtime destruction.
Set the execute budget to cover the async deadline and cleanup.

A scene-dependent Guard(check, events, reason) must return non-secret true.
It is checked immediately, on listed events, before wrapped callbacks and before
success. False/unavailable/secret/throwing checks fail the probe. Limits are eight
guards/eight distinct events. Choose events that cover the actual scene changes;
failed prerequisites do not authorize an effect replay.

Only an explicitly exclusive UI experiment needs ProtectInput(seconds); ordinary
reads/listeners/waits do not. It allows one phase within the remaining budget;
ReleaseInput promptly. It is an in-game shield, not OS control or hardware-action
authority. Combat, leaving the world, cancellation/deadline release it and fail
the probe. A synchronous stall can prevent timers from running. Keep scene checks
and never hold the shield while waiting for reload or scanning memory.

Do not install permanent hooks or mutate Blizzard-owned APIs. Check combat and
secret/protected boundaries before comparison, formatting or branching. Preserve
unavailable/truncated values instead of substituting defaults.

## Test source hypotheses

Carry [fixed source/value-flow evidence](source-value-flow.md), version identity,
conditions and missing links into the check. Consult [retained errors](error-diagnosis.md)
first when relevant. The bridge cannot recover arbitrary locals or a complete
historical taint chain. For secrets, report the marker or unavailable state,
never raw/coerced values. Do not repeat known forbidden operations or hook/
replace protected Blizzard objects to observe them.

Bound the observation window/output and cleanup, then read the verified report's
business outcome separately from transport and closure. Reconcile it with the
suspected path. A normal observation outside failing conditions cannot disprove
the fault. Another experiment must resolve a material remaining distinction;
a timeout or absent event alone is not a reason to repeat unchanged.
If repair/deployment is authorized, verify the updated runtime and repeat the
relevant check with both results retained. Root cause, successful probe and
verified repair are separate claims. Stop when answered or blocked; recover any
outstanding lifecycle even when the cause remains unknown.
