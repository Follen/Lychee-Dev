# Mailbox v1 recovery

Preserve the project, CON, request key, exact source and original execution
budget. Run [doctor](live-doctor.md) before any drive. Status is read-only;
resume reconciles the original persisted exchange.

```text
lycheedev live status <CON-id> --project <project-directory> --format json
lycheedev live resume <CON-id> --project <project-directory> --wait-seconds 120 --format json
lycheedev live cancel <CON-id> --project <project-directory> --format json
lycheedev live disconnect <CON-id> --project <project-directory> --format json
```

## Input and recovery

There is one outstanding business request. `inbox.command` contains one whole
command, written once; `inbox.stop` carries cancel, close or final ACK/close.
These rows have separate host writer claims. Cancel requests an outcome; it is
not proof that synchronous Lua stopped or that effects did not occur. Read the
exact terminal result and `executionStarted` before reporting cancellation.

If a command write was attempted but no acceptance was observed, do not resend
it. An exact stop intent can revoke the unused ready challenge. If an older
terminal result is still pending, the stop also carries its exact ACK. The
addon retires that older result and retains a `not_started` terminal record
for the new request together; that new fact remains until its own exact ACK.
A partial/mixed write or lost host receipt stays uncertain
until the original runtime provides evidence; never switch to a different
process, actor or row to repeat it.

Character selection, loading and `/reload` block new commands. They do not
turn an unknown outcome into `not_started`. Read the independent native
[world and reload diagnostics](live-doctor.md) before another drive. If runtime
identity changed, retain the old journal and start a new session only after
the old outcome and claim have been handled honestly.

## Durable result and next command

The CLI has a transfer deadline independent of the addon execution budget.
Host timeout renews neither. Verify the terminal record and all result pages,
then durably save exact result bytes and digest. The **next** whole command
carries the previous terminal ACK digest in its header; addon validates that
ACK and the replacement command before retiring either result or ready token.
If no next command is needed, final disconnect sends ACK/close on `inbox.stop`.
If close arrived during execution, a later final ACK/close may still be needed.
An uncertain stop write is observed under its original message ID, not replayed.

A verified failed probe is still a result. A verified result with cleanup
pending remains useful evidence and an unfinished recovery obligation. A
completed exact request retry reads retained evidence without re-executing it.

## GC, damaged rows and runtime retirement

The addon keeps strong private references to frozen inbox rows. Ordinary GC
cannot reclaim a reachable row; addon repair can replace a damaged generation
only after every host writer is drained. At most one active and one retired
generation exist. A second unresolved fault quarantines the runtime. Do not
repair or replace a row merely because a checksum sees an in-flight mixed
publication; observe the original write and retry the read.

Reload, logout and process exit destroy the old runtime despite those GC roots.
No address or result authority crosses that boundary. Preserve unknown effects
and the old journal. The CLI may retire only its **local** process claim after
draining its exact command/stop writers and proving the runtime or actor was
replaced; this does not claim in-game ACK or a known business outcome. Do not
manually clear claims, forge a result, force GC or replay a
probe to escape pending work. Same-build windows have distinct PID/creation,
runtime and actor pins. A new build needs a verified recipe and matching ABI;
one located RVA is not writer qualification.

Intentional reload remains an independent operation:

```text
lycheedev live reload --project <project-directory> --session <CON-id> --request <reload-key> --format json
```

It uses the small stop row with a durable prepare challenge and exact lease
message, and must serialize with the command writer. An uncertain prepare or
lease is observed by its original message ID, never sent again blindly. It
preserves any unresolved command.
After runtime replacement, run doctor and establish a new session for new work.
Old duplex, slot and LoD journals are not upgraded or relabeled as mailbox v1.
