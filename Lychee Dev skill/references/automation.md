# Automation: task delivery, reload and receipt

This reference covers the automated loop between an Agent, the Python tooling
in `scripts/`, and the in-game Automation page (`/dev` > Automation). The full
design lives in the addon repository at
`add-on/docs/design/AutomationOpticalTransport.md`; this page is the practical
contract and describes the CLI exactly as implemented.

## The loop in one paragraph

You (the Agent) prepare a bounded Lua probe and deliver it as one task block in
the installed `Modules/Automation/auto/auto.lua`. The block only registers
data; it never runs at load. After an input-side `/reload`, the command
`/dev auto run <task-id>` executes the block. While it runs, three RGB status
blocks show at the top-left of the screen. When the result is committed to the
in-memory SavedVariables database, they are replaced by one static QR code at
the top-left that carries only the identity:
`{"v":1,"ticket":"...","task":"...","run":"...","ts":...}`. `capture`, `run`
and `bugs` keep one WGC capture session open and poll it until that notice is
decoded or `--timeout` expires. The tooling then cross-checks the notice `task`
and `run` against the request it actually delivered, logs a `reload_requested`
intent, sends one `/reload` so the game writes the database to disk, then reads
the exact Ticket out of the SavedVariables file, verifies its identity,
payload and checksum, and stores the complete report as local artifacts.
`/dev auto bug <n>` skips the task file and the input-side reload; it snapshots
recent error records and follows the same notice -> reload -> read path.

## Hard rules

- One execution per client at a time: while a request is running the addon
  answers new requests with a busy message. Finished results that you have not
  acknowledged may queue up to three; only the cap refuses new work, and a
  reload resets the count.
- Always acknowledge what you read. The addon cannot see whether a result was
  picked up, so run `ack --status received` (or `failed`) after reading one.
  Without it the result keeps occupying a backlog slot until a reload.
- The same `requestId` never executes twice in one session; re-running the
  command re-shows the same notice instead.
- Results awaiting their reload are protected: normal export pruning, record
  deletion and cache clears skip them. `/dev auto stop` only hides the notice
  UI; it never cancels a run and never deletes pending results.
- One notice triggers at most one reload. The host log deduplicates on
  `(installation, requestId, ticket)`; after a script restart, check the log
  and the SavedVariables file before repeating anything.
- A decoded notice is accepted only when its `task` (and `run`, when known)
  matches the command that was just delivered. A stale QR left on screen from
  an earlier run is logged as `notice_identity_mismatch` and ignored.
- Host wait timeouts mean "unknown", not "failed": never auto-rerun a task
  because its notice was missed. Inspect first (see Recovery).

## Task blocks

Write task blocks only through the CLI so locking, escaping and limits stay
correct:

```text
python scripts/automation.py task upsert --install-dir <addon dir> \
  --id inspect-example --source-file probe.lua \
  --request-id req-20260912-000001-aaaaaa --revision rev-1
python scripts/automation.py task list --install-dir <addon dir>
python scripts/automation.py task remove --install-dir <addon dir> --id inspect-example
```

- Task IDs are `[A-Za-z0-9_-]{1,64}`. Each Agent owns its own task IDs;
  never remove or rewrite another task's block.
- A new execution needs a new `requestId`. Updating the source always means a
  new `requestId` and a new `revision`.
- The writer verifies `--expect-revision` on replacement, holds an exclusive
  lock file (`auto.lua.lock` in the installation directory), preserves
  everything outside the `BEGIN`/`END LYCHEE DEV TASK` markers verbatim, and
  replaces the file atomically. It refuses more than 16 blocks, more than
  1 MiB of registry, or a source over 256 KiB.
- The source file is read as raw bytes and must be valid UTF-8, but it does not
  have to be ASCII: the generated Lua shell stays ASCII and escapes every
  non-ASCII byte as `\ddd`. `sourceBytes` and the block's `sourceChecksum` are
  computed on those exact source bytes, so a CRLF source file keeps its CRLF.
- `task upsert` prints one JSON object:
  `{"taskId":...,"requestId":...,"revision":...,"sourceBytes":N,"fileHash":"<sha256 of the written file>"}`.
  `fileHash` is recorded after the atomic write, so it identifies the file
  revision another writer may have replaced.
- `task list` prints `taskId<TAB>requestId<TAB>revision<TAB>sourceBytesB`;
  `task remove` prints `removed <id>` or `<id> not present`.
- Task sources must be Lua 5.1 with clear boundaries: register cleanup
  handlers with `ctx.OnCleanup(fn)`, stop your own timers/frames/events there.
- Release packaging always ships an empty registry; keep local probes out of
  commits.

## The execution context

The task runs with full access to WoW globals inside a captured environment:

- `print(...)` — captured into the report stdout (bounded by the block's
  `outputLimit`, default 384 KiB).
- `Finish(value, ...)` — complete the task successfully with return values.
- `Fail(message[, code])` — complete as failed.
- `SetAsync()` — declare the task asynchronous; the chunk returning then does
  NOT complete it. Sync tasks end when they return; async tasks must call
  `Finish`/`Fail` from their own callbacks. Late callbacks are ignored.
- `IsCancelled()` — cooperative cancel flag; `/dev auto cancel <task-id>`
  finalizes the run as `cancelled` and runs your cleanup handlers.
- `OnCleanup(fn)` — register cleanup; handlers run on any finalization.
- `Log(message)` — bounded lifecycle log line (128 entries / 16 KiB in the
  report).

The architecture cannot force-stop an infinite loop and cannot roll back side
effects. Generate bounded, cancellable probes only.

## In-game commands

| Command | Behavior |
| --- | --- |
| `/dev` | Opens the workbench (unchanged). |
| `/dev auto run <task-id>` | Validates the loaded block and executes its current request. |
| `/dev auto bug <1-100> [request-id]` | Snapshots up to 100 saved error records (all sessions, provider storage order, newest last-first). |
| `/dev auto status <task-id>` | Prints the latest execution state; re-shows the notice when the result still exists. |
| `/dev auto show <ticket>` | Re-shows the notice for a kept historical result. |
| `/dev auto cancel <task-id>` | Cooperative cancel; saves a cancelled report and its Ticket. |
| `/dev auto ack <ticket> received\|failed` | Records that the host read this ticket (or failed to). Clears the pending-flush block so the next run can start **without a reload**. |
| `/dev auto identify` | Shows the identity marker: a small QR code at the top-left carrying this character, realm, client and build, so a host with several windows open can learn which window is which. Not shown by default; it shares the corner with the completion notice. |
| `/dev auto unidentify` | Hides the identity marker. |
| `/dev auto stop` | Hides the notice UI only; the pending record and the busy state stay. |

### Identity marker vs completion notice

Both are small JSON QR codes at the top-left, and they are told apart by shape, not
by position:

| | identity marker | completion notice |
| --- | --- | --- |
| has `id` + `client` | yes | no |
| has `ticket` + `task` + `run` | no | yes |
| shown when | the user runs `/dev auto identify` | a result is committed |
| purpose | identify the window before input | deliver the ticket after a run |

`lycheedev instances --identify` reads the markers through window capture, so no
window is focused and nothing is typed. This is what lets a caller name the window
it is about to drive instead of guessing from a pid. Two windows showing the same
character and realm remain indistinguishable.

### Reporting the read outcome back

The plugin cannot observe whether the host actually read a result, so nothing on
the game side changes when a ticket is picked up. While a result is pending its
disk write, the controller answers every new request with the busy message, and
that block only ends with a reload — unless you tell it what happened:

```text
python scripts/automation.py --installation retail-main ack \
  --hwnd 0x1234 --ticket LYCHEE-... --status received
```

That sends `/dev auto ack <ticket> received` through the normal command channel.
Use `--status failed` when the read failed; the reason stays in your host log.
The index record keeps `receivedAt` and `receivedStatus`, and the Automation page
shows "Read by the host" / "Host read failed" instead of a permanent
"Awaiting reload". Acknowledging an unknown ticket or an invalid status changes
nothing and reports why.

The built-in error task is reported with task ID `bug` and inner report
`requestType` `bug`; its `params` are `{"count": n, "scope": ...}`.

## Host CLI

Global options, valid for every subcommand:

- `--data-dir <dir>` — host data directory holding the log, artifacts and
  profiles; default `%LOCALAPPDATA%\LycheeDev\automation`.
- `--installation <name>` — installation scope for the log and the reload
  dedup key; default `default`. Pass the same name to every command of one
  orchestration session, and use `--all-installations` when inspecting more
  than one.

### identify

Reads the in-game identity marker from one or more game windows, so a window can
be named before anything is typed into it. Nothing observable from outside the
game says which character is behind which window; the marker is how the game
tells you.

```text
python scripts/automation.py identify --hwnd 0x1234 [--hwnd 0x5678] [--timeout 1.5]
python scripts/automation.py identify --windows-json <instances.json>
```

- `--windows-json <file>` takes the instance list printed by
  `lycheedev instances`; each entry needs an `hwnd`. Prefer this when probing
  several windows, because the list survives the trip intact.
- Prints one entry per window, in the input order, as JSON:
  `{"hwnd":...,"pid":...,"flavorId":...,"identity":{...}|null,"error":null|"..."}`.
- **`identity: null` is not an error.** It is the ordinary state until the user
  runs `/dev auto identify` in that window. Only `error` marks a window that
  could not be probed.
- Exit code 5 means no window yielded a marker (including when every window was
  unreadable); a partly readable set exits 0. A missing capture package exits 4.
- Capture reads the window surface directly, so **no window needs the
  foreground** and nothing is typed. Windows are probed concurrently.
- A window showing the completion notice instead of a marker is not
  mis-identified: the two payloads are told apart by shape, not position.

Ask the user to run `/dev auto identify` in each window they care about, then
probe, then confirm the character back to them before sending any command.
`/dev auto unidentify` hides the marker again.

### task

`task upsert|remove|list` with `--install-dir <dir>` or `--profile <name>`.
Upsert also takes `--id`, `--source-file`, `--request-id`, `--revision`,
`--expect-revision`, `--created-at`, `--expires-at`, `--expected-interface`
and `--output-limit`. Generated IDs are `req-<UTC stamp>-<hex>`.

### profile

```text
python scripts/automation.py profile set --name retail-main --client retail \
  --wow-root <WoW root> --addon-dir <addon dir> \
  --sv-path "<WoW>/WTF/Account/<account>/SavedVariables/Lychee Dev.lua"
python scripts/automation.py profile show --name retail-main
```

`profile set` requires `--client`, `--wow-root` and `--addon-dir`; both
directories must exist. `--client` accepts `retail`, `classic`, `titan` and
`forever`. The stored profile records the client folder, and every accepted
location for that client is kept so a build that moved folders still resolves:

| `--client` | Client folder |
| --- | --- |
| `retail` | `_retail_` |
| `classic` | `_classic_` |
| `titan` | `_classic_titan_` |
| `forever` | `_classic_beta_`, or `_forever_` |

A folder name is a location rather than an identity: WoW: Forever currently ships
in `_classic_beta_` on the test track, which a MoP-era classic test client also
uses. `sv find` therefore tries every accepted folder under `--wow-root` and
falls back to the root itself, rather than trusting the recorded name.
Profiles are written atomically to `<data-dir>/profiles.json`.

### sv find / sv read

```text
python scripts/automation.py --installation retail-main sv find --profile retail-main
python scripts/automation.py --installation retail-main sv read --profile retail-main \
  --ticket LYCHEE-... --task inspect-example --request-id req-... --revision rev-1
```

- `sv find` requires `--profile`, resolves the client folder under `--wow-root`
  (a profile may point at the WoW root or at the client folder itself) and
  prints each account-level `SavedVariables/Lychee Dev.lua` candidate. It exits
  1 when there is no candidate.
- `sv read` requires `--ticket` plus either `--sv <path>` or `--profile`. It
  refuses any file that is not named `Lychee Dev.lua` and refuses `.bak`.
- Optional identity checks: `--task`, `--request-id`, `--revision`,
  `--created-at`, `--request-type task|bug`.
- `--retries` (default 3) and `--retry-delay` (default 0.5 s) bound the reads
  of a torn, replaced or temporarily unreadable file; the failure message
  reports how many attempts were made.
- The reader uses a restricted parser (no Lua evaluation), exactly locates
  `LycheeDevDB.exports.records[TICKET]`, verifies the envelope (`schema`,
  `ticket`, `createdAt`), the payload (`mediaType` `application/json`,
  `encoding` `utf-8`, real UTF-8 byte count against `byteCount`, 1 MiB report
  limit), the metadata identity (`taskId`, `executionId`, `revision`,
  `resultSchema`, terminal `status`, `complete`, `checksumAlgorithm adler32`)
  and recomputes the Adler-32 `contentChecksum`. It then parses the inner
  `lychee.automation.result.v1` JSON report and verifies its version, terminal
  status, `complete`/`incompleteReasons`, client/character `environment`,
  `requestType` and, for bug reports, `params`.
- Success prints
  `{"ticket":...,"status":...,"complete":...,"byteCount":N,"sha256":"...","attempts":N,"artifacts":{...}}`.
- Artifacts (`evidence.json`, `report.json`, `content.json`) land under
  `<data-dir>/received/<ticket>/` and are written byte-for-byte: the recorded
  `byteCount` and SHA-256 describe the exact bytes on disk, with no newline
  translation of `payload.content`. The host log records `received` with the
  attempt count and whether the file changed during the read.

### capture / send

```text
python scripts/automation.py capture --hwnd 0x1234 [--pid <pid>] [--exe-path <exe>] \
  [--timeout 120] [--interval 0.1]
python scripts/automation.py send --hwnd 0x1234 --text "/reload" \
  [--pid <pid>] [--exe-path <exe>] [--request-id req-... --ticket LYCHEE-...]
```

- `capture` opens one `windows-capture` session for the window, decodes the
  QR with `zxingcpp.read_barcodes` (QR only) and prints the notice JSON. It
  compares the top-left ROI on every `--interval` (default 0.1 s) until
  `--timeout` (default 120 s) expires, then exits 5. Only the newest ROI is
  kept, so frames never pile up.
- `send` logs `command_prepared` before input and `command_submitted` only after
  verified keyboard submission. Neither event proves game execution. When the text is `/reload`
  and both `--request-id` and `--ticket` are given it also applies the reload
  dedup key, so the same notice can never trigger a second reload.
- Both confirm the window identity (HWND, and `--pid`/`--exe-path` when given)
  immediately before use. Foreground input also checks focus between steps;
  background messages stay bound to the HWND without changing focus.

### run / bugs

```text
python scripts/automation.py --installation retail-main run \
  --task inspect-example --sv "<...>/Lychee Dev.lua" --hwnd 0x1234 \
  --install-dir <addon dir> [--request-id req-...] [--revision rev-1] \
  [--timeout 120] [--interval 0.1]
python scripts/automation.py --installation retail-main bugs \
  --count 20 --sv "<...>/Lychee Dev.lua" --hwnd 0x1234 [--timeout 120] [--interval 0.1]
```

- `run` does not send the input-side `/reload` itself: after `task upsert` the
  new block is only in the file, so reload the client first (or run a task that
  is already loaded), then call `run`.
- `run` delivers `/dev auto run <task-id>`, then polls for a notice whose
  `task` equals `--task` and whose `run` equals the loaded block's `requestId`
  (read from `Modules/Automation/auto/auto.lua` when `--install-dir` or
  `--profile` is given). A missing task block is an explicit failure.
- `bugs` validates that `--count` is 1-100 without silently fixing it,
  delivers `/dev auto bug <count>` and requires a notice with task `bug`.
- On a matching notice both flows log the `reload_requested` intent, snapshot
  the SV file, send one `/reload`, wait for the file to change within
  `--timeout`, then run the same verification as `sv read`.
- Because a task can run far longer than one capture interval, the capture
  session stays open for the whole wait; it stops when the notice is decoded,
  the window closes, or the timeout expires.

### status / recover

```text
python scripts/automation.py --installation retail-main status --limit 50 [--event received]
python scripts/automation.py --installation retail-main recover
```

Both read the host log scoped to `--installation` (use `--all-installations`
to read every scope). `status` prints the most recent records (`--limit`,
default 20; 0 prints all). `recover` prints recent records and lists tickets
whose reload was requested but never received. For those, read the
SavedVariables file first: if the Ticket is already on disk, run `sv read`
directly; if not, exactly one explicit reload retry is allowed — never a loop.
If the game crashed before writing, the report is lost; record that
uncertainty in your findings.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Success. |
| 1 | Invalid usage, missing binding/profile, unsafe path refused, non-UTF-8 source, or an unexpected failure (with traceback). |
| 2 | Task registry failure (schema, kind, marker, revision conflict, lock/limit refusal). |
| 3 | SavedVariables read or verification failure after all attempts. |
| 4 | Window identity, command input, or missing optional capture dependency (`windows-capture`/`numpy`/`zxing-cpp`) failure. |
| 5 | No notice decoded before the timeout, no window yielded an identity marker, or the game did not rewrite its SavedVariables file in time. |

Expected failures print one `error: ...` line on stderr; they never dump a
traceback for a missing dependency, a missing argument or a bad source file.

## Optional dependencies

```text
python -m pip install --target <dir> -r scripts/requirements.txt
```

`scripts/requirements.txt` pins the verified versions: `zxing-cpp==3.1.1`,
`windows-capture==2.0.1`, `numpy==2.5.3`, `opencv-python==5.0.0.93`. The code
uses only the interfaces those versions actually expose: `read_barcodes`
(zxing-cpp 3.x, `read_bars` does not exist), `WindowsCapture(window_hwnd=...)`
with `on_frame_arrived`/`on_closed` handlers, `start_free_threaded()` /
`CaptureControl.stop()` and `Frame.convert_to_bgr()`.

`capture`, `run`, `bugs` and `identify` read the screen and need these packages;
without them they fail with an explicit dependency message and exit 4 rather than
a traceback. `send` and `ack` import the same module but only use ctypes, so
keyboard delivery works without them. Everything else — `task`, `profile`, `sv`,
`status`, `recover` — runs on the standard library alone.

## Recovery

```text
python scripts/automation.py --installation retail-main recover
python scripts/automation.py --installation retail-main status --event run_unresolved
```

`recover` prints recent host log events and lists tickets whose reload was
requested but never received. For those, read the SavedVariables file first:
if the Ticket is already on disk, run `sv read` directly; if not, exactly one
explicit reload retry is allowed — never a loop. `run_unresolved`,
`sv_write_timeout` and `notice_identity_mismatch` records tell you which step
stopped and why. If the game crashed before writing, the report is lost;
record that uncertainty in your findings.

## Known verification gaps

Do not describe these as proven, and do not repeat them as if they were:

- **On-screen rendering of the notice and identity QR codes.** The encoding
  round-trips (the addon's own Lua encoder produces a matrix that the host's
  `zxing-cpp` decoder reads back, at every module size from 2 to 8 px), but the
  actual pixels drawn by the game have not been measured. Marker size and
  scannability under DPI/UI scaling, occlusion, minimization, exclusive
  fullscreen and HDR are all unmeasured.
- **Identity probe against a window that really shows a marker.** The capture
  pipeline itself has been exercised on a live client (frames arrive in 78-94 ms,
  ~3 ms to decode an empty ROI, no frame errors), but no probe has yet read a
  marker the game actually rendered. `/dev auto identify` has not been run in a
  game.
- **SendInput delivery reliability.** Keyboard input reaches the foreground
  window through the focus-verify-paste-confirm sequence; whether the game
  processes a given slash command still depends on client state, and `SendInput`
  returning success does not prove it did.
- **Real SavedVariables write timing** and the reload cost of a large report.
- **WoW: Forever in a running client.** Its TOC, client profile and event catalog
  are statically verified and pass the four-client matrix, but the addon has not
  been loaded in a Forever client.

The offline-verified parts are the task registry writer, the restricted
SavedVariables reader and verifier, the session log/dedup/artifacts, the notice
validation, the identity marker shape and probe concurrency, the polling shape
and the CLI exit behaviour; they are covered by `scripts/selftest_offline.py`.

### Automatic input and correlated acknowledgements

Automatic input remains the default, using background `--mode messages`.
Explicit `--mode foreground` cancels the current edit with
Escape, opens chat through the default slash key, replaces its contents, and copies the selected edit text back
for an exact comparison before submitting. A unique clipboard sentinel detects
an ignored copy operation. HWND/PID/executable and foreground are checked between
steps. Input is bounded and is never automatically retried after a timeout or
focus loss. `--mode messages` is a separate background transport: it confirms the bound
HWND/PID/executable before every PostMessage, cancels the old edit with Escape,
opens chat, posts UTF-16 WM_CHAR units and submits. It never focuses the target,
uses SendInput or touches the clipboard. Queue acceptance is not edit-state
verification: ignored Escape/Return or another focused game widget remain
unverified until real-game testing. Always require the correlated game receipt,
and never blindly retry an unresolved background submission.
Both modes accept only one slash command up to 255 UTF-8 bytes.
For foreground mode, the slash key must
open chat on the active keyboard layout and game keybindings; otherwise text
readback fails without submitting. There is no initial Enter that could send a
pre-existing draft if Escape is ignored.

In foreground mode, if focus is lost, no cleanup keys are sent into the newly focused application.
The game may retain an unsubmitted draft; the next transaction cancels it before
opening fresh input. Do not manually press Enter to resume an interrupted send.
Clipboard text is restored only while its value still belongs to the operation.
Readback verifies edit text, not the identity of the focused game widget: a
matching game receipt is still necessary to establish execution.

`ack --ticket <ticket> --status received|failed [--timeout 120]` supplies a fresh
nonce and waits for a QR containing that nonce, Ticket and outcome. A timeout is
`ticket_ack_unresolved` (exit 5), never confirmed success. The addon retains the
existing two-argument command for manual use. A nonce receipt never replaces a
different Ticket's pending completion notice. Update addon and tooling together;
older addons cannot emit the new receipt.

`run` requires the installed block or an explicit request ID; `bugs` sends its
request ID to the addon and filters the completion notice by that same ID.
Output reload still resolves the exact Ticket through verified SavedVariables.
