# AGENTS.md

Guidance for agents working in this repository.

## Current Rust migration authority (2026-10-15)

Read `handover.md` first: it is the sole current decision/status/evidence source.
`DESIGN.md` derives the target architecture and `todo.md` is the ordered checklist.
Use only `DESIGN.md`, never add a case-only `Design.md` duplicate. Do not create a
`handoff` document or competing handover. All older guidance below and in
`docs/toolkit/` is historical where it conflicts with this section.

- Current behavioral oracle: tagged Go v3.2.2, peeled commit
  `0bec60e4406f5d5dc155ee25cef44f9a710e37e8`, not moving main. Prior v3.2.1 `10b561db5706702cd42e047eaeed4ccb5e825351` and v3.2.0
  `3057a9666ab078270b63c7f06de748069b0d1f4b` remain historical test oracles.
  Apply the cumulative 3.2.1 and 3.2.2 increment matrices without relabeling old results or changing release versions.
- Reuse existing Rust code when differential evidence supports it; decide
  retain/adapt/replace per module. Do not restart indiscriminately.
- Only host language and Source engine change: Rust with EmmyLua direct analysis
  libraries/native worker/session replacing LuaLS. All business behavior, the
  same addon, CLI, Skill and complete `Lycheedev mailbox protocol v1`, including
  input/recovery/diagnostics, must match the fixed Go release. Old Rust exact
  COMMIT/four-slot redesigns and old input versions are not current authority.
- For owned business/cross-platform Rust, prohibit unsafe with suitable crate
  forbid/deny. Only unavoidable Windows FFI/memory boundaries may use minimal
  reviewed unsafe with per-site SAFETY, pointer/lifetime/thread invariants and
  tests. No blanket module exemptions or performance-driven unsafe. Inventory
  dependency unsafe separately. Reuse Go3.2.2 Skill with only necessary changes.
- Performance target: controllable elapsed Rust <= 30% of Go (at least 70% less),
  excluding separately recorded downloads and uncontrollable external waits.
  Same machine/data and equivalent results/checks; independent cold/warm repeats
  and per-path evidence. No unmeasured guarantee or reduced functionality/checks.
- The first delivery is documentation-only; subsequent full implementation is
  now authorized. Do not operate games, publish, merge main or change other
  projects/Go PR #13. Push each authorized batch promptly to the same dedicated
  Rust draft PR, updating handover. After implementation, complete its four
  sequential audit/fix/verify/independent-review loops to convergence; record
  exact trees and evidence, never promise mathematical zero bugs.
- Run independent offline and automated Windows checks before tests requiring
  user/game/specific interactive-machine cooperation. Keep all such tests in
  the final section of handover.md. Windows-only does not mean game-dependent.
- Preserve historical test/failure evidence and distinguish implemented,
  historical-pass/fail, unverified, blocked and not_run. Prove absent consumers
  before deleting dead code; don't assume luac/tree-sitter are obsolete because
  LuaLS is replaced. Recover exact modified Emmy bytes or publish a separately identified, fixed-public-
  upstream reconstruction with reviewed patches and full custody tests before
  claiming a clean checkout is reproducible. Never call new bytes the old fork.
- Existing disk/process ownership and evidence-retention rules below continue
  where compatible. Historical release/channel values are not current release
  permission. Product tests are not claimed for a documentation-only change;
  validate Markdown links, casing, scope and diff instead.

## Historical Rust refactor authority (2026-10-08; superseded)

For this worktree's Rust refactor, the user's revised requirements in
`docs/toolkit/rust-refactor-requirements-2026-10-08.md` take precedence over the
historical Go contracts below where explicitly changed. Preserve the frozen Go
v3.1.2 oracle and its evidence. In particular:

- Mailbox is the sole Live state, readiness, receipt and result channel.
  Color blocks, optical heartbeats and WGC are removed from the Rust candidate;
  there is no optical fallback. Record those explicit compatibility changes.
- Redesign the complete Native protocol before broad replacement. Existing
  EXECUTE/SETTLE code is reference material, not an approved final protocol.
  Independent fresh-context audit and executable fault/model validation must
  close correctness, isolation, recovery and resource issues before finalizing.
- Managed installations provide 500 slots. Final acceptance includes Retail,
  Classic, Titan, Forever Asia Beta (`wow_classic_beta`) and Forever CN Beta
  (`wow_cn_beta`) as distinct products/channels, even at the same build, with the
  required single/multiple instance and mixed-build scenarios. Missing evidence
  remains unaccepted; the historical Forever exclusion does not apply here.
- The latest 2026-10-12 user revision replaces private-heap discovery with Go
  v3.1.0 runtime instruction-pattern/RIP-relative Lua root lookup. Match only
  the selected module's mapped runtime code, derive root RVA from current
  instructions, then read Lua state -> globals -> LycheeDevInternal -> Mailbox.
  Stop heap-wide scans, hints and nearby-search fallback as a product route;
  preserve tested prototypes as historical evidence. Verify unique recipes,
  operands, mappings, anchor/root/path stability and validated Lua layouts.
  Disk-static finalization is not a gate. Current identity, authentication,
  freshness and protocol checks still govern input.
- Main and child agents use GPT-6 Astra / high. The main agent coordinates,
  integrates and independently verifies. Keep actual game acceptance last.
- The 2026-10-09 user-edited goal replaces the old Source engine prerequisite.
  Design Source from its business contracts and validate EmmyLua Analyzer Rust
  through its parser/analysis libraries first. LuaLS, LSP, the old broker and
  tree-sitter are not mandatory. Preserve old failures and map their risks into
  the new design; do not keep patching a replaced chain because it already exists.
  Complete design and a minimal complete prototype before migrating default
  commands, cache/session ownership, packaging and Skill. Never execute studied
  source or repository configuration. Query, cleanup and error-return time must
  fit their defined total budget; keep coverage, completion, truncation and
  uncertainty distinct.

## What this project is

Lychee Dev Toolkit 2.0: one Go CLI (`lycheedev`), one in-game Lua workbench
addon (`addon/`), one agent skill (`skills/lycheedev/`), one npm distribution
package (`packages/npm/lycheedev/`). Source research, game data, asset export
and live game investigation share one workspace, one evidence chain and pinned
references. The legacy Python-delegation implementation (1.x) is retired; its
history stays in git, its user data is never imported.

Authoritative documents, in order of precedence for implementation work:

1. `docs/toolkit/design.md` — architecture, contracts, naming.
2. `docs/toolkit/implementation-status.md` — current verified facts; honest
   boundaries (`not_run` stays `not_run`).
3. `docs/toolkit/regression.md` — acceptance matrix.
4. `docs/toolkit/release-3.0.2.md` — Windows CI and release contract for the current candidate.
   `release-2.0.1.md` is retained as the immutable historical contract for the
   already-published release.

Use `go test ./...` and `go vet ./...` at the repository root. The Lua protocol
suites need a Lua 5.1 interpreter (`LYCHEEDEV_REQUIRE_LUA51=1`); build one with
`node tests/tools/build-lua.mjs`.
The repeatable functional baseline is `node tools/baseline.mjs`; setup and the
separate real-client entry are documented in `tests/baseline/README.md`. Keep
manual/other-client acceptance separate from automated fixture results.

## Repository layout

```text
cmd/lycheedev/      program entry
internal/           command, selection, codebase, records, live(+journal),
                    bridge, evidence, vault, desktop, delivery, buildinfo
addon/              the in-game Lua side (workbench + bridge)
skills/lycheedev/   the versioned skill source (references + thin launcher)
packages/npm/       distribution package (binaries + payload, no runtime deps)
protocol/           cross-language protocol samples and fixtures
tests/              addon, protocol, process, qrfixture workbenches
tools/              version, packaging, release and consistency tooling
docs/toolkit/       design, status, regression, release contracts
```

## Non-negotiable acceptance criteria

1. **Zero cost while disabled.** Opt-in addon features create no frames, no
   events, no hooks until enabled. The owner-approved r4 bootstrap foundation
   is separately budgeted: one loader, one binding owner, three binding buttons;
   no idle events, hooks, timers or OnUpdate after registration. The opted-in
   reload beacon has a hard 45-second lifetime and stops on receiver wake.
2. **Zero behavior change without opt-in.** Only narrowly scoped bug fixes may
   change existing behavior.
3. **Event-driven and bounded.** No polling gates; bounded queues, budgets and
   pagination everywhere; truncation is always visible.
4. **Zero taint risk.** Protected UI and Blizzard objects are trust boundaries;
   combat lockdown and secret values are handled before unsafe operations.
5. **One shared API surface.** Client differences live in
   `addon/Core/Compat.lua` or the generated client catalogs, never scattered.

## Client matrix

Retail `120100`, Classic `50504`, Titan `38002` are the 2.0 acceptance matrix.
Forever `16001` code paths stay in the tree but are unverified and excluded
from acceptance. A client folder is a location, not an identity: read
`.flavor.info` and `version.txt` before trusting a directory name.

## CLI and command surface

- `lycheedev describe --format json` is the only command directory. The parser,
  help and the skill's generated `references/commands.md` all derive from the
  contract table in `internal/command/command_contract.go`.
- Never advertise an unimplemented command; `tools/skill-contract.mjs` fails the
  build when the skill references a missing command or flag.
- Exit codes per design §7: 0 complete, 2 arguments/selection, 3
  capability/environment, 4 invalid input/protocol, 5 external failure,
  6 pending, 7 cancelled, 8 internal. Map module errors explicitly in
  `internal/command/query.go` or the dispatch helpers.
- Results are `lycheedev.result.v1` envelopes; JSONL only counts as complete
  with a legal `end` frame and exit 0.

## Live (game) rules

Native `CON-...` connections use the memory/200-slot architecture described in
`docs/toolkit/live-slot-routing-2026-09-29.md` (current slot allocation and migration),
`docs/toolkit/live-input-architecture-2026-09-28.md` and the current
`skills/lycheedev/references/live-investigation.md`. Those contracts govern
native connection input, recovery, reload, disconnect and installation.
The receiver/QR `OP-...` and `BTP-...` rules below remain historical recovery
contracts; do not apply their keys, queue or ACK sequence to native connections.

- The dedicated receiver is the machine input path, including first-contact
  identify/connect/reset. Its minimal wake entry is available when the addon
  loads; wake does not authorize probe execution. CLI sends bounded LDB1 stage,
  verifies the staged receipt, triggers physical submit, reads the fresh
  `receiver_commit_ready` challenge, then sends an exact LDC1 commit and final
  Enter for intent-v2 dispatch from a key event. OnTextChanged never executes
  business code. Wake is Ctrl+Alt+], submit Ctrl+Alt+Shift+], close Ctrl+Alt+[.
  Never interpret a staged receipt or old submit key as accepted execution.
- Each command hides the receiver and releases focus before business dispatch.
  Asynchronous execution must leave no waiting panel or handshake QR over the
  scene. Terminal receipts remain necessary; finish visual sampling before
  Finish/Fail, and use complete execute/finish to clear the terminal display.
- Bootstrap input is journaled before sending. A pending `BTP-...` is recovered
  with `live status`/`live resume` on that ID; resume observes the original
  receipt and does not resend unknown input. Keep its selected target fixed.
  For installation activation or bridge loss, `live reload fallback` selects one
  clean managed installation and PID, journals the fixed Esc×3 Enter /reload
  Enter sequence, and verifies the runtime. Pending `OP-...` recovery does not
  replay uncertain keys or override another owner.
- Probe queues require a **clean managed addon**: files must match the
  installation receipt byte for byte. Deploy addon updates with
  `addon install` from a release root; overlay-copying the repo onto a managed
  installation makes it `modified` and the queue will (correctly) refuse it.
- `live run` executes only an already loaded operation and stops at a verified
  report. `live ack` explicitly acknowledges that exact operation, retires its
  queue entry and releases its window ownership; ACK does not trigger a cleanup
  reload. Use standalone `live reload` when requested or needed within the
  authorized task; the agent performs it automatically. Reconnect automatically
  when a reload changes readiness, retaining the selected window and character.
- `live probe load` requires `--budget-seconds <1-120>`. The budget is immutable
  for that operation; resume does not restart its execution time. Loading and
  SV persistence can reload the client, so probes that need transient scenes
  rebuild them after load and finish observation before report flush.
- Live results distinguish `report.state` (verified/unavailable) from
  `cleanup` (pending/complete). A verified report with pending cleanup is a
  usable result plus a recovery obligation — never report it as a failure.
- `live finish` combines ACK with verified display cleanup and archives the clear
  evidence. Its `complete=true` also requires `display.state=cleared`; retrying a
  successfully finished operation is read-only. Keep atomic `live ack` for requests
  that intentionally retain the display.
- `live reload` may activate an older runtime only when the current CLI's exact
  version is installed as a clean managed addon. It records from/to release and
  manifest commit, preserves window/actor/nonce checks and rechecks bytes before input.
- `live abandon` requires an explicit decision for unresolved probe/bugs work
  after preparation through ack_requested, including zero/partial/unknown input.
  Missing reports stay unavailable and execution/ACK remain unconfirmed.
  It preserves evidence, retires only the exact disk queue
  entry and releases ownership without game input. `cleanup=abandoned` and
  `complete=false` never mean ACK or runtime unload succeeded.
- Real-machine observation goes through `internal/desktop` WGC capture; screen
  screenshots are not evidence for D3D windows.
- Interactive desktop and game acceptance are recorded manually by the owner.
  CI has no self-hosted interactive desktop job or desktop-evidence gate.

## Change and verification workflow

### Rust disk and process ownership during this refactor

- Reuse this worktree's project Cargo cache. Use at most two controlled build
  channels: `target` for ordinary dev/test/release and `target-symbols` for the
  optimized `symbols` profile. Do not create a fresh target per fixture or
  package. Daily dev/test uses debug=1, dependency debug=0 and incremental;
  one-shot builds disable incremental. Preserve a matching EXE/PDB pair from
  the same `symbols` invocation in its sealed artifact record.
- Run Rust build/package runners through `tools/rust-disk.mjs` admission and
  ownership records. Initial caps are 20 GiB combined build caches and 5 GiB
  scratch, with a free-space reserve. Exceeding a cap stops admission until
  explicit terminal, rebuildable owner records have been reviewed. Existing
  unregistered targets and scratch are protected, not silently adopted.
- Classify build cache, scratch, immutable artifacts and evidence separately.
  Record owner, state, active process or held channel lease and retention.
  Never reclaim by age/mtime, dead PID alone, or another agent's namespace.
- Fixture input/home/prefix isolation remains required; reuse fixed-hash EXE
  and tgz when rebuilding is unnecessary. In success and failure paths, seal
  command/input hashes and logs before cleanup. Reap owned process handles
  before removing their scratch; unresolved or interrupted processes retain
  scratch for explicit recovery. Keep artifact and evidence receipts.
- The formal corresponding-source gate still requires two independent fresh
  Cargo homes and offline rebuilds. Run them sequentially within controlled
  channels, preserve the comparison artifacts and evidence, and reclaim only
  their terminal, registered rebuild caches afterward.

- One focused behavioral change at a time. Inspect the nearest equivalent
  feature and the relevant contracts before editing.
- Run `go build ./... && go vet ./...` after every edit batch; run the affected
  package tests before moving on; the full `LYCHEEDEV_REQUIRE_LUA51=1 go test
  -count=1 ./...` must pass before any commit you present as done.
- API facts come from the pinned baselines recorded in
  `docs/toolkit/implementation-status.md`, never from memory.
- Test both disabled and enabled addon states; stateful changes also need
  `/reload`, relogin, first-install and upgrade coverage.
- Visual changes need before/after screenshots from the real client; locale
  keys change in `Core/Locale.lua` and `Core/Locale_enUS.lua` together.
- Never call the retired wowdoc/wowdata/Python entrypoints; never read or
  import legacy user data (`LycheeDevDB`, `DumperDB`, old workspaces).

## Release discipline

- **Keep npm `latest` unchanged at `2.5.1` unless the owner explicitly requests
  changing that tag.** Publish new versions (including 3.0.2) under `next`;
  never move `latest` temporarily or promote a release automatically. Update
  the version-bound `release/npm-channel.json` for each release and verify
  both tags before and after publication. Keep the GitHub latest release
  unchanged for this channel as well.
- Version source is `release/version.json`; `node tools/version.mjs --write`
  syncs package, TOCs, addon runtime and Go buildinfo. `--check` gates CI.
- Publishing requires: clean tree at the tagged commit, required CI jobs
  green, `release.mjs assemble` (license gate, windows-amd64 binary,
  corresponding-source rebuild compare), sealed digests, isolated
  `--ignore-scripts` install smoke, windows run evidence, OIDC publish
  without token fallback, registry read-back, then the GitHub Release.
- The product ships Windows amd64 only (2026-09-23 owner decision):
  non-Windows platforms are not built, shipped or accepted.
- A published tgz is immutable; failures after publish use new versions, never
  a moved tag.
