# Bounded memory and CPU investigations through Run

Use this reference when a Lychee Dev investigation concerns memory, CPU, stutter, startup or repeated operations. The output is a question-specific script and evidence interpretation, not a replacement background profiler.

## Define the question before the probe

Read the target project's applicable performance rules, lifecycle design and existing raw evidence. Record the current task's authorization separately: diagnosis, proposal, implementation and deployment are different scopes. A design target is not a verified property.

Establish the symptom, reproduction action, target addon and companion packages, exact client/build, addon/source revisions, language, data scale, and current profiling/GC settings. Identify already available Tickets before requesting another run. State a falsifiable hypothesis and the observation that would distinguish it from alternatives.

Check actual third-party integrations and dependency versions. An installed or referenced addon is not automatically exercised by a probe. Use statuses such as `live`, `replay`, `missing_dependency`, `failed`, `timeout`, and `not_tested` per scenario. Preserve raw evidence and failed rounds rather than selecting the fastest result.

## Measurement meanings

| Observation | Required interpretation |
| --- | --- |
| Memory before/after with natural GC | A live observation influenced by collection and unrelated client work; not cumulative allocation or retained bytes |
| Heap growth with GC suspended | Uncollected allocation within that measured scope; not a natural-running peak or target-addon attribution |
| Heap difference after controlled GC | Retained difference for the measured scope; report global collection separately and keep the original values |
| Peak | Maximum actually observed at a stated sampling frequency; unobserved peaks remain unknown |
| Whole Lua heap / process memory | Client-wide scopes; neither equals one addon's Lua tables or native UI cost |
| Addon memory counters | State whether refreshed or cached, include companion packages, and do not equate object counts with bytes |
| Reachable object or table counts | Structural evidence and ownership clues; shared roots, unreachable closures and native allocations prevent complete byte attribution |
| Warm repetition | Previously used code and caches; rebuilding a Lua catalog does not reset game-internal caches |
| First observed round | First in this test, unless a real fresh session and startup path were verified |

A negative post-GC delta may include earlier garbage. Report no observed retained growth and the raw delta; do not advertise negative memory usage. Count creation, allocation, retained growth and native object high-water marks separately. Hiding a Frame does not prove its memory is released.

## Separate work and observer cost

Distinguish these phases when relevant: game API acquisition, temporary capture, normalized business records, SDK validation/copy/registration, indexing, query, UI construction/rendering, Lua source compilation, native addon loading and cleanup. Mark nested timings as inclusive or exclusive; do not add a parent interval to its children. Report total completion latency and the longest uninterrupted synchronous segment, with sample count and timer resolution.

Build a baseline without instrumentation first. Measure control/observer work separately: global memory refresh, full GC, hook installation and dispatch, traversal, timer scheduling, serialization and UI refresh. A global pause is not the target addon's exclusive CPU. A profiler's addon attribution may depend on execution/driver ownership; a source filename alone is insufficient evidence. Sampled frame percentiles do not describe all frames, and the sum of periodically sampled frame CPU values is not interval CPU.

Do not default to scanning `_G`, all Frames or every object on a timer. The removed Lychee Dev capture performed global Frame enumeration and global memory refresh during capture, showing how observation can scale with unrelated UI. Code-path counts demonstrate that work; they do not prove which native API caused all user-visible stutter. Add a broader or instrumented probe only to test a specific remaining hypothesis.

Use the same inputs, runtime, warmup and measurement boundaries for comparisons. Do not combine offline SDK/index timings with live acquisition timings to calculate game first-open latency. An instrumented hotspot experiment and an uninstrumented latency baseline answer different questions; show both without treating instrumentation overhead as application cost.

## Ownership before execution

Identify the authoritative Provider/source, current registry/index, consumer-held snapshots, persistent data, queues, subscriptions, timers and UI/native roots. Prefer a private replay instance when exercising SDK or index operations. Validate that real Provider counts/identities, index generation, business records, settings and relevant saved data remain unchanged; a private instance must not retain or mutate live record tables by alias.

Do not unregister real Providers, clear a live index, overwrite callbacks, invoke business actions, or replace global APIs just to create an experiment. If safe isolation or exact restoration is unavailable, mark that scenario untested and use a different probe. A weak-reference recovery check must not keep the test's own strong references alive.

Do not install irreversible `hooksecurefunc`/Blizzard hooks for temporary profiling. Prefer existing counters or wrappers inside an isolated harness. Save and restore any owned hook/profiling state in all exit paths. A disabled guard on an irreversible hook is not equivalent to removing it. Avoid GC policy changes unless the initial state can be established and restored on this client; Lua 5.1 does not provide every modern Lua GC query. Full GC is a separately identified diagnostic intervention, never a proposed production close handler.

## Choose the delivery surface before measuring

A complete study is not a giant pasted source bundle. Record the actual input's bytes, line count and longest line before delivery. Syntax and isolation checks do not test native EditBox paste/layout behavior at that size. Do not hide volume in compressed, encoded or obfuscated strings. Use only a size budget supported by actual input-path evidence; do not invent a universal safe threshold.

Use Run for compact, question-specific probes. When many source files or a complex scenario harness are needed, consider an independent, opt-in diagnostic addon/carrier with normal TOC loading and a short explicit start command. Explain installation, exact load/reload requirements, dependencies, output, stop/timeout behavior and cleanup. This is a scale-based choice, not the default for every diagnosis. Loading must use verified client capabilities; the game cannot be assumed to read arbitrary external files or hot-load newly written Lua. Do not install or enable such a carrier outside the current authorization.

If a user reports an immediate paste hang before Run was clicked, investigate the input path first. Lychee Dev's editable multiline box currently has no explicit input-size cap and requests scroll geometry updates on TextChanged; its cursor update is deferred. There is no syntax coloring, line-number pass, compile, execution or history save during paste. Native paste/layout may still be expensive; the Lua path alone does not establish the stall's exact cause. Stop giant-paste retries and choose a lighter entry point. Do not blame the script's business work or GC if it never ran.

Pasted text stays in the current process's edit box, including when the same window is reopened, but is not automatically persisted or executed. History is written after an explicit Run; the existing history-code truncation is a storage rule, not proof of safe paste size. A client restart/reload must not be described as automatically replaying pasted scripts.

## One entry point, one bounded scenario matrix

Before writing code, enumerate the scenarios and how each is reached. A useful matrix contains:

```text
scenario id | question | live/replay/dependency | input size | phases
precondition | work/duration/output limits | external action
correctness invariant | cleanup/restore checks | expected evidence
```

Combine the agreed automatic scenarios into one coordinated Lua 5.1 test and one report, using the delivery surface chosen above. Use finite phases with both per-batch work limits and an overall deadline, bounded traversal depth/node counts and output/retention limits. Do not install a perpetual sampler. Check combat and cancellation at phase boundaries, invalidate late callbacks by run identity, and stop all owned work before final publication. A deadline cannot preempt one long native call; exclude it or disclose that limitation.

Avoid reentry: a second execution must reject an active run or safely cancel and clean it up. Preserve the last complete result until the next run can publish a complete replacement. Missing dependencies and failed phases are evidence, not reasons to silently return an empty successful report.

Keep measured work separate from report construction, equality checks and export serialization. Run validation outside measured intervals and record its result. Report collection/filter counts, source identities and business equivalence where needed, without executing live actions to check their payloads.

A synchronous Run return is serialized when execution returns. For staged/asynchronous work, inspect the installed runner/export source and validate a completion writer before starting; do not assume a global addon namespace or a stable private API. Use one final saveable result or Ticket. If the runner's size limits would truncate the report, use a verified full-evidence export path; an on-screen excerpt is not a complete artifact. Do not solve this by silently adding another paste step to an agreed one-paste workflow.

Cold login, real combat, manual user actions and external dependencies cannot be simulated into verified coverage. One script can orchestrate or label those boundaries, but it must report what actually occurred. If another task owns a pending game probe, coordinate the runtime slot rather than altering its script, clipboard or session.

## Validate before giving the script to the user

Validate the actual delivery path and scale separately from execution. Use `luac -p <probe.lua>` or another Lua 5.1 parser, then run a bounded offline harness at the actual entry/completion seam. Exercise normal completion, missing dependencies, errors during phases, timeout, cancellation/combat, repeat execution, stale callbacks, live-state isolation, cleanup/restoration and complete export. A syntax check alone cannot prove any of these. Use controlled mocks for failure injection; do not reproduce a severe freeze in the live client as the first test.

Report actual validation results and limits. The harness can verify sequencing and work budgets; it cannot establish native API duration, WoW GC behavior, taint safety or real rendering. Use instrumented and uninstrumented runs separately. If the script cannot safely cover a requested scenario, narrow that scenario explicitly instead of shipping an unvalidated destructive fallback.

Announce clipboard delivery and verify readback if clipboard use is authorized. Keep the delivered version stable until executed or explicitly replaced. Inspect available computer-control capabilities before use; after one unsuccessful focus/state correction, deliver one manual paste rather than retrying blind game input. Ask only for the minimum external action that remains.

## Read and decide

Use the exact Ticket received by the owning task. Validate record-level `schema`, `source` and `environment`, then read the entire `payload.content` and separately validate any embedded report schema, script version, scenario statuses and cleanup results. Metadata may be incomplete and is never a substitute. If persistence is pending, identify the one required `/reload` or logout step; do not misreport an in-memory Ticket as missing evidence.

Separate observed facts, causal hypotheses, feasible options and unverified goals. For a memory reduction proposal, include the fixed-cost floor, recoverable source, all-package retention, restoration latency, complete query results, business equivalence and UI experience. Moving costs between packages or hiding results cannot establish success. If a target is infeasible under current evidence, explain the retained roots and trade-offs rather than weakening correctness to make the number pass.

Choose a next experiment only if it resolves a specific uncertainty. Do not run an endless collection loop once the user can make the requested decision.

## Reference discipline

- Read the target project's `PERFORMANCE.md`, lifecycle design and validation records as project policy/evidence, preserving their stated environment and verification status. Do not hard-code a past project's memory target, local paths, provisional SDK version or plan-only authorization into future tasks.
- EllesmereUI at commit `8da5dfe182c7809e3f61b5a9ca856d16b891726f` provides concrete references: [contribution criteria](https://github.com/EllesmereGaming/EllesmereUI/blob/8da5dfe182c7809e3f61b5a9ca856d16b891726f/.github/CONTRIBUTING.md#L22-L44) for opt-in/idle cost, and [ticker ownership and unsubscribe behavior](https://github.com/EllesmereGaming/EllesmereUI/blob/8da5dfe182c7809e3f61b5a9ca856d16b891726f/EllesmereUI_Ticker.lua#L8-L115). Treat its attribution comments as upstream evidence to verify for the measured build, not a universal engine guarantee. Its single-client policy does not override Lychee Dev's client matrix.
- Verify API facts against the target's locked client source using wowdoc when needed. Tags can move: retain and use the resolved immutable commit recorded by the project. A native API's existence does not prove a particular timing or memory attribution claim.
