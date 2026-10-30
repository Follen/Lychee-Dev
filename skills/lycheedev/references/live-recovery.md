# Native live recovery

Read for pending input, interruption, contention, reload or unfinished cleanup.
Keep the original project, CON and request. Use installed status/resume/help;
the CLI reconciles protocol state. Persisted status is not a fresh runtime sample.

## Recover without replay

Choose from returned continuation, goal, blocker and remaining budget:
- continue: drive the original CON serially when no unresolved hard error blocks it
- wait_active_driver: wait for that host call; do not start a competing driver
- wait_external: wait for or resolve the named dependency, then resume
- needs_decision: choose within existing task authority; it is not automatically
  another permission prompt
- budget_exhausted: retain results and use permitted cleanup, without renewing
  business time
- completed: inspect this command's outcome and the overall task separately

Continuation does not override errors. A nonretryable read/compatibility failure
needs changed evidence or prerequisites, not a resume loop. Pending, contention
or budget exhaustion alone does not call for reload. Report retained IDs and
concrete blocker when no safe progress remains; blocked cleanup is not complete.

Submitted input proves queue submission, not execution. An uncertain outcome or
intent without an outcome does not permit replay. Missing/partial Mailbox reads
prove neither nonexecution nor absence of a result. Do not replace request keys,
edit journals, clear claims/slots or send raw input to force progress.

A verified report with release pending is useful evidence plus unfinished cleanup.
The CLI owns its receipt wait and bounded cleanup reload; resume the same CON,
not a new probe. Observation recovery can create a bounded new attempt after
verified runtime loss; opaque execution after possible commit stays unknown.
Observe authorized external postconditions if they can resolve that uncertainty.
A new runtime proves old resources ended, not that no persistent effect occurred.

Explicit reload is for a concrete recovery/idle-reload need within task scope,
using the same CON and stable reload key. While connected it requires a fresh
binding before business continues. During pending disconnect it only retires the
old runtime and closes the old CON; it neither binds the replacement nor restarts
business. A successful close/reload does not verify an unavailable report or
prove this CLI caused the reload. Submitted/uncertain reload input is never replayed.

## Ownership and runtime retirement

CLI exit, elapsed time or an in-game panel does not release durable ownership.
Resolve foreign ownership with its owning task. Close using the old CON/project.
The CLI must verify process exit with creation identity, fresh replacement-runtime
input in that same process, or saved runtimeEnd evidence. Access/read failures,
descriptors and cached samples prove none of these. Character selection can stay
pending until replacement proof exists. Retirement does not send the old probe
or transfer it to a new actor. A new CON must fit the original target scope.

A not-yet-bound activation can be cancelled input-free only when saved evidence
proves no intent or not_sent with zero queued messages/progress. Let the CLI
verify this condition; activation_cancelled/closed does not claim successful
activation or business completion. Disconnect never commits prepared business
merely to finish cleanup.

## Input and recovery

Native input relies on fresh Mailbox identity/readiness and effective bindings.
Combat, loading, minimized targets, stale evidence or restricted editors remain
blockers. Keyboard activity pauses new chords; wait for the condition to change
and retain the request/deadline. Submitted input still waits for its own receipt.
For input_binding_unavailable, use read-only `doctor` on the same CON; do not
change bindings, choose a chord or repeat wake as a test. Input diagnostics and
UI activity are not execution receipts. Ordinary editor text/focus need not be
cleared; never simulate chat input. For a returned manual reload wait, follow
[live startup](live-startup.md) in the exact selected window.

Shared publication/slot_pool_full is contention, not proof of a lost connection.
Wait for real availability through owning work. The CLI owns capacity reload;
do not delete reservations or move payloads. One instance's cleanup cannot erase
another's claim. A ready connection does not complete a pending disconnect.

## Durable evidence and older journals

Keep the whole `.lycheedev/live` directory: journals/history, referenced code/
results, artifacts and metadata. Missing/corrupt evidence is a recovery error,
not permission to recreate an empty ledger. The CLI may repair an incomplete
final append only under the owning lease with the prior chain verified.

Durable budgets are 600 seconds for business/explicit reload, 120 for initial
bind/first close, and 600 for activation carried into bind. Manual first-reload
waiting starts no bind budget; other time between calls counts. Resuming, closing
again or changing keys does not extend a goal. Host cancellation does not confirm
Lua cancellation. Budget exhaustion can still permit input-free retirement.

Older native wire/identity journals cannot be driven by the current protocol;
preserve them for matching-version diagnosis rather than rewriting them. A
compatible older artifact serialization is not old-wire compatibility. Cleanup
callback failure can retain a verified report and require runtime destruction;
keep the original resourcesReleased=false and actual cleanupMethod, rather than
claim normal release succeeded.
