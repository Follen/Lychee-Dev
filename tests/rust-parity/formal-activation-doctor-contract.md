# Formal activation and read-only doctor candidate

Oracle: Go v3.2.0, commit `3057a9666ab078270b63c7f06de748069b0d1f4b`.
This is an implementation candidate, not an acceptance or Windows/game result.
All 32 new Rust tests are **NOT_RUN**. Local Cargo/rustc/compilation was prohibited.
Rustfmt parsed the new files and a separate copy with the integration declarations.

## Integration declarations

The author did not edit any frozen parent module. The parent must apply these
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
2. `close_for_repair` records close before continuing business. Its current
   production adapter verifies the original release's clean main addon, so clean
   older installations can close. **Modified-main cleanup remains blocked** at
   that verifier until the separately reviewed exact-original-unbind
   `RepairPublication` capability is integrated into NativeBackend. No validation
   is bypassed in this candidate.
3. The new tests cover modeled protocol effects and real private project files,
   not native Windows process input, interactive clients or game acceptance.
   Automated Windows own-process fixtures and actual invocation envelopes remain
   required. Rustfmt is parsing/formatting evidence, not type checking.
4. Historical/native reader and lower-level protocol tests still carry their
   own evidence. This batch does not claim whole-tree Go JSON edge-case parity,
   performance, installer acceptance, or the 70% performance target.
