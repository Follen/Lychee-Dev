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

The CLI selects the verified runtime's input observation path. Capable runtimes
use normal readiness checks; missing signals or stale evidence stay pending.
Only supported first-install/older-runtime conditions permit the fixed activation
fallback. Agents do not send that key sequence manually. Input intent and outcome
are journaled; repeating the request resumes activation without replaying uncertain
input. A startup beacon is only a temporary hint, never identity or bind proof.

If the game is at character selection, loading, in combat, explicitly disabled,
or otherwise cannot accept the narrow input, preserve pending evidence and
inspect that condition. An absent descriptor does not prove a build mismatch or
that a reload executed. Client versions that cannot discover newly installed
addon folders through reload may require a client restart; do not loop reloads
or claim that disk files prove those folders are loaded. Keep the selected actor
and installation fixed. A restart or character switch follows the user's scope.

## Ownership and recovery

Retain the original CON and project on failed activation or interrupted cleanup.
A foreign owner's claim is not an expired timeout. Use [ownership and runtime
retirement](live-recovery.md#ownership-and-runtime-retirement) for process exit,
character changes or a replaced runtime; never delete claims or slot files.

Do not use legacy bind/reset/QR commands for a native CON connection. The fixed
keys are part of the protocol; changing them in the UI or through an external
macro would desynchronize the CLI. Slot allocation, publication, consumption,
capacity reload and shared-installation coordination belong to the CLI.

When activation completes, continue the authorized investigation immediately via
[live-investigation](live-investigation.md). Connection-only tasks do not need a
fabricated probe; close their quiescent CON with `live disconnect` when done.
