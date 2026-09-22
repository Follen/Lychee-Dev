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

Report bodies (`lycheedev.result.v1`) and queue files (`lycheedev.queue.v1`)
travel through SavedVariables and their own encoders, not through this signal
schema.
