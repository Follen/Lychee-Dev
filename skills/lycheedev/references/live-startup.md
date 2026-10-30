# Doctor and Live startup

Read for first contact, changed addon files or readiness failure. Disk installation
and loaded runtime are separate. Native Live uses Lycheedev mailbox protocol v1,
a named Mailbox, 500 managed slots and effective execute/reload bindings.

## Diagnose the selected target

Use `lycheedev doctor` for workspace, Data, Source and selected Live checks.
Add `--installation`/`--pid`, or `--session CON_ID --project ORIGINAL_PROJECT`
to diagnose the exact target. A version label alone does not establish commit,
protocol or slot inventory.
Keep old CON evidence with matching-version diagnosis; do not replace journals.

Retain installation/PID or original project/CON. Passive discovery is for missing
target identity, not a reason to retarget. Doctor is read-only and creates no
connection/input. Read each check's state, detail and nextStep; bootstrap evidence
explains startup, while fresh INPUT governs machine input.

## Complete the authorized repair

Necessary deployment/activation/recovery can already be covered by the live task;
use `doctor --fix` within that scope. Modified/unmanaged files need
[installation diagnosis](installation.md), not overlay copies. Continue existing
connections and returned activation IDs, never replace pending business.

Use `doctor --fix --hard-reset` when the fixed slot pool needs an explicit
rebuild. It archives the original slot manifest and all 500 fixed slot paths,
including unknown payloads and reservations, before initializing the pool.
Keep the returned repair archive and original CON for interrupted recovery;
retry the same repair rather than choosing a new archive. A slot reset does
not settle or replay the original connection's pending input or report cleanup.

- Clean ready runtime: connect or resume as appropriate
- slotReloadRequired: let connect/resume perform its guarded reload; the flag
  does not authorize replay of unknown input
- Compatible fresh reload binding under an older protocol name: let the CLI
  perform the verified upgrade reload, then bind the current runtime
- manual_reload_required: ask for one user /reload in the exact selected window,
  then resume the returned CON; do not send chat text
- restart_required/incomplete loaded inventory: retain the blocker and restart
  only within task scope; repeated reloads cannot discover missing folders
- Foreign owner, combat, restricted focus or stale input: resolve/wait for the
  reported condition, retaining the original IDs

Fix may close the selected PID's existing CON before replacing managed files;
unresolved input or another instance's claim can block a shared installation
update. One PID's reload never activates another PID. After restart, bind the
new process within scope; do not transfer old input or a named actor's request.

First installation may need manual reload because no runtime can register keys
or verify where text would land. Waiting for that action does not spend the later
bind budget. The compatibility reload-fallback command follows the same guarded
activation rules; it does not permit blind Esc/Enter/chat input. Existing user
bindings stay intact; the CLI reads actual effective bindings from Mailbox.
