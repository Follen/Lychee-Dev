# Controlled metadata consumers, R2

This stage adds the requested real consumer wiring on top of immutable R1. New fixed Go target is 3.2.1 `10b561db5706702cd42e047eaeed4ccb5e825351`. The relevant vault, pins and capture publication sources are compared byte-for-byte with prior Go 3.2.0 in consumer-audit.json. This is not a claim that all of Go selection stayed unchanged: client/source 12.1.5 baseline updates are separate work.

## Selection

`load_pinned_controlled(home, id, control)` retains the existing decode, validation and identity diagnostics, with controlled metadata opening and row reading, then explicit connection close. The existing `load_pinned` keeps its legacy entrypoint and validation body. `pin_selection_controlled` uses the controlled loader for a parent, opens its writer with the same caller control, performs the existing byte-idempotent controlled put, explicitly closes, then checks the original control before returning. No pin/capture aggregate transaction is introduced. Publication remains a single insertion/CAS, and cancellation observed after successful publication does not undo or replay the committed pin.

## Bounded capture

Only `commit_capture_bounded` and its common `commit_internal` are changed. The shared content lock is still held across body publication and manifest insertion; its controlled branch now acquires with the caller control and both branches check explicit lock close. The body identity, timestamp/manifest construction and publication order remain body first, manifest second. The controlled branch now opens metadata with the same control, calls put_controlled, checks its close, releases the content guard, then checks control. Failure after body publication leaves original immutable bytes intact; it does not invent a manifest, delete evidence or retry publication. A late cancellation after manifest commit preserves the committed manifest and reports the stop. Tests inspect this exact durable outcome.

The existing immediate `workspace.busy` contract for content-lock contention is preserved. R1's separate schema lease retries within the original deadline. Existing non-controlled capture metadata entrypoints remain unchanged. Directory/open_blob/Destination-internal Drop cleanup and other verified/budgeted/streamed capture APIs are not claimed migrated by this slice.

## Narrow metadata support

A new `Metadata::read_controlled` retries actual SQLite contention under the original deadline, retains existing read decoding/missing diagnostics, checks cancellation, and preserves read plus timeout-restoration errors. `put_controlled_policy` now uses the original absolute deadline instead of refreshing a five-second cutoff. Every active transaction exit explicitly checks rollback, including successful byte-idempotence and failure/cancellation; no implicit Drop rollback retry hides that cleanup. Primary, rollback and timeout-restore errors use the existing frozen historical typed combiner. Legacy unqualified open/read/put and frozen historical writer bytes remain unchanged. Controlled read and put retain the existing five-second timeout restoration convention for subsequent unqualified operations; callers must continue using controlled operations when needed.

This is boundary control, not syscall/SQLite-statement preemption. No new background threads, network work or generic resource framework is introduced. Test-only observers are private generic callback parameters, with no-op production callers, and allow deterministic stop checks immediately before or after publication.

## Test state

Sixteen new tests: four metadata, five selection, seven bounded capture. All Rust/Windows execution is NOT_RUN. Fixtures own unique roots, use real SQLite or file locks, close healthy handles explicitly, and remove only their own roots. No detached workers are used. Typed close-failure tests inject error values only; no kernel/SQLite close failure is claimed executed. Tests cover original deadline, read-after-open contention cancellation, rollback/autocommit, pre-/post-publication cancellation, retained bytes/identity and separate pin/capture durability.

Rustfmt syntax/static verification does not establish compilation or runtime success. The approved local pure runner has no SQLite/core target; no Cargo, lease/cache adoption, main-tree edits, push, game or release operation was performed. Existing formatting differences before bounded.rs::commit_internal are preserved; parse validation emits formatted output without modifying that unrelated prefix.
