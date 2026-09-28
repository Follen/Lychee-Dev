# Memory + slot live: installation-to-release test plan

Status: **implementation and scoped testing in progress; full acceptance incomplete**. The [native workbench](../../tests/channel-live/README.md) and [2026-09-28 implementation record](live-memory-slot-implementation-2026-09-28.md) identify the tests actually run. Their component/Retail results do not automatically satisfy the complete journeys below, especially public CLI, multi-instance recovery and the full client matrix. Contracts: [architecture](live-memory-slot-plan.md), [UI](live-memory-slot-ui.md), [audit](live-memory-slot-audit.md).

## Acceptance scope and truth sources

Windows amd64 only. Required client profiles are Retail 120100, Classic 50504 and Titan 38002, with the actual executable build, product and `.flavor.info`/`version.txt` recorded for every run. Forever 16001 remains outside release acceptance; fixture compatibility is not real-client acceptance. No wowdump installation, calls, recipes or external memory-tool fixtures are needed for any new required test.

Pass means the declared postconditions and retained evidence agree. It does not mean a command returned zero, a key was sent, a string was found, the logo disappeared or the reload patch appeared. Keep these independent facts in results:

- Installation: disk publication committed, expected runtime loaded, all 64 slots registered, connection verified.
- Operation: business outcome, result verification/durability, runtime release, display/input cleanup.
- Reader: requested record verified, enumeration epoch, coverage complete/incomplete and exact gaps.
- Recovery: original operation/step preserved, attempt lineage, target identity and final action/wait reason.

Use an independently modeled fake game as the offline oracle: its execution log, effect counters, retained-result table and loaded-slot bitmap determine what happened. Do not derive expected truth from the same reducer or memory decoder under test. Real-client probes have predefined arithmetic/payload digests and observable non-destructive counters; compare addon acceptance with host journals and reports. File existence or a rendered label alone is insufficient.

## Test layers and order

1. **Deterministic Go/Lua protocol tests.** Fake clock/process regions/filesystem/input/game; inject faults before and after every irreversible boundary. Lua 5.1 fixtures validate real addon handlers, not a second implementation of their semantics. Seeded event schedules explore interleavings; retain the seed and minimized failing trace.
2. **Native Windows integration.** A controlled child process allocates readable/private, mapped, guarded and released regions and emits fixture truth over a separate test pipe. The production reader consumes its memory without that pipe; the harness compares results with the pipe oracle. Exercise true process death, partial reads, locks and durable-write behavior. This does not prove WoW runtime freshness.
3. **Isolated package/install/update.** Temporary npm prefix, home/project directories and simulated client installations; loopback registry or packed release payload. No replacement of the developer's installed CLI/Skill/addons. Include an environment without wowdump and a sentinel that fails the test if any external scanner is invoked. Add real `C:`/`D:` volume coverage where available; record unavailable environments as skipped, not passed.
4. **Real-client end-to-end.** Clean managed main addon plus 64 slots; WGC captures of the selected window; explicit build/actor/process identity. Run installation, reload, connection, execution, collection, release and recovery without manually repairing intermediate state. Stop destructive test progression on an unresolved opaque effect; recover that operation before the next case.

First prove startup freshness/replay rejection, shared-installation addressing and control-budget feasibility in a narrow lab. These are prerequisites for replacing production QR transport. Then add the installation journeys and full fault matrix. Do not ship an unproved transport merely because its offline mock implements the desired behavior.

## Golden journeys

Each journey uses the real public CLI contract when implemented. Do not publish speculative command names or flags. The test harness records the actual argv before launch, request keys and an output directory; it drives structured same-operation continuation, not private JSON edits.

### J1 — first install, client initially closed

1. Install the package into an isolated prefix; verify CLI, versioned Skill and payload manifests.
2. Install the main addon plus 64 inert slots into the selected client. Assert one complete receipt, correct client identity and `activation_required`; no manufactured live session.
3. Start the selected real client through the agreed interactive test setup and log in. Verify passive character/realm/build discovery without a receiver panel or probe execution.
4. Connect. Assert one logical owner, fresh runtime binding and exactly 64 known loadable slots, with the expected starting loaded bitmap.
5. Execute a deterministic read-only probe (sum 1..10 = 55) and a versioned UTF-8/binary payload fixture. Assert prepare precedes commit, result is validated and flushed before release, and input is free at business entry.
6. Finish collection/release. Assert the result remains readable from `.lycheedev`, retained runtime payload is released, tombstone remains, activity is hidden and logical connection is idle.
7. Repeat the exact request and resume the completed operation. Assert unchanged result/operation identity and no new business execution, slot use or reload.
8. End the investigation by closing its idle connection. Verify the old owner is unbound before another project can claim it. Fault-inject a lost unbind response and recover without bypassing ownership.

### J2 — install while client is online, then reload

1. Select an exact running PID/creation identity/window without relying on an absent new bridge; claim it exclusively and install the complete package.
2. Assert disk success with runtime still absent/old. Journal installation activation separately from probe execution.
3. Perform the authorized targeted activation reload. Fault-inject CLI death before input and after uncertain delivery; resume by observing the same activation, without unconditional repeated `/reload`.
4. Observe the single patch if available, then prove the new runtime/version, intended actor and slot inventory through the native protocol. The patch disappearing or 45 seconds elapsing is not success or failure by itself.
5. If the client cannot register new directories on reload, return and retain `client_restart_required`; after the test operator performs that prerequisite, continue the same activation. Count this as documented restart behavior, not reload-only success.
6. Run J1 steps 5–8. No manual reconnect, slot edits or nonce selection are permitted in the successful path.

### J3 — upgrade an existing installation and Skill

1. Begin with a pinned old managed release, existing project evidence and an idle logical connection. Separately test an active/unresolved operation: update must wait/reconcile rather than overwrite its files.
2. Update CLI/Skill/main addon/64 slots from one sealed package using the installation transaction. Inject failure after each component publication; no partial state may claim runnable or rewrite old operation history.
3. Verify the old running game remains identified as old, and any already running CLI retains its pinned implementation. No memory-slot command is dispatched merely because npm installation succeeded.
4. Activate the new runtime, rebind and run J1 steps 5–8. Verify per-instance activation outcomes when several instances share the installation.
5. Resume a compatible saved operation with a newly launched CLI; reject incompatible protocol/journal/code versions explicitly. Keep historical old-transport evidence readable without treating it as new-transport readiness.

### J4 — install/update another addon, reload and continue

1. On a connected instance, finish or checkpoint the active safe probe before a planned addon installation that needs reload. Share installation maintenance admission with the live bridge; do not mutate an installation behind its coordinator.
2. Deploy a small deterministic fixture addon through its supported installation path. Record its files/release separately from Lychee's manifest; installing another addon must not silently invalidate or rewrite Lychee's 64-slot ledger.
3. Request the authorized reload. Observe new runtime, fresh slots, same intended actor/build and re-established owner. Verify the fixture addon actually loaded and reports its expected version; `/reload` input alone is not proof of addon activation.
4. Restore setup/subscriptions and continue the same resumable workflow. Its completed durable step is not rerun; runtime-local prerequisites are rebuilt and any historical observation gap is explicit.
5. Repeat with user-triggered reload racing publication, reload during probe execution and a fixture addon failing to load. Reconcile effects, preserve diagnostics and avoid declaring installation or probe success from only the new Lychee runtime.

## Installation and activation cases

| ID | Setup / injected fault | Required result |
|---|---|---|
| INS-01 | Clean prefix and empty client AddOns | Main + exactly 64 slots, inert payloads, matching receipt/versions; no wowdump prerequisite |
| INS-02 | Online client has no Lychee runtime | Activation reload can bootstrap without circular session/readiness requirements; no business execution before fresh binding |
| INS-03 | Files updated, old addon still in memory | Report runtime-old/activation-required; refuse new-protocol execution |
| INS-04 | Kill installer after each of the 65 directories and receipt replacement; manual reload during publication | Recover transaction or report incomplete; mixed runtime/files cannot accept new business work; no successful activation claim |
| INS-05 | Disk full, denied path, locked file, failed flush/rename, cross-volume source staging | Preflight or bounded recoverable failure; no false receipt commit or evidence deletion |
| INS-06 | Edit immutable loader/TOC/main file; edit a slot outside publication | Detect exact drift; broad mutable-directory exclusions are forbidden |
| INS-07 | Valid journaled payload generation vs stale/unrecorded generation | Only exact managed publication accepted; clean payload changes do not falsely make the whole install unusable |
| INS-08 | Missing/disabled/wrong-interface slot or missing main dependency | Exact inventory/capability error before admission; do not silently use 63 slots |
| INS-09 | Add new directories online on each supported client | Record observed reload discovery; restart-required path works where needed; Retail lab evidence is not copied to other profiles |
| INS-10 | Explicitly disabled bridge, combat/loading, no foreground, key binding conflict | No unauthorized auto-enable or unsafe reload; precise waiting/capability state and same activation retained |
| INS-11 | Client exits/restarts or actor changes during activation | Old PID/window authority rejected; resume only under exact task target constraints, never infer identity from path/name alone |
| INS-12 | Two instances share installation during update | Maintenance waits for all affected owners, no partial hot replacement; each runtime activation tracked separately |
| INS-13 | Update while an old CLI process runs; compatible/incompatible operation versions | Pin active implementation; checked continuation or explicit incompatibility, never implicit journal rewriting |
| INS-14 | npm/Skill replacement interrupted; stale removed Skill resource remains | Complete coherent resource set after repair; obsolete owned files handled by manifest, unrelated user files preserved |
| INS-15 | Another addon installed then reload (J4), including load error | Verify its actual loaded version and resume Lychee; retain diagnostics when activation fails |

## Protocol, slots and ownership

| ID | Setup / injected fault | Required result |
|---|---|---|
| PRO-01 | First install, bridge not yet connected, no saved session | Passive candidate discovery and narrow wake work; binding precedes any business authority |
| PRO-02 | Replay old HELLO/COMMIT/RELEASE plus queued key events after same-PID reload | Old transcript cannot bind/exe/delete; startup freshness demonstrated on each profile, not mocked into existence |
| PRO-03 | Wrong connection, driver fence, actor, build, runtime, slot, length or digest | Reject without business effect; consumed-but-invalid loaded slot remains consumed |
| PRO-04 | Duplicate/late activation and submit; key-down/up loss; ordinary unrelated keys | At most the authorized exact operation executes; no generic 'execute whatever is next' transition |
| PRO-05 | All 64 slots, rejected slots, uncertain consumption and exhausted control reserve | Bitmap and physical reservations reconcile; admission prevents stranding under the declared finite fault budget |
| PRO-06 | Overwrite a loaded slot; failed/partially loaded addon; missing loaded acknowledgement | No reuse based on missing reply or API success; reconcile loader status and callback facts |
| PRO-07 | Data contains Lua syntax, quotes, long-string terminators, NUL and multibyte text | Generated data wrapper is round-trip safe; nothing becomes executable before main-addon validation |
| PRO-08 | Runtime report/tombstone capacity full | Backpressure before acceptance; no eviction of unacknowledged results or deduplication entries still required |
| PRO-09 | Payload publication before retained-result commit; stale confirmation; digest collision/conflict fixture | Publication ordering/identity check rejects false currentness; confirmation cannot select a different BODY |
| PRO-10 | Two projects connect to one process; two drivers resume one owner | One logical connection and one active driver; loser reports owner-busy with actionable reference |
| PRO-11 | Driver dies; clock/lease ages; owning project directory disappears | OS lock release/age/deletion does not silently free logical owner; recover before transfer |
| PRO-12 | Two processes, same install and physical slot, interleaved file writes and delayed keys | Exact intended consumer only; unknown consumption blocks overwrite; progress after finite interference |
| PRO-13 | Two different installs/windows send overlapping modifier sequences; hard kill midway | Desktop input mutex serializes bursts; exact foreground recheck; normal paths release injected modifiers, interrupted bursts reconcile before more input, physically held user keys not blindly released |
| PRO-14 | Maintenance racing driver admission, installation aliases/junctions, new process appears | One canonical install resource, stable lock ordering, no deadlock or unaccounted hot update |
| PRO-15 | Investigation ends, owner unbinds, response is lost, another project connects | Retire claim only after verified unbinding/quiescence or proved runtime disappearance; completed tasks do not strand idle ownership |

## Native reader and cache

| ID | Fixture / fault | Required result |
|---|---|---|
| MEM-01 | Mixed private/image/mapped/free/reserved/readable/guard/no-access regions | Only eligible private committed readable bytes scheduled; exclusions and access failures explicit |
| MEM-02 | Every magic alignment across chunk, worker-task and adjacent-region boundaries | No missed valid anchor, no duplicate acceptance; never combine across a hole |
| MEM-03 | Multiple old runs, ACKed BODY residue, same bytes under different tickets | Only exact freshly bound HEAD/confirmation authorizes the selected BODY |
| MEM-04 | Invalid version/state, oversized length, checksum/trailer corruption, torn publication | Reject safely with bounded allocation; large payload is not read before association validation |
| MEM-05 | Worker partition gap, short read, unmap/protection change during scanning | Automatic bounded gap supplementation; remaining gaps keep coverage incomplete even if result verifies |
| MEM-06 | One huge region, thousands of tiny regions, imbalanced workers | Dynamic queue keeps workers available; coverage is range-union based, including overlap/duplicate work |
| MEM-07 | HEAD disappears or conflicts during BODY read; fresh confirmation not found | No stale current result or inferred absence; bounded reconciliation with separate verification/coverage facts |
| MEM-08 | Cache disabled from first discovery through reload and result release | Entire journey works; no hidden warm-cache prerequisite |
| MEM-09 | Valid warm address/region hints | Same validator and outcome as cold path; accurate partial/full coverage and path metrics |
| MEM-10 | Freed/reused addresses, stale build/runtime, corrupt/oversized hint file, failed hint write | Ignore invalid hints and continue full eligible scan in the same request; no operation failure solely from cache damage |
| MEM-11 | 64 stale hints, exhausted priority byte/time budget | Remaining ranges scheduled fairly; no repeated full-scan restart or starvation; deadlines honored between reads |
| MEM-12 | PID reused, same address with different content, different process creation time | All old record authority/hints invalidated; no cross-process evidence adoption |
| MEM-13 | Scalar and SIMD scanners on identical generated byte layouts, CPU lacking SIMD | Identical hits/verification/coverage; safe fallback, no unsupported instruction |
| MEM-14 | Cancellation/process exit/permission loss during any worker read | Workers and handles join/close; typed partial result, no hang, no hidden external scanner fallback |
| MEM-15 | Cache hit in full-audit mode | Continue the full observed region set; never substitute a point read for full coverage |

Include tiny and maximum allowed reports, empty data, UTF-8, binary NUL and concurrent retained reports. Keep maximum report/candidate/scan limits in shared fixture configuration; test exactly at and just beyond each bound.

## Crash/reload and continuation matrix

Use these boundaries as fault injection hooks: intent flush; slot publication reservation; temp-file flush; replacement; before/after key delivery; prepare accepted; before/after exact commit; business effect; runtime result commit; confirmation generation; local result flush; durable result event; release request; release acceptance; final evidence flush.

At each applicable boundary inject CLI kill, game reload, game exit, lost/duplicated input or read failure. Add actor switch/window loss to input/execution boundaries and disk faults to persistence boundaries. Document structurally inapplicable combinations instead of treating them as passes. CI explores at least 100 deterministic seeds of 200 actions plus explicit targeted cases; every discovered regression adds a fixed trace. This bounded model is not a proof against arbitrary infinite schedules.

| ID | Scenario | Required result |
|---|---|---|
| REC-01 | CLI dies with zero/partial/unknown input | Same operation/attempt inspected first; no replacement execution from timeout alone |
| REC-02 | Result bundle flushed, durable event absent | Adopt only a complete exact-bound orphan bundle; do not rerun completed work |
| REC-03 | Release happened but acknowledgement lost; released BODY still in heap | Durable result reused, dedup tombstone prevents replay; confirm release honestly |
| REC-04 | Reload before result durability | Replay-safe observation restarts with linked new attempt; resumable step uses check/apply; opaque effect remains explicit if irreducible |
| REC-05 | Reload after result durability | Result remains usable; old cleanup becomes runtime-gone, not fabricated ACK; subsequent work rebinds |
| REC-06 | Effects happened, reply lost, workflow check finds achieved | Persist verified checkpoint and skip apply; effect counter stays one |
| REC-07 | Check proves not-achieved then user changes subject/precondition | Revalidate at apply; no action on a different target or automatic non-idempotent duplicate |
| REC-08 | Completed prerequisite was runtime-local | Restore setup and recheck prerequisite before dependent step; preserve durable completed business results |
| REC-09 | Missing/corrupt project journal and/or host ledger, with runtime alive/dead | Bounded evidence-based recovery; no silent reset, free-owner claim or fabricated history |
| REC-10 | Target unavailable beyond overall deadline, then returns | Durable actionable waiting; later resume finishes same operation when safe, no background-scheduler claim |
| REC-11 | User closes console/indicator vs explicitly disables bridge | Close has no business semantics; disable releases input and recovery does not undo opt-out |
| REC-12 | Code/manifest/request-key changes on resume | Explicit conflict/version check, no altered step smuggled into the old operation |
| REC-13 | CLI terminates after accepting async probe | Actual running indicator remains, result retained with bounded storage; later driver resumes collection without rerun |
| REC-14 | User manually reloads repeatedly, then stops | Finite-fault replay-safe fixture eventually completes; exhausted deadline returns waiting without infinite reload/scan |

For resumable fixtures, test three steps with durable dependencies, one idempotent mutation, one runtime-local subscription and one historical event observation. Assert both no duplicate mutation and honest gaps in observations. For opaque fixtures, a correct `execution_unknown` with retained evidence is a passed ambiguity-handling test, not a successful business outcome.

## UI and input acceptance

| ID | Scenario | Required result |
|---|---|---|
| UI-01 | Wake, dispatch, 60-second async probe, result collection, release | Continuous top-left logo with truthful localized labels; no QR/receiver card; hidden when idle |
| UI-02 | Keys/mouse/IME while connecting; emergency close; dispatch | Ordinary input does not abort the connecting view; bounded shield releases before business entry; emergency close remains usable |
| UI-03 | CLI crash, expired 20-second input window, focus/combat loss, renderer error | No permanent addon keyboard/mouse shield or invisible full-screen frame; interrupted host modifier bursts handled by PRO-13; protocol reconciles separately |
| UI-04 | Opted-in reload, slow loading, repeated events, wake, 45-second expiry | One 8 x 8 color patch in 12 x 12 surround; R/G/B at 500 ms; absolute deadline and permanent stop-on-wake |
| UI-05 | Frozen/stale/skipped WGC frames, scene colors, wrong window, absent patch | Hint miss cannot create false reload success; native binding still required |
| UI-06 | Reduced motion, Chinese/English, UI scales, windowed/fullscreen | Static pose without OnUpdate when requested; readable unclipped text; small fixed corner footprint |
| UI-07 | Probe ends while driver gone; late Finish from older attempt | Bounded collection presentation; no stale running label or hiding the next operation; retained result unaffected |
| UI-08 | Passive descriptor, bridge disabled, idle owner, repeated connect/finish | No idle animation/timer growth; view has no dependence on ReceiptView or console placement |

Real visual evidence uses WGC from the selected D3D window. Offline screenshots or frame mocks do not satisfy these cases. Record physical keyboard/mouse behavior explicitly; an assertion that a Lua frame is hidden is not proof that focus/input returned. UI labels must agree with the operation trace at the capture timestamp.

## Skill and unchanged feature regression

Give the Skill scenario prompts for fresh installation, disk-new/runtime-old, missing slot, uncertain reload, owner-busy, same-project resume, exhausted slots, wrong character, corrupt cache, durable-result/pending-release and opaque-effect ambiguity. Check that it uses only advertised public commands, preserves the operation ID/project/target, lets the CLI manage slots/cache/reload and does not call wowdump or hand-edit state. A pending observation is followed through structured resume within budget, not converted into a new request. No command text is generated from a proposed but unimplemented contract.

Run the current source/data/SQL/assets/validation baseline as well. Assert the new live path does not change selected source commits, DB2/Hotfix pagination, SQL results, LuaLS integration or existing user workspace files. Update old live baseline assertions only where the approved replacement changes behavior; retain equivalent execution, identity, durability, cleanup and locale coverage rather than dropping QR-specific tests without replacement.

Existing verification entrypoints, once implementation is ready:

```powershell
go build ./...
go vet ./...
$env:LYCHEEDEV_REQUIRE_LUA51 = '1'
go test -count=1 ./...
node tools/version.mjs --check
node tools/skill-contract.mjs
# With the pinned Lua 5.1/LuaLS paths configured as documented:
node tools/baseline.mjs
```

New native/process/installation cases should integrate with the existing test and baseline infrastructure. The current `tools/live-baseline.mjs` does not yet implement this new transport's journeys; extend it or its fixture modules during implementation, keeping the contract table as the source of CLI truth. Do not claim the commands above execute every new planned case today.

## Performance and soak protocol

Preserve the historical approximately 13 GB / 15.7-second eight-process lab observation, including its incomplete coverage. The newer native nonce lab's results are a separate baseline with a different observed heap/conditions. Neither is a production SLA or a controlled speedup comparison.

For a reproducible comparison, record CPU/RAM/OS, exact game/CLI commit, heap region set/eligible bytes, worker count, scan mode, payload sizes, cache state, game scene and background load. Test scalar and SIMD with 1/2/4/8 workers on controlled layouts, then 1/8 workers on the real client. Collect at least 20 samples per relevant cold/warm/cache-off mode, interleave mode order, and report median/p95, bytes/sec, worker utilization and peak scanner memory. CPU-cache warmth is distinct from this tool's address hints.

Measure both full-scan duration/coverage and user-visible timings: install-to-activated, connect-to-ready, dispatch-to-durable-result, durable-result-to-release and reload-to-resumed-result. Include hint work/fallback time and game frame-time impact where measurable. Warm-cache results must never be compared to full-audit times as if they covered the same bytes.

Do at least 20 same-PID reload/rebind cycles per required profile and record success, deadline waits, old-record rejections and residual coverage gaps separately. Add a 30-minute bounded-soak session and at least three pool exhaust/drain/reload cycles (all 64 physical slots), with synchronous and asynchronous probes. Trace report/tombstone limits, open handles, goroutines, addon frames/timers and journal growth; retained evidence may grow by design, live resources must stay within declared bounds.

Safety acceptance is zero wrong-target dispatch, zero stale-result acceptance, zero silent coverage falsehoods and zero duplicate non-idempotent apply in the test set. This is not a claim of mathematically zero field errors. Required functional cases must all pass. Treat unexplained cold-scan p95 regression above 10% versus a same-machine contemporaneous cache-off baseline as a performance investigation gate; correctness cannot be traded for that target. Quantify the warm-cache benefit rather than promising an unmeasured instant connection. Retain failed/timeout samples in the report; never benchmark only successful fast runs.

## Evidence and release decision

Each test run gets a new evidence directory and immutable manifest containing: test ID/status, source commit/tree digest, fixture seed, actual commands/exit codes, client build/process creation identity, installation receipt/generation, project/connection/operation/attempt IDs, request hash, input trace, slot generations, reader metrics/gaps, result/confirmation digests, reload/WGC timestamps, assertions and failure reason. Hash referenced artifacts. Do not archive raw process pages; keep only protocol records and the relevant evidence. Visual evidence may contain player information and remains local unless separately authorized for publication.

Statuses are `passed`, `failed`, `blocked`, `not_run`; skipped prerequisites and inaccessible clients never become passes. A verified result can coexist with pending cleanup or incomplete scan coverage and must be reported that way. New test names and runner aggregation must fail closed when a required case is missing.

Production replacement requires the three feasibility gates, all offline/native/isolated cases, the installation-to-release journeys on the full accepted client matrix, multi-instance/shared-installation races and actual UI/input evidence. If a required real environment is unavailable, keep the replacement unaccepted rather than silently narrowing the supported matrix. Publishing then follows the existing clean-tag, CI, assembly, digest, isolated npm and registry read-back contract; this plan neither publishes a release nor marks a test run complete.
