# Historical cancel/abandon candidate

Status: implemented staged source, independently reviewed statically; **all 44 new Rust tests NOT_RUN**. No Cargo, Go, native input, game operation, publication or production source write was performed by this task. Rustfmt parsing is not a typecheck. This candidate does not establish full behavioral parity or performance acceptance.

## Frozen authority and scope

Go v3.2.0 commit `3057a9666ab078270b63c7f06de748069b0d1f4b` is the behavioral authority. The `oracle/internal` copies are byte-verified against that git commit. Go 1.27.1 stdlib sources cover RawMessage marshaling, partial JSON decoding, `,string`, time receiver mutation and SimpleFold. The existing pinned `lycheedev_windows::mailbox_input::equal_fold_utf16` implementation/table is reused; only valid JSON-decoded/path Rust strings are encoded as UTF-16. No copied Unicode implementation or native authorization path is introduced.

The composition is HEAD `2ae19cb36431369ab2eaabc493b184f8b14711fe`, then the native-recovery publish snapshot owners, then all 29 readonly R2 owners frozen by `a05d2db91e072591c9916f7c5c9f22c9071cdb39050d74b54f7db1313c38e348` (the machine-readable manifest contains the authoritative complete hash), followed by these 20 owners. Additional tracked test/include/manifests come from that same HEAD. `dependency-base.json` records exact pre-edit bytes. Inherited readonly test state remains NOT_RUN; none is promoted by this batch.

## CLI flags, admission and output

- `live cancel <operation-id>` and `live abandon <operation-or-bootstrap-id>` are mutating commands. Exact described flags are `--home <root>` and `--format text|json|jsonl`; they do not inherit `--project`, `--wait-seconds` or `--no-cache`. Existing parser/help machinery is reused.
- Missing positional: `missing selection file or record ID; use describe`, exit 2, before workspace access. A `CON-` positional follows the existing CON command whitelist and is rejected with the fixed cleanup-is-automatic message before native factory/workspace opening.
- `live abandon BTP-...` alone dispatches bootstrap retirement. `live cancel BTP-...` uses the OP work lookup, as fixed Go does. Other IDs are not reinterpreted as native input or new execution.
- Both commands are registered and have real default formal dispatch. Static registry/inventory extraction finds all 84 definitions; the executable describe assertion is NOT_RUN. The old generic native service facade is not a substitute handler.
- OP response always carries the requested operationId, and carries typed result/context.stage when the returned record has an operation ID. Status is reread after retirement; if it has no outcome, the partially decoded record supplies pending projection. Original retirement and read errors are joined.
- OP abandoned status adds the exact Go warning: `Cleanup abandoned explicitly; available evidence retained; missing report results remain unknown. No game ACK or runtime unload was performed.` BTP returns its typed attempt, including partial/zero attempt on error, without that OP warning or invented stage.
- Error classification inspects all recognized cleanup-tree leaves in fixed-Go priority: resource busy, invalid transition, cancellation, deadline/pending, legacy workspace, unsupported/newer schema, blob sentinels, generation conflict, missing record. The original message and structured cause tree are retained. Existing command fault projection supplies stage, retryability, resumeOperationId and exit status.

## OP authorization and state transitions

Sources: `history_retire.go`, `journal/book.go`, `journal/window_run.go`, `journal/window_owner.go`, `workspace.go`, `vault/access.go`.

1. Validate existing workspace and SQLite schema, then acquire the bounded hashed `operation:<id>` lease. Inspect exact `work/<id>` schema, ID and document/record generation.
2. Only `probe`, `faults`, `reload` intents are supported. Installation precedence is nonempty target.client.directory, otherwise load.installation when load exists, otherwise direct installation. It must be absolute; owner scope is Interface/AddOns under that historical installation.
3. `cleaned` or `abandoned` stage is idempotent: only retire the exact disk claim, with no work-document rewrite. Terminal OP claim equality checks workspace+operation ID, matching Go; the stored optional intent hash is deliberately not an additional terminal requirement.
4. Otherwise acquire admission + exact execution ownership. Existing claim must match workspace/operation; nonempty stored intent hash must match SHA-256 of fixed-Go serialized intent with goal omitted. Check claims and reread resource/intent/generation; reject terminal statuses.
5. Missing claims can be reconstructed only from durable prepared/pending work with original admission.workspaceId and canonical admission parent matching by Go SimpleFold, and after rereading unchanged generation and intent while admission is held. A process-local remembered owner or ID alone is not permission.
6. Cancel is permitted only for prepared + pending + absent or exactly raw `null` observation. It transitions once to cleaned/cancelled. Any recorded/unknown effect requires abandon.
7. Probe/faults abandon accepts prepared, load_requested, loaded, dispatch_requested, reported, flush_requested, persisted, verified, ack_requested, acknowledged. Reload accepts prepared or reload_requested. All three can resume an already abandoning stage. Abandon checkpoints abandoning/running, then abandoned/abandoned; these are intentionally separate durable transitions.
8. Every stage uses exact stage+generation revalidation and CAS. The terminal work update and clearing of `ownership/<resource>` are one SQLite transaction; foreign local ownership prevents the terminal write and retains the abandoning checkpoint. Intent/request/observation evidence is preserved. No execution or ACK is inferred.
9. Explicitly close the execution handle before terminal disk-claim retirement; reacquire admission/execution ownership before deleting the matching marker. Every close failure is retained with the primary error. Unknown reports remain unavailable and complete remains false.

## BTP authorization and state transitions

Sources: `history_bootstrap.go`, `journal/window_owner.go`, `vault/metadata.go`.

- First inspect the historical attempt. A claimed, nonabandoned attempt requires its exact full WindowOwner and execution lease when a disk marker exists. A missing claim is allowed only at prepared; otherwise `live.bootstrap_claim_missing`. Unclaimed attempts have no fabricated claim.
- Reread inside the write boundary. Corrupt schema/ID or decode returns the partially decoded attempt with `live.bootstrap_attempt_corrupt`. Already abandoned is idempotent. Confirmed returns `live.bootstrap_already_confirmed`.
- Read both current-by-resource/action and window-by-resource references. Fixed Go decodes them into the same `bootstrapCurrent`; a null/empty/null-id second object preserves the previous ID. Mismatch or missing references cannot authorize mutation.
- Set phase=abandoned and updatedAt only. Preserve unknown map fields, raw numeric spellings, duplicate-key last value, invalid UTF-8 bytes inside RawMessage strings, and Go json.Marshal HTML/U+2028/U+2029 escaping. Exactly one generation-checked attempt-document CAS; references are not rewritten.
- Close any held driver, then retire the claim only when the write and close succeeded. Full BTP owner equality includes workspace/resource/attempt/intent hash. The hash uses Go's uppercase identity DTO field order ID, Target, Action, Request, Arg. No native transport or business replay is reachable.

## SQL, bounds and cleanup

`HistoricalWriter` is a typed capability over existing SQLite/control/lock mechanisms, not sequential calls to Metadata.put. It validates 1..64 unique mutations, keys <=512 bytes, generations in [0, i64::MAX), raw JSON <=1 MiB. Expected generation 0 is strict creation and respects deleted_generations. Writes use one IMMEDIATE transaction and exact affected-row checks. A stale second row rolls the first row back. COMMIT contention retries retain the transaction; failure/cancellation executes a nonblocking explicit ROLLBACK and preserves rollback/lease/connection-close failures. Schema initialization also uses an explicit transaction rather than replaying an entire BEGIN/COMMIT batch after partial success. SQLite busy waits and lease acquisition remain bounded by OperationControl; no caller deadline is reset.

## Partial JSON and evidence boundaries

Go saves ordinary field type errors while continuing later fields; custom time failures and certain `,string` literal failures stop outer decoding. The retirement-specific typed partial decoder preserves this error-result projection without replacing the readonly parser. Duplicate object/pointer fields merge, raw request/observation bytes survive, history slices reuse backing entries. A failed quoted time resets its receiver to zero; non-string/null preserves the old value. The inner `,string` validator follows the fixed stdlib unquoteBytes rules, including lone surrogate replacement and its accepted escaped apostrophe. Cancellation/deadline always remains a fatal typed stop. These cases are static-reviewed and have new NOT_RUN fixtures.

## Known limits, not passed

- No Rust compile, unit test, differential executable test, Windows filesystem/lock test, fault-injected close test or performance run was performed. Test functions and YAML selectors are present only.
- Inherited readonly generic JSON syntax/type diagnostic wording and corrupt-DEFLATE offset/error text remain non-identical to Go; this retirement decoder also reuses Rust diagnostic text for ordinary JSON errors. State/cause preservation does not make those messages equivalent.
- Inherited shared workspace Value projection still cannot express every deferred Go time.MarshalJSON failure for accepted offsets >=24h. No claim that this batch closes that readonly limitation.
- Unknown business work is never reexecuted, confirmed, ACKed, unloaded or converted to native input. Real game acceptance remains separate and last.
- The staged source tree still requires the existing explicitly owned Emmy reconstruction/vendor/cache preparation for a controlled build. This task does not copy or rebuild that external generated tree.
