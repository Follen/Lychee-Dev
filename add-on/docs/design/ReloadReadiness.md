# Reload readiness handshake

The host recognizes readiness locally instead of sending full-window screenshots
to an agent after every reload. The existing WGC/QR stack is reused. This is an
explicit automation operation; ordinary `/reload` remains unchanged.

## Protocol

`automation.py reload --mode messages --hwnd ... --pid ... --exe-path ...`
generates a nonce, persists its window binding in the host log, then sends
`/dev auto reload <nonce> [ticket]`. A ticket is supplied by the output-report flow;
the controller rejects interruption of an active task or a different visible
completion notice. It never retries the reload command automatically.

The addon saves one bounded schema-1 request under `LycheeDevDB.reloadHandshake`.
It holds the nonce (at most 64 ASCII characters), timestamp and character/realm.
The new Lua session consumes it after the owning `ADDON_LOADED`. An expired request,
normal login, different character, combat or world exit cannot produce ready.
Unknown future schemas are preserved and cannot be overwritten.

Both `PLAYER_ENTERING_WORLD(..., isReloadingUi=true)` and
`LOADING_SCREEN_DISABLED` are required, in either order. The slash handler and
automation overlay must be loaded. The ready QR is independent of a result Ticket:

```json
{"v":1,"kind":"reload","run":"req-...","status":"ready","ts":1234567890,"client":"retail","build":"12.1.0.69875"}
```

The host accepts only this nonce, logs `reload_ready_confirmed`, sends
`/dev auto unidentify <nonce>` and observes disappearance before logging
`reload_ready_cleared`. Normal stdout is a small JSON result, without images.
The signal proves entry into the world and the automation command path, not the
completion of arbitrary third-party asynchronous initialization.

`run` and `bugs` use the same handshake for their output reload before checking
the exact SavedVariables Ticket. Disk write/read and readiness remain separate
claims. ACK of the report remains a separate operation.

## Failure and recovery

Timeout returns the nonce and a local cropped `reload/<nonce>/timeout.png` when a
frame was available. No image is emitted into model input. Only an abnormal run
needs image inspection. The reload intent remains unresolved.

`reload --resume <nonce>` requires the original installation/HWND/PID/executable.
It waits for the same marker without another reload. If readiness was already
confirmed, it only resumes cleanup. Completed history is reported as
`already_completed`, not fresh evidence about the current world state.

The addon expires handshake resources after 120 seconds. Resume after expiry can
remain unresolved; it does not re-arm or silently perform another reload.
The addon and host must be updated together. Older addons do not support this
command; there is no automatic fallback to blind `/reload`.

## Cost and lifecycle

A single nonvisual bootstrap frame subscribes to `ADDON_LOADED` to discover the
persisted opt-in request. This fixed startup cost is required because SV cannot
be read safely at file scope. With no request, its listener unregisters immediately;
there is no overlay, timer, polling, database initialization or idle work.

An enabled handshake reuses that frame, the existing bounded QR pool and one
120-second expiry timer. It is event-driven, has no OnUpdate, and unregisters all
owned events/cancels the timer on cleanup, timeout, combat or world exit. The host
keeps one bounded cropped image while waiting and writes it only on timeout.
No performance improvement or numeric memory claim is made.

## Validation, 2026-09-20

- Nine host protocol tests pass: schema/nonce rejection, stale frame filtering,
  successful cleanup, timeout/resume without resending, missing-frame cleanup
  failure, binding mismatch, ticket forwarding, output read ordering and PNG RGB.
- Lua handshake tests pass in the retail/classic/titan/forever offline matrix:
  disabled startup, same-session refusal, both event orders, old cleanup nonce,
  ordinary login rejection, expiry, character mismatch, combat and reload failure.
  Controller tests verify that only the matching pending Ticket permits output reload.
- Existing host selftest and input-safety tests pass. Full addon matrix, Lua syntax,
  static compatibility and packaging tests pass. Recursive wowdoc validation checks
  45 Lua files with no diagnostics; this is static evidence, not live correctness.
- CLI vendor/selftest and package dry-run verify the distributed command and module.
- No in-game test, installed-skill replacement, game-directory sync, GitHub push or
  npm publication was performed: the parent task is actively using that shared
  game instance and installed automation helper. This side task only changes the
  independent Lychee Dev source repository. Live event ordering, background QR
  detection across a real reload, minimize/reopen and combat remain unverified.

## Versioned API evidence

`wowdoc source list` and `source check` preceded exact-tag queries. Source is
`wow-ui-source`; requestedRef equals matchedTag for each row:

| Product | Ref | Resolved commit |
| --- | --- | --- |
| retail | 12.1.0 | 4e3cbb8c5609e4bfc332c0aebbfa4d79731fab59 |
| classic | 5.5.4 | ecadf9d3326fa87828cacca7f13c0ab5f41840a6 |
| titan | 3.80.2 | 1303c9bdbb7f320fa45db1eaee73acaae3b71fe8 |
| forever | 1.60.1 | 4d5d706b8e01c5ebe01c8dd9b7a07151d8d37069 |

Paths and excerpts are identical at these pins:

- `Interface/AddOns/Blizzard_SharedXML/InterfaceUtil.lua:1`: `function ReloadUI()`
  delegates to `C_UI.Reload()`.
- `Interface/AddOns/Blizzard_APIDocumentationGenerated/SystemDocumentation.lua:112`:
  `PlayerEnteringWorld` / `PLAYER_ENTERING_WORLD`, payload includes
  `isInitialLogin` and `isReloadingUi` booleans.
- `Interface/AddOns/Blizzard_APIDocumentationGenerated/LoadingScreenDocumentation.lua:15`:
  `LoadingScreenDisabled`, literal `LOADING_SCREEN_DISABLED`.
- Same file at line 21: `LoadingScreenEnabled`, literal `LOADING_SCREEN_ENABLED`.

Raw source records and test logs remain under the local ignored `Analyze/reload-*`.
