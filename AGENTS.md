# AGENTS.md

Guidance for agents working in this repository.

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
4. `docs/toolkit/live-mailbox-v1-architecture-2026-10-04.md` — current branch's
   live protocol and qualification contract; the reconstruction plan records
   remaining work, not completed acceptance.
5. Version-bound release documents retain their own release scope. The mailbox
   candidate still has a 3.1.1 label; that does not give it the published release's
   protocol acceptance or authorize publication.

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
   events, no hooks until enabled. For this mailbox branch, the one-shot core
   loader is separately budgeted; disabled mailbox allocates no arena, events,
   timers or OnUpdate. Earlier r4 binding buttons and reload beacon belong to
   the retired input protocol, not the current disabled-state allowance.
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

The current branch's protocol authority is
`docs/toolkit/live-mailbox-v1-architecture-2026-10-04.md`, with operational guidance
in `skills/lycheedev/references/live-investigation.md`. Lychee Dev mailbox protocol
v1 (`lycheedev.mailbox.v1`) supersedes the earlier duplex and native slot/routing
contracts for this branch. Those contracts and published 3.1.1 acceptance records
retain only their historical scope. There is no LoD, keyboard bootstrap, optical
readiness or old schema/wire/layout compatibility here.

- Disabled transport allocates no arena, event subscriptions, timers or OnUpdate.
  The one-shot core loader is separately budgeted. The owner authorized bounded
  polling while mailbox is enabled; it is not a general exception for features.
- `inbox` is CLI to addon; `sendbox` is addon to CLI. Identity, SHA256, exact
  uint64 sequences, fresh private challenge and one outstanding request govern
  execution. One preallocated command row carries one complete source of up to
  1 MiB in one native write. The next command carries the exact ACK for the
  previous durably saved result; final disconnect uses the small stop row.
- Run read-only targeted doctor before each live drive. Business readiness must
  not block independently eligible cancel, result retrieval or disconnect.
- Retain project, CON, request, source and budget on pending work. Unknown effects
  do not replay. A cancelled unaccepted command retains exact `not_started`
  terminal evidence and consumes its challenge; it is not retransmitted.
- Private strong roots retain every exchange row. Repair permits at most the
  current and one retired arena; command and stop writers must drain before
  exact repair acceptance can release retired roots. They do not pin VM lifetime.
- The CLI owns native publication and leases. Production publication uses the
  direct, unsuspended route, without debugger attach, game-function hooks or
  thread suspension. The stopped-helper route is disabled after its preserved
  Retail security crash. Do not bypass eligibility with raw writes, guessed
  offsets, deleted claims, suspension or injection. Raw address research is a
  separate, explicitly scoped workflow.
- Build-bound read/RVA qualification does not establish native write capability.
  Every writer profile needs independent array/number and collector/lifecycle
  evidence. Exact Retail 12.1.0.69933 image qualification is limited to the
  documented small/1 MiB/close direct trial and three in-probe GC cycles.
  Reload/new-runtime follow-up, same-build dual instances and other builds
  remain not_run; every other hash/build remains write-ineligible. Addon
  self-tests and offline fixtures do not establish CLI-to-game execution.
- Do not initiate reload during a command write; tell the user to avoid manual
  /reload in that interval. Normal reload after request cleanup is allowed.
  The owner accepts the residual external-reload race between the final check
  and WPM: private roots do not keep a destroyed Lua VM alive. Do not describe
  a finite trial as having eliminated it.
- Deploy sealed candidate files using addon install. A clean managed installation
  must match the CLI; no overlay copying. First enablement uses /dev connect;
  replaced incompatible runtimes require an explicit manual reload.
- Cosmetic transport feedback shows Agent执行中 only while probe code runs.
  Real D3D visual evidence goes through internal/desktop WGC.
- Real client and interactive desktop acceptance remain separate from CI fixtures;
  there is no self-hosted interactive desktop job or desktop-evidence gate.
## Change and verification workflow

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
