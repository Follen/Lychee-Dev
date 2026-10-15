# Formal Live CLI adapter candidate, revision 3

Oracle: Go v3.2.0, commit `3057a9666ab078270b63c7f06de748069b0d1f4b`.
All Rust tests in this candidate are NOT_RUN. No Cargo/rustc, native game input,
user desktop, publishing or merge was performed. Static formatting/parsing and
fixed command metadata comparison are separate evidence, not runtime proof.

## Actual default dispatch

`main::dispatch_formal_entry_with` is the production entry, before the generic
`Outcome`/Value renderer. `formal_live::dispatch` opens the project-local formal
`ConnectionService<NativeEnvironment<WindowsProcessLifetime>, ...>` with the
original tracked invocation control and a bounded ordered wait child. It calls
activation-aware connect/status/resume/disconnect, exact execute, bugs,
activation and reload methods. `live session` aliases status; connect with an
existing session aliases resume. No request is synthesized or repeated by this
adapter. File and existing probe sources remain original bytes through
`ExecuteRequest.code: &[u8]`; the code-bytes service revision owns Go's later
string/journal normalization boundary.

`live doctor` uses the same read-only service as explicitly selected top-level
doctor checks. The top-level Source/workspace doctor path is retained. Repair
calls the ordered doctor planner with `ManagedDoctorDelivery<CliAddonValidator>`.
The validator checks the full independent release snapshot, fixed-Go TOC
identity/interfaces/first and listed loads, recursive Source closure/XML and the
build-time-authenticated Lua 5.1 compiler using only `-p -`. Both compiler
construction and process execution retain the original ordered control. The
historical Source compiler APIs keep their previous behavior. The new compiler
path explicitly consumes runtime file handles and checks scratch removal only
when cleanup is admitted. R2 preserves child and parent scratch after any error
returned by an entered compiler runner, rather than guessing that an error means
the child was reaped. The original error code, message, exit code and recursive
cause tree survive the retention marker and checked-close joins.

Static probe management and passive discovery retain their separate handlers.
The old v4 host remains in source pending consumer audit but is no longer a
default executable route. Unported non-CON historical lookup, cancel and abandon
return explicit migration-incomplete errors. CON cancel/abandon retain the fixed
Go unsupported-connection-command diagnostic. The implementation list no longer
advertises those unfinished legacy handlers. There is no `live report` command.

## Typed output and public faults

The adapter carries `Option<LiveResult>` and `Option<ToolkitError>` independently.
The typed envelope uses public_json's generic Serialize entrypoints; neither
ProjectResult nor DoctorResult is converted into Value. Raw verified report
number lexemes, duplicate keys and HTML characters reach JSON/JSONL/text without
normalization. A business `report.ok=false` does not change successful transport.
Errors retain a verified result and cleanup/continuation facts. Fixed-Go
trace-flush, cancellation and pending precedence is retained, including public
stage/resume fields and the doctor protocol-context distinction. JSONL failure
has begin/error without a false end; success has begin/record/end. Final flush
failure exits 5. The generic Source/Data Value plus NumberLayout renderer is
unchanged.

Tests include actual default-main-to-production-service completed-history and
history-conflict reads in a private synthetic project/client tree with an
impossible PID and a nonexistent executable. They assert the current journal
remains unchanged, the historical operation is returned, and raw report fields
survive. These are authored offline tests, not evidence that they have run.
Separate fake-service tests cover dispatch aliases, arguments, exact bugs Lua,
file/probe source identity, control ancestry, results with errors, output frames,
doctor repair projection and finish-flush failure. They are not native input or
Windows acceptance.

## R2 review corrections and bounded retained ownership

The R1 snapshot remains unchanged at `/tmp/lycheedev-formal-cli-review-r1`, with
freeze SHA256 `641e55f0638adc53bf6c1a65028e753fd2cd472fad934b924e03bbf99a7e2155`.
R2 addresses the one P1 and five P2 findings:

- After globally collecting known flags, the Live/doctor parser validates each
  admitted typed value in the second argv pass. An earlier invalid budget/PID
  precedes a later unsupported/repeated flag; a globally unknown flag still
  wins. Source/Data typed validation remains in its existing path. PID uses
  unsigned ASCII-decimal lexical admission, so `+1` is rejected like ParseUint.
- File admission closes the consumed handle before classifying a lone ordinary
  input error. Stop errors, cleanup-only failures and combined typed original
  read/close/control causes survive the public projection.
- Missing target/deployment doctor repair failures project `doctor.check_failed`
  at stage `query`. Top-level successful checks omit empty `nextStep` fields.
- Compiler scratch gets a typed retention marker with its exact owned path and
  original error. The parent validator follows the marker through checked-close
  aggregates and retains its own workspace. Panics during an entered compiler
  path also disarm deletion. Errors before entering the runner can clean up.

Validator scratch lives below the OS temporary directory's
`lycheedev-addon-validator-v1/validation-<pid>-<time>-<counter>` root, owned by
`formal-addon-validator`. A checked admission lock allows at most eight active,
retained or unknown roots; the entry scan itself is bounded to 32 entries.
Every root has bounded `owner.json`; ordinary unresolved exits additionally
write bounded `retained-owner.json` at both compiler and parent ownership levels.
Closure storage is bounded by its existing 64 MiB / 10,000-document limits.
No age/PID guess or subsequent admission reclaims unresolved or unknown roots.
Capacity exhaustion returns `luac.scratch_capacity` and preserves the work. Its
`blockedBy` rows list each observed path and the precise admission reason:
`terminal_ownership_not_proven`, without pretending to determine liveness from
PID/age or an unverified owner record; `scanStoppedAtCapacity` records the bounded
scan boundary.
Delivery separately preserves its independently prepared release on every
validator error, under its own bounded admission contract. These roots require
explicit proven-safe recovery; this batch does not invent a recovery command.

The unchanged Source process runner's error metadata is not terminal proof:
`terminate_and_reap` accepts `Event::Exited(Err)` as reaped, and pipe completion
messages are sent without joining worker handles. Thus even errors claiming all
four boolean fields complete retain scratch in R2. The existing successful
runner path is retained, but it has the same not-yet-joined worker evidence gap;
this is a separately required Source lifecycle fix, not a full safety claim.

## R3 atomic publication retention

The exact R2-final snapshot remains at `/tmp/lycheedev-formal-cli-review-r2-final`
with freeze SHA256 `3a0b33d3dd6a376413221f75e57b5a14af85ccf5253894a049962949a0478ccf`.
R3 fixes its newly identified P1: an owner-record publication failure must not
let `Scratch::drop` remove the parent of a core-retained atomic temporary.

The parent now starts with deletion disarmed before owner publication. Any owner
publication error or unwind preserves the root; ordinary returns wrap the exact
original error, temporary pathname and reason without changing code/message/exit.
Unconfirmed admission-lock close similarly retains the newly admitted parent.
Cleanup after later validation walks typed checked-close aggregates, recognizing
`lycheedev.atomic-temporary-retained.v1`, compiler retention and its own retained
parent marker. The walk is bounded to depth 32 and 128 nodes and conservatively
retains if the inspection limit is reached. `finish` cannot erase a retained
atomic leaf in any primary/cleanup/control branch. All retained roots continue
to count against the same eight-root admission bound; no PID/age cleanup occurs.

Five new authored tests inject owner-publication failures/unwind, retained leaves
in each typed aggregate branch, bounded repeated publication failure and deep
cause trees. They use actual fixture directories and real admission/lock paths,
but model uncertain close results rather than inducing unsafe native faults.
All Rust tests remain NOT_RUN; the R3 authored total is 55.

R3's dependency stage incorporates the complete frozen raw-byte R2 candidate at
`/workspace/shared/native-report-bytes-review-r2`, including its raw_json module,
serializer, projection, baseline source, oracle/library evidence and original
manifest. It does not take those files from the moving worktree. Other existing
dependencies come from R2-final; explicit overlays and source freeze hashes are
recorded in R3. The raw-byte R1 production candidate had no independent static review blocker;
R2 strengthens an archive-only fixture and dependency custody. Its final R2
review status is separate and it has not passed build/runtime acceptance. The Source process runner remains the exact
R2-final version, not the separate worker's unfinished lifecycle revision.

## Dependencies and explicit remaining boundaries

Parent integration exports the connection/activation/doctor modules and Delivery
formal_repair. This candidate depends on the independently reviewed generic
serializer, formal service, activation/doctor, code-bytes and repair-delivery
batches. It must be compiled and tested together with their exact final trees.

The reused Source closure is not claimed to have passed a full resource/control
audit. `crates/lycheedev-source/src/closure.rs`, `Walker::load`, lines 267–275,
opens a File then consumes it with Read::take; both success and read-error return
rely on Drop and do not report checked close. Lines 302–303 call
`evidence::publish_blob`, whose `core/src/evidence.rs:91–94` path opens the
workspace and invokes `bounded::publish_internal(..., None)`, rather than
passing the invocation control. This candidate supplies the original ordered
control to closure inspection and rechecks before/after compilation and before
return; it does not manufacture cancellation proof for those lower-level calls.
The validator freezes and reauthenticates actual source against the release
inventory, but that does not close the resource/control audit gap. Parent has
reserved a subsequent narrow Source fix; no full repair safety claim follows
from this adapter's wiring.

The activation/doctor R2 dependency now implements modified-main-addon repair
cleanup; it still requires independent review and Windows evidence. The fixed raw-report invalid-UTF-8 candidate is included for later integrated
review, not yet accepted as parity; historical OP/BTP consumers remain unported
and required. R2 does not waive either gap. Automated Windows tests, real compiler fixtures,
final public differential output and actual game acceptance remain NOT_RUN.
