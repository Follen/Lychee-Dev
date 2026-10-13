# Native v1 isolated prototype

This directory is intentionally not imported by addon TOCs or any production
Rust module. `crates/lycheedev-live/tests/native_v1_prototype.rs` alone imports
`gate.rs`. No game input, deployment, optical or WGC path is exercised.

`node protocol/native-v1-prototype/run-lua.mjs` runs fixed real Lua 5.1 vectors,
copies historical authenticated public records while private combat is blocked,
and cross-checks the MACs with Node crypto. It also reproduces the old generic
APPLY consuming a newly staged command from real Lua callbacks through a
once-only loader. That counterexample is expected, and is not a passed protocol.

`node protocol/native-v1-prototype/run-rust.mjs` acquires the normal controlled
ordinary build channel, hashes the out-of-crate inputs before and after the
build/run, tests the isolated candidate Rust gate against real Lua output, and
seals the actual test EXE/PDB. Coordinate with the shared build owner first.
The local Windows sandbox cannot canonicalize this worktree for Rust or launch
the installed Lua interpreter; the observed run required narrowly scoped host
execution. It does not require deploying or authorizing input to WoW.

The candidate establishes only this property: within a fixed trusted binding,
copied historical state cannot satisfy a fresh OS-random attempt nonce plus a
bounded observation round trip. It removes v0's unsupported two-sample freshness
inference and separate permit. Authentication isolation in WoW, runtime identity,
first activation, generic physical key binding and cleanup remain unproved.

Failure evidence is append-only under `.tmp/rust-supervision/native-v1-*` and
the normal `.tmp/rust-disk/evidence` receipts. Use the v1 proposal for exact
successful and failed evidence references; keep the frozen v0 proposal unchanged.
