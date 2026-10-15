# Go 3.2.1 incremental migration: Stage A

This is a narrow candidate, not a completed migration or a new release. The
current business oracle is Go v3.2.1 `10b561db5706702cd42e047eaeed4ccb5e825351` (annotated tag
`637584b8b976966c3863c1889e9ff86fe55493d4`); the business increment is
`83463a03ac99050f6d9e15d9f27849cde53dab10`. The prior Go 3.2.0 pin
`3057a9666ab078270b63c7f06de748069b0d1f4b` and every historical test result remain unchanged.
`go-3.2.1-increment-oracle.json` authenticates the exact 45-file released diff
and separately identifies unchanged command and runtime-layout sources.

## Increment table

| Contract | Exact source | Stage A adaptation | Evidence and remaining work |
| --- | --- | --- | --- |
| Current Retail product is 12.1.5 | [products.go](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/internal/selection/products.go) | Seven delivery products retained, only current Retail series/interface advances | Static catalog equality passed; client execution NOT_RUN |
| Retail 12.1.0 remains compatible; exact source pins map to 120105/120100 | [baselines.go](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/internal/selection/baselines.go), [baselines_test.go](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/internal/selection/baselines_test.go) | One verified-baseline catalog feeds source interface and installation identity; previous exact source pin retained | Source literals authenticated; Rust exact-pin tests NOT_RUN |
| Product plus build chooses the baseline after catalog fallback | [client.go](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/internal/selection/client.go), [client_test.go](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/internal/selection/client_test.go) | Product/build selection replaces first-product-only choice; existing metadata controls and regional isolation retained | New positive, conflict, unsupported and ambiguity tests NOT_RUN |
| Catalog health permits distinct builds for a product | [checks.go](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/internal/selection/checks.go) | Validates product/build and productCode/build uniqueness, digest, interface, TOC and series; removes hardcoded four-product success | Three Rust validator tests NOT_RUN |
| Addon recognizes both Retail interfaces | [ClientGate.lua](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/addon/Core/ClientGate.lua), [TOC](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/addon/Lychee%20Dev.toc), [client-gate tests](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/tests/addon/t_client_gate.lua) | Exact new ClientGate and TOC interface line; existing candidate release values retained | Exact business-byte/static checks passed; standalone Lua fixture NOT_RUN |
| Slot migration accepts the previous six-interface TOC | [slots.go](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/internal/delivery/slots.go), [slots_test.go](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/internal/delivery/slots_test.go) | Current seven-interface list plus exact previous Retail list; existing unknown-payload/ownership guards retained | Static checks passed; 500-slot maintenance tests NOT_RUN |
| Seven interfaces share six event catalog families | [addon-package.mjs](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/tools/addon-package.mjs) | Validator declares seven interfaces and six families, including correction of an inherited four-family assumption | New Node increment checks 7/7 passed; broader package suite 12/13 passed, known version gate failed |
| Keyboard activity gates only fresh chords | [input.go](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/internal/desktop/input.go), [keyboard_activity_windows.go](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/internal/desktop/keyboard_activity_windows.go), [input_native_windows.go](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/internal/desktop/input_native_windows.go) | Deferred to separate Stage B; no Windows or input files changed in Stage A | NOT_IMPLEMENTED by this increment |
| Fresh INPUT, original receipt/deadline, no replay | [keyboard_wait_test.go](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/internal/live/channel/keyboard_wait_test.go), [live recovery](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/skills/lycheedev/references/live-recovery.md), [protocol](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/docs/live-protocol.md) | Separate Stage B must preserve current Rust recovery improvements and extend keyboard-only retry tests | NOT_RUN; existing recovery evidence is not transferred to this pin |

## Boundaries

- No Rust Source engine, loaded source input, performance budget, memory limit,
  Windows FFI, ABI profile/hash or historical runtime evidence was changed.
  Runtime admission obtains fresh metadata through the updated client resolver;
  it does not acquire new runtime acceptance evidence.
- `addon/Core/Runtime.lua` remains the previous 3.2.0 bytes. Its upstream change
  is release-number-only. The TOC also retains 3.2.0. The existing project release
  manifest remains 3.1.2. This version mismatch is a recorded inherited blocker,
  not silently normalized by packaging and not permission to bump a release.
- The old static oracle and formal Lua custody fixtures remain unchanged.
  They are historical 3.2.0 evidence; they will correctly report drift against
  a changed candidate until an explicit separate current-pin harness is reviewed.
- The new Node checks authenticate source facts and packaging structure only.
  They never execute Go, Rust, addon Lua, a game, or researched repository code.
- Rustfmt parsed the staged Rust using the existing official Rust toolchain.
  Rust compilation/tests and Lua execution were NOT_RUN: this Linux staging task
  did not use a controlled Windows runner. No bare Cargo or cache adoption occurred.

## Review and integration

`release-321-increment-freeze.json` records owner and dependency bytes for this
stage. The patch is intentionally unapplied to the main worktree. Before applying,
verify the old owner hashes against the coordinator tree; independently review the
narrow product/client/slot/packaging scope, then rerun current-pin tests on the
actual integrated tree. Existing metadata/SQL/legacy and Source enhancements are
outside this patch and must remain intact.
