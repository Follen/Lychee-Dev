# Native live startup and installation

Use this reference for first installation, an upgraded addon, or contact without
a usable CON connection. Disk installation and runtime activation are separate.
The CLI owns the main addon and 64 LoadOnDemand slots; never overlay-copy files,
edit slot payloads or import old queues/SavedVariables.

Use the shared installation candidates to select a verified client directory;
directory names alone do not establish product identity. A game root may contain
several clients, and a client may have several processes: retain the selected path,
PID and later character binding. Resolve only remaining ambiguity, not a fresh
choice on every retry. Local selection errors never authorize another installation.

1. Select the authorized installation and PID. Inspect `addon status`; the main
   addon must be a clean managed installation matching the CLI, and all 64 slot
   files must pass inspection. Use [installation](installation.md) for deployment.
2. Use `live connect` with the invoking project's `--project` and exact target.
   It discovers memory identity and binds with a new nonce. If the runtime is
   absent, it records an activation and performs one fixed reload automatically.
3. Preserve any returned CON ID, including activation-pending results. Use status
   and resume in the same project; do not start another connection or resend keys.

After an intentional installation/update with no usable connection, explicit
activation is also available:

```text
lycheedev live reload fallback --project <project-directory> --installation <client> --pid <pid> --request <activation-key> --wait-seconds 120 --format json
```

When the verified runtime advertises supported input observations
(`lycheedev.input.hybrid.v1` or compatible `lycheedev.input.v1`), even this fallback
command uses the ordinary coordinator: observe, send one Esc only for an editor
blocker, observe again, then send Enter, /reload, Enter once ready. An already
unblocked runtime needs no Esc. Missing color blocks, unavailable capture or
stale evidence on a capable runtime stays pending; it never silently downgrades
to blind input or changes the selected process. The CLI selects and checks the
observation path; agents do not inspect colors or manage its capture stream.

Only the existing first-install/older-runtime activation conditions permit the fixed
Esc x3, Enter, /reload, Enter fallback. Its intent is saved before
the burst and its structured input outcome afterward. A lost outcome remains
uncertain. Repeating the request resumes the same activation; only proven zero
input permits another attempt. Where the legacy startup beacon is used, a single
RGB square changes
phase near the top-left for up to 45 seconds and stops on wake. Seeing it is a
hint, not identity proof: a newly observed runtime and fresh bind must follow.

If the game is at character selection, loading, in combat, explicitly disabled,
or otherwise cannot accept the narrow input, preserve pending evidence and
inspect that condition. An absent descriptor does not prove a build mismatch or
that a reload executed. Client versions that cannot discover newly installed
addon folders through reload may require a client restart; do not loop reloads
or claim that disk files prove those folders are loaded. Keep the selected actor
and installation fixed. A restart or character switch follows the user's scope.

## Ownership and recovery

A process has one logical owner across projects. CLI exit or elapsed time does
not release it. A foreign claim is a conflict to resolve with the owning task,
not an invitation to remove its ledger. The short driver lock is released by the
OS after a crash; the durable CON claim and slot reservation remain for recovery.

If the selected game process exited, use `live disconnect <CON>` from its owning
project before upgrading that installation. The CLI checks PID plus creation
time, journals the OS proof, and retires only that connection's reservations and
claim without sending input. Access errors or missing windows do not prove exit.
An unconfirmed operation remains `execution_unknown`, `reportState=unavailable`,
`complete=false` even when `closed=true`; a candidate result is not promoted.
Keep the old project journal and connect the new process separately. Never
delete owner files to make an installation upgrade succeed.

Process exit is not the only cleanup proof. If reload or a character switch
replaced Lua inside the same process, use the old CON's `live disconnect` in its
owning project. The CLI checks the fixed PID, creation time and ownership claim,
then verifies that another runtime in that process actually generated a new
input sample. Descriptors, cached addresses and old heap samples are insufficient;
this proof does not compare game time with the host clock. The CLI persists
`runtimeEnd` before retiring only the old connection's exact pending
reservations. Interrupted retirement can continue from that saved evidence,
even if the game subsequently returns to character selection. This
path sends no input, binds no new runtime, and does not change the old actor or
replay its probe. An unresolved old result remains `execution_unknown`, even
when the old CON closes. Without a newly generated sample, including character
selection with no new producer, cleanup stays pending. Preserve the CON and
claims; do not delete ownership files, replay the probe or resend uncertain input.

If normal disconnect cannot finish, inspect its continuation first. A pending
result or exhausted close budget alone is not a reason to reload. When explicit
reload is needed and within the authorized task, a still-bound closing CON can
use the [connected reload recovery](live-recovery.md#recover-without-replay)
path: its separate control budget permits reload of the old runtime if needed,
then verified retirement of the old CON. An already replaced runtime can retire
without new input. It does not bind the replacement or
restart business. An existing reload/recovery must be continued under its
original request; do not replace it to obtain another budget.

For a changed actor, a new investigation uses a new CON only for the target
selected within the user's authorization. Do not transfer the old request or
automatically run its probe on the new character. Keep the old journal and its
unknown result separate from any newly authorized work.

Do not use legacy bind/reset/QR commands for a native CON connection. The fixed
keys are part of the protocol; changing them in the UI or through an external
macro would desynchronize the CLI. Slot allocation, publication, consumption,
capacity reload and shared-installation coordination belong to the CLI.

When activation completes, continue the authorized investigation immediately via
[live-investigation](live-investigation.md). Connection-only tasks do not need a
fabricated probe; close their quiescent CON with `live disconnect` when done.
