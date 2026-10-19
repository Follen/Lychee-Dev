# Live investigation

A live observation establishes behavior on one verified client under its recorded
conditions. Source, static data and Hotfix records establish different facts.
Start with existing CON status/results; use [recovery](live-recovery.md) for pending
work, [startup](live-startup.md) for readiness, [retained errors](error-diagnosis.md)
or [performance](runtime-investigations.md) when those answer the question.
Write a [probe](live-probes.md) only for missing evidence.

## Select and connect

Reuse the selected installation/PID or discover passive instances. Match verified
executable path and process creation identity, not name, folder or build alone.
Resolve only remaining target ambiguity. Unknown actor stays unknown; current-
character or multiple-client scope need not become a new approval question.
Retain returned project, CON, journal, runtime and process/actor identity.
A data snapshot neither selects nor authenticates the running process.

One process has one durable owner and one active driver. Same-build instances
remain separate even with shared addon files. Use installed help for connect/
execute/disconnect syntax. Live uses Lycheedev mailbox protocol v1 and a matching
clean 500-slot installation. The CLI owns guarded runtime-code/Lua/Mailbox lookup,
identity and input checks. Unsupported layout, ambiguity or unreadable coverage
is a diagnostic gap, not permission for heap scans, guessed RVAs or raw input.
A successful lookup with scanComplete=false does not prove global uniqueness.

## Complete an investigation

Use a stable request key and exact source or immutable probe revision. The Lua
execution budget, host wait and durable recovery deadline are different; a host
timeout does not cancel Lua or renew its budget. Observation policy is for an
entire safely repeatable experiment after verified runtime loss. Opaque is the
default for effects that cannot safely repeat. Neither allows manual replay of
unknown input. A changed experiment needs prior operation closure and a new key.

Read three independent facts:
- reportState=verified: exact result bytes were retained and verified
- report.ok, assertions and coverage: business outcome and observed scope
- complete with cleanup=complete: this command's lifecycle finished

A verified Lua error is evidence; pending cleanup still needs recovery on the
same CON. Exact completed-request replay reads retained bytes without rerunning
Lua; changed code/arguments are not that replay. Native work needs no legacy
OP/BTP ACK, finish or hide sequence. Capacity recovery may reload, so scene-
dependent probes must reconstruct prerequisites or record their absence.

Disconnect at the end unless continued use is needed. Closed records durable
connection closure; it does not prove the ownership claim was released. Check
the current error and retirement result: if lease close or retirement failed,
continue cleanup on the original CON/project. Closing also does not prove an
unavailable/unknown business effect never ran. Preserve the whole
`.lycheedev/live` recovery unit.

## Test source hypotheses

Carry fixed locations, version correspondence and remaining conditions into
[bounded hypothesis testing](live-probes.md#test-source-hypotheses); feed the result
back into that path. An unrelated successful probe does not validate it.
For pending input, interruption or reload, use [recovery](live-recovery.md).
