# Offline Live wall-time diagnostic

This is a **Rust-only diagnostic**, not B06 performance acceptance. It runs the
real Rust Driver, protocol codec, durable journal, snapshot and terminal
projection against the authenticated fixed-Go323 Lua peer. It does not open a
game, send native input, use a network service or execute arbitrary caller Lua.
The Go behavior oracle remains `3766f9cacb52e93a803a626187e2a1d54a445468`.

The existing formal9 bridge/runtime/113-file fixture authentication and peer
ownership are reused. The default nine recovery cases and their fixed source
allowlist are unchanged. Only `Peer::start_performance` enables the three exact
locally generated source strings and diagnostic counters. This benchmark does
not replace the nine recovery/error tests or claim new real-client coverage.

## Inputs and bounded execution

- Result-string sizes: 256, 16,384 and 131,072 **UTF-8 bytes** of ASCII `x`, plus
  fixed Unicode/newline/NUL/quoted fields. These are not exact protocol BODY
  lengths. Each report separately records the actual BODY payload byte length,
  checksum and SHA-256 from Lua; the full BODY remains below the original
  512-KiB protocol limit.
- Prior active journal histories: exactly 1, 255 and 256 authenticated events.
  Each sample uses actual durable append operations to construct its history.
- Three independent fresh fixtures/processes per size/history cell, interleaved
  by repeat: 27 samples total. These are not labeled cold/warm, and three samples
  are not substituted for the required independent 20-pair acceptance.
- A checkpoint at exactly 255 is a byte-identical no-op; at 256 it produces one
  active checkpoint event and an immutable archive containing every original
  byte. The subsequent append reports its actual input event count (1 or 255),
  not the pre-checkpoint count. The measured `journal_load_at_history` always
  uses the original 1/255/256 history.
- Each sample retains formal9's original 20-second operation control and the
  existing five-second owned-peer cleanup grace. The dedicated root invocation
  has one fixed 690-second aggregate bound (27 × 25 seconds plus 15 seconds for
  initial authentication/evidence). Production deadlines and stack are unchanged.
- One explicit ignored unit test is selected with `--exact --ignored`. Ordinary
  library tests do not unexpectedly launch these 27 peers. No samples are
  discarded or replaced after a failure; failure ends the matrix and retains
  all available original evidence.

## Windows invocation

Only use the sole coordinated Windows testing queue and its existing ordinary
Cargo channel. This entry does not require or execute a CLI artifact. It builds
only an optimized Live library test executable from the current exact source:

```sh
node tools/rust-live-offline-performance.mjs <sealed-Lua-build-report.json> <fresh-test-artifact-directory>
```

The first path must be an existing authenticated `build-report.json`; the second
must be a new contained directory. There is no bare-EXE, source-file, Lua-code,
budget, repetition-count or arbitrary command option. Omitted arguments select
`.tmp/ci-formal-lua-runtime/build-report.json` and
`.tmp/ci-live-offline-performance-test`. Preserve occupied directories and their
receipts. Do not overwrite old reports or bypass an occupied channel.

The runner uses existing `runCargo` admission/owner/target logic, source hashes
before/after compilation, Cargo's same-invocation structured artifact message,
exact test executable hash, build owner proof, current support hashes, sealed
Lua runtime receipt and fixed-Go fixture authentication. The new release-test
artifact records the source identity actually executed. A matching unused CLI
build is deliberately not a prerequisite. The original formal9 runner's
separate CLI identity contract is unchanged.

The controller requires the exact single test, all 27 reports, all nine complete
three-sample cells, exact input/BODY identities and normal cleanup proofs for
every peer. The existing owner seals raw logs, original journal segments,
artifacts, reports, peer records and pipe/lifetime evidence before successful
scratch cleanup. Failed or uncertain cleanup retains scratch.

## Reading the timing

All measurements use real monotonic `std::time::Instant` or Node's
`performance.now()`. The original fixed synthetic protocol/INPUT clocks remain
fixture data; they never become elapsed-time samples. No sleeps are added.

1. `ownedInvocationWallMillis` is the complete **wrapper-inclusive** owned
   invocation: request preparation, PowerShell startup/compilation, test-process
   startup, test execution, cleanup and result collection. It is not a native
   root-process clock and is never subtracted to manufacture a speedup.
2. The test's `totalWallNanos` is initial authentication + all complete sample
   walls + between-sample/evidence remainder. The final test report write and
   libtest/process shutdown remain in the wrapper-inclusive interval.
3. Every `sampleWallNanos` equals its ordered, non-overlapping scopes plus
   `unattributedWallNanos`. This includes fixture setup, actual history seeding,
   peer startup, correctness checks, probes, identity rechecking and checked
   cleanup. The following sample/test remainder contains sample-report writes.
4. The Driver interval is replaced, not augmented, by journal calls + backend
   calls + orchestration/measurement remainder. Those synchronous intervals do
   not overlap. Backend time includes the offline transport's additional
   durable-intent/result guards and fixture file IO.
5. `nestedPeerExchange` is inside backend time. It includes Lua execution,
   pipes, scheduling and fixture IO; it is neither pure wait nor game latency.
   `nestedPeerRecords` is also inside backend time: actual repeated record reads
   and production-codec decodes, including later-rejected stale records. Their
   counters describe this offline peer, not native process-memory reads.
6. `state_snapshot`, standalone `codec_decode`, state serialization/roundtrip,
   borrowed/owned terminal projections and their public serialization are extra
   disjoint diagnostic scopes. They are not charged a second time to Driver.
   Borrowed projection precedes owned projection and can warm data; their
   timings are not an unbiased paired speed comparison. Pointer/capacity and
   byte-equivalence assertions are the ownership evidence.

The controller also reports `controllerThroughVerificationMillis` (including
compilation/preparation) and returns `controllerWallMillis` after evidence
sealing/scratch cleanup. Neither is mislabeled product-only latency. Raw samples
are retained; min/median/max for each three-sample cell are diagnostic summaries.
Unattributed wall is not automatically CPU, physical-disk saturation or sleep.

## Correctness and memory facts

Each sample verifies complete journal checksum/sequence order, checkpoint archive
bytes, actual BODY/checksum/stored-result equality, Unicode/NUL preservation,
original request/ticket, durable-result-before-release, exact six input actions,
unique nonces, no completed-request replay, unbound/no-transaction completion,
full codec re-encode bytes, corrupt-codec rejection, complete State roundtrip,
byte-identical public borrowed/owned output and moved report pointer/capacity.
Low-level Driver disconnect unbinds; final ConnectionService retirement is a
separate consumer and is not falsely asserted here. First checked-close errors
remain errors; a consumed/missing owner is not treated as checked-close proof.

`ownedBytes` contains capacities and code-provable bulk copies for the extra
snapshot/codec/projection diagnostic scopes. `nestedPeerRecords.payloadCopyBytes`
counts actual production codec payload copies across all offline Peer decodes.
Neither is a whole-process allocation total. Allocator calls, total requested
bytes and peak RSS remain `null`; no unsafe allocator hook is introduced. The
simultaneously retained diagnostic outputs also make these fixtures unsuitable
for claiming product peak memory.

The current fixed Go host recovery test is not an equivalent timed adapter for
this expanded result-size/history/owned-projection matrix. Reusing its Lua
business peer establishes neither same-input Go host timing nor a speed ratio.
`goHostComparison=not_available` and `acceptance=diagnostic_only` are explicit.
A future comparable host adapter must preserve the same input, checks, journal
work and complete timing before any Go/Rust ratio is reported.

## Portable checks and current boundary

```sh
node --test tests/rust-performance-paired/live-runner.test.mjs tools/rust-formal-lua-bridge.test.mjs
```

These 56 Node tests validate fixture identity, strict report arithmetic, negative
samples, exact executable/result selection, ownership-entry wiring and the
unchanged original nine-case controller. Their synthetic numeric parser inputs
are not benchmark evidence. Rustfmt parsing is also not compilation. Actual
Rust compilation and the 27-sample Windows run remain **NOT_RUN** until the
coordinator records the exact tested artifact and sealed run.
