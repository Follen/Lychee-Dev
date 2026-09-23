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
4. `docs/toolkit/release-2.0.1.md` — Windows CI and release contract.

Use `go test ./...` and `go vet ./...` at the repository root. The Lua protocol
suites need a Lua 5.1 interpreter (`LYCHEEDEV_REQUIRE_LUA51=1`); build one with
`node tests/tools/build-lua.mjs`.

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
   events, no hooks until enabled.
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

- The only bootstrap inputs are `/dev bridge identify <nonce>` and the connect
  opt-in, sent by `live connect` after per-window identity checks. Everything
  else requires a saved session and fresh readiness evidence.
- Probe queues require a **clean managed addon**: files must match the
  installation receipt byte for byte. Deploy addon updates with
  `addon install` from a release root; overlay-copying the repo onto a managed
  installation makes it `modified` and the queue will (correctly) refuse it.
- A completed run's cleanup reload returns the addon to its disabled default;
  the old session nonce is dead. The agent invokes `live connect` to build a
  new session before the next run; the user never types `/dev connect`.
- Live results distinguish `report.state` (verified/unavailable) from
  `cleanup` (pending/complete). A verified report with pending cleanup is a
  usable result plus a recovery obligation — never report it as a failure.
- Real-machine observation goes through `internal/desktop` WGC capture; screen
  screenshots are not evidence for D3D windows.
- Interactive desktop and game acceptance are recorded manually by the owner.
  CI has no self-hosted interactive desktop job or desktop-evidence gate.

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
