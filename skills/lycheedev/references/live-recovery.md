# Duplex recovery

Preserve project, CON, request key, exact source and execution budget. Run
[doctor](live-doctor.md) before drives. Status is read-only; resume reconciles
the original persisted exchange.

```text
lycheedev live status <CON-id> --project <project-directory> --format json
lycheedev live resume <CON-id> --project <project-directory> --wait-seconds 120 --format json
lycheedev live cancel <CON-id> --project <project-directory> --format json
lycheedev live disconnect <CON-id> --project <project-directory> --format json
```

One business request can be outstanding. Cancel and close use independent control
lanes; they do not need the business driver lease. Cancel is a request, not proof
of interruption. Synchronous Lua cannot be preempted; asynchronous completion
requires actual cleanup and an exact terminal result. Disconnect can proceed
without waiting for an unanswered cancel, but ownership retirement still needs
verified resource release and drained host writers.

Character selection or a loading screen blocks new business. It does not by
itself revoke cancellation or close authority. Follow the independent native
[world/reload diagnostics](live-doctor.md): observed reload or unknown lifecycle
stops all writes and preserves the original journal; wait and re-observe that
same process instead of writing to retained addresses.

The CLI retains a transfer deadline separate from the addon execution budget.
A host timeout does not cancel Lua or renew either budget. The exact terminal
manifest and all result page hashes must verify, result bytes must be durable,
and the exact ACK and RELEASED must be observed before another business request.
A successful or failed verified report with pending release is useful evidence
with an unfinished cleanup obligation.

If the public arena is lost, the addon revokes its generation and retains its
private execution ledger and old arena until host writers are drained. The CLI
can repair only from that proof. A private `not_started` result permits resending
the same request ID, source and digest to a new arena. Already executed work
returns its original result. Missing ledger, unknown execution or an uncertain
commit never permits automatic replay, even for read-only probes.

Actual loss of the private roots or ledger is not recoverable by guessing an
address. Stop writes and preserve uncertainty. A different process, actor or Lua
runtime cannot inherit old request authority. Do not clear claims, alter journals,
force GC, inject a runtime, or recreate empty state to escape a pending request.

Intentional reload is an independent, nonce-correlated command:

```text
lycheedev live reload --project <project-directory> --session <CON-id> --request <reload-key> --format json
```

Reload ending the old runtime does not prove its probe had no effects. Preserve
the old outcome and bind the replacement only after exact retirement. A new key
means a new experiment after completion, never a retry shortcut.
