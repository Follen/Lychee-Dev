# Fixed-release Mailbox v1 contract adapter

Oracle: Go v3.2.0, peeled commit
`3057a9666ab078270b63c7f06de748069b0d1f4b`.
All Go paths below are relative to that tree. The repository `handover.md`
remains the sole migration decision/status/evidence source.

This is the fixed-release decoder/data boundary used by the existing named Lua
reader and native read-only identity observation. It does not publish slots or
send input. No real process/game was operated to implement this slice.
The default Live transaction route remains unavailable. The legacy injected-root
route now fails closed before constructing its old four-slot driver; completing
the formal driver must replace that temporary migration boundary.

## Source disposition

| Existing source / fixed Go source | Disposition in this slice |
| --- | --- |
| Rust `mailbox::Memory` safe interface / Go `memory.Source.Read` | Retain the reader abstraction for injected synthetic memory and the existing budgeted native adapter. No FFI change. |
| Rust `mailbox.rs` wire magic `LYCMEM07`/`LYCEND07`, kinds 1–5 | Replace existing `mailbox::encode/decode` with delegation to `wire.rs`: `LYCMEM06`/`LYCEND06`, kinds 1–6, 80-byte header, 40-byte trailer, Adler32, 512 KiB payload cap. Do not mix the two byte formats. |
| Go `internal/bridge/memory_record.go` and `internal/live/memory/records.go` | Implement fixed-header qualification before bounded second read/allocation; reject torn headers; qualify BODY with exact nonzero tokens plus HEAD length/checksum, including empty BODY. Return borrowed payloads when decoding existing bytes. |
| Go `internal/live/channel/protocol.go` | Implement identity and receipt field/header checks, exact envelope correlation and distinct rejected receipts. Keep known 200-slot descriptors separate as manual-activation evidence. Do not relabel old schemas or grant business input. |
| Go `internal/live/channel/input.go`, `input_bindings.go`, `input_attempt.go` | Implement pure INPUT schema/readiness/profile/target/time checks; strict optional diagnostic whitelist and raw 512-byte bound; only known zero-message `not_sent` can be considered for re-preparation. These are necessary predicates, never input authority. |
| Go `internal/live/memory/lua_mailbox.go` and map metadata test | The logical exact-token view remains a data helper. Existing `mailbox.rs` now uses formal schema/kinds and actual named Lua traversal with schema-only one-bucket fixtures, missing receipts/empty BODY, bounded two-read wire checks, and path-guard-before-Accept. Complete Go firstReadFailure diagnostics remain a later slice. |
| Rust `native.rs` target discovery / `runtime_observation.rs` | Replace old identity.v3 parsing with the formal decoder. Observe optional inventory without equating capacity with loaded slots; retain manual-activation diagnostic codes. Default missing-root locator-not-finalized and ObservationOnly remain. Explicit old-root input construction is temporarily blocked pending the complete formal driver. |

The `IdentityPublication` variants are Rust representation choices, not new
wire tags. `ManualActivationRequired` corresponds to Go's joined
`memory.mailbox_incompatible` and
`live.channel_legacy_identity_requires_activation` result. An old 500-slot
capability is retained with its original schema for activation inspection.
A current identity still proves neither fresh INPUT nor an authorized action.

`read_record` must be called through a selected, budgeted reader. It deliberately
does not claim path stability: the named-field adapter must recheck the root,
publication path and selected process identity before consumers use its bytes.
`BodyQualification` records caller-supplied verified-HEAD evidence; constructing
it does not verify that HEAD. Likewise, `ExpectedEnvelope` is only the bounded
set of fields consumed by these correlators, not a slot writer or transaction.

Optional `inputAttempt` diagnostics cannot supply a receipt, move a transaction,
change readiness, or permit replay. Invalid optional data is discarded while
original target/readiness/time failures remain failures. INPUT DTO serialization
preserves validated inputAttempt evidence but omits host observedAtMillis; its
custom deserializer uses raw JSON for the same 512-byte diagnostic limit in
both wire and journal inputs. The inspect_input/decode_input wire entrypoints
impose the outer 2048-byte bound; DTO journal reads do not. A missing persisted
input outcome remains unknown, even with a `received` diagnostic or fresh INPUT.

## Fixed Go fixture mapping

Pure contract tests use the `go_mailbox_v1_` prefix. Native read-only integration
fixtures additionally live in `tests/mailbox_lookup.rs` and
`runtime_binding::tests`; `tests/sealed_memory.rs` is a Windows self-owned-process
fixture, not a game test. None establishes whole-driver or real-client acceptance.

| Rust tests (suffix after the prefix) | Fixed Go source/test mapping |
| --- | --- |
| `wire_exact_empty_body_golden`, `wire_all_kinds_and_payload_bounds`, `wire_rejects_corruption_and_old_rust_protocol`, `old_wire_names_rejected_even_with_valid_header_checksum`, `fixed_go_binary_record_vector` | `internal/bridge/memory_record_test.go`: `TestMemoryRecordIntegrityAndBounds`, `TestMemoryRecordRejectsOldWireVersions`; Bootstrap kind 6 from `memory_record.go`. The binary golden uses the exact Go test's UTF-8/NUL payload and tokens, independently encoded with Python struct/zlib; it is not claimed as a fresh Go test execution. |
| `token_exact_lowercase`, `identity_schema_and_capacity`, `identity_header_correlation` | `internal/live/channel/protocol_schema_test.go`: `TestIdentityAndReceiptOnlyCurrentSchema`, `TestIdentityPublicationRejectsMalformedChecksummedHeaders`; `tokenBytes` in `protocol.go`. |
| `legacy_200_is_activation_only`, `old_500_capability_stays_labelled` | Same file: `TestLegacyIdentityOnlySelectsKnownUpgradeCapabilities`, `TestPublishedLegacyIdentityRequiresManualActivation`. |
| `receipt_exact_match_and_rejection`, `confirmation_and_bind_zero_ticket` | `decodePublicationReceipt` and `decodeReceipt` in `protocol.go`; current-schema receipt case in `protocol_schema_test.go`. |
| `synthetic_memory_empty_body_and_allocation_gate`, `synthetic_memory_torn_read_and_report_bounds`, `body_static_header_table_before_payload_read` | `internal/live/memory/records_test.go`: `TestBodyRequiresExactHeadAuthorization`, `TestBodyStaticHeaderRejectedBeforePayloadRead`; `ReadRecord` second-read/header check in `records.go`. |
| `schema_only_public_map_missing_is_not_empty_record` | Logical subset of `internal/live/memory/lua_mailbox_map_metadata_test.go`: `TestMailboxSchemaOnlyMapsKeepMissingRecordsComplete`. Its actual Lua counterpart is now `tests/mailbox_lookup.rs::schema_only_lua_maps_preserve_exact_missing_keys`; neither is a real-client result. |
| `disabled_diagnostic_never_grants_receipt_or_replay`, `optional_diagnostic_is_strict_and_non_authoritative`, `diagnostic_dto_roundtrip_preserves_evidence_not_authority`, `dto_optional_fields_and_raw_bounds_match_go` | `internal/live/channel/input_attempt_test.go`: `TestDisabledSlotDiagnosticPreservesInputEligibility`, `TestInputDiagnosticOptionalPreservesEligibilityAndErrors`, `TestInputDiagnosticRequiresReceivedBoolean`. Journal/probe portions of that Go suite are not covered by these pure predicates. |
| `input_readiness_and_freshness_are_separate_gates`, `two_key_binding_contract` | `internal/live/channel/input_test.go`: `TestInputObservationRejectsStaleAndOtherTargets`; `input_bindings_test.go`: `TestInputBindingsRequiredExactProfiles`, `TestInputBindingsUnavailableDoesNotAuthorizeKeys`; desktop `ValidateMailboxBindings`. |
| `unknown_input_never_replays`, `missing_outcome_time_remains_unknown_after_roundtrip` | Pure boundary from `internal/live/channel/input.go` `submitInput`; `input_test.go`: `TestMissingInputOutcomeNeverAuthorizesResend`, `TestProvenZeroInputResumesSameSlot`. Persistent journal/recovery/cadence behavior remains separate. |

This slice covers emitted data and the listed malformed/boundary cases. It does
not claim general Go `encoding/json` compatibility (for example case-insensitive
field-name matching), complete Bootstrap payload decoding, or full P01–P14
acceptance. A byte-compatible record with kind 6 is still only a record.

## Read-only consumer integration and remaining work

The existing `mailbox.rs` reader now uses formal v1 wire/schema, INPUT kind 5,
Confirmation kind 4 and Bootstrap kind 6. Bootstrap selects BootstrapMailbox;
receipts/bodies require the current schema. PreviousMailboxSchema is readable
only for identity/INPUT upgrade evidence. No LYCMEM07 fallback remains in this
reader. Existing `runtime_binding.rs` continues to supply its selected runtime,
layout admission and cumulative budget without an additional scanning route.

`runtime_observation.rs` and native target discovery consume the shared formal
identity decoder. Capacity 500, optional inventory, compatibility and retained
legacy-200 diagnostics are descriptive facts, not input authority. In particular,
inventory zero/missing does not make a structurally valid publication unreadable.

Additional source-to-fixture mapping:

- `tests/mailbox_lookup.rs::schema_only_lua_maps_preserve_exact_missing_keys`
  and `empty_body_is_a_record_but_missing_body_is_not` map the actual synthetic
  Lua-table path to Go `lua_mailbox_map_metadata_test.go` and `records_test.go`.
- `accept_is_never_called_before_path_guard_or_for_missing_token` and
  `retired_record_short_read_rechecks_only_captured_path` cover Go
  `lua_mailbox_publication_test.go`'s path-change ordering and retired-prefix
  recheck. Guards and reads share the existing local and request-wide budgets.
- `bootstrap_uses_separate_namespace_and_confirmation_uses_kind_four` and
  `previous_publication_schema_is_only_identity_or_input_evidence` map the
  named-field branches/schema restrictions in Go `lua_mailbox.go`.
- `runtime_binding::tests::observation` now supplies formal identity bytes through
  its real synthetic Lua reader. It covers legacy-200 activation diagnostics,
  optional/zero/partial inventory, selected local product/build mismatch,
  cancellation/budgets, and reader release on both success and failure.
- `go_mailbox_v1_stale_two_key_upgrade_is_capability_only` maps Go
  `activationInputCapability`: inspect_input retains a structurally valid stale
  sample for capability-only inspection; decode_input still rejects staleness.
  Old three-key INPUT is not promoted to a valid two-key capability.

Not implemented by this slice: complete Go firstReadFailure/path diagnostics,
Bootstrap JSON interpretation, complete journal/transaction/recovery/physical
input, or complete runtime-root/client acceptance. The old native_e2e test still
models the retired four-slot addon. Its new enum arms explicitly return None;
no old fixture was relabelled as a successful formal v1 transaction test.

Remaining replacement sequence:

1. Complete bounded read-failure diagnostics and runtime-root/client parity;
   retain old observation evidence separately from current acceptance.
2. Implement the formal persisted channel state, transaction state machine,
   prepare/challenge/commit/confirm/release, slot capacity, and deadline ownership.
3. Connect native INPUT/physical-key revalidation, intent-before-send and recovery
   to that driver. A stale two-key sample is only activation capability evidence;
   it cannot authorize a key or alone force a manual-activation classification.
4. Replace the temporary native::Services::open legacy-driver block with the
   complete formal driver only after its offline/Windows tests and authorization
   gates exist. Migrate capability reporting and old shared constants with their
   actual consumers; do not rename the old driver into apparent compatibility.
5. Remove superseded four-slot/optical/DTO code only after caller/fixture/package
   audits prove replacement coverage. Preserve original failure evidence.
