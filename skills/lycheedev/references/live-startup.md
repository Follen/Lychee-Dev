# First contact and startup recovery

Read this when no usable session exists or the addon was just installed or
updated. Use the CLI's current help/describe contract; this reference does not
introduce a sessionless reload command.

## Interpret the actual failure

Read `context.candidates` as well as the top-level error. In the current CLI,
`live.candidate_missing` means no eligible match, which can include a window
whose identity could not be read.

| Observation | Next useful step |
| --- | --- |
| Empty candidate list | Check current process and installation constraints. A path spelling mismatch is not proof that the game is offline. Reuse the installation spelling returned by discovery with older CLIs. |
| Several eligible matches | Present product/build, character and realm; resolve the remaining ambiguity before mutation. |
| `busy` | Preserve the owner's operation ID. Recover your own operation; do not steal another agent's window or reload it through desktop tools. For explicit user-authorized abandonment of an eligible probe, follow [live-investigation.md](live-investigation.md). Source/data work can continue. |
| `identity_unreadable` | Inspect disk status and runtime evidence below. A timeout does not identify the cause. `capture: frame` does not prove the game is logged in or the addon loaded. |
| `no_actor`, restricted identity, or not-ready input | Act on the returned reason. Login/character selection needs task authorization; combat or text focus must actually clear before retrying. |

Inspect disk state with:

```text
lycheedev addon status --installation <client-directory> --format json
```

`managed` proves receipt/file integrity, not a loaded runtime. `absent` needs an
installation within the task's scope; consult [installation.md](installation.md).
Modified/unmanaged files require the documented delivery/recovery path, not an
overlay copy. An update may also leave a running older release: compare actual
runtime evidence rather than treating the on-disk release as already loaded.

## Activate a runtime without a bridge session

If identity cannot be read, do not call `live reload` with an invented or stale
session, alternate connect/reload in a loop, or send raw chat text through a
shell helper. First inspect the exact game window and its addon UI. Distinguish
disabled, missing from the UI, load errors and a running incompatible release
when visible; leave the cause unknown when it is not observable.

Chat showing `Lychee Dev: identity_busy` (visible in a screenshot) is the
signature of a stuck in-game queue, not a missing addon. Before any UI
maintenance, try the dedicated recovery once on the unowned window:

```text
lycheedev live reset --pid <pid> --installation <client> --snapshot <pin> --format json
```

It sends one fixed nonce-correlated trigger that tombstones the current
character's unacknowledged queue entries and reconnects through the normal
bootstrap. Disk-owned windows are refused. A pending result means no matching
receipt was observed; the trigger may already have executed or displayed a
receipt. Keep that outcome unknown and do not repeat it without new evidence.

When the task already authorizes initial setup or game maintenance and a
supported desktop tool is available, the agent can perform the needed UI action
without asking the user to type a command. Read that tool's skill and use its
documented initialization. Verify the installation/process/window, check the
latest CLI candidate ownership, then use the observed addon enable/reload UI
only for Lychee Dev. Keep other live writers paused for this maintenance; if
ownership or UI state is uncertain, stop the mutation and report the missing
evidence. Do not bypass an operation lock with desktop input.

This is UI maintenance, not a CLI bootstrap input or a saved game session. A
window screenshot can guide the interaction, but real-game acceptance requires
WGC capture. Avoid fixed coordinates, blind Return/Escape sequences and assuming
that reload always discovers a newly installed addon. If the UI cannot activate
it, report the observed requirement; restarting the client or selecting a
different character must remain within the user's authorization.

After an observed activation or other relevant state change, run one constrained
`live connect` using the existing fixed snapshot. Only its fresh identity and
ready evidence establish the session. A second failure without new evidence ends
this attempt: retain the result and diagnose it rather than repeat input.

If desktop control is unavailable, report that specific capability gap after
checking the tool's documented entry point. Do not describe it as a skill
deadlock or claim that `/dev connect` must be entered manually.

## Return to atomic live commands

Once connected, use [live-investigation.md](live-investigation.md): standalone
reload requires the returned session and a stable request key; an interrupted
operation resumes by its original ID. A successful connection is not a probe,
reload, upgrade or full regression pass. Continue the authorized task through
report retrieval, acknowledgement and final receipt dismissal in the same turn.
Only when genuinely blocked, hand off the selected installation, snapshot,
session/operation IDs, actual disk/runtime observations and the next action;
do not hand off an unverified cause as fact.
