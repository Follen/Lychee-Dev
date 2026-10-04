# Doctor before live drives

Run doctor from the same project and workspace before every connect, execute,
resume, cancel, disconnect or reload. CLI drive entry points also perform this
read-only preflight and retain its findings in the result context.
This candidate's publication uses Lychee Dev mailbox protocol v1,
`lycheedev.mailbox.v1`; an old schema or 256-row layout is incompatible.

```text
lycheedev doctor --project <project-directory> --installation <client> --pid <pid> --offline --format json
lycheedev doctor --project <project-directory> --session <CON-id> --offline --format json
```

Use either a retained connection or installation/PID selectors. The optional
target check observes the exact process, managed deployment and fresh sendbox;
it does not bind, send keys, execute probes or repair anything. General workspace
checks alone do not establish game readiness.

`nativeReload` and `nativeWorld` are independent of the addon publication. The
native IsPlayerInWorld getter qualifies `worldReady`; login, a nonzero Lua root
and the GameUI mode bit cannot substitute for it. New bind/frame/commit requires
world readiness and no observed reload. Cancel, close, result ACK, lease and repair use their
own authority without requiring world readiness, but every write still rejects
reload, teardown or an unknown lifecycle. Missing world evidence is unknown,
not false and never true.

The Retail lifecycle recipe verifies current instruction anchors and RIP data
targets. A compatible new build is relocated from unique full-text patterns;
zero/multiple matches or changed metadata reject it. Successful relocation is
read evidence, not cross-build writer acceptance. During reload, world can turn
true before the reload flag clears, so inspect both independent diagnostics.

Inspect the per-layer diagnostics, especially `actorReady`, numeric read layout
and `writerProfile`. A resolved root and readable sendbox do not qualify writes.
This candidate has no eligible writer profile: targeted doctor reports that
capability as an error and native writes remain unavailable. The final address
guard cannot prevent VM teardown during an external write. Private roots and a
host writer drain protect controlled arena replacement, but do not pin the VM
or its backing arrays across WPM. Addon self-tests and offline interoperability
tests do not qualify a writer or establish CLI-to-game execution. Do not patch
away the gate, advertise live execution or inherit old-release acceptance.

`ready` means new business is eligible; `controlReady` is independent. A running
request, terminal result awaiting ACK or closing connection can be unready for
new business while still allowing cancellation, retrieval or close. Preserve
the findings and use the coordinator's exact authority checks. Do not require
business readiness for cleanup or treat a warning as permission to replay work.

An unavailable publication is unavailable evidence, not proof that the previous
command never ran. Use the original CON and [recovery](live-recovery.md). A stale
heartbeat or unsupported layout must not cause memory writes to cached addresses.
