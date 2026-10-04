# Lychee Dev mailbox protocol v1: implementation and acceptance

**Current whole-command candidate:** commit `b021251` changed the physical
layout to `single-command-row-v1` and writes one 1 MiB-capacity row per command.
The first formal Retail CLI debugger-stopped write was followed by a client
security crash and no verified execution result. That route is now disabled.
The exact-image [direct Retail trial](live-mailbox-v1-direct-retail-trial-2026-10-04.md)
verified a small probe, a 1 MiB probe and independent close, without debugger
attachment or thread suspension. See also [the stopped-write incident](live-mailbox-v1-retail-trial-2026-10-04.md). The 4 KiB
candidate results below are historical and do not establish acceptance for the
current physical layout.

The direct trial's small request reached observed execution in 267 ms; the
1 MiB request took 19,156 ms. These journal intervals include validation and
host observation, so they are not isolated WPM measurements. The same process then passed a separate probe with three full Lua GC cycles,
verified result and close; the direct-trial record retains its CON/request and
whole-VM counters. It does not prove GC overlapping WPM. The final check
to `/reload` race remains and is an owner-accepted residual risk: the Agent does
not initiate reload during a write and tells the user to avoid manual `/reload`
then; normal reload after request cleanup is allowed. Other builds,
reload/new-runtime execution and multi-instance competition have not passed
this writer's real-client acceptance.

The current contract is [mailbox v1 architecture](live-mailbox-v1-architecture-2026-10-04.md).
Code candidate `73f61a74623b0468c0fe6aa0709a0a51b0453a2a` was installed from a
clean development package, not published to npm. The version label remains
3.1.1; the commit and new schema/layout identify this candidate.

## Historical 4 KiB candidate contract

- `lycheedev.mailbox.v1` / `single-data-row-v1`, new wire magic and SHA256 domains;
  earlier schemas, layouts and wire records are rejected.
- One reusable 4 KiB data row, up to 256 logical frames / 1 MiB per command.
  Exact private receive ACK is required before reusing the row. Seven small
  independent control rows keep cancel/close/result ACK available separately.
- Identity, owner/fence, actor, timestamp/sequence, digest, prepared challenge,
  execution commit and exact result ACK remain checked. Only one command may
  be retained; completion and release are distinct states.
- Private leaf roots, bounded two-generation repair and host writer drain;
  native world/reload recipes and per-write guards; no old input fallback.
- Empty/unchanged inbox polling avoids repeated decoding and deep snapshots.
  Low-word uint64 formatting avoids temporary conversion tables on heartbeats.

## Offline and CI

The frozen-source Windows baseline passed with locally built **x64 Lua 5.1.5**:
`Analyze/duplex-mailbox/evidence/offline-final-20261004-09/report.json`.
It includes build, vet, uncached full Go/mandatory Lua suites, Node distribution
tests, version/skill/generated-contract checks and source stability. The 1 MiB
single-row transfer and 140 released requests passed; lifecycle fixtures cover
GC roots, repair/drain, cancellation, close, ACK and stale/partial input.

Both code-candidate CI runs passed all jobs: PR `37177344681`, push `37177342717`.
Earlier runs at `32f3df2` failed the same idle allocation threshold. That failure
was reproduced with x64 Lua locally: 133.09 KiB / five seconds versus a 128 KiB
budget. The formatting fix reduced it to 111.89 KiB; the budget was not relaxed.
Cold retained increment was 100.38 KiB, all-lane shadow caches 189.54 KiB,
and retained idle growth after full GC was below 1 KiB. These are stock-Lua
fixture values, not WoW measurements. Earlier documentation-time SOURCE and
allocator failures remain in their original records.

## Real Retail observation and addon-owned execution

Observed 2026-10-04, Retail `12.1.0.69933`, PID `140100`, creation identity
`140100:134355550237345516`, executable
`D:\Game\World of Warcraft\_retail_\Wow.exe`, SHA256
`d41f11de411f6fdb280a7c1c6380ba3d1b17614f03e5aca8fabce2cc715cd7dd`.
Actor: 灵止光 / 死亡之翼, GUID `Player-741-066A34E3`.
No other executable or instance was targeted.

Managed addon bytes were clean. After manual reload, targeted doctor observed
the new schema/layout, fresh advancing heartbeat, exact actor/process identity,
`world_ready` and `no_reload_observed`. The native mode was `0x0211`: world bit 4
set, reload request/worker bits 7/8 clear; Glue pending byte was zero. Runtime
`6f0cb345ee6c2df5018ae284b9ee585b`, arena
`37a97ae472ac56050afc2007e11c0050` were checked against the self-check report.

A separate one-shot addon, held only under ignored `Analyze/mailbox-v1/`, used
Go-generated v1 records in an **addon-owned local arena**. The actual execution
path used the installed `DuplexProtocol`, `ProbeExecution`, `CaptureWriter`
and SHA256 implementations. Its small probe only read build/world APIs.

The report passed: one execution, one repeated-frame observation without a
second execution, verified prepared challenge, exact result ACK/release, and
private roots surviving full GC after the public row reference was removed.
The result was 60 bytes; the whole self-check took approximately 1.547 seconds.
All its events/timers and working arena/fixture references were retired.
The three installed verification files were then hash-checked against their
own receipt and moved out of AddOns into the local evidence archive. No extra
game input or reload was sent for cleanup; the managed main addon remained clean.

Report SHA256:
`f5c80a6c8dcdb15f5b9c71ceefb8fa348213e624df09939c5e6738795e635f78`.
The readback recomputed the digest, reread Lua root/global/mailbox/report
identities and row topology, and checked lifecycle flags before/after. It is a
consistent checked observation, **not an atomic snapshot or write authority**.

| Measured object | Before self-check | After release/full GC | After one-second idle/full GC |
| --- | ---: | ---: | ---: |
| Lychee Dev addon memory counter (KiB) | 1813.585 | 1818.187 | 1813.007 |
| Whole Lua VM counter (KiB) | 317981.847 | 333086.245 | 351835.772 |

The addon counter was approximately **1.77 MiB** and did not grow across this
short sample. The whole VM includes other addons and grew during the sample;
that growth cannot be attributed to Lychee Dev from these observations.
This is not a long-duration or maximum-input peak measurement, nor a matched
before/after reproduction of the owner's earlier 70 MB observation.

Actual backing: one data array with capacity 2048 TValue cells and seven
control arrays with capacity 512 cells each, stride 24 bytes. Total physical
exchange-array storage is **135168 bytes / 132 KiB**, excluding table headers,
hashes, strings and observation shadows. The old 256 data arrays alone were
12 MiB; the new whole-plugin counter and this array total measure different
objects and must not be combined as interchangeable figures.

### Preserved failures and evidence

The first test addon did not load: `Dependencies: Lychee Dev` was parsed into
space-separated dependencies and the game reported missing `Dev`. The test
TOC was corrected to wait for the exact main addon/startup event without that
dependency declaration; main-late and namespace-only memory API fixtures passed.
The next reader incorrectly treated a Lua boolean's unused upper 32 bits as
meaningful. Its failed capture was preserved; a low-32-bit/tag-checked decoder
with a nonzero-upper-word fixture then passed on a new capture. No production
writer eligibility was changed to bypass either failure.

Local evidence (not release/CI inputs):

- `Analyze/mailbox-v1/verification-addon/installation-receipt.json`
- `Analyze/mailbox-v1/verification-addon/real-game-readback.jsonl` — missing test global
- `Analyze/mailbox-v1/verification-addon/real-game-readback-after-toc-fix.jsonl` — boolean reader failure
- `Analyze/mailbox-v1/verification-addon/real-game-readback-after-done-bool-fix.jsonl` — passed,
  SHA256 `ac1d5b98623125fba2d192c1bcecf114a484da857120e36f339674b85e398b80`
- `Analyze/mailbox-v1/verification-addon/real-game-report.json`
- `Analyze/mailbox-v1/verification-addon/cleanup-receipt.json`
- `.tmp/mailbox-v1-73f61a7-doctor-final.json`

The preserved FrameXML log still has the original 12:34:47 dependency error;
its timestamp did not advance on the successful reload. It cannot prove the
absence of all later errors. The completed in-memory report establishes that
the corrected test scripts actually ran.

## Historical 4 KiB acceptance boundary

At the historical 4 KiB self-check above, all native writer profiles were
ineligible. Targeted doctor deliberately
reported `healthy=false` / `live.duplex_writer_profile_unverified`. Observed
reload flags and strong roots do not close the final-check-to-WPM VM teardown
race. No external game-memory writes, native CON probe, debugger attachment or
thread suspension were used for this self-check.

At that stage, native CLI-to-addon execution, true maximum-input game peak,
game-side cancel/disconnect/reload/repair during external writes, same-build
dual instance, other clients and real cross-build writer reuse were **not_run**.
Offline protocol and addon-owned execution did not qualify those paths. Those
historical restrictions do not erase the later exact-image direct trial.
The earlier candidate was not merged or published by this self-check.

## Current whole-command remaining acceptance

| Scenario | Evidence state |
| --- | --- |
| Exact Retail 12.1.0.69933 image: small and 1 MiB direct command, readback, result and final close | passed, restricted to the direct-trial identities |
| Three full GC cycles in a later probe on that same process, then verified result and close | passed; not simultaneous GC/WPM or long-duration safety |
| Reload after request cleanup, replacement runtime and a new command | not_run |
| Same-build real dual instances and competing host drivers | not_run for real clients; do not substitute offline fixtures |
| Classic/Titan writer and another Retail hash/build | not_run; write-ineligible |
| Forever writer | excluded from the acceptance matrix |
| True 1 MiB game memory peak, matched CPU/frame/latency distributions, 140 real commands and long idle | not_run |
| External reload starting after final check or during WPM | residual risk accepted by owner; not eliminated |

The [reconstruction plan](live-mailbox-v1-reconstruction-plan-2026-10-04.md)
defines remaining implementation and scene-specific acceptance. It does not
upgrade an unrun scene to passed. New offline or real-client results must retain
exact CLI/addon commit, install receipt, product/build/hash, PID/creation,
runtime/arena, actor, CON and request/control IDs; previous release acceptance
cannot be inherited by this layout.
