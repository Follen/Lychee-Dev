# Historical Native-v4 source

This directory preserves exact bytes from commit
`6699bbff2d04075e25fafd587cb0de1618561f01`. It is source evidence for the
retired memory-slot-v4/mailbox-v2 candidate, not current product acceptance.
The current behavior oracle remains Go 3.2.3 at
`3766f9cacb52e93a803a626187e2a1d54a445468`; Rust candidate 3.5.0 uses beta
distribution configuration without publication authorization.

`manifest.json` records every original path, Git blob, original SHA-256,
historical path, identical historical SHA-256, and isolation reason. The
sixteen preserved files comprise seven addon extras, three automatically
discovered Lua suites, five Lua protocol fixtures, and the implicitly
discovered Rust integration test. Nothing here is a compatibility shim.

## Historical only, not a runner

There is no executable entry for this archive. Original relative paths have
not been rewritten. These bytes need their complete original checkout and
dependencies to be investigated; this directory is not that checkout.
Do not run the archived tests against today's addon or label them current
Go mailbox-v1 coverage. Their earlier success or failure evidence retains
its original commit and status. Existing evidence files are not deleted,
edited, or relabeled by this isolation.

Moving the three Lua suites out of `tests/addon/t_*.lua` removes them from
the old Go discovery glob and BASE-17's encompassing test. Moving
`native_e2e.rs` out of a Cargo integration-test directory removes its
implicit all-target compilation and unfiltered cargo-test entry. Neither
Cargo manifests nor CI workflow lines are used to hide those entries.

## Current coverage retained

The authenticated Go 3.2.2 Lua source set, its 220 observations and six
profiles, the formal 55-file addon closure, and the real Rust
`formal_lua_bridge` Driver/codec/durable-journal test remain untouched.
Go 3.2.3 changes addon release labels only relative to 3.2.2; its
`tests/addon/env.lua` and `t_about.lua` version expectations also change.
This isolation does not relabel 3.2.2 results as 3.2.3 runtime acceptance.
The NativeRuntime rejection fixtures remain active and unchanged.

Fourteen other addon extras still have independent legacy consumers and
remain in the production source tree pending their own migration. The full
static same-addon gate therefore still fails. This partial isolation does
not establish full implementation, runtime parity, performance, or final
audit completion.
