# Go 3.2.1 durable held-key retry regression, E candidate

## Scope and provenance

Oracle: released Go v3.2.1, peeled commit `10b561db5706702cd42e047eaeed4ccb5e825351`.
Earlier `3057a966...` evidence remains historical. E adds one actual filesystem
regression; it changes no production journal behavior, release or native input.

The driver owner is stacked on frozen B2 bytes, SHA256
`07d84a39d94c907aa87bd4bdb694884483a84fe3e575321bfbabe3d38f242024`.
B2 patch: `f9750bca3fae47bbf70a87ab8fd25e46fd7155994377e07bbfaf7a78402bb704`.
B2 freeze: `17d9ce34f210fe8e6fae15a5ab544e8f66e25142ea02c4abe8e038a601f2`.
Remaining sources/dependencies are individually hashed current-worktree snapshot
bytes. Observed HEAD is provenance only: stacked bytes are not claimed to be that
commit's bytes. All existing B2 driver test bodies are preserved.

## Actual regression boundary

`go_321_durable_keyboard_retries_preserve_archive_capacity_and_original_deadline`
reuses FakeBackend/FakeClock/Tokens, continuation Schedule/target, and the existing
durable Fixture. A thin transport adapter cancels each bounded turn after input
or receipt observation, and qualifies each new INPUT through the production wire
checksum and input decoder. Runtime discovery/replacement/retirement fail closed.

Every turn explicitly closes the real execution lease, reacquires and loads
DurableChannelJournal. Fixture trees remain retained under the existing policy.
Fixture type and new/open/seed become crate-visible only inside cfg(test); its
containing test module becomes sibling-visible. Production visibility is unchanged.

One end-to-end Rust test asserts:

- Already-bound original request, 600,000 ms budget, 500 ms clock advance per turn.
- 600 alternating desktop.keyboard_active/desktop.key_already_held known-zero
  outcomes, fresh checksum-valid INPUT, unchanged request/ticket/nonce/deadline,
  one publication and actual checkpoint rotation under the production threshold.
- Attempt 601: uncertain, one queued message, exact error. Two disk-reloaded
  resumes observe only its original receipt, with no new INPUT or replay.
- Original deadline persists sticky exhaustion without sending. Close intent
  survives reload and remains closing_exchange_unconfirmed without republishing,
  replaying or changing the original business deadline.
- Traverse immutable predecessors. Validate every event hash/sequence/link,
  predecessor tail checksum, byte-for-byte archive retention and hydrated
  artifact digests. Compare all 601 unique full intents and 601 exact outcomes,
  original request/envelope/budget, exact reason/time/queued-count; exactly 600
  outcomes are known zero. Reject duplicates, loops, missing/outside archives.
- At least two actual segments; every segment stays below MAX_EVENTS (1024).
  The 600 turns exercise real checkpoint capacity, not a FakeJournal counter.

## Fixed Go citations

- [Bounded peer and regression, lines 17–230](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/internal/live/channel/input_journal_test.go#L17-L230)
- [Checkpoint before input intent and exact outcome flush, lines 75–106](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/internal/live/channel/input.go#L75-L106)
- [Checkpoint thresholds and predecessor traversal, lines 13–100](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/internal/live/channel/history.go#L13-L100)
- [Checksum, hard limits and immutable checkpoint storage](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/internal/live/journal/memory_log.go#L18-L220)

## Evidence and execution

Node checks custody and lexical coverage only. Rustfmt parses the owners; neither
is typechecking or Rust execution. E Rust compilation/test, Windows filesystem,
actual keyboard/game behavior and performance are NOT_RUN. The available product
runner requires Windows. Existing Linux pure recipes do not include Live/journal.
No new framework, bare cargo/rustc, cache adoption, download or toolchain install.
No new result is attributed to earlier candidates.

After exact delta integration in an authorized complete Windows checkout, use:

    node tools/rust-cargo.mjs --owner release-321-durable-keyboard --one-shot -- test --locked --offline -j 1 -p lycheedev-live --lib go_321_durable_keyboard_retries_preserve_archive_capacity_and_original_deadline -- --nocapture --test-threads=1

Also retain existing driver, durable_journal and recovery/reload groups. This
isolated audit snapshot is not a replacement workspace or a new runner. Independent
review and actual controlled Rust/Windows execution remain required.
