# Live doctor before driving the game

Run doctor before **each** invocation that may drive the game: `live connect`,
`live execute`, `live resume`, `live reload`, `live reload fallback`, `live cancel`
and `live disconnect`. This applies to continuation and cleanup calls as well
as new business. Run it again for the next call, even if the last doctor passed.
Read-only `live status`, retained results and history can be inspected without
this drive preflight. This rule does not add live actions to source/data tasks.

## Current supported check

Use the installed CLI contract. In the selected project's working directory:

```text
lycheedev doctor --format json
```

Retain the same explicit workspace home, if one was supplied. `--offline` is
available when the task's network policy requires it; it leaves source-mirror
remote currency unknown. Preserve the doctor result or its concrete check IDs
with the next live action's evidence, so a finding can be explained later.

As implemented in 3.1.1, doctor checks workspace health, target catalog, bundled
LuaLS, and the source mirror when the nearest project declares a product. It
does **not** accept target PID, installation, connection or scope arguments. It
does not inspect an in-game inbox/sendbox, prove a fresh heartbeat, establish
runtime readiness, bind a character, diagnose business busy, or certify a clean
managed addon. Do not invent those fields or interpret `healthy: true` as live
readiness. A LuaLS warning concerns source semantic analysis; it is not proof
that an existing live connection cannot be closed.

Doctor is read-only. Its suggestions do not authorize installation, update,
source sync, reload, claim deletion or a different target. Follow the original
task's authorization and the actual live command's checks. A general doctor
failure is evidence to assess, not a substitute for the CLI's live continuation.

## Keep identity and continuation

Keep the selected project, CON, request, installation, PID plus creation identity,
runtime and actor fixed across doctor and the following live call. Run doctor
in that project's context; it has no flag that carries the CON binding. A PID
or runtime change requires the existing [recovery](live-recovery.md) path and,
when appropriate, a fresh connection within the authorized scope. Do not infer
that two windows share identity because they have the same build or files.

Inspect the original connection's read-only status when work is pending. An
active driver or outstanding business request blocks **new business**; it is
not permission to replace the CON/request, clear ownership or replay unknown
input. Retain verified failed reports as outcomes and keep unfinished cleanup
visible. No missing sample, screenshot, descriptor or doctor result proves that
an uncertain request was never executed.

Always run the diagnostic check, including before an authorized cancel,
disconnect or input-free recovery. General workspace/source/LuaLS findings do
not create an extra prerequisite that traps cleanup behind new-business
readiness. Continue the exact authorized recovery when the installed CLI can
verify its required identity and ownership; obey its returned continuation and
remaining budget. A foreign active driver still requires serial coordination,
and command failure still leaves the original evidence intact. Do not bypass a
real recovery blocker or claim resources were cleared without verified proof.

If current doctor cannot answer a live readiness question, say which evidence
is absent and let the supported live command perform its own checks. Do not
manually decode colors, read/write addresses, manufacture heartbeats or restart
unknown work to fill that gap. Use [startup](live-startup.md) for authorized
activation and [recovery](live-recovery.md) for retained work.
