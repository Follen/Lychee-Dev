# Run-based runtime investigations

Lychee Dev provides the Run page and a common evidence workflow. The `wowdev` skill helps an Agent turn a specific question into a bounded Lua investigation; the addon does not supply a general performance monitor.

## One investigation, one report

1. Tell the Agent what feels wrong, how to reproduce it, and which addon/client is involved. Include an existing Ticket if available.
2. The Agent inspects the relevant source and prepares one Lua 5.1 script for the agreed scenarios. The script identifies its version, limits, dependencies, measurement scope, and cleanup path. It must preserve real addon state and user data.
3. Open `/dev` > **Run**, paste the reviewed script, and execute it once outside combat. A short investigation returns a structured report. A staged investigation must use a completion/export path verified against the installed addon; returning while callbacks are still running does not capture the final report.
4. Save the completed result, copy its `LYCHEE-...` Ticket, and reload the UI or log out to write SavedVariables. Give the Ticket to the task that owns the investigation.
5. The Agent validates the record's schema, source, client/build and full `payload.content`, then reports findings, limitations and a concrete next decision. Metadata alone is not the evidence.

A complete investigation does not require pasting all of its implementation source. Use Run for compact probes; source-heavy studies can use a separately installed diagnostic addon and a short entry command, with verified TOC loading, dependencies and cleanup. Do not assume arbitrary file access or hot loading inside WoW, and do not compress a giant bundle to hide its size. Syntax checks do not establish native paste responsiveness; no universal safe size is claimed.

One entry point covers the scenarios the test can actually run. A real cold login, entering combat, or interacting with another addon may still require an explicit user action. Missing dependencies, replayed data, failed phases and untested scenarios must remain visible in the report.

## Measure the question

For memory, distinguish retained memory after controlled collection, allocation while collection is suspended, natural-GC observations, and peak memory. A whole-client heap delta or an object count is not the target addon's byte usage. Keep all companion packages in the comparison scope.

For latency, separate game API acquisition, record construction, SDK processing, indexing, query, UI creation and source/native loading. Report the longest synchronous segment as well as total elapsed time. Rebuilding from already loaded code does not create a cold game cache. Do not add offline stage timings to in-game timings to invent a first-open result.

Global memory refresh, full GC, hooks, object walks, instrumentation, formatting and UI updates can disturb the measurement. Start with the smallest read-only baseline, measure observer work separately, and only add an intrusive probe when it answers a stated hypothesis. Do not make global pauses look like target-addon CPU. Any temporary settings or owned resources need a tested cleanup path; irreversible hooks are unsuitable for a throwaway probe.

Before handing over a script, the Agent checks Lua 5.1 syntax, normal completion, missing dependencies, failure/timeout, repeat execution, state isolation and cleanup. Budgets must cover work per batch, total duration, traversal and retained output. A Lua script cannot preempt a single long native call; if that call is required, its limitation must be explicit.

## Existing evidence and settings

The Performance page, capture sampler, health snapshot and function benchmark tools have been removed. Existing performance reports stay readable in **Saved Records** under their original Tickets. Run, Objects, Events, Trace, Errors and generic evidence export remain available.

Removing the tools does not change global client settings. If an older session enabled `scriptProfile`, a normal comparison may require `/console scriptProfile 0` followed by `/reload`; record the chosen setting rather than silently changing it.

The evidence format and persistence boundary are documented in [EvidenceProtocol.md](EvidenceProtocol.md). Current client baselines are in [Compatibility.md](Compatibility.md).

## Input-path limitation

The current Run editor is a native multiline EditBox with no explicit input-size cap. TextChanged reads scroll geometry and updates the scrollbar; cursor scrolling is deferred. Paste does not trigger compilation, execution, syntax coloring or history saving. Large input can still impose native text/layout work. The reported immediate hang while pasting a large study has not been reproduced in a controlled client session, and its precise native cause is unconfirmed. Do not retry that large paste or interpret a storage truncation limit as a validated input budget.
