# Live investigation

Use this for evidence about the running client. Source describes possible
behavior; static data and Hotfix captures describe fixed records. A live
observation establishes what happened on its selected client and conditions.

Start with retained state: status/results for an existing CON, [recovery](live-recovery.md)
for unfinished work, or select/connect below for a new task. Reuse a matching
idle CON. Read [startup](live-startup.md) when activation or installation is needed.
Use [retained errors](error-diagnosis.md) or [performance measurement](runtime-investigations.md)
when they answer the question; write a [probe](live-probes.md) only for missing evidence.

Native 3.1 uses a build-bound Lua root and the named public Mailbox, with a
matching clean managed duplex installation. The CLI validates the current process,
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
character scope already resolves that choice within its stated limits.

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
do not make them interchangeable. A fresh bind establishes runtime ownership;
execution remains within the authorized task. A foreign
project's claim, missing publication or stale sample is not an expired timeout.

## Complete an investigation

```text
lycheedev live execute --project <project-directory> --session <CON-id> --file <probe.lua> --request <stable-key> --budget-seconds 30 --wait-seconds 120 --format json
```

Use `--probe <immutable-PRB-revision>` instead of a file for registered source.
Keep the exact code, request and budget for continuation. `--budget-seconds`
bounds addon execution (1..120); `--wait-seconds` bounds this host call
(1..600, default 120). The [durable recovery deadline](live-recovery.md#durable-evidence-and-older-journals)
survives between calls. Host timeout does not cancel Lua or renew either budget.

The inbox accepts up to 1 MiB of source with one outstanding request. The addon checks identity, challenge, sequence, frame hashes and the complete request SHA256 before execution. Unknown execution is never replayed; only a retained private not_started proof permits the same request to be retransmitted after arena repair.

Read the result on three independent axes:

| Fact | Meaning |
| --- | --- |
| `reportState: verified` | Exact result bytes were verified and retained |
| `report.ok`, error, assertions and result coverage | Whether the probe's business check succeeded and what it observed |
| `complete: true` with `cleanup: complete` | This command finished its lifecycle |

A verified Lua error/timeout is useful evidence, even when its business result
failed. Pending cleanup still needs [recovery](live-recovery.md), not another
execution. Completed native work needs no ACK/finish/hide command. Repeating
the exact completed request reads retained bytes without rerunning Lua, even
after later operations or game exit; edited parameters/code are not that retry.

The CLI may reload at a quiescent capacity boundary. Scene-dependent probes
must reconstruct prerequisites or record that they cannot. Ordinary observation
does not need an input shield, and UI activity labels prove neither execution
nor result retrieval.

```text
lycheedev live disconnect <CON-id> --project <project-directory> --wait-seconds 120 --format json
```

Disconnect when the authorized investigation ends unless continued use is needed.
It verifies unbind or [process/runtime retirement](live-recovery.md#ownership-and-runtime-retirement)
before releasing ownership. Completed-close retry is read-only or repairs its
interrupted host retirement. `closed: true` can coexist with unavailable report/
`execution_unknown`; connection cleanup does not establish a business outcome.
Keep the project's whole `.lycheedev/live` evidence directory together.

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

Read [recovery](live-recovery.md#recover-without-replay) before resuming or reloading.
Pending alone does not call for reload or an unchanged retry loop.
