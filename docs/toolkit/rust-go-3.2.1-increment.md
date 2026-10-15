# Released Go 3.2.1 migration increment

Current authority: [handover](../../handover.md). Current target is now the cumulative [3.2.2 increment](rust-go-3.2.2-increment.md). This preserved 3.2.1 map remains required, not a passing report.

- Preserved increment source: `v3.2.1`, annotated tag `637584b8b976966c3863c1889e9ff86fe55493d4`, peeled commit **`10b561db5706702cd42e047eaeed4ccb5e825351`**.
- Historical baseline: `v3.2.0`, **`3057a9666ab078270b63c7f06de748069b0d1f4b`**. Existing fixtures, binaries, hashes and results keep this identity.
- Business increment: `83463a03ac99050f6d9e15d9f27849cde53dab10`; later release bookkeeping is not permission to change Rust package versions. Main `1ee747e021da74a53643ec91ebdb16f6ad22e03c` is not the release pin.
- Verification: remote tag object and local exact commit fetch checked on 2026-10-15. [Exact release diff](https://github.com/Follen/Lychee-Dev/compare/3057a9666ab078270b63c7f06de748069b0d1f4b...10b561db5706702cd42e047eaeed4ccb5e825351).

All source paths below remain at the exact 3.2.1 release. Full acceptance remains **PENDING**. The initial candidate-status narrative has been superseded by the implementation and exact-run evidence in handover; this map is not a second live status ledger. Integrated code and individual passing groups do not establish complete cumulative acceptance.

| ID | Contract increment | Fixed Go sources / tests | Rust acceptance |
| --- | --- | --- | --- |
| G321-01 | Retail 12.1.5 / 120105 plus compatible 12.1.0 / 120100 | `internal/selection/baselines.go`, `baselines_test.go`, `client.go`, `client_test.go`, `checks.go`, `products.go` | Resolve product plus build rather than product alone; validate catalog uniqueness and unsupported/mismatched identity; retain old source pin 31c7f7b→120100 and add exact 18c6d677b605b312f3d813d498404631fb941977→120105; seven DataProducts profiles (including separate Forever Asia/CN beta tracks) plus the legacy forever fallback must be preserved; these are distinct from seven approved runtime/source baseline observations. |
| G321-02 | One addon and slot catalogs cover the approved interfaces | `addon/Core/ClientGate.lua`, `addon/Lychee Dev.toc`, `internal/delivery/slots.go`, `slots_test.go`, `tests/addon/t_client_gate.lua`, `tools/addon-package.test.mjs` | Offline Lua gating, main/slot TOC interface lists, exact product/build selection, package consistency; existing addon release strings are not automatically bumped. |
| G321-03 | All physical keyboard keys released, then activity quiet ≥500 ms | `internal/desktop/input.go`, `input_modifiers_test.go`, `keyboard_activity_windows.go`, `keyboard_activity_windows_test.go` | Include VK_CANCEL 0x03 and keyboard VK 0x08–0xFE; exclude mouse/reserved 0x04–0x07. Hook observes last event timestamp only, passes events onward, no text/key history. Fake-clock state tests and owned Windows lifecycle tests prove reset, startup, close and failure cleanup. |
| G321-04 | Fresh authorization and owned input cleanup | `input_native_windows.go`, `mailbox_input_windows.go`, `mailbox_input_windows_test.go`, `input_lock_windows_test.go` | Let an already-started chord finish and release its owned keys before waiting; cancellation releases only keys pressed by that attempt; after quiet time re-read fresh INPUT and revalidate identity/foreground. Preserve ordered cancellation, checked key release/hook teardown and original total deadline. |
| G321-05 | Unknown/submitted operations never replay | `internal/live/channel/input_windows.go`, `native_windows.go`, `keyboard_wait_test.go`, `input_journal_test.go` | Distinguish known not-sent preparation from submitted/uncertain. Wait for original exact receipt; activity/resume cannot resend business input or refresh deadlines. Test interruptions before/after submission, quiet wait expiry, cancellation and recovery. |
| G321-06 | Skill, protocol and acceptance evidence agree | `skills/lycheedev/references/live-recovery.md`, `docs/live-protocol.md`, `Design.md` | Synchronize actual CLI recovery steps and independent Skill use; retain source-first fixed-version rules. Re-export and diff current command/addon/protocol oracle, preserving historical 3.2.0 fixtures rather than relabeling them. |

## Integration order

1. Update current documentation pin first, preserving historical evidence.
2. Stage catalog/addon/selection changes and keyboard/input changes independently against exact Rust owners; no wholesale main merge.
3. Review each increment and map fixed Go tests to actual Rust tests. Run local pure/Lua checks first, Windows-specific fixtures in meaningful native batches.
4. Rerun affected full CLI/addon/Skill/protocol and package checks; re-establish equivalent outputs before the unchanged Rust ≤30% of Go controlled-time measurement.
5. Keep real-user/game keyboard and mixed-client tests in the final section of handover, with blank result fields until performed. Per-batch review is not the four final audit loops.

Emmy direct-library/native-session architecture, full XML/Lua enhancements, minimal unsafe boundaries and the same draft PR delivery workflow remain in force. No new release, main merge or game operation is authorized by this baseline update.
