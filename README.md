# Lychee Dev Toolkit

A unified, native toolkit for World of Warcraft addon engineering: one Go CLI
(`lycheedev`), one in-game Lua workbench addon, and one agent skill. Source
research, game-data queries, asset export, and live game investigation share a
single workspace, a single evidence chain, and reproducible pinned references.

| Component | Location | What it is |
| --- | --- | --- |
| CLI | `cmd/lycheedev`, `internal/` | Native Go binary: source, data, asset, live, evidence, delivery commands |
| Addon | `addon/` | The in-game `/dev` workbench and the protocol bridge the CLI drives |
| Skill | `skills/lycheedev/` | Agent skill: workflow routing and evidence discipline over the CLI |
| npm package | `packages/npm/lycheedev/` | Distribution: platform binaries + addon/skill payload, zero runtime dependencies |

## Supported clients (2.0 acceptance)

| Client | Interface | Build |
| --- | --- | --- |
| Retail (Midnight) | `120100` | `12.1.0` |
| Classic (Mists) | `50504` | `5.5.4` |
| Classic Titan (Wrath) | `38002` | `3.80.2` |

Forever (`16001`) code paths remain in the tree but are **not verified and not
part of the 2.0 acceptance matrix**.

## Requirements

**Windows 10/11 x64 only.** The toolkit ships, verifies and accepts Windows
amd64; a logged-in desktop session is required for live game automation.

## Install

```bash
npm install -g lycheedev          # CLI + addon + skill payload
lycheedev addon install --release <distribution-root> --installation <client-directory>
lycheedev skill install --release <distribution-root> --path <parent-directory>
```

`npm install --ignore-scripts` works; the launcher needs no install scripts and
no runtime npm dependencies.

## Quick start

```bash
lycheedev init                                        # new-format workspace (~/.lycheedev)
lycheedev target resolve --installation <client> --region cn --locale zhCN
lycheedev source query C_Spell.GetSpellInfo --snapshot <pin>
lycheedev data db2 schema Map --snapshot <pin> --cdn
lycheedev live connect --snapshot <pin>               # automatic discovery, identity and in-game opt-in
lycheedev live run --session <id> --file probe.lua
lycheedev doctor
```

`lycheedev describe --format json` is the machine-readable command directory;
[skills/lycheedev/references/commands.md](skills/lycheedev/references/commands.md)
is generated from it. Every result is a `lycheedev.result.v1` envelope with
evidence captures; every live action is recoverable by operation ID.

## Development

```bash
go build ./... && go vet ./...
LYCHEEDEV_REQUIRE_LUA51=1 go test -count=1 ./...   # full matrix; Lua 5.1.5 required on Windows
node tools/version.mjs --check                     # version-source consistency
node tools/skill-contract.mjs                      # skill ↔ command-surface consistency
node tools/release.mjs assemble --out <dir> --npm-cli <npm-cli.js> --cgo zero
```

Windows CI is the release gate: five required jobs (`windows-contract`,
`windows-process`, `windows-addon`, `windows-package`, `ci-required`).
See [docs/toolkit/release-2.0.1.md](docs/toolkit/release-2.0.1.md)
for the release contract and [docs/toolkit/regression.md](docs/toolkit/regression.md)
for the acceptance matrix.

## Design and status

- [docs/toolkit/design.md](docs/toolkit/design.md) — architecture and contracts
- [docs/toolkit/implementation-status.md](docs/toolkit/implementation-status.md) — verified facts and boundaries
- [docs/toolkit/capability-inventory.md](docs/toolkit/capability-inventory.md) — legacy capability mapping

## License

MIT — see [LICENSE](LICENSE). Portions adapted from the same author's wowdata
repository are dual-licensed AGPL-3.0-or-later OR MIT; wowdoc-derived portions
are MIT. Full third-party inventory:
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). Corresponding source ships
with every release.
