# Keyboard input lab

An isolated addon and Windows host experiment for the proposed bridge receiver.
It echoes a bounded ASCII string and counts accepted submissions. It never loads
or evaluates submitted Lua. This is test tooling, not a new `lycheedev` command.

## Run locally

From the repository root (Windows amd64, Go, Node, Lua 5.1):

```powershell
node tests/bridge-input/lab.mjs build
node tests/bridge-input/lab.mjs test
node tests/bridge-input/lab.mjs install --installation 'D:\Game\World of Warcraft\_retail_'
```

Installation creates only `Interface/AddOns/LycheeInputLab`. It refuses unknown
or edited files in that directory and never overlays a managed Lychee Dev addon.
It copies the repository's existing QR encoder with its BSD notice intact and
generates a unique install token. Every reinstall requires runtime activation;
an old token does not prove that newly installed files loaded. No account key
bindings are saved and no production SV or legacy user data is imported.

Choose the exact PID and client directory. Each output directory must be new:

```powershell
node tests/bridge-input/lab.mjs run --pid 12345 --installation 'D:\Game\World of Warcraft\_retail_' --action inspect --out .tmp/input-before
node tests/bridge-input/lab.mjs run --pid 12345 --installation 'D:\Game\World of Warcraft\_retail_' --action inspect --capture-area window --out .tmp/workbench-before
node tests/bridge-input/lab.mjs run --pid 12345 --installation 'D:\Game\World of Warcraft\_retail_' --action reload --out .tmp/input-activation
node tests/bridge-input/lab.mjs run --pid 12345 --installation 'D:\Game\World of Warcraft\_retail_' --action probe --out .tmp/input-background
node tests/bridge-input/lab.mjs run --pid 12345 --installation 'D:\Game\World of Warcraft\_retail_' --action dismiss --out .tmp/input-cleanup
```

- `inspect`: read-only native window identity and WGC capture; no input.
  `--capture-area window` captures the full game window for workbench visual
  review; the default bounded receipt area is unchanged. The output is
  `before.png` inside the new evidence directory.
- `observe`: read-only capture/decode of the installed lab's receipt.
- `reload`: one fixed `Esc × 3 → Enter → /reload → Enter`, then verify a new lab
  startup receipt with the current install token. It does not restart the game
  or claim that every client discovers newly installed addons on reload.
- `probe`: read baseline, wake, verify focused `ready`, send a checksummed echo,
  read exact `staged`, submit, verify payload digest and exactly one acceptance,
  then check that repeating submit does not accept twice. Once focused readiness
  is verified, success and failure both attempt to release input and verify the
  lab receipt is cleared; cleanup failure stays explicit.
- `dismiss`: send the lab's release shortcut and verify its receipt is absent.

`postmessage` is the default and never activates the window. The separate
`--mode sendinput` comparison requires the chosen game window **already** to be
foreground; it does not activate it or silently fall back. This experimental
mode is excluded from normal CLI builds. It is not approval to change the
product's background-only transport contract.

The sender records the target keyboard layout. OEM bracket virtual keys currently
test US bracket positions; a successful Chinese IME/layout run does not prove
all non-US layouts. The input mutex and per-message HWND/PID/process-start checks
come from `internal/desktop`. The host also refuses input while any production
operation owns that window, including an unresolved operation without an active
sender. Inspecting a busy window is allowed; mutating it is not.

## Addon controls

| Action | Key |
| --- | --- |
| Wake | Ctrl+Alt+] |
| Submit | Ctrl+Alt+Shift+] |
| Release focus and hide lab receipt | Ctrl+Alt+[ |

The visible **Release input** button and manual `/lycheeinputlab hide` also close
the lab. `/lycheeinputlab` manually opens it; that is useful to separate receiver
behavior from a broken host wake, but is **not** evidence that host wake worked.
Bindings activate on key-down. A lone wake character `]` is discarded before
protocol input starts; readiness does not wait for an EditBox key-up event that
may never arrive after focus changes. While the dedicated receiver owns focus,
`]` submits only an already validated frame and `[` cancels. These punctuation
keys are not valid echo payload characters. Global actions retain the chords
above. Ordinary Enter remains unrelated to submission. Existing bindings cause
a startup conflict rather than being overwritten. Closing persists the hidden
preference across reload for the same install token; a later probe can wake it
without a baseline card. Reinstalling creates a new token and startup receipt.
Ordinary Enter/Esc/Tab do not submit. Focus loss, leaving world, combat, or the
non-renewable 20-second deadline releases an active receiver. Repeated wake does
not renew its deadline. After acceptance, wake cannot open another receiver until
explicit dismissal; this also fences delayed submit keys after focus release. No permanent OnUpdate loop or global input hook is used.

The lab receipt is offset below the production receipt at `(16,-512)`; it must
not cover Lychee Dev's `(16,-16)` card. This is test UI, not the product's proposed
visual design. The lab does not install a full-screen mouse-blocking modal.

## Evidence and limits

`intent.json`, `expected.json`, `observations.json`, `result.json` and WGC PNGs
separate queued messages from received and accepted input. `LycheeInputLabDB`
keeps at most 64 event entries and marks truncation; it is written to disk only
by the game's normal SV lifecycle. It is not a realtime result channel.

Failure to observe wake means the host sends **no body or submit**. A timeout is
not proof that keys were ignored. Output folders are never reused and uncertain
attempts are not automatically retried. Inspect the retained evidence first.
The lab's repeated-submit check covers one completed transaction, not delayed
commit keys crossing receiver attempts; production commit correlation remains
to be designed and fault-tested.

Offline Lua assertions cover cold wake, focused-key dispatch, exact-frame parsing,
checksums/nonces, freezing, no ordinary-key submission, duplicate submit, timeout,
focus loss, idempotent wake and bounded logs. Native window tests prove Win32
message order and key release, **not** WoW's physical modifier state. Only an
observed game `ready → staged → accepted` chain proves that tested client/mode.
Classic/Titan, alternate layouts, combat and relogin require separate live runs.

The pinned Retail key-name reference is
[KeyCommand.lua at 09b9db7](https://github.com/Gethe/wow-ui-source/blob/09b9db7948abc9b9648dedaab51eb0cf3ee67b31/Interface/AddOns/Blizzard_SharedXML/KeyCommand.lua).
The canonical order is ALT, CTRL, SHIFT. Source support alone does not prove
PostMessage changes the modifier state observed by that code.

The host scans bounded overlapping left-hand image bands for the lab card. This
allows the small-module decoder to find the lower card without moving or changing
the production receipt. Recognized startup conflicts stop probes before input.
