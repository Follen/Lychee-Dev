# Fixed-release Mailbox v1 contract adapter

Oracle: Go v3.2.0, peeled commit
`3057a9666ab078270b63c7f06de748069b0d1f4b`.
All Go paths below are relative to that tree. The repository `handover.md`
remains the sole migration decision/status/evidence source.

This is the first decoder/data-boundary slice of the existing Live migration.
It does not create a second protocol, publish slots, open processes, send input,
operate a game, or make the default Live route usable. The old Rust route is
still present until its individual consumers are replaced and verified.

## Source disposition

| Existing source / fixed Go source | Disposition in this slice |
| --- | --- |
| Rust `mailbox::Memory` safe interface / Go `memory.Source.Read` | Retain the reader abstraction for injected synthetic memory and the future native adapter. No FFI change. |
| Rust `mailbox.rs` wire magic `LYCMEM07`/`LYCEND07`, kinds 1–5 | Supply the fixed-release replacement in `wire.rs`: `LYCMEM06`/`LYCEND06`, kinds 1–6, 80-byte header, 40-byte trailer, Adler32, 512 KiB payload cap. Do not mix the two byte formats. |
| Go `internal/bridge/memory_record.go` and `internal/live/memory/records.go` | Implement fixed-header qualification before bounded second read/allocation; reject torn headers; qualify BODY with exact nonzero tokens plus HEAD length/checksum, including empty BODY. Return borrowed payloads when decoding existing bytes. |
| Go `internal/live/channel/protocol.go` | Implement identity and receipt field/header checks, exact envelope correlation and distinct rejected receipts. Keep known 200-slot descriptors separate as manual-activation evidence. Do not relabel old schemas or grant business input. |
| Go `internal/live/channel/input.go`, `input_bindings.go`, `input_attempt.go` | Implement pure INPUT schema/readiness/profile/target/time checks; strict optional diagnostic whitelist and raw 512-byte bound; only known zero-message `not_sent` can be considered for re-preparation. These are necessary predicates, never input authority. |
| Go `internal/live/memory/lua_mailbox.go` and map metadata test | Include a logical exact-token public-map view and fixture; schema metadata is never a record key. Actual Lua hash-table traversal, path guards and read-failure diagnostics are **not** implemented by that view. |
| Rust default `native.rs` locator / `runtime_observation.rs` | Unchanged: locator-not-finalized and ObservationOnly limitations remain. |

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
both wire and journal inputs. Only decode_input imposes the outer 2048-byte
wire bound. A missing persisted
input outcome remains unknown, even with a `received` diagnostic or fresh INPUT.

## Fixed Go fixture mapping

Rust tests use the `go_mailbox_v1_` prefix so Windows CI can select only this
noninteractive contract slice. They are not whole-driver or real-client tests.

| Rust tests (suffix after the prefix) | Fixed Go source/test mapping |
| --- | --- |
| `wire_exact_empty_body_golden`, `wire_all_kinds_and_payload_bounds`, `wire_rejects_corruption_and_old_rust_protocol`, `old_wire_names_rejected_even_with_valid_header_checksum`, `fixed_go_binary_record_vector` | `internal/bridge/memory_record_test.go`: `TestMemoryRecordIntegrityAndBounds`, `TestMemoryRecordRejectsOldWireVersions`; Bootstrap kind 6 from `memory_record.go`. The binary golden uses the exact Go test's UTF-8/NUL payload and tokens, independently encoded with Python struct/zlib; it is not claimed as a fresh Go test execution. |
| `token_exact_lowercase`, `identity_schema_and_capacity`, `identity_header_correlation` | `internal/live/channel/protocol_schema_test.go`: `TestIdentityAndReceiptOnlyCurrentSchema`, `TestIdentityPublicationRejectsMalformedChecksummedHeaders`; `tokenBytes` in `protocol.go`. |
| `legacy_200_is_activation_only`, `old_500_capability_stays_labelled` | Same file: `TestLegacyIdentityOnlySelectsKnownUpgradeCapabilities`, `TestPublishedLegacyIdentityRequiresManualActivation`. |
| `receipt_exact_match_and_rejection`, `confirmation_and_bind_zero_ticket` | `decodePublicationReceipt` and `decodeReceipt` in `protocol.go`; current-schema receipt case in `protocol_schema_test.go`. |
| `synthetic_memory_empty_body_and_allocation_gate`, `synthetic_memory_torn_read_and_report_bounds`, `body_static_header_table_before_payload_read` | `internal/live/memory/records_test.go`: `TestBodyRequiresExactHeadAuthorization`, `TestBodyStaticHeaderRejectedBeforePayloadRead`; `ReadRecord` second-read/header check in `records.go`. |
| `schema_only_public_map_missing_is_not_empty_record` | Logical subset of `internal/live/memory/lua_mailbox_map_metadata_test.go`: `TestMailboxSchemaOnlyMapsKeepMissingRecordsComplete`. Native layout, read accounting and guard coverage remain pending. |
| `disabled_diagnostic_never_grants_receipt_or_replay`, `optional_diagnostic_is_strict_and_non_authoritative`, `diagnostic_dto_roundtrip_preserves_evidence_not_authority`, `dto_optional_fields_and_raw_bounds_match_go` | `internal/live/channel/input_attempt_test.go`: `TestDisabledSlotDiagnosticPreservesInputEligibility`, `TestInputDiagnosticOptionalPreservesEligibilityAndErrors`, `TestInputDiagnosticRequiresReceivedBoolean`. Journal/probe portions of that Go suite are not covered by these pure predicates. |
| `input_readiness_and_freshness_are_separate_gates`, `two_key_binding_contract` | `internal/live/channel/input_test.go`: `TestInputObservationRejectsStaleAndOtherTargets`; `input_bindings_test.go`: `TestInputBindingsRequiredExactProfiles`, `TestInputBindingsUnavailableDoesNotAuthorizeKeys`; desktop `ValidateMailboxBindings`. |
| `unknown_input_never_replays`, `missing_outcome_time_remains_unknown_after_roundtrip` | Pure boundary from `internal/live/channel/input.go` `submitInput`; `input_test.go`: `TestMissingInputOutcomeNeverAuthorizesResend`, `TestProvenZeroInputResumesSameSlot`. Persistent journal/recovery/cadence behavior remains separate. |

This slice covers emitted data and the listed malformed/boundary cases. It does
not claim general Go `encoding/json` compatibility (for example case-insensitive
field-name matching), complete Bootstrap payload decoding, or full P01–P14
acceptance. A byte-compatible record with kind 6 is still only a record.

## Consumer replacement order

1. Adapt the existing `mailbox.rs` named Lua lookup to these fixed-release
   record/header/selector types while preserving the shared `Memory` budget.
   Add actual schema-only Lua-table fixtures, BootstrapMailbox selection, public
   map absence, path replacement, guard failure and first-read-failure coverage.
   Run Go Selector.Accept-equivalent caller predicates only after path guard
   rechecks; a wire checksum alone is not a stable publication.
   The old lookup currently checks `crate::MAILBOX_SCHEMA` and maps INPUT to
   kind 4; both need explicit fixed-Go replacement, not global constant edits.
2. Route `runtime_binding.rs` and `runtime_observation.rs` read-only consumers
   through the migrated named lookup and `decode_identity`. Preserve selected
   module/runtime identity, bounds, guard rechecks and `ObservationOnly`.
3. Migrate `native.rs`, host/lifecycle/journal/input consumers together with
   complete fixed-Go prepare/challenge/commit/confirm/release and unknown-input
   fixtures. Connect fresh INPUT and physical-input checks only after their
   separate authorization/intent gates exist. Never infer permission from this
   module's successful parsing or from any observation DTO.
   Go activationInputCapability also retains structurally valid stale two-key
   INPUT for capability-only inspection. The current Result-only decode_input
   returns a stale error without that sample; extend the adapter before wiring
   this activation path. Staleness must neither authorize keys nor by itself
   force a manual-activation classification.
4. Remove superseded wire/DTO helpers only after a repository-wide caller and
   fixture audit proves all consumers have moved; retain historical evidence.

The public export makes the contract available to those planned consumers. It
is not a claim that they have already been connected or that Live is complete.
