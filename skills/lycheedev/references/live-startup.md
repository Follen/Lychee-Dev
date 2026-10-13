# Live doctor and startup

Use this for first contact, a newly installed or upgraded addon, or a startup
failure. Disk installation and a loaded runtime are separate facts. Native live
uses Lycheedev mailbox protocol v1: a named Mailbox, 500 slots and two formal game bindings.

## Diagnose the selected target

Retain the authorized installation and PID, or the original project and CON.
Use `live instances --passive` only when a target is missing. Folder names,
shared builds and the Windows foreground window do not identify a connection.

```text
lycheedev live doctor --project <project-directory> --installation <client> --pid <pid> --format json
lycheedev live doctor --project <project-directory> --session <CON-id> --format json
```

Doctor checks the managed files, 500-slot inventory, PID ownership, runtime
locator, independent bootstrap diagnostic, current runtime and fresh INPUT.
It sends no keys and creates no connection. Read each check’s state, detail and
nextStep. A bootstrap snapshot explains startup; only fresh INPUT authorizes
machine input. Top-level `doctor` with the same target selectors includes these
same checks alongside workspace/data/source checks.

## Complete the authorized repair

Use the installed CLI's describe/help to confirm this command is available.
Within an authorized live task, activation and recovery need no repeated approval.
Use `--fix` when diagnosis requires installation, activation or connection recovery:

```text
lycheedev live doctor --fix --project <project-directory> --installation <client> --pid <pid> --format json
```

The packaged CLI uses its own distribution. Development or separately staged
releases can supply `--release <release-root>`; `--output <archive-directory>`
selects the upgrade archive. The CLI installs absent files or upgrades a clean
managed installation through the existing installer. Modified/unmanaged files
need [installation](installation.md) diagnosis, not an overlay copy.

| Result | Continue |
| --- | --- |
| Ready runtime, clean installation | Connect and keep the returned CON |
| Two formal bindings loaded under the previous protocol name | Let fix/resume send the verified reload key once, then bind the current protocol; do not ask for manual reload merely because the name changed |
| Existing connection in this project | Resume its original request; pending input is not replaced |
| Foreign project owns the PID | Continue from the owning project; do not delete its claim |
| manual_reload_required | Ask the user to reload once in the selected game window, then resume the returned CON |
| Bootstrap failed | Read its startup reason and addon errors; the manual reload binding remains available |
| restart_required / incomplete loaded inventory | Restart that selected client within scope; repeated reloads do not discover missing folders |
| Combat, restricted editor, stale INPUT | Preserve the CON and wait for the reported condition to change |

When updating a selected PID that already has a connection, fix first completes
its recovery and disconnects before replacing files. Unresolved input or another
instance’s claim can block the shared-directory update; retain the original IDs.
Each process sharing the installation must load and verify the new runtime
independently. One PID’s successful reload never activates another PID.

## First installation and reload

A loaded runtime with fresh two-key INPUT can reload automatically during a
protocol-name upgrade. The CLI reads its effective reload binding, journals
one key delivery and observes the new runtime. Its old business wire is not
reused. A pending activation is reassessed by resume when this capability is available.

If the addon is not running, it cannot register a key or report where text would
land. The CLI records `activation_waiting_manual_reload` and sends nothing.
The user enters `/reload` once in that exact game window; then:

```text
lycheedev live resume <CON-id> --project <project-directory> --wait-seconds 120 --format json
```

Manual waiting does not spend the later connection budget. Once the CLI observes
the new runtime, normal bounded binding begins. If the client must restart to
find new slot folders, retire the old process's connection and bind the new PID;
do not transfer unknown input to it. Preserve the selected actor when specified.

The compatibility command `live reload fallback` follows the same activation
rules. It no longer sends Esc, Enter or chat text. Older input-v3/hybrid-v2
runtimes require manual activation; their records do not authorize new keys.

## Keyboard behavior

The keybinding page shows Execute and Reload under Lychee Dev. Defaults are
Ctrl+Alt+F12 and Ctrl+Alt+F11; free conflict fallbacks are F10 and F9. Existing
player assignments are preserved, and the CLI reads actual effective keys from
Mailbox. `/dev receiver reset` fills missing actions using free defaults.

Ordinary EditBoxes selectively forward these two actions while keeping draft,
cursor and focus. Password/secure/protected editors stay blocked. Native input
uses zero Esc; reload directly invokes the UI API. A submitted or unknown key
is observed through the original CON rather than retried. See
[live recovery](live-recovery.md) for retained input and runtime retirement.
