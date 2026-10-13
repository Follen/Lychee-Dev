# Exact COMMIT isolated prototype

This increment preserves all v0/v1/P0 sources and frozen evidence. It imports
the existing real Lua 5.1 crypto and observation publisher; it never imports a
production addon or sends Windows/game input.

`receiver.lua` has actual STAGE and COMMIT callbacks. They call a bounded
once-loader, then parse the actual immutable slot file returned by `loadfile`.
The installed empty wrapper once-load behavior is a declared model assumption,
not a verified WoW fact. COMMIT is an exact request file published after fresh
observation and durable authorization; the key itself carries no identity.

The receiver uses a fixed trusted binding, consecutive slot pairs and a maximum
of 500 slots. It is an entry/publication mechanism experiment, not a complete
500-slot allocation or runtime authentication protocol. No expiry primitive,
first activation, BODY cleanup or upgrade/reload recovery is proved here.

`run-lua.mjs` drives actual immutable fixture files and real Lua callbacks from
a Node host. `run-auth-expiry.mjs` uses the separate authenticated receiver
version, validates privately signed Mailbox bytes with Node crypto, and retains
the expected counterexample where a game clock frozen over pause permits an old
queued callback after the host's 750ms deadline. The advancing-clock branch is
a model assumption, not a proved WoW clock primitive. Both runners lack an
outer Job and make no parent-abort descendant cleanup claim.

`host.rs` is an isolated Rust host/test increment. Its controlled execution and
sealed EXE/PDB plus exact source inputs are recorded in the exact-COMMIT document
below; only that fixed scoped result is evidence. It checks actual current time at new fixture input,
and treats expiry discovered after publication as an exposed uncertain file.
Its JobGuard/channel Drop path includes an expected host-panic scenario.

Evidence and source snapshots are referenced by
`docs/toolkit/rust-native-protocol-exact-commit-2026-10-08.md`. Neither these
scoped passes nor the clock counterexample closes the full protocol audit.
The four-slot file design is a comparison baseline, not a proved lower bound;
the independently prepared two-slot candidates and their limited evidence are
in `docs/toolkit/rust-native-two-slot-candidates-2026-10-08.md`.
