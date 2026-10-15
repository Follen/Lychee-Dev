# Formal activation and read-only doctor candidate, revision 2

Oracle: Go v3.2.0, commit `3057a9666ab078270b63c7f06de748069b0d1f4b`.
This is an implementation candidate, not an acceptance or Windows/game result.
All 40 new Rust tests are **NOT_RUN**. Local Cargo/rustc/compilation was prohibited.
Rustfmt parsed the new files and a separate copy with the integration declarations.

## Integration declarations

The original frozen parent declaration files remain unchanged. Revision 2 adds
only NativeBackend action-aware acquisition, platform dispatch and its test-child declaration, coordinated
with that owner. The parent must apply these
exact declarations before testing/using this candidate. The freeze records a
separate stage with them applied so an independent reviewer can inspect actual
module reachability rather than unattached source files.

- `go_mailbox_v1/mod.rs`: `pub mod connection;`, `pub mod activation;`,
  `pub mod doctor;` (do not duplicate an existing declaration).
- `admission.rs`: `#[path = "activation_store.rs"] pub mod activation_store;`.
- `durable_journal.rs`: `#[path = "activation_history.rs"] pub mod activation_history;`.
- `native_backend.rs`: `#[path = "activation_backend.rs"] mod activation_backend;`.
- `connection.rs`: path attributes naming the sibling files
  `activation_service.rs`, `activation_reload.rs`, `doctor_service.rs`,
  `doctor_repair.rs`, `doctor_delivery.rs`; public modules are
  `activation_service`, `doctor_service`, `doctor_repair`, `doctor_delivery`;
  `activation_reload` need not be public.
- `connection/native.rs`: `#[path = "../activation_native.rs"] mod activation_native;`
  and `#[path = "../doctor_native.rs"] mod doctor_native;`.
- Delivery `installation.rs`: `mod inspect_controlled;` and
  `pub use inspect_controlled::inspect_main_controlled;`.
- Delivery's independent repair batch exports `pub mod formal_repair;` and its
  final `AddonValidator`/`apply` API. That batch is a real dependency of the
  `doctor_delivery` production bridge, not a mock replacement.

The submodule layout deliberately reuses existing private admission, owner,
execution-lock, journal-replay and native helper implementations. It does not
introduce another lock namespace, legacy Transport or an independent command.

## Public application API

Use the same `ConnectionService<NativeEnvironment<...>, ...>` and original
tracked `OperationControl` as the formal connection candidate. CLI calls:

- `connect_with_activation(request, control)` resolves one target, retains an
  existing owner, and either creates the ordinary connection or activation.
- `status_with_activation(id, control)` is read-only; only the exact absent
  channel log selects activation status. Corrupt/torn/invalid state is not new work.
- `resume_with_activation(id, control)` and `disconnect_with_activation(id, control)`
  dispatch the exact original CON. No unknown business or activation input is replayed.
- `activate(request, key, control)` serves the existing `live reload fallback`:
  explicit installation/PID, clean managed deployment, stable key/index, same CON.
- `reload(id, request, control)` searches authenticated current/archive history
  before native open and again under the lease. Completed old reloads are read-only.
- `doctor(request, session, control) -> DoctorResult` is the shared read-only
  implementation. CLI supplies its original default 30-second invocation budget.
- `top_level_live_doctor(request, session, control) -> Option<DoctorResult>`
  returns `None` without an explicit target. Otherwise it uses an original-control
  child capped at 10 seconds. INPUT sampling has a separate 1.5-second cap.
- `doctor_service::validate_doctor_options` rejects fix-only flags without fix
  and session combined with installation/PID, before IO.
- `doctor_fix(report, options, delivery, control)` takes `DoctorFixOptions`
  (`session`, optional `release`, optional `bundled_release`, optional `output`,
  `request`) and a `RepairDelivery`. It preserves the original project/owner,
  closes before replacing files, delegates installation, then activates that PID.
- `doctor_delivery::ManagedDoctorDelivery<V>::new(current_release, validator)`
  is the production bridge for `V: delivery::formal_repair::AddonValidator`.
  It takes formal installation admission, calls the actual Delivery repair
  implementation, then joins the checked-close error. CLI must provide the real
  TOC/XML/Lua closure validator, not a no-op validator.

`ServiceOutcome` retains its result and full `AdmissionError` tree. Doctor's
public checks retain exact state/detail/nextStep semantics; its skipped `errors`
field retains typed read/control/close failures for the caller. A ready INPUT
with `slot_addon_disabled` remains unhealthy and never becomes an execution
receipt or permission to replay. Slot metadata count explicitly does not prove
that 500 addons are enabled/loadable.

## Activation ownership and budgets

`ActivationPhase` models prepared, waiting_manual_reload, input_attempted,
runtime_selected and cancelled; unknown/missing phases are invalid. Only v3 is
accepted. Waiting/cancelled must have zero progress and either no outcome or a
valid not_sent/zero-messages outcome. Unknown/submitted input cannot be cancelled
as unsent. The original durable budget, clock rollback and exhaustion survive
resume; read-only status observes a copy. Waiting for a manual reload is
unbudgeted, and the connection budget starts once at observed activation.

Activation uses the original target metadata and shared window claim before any
native effect. Short admission and try-only execution locks are the existing
ones. The first channel event is exclusively created before runtime_selected is
saved; a crash after handoff resumes ordinary binding on the same CON. A failed
save stops dependent effects. Explicit cancellation is durable before retiring
only the exact old claim and never creates a business log.

Current-schema capability and a valid old-name two-key profile select the
ordinary reload coordinator. A stale old-name profile proves capability only;
the coordinator still requires fresh INPUT before sending. No usable profile
persists manual_reload_required. No Esc, Enter, chat command or guessed key is
implemented. Replacement with incomplete 500-slot discovery requires a client
restart rather than repeated reload. An input_attempted activation observes;
it never resends its unknown historical input.

## Read-only native and deployment boundaries

The production diagnostic uses `NativeReader` directly, revalidates the selected
window before/after reads, supplies no trace path, and owns no publisher/sender.
Bootstrap kind 6 is decoded separately and cross-checked against header runtime,
current schema and release. A failed bootstrap remains visible when the business
mailbox is empty. Previous two-key INPUT is upgrade-only; old 200-slot descriptors
and old three-key profiles do not authorize business or blind reload input.

`installation::inspect_main_controlled` is read-only with bounded receipt,
inventory, tree and byte counts, original-control checks per read/chunk and
checked close for every opened file. It distinguishes absent/unmanaged/managed/
modified and never converts independent IO/control/close failure into a successful
managed/modified assessment. It reuses the existing receipt DTO and core finish
function. Directory-enumeration objects still use their standard RAII destructor;
this does not claim an OS API for checking directory-enumerator close failures.

## Explicit remaining acceptance boundaries

1. Parent module declarations, final CLI routing and the independent Delivery
   repair/validator batch must be integrated and compiled together. Source-level
   generic wiring is not executable evidence.
2. Modified-main close now uses the real `RepairPublication` capability in the
   existing NativeBackend/DiskLease. At publish, verify and route-verify it rereads
   the authenticated original CON, exact target/owner, durable close intent and
   exact current unbind under the original control and still-held execution lease.
   Other publication actions acquire a clean original-release manifest pin
   before the short lease and retain the original clean-main checks. Main-byte verification is bypassed
   only for that cleanup capability; original release/pool/reservation/route, fresh
   INPUT and the same publication lease through the physical burst remain required.
   Already-running operations may still need confirm/release cleanup. On clean
   main bytes these retain the existing path; on modified main bytes those wider
   cleanup actions remain blocked by the ordinary verifier because the reviewed
   capability authorizes only unbind. Full Go repair parity for those states is
   not claimed. This integration is a review candidate, not Windows evidence.
3. The new tests cover modeled protocol effects and real private project files,
   not native Windows process input, interactive clients or game acceptance.
   Automated Windows own-process fixtures and actual invocation envelopes remain
   required. Rustfmt is parsing/formatting evidence, not type checking.
4. Historical/native reader and lower-level protocol tests still carry their
   own evidence. This batch does not claim whole-tree Go JSON edge-case parity,
   performance, installer acceptance, or the 70% performance target.

## Revision 2 independent-review corrections

- Explicit reload first reads the current log without tail recovery. A torn
  current log blocks both an old completed request and an unknown request with
  no log/artifact writes, native open or input. Explicit resume retains its own
  separately reviewed recovery behavior.
- An input_attempted activation that observes no eligible replacement returns
  activation_waiting_runtime, including after budget finalization; its original
  durable deadline and unknown input facts remain unchanged and never replay.
- Activation publishes its window owner only after target, activation and index
  persistence succeeds, as Go's entire persist callback requires. An index write
  error retains orphan records but does not publish ownership or a business log.
- Production modified-main cleanup is covered by real Delivery filesystem and
  publication-lock fixtures connected to NativeCore with a scripted reader and
  sender. Separate real CON fixtures verify durable authority and unknown-business
  close ordering. Native OS input remains NOT_RUN.

## Evidence custody

The original a3139499 freeze and both original stages are retained unchanged.
The original stage did not save the exact slots.rs/storage.rs dependency bytes;
those hashes are unverifiable and are not reconstructed or claimed as reviewed.
Revision 2 saves exact owned sources and every source/manifest in the four-crate
Live/Delivery/Core/Windows dependency closure, plus referenced compile-time test
fixtures. A distinct reachable stage applies the recorded parent declarations.
Both source and stage byte inventories accompany the new freeze; dependent
Delivery and Execute-bytes snapshots are identified and checked before freezing.
This is a review snapshot of that dependency closure, not a standalone whole
workspace distribution, compiler result, CLI/Source-validator acceptance, or
third-party dependency custody claim.

## Required next repair-capability extension (not waived)

Fixed Go `channel/driver.go:312-320` cancels an unissued prepared operation and
waits instead of committing a prepared request. `driver.go:173-178` observes an
existing prepare/commit (or ordinary bind) while Closing, without republishing
or waking it. These exclusions must remain intact when the capability expands.

The still-blocked modified-main cleanup states are precise:

- `driver.go:386-417`, Rust `driver.rs` Running/ConfirmReady: the exact original
  HEAD/BODY result has passed result validation and `report_candidate` is durable;
  ConfirmReady creates/persists an exact `confirm` envelope using that operation's
  ticket and current original runtime/owner/fence/actor. Only its exact reported
  receipt with matching prepared nonce, byte count and checksum advances to
  ResultVerified. A future repair capability must authenticate that persisted
  state/envelope and preserve the normal unknown-input observation rules.
- `driver.go:418-457`, Rust ResultVerified/ReleaseReady: the original result
  artifact is written before `result_durable`; ReleaseReady persists the exact
  `release` envelope with the same ticket and report bytes/checksum. The loaded
  original journal must authenticate its referenced result artifacts. Only the
  exact released receipt or the existing separately verified runtime-destruction
  recovery may finish cleanup. Missing/unknown release input is never replayed.

Those actions currently retain ordinary clean-main publication/verification and
are covered by a regression proving clean confirm/release still works while a
modified main blocks. Extending the immutable Delivery capability and its Live
persisted-state authenticator is required for full fixed-Go modified-main repair,
with a separate independent review. Do not permit prepare/commit, arbitrary
payloads, new requests/tickets, or relaxed INPUT/route/lease/close checks.
