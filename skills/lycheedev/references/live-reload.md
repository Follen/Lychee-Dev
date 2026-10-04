# Mailbox v1 reload

Use this for intentional reload after the current request and retained terminal
result have been cleaned up. Pending or unknown execution alone does not call
for reload; first follow [recovery](live-recovery.md). Ordinary reload is allowed
when no command write is active. Do not initiate it during a write, and tell the
user to avoid manual `/reload` during that interval. The last native lifecycle
check cannot exclude an external reload starting during WPM; GC roots protect
reachable rows, not a destroyed Lua VM.

Run targeted [doctor](live-doctor.md) from the original project and exact CON
before this drive. The CLI enforces current eligibility and serializes reload
with its command writer; the Skill does not maintain an alternative gate.

```text
lycheedev live reload --project <project-directory> --session <CON-id> --request <reload-key> --format json
```

Reload uses the small stop row: a durable prepare intent obtains a private
30-second challenge; a distinct exact lease message cites that challenge and
the original prepare ID. The addon schedules ReloadUI after accepting it.
An uncertain prepare or lease is observed under its original message ID, never
sent again blindly. Preserve the original journal and all request evidence.
A stale address, early world readiness or missing reload receipt cannot prove
that a replacement runtime is ready.

After runtime replacement, run targeted doctor and establish a new CON for new
work. Recheck process creation identity, actor, runtime/arena and clean addon
bytes through [startup](live-startup.md). Do not carry an old challenge, address,
claim or operation into the replacement runtime; do not rerun uncertain Lua.
For a new probe in the continued investigation, use a fresh request key and
reconstruct any transient scene prerequisites. Read the old completed request
only as retained evidence; it is not an instruction to execute it again.
The exact-image direct Retail trial passed small/1 MiB execution and close,
but write-after-reload and a new-runtime follow-up are still not_run.
