# Fixed Go 3.2.3 formal Lua increment

Current oracle: `3766f9cacb52e93a803a626187e2a1d54a445468` (Go v3.2.3).
The complete manifest covers 58 fixture and 55 addon raw Git objects. It is a
new identity, not a relabeling of the frozen Go 3.2.2 manifest or observations.

Compared with `0bec60e4406f5d5dc155ee25cef44f9a710e37e8`, all 113 objects were
checked individually. Only four one-line version literals changed:

- `tests/addon/env.lua` and `tests/addon/t_about.lua`: original 3.2.3 bytes are
  retained here; the other 56 fixtures reuse the authenticated 3.2.2 archive.
- `addon/Core/Runtime.lua` and `addon/Lychee Dev.toc`: the manifest pins the
  original 3.2.3 bytes. `release-fields.json` allows only the two exact complete
  current 3.5.0 file identities and verifies that reversing each sole version
  literal recovers its fixed Go 3.2.3 hash. No other addon bytes are exempted.

The complete old fixture tree and old manifest remain authenticated unchanged.
Execution, case enumeration, stop rules and staged-tree checks reuse the old
checker. The current interpreter build/report, acceptance report and Rust bridge
all bind the new manifest and release overlay. Old interpreter build reports
cannot be reused as current ones. Historical 3.2.0/3.2.1/3.2.2 results retain their
own oracle and do not establish a current 3.2.3 execution result.

## Checks and Windows execution

Portable Node tests inspect all 113 current raw Git objects, complete inventories,
actual staged bytes, exact release allowances, tampering and both bridge identity
definitions. They do not launch Lua, Rust or a game:

```sh
node --test tools/rust-formal-lua.test.mjs tools/rust-formal-lua-bridge.test.mjs
```

Actual current execution remains **NOT_RUN** until separately sealed on Windows
amd64. Use the existing workflow's pinned Rust/Node/MSVC/SDK and authenticated
Emmy preparation. Respect current disk admission, channel ownership and leases.
The standard fresh-output sequence is:

```sh
node tools/rust-build.mjs .tmp/ci-cli-artifact dev --one-shot
node tools/rust-formal-lua.mjs build .tmp/ci-formal-lua-runtime
node tools/rust-formal-lua.mjs run .tmp/ci-formal-lua-runtime/build-report.json .tmp/ci-cli-artifact/artifact.json
node tools/rust-formal-lua-bridge.mjs
```

Do not run these over occupied output directories or rewrite old reports. If
fixed paths are occupied, preserve their evidence and resolve ownership through
the existing controlled workflow first; do not delete a lease or choose another
Cargo cache to bypass it. The standalone Lua command accepts fresh custom build
and artifact paths; the nine-case bridge deliberately retains its fixed CI paths.

Expected Lua denominator: 36 addon suites × six mocked profiles = 216, plus four
standalone protocol suites = 220. The separate Rust bridge must report all nine
exact recovery scenarios, no ignored/filtered tests, and sealed peer lifetime
evidence. These are not real-client or seven-product acceptance. The two other
host bridge gaps are unchanged; the Lua-only report still lists all three host
tests as outside its own scope.
