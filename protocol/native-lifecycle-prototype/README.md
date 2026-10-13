# Shared private lifecycle prototype (prepared, not run)

This source increment is isolated from the addon and product crates. It has no
client input or runtime-key claim. `business.lua` is a single private RUN /
RECEIVED / CANCEL / CLOSE receiver for transport comparison; there is no separate
recovery copy per candidate. `commands.lua` defines its canonical logical bytes.
Transport attempt nonce and physical slots are envelope fields, so an exact
logical retry cannot silently become a new execution or budget.

The receiver retains one task/code and one bounded immutable result body,
publishes BODY before its state, and permits cleanup only after an exact
RECEIVED refers to the current result. A callback return or exception is not
cleanup proof. The adapter inspects its actual owned resources; an effect then
exception retains BODY, and a later new-number retry first inspects already
closed ownership instead of blindly repeating the effect. Public BODY clearing
must succeed before private bytes and task are released. A separately bound
completion survives unrelated command-state publication.

Start/publication/clear/close adapter contracts are explicit: return **true only after
verifying the owned effect**; false/nil or an exception is unconfirmed failure.
The common receiver checks both `pcall` and that exact success value. A real
adapter must normalize void-success system APIs and inspect the owned reference
or resource; the receiver must not guess that a non-throwing call succeeded.
Fault cases must include normal false returns and effects followed by false or
exceptions. A failed BODY/state publication retains private bytes; a failed
public clear cannot release private BODY/task or certify cleanup completion.

Unknown launch, timeout and cancel remain unavailable/unconfirmed until actual
producer completion or verified resource stop; they are not business failure
or success. The original execution deadline and first-cleanup deadline cannot
restart. Cleanup side-effect attempts and private history are bounded. Old
command numbers outside retained history are stale/unavailable, never rerun.
This is not a complete caller-key lifetime or cold-runtime deduplication policy.

The forthcoming shared Rust `advance` method must authenticate actual published
bytes and durably save/hash the exact BODY before preparing RECEIVED. Normal and
restored operation use the same method. Unknown input or lost records remain
unknown/quarantined; result saving does not prove input delivery/drain.

`driver.rs` and `host.rs` now contain that isolated preparation, but neither has
been compiled or run. The host imports exactly one driver and runs the same
fault scenarios through both real Lua callback transports. Root has now allowed
one first controlled P3 window: the thin `native_shared_lifecycle_prototype`
wrapper and `run-rust.mjs` are prepared. The runner invokes the existing
`tools/rust-cargo.mjs` ordinary owner/cache, seals all P3 sources before/after,
and preserves exact EXE/PDB and partial evidence on test failure. See
`docs/toolkit/rust-native-shared-lifecycle-matrix-2026-10-08.md` for written
assertions, uncovered cuts and the conservative restored-intent quarantine's
remaining liveness gap. Completion persistence contains stable exact identity,
so unrelated command publication cannot rewrite or invalidate it.

Tests must enter through real Lua button/once-loader or token callbacks, not
directly dispatch semantic commands from the host. The test adapter's resource
observations are declared fixture authority, not a proof that WoW frame/hook/
timer/focus state or the real execution engine can supply them. P1 private key,
cold binding, reflection/hook isolation and cross-pause clocks remain open.
