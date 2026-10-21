# Fixed Go 3.2.4 formal Lua increment

Oracle: tag `v3.2.4`, peeled commit
`29bdbdaff5f064ec212ff2064380b1ccac652eda`. All 58 fixture and 55 addon
objects are pinned by raw Git blob identity, byte count and SHA256.

The 3.2.3-to-3.2.4 increment contains three changed fixtures (`env.lua`,
`t_about.lua`, `t_focus_bindings.lua`) and four changed addon files
(`FocusBindings.lua`, `InputState.lua`, `Runtime.lua`, the unified TOC).
The two bridge files and focused-binding test match the tag byte for byte.
The new test retains typing/key/lifecycle coverage and adds combat entry,
original-handler combat changes, deferred teardown and restart-before-cleanup.
It remains one of 36 suites run independently under six mocked profiles.

Only the exact Runtime/TOC version literals retain Rust release 3.5.0.
The staged Go fixture `env.lua` and `t_about.lua` remain original 3.2.4 bytes;
the repository's ordinary addon tests retain their existing 3.5.0 expectations.
Unchanged fixtures reuse the authenticated 3.2.3/3.2.2 inventories. Those
manifests, fixture bytes and historical execution evidence are not rewritten.

The current tools bind this manifest and release overlay. Current execution is
**NOT_RUN** until separately sealed: 216 addon/profile processes plus four
standalone protocol processes, and the separate nine-case Rust bridge.
Old interpreter/build/bridge reports do not establish a 3.2.4 pass.

Portable checks (no Lua, Cargo or game execution):

```text
node --test tools/rust-formal-lua.test.mjs tools/rust-formal-lua-bridge.test.mjs tests/rust-performance-paired/live-runner.test.mjs
```

The existing `tools/rust-formal-lua.mjs build/run`,
`tools/rust-formal-lua-bridge.mjs` and `tools/rust-live-offline-performance.mjs`
remain the execution entries. Use fresh output paths, owned user isolation,
the default disk policy and no one-shot Cargo override. The bridge requires a
matching current sealed CLI artifact. These fixtures do not operate a client
or establish final protocol, game behavior or paired Go performance acceptance.
