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
unfinished cleanup. Choose from the returned `continuation`, including its
goal, blocker condition and remaining budget:

| Kind | Next action |
| --- | --- |
| `continue` | Continue the original CON serially within its budget; reconcile the receipt or focus condition |
| `wait_active_driver` | Wait for the existing host call; do not start a competing resume |
| `wait_external` | Resolve or wait for the named dependency to change, then resume |
| `needs_decision` | Inspect the concrete outcome/blocker and choose an action covered by the task's scope |
| `budget_exhausted` | Retain results and consider permitted cleanup/recovery; do not renew business time |
| `completed` | Read this command's terminal outcome; check whether the overall investigation is done |

`needs_decision` is not an automatic permission prompt. Necessary recovery can
already be authorized; an unresolved opaque effect may instead require an
independent observation or a truthful handoff. Do not infer recovery from an
internal stage alone. Keep each instance's project, CON and request separate.
Do not replace a request, edit its files, clear ownership or repeat raw input to
force progress. A cached hit never bypasses identity/checksum/fresh-confirmation
validation. A missing or partial Mailbox observation is not proof that input
was never executed or that no result exists.

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
it. New work on a changed actor requires a fresh CON within the task's target
scope. An existing arbitrary-character scope can cover it after safe retirement;
the old operation is never transferred.

`journal.resource_busy: active window driver` means another host CLI still holds
the execution lease. A game UI frame does not own that lease. Wait for the active
call to finish before recovery; pressing Escape cannot release a host lease.

Disconnect does not commit prepared business merely to finish it. If the exact
prepared exchange needs explicit reload, preserve its blocker and use the
existing reload command only within the authorized scope. Closing is not proof
that an already submitted opaque operation was cancelled or never ran.

An activation may still be prepared before an ordinary connection journal
exists. `live disconnect` can cancel it without game input only when its saved
state proves no input intent or a `not_sent` outcome with zero queued messages
and no input progress. It preserves the activation record as
`activation_cancelled` and releases only its exact window owner. Status, resume
and repeated disconnect retain this terminal state; `closed=true` does not
claim a successful activation or business result (`complete=false`). Submitted,
uncertain or runtime-selected activations do not use this cancellation path.

## Ownership and runtime retirement

A PID has one durable owner across projects and one active host driver. CLI exit
or elapsed time does not release the claim. Resolve foreign ownership with the
owning task; a crash releases the OS driver lock, not durable reservations.

Use the old CON's `live disconnect` in its owning project. The CLI can verify
PID plus creation-time process exit, or verify another runtime actually wrote a
new input sample in the same fixed process. Access errors, missing windows,
descriptors or cached samples prove neither case. Replacement requires fresh,
increasing input samples from the new runtime in that same process. The CLI
revalidates the build-bound root and named Mailbox path within its budget;
it does not relocate samples by scanning the heap. Game time is not compared
with the host clock to establish replacement.

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
current runtime capability. Hybrid optical readiness keeps identity, routing
and reports Mailbox-backed. Unsupported native wire/input versions are rejected;
they are not a memory-only fallback. Missing signals do not permit blind input.
Agents do not decode colors, manage capture or locate memory addresses.

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

After the owner resolves the actual key conflict, the native addon supports
manual `/dev receiver reset` to reselect its fixed primary/backup profiles. It refuses combat,
active receiver input and remaining conflicts; it does not change account keys
or saved custom profiles. This command repairs bindings only, not the CON or
pending exchange. If the CLI cannot send input, ask the owner for this specific
repair instead of sending raw keys. A reload retires the old runtime; close its
original CON through verified retirement before a new investigation.

Native INPUT v3 publishes the actually effective F12 or F11 profile, paired with
hybrid v2. Primary conflicts produce a warning and try the backup without
changing the player's bindings. The CLI verifies this profile before sending;
agents must not choose a chord from the warning, mix profiles or substitute a
key themselves. If both profiles conflict, resolve that specific conflict with
the owner; ready colors alone do not authorize input. A fixed-F12 older runtime
is not compatible with this profile contract.

Input evidence is distinct from execution: `not_sent` proves zero messages were
queued; `submitted` proves only queue submission; `uncertain` (or an intent with
no outcome) does not permit replay. These facts drive CLI recovery. Do not
reinterpret them into manual retry instructions. Use the same CON/project and
`live resume`; inspect `waiting` when present. Recheck after a changed condition
within the authorized task; do not repeat indefinitely against an absent process,
foreign ownership or an unresolved opaque execution outcome.

Optional input-attempt diagnostics are not execution receipts. Missing counters
do not prove that a key was never delivered; unchanged counters do not authorize
replay. Diagnose unsupported roots/layouts or read gaps from retained evidence;
do not fix them by copying an RVA, scanning memory or looping reloads.

`waiting: shared_publication` is shared-file contention inside one installation,
not proof of a lost connection. Native 3.1.0 requires the new Mailbox wire,
identity and 200-slot pool; it rejects older native connection identities.
Retain incompatible evidence for its matching older CLI rather than applying
the new driver to it. The current 200-slot allocator can skip another
owner's reservations for a new exchange only before publication or input. The
runtime skips empty/foreign slots and stops at the first envelope for itself;
it does not skip its own pending work. Once published, submitted or uncertain,
the original slot and nonce remain fixed. Agents neither choose slots nor move
payloads; they continue the original CON/project/request serially.

A short publication lease can clear when its active call ends. If all forward
slots are occupied, continuation reports `blocker.kind: slot_pool_full` with
`condition: forward_slot_available`. Wait for actual availability to change,
usually through the owning connection's recovery; do not busy-loop resume,
delete reservations or change request keys. Capacity reload belongs to the CLI
and does not erase another instance's claim or prove unknown input never ran.
A combat or focus wait retains its reservation, not the short publication lock.

`complete` applies to the current command: a connected runtime does not complete
a pending disconnect, and a previous report does not complete reload. UI activity
and reload hints are not execution or result proof.

The whole `.lycheedev/live` directory is the recovery unit. Keep connection logs,
linked history segments, content-addressed `connections/artifacts`, target metadata
and result files together. New journals reference exact code/result bytes by digest;
compatible inline snapshots remain readable. Missing or corrupt referenced content is
a recovery error, never a reason to generate another request key.

## Durable evidence and older journals

Durable deadlines are 600 seconds per business request or explicit reload,
120 seconds for initial bind or first close, and 600 seconds for activation
carried into its bind. Time between calls counts. Resume, repeated close and
changing request keys do not extend a goal. Close retains its own allowance
after business exhaustion; existing evidence remains readable. Detected clock
rollback stops automatic work; an unseen offline clock change cannot be detected.

Current native writes use channel-v2 and activation-v3 with the current Mailbox
identity. Older native wire/identity journals are rejected before driving or
budget migration; preserve them for diagnosis with their matching release.
An older artifact serialization within a compatible connection is a different
case and does not establish old-wire compatibility. A CLI wait timeout or
interruption ends the host call, not the game-side probe or its cleanup.
A cancelled host wait uses exit 7; it does not confirm Lua cancellation.

Cleanup callback failures retain the verified report and trigger a journaled
reload; a fresh binding then records `cleanupMethod: runtime_destroyed`. The
original `resourcesReleased: false` is preserved. It is distinct from a normal
release acknowledgement. Journal segments rotate only at idle boundaries and
remain linked under the connection's `.jsonl.history` directory; preserve these
alongside the active log. A missing segment must not be worked around by issuing
a new request key.
