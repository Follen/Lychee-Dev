# Titan / Forever native transport regression — 2026-09-28

Scoped interactive evidence on `codex/nonce-memory-transport`, development
version 2.5.1. This is not a release gate or a claim that all client combinations
are supported. No tag or npm publication belongs to this run.

## Observed targets

| Profile | Actual build / interface | Process | Actor |
| --- | --- | --- | --- |
| Titan (时光服) | 3.80.2.69874 / 38002 | WowClassic.exe, PID 41180 | Qingtianjiuz — 时光I |
| Forever | 1.60.1.70009 / 16001 | WowB.exe, PID 48060 | Auto — Forever |

The user's “Classic” window resolves to Titan, not Classic 50504. Installation
metadata, executable identity and fresh addon identity agree. The two processes
use different installation directories and physical slot pools. Forever's
scoped experimental results do not change the formal client acceptance matrix.

Both installations were upgraded through managed delivery from the old QR
addon to the candidate main addon plus 64 LoD slots. Fixed reload and fresh
native binding observed all 64 slots without restarting either game process.
No external wowdump, overlays of repository files, or imported legacy user data
were used.

## Public baseline

`tests/channel-live/public-baseline.mjs` passed 20 CLI calls on each client:
13 consecutive structured probes, automatic capacity reload, hash-linked
journal rotation, historical request retry, syntax-error reporting, cleanup
failure with verified reload recovery, native diagnostic snapshot, and repeated
disconnect. Reports and original envelopes are retained in:

- `.tmp/channel-live/titan-20260928/baseline.json`
- `.tmp/channel-live/forever-20260928/baseline.json`

Both baselines report `complete=true`. Diagnostic provider snapshots were
complete; existing NDui/BugSack and older manual slash-command errors were
retained as evidence, not deleted or attributed to this transport.

## RGB regression found and fixed

Titan's three original reload observations recorded zero patch transitions,
despite successful fresh runtime binding. Instrumentation confirmed the reload
and loading-complete events and their arguments. A native WGC capture showed a
6×6 blue square at physical coordinates `[4,10)`, inside a one-pixel black
surround `[3,11)`. UI scale .71 changed the nominal 8-pixel rendering. The
detector's symmetric center samples could not recognize this even-sized square
with its thin border.

`internal/desktop/reload_patch.go` now validates the full square and surrounding
black ring for bounded 4–12 pixel sizes. The new thin-border regression failed
before the change and passes afterward, including rejection of a broken ring.
Temporal RGB transitions and fresh runtime/actor binding remain separate
requirements; a patch never authorizes input or establishes identity.

Post-fix Titan real reload observed two transitions. Original failures and the
diagnostic capture remain under `.tmp/channel-live/titan-rgb/`; passing reload
evidence is under `.tmp/channel-live/titan-rgb-fixed/evidence/`. Temporary addon
instrumentation was removed, and Titan was restored through managed delivery
to the same candidate addon used by Forever.

The first extended run exposed another scale boundary: Forever rendered a
5×5 square at `[4,9)`. The initial 6-pixel lower bound rejected it; that failure
is retained in `.tmp/channel-live/cross-clients-20260928/report.json` and its
capture in `.tmp/channel-live/forever-rgb/`. The original interrupted observation
was resumed on the same connection to a verified report before disconnecting.
The expanded regression range includes this case; final rerun evidence is kept
in a separate directory rather than overwriting the first failed report.

## Extended baseline

The repeatable runner is `tests/channel-live/cross-client-baseline.mjs`; raw
evidence and its final result are under
`.tmp/channel-live/cross-clients-final-20260928/`. Its `report.json` records
**28 successful test steps, complete=true** (expected pending/unknown exit codes
are asserted as such). The original failed run remains separate.

Its scope is concurrent async execution with actor/build isolation, input release,
cache-off Unicode payload verification, probe timeout, active observation reload
and same-operation continuation, and opaque interruption without automatic
replay. The opaque fixture is read-only code intentionally classified as opaque
to exercise uncertainty without introducing an actual side effect. WGC images
record the top-left activity indicator during execution and cleanup afterward.

Both clients passed. The independent `post-audit.json` checks their persisted
journals: each interrupted observation completed on attempt 2 under the original
operation ID; each opaque operation remained `execution_unknown` on attempt 1
after repeated resume. Both connections closed with `complete=false` for that
unknown business outcome. Each client has two captured reloads with two RGB
transitions apiece. Final WGC images were inspected: no Lychee receiver, activity
indicator or RGB patch remained. Both managed installation checks report 64
managed slots and zero pending slot publications.

Final connection IDs:

- Titan: `CON-c16e86d4b3ef93c7047af6c7fe8c6005`
- Forever: `CON-003cd342d2cd7744651ea2dd9c904a89`

Build/vet, affected desktop/channel tests and the final mandatory Lua 5.1 full
Go suite passed. Full output is
`.tmp/channel-live/full-go-cross-client-final-v2.txt`. Version check remains
2.5.1; Skill contract validation reports 87 commands, 202 references and zero
violations. The new runner passes Node syntax validation; `git diff --check`
passes. These results do not replace CI, publication checks or owner visual
approval of the entire workbench.

Two processes sharing one physical installation, Classic 50504, complete ledger
loss, relogin/process restart, and comprehensive installation crash injection
remain outside this run. The broader outstanding architecture gates remain in
[the implementation record](live-memory-slot-implementation-2026-09-28.md).
