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
Only the owner-authorized direct, unsuspended route can write that exact image;
small and 1 MiB commands plus close passed one real-client trial. A later
probe on the same process completed three full GC cycles, verified result and
close; it did not exercise GC overlapping WPM. Reload/new-runtime execution
and same-build dual-instance scenes remain not_run, so do not advertise them
as accepted. This scene gap is distinct from exact-image write eligibility:
for an authorized controlled dual-instance check, each process must independently
pass the CLI's current exact-image profile and fresh runtime gates. A shared
build or installation grants neither instance the other's authority. Other
image hashes/builds remain write-ineligible; a relocated recipe is read-only
evidence, not permission to inherit another image's writer qualification.
A final address check alone
cannot prevent VM teardown during an external write. Private GC roots and a host
writer drain protect controlled arena replacement, but do not pin the whole VM
through reload. The owner accepts that residual risk: do not initiate reload
while writing, tell the user to avoid manual `/reload` then, and allow normal
reload after request cleanup. Addon self-tests and owned-process fixtures do not establish
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
