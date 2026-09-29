# Live investigation

Use live work to resolve a question about the running client that existing source,
data or retained reports cannot answer. Keep those evidence types distinct: source
explains possible behavior, data supplies versioned records, and a live observation
establishes what happened on the selected client. Carry exact source/data pins into
the experiment; a data build selection does not select a game process.

1. Existing CON: read status and retained results first. Resume unfinished work
   through [recovery](live-recovery.md). If it is idle and still matches the
   authorized target, reuse it for the next distinct task.
2. New live task without a CON: select and connect below. Read [startup](live-startup.md)
   only for installation, activation or contact without a usable runtime.
3. Need new evidence: use [probe design](live-probes.md), [retained errors](error-diagnosis.md)
   or [performance measurement](runtime-investigations.md) according to the question.
4. Inspect the verified report, finish cleanup, and disconnect when done.

The CLI owns native input, memory/optical observation, slot coordination and
recovery. Agents do not send keys, decode colors or manipulate slot/claim files.
Keep the invoking project's `.lycheedev/live` evidence directory together.

## Select and connect

Use `live instances --passive` for window inventory without input. Connect directly
when installation/PID are known; never choose a different character to bypass a
busy or unavailable target. The selected process creation identity and actor remain
fixed throughout recovery. One process has one logical connection and one active
CLI driver. Same-build instances still have separate PID/CON/request identities;
shared installation files do not make them interchangeable. A foreign project's durable claim is not an expired timeout.

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
result verification, durable storage and runtime release.

Recovery has persistent deadlines; resume does not reset them. A CLI timeout
ends the host call, not the Lua probe. Read [budget and evidence rules](live-recovery.md#durable-evidence-and-older-journals)
when recovery is interrupted or exhausted.

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

A verified report with pending cleanup is usable evidence plus a recovery
obligation. Keep it and follow [recovery](live-recovery.md), without rerunning Lua.

The CLI may reload at a quiescent capacity boundary, so probes must reconstruct
scene prerequisites or declare them unreproducible. Ordinary probes do not lock
input; activity labels do not prove acceptance or result retrieval.

```text
lycheedev live disconnect <CON-id> --project <project-directory> --wait-seconds 120 --format json
```

Disconnect at the end of the authorized investigation unless continued use is
needed. It verifies unbind before releasing host ownership, or uses verified
process-exit/runtime-replacement evidence for input-free retirement; see
[ownership and recovery](live-startup.md#ownership-and-recovery).
A repeated completed
close repairs any interrupted host retirement without new game input.
For interrupted opaque work, `closed: true` does not imply a known business
outcome: preserve `operationState: execution_unknown`, `complete: false` and the
missing report. Do not turn successful connection cleanup into task success.

## Design a discriminating probe

Use [probe design](live-probes.md#design-a-discriminating-probe) before creating
new Lua: pin the relevant source and define the observation that separates hypotheses.

## Write bounded probes

Use the [bounded API and cleanup rules](live-probes.md#write-bounded-probes).
A host timeout cannot preempt synchronous Lua; ordinary observation needs no input shield.

## Test source hypotheses

Carry the unresolved source path into [live hypothesis testing](live-probes.md#test-source-hypotheses),
then return the observed result to that path. Do not infer source correctness from
an unrelated successful probe.

## Input and recovery

For input blockers, same-installation contention or uncertain input, read
[input recovery](live-recovery.md#input-and-recovery). One original CON/request
continues serially; unknown input is never replay permission.

## Recover without replay

Read [recovery](live-recovery.md#recover-without-replay) before resuming or reloading.
Pending alone does not call for reload; unchanged blockers do not justify a retry loop.
