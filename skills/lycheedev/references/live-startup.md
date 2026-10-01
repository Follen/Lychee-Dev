# Native live startup and installation

Use this reference for first installation, an upgraded addon, or contact without
a usable CON connection. Disk installation and runtime activation are separate.
Native 3.1.0 uses the public Mailbox with wire06, slot-v3, identity-v2 and
input-v3 with hybrid-v2 readiness and the effective F12/F11 binding profile.
The CLI owns the main addon and 200 LoadOnDemand slots; never overlay-copy files,
edit slot payloads or import old queues/SavedVariables.

Use the shared installation candidates to select a verified client directory;
directory names alone do not establish product identity. A game root may contain
several clients, and a client may have several processes: retain the selected path,
PID, process creation identity and later character binding. Resolve only remaining ambiguity, not a fresh
choice on every retry. Local selection errors never authorize another installation.

1. Select the authorized installation and PID. Inspect `addon status`; the main
   addon must be a clean managed installation matching the CLI, and all 200 slot
   files must pass inspection. Use [installation](installation.md) for deployment.
2. Use `live connect` with the invoking project's `--project` and exact target.
   It resolves the verified Mailbox identity and binds with a new nonce. If the runtime is
   absent under a supported activation condition, it records an activation and
   performs one fixed reload automatically. An explicitly unsupported loaded
   input capability is a blocker, not permission to send fallback keys.
3. Preserve any returned CON ID, including activation-pending results. Use status
   and resume in the same project; do not start another connection or resend keys.

A clean inactive older managed slot installation can be replaced before new
work. Incompatible pending reservations or older connection identities are
rejected, with their evidence preserved; the new driver does not resume old
wire exchanges or use old records as current-runtime proof. Journaled activation
can reload a compatible loaded runtime after a clean managed installation.
For an unsupported loaded protocol, finish the clean installation first and
ask the owner to reload that selected client manually; then verify the new
runtime without replaying pending input. Verify activation separately for every authorized PID,
including same-build instances sharing one installation: 200 files on disk do
not prove any running client loaded them. If the client cannot discover the full
inventory, `restart_required` requires a scoped client restart; do not loop reloads.

After an intentional installation/update with no usable connection, explicit
activation is also available:

```text
lycheedev live reload fallback --project <project-directory> --installation <client> --pid <pid> --request <activation-key> --wait-seconds 120 --format json
```

The CLI selects the verified runtime's input observation path. Capable runtimes
use normal readiness checks; missing signals or stale evidence stay pending.
Only supported first-install/activation conditions permit the fixed activation
fallback. Unsupported input capabilities never use a blind key fallback.
Agents do not send that key sequence manually. Input intent and outcome
are journaled; repeating the request resumes activation without replaying uncertain
input. A startup beacon is only a temporary hint, never identity or bind proof.

If the game is at character selection, loading, in combat, explicitly disabled,
or otherwise cannot accept the narrow input, preserve pending evidence and
inspect that condition. An absent descriptor does not prove a build mismatch or
that a reload executed. Client versions that cannot discover newly installed
addon folders through reload may require a client restart; do not loop reloads
or claim that disk files prove those folders are loaded. Keep pending activation
bound to its selected process; a restart ends that process rather than transferring
its unknown input. Retire the old CON using process/runtime proof, then bind the
replacement under the user's scope. An already authorized current-character task
does not require another character choice; a specifically named actor stays fixed.

## Ownership and recovery

Retain the original CON and project on failed activation or interrupted cleanup.
A foreign owner's claim is not an expired timeout. Use [ownership and runtime
retirement](live-recovery.md#ownership-and-runtime-retirement) for process exit,
character changes or a replaced runtime; never delete claims or slot files.

Do not use legacy bind/reset/QR commands for a native CON connection. The fixed
profiles are part of the protocol: the addon selects F12 or F11 and the CLI
verifies the effective published profile before sending keys. Do not choose
arbitrary chords or infer ownership from a similar addon name. Slot allocation, publication, consumption,
capacity reload and shared-installation coordination belong to the CLI.

When activation completes, continue the authorized investigation immediately via
[live-investigation](live-investigation.md). Connection-only tasks do not need a
fabricated probe; close their quiescent CON with `live disconnect` when done.
