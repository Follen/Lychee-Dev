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

For a verified report whose attempted release has no exact receipt, the CLI
observes the original receipt for five seconds, then journals one automatic
cleanup reload within the original operation deadline. Resume the same CON;
do not issue a new probe or resend release. Reload still requires fresh INPUT,
and cleanup completes only after verified runtime retirement. The report and
original attempt remain unchanged.

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

This closing recovery also covers an initial bind whose input was attempted but
never acknowledged: disconnect first preserves closing intent, then the same
explicit reload can retire it. It does not bind the replacement or resend the
old bind. An ordinary unbound connection does not gain reload permission.

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

An activation may be prepared or waiting for manual reload before a connection journal
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

The CLI journals input under Lycheedev mailbox protocol v1. Current Mailbox
records carry input readiness, identity, routing and results. Native input does
not use colors, capture, QR or a startup beacon. Missing or stale input records
never authorize keys or a blind reload fallback.

Ordinary EditBoxes forward only the two formal Lychee Dev actions while the
bridge is enabled. Focus and draft text stay intact. Current INPUT publishes
`escapeLimit=0`; no Esc is needed. Password, secure text, protected or forbidden
editors return `input_focus_unavailable`; retain the CON and wait for the user to
leave that field. Combat, loading and stale evidence never authorize keys.
Old upgrade journals retain their original bounded Esc counts; do not reset them.
Held Ctrl, Alt, Shift, either Win key or a key in the selected chord delays input
until release. The CLI never forces the user's physical keys up. A user can
still interrupt a chord after that check; only the addon receipt confirms execution.

The enabled INPUT sampler remains active when an AFK interface hides UIParent.
Loading, world state and focus checks still apply. Fresh telemetry does not
prove that a third-party keyboard frame forwards the binding.

In-game edit focus differs from Windows foreground focus. Input targets the
verified HWND through PostMessage without activating or stealing the foreground.
The CLI verifies process/window identity and rejects minimized targets. Agents
do not send keys, clear focus, decode colors or locate memory addresses manually.

For `input_binding_unavailable`, run `live doctor` on the same CON and resolve
the selected target’s effective binding before further input. Do not change user bindings or protocol keys, or repeat wake to
test the conflict. A ready input observation does not prove the shortcut reaches
the receiver. If input was already submitted or uncertain, retain the original
CON and slot reservation and follow official CLI recovery; fixing the binding
does not authorize replay of unknown input. Never delete the slot to unblock
another instance; use [verified retirement](#ownership-and-runtime-retirement)
when the original process or runtime has ended.

The in-game keybinding settings contain execute and reload under Lychee Dev.
Defaults are Ctrl+Alt+F12 and Ctrl+Alt+F11, with F10/F9 as conflict fallbacks.
Existing assignments are preserved. `/dev receiver reset` fills missing actions
using free defaults; it does not overwrite player keys or repair a pending CON.
The CLI reads actual effective bindings from Mailbox; never substitute a chord.
Reload uses its own game binding and calls the UI reload API directly. Legacy
input-v3/hybrid-v2 grants no new keys; install the current addon, reload manually
and resume the retained activation.
The previous name of the two-key protocol is different: its fresh INPUT with
zero Esc may authorize only the upgrade reload. Let the CLI complete that step;
the new runtime publishes the full current name before business resumes.

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
not proof of a lost connection. Native 3.2 development requires the Mailbox wire,
identity and 500-slot pool; it rejects older native connection identities.
Retain incompatible evidence for its matching older CLI rather than applying
the new driver to it. The current 500-slot allocator can skip another
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
is not execution or result proof.

The whole `.lycheedev/live` directory is the recovery unit. Keep connection logs,
linked history segments, content-addressed `connections/artifacts`, target metadata
and result files together. New journals reference exact code/result bytes by digest;
compatible inline snapshots remain readable. Missing or corrupt referenced content is
a recovery error, never a reason to generate another request key.

## Durable evidence and older journals

Durable deadlines are 600 seconds per business request or explicit reload,
120 seconds for initial bind or first close, and 600 seconds for activation
carried into its bind. Waiting for the first manual reload starts no connection
budget until the new runtime is observed. Other time between calls counts. Resume, repeated close and
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
