# Nonce transport lab

Independent opt-in addon, offline scanner simulation, and a separate Windows-only read-only live benchmark. None replaces wowdump or changes the production bridge. Only the explicit `live` executable opens a process.

## Record

Little-endian fixed header (64 bytes): magic `LYCHN001` [0,8), raw nonce [8,24), run [24,32), epoch u32 [32,36), ticket [36,44), state=3 at 44, reserved zeros [45,48), header size u32 [48,52), payload length u32 [52,56), payload Adler32 [56,60), header Adler32 [60,64). Inline payload follows, then `LYCHEND1` and the same 16-byte nonce. Maximum payload: 512 KiB.

All identity fields and the header checksum are checked before payload access. Adler32 is a corruption check, not authentication. A complete record can still be a temporary construction copy: **this lab has no challenge/commit proof**. Callers supply expected run, epoch and ticket. ACK state and old identities are rejected; this is not autonomous current-run discovery.

The addon creates no frames/timers and publishes nothing at login. `/noncelab publish <32 hex digits>` retains an immutable result, refuses reuse of a retained nonce, and caps retention at eight records. `/noncelab clear` releases references, not heap bytes. Nonces used by an actual host must be generated with a cryptographically secure RNG; benchmark constants are only fixtures. A nonce must never be reused after clear/reload.

## SIMD

`anchors_amd64.s` explicitly uses AVX2 to compare first/last pattern bytes at 32 starting positions simultaneously. Full candidates are checked with `bytes.Equal`. Loads are bounded to the input slice; tails use `bytes.Index`. `golang.org/x/sys/cpu` gates AVX2 availability, with a standard-library fallback. The baseline standard library may itself use optimized assembly: this comparison is **custom AVX2 versus Go bytes.Index**, not SIMD versus universally scalar code.

The simulation filters modeled private/committed/readable regions, dynamically distributes 1 MiB chunks to 1 or 8 workers and overlaps each by pattern length minus one. Ownership by match start deduplicates matches. Distinct regions are never stitched, including across holes. It has no real region enumeration, partial ReadProcessMemory recovery, or live cache. Coverage describes immutable supplied buffers only.

## Reproduce

From the repository root, first create `.tmp/nonce-lab`:

```powershell
lua tests/nonce-lab/fixture.lua .tmp/nonce-lab/lua-record.bin
$env:NONCE_LAB_FIXTURE=(Resolve-Path .tmp/nonce-lab/lua-record.bin).Path
go test -count=1 ./tests/nonce-lab/...
go build -o .tmp/nonce-lab/bench.exe ./tests/nonce-lab/bench
.tmp/nonce-lab/bench.exe --out .tmp/nonce-lab/benchmark-random.json
.tmp/nonce-lab/bench.exe --copy-read --out .tmp/nonce-lab/benchmark-copy.json
```

The benchmark uses a deterministic 256 MiB resident corpus, 50 passes (12.5 GiB logical scanned bytes) per trial, three rotating-order trials, and 1/8 workers. Random, zero and ASCII-like pages contain 4094 old records plus one current record. Generic-magic and nonce modes use **the same new record format**. Nonce matching finds the header and trailer copies (two candidates), of which only the header associates with a valid record. `--copy-read` adds a reusable worker-buffer copy; it does **not** model kernel transitions or process memory faults. Repeated resident buffers are not a unique 13 GB working set.

Tests cover Lua/Go byte parity, cross-chunk boundaries, stale identities, ACK residuals, altered checksum/trailer, truncated or invalid candidate storage, distinct tickets with identical payloads, excluded/gapped regions, coverage accounting, all execution modes, and differential SIMD search over 28,672 random cases plus dense-anchor cases. Real worker read-fault retries and post-reload discovery remain outside this simulation.

Results: [report](../../docs/toolkit/nonce-lab-simulation-2026-09-28.md). Earlier real-client evidence stays in [the original report](../../docs/toolkit/transport-experiment-2026-09-28.md); its ~13 GB / 15.7 s result is not directly comparable to this in-process benchmark.

## Explicit live benchmark

Build `go build -o .tmp/nonce-lab/live.exe ./tests/nonce-lab/live`. It requires `--pid`, exact `--created` FILETIME, `--image`, fresh `--nonce`, WGC-confirmed `--run`, `--mode stdlib|simd`, `--workers 1..8`, and a new `--out` JSON file. Epoch 1 / ticket TICKET01 match this lab's fixed publication. Use wowdump targets to independently identify the client and the tagged lab input host to publish the new nonce; never infer the current run from the highest memory hit.

The executable opens only QUERY_INFORMATION | VM_READ, rechecks process identity at completion, enumerates VirtualQueryEx on every invocation, filters committed/private/readable regions, dynamically dispatches 1 MiB tasks, and reuses each worker's buffer. It caps the enumeration at 100,000 regions and 32 GiB, scanning at 120 seconds. Failed blocks receive two bounded retries and page-level salvage; unrecovered gaps remain incomplete. Full-record double reads occur only after header identity and length validation. It records consistent duplicate copies separately and flags conflicting valid records.

`coverageComplete` refers to the initial eligible region set, not an atomic process snapshot. Worker skippedRegions are zero because ineligible regions are filtered before task dispatch; the total excluded region count is reported separately. There is no address cache. `recordVerified` is distinct from coverage completion and from `commitVerified` (always false in this experiment). Separate adjacent virtual regions are not stitched; a nonce crossing such a boundary is not yet supported. Cross-chunk boundaries within each region are covered.

Actual read-fault recovery was exercised by live transient gaps; failure classification and retry behavior are experimental, not production acceptance. No game-memory writes, injected code, debugger attachment or process-wide pause is used.
