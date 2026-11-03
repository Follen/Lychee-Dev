# Memory and CPU interpretation

Fix the symptom, reproduction action, addon and companion packages, client/build,
source revision, input scale, warmup and profiling/GC settings. Choose a bounded
experiment from [probe design](live-probes.md); performance work does not itself
authorize modifying the addon's business state or deploying changes.

| Measurement | What it establishes |
| --- | --- |
| Before/after memory with natural GC | A sampled difference influenced by GC and unrelated client work |
| Heap growth while GC is suspended | Uncollected allocation in that scope, not natural-running peak |
| Difference after controlled GC | Retained difference; report the global collection separately |
| Whole Lua heap or process memory | Client-wide scope, not exclusive addon bytes |
| Addon counters | Refreshed or cached addon values; include companion packages |
| Object/table counts | Structure; shared roots and native allocations limit byte attribution |
| Peak or first-run sample | Only the sampled maximum or first observed round; cold startup needs a verified fresh runtime |

A negative delta is not negative usage. Hiding a frame does not prove release,
and rebuilding a catalog does not reset game caches. Measure observer work when
it matters: memory refresh, GC, traversal, timers, serialization and report UI.
A global pause is not exclusive addon CPU; sampled frames are not all frames.

Keep inputs and boundaries comparable. Separate acquisition, copying, indexing,
querying and rendering; label inclusive timings instead of adding parents to
children. Report total latency, longest observed synchronous segment, sample
count and timer resolution. Offline replay does not establish game latency.

For Live transport, separate process/module setup, Lua-root discovery, named
Mailbox reads, readiness/input wait, probe execution and result retrieval.
Keep window state, FPS, loaded capability and candidate binary fixed. Same-build
processes remain different targets. A successful named read is not whole-process
memory coverage, and a fast read does not imply fast end-to-end completion.

Prefer isolated replay objects for registry/index tests and prove they do not
alias live mutable state. Preserve and restore owned profiling settings. Do not
clear real registries, unregister providers, replace global functions or install
permanent hooks for a timing sample. Change GC policy only when its initial state
can be established and restored; full GC is an explicit diagnostic intervention.

Retain raw values, failed rounds, observer cost, correctness checks and cleanup.
Distinguish live, replay, missing dependencies and untested conditions. Lua mocks
or syntax checks do not establish native timing, rendering, login or combat.
A reduction must preserve business behavior and account for retained roots,
companion packages and restoration cost; moving work elsewhere is not a saving.
