# Native live recovery

Read this for pending input, interrupted work, contention, reload or unfinished
cleanup. Preserve the original project, CON and request. The CLI owns protocol
reconciliation; choose the next action from its continuation, not its internal stage.

## Recover without replay

```text
lycheedev live status <CON-id> --project <project-directory> --format json
lycheedev live resume <CON-id> --project <project-directory> --wait-seconds 120 --format json
```

Status reads persisted evidence. Resume acquires the same connection's driver
lease, reconciles original nonces and continues within both the caller's wait
limit and the original durable deadline. Exit 6 retains usable results and
unfinished cleanup. Follow continuation: continue the same request serially,
wait for its existing call, resolve the reported external condition, or report
the exhausted budget/required decision. Do not infer a recovery action from an
internal stage alone. Keep each instance's project, CON and request separate.
Do not replace a request, edit its files, clear ownership or repeat raw input to
force progress. A cached hit never bypasses identity/checksum/fresh-confirmation
validation. Incomplete scan coverage is not proof that no result exists.

After user reload, a descriptor alone is only a candidate. Continuing business
requires a fresh bind; closing the old CON instead uses [verified retirement](#ownership-and-runtime-retirement).
Resume may attempt input-free cleanup when closing, after budget exhaustion or
from saved `runtimeEnd`, without renewing business time.
Repeatable observations can resume in a new attempt, bounded to three attempts.
For opaque execution after a possible commit, `execution_unknown` remains unknown;
inspect external postconditions using an authorized independent observation
instead of repeating the effect. A fresh runtime may prove old runtime resources
are gone without proving that the old script never changed persistent state.

A partial final journal write can be repaired under the owning driver lease only
when the entire preceding chain validates; the original bytes are retained.
Corrupt complete events, missing journals, foreign ownership, incompatible files
or an unavailable client require diagnosis. Never recreate an empty ledger to
turn uncertainty into a new operation. Report the exact blocker and retained IDs
if no safe progress remains; do not label that handoff complete.

For an intentional idle reload, use a stable request key:

```text
lycheedev live reload --project <project-directory> --session <CON-id> --request <reload-key> --wait-seconds 120 --format json
```

The same explicit reload can recover a bound connection whose pending exchange
cannot obtain a receipt (for example, `confirm_ready` after submitted input).
It preserves the old exchange and candidate report, uses normal input readiness,
and, while the connection remains open, requires a fresh runtime binding to
complete reload and continue business. Observation work may then become eligible
for a later resume under its existing policy.

If disconnect is already pending, explicit reload has a narrower purpose: verify
that the old runtime has ended, sending the authorized reload only if needed,
then close the old CON without binding the replacement or restarting unfinished work.
It uses its own 600-second control budget even when the original close budget is
exhausted; neither that budget nor the business budget is renewed. Keep the same
CON/project and reload request key for continuation. A submitted or uncertain
reload input is never replayed, and an active reload/recovery cannot be replaced
by a new request. Completion here means the CON is closed, not connected to the
replacement runtime; it does not prove this CLI sent or caused a reload.
Saved `runtimeEnd` evidence needs only input-free retirement.
The original input record remains the authority for what was sent.

Use this explicit recovery only when needed within the authorized task, after
inspecting the concrete blocker; pending, contention or budget exhaustion alone
does not call for reload. Without replacement proof, retain pending cleanup and
wait for the reported condition to change. The separate input-free retirement
path may close the old CON without any reload input. An unconfirmed opaque report
remains unavailable/execution_unknown; successful reload or close does not verify
it. New work on a changed actor requires its own authorized target and new CON.

`journal.resource_busy: active window driver` means another host CLI still holds
the execution lease. A game UI frame does not own that lease. Wait for the active
call to finish before recovery; pressing Escape cannot release a host lease.

Disconnect does not commit prepared business merely to finish it. If the exact
prepared exchange needs explicit reload, preserve its blocker and use the
existing reload command only within the authorized scope. Closing is not proof
that an already submitted opaque operation was cancelled or never ran.

## Ownership and runtime retirement

A PID has one durable owner across projects and one active host driver. CLI exit
or elapsed time does not release the claim. Resolve foreign ownership with the
owning task; a crash releases the OS driver lock, not durable reservations.

Use the old CON's `live disconnect` in its owning project. The CLI can verify
PID plus creation-time process exit, or verify another runtime actually wrote a
new input sample in the same fixed process. Access errors, missing windows,
descriptors, cached addresses and old heap samples prove neither case. Runtime
replacement proof does not compare game time with the host clock. The CLI may
relocate distant sample addresses once within its existing observation budget;
this is not an Agent-directed scan or permission to extend the deadline.

For runtime replacement, the CLI saves `runtimeEnd` before retiring only the
old CON's exact reservations.
Saved proof can finish interrupted retirement even after character selection.
Without a new sample or saved proof, character selection stays pending. This path
sends no input, binds no replacement, and does not change the old actor or replay
its probe. Retain unresolved `execution_unknown` results even if `closed=true`.
A new investigation on another actor needs a new CON for the authorized target;
never automatically transfer the old request or probe.

## Input and recovery

The CLI journals input and selects the observation adapter from the verified
runtime capability. Hybrid optical readiness keeps identity, routing and reports
memory-backed; older input-v1 runtimes use memory observations. Missing signals
do not permit blind fallback. Agents do not decode colors or manage capture.

With an ordinary focused editor, the coordinator may send one Esc and observe
again; Esc can close/cancel that UI. Combat, unknown observations, missing signals
and stale evidence do not permit Esc. Held system keys wait for release. A fresh
capture frame or changing heartbeat alone proves neither runtime identity nor
freshness of the Lua state. Do not infer readiness from a screenshot or extend
input time limits to bypass a pending result. No manual foreground-input or
color-decoding workflow is required; native invocation keys and slots belong to
the CLI.

For `receiver_binding_conflict`, `receiver_binding_ineffective`, or no effective
machine shortcut, resolve the selected target's binding conflict before further
wake attempts. Do not change user bindings or protocol keys, or repeat wake to
test the conflict. A ready input observation does not prove the shortcut reaches
the receiver. If input was already submitted or uncertain, retain the original
CON and slot reservation and follow official CLI recovery; fixing the binding
does not authorize replay of unknown input. Never delete the slot to unblock
another instance; use [verified retirement](#ownership-and-runtime-retirement)
when the original process or runtime has ended.

Input evidence is distinct from execution: `not_sent` proves zero messages were
queued; `submitted` proves only queue submission; `uncertain` (or an intent with
no outcome) does not permit replay. These facts drive CLI recovery. Do not
reinterpret them into manual retry instructions. Use the same CON/project and
`live resume`; inspect `waiting` when present. Recheck after a changed condition
within the authorized task; do not repeat indefinitely against an absent process,
foreign ownership or an unresolved opaque execution outcome.

`waiting: shared_publication` is shared-file contention inside one installation,
not proof of a lost connection. Inspect the concrete blocker in continuation:
a short publication lease can clear when its active call ends, while a durable
slot reservation needs its original owner's recovery. Keep the same CON/request
and nonce; do not delete a reservation or repeatedly resume without a change.
A combat or focus wait retains its reservation, not the installation's short
publication lock. Shared-installation instances still share physical slot files; the current
protocol does not promise progress past an unresolved reservation in their next slot.

`complete` applies to the current command: a connected runtime does not complete
a pending disconnect, and a previous report does not complete reload. UI activity
and reload hints are not execution or result proof.

The whole `.lycheedev/live` directory is the recovery unit. Keep connection logs,
linked history segments, content-addressed `connections/artifacts`, target metadata
and result files together. New journals reference exact code/result bytes by digest;
older inline snapshots remain readable. Missing or corrupt referenced content is
a recovery error, never a reason to generate another request key.

## Durable evidence and older journals

Durable deadlines are 600 seconds per business request or explicit reload,
120 seconds for initial bind or first close, and 600 seconds for activation
carried into its bind. Time between calls counts. Resume, repeated close and
changing request keys do not extend a goal. Close retains its own allowance
after business exhaustion; existing evidence remains readable. Detected clock
rollback stops automatic work; an unseen offline clock change cannot be detected.

Older channel-v1 journals receive one marked legacy budget window on their first
drive, persisted before input; reading status does not migrate them. That window
starts at migration because prior elapsed time is unavailable. A CLI wait timeout
or interruption ends the host call, not the game-side probe or its cleanup.
New writes use channel-v2 (activation-v2 for activation); older CLIs cannot drive
those journals. A cancelled host wait uses exit 7; it does not confirm Lua cancellation.

Cleanup callback failures retain the verified report and trigger a journaled
reload; a fresh binding then records `cleanupMethod: runtime_destroyed`. The
original `resourcesReleased: false` is preserved. It is distinct from a normal
release acknowledgement. Journal segments rotate only at idle boundaries and
remain linked under the connection's `.jsonl.history` directory; preserve these
alongside the active log. A missing segment must not be worked around by issuing
a new request key.
