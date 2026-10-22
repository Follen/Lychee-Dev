# Bounded memory and CPU investigations

Read for memory, CPU, stutter, startup or repeated-operation cost. Use
[live delivery](live-investigation.md) within the existing task; measurement does
not authorize business-state changes or deployment. Retain earlier evidence.

## Define the measurement

Record symptom/action, addon and companions, client/build, source, input scale,
warmup and profiling/GC settings. Compare matching candidate/target workloads;
a design target is not a measured property. Separate total latency from the
longest sampled synchronous segment and report timer resolution/sample counts.
Nested timings are not additive unless disjoint.

- Natural-GC before/after deltas mix collection and unrelated work
- GC-suspended growth measures uncollected allocation, not natural peak
- Controlled-GC deltas measure retained difference in that scope; collection is
  a separate intervention, and negative deltas are not negative memory usage
- Whole Lua/process memory is client-wide, not per-addon attribution
- Addon counters may be cached; state whether refreshed and include companions
- Counts, sampled peaks and first observed rounds do not prove total native cost,
  true maximum or cold startup without verified startup conditions

Start from an uninstrumented baseline. Account for observer cost: refresh, GC,
traversal, timers, serialization and reports. Global pauses are not exclusive
addon CPU; sampled frame timings are not total interval CPU. Offline replay does
not prove game startup latency. Hiding frames does not prove release.

For transport measurements separate root lookup, readiness/waits, input, Lua,
retrieval and flush when observable. Guarded module-code/Mailbox lookup is not
heap scanning; coverage gaps remain. Current no-cache compatibility does not
select another lookup algorithm. Keep window/FPS/runtime/binary fixed. Input-free
readback of a completed request is not a new execution timing. No receipt after
submission neither proves nondelivery nor permits another attempt.

## Isolate, bound and restore

Prefer private nonaliasing replay for registry/index experiments. Do not clear
live indexes, unregister providers, replace global APIs or install irreversible
hooks for a measurement. Preserve/restore owned profiling state on every exit;
change GC policy only if its prior state can be established and restored.
If isolation/restoration is uncertain, keep the scenario untested.

Use [bounded probes](live-probes.md#write-bounded-probes), including each batch and
output, late-callback invalidation and cleanup before report construction.
A timer cannot preempt synchronous native work. Syntax checks/isolated harnesses
can test failures and restoration, not real taint, timing, combat or rendering.
Do not replace CLI delivery with a huge manual paste.

Retain failed rounds and raw deltas as well as successful observations. Separate
live, replay, missing dependency, timeout and untested cases. A verified report
with failed assertions is not successful validation. Recover pending cleanup on
the same CON and retain any restoration gap. A proposed reduction must preserve
business behavior and account for retained roots/companions and moved costs.
Continue only experiments that resolve a material uncertainty, not a permanent sampler.
