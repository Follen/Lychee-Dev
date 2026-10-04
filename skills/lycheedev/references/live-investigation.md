# Live investigation

Use this for evidence about the running client. Source describes possible
behavior; static data and Hotfix captures describe fixed records. A live
observation establishes what happened on its selected client and conditions.

Start with retained state: status/results for an existing CON, [recovery](live-recovery.md)
for unfinished work, or select/connect below for a new task. Reuse a matching
idle CON. Read [startup](live-startup.md) when activation or installation is needed.
Use [retained errors](error-diagnosis.md) or [performance measurement](runtime-investigations.md)
when they answer the question; write a [probe](live-probes.md) only for missing evidence.

This candidate uses **Lychee Dev mailbox protocol v1**, schema
`lycheedev.mailbox.v1`, a build-bound Lua root and the named public Mailbox, with a
matching clean managed installation. The CLI validates the current process,
typed path, publication, owner/fence, challenge and exact result. Old native
wire/identity is rejected. Unsupported code/layout, ambiguity or read gaps
fail closed; they do not invoke a heap scanner or reuse another build's RVA.
`--no-cache` follows this same path. Raw address research is a separate task.

## Select and connect

`live instances --passive` inventories targets without input or actor binding.
Reuse a supplied installation/PID or a uniquely matching target under the user's
scope. Match the verified executable path and process creation identity, not
just `Wow.exe`, a folder name or a shared build hash. If multiple targets remain
and the task does not choose them, present known product/build, actor/realm and
PID; unknown identity stays unknown. “Test both” or an established arbitrary-
character scope already resolves that choice within its stated limits. Target
selection does not waive writer qualification: diagnose each process separately,
use only the CLI's eligible exact-image route, and report scene acceptance gaps
from [doctor](live-doctor.md).

```text
lycheedev live connect --project <project-directory> --installation <client> --pid <pid> --character <name> --realm <realm> --wait-seconds 120 --format json
```

Actor filters are useful when the task names a character. If it permits the
current character, use the supported selection without inventing a name and
retain the returned actor. Save `result.session`, project, journal, runtime,
installation and process/actor identity. A data `--snapshot` constrains data
context independently; it neither selects nor authenticates a running process.

One process has one durable connection owner and one active CLI driver. Each
same-build instance keeps its own CON/request and target; shared addon files
do not make them interchangeable. The first valid command binds the selected
runtime and actor to its owner/session; connecting alone does not establish
that in-game binding. A foreign
project's claim, missing publication or stale sample is not an expired timeout.

## Execute and finish

Read [execution](live-execution.md) for command submission, immutable budgets,
result interpretation and final disconnect. Read [reload](live-reload.md) for
an intentional reload after request cleanup. Pending alone does not call for
reload or another publication of the original source.

## Design a discriminating probe

Read [probe design](live-probes.md#design-a-discriminating-probe) for a new Lua
experiment or an unresolved source hypothesis.

## Write bounded probes

Use the [bounded API and cleanup rules](live-probes.md#write-bounded-probes).
A host timeout cannot preempt synchronous Lua.

## Test source hypotheses

Carry fixed locations and conditions into [live hypothesis testing](live-probes.md#test-source-hypotheses),
then feed observations back into that path. An unrelated successful probe does
not validate the suspected source or failing conditions.

## Input and recovery

For input blockers, contention or uncertain input, use [input recovery](live-recovery.md#input-and-recovery).
Keep one original CON/request driven serially.

## Recover without replay

Read [recovery](live-recovery.md#input-and-recovery) before resuming or reloading.
Pending alone does not call for reload or an unchanged retry loop.
