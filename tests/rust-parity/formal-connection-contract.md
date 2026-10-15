# Formal CON application service candidate

Oracle: Go v3.2.0, commit `3057a9666ab078270b63c7f06de748069b0d1f4b`.
This batch does not switch the CLI, register commands, implement activation or
repair/doctor, or claim Windows execution. All Rust tests below are NOT_RUN.

## Integration contract

Export `pub mod connection;` from `go_mailbox_v1/mod.rs` (parent-owned integration).
The r2 snapshot includes an independently staged copy of that file with only
this one export added. Its source/base and staged hashes are in the freeze.
The shared worktree's module declarations are untouched; parent applies this
exact export before compiling or counting the new tests.
The production constructor is:

```rust
let environment = NativeEnvironment::new(project, release, WindowsProcessLifetime)?;
let mut service = ConnectionService::open(
    project, environment, SystemClock, SystemTokens, SystemScheduler::default(),
)?;
```

Use the original invocation's tracked `OperationControl`, with a child for the
user's existing `--wait-seconds` (default 120). Do not replace it with a fresh
uncancelable budget. Persisted goal deadlines are handled by Driver and are
never reset by a new invocation. Existing files/probes resolve to exact source
bytes in the public CLI layer; no new request syntax is introduced here.

- `connect(&TargetRequest, control)` takes optional installation/PID and a
  `TargetFilter` for character/realm/build/product. Existing selected claims
  resume their original CON. New discovery requires a clean current main addon,
  matching runtime product/build, all 500 slots, and formal INPUT capability.
  Missing/old runtime activation remains a separate unfinished route.
- `resume(id, control)` repairs only an authenticated torn final record, restores
  under the exact claim/execution lease, and continues its original durable goal.
- `status(id, control)` is read-only. It never repairs, opens a game, retires a
  claim, or updates a deadline.
- `execute(id, ExecuteRequest { request, code, budget_seconds, policy }, control)`
  supports the existing 1–120-second budget and typed `Opaque`/`Observation`
  policy. It validates exact retained identity before native open and again under
  the lease. Completed old requests return their historical projection without
  replacing current work. Unknown input/request state is never replayed.
- `report(id, request, control)` is a library-only history lookup for consumers.
  It is not an invented `live report` command. Candidate/unconfirmed bytes do
  not appear as verified reports.
- `disconnect(id, control)` records close intent before continuing. The independent
  ended-process path rechecks original PID/creation under the lease, saves the
  observation, retires only saved exact envelopes, saves closure, closes the
  lease, and retires only the original claim. A live replacement HWND is never
  selected. Closed unknown business work remains incomplete.

`ServiceOutcome` preserves `result: Option<ProjectResult>` alongside
`error: Option<AdmissionError>`. Never discard the result just because an error
exists: verified report, business `report.ok`, cleanup, connection closure, and
transport error are separate facts. Serialize the typed ProjectResult directly:
`RawReport` retains duplicate keys and number lexemes; converting through an
ordinary Value may lose duplicate fields. The outer public encoder still owns
fixed-Go whitespace/HTML escaping and overall result-envelope contracts.

`classify_error` returns an optional `PublicFailure` with original full message,
code, signed exit code, and structured details. Fixed-Go trace-flush failure (5)
precedes cancellation (7), which precedes pending (6); explicit business codes
retain fixed-Go argument/capability/protocol/external classification. Other
Toolkit leaves preserve their original code/exit/details. `None` leaves legacy
non-Toolkit OS/claim errors to the host's existing classification. Keep the full
AdmissionError tree for joined close errors. Public CLI projection tests still
need to run against the final dispatch integration; library serialization tests
are not that evidence.

## Resource boundaries and remaining work

All service-owned native backends, execution leases, ended-process publication
leases, and the returned main-addon receipt handle receive explicit consuming
checked-close attempts, including business/error exits. Independent close
failures are joined. Drop remains the unwind fallback, not successful-close
proof. This revision depends on the separate resource-cleanup candidate for checked
`storage::locked_sha256` construction and release inventory handles; its exact
files/receipt are frozen dependencies and its Windows tests remain NOT_RUN.
Publication's private `verify_addon` still has Drop-only receipt error exits
and retains a successful receipt handle across the burst. Repair/modified-addon
cleanup needs a separate narrow publication policy; this batch does not claim
those lower-level gaps are fixed.

The helper `installation::verify_main_addon_controlled` uses core's consuming
`storage::finish_file`, with the original control. It never scans shared slots.
Publication's private receipt verification still
retains its own handle through the input burst; it must not be replaced with the
new early-close helper. Both use the same bounded inventory verifier.

Activation status/resume, explicit reload API/history lookup, repair/doctor and
full public CLI dispatch remain separate integration work. Native factory,
process-lifetime adapter, own-process tests, cross-process history fixture, and
all existing backend regression tests require automated Windows execution.
No gameplay, user desktop, publish, Cargo or rustc operation was performed.

## Independent review revision 2

The initial review found one P1 and four P2 issues. This candidate changes the
retirement probe to an ordered child, routes a legacy unfinished unbind resume
through Disconnect's original-process path, retains structured control-plus-close
causes, repairs the torn-tail fixture's pre-corruption path capture, and uses
the core selection error projection without rescanning the observed report.
The native regression traverses real Driver continuation into real NativeCore
retirement, including both nested ordered-child boundaries. The journal/OS
source in those tests remains modeled; it is not a game acceptance claim.

`ChannelError::Toolkit` preserves complete original details and recognized
`lycheedev.file-cleanup.v1` causes. Cause expansion is bounded at 32 levels;
malformed/opaque detail trees remain non-pure failures. A close/IO cause cannot
be swallowed by optional-observation timeout handling.

An additional invalid test envelope (zero fence/empty ticket) was corrected
without weakening its retirement assertions. All revised Rust and Windows
tests remain NOT_RUN until the controlled Windows run.

The main-addon helper's actual public signature is
`verify_main_addon_controlled(path: &Path, release: &str, control: &OperationControl) -> Result<VerifiedInstallation>`;
there is no `with_close` API or caller-supplied close callback. Native deployment
maps only explicit detail-free non-clean/invalid-payload states to
`live.channel_clean_current_addon_required`. Raw IO, malformed-receipt, control,
and structured cleanup failures retain their original typed causes and details.
Full fixed-Go modified-state equivalence (including a missing inventory resource)
awaits the separately authored controlled inspection helper. This intermediate
verifier must not be described as complete inspection parity.

Both native deployment and original-process publication retirement derive the
fixed-Go `Interface/AddOns/Lychee Dev` directory through one pure saved-target
projection. The regression compares it with core's actual addon destination on
an owned client tree. This corrects the initial candidate's missing-space path;
it does not rediscover a replaced client during original-process retirement.
