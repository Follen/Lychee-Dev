# CPU and memory investigation

Record workload, addon/companions, client/build, source, input scale, warmup and
profiling/GC settings. Compare matching workloads from an uninstrumented baseline.
Separate total latency from sampled synchronous segments; report resolution,
samples and observer cost. Nested timers are not automatically additive.

Natural-GC deltas mix collection/unrelated work; GC-suspended growth measures
uncollected allocation; controlled-GC deltas measure retained difference, with
collection a separate intervention. Process/Lua totals are client-wide, and addon
counters may be cached. Counts/sampled peaks do not establish full native cost,
true peak or cold startup. Hiding UI does not prove release.

Prefer private, nonaliasing replay for index experiments. Bound each batch,
observation window and output; invalidate late callbacks and restore owned state
on every exit. Do not clear live indexes, replace global APIs or install permanent
hooks. Change GC policy only when its prior state can be established/restored.
A timer cannot preempt synchronous native work.

Use the [Live workflow](live-investigation.md) and [probe lifecycle](live-probes.md#write-bounded-probes).
Transport waits and input-free retained-result reads are not Lua execution time.
Keep failed rounds/raw deltas and distinguish live, replay, timeout and untested
scopes. Continue experiments only while they resolve a material uncertainty;
finish owned cleanup even when the cause remains unknown.
