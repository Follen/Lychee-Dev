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

## Native lifecycle read gate

Retail uses a separately proven native `IsPlayerInWorld` getter (mode word bit
4), GameUI reload request (bit 7), reload worker lifetime (bit 8), and the Glue
reload request byte. GameUI mode bit 9 is not world readiness. Bit 1 is a teardown
candidate, not a complete logout proof. Unknown observations fail closed.
Every individual eight-byte write must run the gate after TValue/page checks
and recheck the context afterward. New bind/frame/commit also requires native
world readiness. Independent cancel/close/ACK/lease and repair do not require
world readiness, but still cannot write during reload or unknown lifecycle.

`retail-ui-reload-state-rip-v1` verifies complete current anchors for the known
hash and decodes RIP-relative data references. For other compatible Retail
builds it scans bounded complete `.text` for unique instruction patterns and
validates widths, shared mode-word references and readable/writable nonexecute
PE data ranges. The world getter qualifies independently; absent or ambiguous
world evidence stays unknown without erasing valid reload read evidence. Cached
binding proofs do not cache changing flags or grant write capability. Synthetic
relocation tests do not certify unseen client builds.

Targeted doctor reads this lifecycle before Lua-heap traversal and reports
native reload/world independently even when the loaded addon is absent or
uses an obsolete schema. Its world warning restricts new business; it does not
require business readiness for cleanup. See the portable
[read-only research record](research/reload-state-69933/README.md).

Owner-triggered Retail reload was observed on 2026-10-04 in the same process
instance: bit 8 was set across replacement of the Lua root, bit 4 cleared and
returned, then bit 8 cleared. World readiness returned while bit 8 was still
set, directly demonstrating why both gates are necessary. The new Go reader
then verified current anchors and reported `world_ready`/`no_reload_observed`
without writing. These are non-atomic read observations, not a writer lifetime
pin or real duplex execution acceptance. All writer profiles remain ineligible.

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

A Windows self-process fixture reproduces allocator ABA with real WPM after
all ordinary cell/page checks. The final read gate rejects an already observed
reload and rejects cancellation occurring inside the guard. A separate
limitation fixture deliberately reuses the suballocation just after a passing
guard and demonstrates the remaining overwrite; it also asserts the Retail
writer profile stays ineligible. These fixtures target only their own allocated
test page. Future profile qualification also needs measured per-cell gate cost
over real 1 MiB transfer; offline codec throughput cannot substitute for it.

The 1 MiB limit describes logical source, not total RAM. Lua numeric TValue
storage, table capacity, compiled code, result encoding and probe-created
objects add overhead. The transport reuses its fixed arena and releases source,
compiled closure, callback resources and runtime result after verified release;
it does not retain all 140 or 1000 commands. User probes can independently store
objects in globals or another addon; that is outside transport retention.
The automation page projects only the current request or one release identity;
it cannot execute work, acknowledge results or import old queue/history records.

## Verification and remaining work

The native lifecycle follow-up baseline passed on 2026-10-04 03:04:51 UTC:
build/vet, required Lua 5.1 uncached full Go suite, all 17 required cases, Node
distribution tests, real LuaLS, version/skill/generated-reference checks and
unchanged-source verification. Its source digest is
`14512ac94a3e4304be2d54de45887ff60985270e1afb6e3f15042db51b7cfe24`;
raw local report is `Analyze/duplex-mailbox/evidence/offline-final-20261004-03/report.json`.
Affected duplex/host/memory race tests and skill-creator validation also passed.
New fixtures cover relocation, ambiguous/missing/mutated anchors, image/read
failures, world-independent cleanup, private proof tampering, observed reload,
post-guard cancellation and the deliberately retained allocation ABA limitation.
Verification text was added afterward without changing product source.

Package install/release smoke now exercises the duplex CON contract and exact
read-only durable-result recovery, instead of deleted OP commands and an obsolete
test name. Corrupt stored result hashes are refused without rewriting state.
The race fixture establishes durable result/ACK progress before its short wait;
it no longer assumes a scheduling-sensitive timeout guarantees that progress.

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
