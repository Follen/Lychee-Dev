# Memory / slot implementation and scoped acceptance — 2026-09-28

Status: **in progress; not release acceptance**. Branch
`codex/nonce-memory-transport`. The native reader, protocol, slot delivery,
runtime and journal driver are implemented and exercised by the development
[workbench](../../tests/channel-live/README.md) and the public CLI. Native
connect/execute/status/resume/reload/bugs/disconnect and the versioned Skill
now use project-local memory/slot orchestration. Legacy atomic routes remain
available for old records; their removal is still outstanding. Do not publish this tree or
describe the migration as complete. No tag or npm publication was performed.

## Implemented boundaries

- `internal/live/memory`: read-only Windows process identity checks, fresh
  MEM_PRIVATE/committed/readable enumeration, eight dynamically scheduled
  workers, streaming overlap, AVX2/scalar matching, strict bounded records,
  optional hints and explicit coverage. A short read triggers one fresh region
  map and bounded page salvage. Disappeared mappings remain coverage gaps.
- `internal/bridge`: shared binary memory records and escaped, data-only slot
  envelopes. BODY allocation requires the exact HEAD binding and size/digest.
- `internal/delivery`: 64 generated LoD addons, exact static/payload integrity,
  durable publication reservations, resumable installation and pool upgrades.
  Installation status exposes the pool separately; main-upgrade resume also
  completes the pool. Unknown extra files or changed bytes are refused.
- `addon/Bridge`: passive actor/build/runtime descriptor; fixed-key slot loader;
  bound owner, prepare/commit, retained results, fresh confirmation and release;
  bounded async callbacks, deadlines and cleanup. Slot loaders are passive
  outside the explicitly expected load. Input shielding ends before business
  execution and on errors. No QR is used by this new path.
- `internal/live/channel` and `journal`: original-nonce continuation, flush
  before input, raw result bytes durable before release, cross-project process
  ownership and short OS driver leases. The native workbench stores these under
  its calling project's `.lycheedev/live`. Public CLI requests use stable keys;
  completed retries are read-only even after later work. Quiescent journal
  rotation preserves hash-linked immutable segments and historical requests.
  A torn final write is preserved separately before restoring a verified prefix;
  complete corrupt records or missing history never authorize a replacement run.
- Runtime loss: fresh binding precedes retirement of the exact old reservation.
  Safe observation work retains its logical operation and gets a new ticket and
  attempt (maximum three); possibly committed opaque work remains execution_unknown.
  Durable results survive runtime loss, with cleanup recorded as runtime_destroyed.
- Installation activation and capacity reload: fixed reload input is journaled
  once; a new runtime and fresh bind are required. Registered inventory is 64.
  Admission preserves control and tombstone capacity; the thirteenth normal task
  triggers an automatic reload at a quiescent boundary. Managed removal archives
  the main addon and 64 slots with one resumable intent and full drift checks.
- Presentation: top-left bouncing Lychee with running/collecting states; one
  RGB reload patch, stopped at wake and bounded to 45 seconds. WGC images are
  retained locally; no user screenshots were committed to the repository.

## Real Retail observations

Retail **12.1.0.69933 / 120100**, one explicitly selected process and actor.
Process creation identity, full character/realm/GUID, nonces and runtime tokens
remain in local project evidence. These were automated observations, not a
substitute for the owner's manual interactive acceptance.

| Scenario | Observed result |
| --- | --- |
| Managed candidate installation plus 64 slots | Candidate main loaded; native descriptor discovered after entering the world |
| First binding failure | `DEP_MISSING`; no business probe executed. Exact pre-release TOC dependency repaired, payload and reservation preserved |
| Reload and binding recovery | Two RGB transitions observed; exact old nonce received a runtime-changed rejection; a fresh bind verified the new runtime |
| Ordinary structured/Chinese result | `LMO-3b978d897b7f37841662508c2d997b57`, complete, durable result and confirmed release |
| 25-second async probe and resumed collection | `LMO-9d81ca909a9a2911fc34826645a7131c`, complete; `inputReleased=true`; WGC shows only the top-left Lychee and running label |
| Automatic async continuation with cache disabled | `LMO-0eebd22e142dd27ea034517e17ec9321`, one bounded invocation waited through pending observations to confirmed release; `inputReleased=true`, then verified disconnect |
| Expected Lua exception | `LMO-97bc79e87e368e3e63093c29575eb9d9`, verified business failure, resources and transport released |
| Async deadline | `LMO-390fe208604214b3230b8e736e797f23`, verified `probe_timeout`, complete release; completed retry performed no scans/input |
| Large UTF-8 / HTML / newline payload | `LMO-9ab05465bf3673b30b951baa6307f58a`, 90,211 report bytes, 8,192 repetitions preserved, cache disabled, complete release |
| Competing project, same process | Second project refused with `journal.foreign_owner`; existing connection preserved |
| Reload, cache disabled, new runtime and fresh probe | `LMO-e80d399c6fe3ba72b911ad9dc28497e8`, complete under the new runtime; connection then unbound and host claim retired |
| Public CLI normal execution and identical-request retry | `LMO-3e6fb4c665863cb2eb042ee0c429504d`, complete; retry read saved result without game input |
| Public CLI pending async and resume | `LMO-4237be345000e206d04caba56f4cced5`, first call pending, resume completed the same operation; reload refused during active work |
| User-style reload during a read-only probe | `LMO-589465ecb8c724e1a1268cd7f3339222`, runtime changed, same operation/request continued as attempt 2 with a new ticket and confirmed release |
| Managed upgrade and public activation | `CON-72e5f1142703e89ebd010c6862a643fb`, one fixed reload, fresh binding, inventory 64 |
| Thirteen consecutive public operations | All complete; task 13 automatically changed runtime at the capacity boundary and continued its already journaled request |
| Lua syntax error | `LMO-5c650d223d3120b2f3f4b13e2a8a04cd`, verified compile_error, no business commit; result durably stored and released |
| Cleanup callback throws | `LMO-a0c6199623419822f3483e062c90c04e`, original report keeps resourcesReleased=false; automatic reload and fresh bind complete cleanup as runtime_destroyed |
| Public native error collection | `LMO-96ec11f765ec47e897a5b09ab0db5681`, provider report verified and released; incomplete historical provider fields remain visible. This test exposed an outer completeness flag, subsequently fixed to reflect snapshot.complete |
| Final automated public CLI baseline | `CON-095b554b5723802671fadc32ca877bdb`, 20 command invocations passed: cache-off connect, 13 normal tasks, historical request retry after journal rotation, syntax error, cleanup failure, diagnostics and two disconnects. Baseline assertions verified runtime rollover, original operation identity, unchanged journal on historical retry, truthful diagnostic completeness and closed ownership |
| Runtime loss with default opaque policy | `LMO-0006f984c4faf314e1a0f9e10b83d153`, remains execution_unknown at attempt 1 after actual reload and fresh binding. Repeated resume returned exit 5 without another prepare/commit. Explicit disconnect then retired the connection; no report or business completion was invented |

The final opaque closeout exposed a presentation defect: closing ownership set
`complete=true` even when the retained operation was execution_unknown. It is
fixed and covered by `TestClosingUnknownOperationDoesNotInventCompletion`.
Post-fix read-only status and repeated disconnect retain `closed=true`,
`complete=false`, `reportState=unavailable`; their separate `*-fixed.json` files
preserve the corrected observations without editing the original test evidence.

First reload attempted at character selection did **not** count as success.
The later in-world reload had both patch evidence and fresh runtime binding.
One discovery during world loading found a valid candidate but had memory gaps;
it remains incomplete coverage, not proof that all current records were found.

Local evidence roots:

- `.tmp/channel-live/retail-a/summary.json` and `evidence/`
- `.tmp/channel-live/retail-after-reload/summary.json` and `evidence/`
- `.tmp/channel-live/retail-b/evidence/` for competing admission
- `.tmp/channel-live/retail-final/` for bounded automatic async continuation
- `.tmp/channel-live/retail-public/` for public execution, reload and interruption recovery
- `.tmp/channel-live/retail-capacity/` for activation and thirteen-task rollover
- `.tmp/channel-live/retail-errors/` for syntax, cleanup failure and native diagnostics
- `.tmp/channel-live/retail-baseline-final/baseline.json` for the repeatable public CLI baseline
- `.tmp/channel-live/retail-opaque-final/` for opaque interruption, repeated resume and disconnect
- `.tmp/channel-live/benchmark/evidence/20260927T211951.721672000-scan-benchmark/`

The summary is reproducible with `tests/channel-live/summarize.mjs`.
Result artifacts preserve bytes as base64; their connection journal records
confirmed release separately from the immutable verified-result artifact.

## Native scan measurements

Same idle Retail process, consecutive complete scans, no hint cache:

| Workers | Planned / scanned bytes | Time | Coverage / gaps |
| --- | --- | --- | --- |
| 1 | 11,064,647,680 / 11,064,647,680 | 63.803 s | complete / 0 |
| 8 | 11,064,672,256 / 11,064,672,256 | 8.843 s | complete / 0 |

This run measured approximately **7.2×** throughput scaling, or 90% of ideal
eight-way scaling. The process's mappings changed by 24,576 bytes between runs;
these are live observations, not identical frozen inputs. The prior approximate
**13 GB / 15 s** baseline remains historical evidence and is not directly
comparable. Earlier in this run, eight workers scanned 11,338,694,656 bytes in
8.612 seconds with complete coverage. A verified lookup may end early and
correctly reports incomplete coverage; it is not a full-scan benchmark.

## Offline verification

Build/vet and `LYCHEEDEV_REQUIRE_LUA51=1 go test -count=1 ./...` passed after the
current code changes; raw output is `.tmp/channel-live/full-go-native-final.txt`.
Node tooling: 75 passed, one environment-dependent skip, zero failures in
`.tmp/channel-live/node-native-final.txt`. Version check reports 2.5.1 unchanged;
generated Skill contracts report 87 commands, 202 references and zero violations.
These checks do not imply an interactive CI or release gate passed.

New coverage includes real Go ↔ Lua 5.1 host restarts after bind, prepare, commit,
confirm and release; stale-runtime bind recovery; exactly one business execution;
result durability before release; disabled/enabled input handling; loader errors;
12,000 seeded SIMD/scalar comparisons; a real Windows child-process scan across
a 1 MiB boundary; stale process creation identity; transient read repair and
disappeared mappings; exact result-byte persistence; modified slots; and upgrade
interruption between main publication and pool installation; partial 65-member
removal recovery and refusal to move modified slots; connection journal rotation
with historical idempotent retry and refusal to replay when archived history is missing.

## Still required before replacing the public transport

1. Two simultaneously running processes sharing one physical slot pool; real
   driver crash/race and foreground/modifier interference. Offline reservation
   tests are not equivalent to that interactive matrix.
2. Recovery after complete project/host ledger loss and a process restart with a
   new creation identity. Current code preserves ownership and refuses ambiguous
   replay; it cannot reconstruct a lost opaque result from nothing.
3. Comprehensive crash injection at every installation/update transaction boundary,
   first addition of addon directories on each client, and another addon's install
   followed by reload. Managed upgrade/activation and isolated partial pool/remove
   recovery have passed, but do not cover every journey.
4. Startup freshness on every accepted client, protocol generation/manifest binding,
   removal of legacy QR production routes and settlement of the proposal's explicit
   driver-fence advancement. Current driver exclusion uses OS leases and owner tokens;
   there is no timed ownership stealing or forced takeover.
5. Classic 50504 real-client runs and owner visual acceptance. Titan 38002 and
   Forever 16001 now have scoped [real-client regression evidence](live-memory-slot-clients-2026-09-28.md).
   General stateful prerequisite reconstruction is probe-owned;
   interrupted opaque effects remain unknown rather than automatically replayed.

Forever's specific real-client results supplement the four-profile Lua fixtures;
they do not promote it into the formal client acceptance matrix. None of the
remaining items is marked passed by a different scoped workbench run.
