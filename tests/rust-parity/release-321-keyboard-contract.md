# Go 3.2.1 keyboard increment, stage B2

## Scope and status

Candidate source only, isolated from stage A (products/addon/catalog) and stage C
(Skill wording). Oracle: released Go 3.2.1 commit
`10b561db5706702cd42e047eaeed4ccb5e825351`; the earlier Go 3.2.0
`3057a9666ab078270b63c7f06de748069b0d1f4b` results retain their original pin.
The exact incremental source hashes are in `go-3.2.1-increment-oracle.json`.
No own release version, addon runtime ABI, journal schema, live command or Source
engine is changed by B. Main-worktree writes, commits and publication are outside
this candidate. The full stage owner/dependency hashes accompany this contract.

Rust formatting/parser checks and six Node custody/lexical checks pass. Rust
compilation, Rust tests, native hook tests and Windows own-window fixtures are
**NOT_RUN**. No running game or user input was exercised. Static checks are not
behavioral acceptance, compilation, resource-leak proof or performance evidence.

## Increment table and precise source authority

All links below name the fixed released source rather than moving main.

| Go 3.2.1 change | Rust candidate contract | Evidence and limits |
|---|---|---|
| [All keyboard VKs and 500ms quiet](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/internal/desktop/input.go) | Safe `keyboard_state.rs` checks VK `0x03..0xff`, skips `0x04..=0x07`, requires every key released and 500ms since last event/startup. Uses monotonic nanoseconds, not rounded milliseconds. | Pure tests cover every VK, W held without repeats, repeated/down/up/brief keys, exact and sub-ms boundaries, stopped/invalid clock. NOT_RUN. |
| [Passive listener](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/internal/desktop/keyboard_activity_windows.go) | One dedicated WH_KEYBOARD_LL thread per native backend's first input; retained across retries; callback updates only an atomic latest timestamp and always chains. No event pointer dereference, key/text buffer, logging or key synthesis. | Own hook install/close and expired-start fixtures authored, NOT_RUN. |
| [First-down/readiness ordering](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/internal/desktop/input_native_windows.go) | Two try-only checks: before the fresh INPUT guard, then after it before the first posted down, followed by the original final control/guard/window check. No quiet check/wait once that chord starts. Existing 40ms cadence, full identity and per-message guard remain. | Fault tests check known zero at each admission, no locked wait, six-message active chord completion; NOT_RUN. |
| [Native ownership and outcome](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/internal/live/channel/input_windows.go) and [close](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/internal/live/channel/native_windows.go) | Monitor startup precedes publication lease acquisition. Mandatory monitor passed into production input boundary; no optional readiness bypass. `desktop.keyboard_active` is retryable only when positively zero queued. One-use INPUT witness is consumed even when busy. Close joins independent monitor/process/diagnostic errors. | Tests include zero/partial/full queue failure, witness reuse, startup-before-lease and joined monitor/process errors; NOT_RUN. |
| [Original receipt and fresh INPUT](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/internal/live/channel/keyboard_wait_test.go) | Existing driver/recovery/receipt production remains unchanged. New tests retain original envelope, nonce and durable budget, require a newer INPUT on known-zero retry, and forbid new INPUT/send for submitted/uncertain/missing outcomes while keyboard is busy. | New in-memory driver tests, NOT_RUN. They do not prove real disk custody. |
| [Repeated retries and archive capacity](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/internal/live/channel/input_journal_test.go) | Existing three-turn known-zero driver regression now alternates held/quiet reasons. Existing independent durable checkpoint/immutable archive tests are retained. | The combined 600-turn real-journal, unknown-receipt, original-deadline scenario remains an explicit validation gap. Do not call these separate tests its equivalent. |
| [Cancellation/chord regression](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/internal/desktop/mailbox_input_windows_test.go) | Existing cancellation and errors release only keys successfully posted by this attempt, in reverse order, under independent bounded cleanup; changed identity prevents any further post. Physical keys are never forced up. | Existing fault and owned invisible HWND fixtures retained/adapted, NOT_RUN. |
| [Protocol wording](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/docs/live-protocol.md) | No Esc, Enter, chat or blind fallback; no foreground activation; fresh mailbox INPUT and exact addon receipt remain required. Submitted/uncertain receipt observation is independent of keyboard readiness. | Main tree currently has no `docs/live-protocol.md`; do not import an entire unrelated Go document merely to transplant its paragraph. This table records the protocol delta; stage C carries the exact Skill paragraph. |

## Windows boundary, lifetime and resource review

Only `lycheedev-windows/src/mailbox_input/keyboard_activity.rs` introduces unsafe
sites. Safe orchestration/state modules retain `forbid(unsafe_code)`; live
business code adds no unsafe. Every native site has a local SAFETY explanation.
No `unsafe impl Send/Sync` is introduced. No raw event pointer enters Rust state.

- The hook callback's code address is process-lived. Per-thread immutable TLS owns
  an Arc to safe atomic state. Callback arguments remain opaque scalar values;
  no key names/text or `KBDLLHOOKSTRUCT` are read. The latest timestamp and two
  lifecycle/error flags are the only changing observations.
- The unnamed stop event is uniquely transferred into `OwnedHandle`, then safely
  cloned for the worker. Each owner closes its own handle. The thread owns the
  clone and Arc even if the parent experiences a cleanup timeout.
- `GetModuleHandleW(None)` is borrowed process lifetime, never freed. Hook install,
  message pumping and unhook all run on the same dedicated OS thread.
- `MsgWaitForMultipleObjectsEx` waits for the independent stop event or messages;
  `PeekMessageW` dispatches sent hook callbacks. A bounded 256-message drain checks
  stop priority. There is no timer loop polling keyboard quietness. Shutdown does
  not depend on successfully posting to an as-yet-uncreated message queue.
- Startup has the original caller deadline and a two-second cap; it never creates
  a new operation deadline. The caller rechecks original ordered cancellation
  after startup and preserves both startup/control failures. This is a bounded
  synchronous start, not instantaneous cancellation responsiveness.
- Explicit close marks state stopped, signals its event, then waits at most one
  second for the owned thread handle to become signaled before joining. Timeout,
  wait failure, panic and unhook errors are visible. An unconfirmed join can be
  retried; a completed join is consumed once. Reader close and diagnostics still
  run when monitor close fails.
- On an unconfirmed close followed by object drop, Rust's JoinHandle detaches;
  the already-signaled worker retains its own event and Arc. This prevents memory
  unsafety, but **does not prove prompt OS hook/thread reclamation**. The explicit
  error remains a failure. Drop must not add another hidden one-second wait after
  an explicit close attempt. Native timeout/failure injection is still required.
- Windows may silently remove an over-time low-level hook. This implementation
  follows the Go event-monitor approach and keeps callback work minimal; passive
  installation alone cannot prove continuing OS delivery. Controlled Windows
  evidence is required before claiming lifecycle or quiet-period acceptance.

The API signatures were checked against the pinned `windows` 0.62.2 generated
[SetWindowsHookExW](https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/UI/WindowsAndMessaging/fn.SetWindowsHookExW.html),
[MsgWaitForMultipleObjectsEx](https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/UI/WindowsAndMessaging/fn.MsgWaitForMultipleObjectsEx.html),
and [GetModuleHandleW](https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/System/LibraryLoader/fn.GetModuleHandleW.html)
documentation. [Microsoft's LowLevelKeyboardProc contract](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc)
explains the installer-thread message loop, event-before-async-state ordering,
chain requirement and timeout risk. Documentation review is not compilation.

## Acceptance order

1. Independently review exact frozen owner files, unsafe sites, APIs and test map.
2. Use the repository's controlled official-tool runner for compilation and pure
   tests; do not bypass ownership/admission with bare cargo or adopted caches.
3. Run automated Windows hook/owned-window fixtures without a game. Complete the
   combined real-journal retry/archive scenario and resource failure injection.
4. Only after those gates, schedule separately authorized actual-client checks.

`release-321-keyboard-test-map.json` names the old retained and new incremental
cases individually. Historical test passes are never relabeled as 3.2.1 passes.

## B2 independent-review fix

The B1 candidate checked cancellation/guard/window before the final all-VK
readiness scan and could post a first Ctrl-down after a deadline, cancellation
or identity change occurring during that scan. B2 moves the second readiness
check ahead of the existing final `self.check()` in `Attempt::key`, matching the
fixed Go `chordKeys -> keyboardReady -> key/post -> check` order. This keeps the
original guard count and never checks keyboard quietness after a chord starts.
The new regression injects deadline, cancellation, HWND identity and owner-guard
changes during a successful second readiness check; all require zero queued
messages, zero cleanup key-ups and exactly one mutex finish. Rust execution is
NOT_RUN; B1 evidence is preserved separately.
