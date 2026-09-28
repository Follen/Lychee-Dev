# Live memory + slot architecture proposal

Status: implementation in progress; revised by the [robustness audit](live-memory-slot-audit.md). Branch: codex/nonce-memory-transport. Native components, public CLI/Skill orchestration and a scoped Retail chain have been exercised; see the [implementation record](live-memory-slot-implementation-2026-09-28.md). Multi-instance/client and protocol-generation gates remain open. Removes the old live transport only after acceptance; source/data functionality and existing evidence remain separate. The [UI contract](live-memory-slot-ui.md) and [end-to-end test plan](live-memory-slot-tests.md) define the required acceptance, not a claim that it has passed.

## Product contract

2026-09-28 control-architecture review: [统一决策与输入证据](live-input-architecture-2026-09-28.md).
This defines the next consolidation, not completed implementation. Preserve the
memory/64-slot architecture while replacing dispersed input/recovery decisions
with one coordinator and structured evidence. Do not treat recent focus fixes
as acceptance of the overall control architecture.

- Default installation includes 64 predeclared LoadOnDemand input addons and one main addon. CLI writes a complete envelope to the next unused slot, atomically replaces the file, then sends a fixed activation key. Game memory is read-only from outside.
- Native slot invocation is fixed **Ctrl+Alt+F12** (2026-09-28 owner decision).
  The optical receiver's old bracket profile is not reused. Both components
  must be upgraded together; unresolved old-runtime input is not re-keyed or
  replayed. Saved custom optical bindings cannot override the native profile.
  Before a slot burst, the CLI reads fresh memory input telemetry. Ordinary
  editor focus permits one journaled Esc, followed by a new observation; repeat
  until clear within the caller deadline. The owner explicitly accepts Esc
  closing/cancelling the focused UI. Never directly mutate a foreign editor.
  Combat, secret/unavailable focus and binding conflicts still fail closed. A third-party keyboard handler
  may suppress even a function key: delivery remains an attempt until a fresh
  slot receipt proves acceptance, never grounds for blind replay.
- Input readiness uses memory, not an additional color patch. The owner-requested
  native `InputState` sampler is a separate explicit idle-cost exception: one
  invisible frame samples at most every 100 ms while the bridge is enabled,
  retaining one bounded record. Explicit bridge-off removes its OnUpdate.
  No keyboard/mouse hooks or foreign frame enumeration are installed. Samples
  carry runtime, owner/fence, actor/build, next slot and uptime; the Windows
  reader requires a sample after the observation/Esc boundary, at most 500 ms
  old. Missing, stale or clock-incompatible telemetry never means unblocked.
  Every Esc attempt is journaled before input; interruption resumes the original
  published slot with fresh observations. Uncertain slot activation is not replayed.
- The Windows amd64 CLI owns process discovery, memory enumeration/read/search/validation and recovery. **No wowdump dependency**: no executable, library, MCP, recipe, installed profile, installer prerequisite, Skill delegation or fallback. Historical wowdump experiments remain historical evidence only; new production tests and release gates must run without it installed.
- Main addon publishes a small bounded identity descriptor on login/actor changes: protocol/addon version, actual build/product, character/realm/GUID when available, and runtime identity. No polling, visible frame or input capture is required for this descriptor. This owner-requested default metadata is an explicit narrow revision of disabled-cost policy. Preserve the separately budgeted minimal wake foundation; probes, receiving/input shielding and activity UI remain opt-in.
- Discoverable descriptors are candidates, never input authority: stale copies may survive logout/reload. A fresh random challenge, exact target constraints and process creation identity establish a live binding before business execution.
- One small top-left color patch cycles red -> green -> blue, one color at a time, for at most 45 seconds from startup arming. It stops permanently for that runtime at the first accepted wake, not after a possibly slow scan. It only suggests readiness. Reload completion requires new runtime evidence correlated to the request; color alone never proves reload, actor identity or input authorization.
- Remove QR transport and its reader/writer after replacement acceptance. Preserve the existing Lychee squash/bounce animation and short activity label at the top-left. A bounded connecting phase shields game input; execution releases that shield before the probe starts. The activity view is click-through, survives asynchronous execution and clears after result release. Idle ownership has no visible indicator. Geometry, labels, timing and failure behavior are specified in the UI contract.

## Installation through first execution

Installation is an orchestration prerequisite, not a successful connection. Record four different facts: `files_committed`, `runtime_loaded`, `slots_registered`, `connection_verified`. They are milestones owned by the existing delivery/live components, not four independent recovery engines. A matching package version on disk cannot prove any runtime milestone.

1. Resolve the requested project and actual client product/build from the executable and installation metadata; canonicalize junctions/path aliases. Enumerate all processes sharing that installation. Preflight package digests, permissions, free space and conflicting operations before changing files. An absent character descriptor is expected on first install; do not require an existing session to install its own bootstrap.
2. Generate the main addon and exactly 64 slot directories from one release manifest and one slot template, not 64 handwritten implementations. Each slot has an immutable TOC/loader and a narrowly defined mutable payload file, initially an inert envelope. Validate dependencies, client compatibility and the complete expected slot catalog. Neither partial slot counts nor unknown extra load paths inside the managed slot namespace may be silently accepted as a smaller pool. Unrelated addons are outside that namespace.
3. Publish under an installation maintenance reservation after draining/reconciling all known affected instances. Stage and flush files, then use a recoverable multi-directory installation transaction. Windows does not provide one atomic rename for 65 independent directories: mark the installation pending until every component and receipt verifies. Any interrupted mixture stays non-runnable and is completed/repaired from that transaction before activation. Keep only bounded transaction recovery material, not permanent old-version backups.
4. Separate immutable-file integrity from authorized slot generations. A payload is valid only if its exact bytes match the currently journaled publication generation/digest for that physical slot, or the declared inert baseline. Do not broadly ignore slot directories in managed-install checks. External edits remain `modified`; unknown payloads cannot execute. Bind an envelope to the installation generation and protocol as well as runtime/owner. Inventory and installation receipts cover the main addon, all 64 slots and both file roles.
5. If no client is running, retain `activation_required`; startup will perform inventory and binding. If the selected client is running an older or absent bridge, use the narrowly scoped installation-activation reload path. It requires a journaled, exclusive claim on the exact PID/creation identity/window, a clean committed installation and safe foreground/input conditions, but cannot require the not-yet-loaded new bridge's readiness. The only fallback is the already authorized fixed Esc x3, Enter, `/reload`, Enter action; it confers no business execution authority. Do not cycle it blindly after unknown input.
6. After reload, observe a fresh runtime through the native reader, verify release/manifest/protocol and the live inventory of all 64 slots, then bind the selected character/build and connection. The color patch can accelerate observation but cannot finish activation. A new instance must explicitly satisfy target selection; never infer it from an old HWND or follow another character automatically. If a client does not discover newly installed folders on reload, record `client_restart_required` and continue the same activation after the required restart; do not promise or repeatedly attempt reload to fix an unproved capability.
7. First contact uses the narrow wake/bootstrap foundation; it can opt in to a requested connect and negotiate identity, never execute business Lua before binding. Preserve the current owner-approved minimal wake foundation rather than requiring an enabled bridge to enable itself. Explicit subsequent user disable is sticky: automatic recovery must not undo it. The startup-freshness gate below still applies to this foundation.
8. Only after `connection_verified` admit a probe. Persist its intent, acquire resources, prepare/commit, read and validate its result, make the result durable, release runtime storage/activity, and return the result plus any distinct cleanup obligation. A repeat of the same request resumes these milestones; it does not reinstall, reload or execute afresh merely because the last command exited.

Updating CLI/Skill files does not update an already running CLI process or game runtime. A running operation pins its executable/protocol and request hashes. Replacement processes must check journal/protocol compatibility before resuming; unsupported migrations retain evidence and return a specific incompatibility. The update workflow lists activation status per installation/instance, replaces shipped Skill resources coherently, and does not label disk-only installation as live success. Installation/upgrade never rewrites project operation history.

Existing managed r4 installations must be quiescent before switching transports. Drain or explicitly resolve their pending operations using their own supported recovery rules; do not reinterpret old optical/SV receipts as memory-slot acceptance. The old runtime may remain active until activation; no new slot work is admitted against it.

## Ownership and persistence

Use the explicitly selected invoking project (existing project resolution rules), never silently the CLI checkout or addon installation directory. A missing/ambiguous project is resolved before sending input. Establish write access before reserving a slot.

```
.lycheedev/live/
  project.json                    schema and stable project ID
  targets/<target-id>.json         build, actor, installation and last observations
  operations/<operation-id>/
    request.json                  stable operation/step IDs, code hash, request key, target
    attempts/<attempt-id>.json    immutable transport nonce, runtime and slot binding
    events.jsonl                  append-only checksummed recovery log
    state.json                    atomic derived snapshot, rebuildable from log
    result.bin                    exact verified result, durable before release
    result.json                   summary and evidence references
    evidence/                     input receipts, challenge and reload observations
```

Keep a minimal host-wide installation/runtime lease and slot ledger outside project directories. Different projects share the same physical slots and cannot independently allocate them. Use OS-released locks, process creation identities and fencing tokens. A single reducer/reconciler controls transitions; derived files are not a second source of authority. Project data is local and excluded from Git by default without overwriting existing ignore rules.

### One instance, one connection

An instance is `(host ID, PID, process creation identity)` bound to the canonical installation identity, not a character name or HWND. Exactly one durable logical connection owns an instance; exactly one CLI driver may advance that connection at a time; initially allow one active business operation per connection. Different instances may have independent connections. Reconnecting the same owner resumes its existing connection ID rather than creating another one. The runtime independently accepts only the bound connection and current driver fence.

A logical connection survives normal CLI command exit. The OS driver lock covers an active command; its release on crash does not release the durable connection claim. Host arbitration atomically checks/claims ownership before bootstrap or slot mutation. Another project receives busy with owner/operation references and cannot steal based on inactivity or lease age. Explicit transfer requires quiescence, persisted results and reconciled unresolved effects. Missing owner files trigger recovery, not automatic vacancy. Same-owner recovery obtains a new driver fence under the lock and re-establishes it with the addon before further business dispatch. A stale driver must fail both host checks and addon fence checks.

Releasing one operation and ending the logical connection are different actions. An idle connection can support subsequent commands without reconnecting; at the end of the authorized investigation the Skill closes its own connection unless continued use was requested. Closing verifies quiescence and unbinds the runtime before retiring the host claim; a lost unbind response remains recoverable, not permission for another owner to proceed. Runtime disappearance can retire that runtime binding once proven, without pretending it acknowledged. Do not keep a finished task's idle claim forever merely because no timeout takeover is allowed.

Use distinct resource keys for the project journal, instance driver, installation publication and the interactive desktop's actual keyboard input. The last one is required even for different installations: two drivers cannot interleave modifier down/up sequences on the same desktop. Enforce ordering: instance driver -> installation publication (when needed) -> desktop input (only for a key burst) -> brief journal transaction. Never hold a journal lock while waiting on the game, or hold the desktop input lock during a full memory scan. Release injected modifiers in a finally path; recheck foreground/process ownership under the input lock immediately before delivery. Bootstrap and recovery cannot bypass these rules.

Keep key-down/up pairs in the smallest supported native input batch and journal partial delivery. A hard-killed CLI cannot run a finally handler; recovery must inspect an unfinished input burst before sending more keys. Do not claim that an exited process repairs keyboard state by itself, or release physically held user keys indiscriminately. Unsafe/ambiguous physical input conditions yield bounded `waiting_for_input`; the addon shield still expires independently. Input serialization is an application correctness mechanism, not a security boundary against arbitrary other software injecting keys.

Maintenance obtains affected instance-driver locks in a stable instance-key order before installation publication, with bounded acquisition and a rechecked process inventory. A driver never waits for another instance while holding installation publication. New admission observes the maintenance reservation. Missing or changing process inventory prevents publication until reconciled; it cannot license a partial update while an unaccounted instance executes.

Two processes may share one installation and its 64 physical files. Their loaded-slot bitmaps are separate; the file bytes are not. Persist a physical file generation and exact intended consumer before publishing; retain the publication reservation through consumption reconciliation. Unknown consumption cannot authorize overwrite. Other instances wait for that transfer reservation or use a provably free physical slot through the actual transport's addressing rules; do not assume an ordinal "next slot" lets them choose arbitrarily. Parallel game execution is allowed after transfer completion, not concurrent publication into one physical slot. This multiplexing contract requires a dedicated two-process acceptance test.

Before every side effect, durably append intent; record observed outcomes afterward. Atomic replacement alone is not a durable commit: use the platform flush/replace contract. Detect truncated/corrupt event tails; never silently reset a damaged ledger. Results must be flushed before sending release. No automatic deletion of active/unresolved evidence.

## Slot semantics

- Slot identity is installation + bound runtime + index, not project-local index. Slots are consumed once per Lua runtime, including successfully loaded envelopes rejected as invalid. Never reuse a consumed slot in that runtime or overwrite a physical publication with unknown consumption. Once its reservation is reconciled, the same physical file may be republished for a different instance that has not loaded it; the first instance still cannot reload its consumed slot.
- Every envelope includes protocol version, operation ID, random nonce, expected runtime/actor/build, slot index, immutable payload length/hash and lease fence. A slot transports data; the main addon validates before dispatching business code.
- Delayed/repeated keys cannot accidentally execute a newly written next slot: use an arm/commit token flow. An initial key loads/prepares the slot only. The runtime issues a nonce-bound challenge, and a subsequent slot carries the explicit commit. Duplicate callbacks are deduplicated by operation ID and digest. Challenge freshness must come from the expected transaction, not highest epoch in scanned memory.
- 64 means 64 physical slots, not 64 guaranteed probes. Derive admission from the exact compiled action sequence and bounded recovery allowance: `remaining >= transfer + commit + resultConfirmation + release + recoveryReserve`. A fixed reserve of 8 is provisional, not a proof. Validate costs at every resumable step, including setup restoration, and stop admitting new work before control capacity runs out. Read-only memory retries consume time, not slots. Protocol retries must not allocate another slot just because the first reply was not yet found. No finite reserve promises recovery from unlimited manual interference.
- Exhaustion does not automatically interrupt running work. Drain results and release transactions first, then reload at a safe operation boundary and establish the new runtime. If unresolved work or insufficient control budget prevents that, return structured recovery state rather than reload blindly.

## Small state model

Operation: prepared -> dispatched -> accepted -> running -> result_verified -> result_durable -> released. Business failure is a result, not transport failure. `execution_unknown`, cancellation and interrupted reload are explicit outcomes that preserve prior evidence. Transport attempt/slot receipts are child facts, not another competing operation state machine.

`reconcile(operation)` reads durable facts plus fresh observations and returns one legal next action. After restart, resume the same logical operation and inspect its existing attempt first. Preserve a nonce for retries of that exact attempt; a confirmed new runtime requires a new attempt/nonce linked to the same operation. Never repurpose an old nonce for a new target, code digest or execution attempt. Skill never computes slot arithmetic, toggles internal states or hand-writes transport sequences.

Runtime owns bounded accepted-operation deduplication and immutable completed reports until release. Limit report bytes and outstanding work; capacity exhaustion applies backpressure, never evicts unacknowledged results. A checksummed string found in memory is not proof of publication: release and commit challenges must refer to the runtime's retained accepted-result table. Challenge receipts also need their transaction binding; generic string validity never authorizes execution.

Specify publication order, not merely a "challenge" label: commit the immutable result and digest into the accepted-operation table before generating a confirmation record for a fresh caller challenge. The confirmation contains connection, runtime, operation/step, attempt, challenge, outcome and exact result digest. It proves that predicate held when generated; its own later retention is not another condition requiring an infinite acknowledgement chain. First persist the validated result and confirmation as immutable files and flush them, then append the durable result event; recover a complete orphaned result bundle after a crash by checking its full binding. Only then release. Release removes the payload but retains a bounded per-runtime operation/digest tombstone; reserve tombstone capacity at admission so retries cannot execute a released request again. Across runtime loss, rely on workflow reconciliation, not the lost tombstone.

Unbound bootstrap is a feasibility gate: a fresh CLI nonce alone cannot distinguish an old HELLO/COMMIT pair replayed after reload. Define and verify addon-generated per-startup freshness and its binding to the current process before business dispatch; timestamps alone are not accepted as collision-proof. Bootstrap has a narrow identity/bind capability, with no probe execution, effect cancellation or report deletion. The always-present discovery record cannot create readiness or bind an owner. The exact supported addon API/entropy and replay resistance must be proved on pinned clients before treating this gate as resolved.

## Loss and recovery

| Condition | Required behavior |
|---|---|
| CLI exits after slot write or uncertain key delivery | Recover exact intent, inspect runtime acknowledgement, never allocate a replacement request merely on timeout |
| CLI exits after result persisted but before release | Use durable result; reconcile/retry release with deduplication |
| Game reloads/crashes before a result is durably read | Preserve execution_unknown; do not rerun side-effecting code automatically |
| Project journal missing, runtime still present | Recover only fresh challenge-confirmed descriptors/results belonging to known identity; do not infer prior side effects from memory absence |
| Project and runtime evidence both lost | History is unrecoverable; new work may begin only after explicit handling of unresolved effects, not a claim of successful recovery |
| Host slot ledger missing | Reconcile actual loaded slots with live runtime before allocation; if that cannot be established, drain/resolve then reload |
| Same request key with different code or target | Conflict, never silently replace |
| Actor/build/runtime changes | Invalidate readiness and all raw address hints; retain historical evidence and reacquire current target |

SavedVariables may later provide best-effort supplementary recovery but cannot promise durability across hard crashes and must not require per-result reload. Exactly-once game side effects across a game crash and lost evidence are not guaranteed by this design. Automatic replay is limited to explicitly replay-safe work.

## Memory reader

One native reader in the shipped Go executable owns the following pipeline. Use the read-only Windows process APIs through the project's native adapter conventions; do not shell out to another scanner. No debug attach, injection or memory write is part of this transport. Process access failure is a typed capability error, with no hidden privilege escalation or alternate transport.

1. Open and verify the current process creation identity, executable and build. Re-enumerate its current regions for each scan. Select only committed, readable `MEM_PRIVATE`; exclude image/mapped regions, guard/no-access pages and holes. Revalidate regions on short reads or protection changes. Historical addresses never define the scan universe.
2. Use eight workers in one process, dynamically pulling committed-region work. Split a very large eligible region into bounded stream chunks so one worker cannot monopolize it; never divide the virtual address span into eight equal ranges. Keep buffers, queued work, candidate count and payload allocations bounded. Check cancellation/deadline between reads.
3. Compile versioned HEAD/BODY/ticket/trailer anchors into one multi-pattern search pass. Use the lab's native SIMD path where the CPU supports it, with a tested equivalent scalar fallback. Common field values are parsed after an anchor, not treated as millions of unbounded candidates. Preserve at least longest-magic-minus-one bytes at chunk/task boundaries. Adjacent readable regions need an explicit boundary read; never concatenate bytes across an unreadable hole. Deduplicate overlapping hits by address/record identity.
4. Read a small bounded header first. Verify schema/version, run, epoch, connection/fence, operation/attempt, ticket, state, length bounds and HEAD-to-BODY binding before allocating or reading a large payload. Then verify exact length, checksum/digest, trailer and immutable publication confirmation. Old BODY data, including post-ACK residue, cannot become a current result. Conflicting valid candidates cause reconciliation, not selection by largest epoch or highest address.
5. Each worker reports `plannedBytes`, `scannedBytes`, `complete`, `truncated`, `skippedRegions` and exact `gaps` with causes. Aggregate coverage by the union of successfully read eligible byte ranges, not sum of attempts/overlap. Automatically re-enumerate and supplement gaps within a finite retry/time budget; preserve disappeared/unreadable originally planned ranges as gaps. Report `coverage.complete=false` if any remain. Region churn and newly added mappings are separately recorded; full coverage describes one declared observed region set, not an atomic snapshot or timeless absence proof.
6. A lookup can finish once the exact requested record and publication confirmation verify; expose `result.verified` independently of `coverage.complete`. Early stop is never reported as full coverage. Full-audit mode must attempt the entire region set. No match with gaps is unknown, never evidence of absence.
7. Optional cached regions/addresses only influence scan order. Validate every hit against the current binding and complete record; failure automatically falls back to the full eligible region set. Reload/build/actor uncertainty invalidates hints until rebinding. With caches completely disabled, all discovery, execution and recovery paths still work. Batch expected markers from the same protocol phase in one scan rather than launching eight processes or rescanning the entire heap once per candidate.

Phase-specific deadlines share the operation's overall budget: discovery, input/confirmation, execution, result retrieval and release are visible separately. Expired input shielding cannot inherit an arbitrarily large scan deadline. Memory reads never consume slots. Bounded runtime publication keeps the required HEAD/BODY/confirmation alive until release or an explicit runtime teardown; scanner speed must not be the correctness mechanism.

### Cache: one reader, one validator, optional hints

Keep this a small scheduling optimization inside the reader. It has no connection ownership, protocol state, retry engine or separate success definition. The coordinator requests an exact observation; the reader enumerates regions, prioritizes eligible hints, runs the same record/publication validator for every path and continues remaining scan work on a miss. A corrupt or unwritable hint file is an ordinary cache miss, not a broken operation.

- Use one bounded hint collection with two entry kinds: exact record locations and promising region ranges. Start with at most 64 entries and a 64 KiB serialized limit. A verified immutable BODY can be reread at its previous location, but it only becomes a usable result with the exact HEAD/publication binding required by the protocol. A cached identity descriptor or old confirmation cannot answer a new challenge.
- Key hints by process creation identity, installation/build and wire version; record locations also require the already established runtime and exact attempt/ticket/digest. On process/build/protocol changes discard them. On known or possible reload, discard record locations; old region ranges can only influence priority after fresh enumeration intersects them with currently eligible mappings, never establish continuity. Re-establish runtime before using any runtime-bound record hint.
- Hints may live in memory and optionally `.lycheedev/live/cache/memory-<instance-id>.json`. This disposable directory is outside the operation journal; deleting it has no recovery consequence. Store addresses/ranges and validation metadata, not raw process pages, report payloads or authoritative slot/owner state. Validate schema, count and address bounds before use. Do not import external recipes or probe module offsets.
- Reuse the same eight-worker queue and range coverage tracker. Perform bounded point reads first when eligible, then prioritize recently productive regions. The initial hint advantage is capped at 64 MiB of scheduled region data or 250 ms of elapsed hint work, whichever arrives first, with no fresh budget for each stale entry. These are provisional tuning defaults, not latency promises. Yield between bounded reads; an in-flight OS call can overrun the elapsed target. After that, schedule the remaining full region set fairly, including unread portions of priority regions. No second full-scan implementation or repeated whole-heap restart is needed.
- Add hints only from fully validated records. Evict by bounded recency, remove failed record addresses and reduce unproductive region priority. No timer, background polling or expiration lease is needed for correctness. Cache-off simply bypasses this prioritization and uses the same scanner and validator.
- Report path (`cache_hit`, `priority_hit`, `full_scan`), hint bytes/time, total bytes/time, fallback cause and coverage separately. A successful verified lookup may stop early; its coverage remains partial. In full-audit mode, even a cache hit cannot skip unscanned eligible regions.

Acceptance compares cold cache, warm cache, disabled cache, corrupt hints, reused addresses and reload/build invalidation. Cache benefit is measured across discovery -> bind -> result -> durable release as well as individual lookups. Identical outcomes and safety properties must hold in all modes; speed is the only permitted semantic difference. Never add a cache-only recovery path to the Skill.

## Implementation boundaries

Reuse the existing components and keep one lifecycle reducer. Proposed file/package placement is an implementation guide, not an advertised API:

| Location | Responsibility |
|---|---|
| `internal/live/` and `internal/live/journal/` | One coordinator/reconciler; project intents, attempts, durable results, owner claims and recovery |
| `internal/live/memory/` | Native read-only process adapter, dynamic eight-worker scan, coverage and protocol observation; no dependency on an external memory product |
| `internal/bridge/` | Shared bounded wire schema/validation and cross-language fixtures, no workflow decisions |
| `internal/delivery/` | Complete 65-addon package, managed file roles, slot generation publication and recoverable installation/update |
| `internal/desktop/` | Exact-window input and desktop input arbitration; WGC capture and the single-patch hint detector |
| `addon/Bridge/` | Minimal discovery/bootstrap, validated slot dispatcher and bounded result retention; ActivityView and StartupBeacon are views, not owners of execution |
| `skills/lycheedev/` | Project/target selection, probe class/budget and structured continuation, with installation and recovery references |

The native reader can have internal platform and fake-process adapters for tests. Do not create a public generic memory-debugger command surface, a daemon or a second task framework for this feature. Source/data modules and their SQL/source indexes remain independent.

## Unsolicited user actions and disaster recovery

User-triggered reload, relogin, actor changes, UI interference and game termination are normal fault inputs, not exceptional unsupported behavior. The recovery reducer must not require an agent to repair JSON or remember previous conversation state.

- Treat cached descriptors, unchanged PID, the same character and the same virtual address as insufficient proof of runtime continuity. Establish a new challenged runtime generation after each observed/possible reset. Runtime construction timestamps alone are not a collision-proof generation identifier. Bind all business envelopes to the challenge-established generation and actor. Old slot files must be inert/rejected in a new unbound runtime; do not automatically replay a queued slot on login.
- Check process/window and target identity immediately before each input. An unavoidable change between the last check and delivery is handled by addon-side generation/actor validation. Repeated or delayed activation keys cannot confer commit authority. The commit token must bind the exact operation, slot and code digest, never simply mean "execute whatever is pending".
- A reload indication invalidates readiness immediately; absence of the short color patch never proves continuity. A missed reload must also be detected by fresh runtime challenge/readiness evidence. On target loss, stop further keys and allocation, retain the lease recovery record and reconcile. Never automatically follow a different character, different PID or another window based solely on a matching name.
- During loading/login/disconnection, suspend input and wait within the operation's deadline. Resume only after a new binding establishes the intended target. A new generation resets physical loaded-slot state, not the operation journal or its unknown effects.
- A user closing a panel is not cancellation, ACK or failure. UI visibility never drives business outcome. The activity indicator cannot be required for execution or recovery.
- If a reload follows a durable verified result, return the saved result; release of the vanished old runtime is recorded as `runtime_gone`, not a fabricated ACK. New-runtime handshake can restore readiness for subsequent work independently. If a result had only been seen in memory and not durably committed locally, it remains unconfirmed after disappearance.
- If accepted/running work disappears before a durable result, preserve `execution_unknown`. Do not send automatic replay or an inverse action that assumes whether effects occurred. An explicitly replay-safe read-only probe can be reissued under a new attempt and nonce with linkage to the original; mutating work requires task-specific reconciliation or an explicit user decision.
- Disk slot publication needs same-volume atomic replacement, durable intent and a verified digest. If user reload races file preparation, the expected old generation causes rejection; reconcile the loaded-slot state before retrying. Do not recycle an uncertain slot because no acknowledgement was observed.
- CLI restart recovers project evidence first, then acquires the global target/installation lease. Dead process locks can be reacquired, but takeover never erases a possibly executing operation. Two processes must not both proceed based on a stale timeout lease; use OS locking plus persisted fencing.
- Memory publication and local persistence are not an atomic transaction with game side effects. No protocol can reconstruct lost, unobserved results or guarantee exactly-once arbitrary effects across a hard crash. This limit must be explicit in CLI results and Skill recovery guidance.

Required fault injection matrix: trigger manual reload before slot write, after write, between key events, after prepare, before/after commit, during execution, during memory read, before/after local result flush, and before/after release. Repeat with process exit, actor switch, window loss, CLI kill and duplicate/delayed keys. Assert no cross-target dispatch, no old-generation execution, no automatic replay of unknown mutations, durable results surviving interruption, and deterministic bounded recovery. Test same-character/same-PID reload separately: identity equality does not imply runtime equality.

## Automatic continuation and probe recovery contract

`execution_unknown` is a reconciliation state, not the default terminal answer. The reconciler must attempt bounded automatic recovery and continue the same logical operation when evidence permits. Distinguish a stable logical operation/step ID from a fresh transport attempt nonce. A new runtime uses new transport nonces; it does not create an unrelated business operation or erase earlier attempts.

Provide three explicit probe execution classes:

1. Replay-safe observation: rebuild runtime-local setup/subscriptions and rerun the incomplete observation under the same logical step, recording the new attempt and changed observation time. Never silently claim to have recovered a historical transient event.
2. Resumable workflow: a versioned step manifest with stable step IDs, bounded input/output, declared prerequisites and an optional side-effect-free `check` plus idempotent `apply`. A check returns achieved with evidence, not-achieved with evidence, or indeterminate. Achieved skips apply; proven not-achieved permits apply; indeterminate requires a stronger check or explicit policy. Merely lacking a result is not proof of not-achieved. Recheck volatile completed-step prerequisites after runtime changes.
3. Opaque/non-replay-safe script: resume result retrieval and perform available domain reconciliation automatically, but do not promise restoration of a Lua stack or replay unknown side effects. Report the exact unresolved step only after applicable checks are exhausted. An agent cannot relabel arbitrary code as replay-safe to obtain automatic retries.

All three classes recover transport automatically: reopen the project operation, reselect the exact intended target, establish fresh runtime evidence, reconcile slots/control reserve, and continue observation/commit/release as appropriate. A disappeared runtime makes its old transient slot state irrelevant after confirmed rebinding, but does not resolve old business effects. Unknown-input retries follow the envelope deduplication and commit contract rather than synthesizing a new request.

Checkpoint at semantically meaningful step boundaries, not arbitrary Lua statements. Persist the verified step result locally before authorizing a dependent step. The addon waits for the continuation command; it does not autonomously advance a chain whose checkpoint only exists in volatile memory. Checkpoints contain code/manifest hashes, stable step IDs, inputs, verified evidence, runtime/actor/build, attempt nonce and durable outputs. A checkpoint cannot atomically commit arbitrary game effects; step-specific check/idempotency closes that gap when possible.

After reload, rehydrate setup, validate prerequisites, and resume the first unfinished or invalidated step. A process/character reconnect must not silently change task semantics. Detect manifest/code drift and reject continuation with changed code unless explicitly revised under a new workflow version. Journal corruption remains a recovery error, not an excuse to reset the operation.

CLI returns actionable progress (`reconnecting`, `checking_step`, `restoring_setup`, `resuming`, `waiting_for_target`) with retry timing and a bounded overall deadline. While the invocation is active it drives these actions automatically. If the target remains unavailable past the deadline, persist a waiting state and return the exact operation ID plus next action; a subsequent resume continues it. Do not imply that an exited CLI will wake itself without an explicitly configured scheduler or active agent.

Skill guidance: prefer replay-safe probes for observation and step-based probes for multi-stage work; on interruption keep driving the same operation's structured resume path within the task budget. Do not stop merely on the first pending/unknown observation. Only ask about a concrete irreducible ambiguous effect, changed target/code semantics, or an exhausted external prerequisite. Independent workflow branches may continue only when their declared dependencies do not rely on the unresolved effect.

Acceptance: inject reload at every step/transport boundary and prove automatic completion for replay-safe and idempotently resumable fixtures, including loss of an acknowledgement after an effect. Assert no duplicate non-idempotent apply, no lost durable checkpoint, no skipped invalidated prerequisite, no false recovery of historical observations, and bounded, actionable waiting for unavailable targets. Opaque scripts must demonstrate automatic retrieval/reconciliation and honest escalation only at the unresolved effect, not blanket abandonment.

## CLI and Skill

Retain a compact user workflow: select project/installation -> install/activate if needed -> discover/select -> execute -> status/resume. CLI owns slot reserve, fencing, challenge, automatic safe reload and durable release. Proposed commands are not advertised by the shipped Skill until the command contract implements them. After implementation, revise live-investigation, startup, installation and recovery references together; generate commands.md from the contract. Explicitly teach same-operation recovery, target/project selection and unknown execution boundaries, not protocol mechanics.

Apply skill-creator's progressive disclosure: the entrypoint routes live work to one maintained workflow reference, with installation/recovery detail linked only where needed. The Skill never asks an agent to run wowdump, find offsets, assign slots, edit payload files, infer completion from the logo/patch or abandon a pending operation just to try a new request. It recognizes disk-installed/runtime-old, restart-required, owner-busy, waiting-target, durable-result/pending-release and irreducible-effect states. Installation, permitted activation reload and safe continuation stay within the already authorized task; absent authorization for a different target or unknown destructive effect remains explicit.

## Acceptance gates

1. No-cache end-to-end success on each accepted build; per-process selection and identical actor names in multiple instances never cross-route.
2. Kill CLI before/after every durable write, file replacement, key, acceptance, result save and release. Restart converges or reports explicit unknown; no silent replay.
3. Exercise all 64 slots, reserve exhaustion, invalid envelopes, repeated/late keys, concurrent projects, lost host ledger and externally initiated reload.
4. Corrupt/truncate journals; remove project state with runtime present/absent; validate honest partial recovery.
5. Validate old nonce/run residuals, duplicate conflicting records, cross-chunk and adjacent-region matches, partial reads, bounded missing coverage and read-only process access.
6. Verify color sequence in fresh WGC frames, missed frames and expired hints. New runtime handshake, not color alone, closes reload.
7. Real-client visual proof: no QR, upper-left activity, no input focus during probe execution, correct clearing on every terminal path.
8. Skill contract and installation/update manifests include exactly 64 slots and the new flow; full Go/Lua tests and isolated installation/upgrade pass. Legacy evidence remains readable, not silently migrated into successful new-runtime operations.
9. Two projects race to connect to one process: exactly one logical owner; two same-owner drivers race to resume: exactly one advancing driver. Kill the driver at each admission/fence transition; ownership never becomes free solely because the OS lock was released. Deleted project state and expired timers do not bypass recovery.
10. Two processes use the same installation and physical slot index. Interleave file replacement, delayed keys, reload and driver death; neither process executes the other's envelope, and recovery eventually progresses once external interference ceases.
11. Replay old HELLO/COMMIT/RELEASE files and queued key events into a new unbound runtime. No business execution is possible before the new startup freshness gate and owner fence are established. Prove no circular prerequisite for first install or lost-ledger recovery.
12. Exhaust control capacity at every declared recovery boundary; the admission calculator must prevent stranded accepted work under the bounded fault model. Unbounded interference yields durable actionable waiting, never an infinite reload loop or fabricated completion.
13. Follow the installation-to-release journeys and fault matrix in the test plan: fresh install, online activation, old-runtime upgrade, partial 65-directory publication, external file drift, disabled slots and client restart where required. Never equate disk receipt, reload input, color or a valid stale memory record with activated readiness.
14. Run new offline, isolated npm/install and real-client acceptance on a host with no wowdump. Required fixtures must be self-contained. Verify native scalar/SIMD parity and the eight-worker coverage/performance report without cached-address prerequisites.
15. Verify the detailed UI contract: one temporal color patch, top-left Lychee animation/text, bounded connecting input shield, immediate execution-time input release, idle invisibility, reduced-motion behavior, localization and cleanup after all interruption paths. UI failure cannot alter a business outcome or leave input trapped.

Implementation sequence: deterministic reducer/installation/failure fixtures; narrow labs for startup freshness, shared-slot routing and capacity; managed 64-slot deployment and native reader; execute/resume/reload orchestration; UI and QR retirement; Skill/contracts; isolated installation-to-release and multi-client acceptance. Every implemented slice runs its affected tests; gates precede production transport removal. Do not mark the proposal itself as implementation completion.
