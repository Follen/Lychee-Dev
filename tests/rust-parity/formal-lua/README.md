# Fixed Go Lua acceptance entry

Oracle: Go v3.2.0 commit `3057a9666ab078270b63c7f06de748069b0d1f4b`.

`oracle/` preserves every byte of all 38 `tests/addon` files, all 13
`tests/protocol` files, the shared WoW global loader, all four baseline probes,
and `internal/testkit/lua.go`. The hard-pinned manifest records SHA-256, Git blob
SHA-1 and size for these 57 files and all 55 formal addon resources. No source
is taken from a moving branch or from the current, partially historical tests.

## Run

On Windows amd64, with the exact MSVC/SDK versions already specified in
`release/tools/luac.json`:

```sh
node --test tools/rust-formal-lua.test.mjs
node tools/rust-formal-lua.mjs build .tmp/ci-formal-lua-runtime
node tools/rust-formal-lua.mjs run .tmp/ci-formal-lua-runtime/build-report.json .tmp/ci-cli-artifact/artifact.json
```

The builder accepts an optional second path to the exact official
`lua-5.1.5.tar.gz` source archive to avoid downloading it twice. It independently
builds a real interpreter, including `lua.c` and excluding `luac.c`/`print.c`.
It does not change the release's `luac` identity or replace any installed tool.
Official archive SHA-256:
`2640fc56a795f29d28ef15e13c34a47e223960b0240e8cb0a82d9b0738695333`.

The runner copies only authenticated addon resources and original test bytes
to its fresh owned scratch tree, preserving relative paths. It invokes the
original `run.lua`, `wow_globals.lua`, `env.lua` and real suite assertions in
35 suites × six profiles: retail, classic, titan, forever, classic-era and
anniversary. Four independent protocol Lua suites also execute. Every one of
these 214 cases has an independent Windows Job, exit status, captured output,
completion marker check, process/handle cleanup proof and per-case result.
Ordinary assertion failures do not skip later fixtures. A deadline hit, forced
retirement or cancellation stops later launches even after confirmed cleanup. Unverified cleanup,
identity drift, aggregate output exhaustion or the 20-minute total deadline
stops new launches and marks all remaining cases `not_run`.

The 10-minute build and 20-minute acceptance budgets include a cleanup reserve;
each Lua invocation receives at most 20 seconds. Existing owned-process output
limits apply, with a separate 64 MiB evidence budget and reserve before each
launch. `LUA_INIT` and other inherited Lua hooks/search paths are removed.
The interpreter's complete same-owner sealed logs, command requests, process
proofs and outputs are authenticated. The Rust artifact requires its matching
sourceRoot/sourceDigest and complete successful build receipt/logs before and
after the entire run. Source fixtures, staged files, current addon, interpreter
and support identities are also checked around every fixture. Evidence is retained before owned scratch cleanup.

## Meaning and limits

- Node tests check authentication and runner failure paths. They are not Lua
  execution results. Current Windows execution remains `NOT_RUN` until CI runs.
- `go-test-map.json` maps all nine original Go test functions. The TOC check is
  a separately labeled static translation, not part of the 214 Lua executions.
- `slot_server.lua` is retained verbatim. `TestMemoryRecordLuaParity`,
  `TestSlotDriverLuaRecoveryWithoutBusinessReplay` and
  `TestSlotDriverLuaReportPreservesFiniteNumbers` remain explicit, separately
  reported `not_run` gaps for a faithful Rust host integration. This entry does
  not execute the old Go implementation or simulate its results.
- The Rust binary is bound to the evidence for candidate identity only; these
  Lua suites exercise addon Lua, not the Rust binary or bridge implementation.
- API mocks across six harness profiles are not live acceptance for seven
  products. The two Forever beta products are not claimed separately here.
- Existing addon extras and all old test/package consumers stay in place and
  appear in the report. This entry does not claim their cleanup is complete.
