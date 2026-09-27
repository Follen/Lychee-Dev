# Retail keyboard lab acceptance — 2026-09-27

The isolated addon and external host passed two consecutive background
PostMessage echo transactions on Retail 12.1.0.69933 (interface 120100), Windows
amd64, keyboard layout `0x8040804`. The window was not foreground at the start or
at any recorded protocol observation. This is an experimental transport result,
not acceptance of the production bridge refactor.

Controls: Ctrl+Alt+] wakes; Ctrl+Alt+[ dismisses. The test sender uses
Ctrl+Alt+Shift+] to submit. While the receiver owns focus, bracket handling is
local to that receiver; these tests do not prove physical modifier state or
reliable distinction between modified variants after focus changes.

## Verified evidence

- `.tmp/bridge-input-bracket-final-01/result.json`: `passed=true`, idle → ready
  → staged → accepted → duplicate-submit → cleared. Acceptance count 0 → 1.
- `.tmp/bridge-input-bracket-final-02/result.json`: `passed=true`, wake from the
  hidden state → ready → staged → accepted → duplicate-submit → cleared.
  A fresh nonce was observed; acceptance count 1 → 2.
- Each transaction compared the received nonce, body checksum, byte length and
  acceptance count. Duplicate submission left the count unchanged and focus
  false. Cleanup required two fresh WGC frames without the lab receipt.
- Each output directory contains the expected payload and WGC images of the
  observed stages. Queued input alone was never treated as acceptance.

## Fixes established by the experiment

1. A wake character could enter the newly focused EditBox. Waiting for its
   key-up instead left a recorded `opening` state until timeout. The receiver
   now discards the lone wake character before protocol input starts and does
   not require a key-up event to advertise readiness.
2. The small-module QR fallback searched the upper-left region, whereas the
   independent lab receipt was deliberately lower down. The host now checks
   bounded overlapping left-side bands. A formerly missed `binding_conflict`
   receipt was decoded from the live client after this fix.
3. Temporary F-key diagnostics collided with existing user bindings; those
   keys were abandoned and the requested bracket chords restored. No account
   bindings were saved or replaced.
4. Repeated submission after focus release reopened the receiver. Accepted
   transactions now remain sealed until explicit dismissal. The second passing
   run proves dismissal permits the next transaction.

## Automated checks and limits

`go build ./...`, `go vet ./...`, tagged lab tests and tagged vet passed. The
full `LYCHEEDEV_REQUIRE_LUA51=1 go test -count=1 ./...` passed during this work;
affected tagged tests passed again after the final receiver change. Lua tests
cover the wake-character regression and sealed completed transactions. Image
fixtures cover the lower QR card at fractional module scales.

Classic/Titan, other layouts, combat, relogin, and SendInput are not accepted by
these runs. Cross-transaction delayed commit correlation remains a production
design requirement; the seal tested here protects the completed transaction
until dismissal. This lab only echoes bounded ASCII and never evaluates Lua.
