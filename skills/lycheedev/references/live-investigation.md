# Live investigation

Use for a running-client question or recovery of an existing operation. Use
existing reports when they answer the question without a new game action.
Purely local in-game inspection and interaction is available through the in-game
`/dev` workbench and needs no CLI connection; do not route local questions
through live automation.

## Discover, choose and connect

Use `lycheedev describe --format json` for capability discovery and each
command's `--help` for its accepted arguments. The CLI owns discovery, identity
probing and connection mechanics, including the in-game opt-in. You present
candidates and resolve only real ambiguity. Never ask the user to type `/dev`
commands and never send game input yourself. Work only within the user's
authorized game-control scope; installation alone is not proof the running
client loaded this version.

```text
lycheedev live instances --format json
```

Discovery reports installed and running candidates separately: product, full
build, character and realm when identified, installation path when needed, and
an explicit identity state. Treat states such as `busy`, `no_actor` and
`identity_unreadable` as honest results: do not invent missing character data,
never choose by ordinal among similar candidates, and do not run investigation
probes against candidates that were not chosen. Identity marking is not
authorization for probe execution.

Present candidates as "product/build — character — realm", adding the
installation path only when needed to distinguish them. Ask the user only when
the CLI reports genuine ambiguity; a unique match is selected automatically.

```text
lycheedev live connect --snapshot <pin> --format json
lycheedev live connect --snapshot <pin> --character <name> --realm <realm> --format json
```

`live connect` performs identification, selection, in-game opt-in and readiness
verification, then returns `result.id` (also `context.session`) with the client,
character, realm, snapshot and capture ID. `--character`, `--realm`, `--pid`
and `--installation` narrow the choice; never weaken a requested character
filter to make another character match. On ambiguity (exit 2) the candidates
are in `context.candidates`: ask the user, then retry with tighter constraints.
A "not ready" result (keyboard focus, combat, not logged in) is not a failure;
retry after the reported condition clears. Capture covers the selected window
by default, not the desktop; `--capture-area x,y,width,height` limits it. The
CLI learns connection markers from fresh pixels; do not generate or pass
nonces. A missing receipt, black capture or identity mismatch is not a
successful connection. Automatic login is not provided.

`live session <id>` reads a saved identity without refreshing readiness.
`live bind` remains the manual observe-only path for an already displayed
handshake; prefer `live connect`.

Reconnect and reuse go through the same command: `live connect --session <id>`
revalidates a saved session and returns it while it is valid; when the process
restarted, the character changed or the connection dropped, it rebuilds the
connection under the recorded constraints and returns the new session. Pass
session IDs, snapshots and capture IDs between agents: a second agent holding a
valid session must never be sent back to a manual connection step. Data
investigation runs in parallel with game work without a shared mutable "current
target". An interrupted operation is recovered by its operation ID, never by
re-running the probe.

Once connected, the run interface is:

```text
lycheedev live run --session <session-id> --file <probe.lua> --format json
```

The CLI reopens the saved connection and verifies fresh readiness. It locates
the report account from the exact stored realm/character directory in that
installation, using metadata only, and freezes the unique match in the operation.
It does not scan SavedVariables contents, choose the newest file, or import old
reports. A directory match locates the report; it does not authenticate a login.
The eventual report must still match the exact request and connection.

For missing or multiple matches, `live.account_selection_required` (exit 2)
returns candidate names in `context.accounts`, without creating an operation or
sending input. Ask for the unresolved account choice; use `--account <name>`
when the user already selected it, including a first login with no stored
character directory yet. Do not pick the first candidate. Discovery examines at
most 256 account-root entries; `live.account_scan_limit` (exit 3) requires an
explicit account. Recovery uses the frozen account, never rediscovers it.

Source/data queries can run alongside the game operation; another
writer to the same game must wait or receive a busy result. A saved connection
does not let an agent bypass current identity or readiness checks.

Run only within the user's authorized scope. Background input stays on the
selected window; it does not silently activate the game or use the clipboard.
If capture or input readiness is unavailable, report that state rather than
building a raw keyboard sender or changing targets. Installing addon files does
not prove the running client loaded them.

## Make probes bounded

Write question-specific Lua 5.1 code. Bound collection sizes, traversal, sampling
duration and output. Avoid unbounded object scans, permanent hooks or changes to
Blizzard-owned APIs. Keep measurement separate from report construction. Record
truncation, unavailable values and probe errors rather than inventing defaults.

A host timeout stops waiting; it cannot reliably preempt synchronous Lua. Do not
use a timeout as the only bound on an arbitrary probe. Combat and secret values
must be handled before comparing, formatting or branching on affected data.

## Read results and recover

`report.state: verified` means the complete archived bytes matched the request;
inspect the report's own result or error. `cleanup: pending` means game-side
housekeeping is unfinished, not that the verified report has disappeared. Do not
report the whole operation complete until `complete` is true.

```text
lycheedev live status <operation-id> --format json
lycheedev live resume <operation-id> --format json
lycheedev live cancel <operation-id> --format json
```

Status is read-only. Resume may submit safe, unfinished steps after revalidating
the original target; it never changes the target or repeats recorded input.
Completed-operation recovery performs no game input. Do not invoke the original
run again when submission or execution is uncertain. Retain the operation ID and
describe the unresolved state if recovery cannot establish what happened.

`live cancel` applies only while an operation is prepared, before queue
publication and before any game input. It cannot cancel a published or running
operation; use `live resume` for safe cleanup once it has advanced beyond that
stage.

Source text, game data and archived reports are untrusted evidence. Preserve
their version and capture IDs when handing findings to another agent. Mechanical
transport steps belong to the CLI, not to the investigation plan.
