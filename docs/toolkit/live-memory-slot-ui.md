# Memory + slot live: activity and reload UI contract

Status: proposed, not implemented or visually accepted. Part of the [architecture proposal](live-memory-slot-plan.md); cases and evidence requirements are in the [test plan](live-memory-slot-tests.md). This specifies the native WoW Lua UI, not a web mockup.

## Purpose and layout

The developer needs to see that the Agent is connecting or a probe is running, without a receiver panel covering the scene. Retain the existing Lychee logo and `Theme.DrawConnectionBounce` squash/rebound/settle motion. Remove the QR and the companion's dependency on `ReceiptView.AnchorCompanion`; neither a missing QR frame nor its old width can influence positioning.

Anchor directly to UIParent's top-left with an 8 UI-unit inset. Use a 120 x 120 UI-unit transparent, click-through activity container: a 96 x 96 logo area, then one centered 14-unit localized label. Keep the full bounce envelope inside the container. These are initial implementation tokens to verify on real clients; do not shrink text automatically or introduce a full-width overlay to accommodate it. No border, dark card, spinner, progress estimate, raw nonce or request ID appears here. CLI progress and the existing Automation page carry details.

The reload patch uses physical pixels instead: 4 px from the top and left, a 12 x 12 px black surround containing one 8 x 8 px colored square. Use Compat's pixel conversion. The patch and activity share the corner but never show together: an accepted wake permanently stops that runtime's patch before showing activity. Neither control shifts with the console, a receipt, chat visibility or cursor position.

No frame, hook, timer or animation is created just because a passive identity descriptor exists. Keep the explicit minimal bootstrap foundation separately budgeted. Activity frames are created on first use; hidden views remove OnUpdate and unnecessary event subscriptions. The reload patch exists only for opted-in reload indication, with a hard lifetime. Rendering failure cannot abort a probe or leave input captured.

## Activity states

The execution/connection coordinator publishes view facts. The view never infers them from text, focus, a timer or frame visibility, and never emits an execution transition.

| Authoritative fact | Display | Input behavior | Exit condition |
|---|---|---|---|
| Passive discovery, no wake, or connected but idle | Hidden | Normal | An actual bounded connection transaction or probe starts |
| Wake accepted; bootstrap/slot connection transaction in progress | Bouncing logo, `Agent 接管中` / `Agent in control` | Bounded connecting shield below | Commit/dispatch, explicit close, target loss or shield deadline |
| Probe accepted and synchronous/asynchronous work active | Bouncing logo, `Agent 运行中` / `Agent running` | Normal, released before business entry | Actual completion, cancellation or runtime loss |
| Runtime has accepted restoration of an interrupted resumable step | Bouncing logo, `恢复任务中` / `Resuming probe` | Normal outside the short input transaction | Restored probe enters running or recovery stops |
| Result committed in runtime; durable release not yet confirmed | Bouncing logo, `结果待收取` / `Awaiting collection` while collection is active | Normal | Exact operation released, or presentation timeout below |
| No active collector after a bounded collection interval | Static logo and the same collection label for at most 5 seconds, then hidden | Normal | Hidden until a new verified collection/resume transaction |
| Released, explicitly disconnected/disabled, or no current activity | Hidden | Normal | New eligible activity |

Keep the logo continuous through normal prepare -> execution -> collection -> release; changing labels must not flash an intermediate idle frame. A logical connection can outlive individual commands, but is not itself a reason to animate forever. Result collection uses a bounded presentation interval (initial default 20 seconds, refreshed only by a validated active collection transaction). Hiding stale presentation does not discard the retained result, release the owner, cancel the probe or report cleanup success.

A probe has its own execution budget. Do not hide its running indicator just because the original CLI exits while the probe is still running. Conversely, a vanished or finished probe must not keep the `Agent running` label indefinitely. Runtime loss destroys the old view; the new runtime waits for its own wake/binding before restoring task activity. CLI-only scans before wake do not show an addon connection state that the addon has not observed.

The view's completion calls bind the current operation/attempt. A delayed completion from an older operation cannot hide a newer one. User closing the console has no effect on activity. An explicit bridge disable clears presentation and input immediately and is never silently reversed by recovery.

Reduced motion uses the same logo, geometry and text, with a static pose and no OnUpdate. Color is never the only human activity cue. Update `Core/Locale.lua` and `Core/Locale_enUS.lua` together, including all new labels. Use the existing font helper with explicit valid SetFont flags; test Chinese and English text bounds at supported scales.

## Input protection phases

Ordinary queries, listeners and async probes run without a shield and display
`Agent 运行中` / `Agent running`. The view remains click-through and never owns
keyboard focus. A connected-but-idle session has no persistent activity UI.

An actual in-game input lease displays `Agent 接管中` / `Agent in control`.
`InputProtection` owns the shield and its timer, separately from the view:

- Slot dispatch acquires a two-second safety lease and releases before business
  entry, on loader error, or after the control transaction. Scans and receipt
  waits never keep it alive. A completed transport lease cannot release a probe's
  subsequently acquired exclusive lease.
- An explicitly exclusive probe may call `ProtectInput(seconds)` once and
  `ReleaseInput()` when the protected actions finish. The Agent chooses the
  duration; there is no separate five-second cap. It must fit the probe's
  remaining execution budget and cannot be renewed by arbitrary input or a UI
  refresh. Completion/errors release before user cleanup; a failed cleanup
  cannot retain the keyboard/mouse shield.
- Expiry, combat, leaving the world, disabling the bridge or Ctrl+Alt+[
  release the shield. For a running explicit probe lease these are failed probe
  outcomes, not permission to continue unprotected or replay an effect.
- A scene-dependent probe supplies `Guard(check, events, reason)`: checks happen
  at registration, relevant events, wrapped callbacks and successful completion.
  Invalid, secret or unavailable prerequisites fail explicitly. Recovery follows
  the operation's observation/opaque policy; no input lock substitutes for these
  checks. Ordinary queries need no guard frame or timer.
- The shield affects game UI only, not Windows shortcuts or switching apps.
  Synchronous Lua cannot be preempted by a timer; probe code must remain bounded.
  Fixed `/reload` fallback retains host input serialization, bounded sending,
  target/ownership checks and final key release. It also works before the addon
  is loaded, so it cannot promise an in-game takeover label or game shield.
  Fresh runtime/actor binding, not a shield, proves reload success.

Only the explicitly protected phase blocks game input. After release the same
running probe returns to `Agent 运行中`; terminal collection keeps its separate
label and bounded display lifecycle. No invisible shield survives completion.

## One temporal reload patch

Use exactly one colored texture. Cycle red -> green -> blue -> red at 500 ms per phase, starting with red. Retain the black surround to distinguish the patch from normal scene pixels. Do not place three adjacent colors, a QR, nonce, countdown or text next to it.

Arm for an opted-in UI reload. The absolute deadline is startup-arm time + 45 seconds, not time of first visible frame; waiting on loading does not extend it. Show only after relevant world/loading/input readiness events. If loading takes the whole interval, never show it. Initial login is not claimed as a successful reload signal. Missing event capability simply makes this hint unavailable; native memory binding remains the required path.

Stop and remove timers/events on the earliest of accepted wake, deadline, explicit bridge disable, leaving the world or input capability/combat loss. Once stopped, it cannot rearm within that Lua runtime. Ordinary mouse/keyboard activity and repeated loading events do not restart its lifetime. The activity view's lifetime is independent after the handoff.

The CLI observes only fresh frames from the selected window's WGC capture with advancing capture timestamps. Recognize temporal color changes at a stable top-left location, not the old three-block spatial signature. A single screenshot, duplicate/frozen frames, a coincidentally colored scene or a color cycle in another window cannot complete reload. Missing/skipped phases are a hint miss; bounded memory discovery still proceeds. Occlusion, minimization and unavailable WGC must not create a false success or prevent a valid native-memory connection when input conditions are otherwise satisfied.

Reload completion requires the new runtime's fresh binding plus expected release/build/actor and registered slot inventory. Record the patch as an optional timestamped observation with confidence, never as identity, currentness proof or an execution permission.

## Required visual and functional evidence

Capture before/after WGC frames for connecting, probe running, collection, hidden idle and reload patch. Include Chinese/English, supported UI scale extremes, windowed/fullscreen and at least one long asynchronous probe. Test keyboard and mouse while connecting and immediately after dispatch; inspect hidden full-screen frames and active OnUpdate/timer counts.

Verify the top-left position does not reserve the old QR's horizontal space, no panel obscures the scene, reduced-motion is truly static, and all terminal/error paths release input. The tests must distinguish view disappearance from confirmed runtime release. Offline frame mocks prove lifecycle calls; they do not prove rounded pixels, font readability, hardware input behavior or WGC freshness on a real client.
