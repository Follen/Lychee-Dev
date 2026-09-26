# Bounded memory and CPU investigations

Use this reference for memory, CPU, stutter, startup or repeated-operation
questions. Deliver authorized probes through
[live-investigation.md](live-investigation.md); retain existing evidence before
considering another run. Performance investigation does not authorize changing
the target addon's business state or deploying changes.

## Define what the measurement can establish

State the symptom, reproduction action, target addon and companion packages,
client/build, source revision, input scale and profiling/GC settings. Choose a
specific hypothesis and the observation that distinguishes it from alternatives.
Read the target project's performance rules and existing evidence; a design
target is not a measured property.

| Observation | Interpretation |
| --- | --- |
| Memory before/after with natural GC | Influenced by collection and unrelated client work; not cumulative allocation or retained bytes. |
| Heap growth with GC suspended | Uncollected allocation in that scope; not a natural-running peak or one addon's exclusive usage. |
| Difference after controlled GC | A retained difference in the measured scope. Report the global collection separately and retain raw values. |
| Whole Lua heap or process memory | Client-wide scope; neither identifies one addon's tables or native UI cost. |
| Addon memory counters | State whether refreshed or cached, and include relevant companion packages. |
| Object/table counts | Structural evidence; shared roots and native allocations prevent complete byte attribution. |
| Peak or first-run result | Only the maximum actually sampled, or first round observed. State sampling interval; claim cold startup only after verifying a fresh session and startup path. |

A negative post-GC delta is not negative memory usage. Hiding a frame does not
prove its memory was released. Rebuilding an addon catalog does not reset
game-internal caches.

## Separate target work from observation

Start with an uninstrumented baseline. Measure observer work separately when it
matters: global memory refresh, full GC, traversal, timers, hooks, serialization
and report/UI construction. A global pause is not exclusive CPU attributed to
the target addon. Sampled frame timings do not represent all frames or total
interval CPU.

Keep input, warmup, runtime and measurement boundaries comparable. Separate API
acquisition, copying/validation, indexing, queries and rendering where relevant;
mark nested timings inclusive or exclusive rather than adding parents to their
children. Report total latency and the longest observed synchronous segment,
with sample count and timer resolution. Offline replay timings cannot establish
game startup latency. If the hang occurs during input before execution, examine
the input path rather than attributing it to the probe's business logic.

## Preserve live ownership and bound the experiment

Prefer a private replay instance for registry/index experiments. Check that it
does not alias live mutable records. Do not unregister real providers, clear
live indexes, overwrite callbacks, invoke business actions or replace global
APIs merely to obtain a measurement. If isolation or exact restoration cannot be
established, mark the scenario untested and choose another observation.

Do not install irreversible hooks for temporary profiling. Preserve and restore
owned profiling state on every exit. Avoid GC policy changes unless its initial
state can be established and restored on the actual client; full GC is an
explicit diagnostic intervention, not a proposed production cleanup strategy.
Use existing counters or bounded traversal before scanning all globals/frames.

For each scenario record its preconditions, live/replay/dependency status,
duration/work/output bounds, correctness invariant and cleanup checks. Use the
current probe API (`probe:Async`, `probe:OnCleanup`, `probe:Finish` or
`probe:Fail`) described in the live reference for callback-based work. Bound each
batch as well as the whole run, invalidate late callbacks and stop owned work
before publishing. A deadline cannot preempt a long synchronous native call.
Keep measurement separate from report construction and invariant checks.

Validate the actual source with Lua 5.1 and exercise meaningful failure paths
in an isolated harness: missing dependencies, timeout, cancellation, late
callbacks, repeated execution, restoration and full output. A syntax check or
mock cannot prove native timing, taint safety, rendering, login or combat
behavior. A large harness also needs input/loading-path validation at its actual
size; do not substitute an enormous manual paste when CLI delivery is available.

## Report the evidence and its limits

Read the verified report and retain its operation/capture IDs. Distinguish live,
replay, missing dependency, failed, timeout and not-tested scenarios. Keep failed
rounds and raw deltas, not only the fastest result. Describe attribution limits,
observer cost and any incomplete restoration. Complete the normal ACK and
receipt cleanup even when the probe produced a verified diagnostic error.

A memory-reduction proposal must also account for retained roots, companion
packages, restoration latency and business equivalence; moving cost elsewhere
does not establish a reduction. Continue only with experiments that resolve a
specific uncertainty, without turning a bounded investigation into a permanent
sampler.
