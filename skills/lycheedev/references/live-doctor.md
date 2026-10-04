# Doctor before live drives

Run doctor from the same project and workspace before every connect, execute,
resume, cancel, disconnect or reload. CLI drive entry points also perform this
read-only preflight and retain its findings in the result context.
This candidate's publication uses Lychee Dev mailbox protocol v1,
`lycheedev.mailbox.v1` with layout `single-command-row-v1`; the previous
4 KiB data row and older schemas are incompatible.

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
and the GameUI mode bit cannot substitute for it. A new command requires
world readiness and no observed reload. Cancel, close, final ACK and repair use their
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
The exact Retail 12.1.0.69933 stopped-helper route is disabled after a
real-client security crash immediately following its first whole-row write.
Only the owner-authorized direct, unsuspended trial can write that exact image;
its real-client result is `not_run`, and all other builds remain ineligible.
A final address check alone
cannot prevent VM teardown during an external write. Private GC roots and a host
writer drain protect controlled arena replacement, but do not pin the whole VM
through reload. Addon self-tests and owned-process fixtures do not establish
CLI-to-game execution. Do not patch away the gate or inherit old-release acceptance.

`ready` means a fresh one-use command challenge is published; `controlReady` is
independent. A terminal result may coexist with a new challenge because the next
command must carry its exact ACK. Running or closing blocks new commands while
still allowing cancellation, retrieval or close. Preserve
the findings and use the coordinator's exact authority checks. Do not require
business readiness for cleanup or treat a warning as permission to replay work.

An unavailable publication is unavailable evidence, not proof that the previous
command never ran. Use the original CON and [recovery](live-recovery.md). A stale
heartbeat or unsupported layout must not cause memory writes to cached addresses.
