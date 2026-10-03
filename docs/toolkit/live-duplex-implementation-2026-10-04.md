# Duplex implementation candidate — 2026-10-04

This branch replaces the live transport. It does not retain the LoD execution
scheme, old input keys, optical readiness, or old wire compatibility. Published
3.1.1 evidence applies to that release, not this candidate. No version promotion,
npm publication or merge is implied.

## Contract

The addon owns a single fixed inbox (CLI to addon) and sendbox (addon to CLI).
Request source capacity is 1,048,576 bytes, encoded as 256 × 4096-byte frames;
these are transport pieces of one outstanding request, not independently
executable slots. Control lanes are bind/repair, commit, cancel, close, result
ACK, reload and lease. Close and cancel are serviced ahead of business commit.
Their host write leases are independent of the business driver.

Go and Lua implement the 320-byte LYCDPX01 header with exact uint64 counters,
identity, creation timestamp, immutable execution budget, per-frame SHA256,
complete-request SHA256, header SHA256 and odd/even publication stamps. The
private addon challenge authorizes execution only for the exact prepared
request. LYCSBX01 sendbox JSON exposes `ready`, `businessReady`, `transportReady`,
`controlReady`, `actorReady`, heartbeat, exact receipts and terminal/release
identity. Actor readiness requires the current logged-in/world state and exact
ordinary actor values; it does not prevent independently eligible cleanup.

Result source is bounded to 512 KiB and 32 pages. The CLI checks every page and
the complete result, persists it before ACK, and requires exact RELEASED before
allowing new business. Unknown execution never replays. A host wait timeout is
independent of the persisted transfer deadline and actual Lua execution budget.
Synchronous Lua cannot be preempted by cancellation or a host timeout.

Reload uses a private prepare challenge followed by a recorded lease commit.
The old connection is retired only after exact quiescence, writer drainage and
replacement-runtime observation. It does not silently bind a new runtime or
rerun its predecessor's request.

Disconnect preserves the closed binding until a new owner proves the exact old
owner, session, fence and released request/digest in an 88-byte bind proof. Both
owner and session must change, and fence must increase by exactly one without
overflow. Only after validating that proof does the addon reset control and
request sequence watermarks; the new connection can start at request sequence 1.
Old-session frames and controls remain rejected. Unknown bind input is observed,
not replayed. Manual addon disable cancels managed asynchronous work or closes a
request that has not started, while retaining the exact terminal result for its
original connection. Failed cleanup cannot advertise resource release.

Doctor runs before each CLI live drive and can read a targeted fresh sendbox
without binding or repairing. It distinguishes business readiness, cleanup
control eligibility, root/read observations and write capability. Cosmetic
activity has only one active transport state: Agent执行中 during actual probes.

## GC and memory ownership

Private leaf roots retain every writable row independently of public parent
tables. A damaged public arena revokes its generation. At most one retired
generation can remain rooted until all eight host writer leases are drained
and the exact repair handshake confirms release. A second unresolved damage
quarantines the runtime. A retained private `not_started` proof alone permits
retransmitting the same request; missing private ledger never authorizes it.

Strong roots prevent normal GC of reachable objects. They do not make native
WriteProcessMemory atomic with Lua mutation or prevent another Lua writer from
resizing an exposed row's backing array. Root RVA and numeric calibration do
not prove that collector/array ABI. Independently reviewed executable/build
writer profiles are required; unknown and unvalidated profiles reject all
writes before acquiring a new connection claim. This candidate currently has
**no eligible writer profile**. Real native transport acceptance is not_run.

Independent static analysis of the manifest-hashed Retail 12.1.0.69933 runtime
text confirmed Table array pointer at 0x20, count at 0x40 and TValue stride 24.
The lookup at RVA 0x3b993d0 was compared directly with the archived runtime bytes;
the reader and fixtures now use the verified count offset. Executable SHA256 is
`d41f11de411f6fdb280a7c1c6380ba3d1b17614f03e5aca8fabce2cc715cd7dd`;
captured text SHA256 is
`4fa9cbb1a90fa114d26f64a8e2e14e66852a5c71450ede69cdfd079c23348f75`.
This evidence certifies those instructions, not all other builds or a writer.

The same capture shows `table.freeze` setting Table+0x45 and ordinary assignment,
rawset and table.insert checking that flag. Array resize replaces its allocation,
and all collector/VM resize paths are not certified. More fundamentally, a user
reload/logout can destroy the VM between the host's final guard and native write.
Repeated address checks, challenges and OS writer leases cannot close that window.
The pure external-WPM design currently lacks an enforceable target-allocation
lifetime pin. This is an architecture limitation, not an acceptance test that can
be fixed by simply enabling a profile. No suspension, injection or LoD fallback
is added. Static disassembly evidence is kept outside CI in the local research
directory; no developer machine path becomes a build input.

The 1 MiB limit describes logical source, not total RAM. Lua numeric TValue
storage, table capacity, compiled code, result encoding and probe-created
objects add overhead. The transport reuses its fixed arena and releases source,
compiled closure, callback resources and runtime result after verified release;
it does not retain all 140 or 1000 commands. User probes can independently store
objects in globals or another addon; that is outside transport retention.
The automation page projects only the current request or one release identity;
it cannot execute work, acknowledge results or import old queue/history records.

## Verification and remaining work

Offline fixtures and real-client evidence are separate. The Go coordinator has
1 MiB boundary, 1000-request retention, unknown publication, independent
control, durable result, repair crash and reload commit fixtures. Native read
fixtures reject changed roots/arrays, secret numeric values and wrong arena
generations. A Lua probe fixture checks 140 cancelled asynchronous callbacks
release their environments even if external code keeps callback wrappers.

The final offline baseline passed on 2026-10-03 23:45:03 UTC, including build,
vet, uncached full Go tests with required Lua 5.1, real LuaLS integration, all
17 required cases, 71 Node tests (zero skips), version/skill/generated-command
checks and unchanged-source verification. The Go report contains 2256 test
results across 42 packages, zero failures and 37 optional environment/helper
or real-client skips; those skips are not accepted as live evidence. Actual
Go-to-Lua wire exchange covers execute, result ACK, close, fresh bind and another
execute at sequence 1. Targeted Go race tests for duplex and duplexhost also
passed. The initial baseline's stale help-argument assertion was corrected;
the final baseline was a fresh full run.

Lua 5.1 stress measured +261.1 KiB after releasing a 1 MiB request and +8.4 KiB
after 140 further released requests, relative to the already allocated arena
after full GC. This is retained fixture memory, not peak memory, native WoW RAM
or a guarantee against probe-owned globals. The Go fixture also completed
1000 requests while keeping current journal state bounded.

Local raw evidence is under `Analyze/duplex-mailbox/evidence/offline-final-20261004-02/`
and remains outside CI. Its tested source digest is
`37819d560384465a7900126db0da9244d4fce8f5e6437043eee9c1764aa476e9`;
the verification record was added afterward without further product changes.
The independent final architecture review found no remaining reported defects
in this offline draft and no production write-gate bypass. It does not certify
native write lifetime safety.

Real-client activation, native profile qualification, sustained in-game memory
use, same-build dual instances, visual review, relog/reload and cross-build
writer profiles remain independently not_run. All ten real-client/manual
baseline cases retain that status. This is an offline implementation draft;
the pure external-WPM lifetime gap still blocks usable native live delivery.
