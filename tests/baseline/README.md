# Baseline entrypoints after legacy addon isolation

The current behavior oracle is Go3.2.3 commit
`3766f9cacb52e93a803a626187e2a1d54a445468`; Rust candidate is 3.5.0 beta.

`tools/baseline.mjs` remains an offline legacy-Go regression aggregator. It is
not the complete Rust acceptance runner. BASE-08/10/12 now require named
current addon Lua subtests on each of six profiles. BASE-17 still requires
the encompassing workbench suites and unified TOC contract. Missing, skipped
or failed child results cannot pass those cases. Other Go catalog entries
retain their original host regression scope.

The current addon suites use fixed Go323 test bytes, with only release
literals set to 3.5.0. Old widget-only APIs are retained in historical source.
The existing authenticated formal 322/323 Lua fixtures and real Rust
`crates/lycheedev-live/tests/formal_lua_bridge.rs` Driver/codec/durable-journal
regressions remain separate and unchanged. Passing one does not prove the other.

The old `tools/live-baseline.mjs` OP/session/ACK/display runner and its old
probes are preserved byte-for-byte under `tests/history/legacy-ui-00fd643/`.
There is no executable historical runner. Do not run those sources against
the current addon, rewrite their outcomes, or treat old failures as passes.
Current probes in `probes/` are fixed Go323 sources covered offline by
`t_baseline_probes.lua`. Actual games, user input, multi-instance behavior,
Windows and manual cases require separate authorized acceptance.

This cleanup executes no game action, release operation, Cargo or Go suite.
See the isolation manifest for exact original SHA-to-historical SHA custody.
