# Isolated Windows MD5 screening

This benchmark-only entry does not select a production backend, add a dependency,
change any Data checksum/cancellation/durability contract, or evaluate the original
Rust <=30% Go acceptance target. It builds only the existing Windows example, not
a CLI release. Current evidence remains in `data.README.md`/`handover.md`.

The historical CNG result stays unchanged: 64 MiB 84.1 ms versus RustCrypto 77.4 ms,
recorded in [the original report](../../docs/toolkit/rust-refactor-progress-source-data-2026-10-08.md).
It is not a new result or a prediction for the current fixture.

## Two explicit interfaces

`crates/lycheedev-windows/examples/digest_benchmark.rs` without arguments retains
its original three 1/16/64 MiB comparisons: deterministic contiguous bytes, five
RustCrypto whole-message calls followed by five existing CNG calls, arithmetic
means and unchanged JSON fields. CNG creates/destroys the provider/hash per call
and uses internal 1 MiB chunks. This is a legacy whole-slice comparator, not an
externally owned 64 KiB stream. No new FFI/unsafe/API or OpenSSL is introduced.

`--fixture-stream <absolute-root-file>` adds a separate authenticated experiment:

- Exact selected payload: 17,825,792 bytes, every byte 120. SHA256
  `397fa93fdb906d5a278075e087c5b1bef5c5f5fbfb3a080b438a3ba30f435db3`,
  CKey `c9fa02f009aa62f0b0504a86cc983e2b`.
- Exact independent Root: 44 bytes. SHA256
  `554c8419b746697073fe5d0e956e380b98e7b439e925ca28ec8001d457a41d44`,
  CKey `8cea531c64ce544535550008b29e781c`.
- Payload 64 KiB chunks give 272 updates; Root contributes one separate digest/update.
  The combined 273 count never means one concatenated message or invented split.
  It models the bounded update granularity; it does not assert a trace replay of
  every Data decoder return or reproduce file/cache/durable-capture costs.
- Three RustCrypto modes: incremental MD5 with clone-finalize (cold Reader owner
  model), incremental MD5 consuming-finalize, and fused SHA256 then MD5 with both
  states consumed (warm cache hash-order model).
- Six iterations, reversing the three-mode order on odd iterations; each mode
  processes Root then payload with fresh, independent states. All 36 samples,
  including iteration 0, remain. There is no outlier removal or automatic retry.
  There is no claim that correctness-warmed code/data is OS-cold or first use.

Root input content is hash-pinned and all acquired-handle exits use checked close.
The initial path metadata check is not a general race-free no-follow API; the
controller supplies its fresh owned scratch file, and the exact bytes are
authenticated independently.

The stream starts with a fixed contract record, emits 36 individual sample records,
and ends with an explicit completion record. The controller rejects missing,
extra, reordered, nonfinite or negative data and validates every digest/count.

## Timing boundary

Each sample uses `Instant` wall time and explicitly records new/init, update loop,
clone (zero in consuming modes), finalize, explicit original-owner drop, total and
unattributed gaps. All five subscopes plus nonnegative gaps equal the unchanged
sample total. No negative value is clipped. Context/state creation, clone and
finalization/drop costs stay inside the sample lifecycle. RustCrypto currently
uses inline state; dropNanos is the explicit black-box/state-release boundary,
not evidence of a native destructor or allocator operation. Small phase values
include timer/black-box overhead; there is no allocator-cost subtraction.

Input construction/read/authentication, reference hashes, vector checks, output
formatting and logging are outside the sample clocks. They remain inside the
complete native process wall. There is no unrecorded warm-up sample. Sample
updates include the benchmark's shared-deadline check. It is one original 30-second
sample-loop deadline, never renewed per update/message. The controller applies
its separate existing owned-process timeout and retains failures/partial logs.

Original owned-process `CreateProcess` through root/tree reap and output drain is
recorded separately from controller elapsed time and microbench samples. Windows
`GetProcessTimes` CPU covers that original benchmark root only, including all its
setup/correctness work; it is not per-sample CPU or cumulative descendant CPU.
Observer/PowerShell boundaries retain the existing native timing contract.

## Correctness before measurement

The example fails before recording the contract unless all checks succeed:

- All seven RFC 1321 vectors through RustCrypto and the existing CNG whole-message API.
- Eight deterministic binary lengths 55/56/63/64/65 and 65535/65536/65537.
- Four split policies for all three modes: 1 byte, 7 bytes, 64 KiB, irregular splits
  with empty updates. SHA256 is checked against its complete-message reference.
- Ten injected cancellation and ten injected deadline stops across all modes:
  before new, before the second update and before finalization for each mode,
  plus after clone for the clone mode. The original typed
  stop must return with no later callback; scoped owners drop on the early return.
  These deterministic injection checks are not a timing-dependent cancellation race.
- Actual CNG already-cancelled and already-expired initial-stop error strings.
  Its public API has no controllable mid-stream injection point; none is claimed.
- Both retained Root/payload identities cross-checked against CNG outside sample
  timing. Every timed row must match its complete digest(s) and update count.

This is screening code. It does not replace production encoded-block MD5/CKey,
EOF, size, retained-budget, cancellation, publication or checked-cleanup regressions.

## Controlled execution

Run only through the existing authorized Windows testing queue. The planned entry
is:

```text
node tools/rust-md5-benchmark.mjs <authenticated-content-fixture-directory> <fresh-artifact-directory>
```

This controller accepts only the exact existing encoded content family. Before
any fixture decode, it bounds and authenticates the 94-byte, three-record index
(SHA256 `315bdd6da2f530e2c094c588be86ccde64723a1946f9a11bd9e7d3851afee800`)
and 174,716-byte archive
(SHA256 `cf6aae3657915c3ff64a06d01d218aa5841c1633e05a8ef438ef3c09457f4074`).
It repeats this preflight before each fixture load and checks it again afterward.
These identities come from the e253 report's `inputs.fixtures.content.inventory`
([main evidence](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6234095130),
[complete index](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6234099196));
they are not newly invented benchmark inputs.

The controller then authenticates the existing fixture inventory/descriptor,
extracts and rechecks the 44-byte Root, uses `runCargo` to build only this optimized example
with locked/offline dependencies, and selects its executable from that invocation's
structured compiler output. It does not accept an arbitrary executable or use a
newest-file scan. The one-shot build artifact is retained outside its target lease
before cleanup. Source/lockfile/configuration, support scripts, binary and build
receipt identities are checked before and after execution.

Both legacy and stream invocations use `ownedSpawnSync`, real process-tree ownership,
startup diagnostics and separate evidence. Compiler-version probes are preparation,
not benchmark samples. Sealing precedes successful scratch cleanup; failures or
uncertain reaping keep original evidence/scratch. Complete means both invocations
are valid and reaped, all checks pass, and identities remain stable. Building the
example is not itself a benchmark pass.

Node parser/controller tests can run without invoking Cargo or the product:

```text
node --test tests/rust-performance-paired/md5-benchmark.test.mjs
```

Rust syntax parsing is not Windows compilation. Actual build/run results must be
recorded against their exact commit, executable, fixture and original receipts.

## First actual screening: 0890

The [fixed 0890 report](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6234539195) and [terminal evidence](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6234563055) passed the three legacy rows, all36 streaming rows, full correctness/stop checks and both measured process lifetimes; all seven build/preparation/execution owners were reaped. No skipped or replacement samples.

For the authenticated17MiB payload, full-lifecycle median wall was21.318650ms for clone-finalize (range20.9972–24.4811),21.598150ms consuming (21.2522–22.2369), and28.431950ms fused SHA256/MD5 consuming (28.2561–28.8888). Independent44B Root medians were0.0007/0.0006/0.00095ms. These are microbench measurements with the complete lifecycle above, not CLI elapsed or per-sample CPU.

Legacy CNG mean wall was slower at every size:1MiB1.332ms versus RustCrypto1.275ms;16MiB23.647 versus20.218;64MiB86.969 versus81.711. Its whole-slice API remains a distinct comparison. These results do not justify switching the production backend or claiming consuming-finalize saves elapsed time. Historical64MiB results above and all individual new samples remain; no favorable subset replaces them.
