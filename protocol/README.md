# protocol/

Machine-readable protocol contracts for the Go host and the Lua game side.
Nothing at runtime reads this directory; it is the shared shape reference that
both parsers are tested against.

- `signal.v1.schema.json` — JSON Schema for `lycheedev.signal.v1` receipts,
  including the session-free `identity` marker (correlated by `probeNonce`,
  `actorState`, and `sequence` fixed at 0).
- `samples/` — cross-language sample set. `tests/protocol/identity.lua` emits
  Lua-encoded identity receipts whose key sets are asserted equal to these
  samples, and every sample is asserted to parse through `bridge.ParseSignal`
  and re-marshal to the same field set.
- `receipt.v1.schema.json` — optical-only pairing of a receipt with fresh
  readiness in one QR. The tuple `[sessionNonce, sequence, runtimeEpoch]`
  explicitly carries all three changing session facts; actor/release/build are
  shared with the receipt and the retained session. Readiness has no request,
  code or report digest, and always has `inputReady: true`. The tuple sequence
  must exceed the receipt sequence. A nonce contradiction rejects the envelope.
  `bridge.ParseOpticalSignals` expands it into two independent signals; each
  wait still requires a fresh frame. No report body or stored receipt changes.
  Focus/combat/world transitions invalidate the whole display. Producers must
  regenerate readiness from current state; they may not reuse an old tuple.

Standalone `ready` emits its own nonce and actor so read-only binding does not
depend on an earlier identity trigger. `reported`, `acknowledged` and `cancelled`
may omit the nonce and actor; the host completes them only from proved identity.
Old single-signal and two-symbol displays remain readable by the host.

Report bodies (`lycheedev.result.v1`) and queue files (`lycheedev.queue.v1`)
travel through SavedVariables and their own encoders, not through this signal
schema.
