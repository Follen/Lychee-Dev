# First contact and startup recovery

Read this when no usable session exists or the addon was just installed or
updated. Use the installed CLI's help/describe contract for the exact recovery
command surface.

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
| A pending bootstrap attempt with `BTP-...` | Inspect `live status <id>`, then `live resume <id>` for read-only receipt recovery. Neither call sends the uncertain input again. Retain the selected window and installation. |

Inspect disk state with:

```text
lycheedev addon status --installation <client-directory> --format json
```

`managed` proves receipt/file integrity, not a loaded runtime. `absent` needs an
installation within the task's scope; consult [installation.md](installation.md).
Modified/unmanaged files require the documented delivery/recovery path, not an
overlay copy. An update may also leave a running older release: compare actual
runtime evidence rather than treating the on-disk release as already loaded.

First contact, identify, connect and reset can send input before a session
exists. The CLI records those attempts before input and returns a durable
`BTP-...` ID when acceptance is unresolved. Recover that ID before starting
another attempt; queued keystrokes or a visible QR alone do not establish
acceptance. An unknown result does not release another operation's ownership.
If recovery cannot resolve it and the user explicitly decides to stop, `live
abandon <BTP-id>` preserves the unknown result and releases host ownership
without game input. It does not prove that identify, connect or reset had no
effect. Do not use abandonment as an automatic retry step.

## Custom receiver bindings

The addon defaults to `ALT-CTRL-]` wake, `ALT-CTRL-SHIFT-]` submit and
`ALT-CTRL-[` close. Its Settings page and local `/dev receiver bind
<wake|submit|close> <chord>` configure its own runtime overrides;
`/dev receiver reset` restores defaults. Allowed chords begin with `ALT-CTRL-`,
may add `SHIFT-`, and end in `[`, `]`, or `F1`..`F12`. The close key must have
a different terminal key from wake and submit because the focused receiver
cannot reliably distinguish their physical modifiers. Conflict or ineffective
bindings are rejected without overwriting the player's account bindings.

First contact cannot read a custom wake chord before waking the addon. If the
user or verified configuration supplies it, pass the exact chord with
`--wake-binding` to `live instances`, `live connect`, `live reset`, or `live
bind` (with their other required target arguments). `live bind` is for an addon
already connected through its local `/dev connect`: it sends an identity
trigger, checks the effective profile in the fresh receipt, and saves the
connection. It is a game input action even though it does not run business Lua.
The CLI checks the effective three-chord profile in a fresh ready receipt and
saves it with the session.
Do not guess alternate chords after a timeout or infer that a Settings value
became effective without a ready receipt. A later configuration change requires
fresh connection evidence before the CLI sends another command.

## Activate a runtime without a bridge session

If identity cannot be read, do not call session-bound `live reload` with an
invented or stale session or alternate connect/reload in a loop. First inspect
the exact selected process, clean managed installation and available runtime
evidence. Distinguish disabled, missing, load error and an older running release
when observable; leave the cause unknown otherwise.

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

For installation activation or a bridge that cannot be reached, use the selected
target's fixed CLI fallback when that action is authorized:

```text
lycheedev live reload fallback --installation <client-directory> --pid <pid> --request <stable-key> --format json
```

Pass `--session <prior-session>` when a valid prior session helps correlate the
same window and actor. If the selected addon has a known custom wake chord,
pass that exact `--wake-binding <chord>` to the sessionless fallback too; it
must not guess alternate chords after reload. This command alone may send
`Esc` three times, `Enter`,
`/reload`, then `Enter`. The CLI records an `OP-...` attempt and each input step
before sending, checks the exact process/window and clean managed addon, and
verifies the new runtime afterward. Delivery of keys, a black frame or changed
file time alone does not prove activation. Resume the returned operation ID to
observe an unresolved attempt; it must not resend keys whose outcome is unknown.
Do not use fallback to bypass another operation owner or repeat it with a new
request key merely because a receipt is missing.
If observation cannot resolve a fixed reload and the user explicitly decides
to stop, `live abandon <OP-id>` preserves its input progress and unknown
runtime result while releasing host ownership without game input. It does not
establish that reload failed or undo any keys already delivered.

If the addon is disabled in the game UI, reload alone cannot enable it. When
the authorized task includes enabling it and a supported desktop tool is
available, verify the selected process and ownership, make the observed UI
change, then use the CLI for runtime and identity verification. A screenshot can
guide UI maintenance; real-game acceptance uses WGC evidence. Do not infer from
a successful UI click that the new addon version loaded.

After an observed activation or other relevant state change, run one constrained
`live connect` using the existing fixed snapshot. Only its fresh identity and
ready evidence establish the session. A second failure without new evidence ends
this attempt: retain the result and diagnose it rather than repeat input.

If a required enable action cannot be performed, report that specific gap and
retain the selected target and any attempt ID. Do not ask the user to type
`/dev connect` as a routine bridge step.

## Return to atomic live commands

Once connected, use [live-investigation.md](live-investigation.md): standalone
reload requires the returned session and a stable request key; an interrupted
operation resumes by its original ID. A successful connection is not a probe,
reload, upgrade or full regression pass. Continue the authorized task through
report retrieval, acknowledgement and final receipt dismissal in the same turn.
Only when genuinely blocked, hand off the selected installation, snapshot,
session/operation IDs, actual disk/runtime observations and the next action;
do not hand off an unverified cause as fact.
