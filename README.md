<div align="center">
  <img src="addon/Media/Logo.png" width="88" alt="Lychee Dev" />
  <h1>Lychee Dev Toolkit</h1>
  <p><strong>From WoW source code to game data to a verified in-game answer.</strong></p>
  <p>A native CLI, an in-game workbench, and an Agent skill for World of Warcraft addon development.</p>

[![npm](https://img.shields.io/npm/v/lycheedev?color=ef6b78&label=npm)](https://www.npmjs.com/package/lycheedev)
[![CI](https://github.com/Follen/Lychee-Dev/actions/workflows/toolkit-ci.yml/badge.svg?branch=main)](https://github.com/Follen/Lychee-Dev/actions/workflows/toolkit-ci.yml)
[![Release](https://github.com/Follen/Lychee-Dev/actions/workflows/toolkit-release.yml/badge.svg)](https://github.com/Follen/Lychee-Dev/actions/workflows/toolkit-release.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-52a788)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows_amd64-0078d4?logo=windows)](#requirements)
[![Node](https://img.shields.io/badge/node-%E2%89%A522.14.0-417e38?logo=nodedotjs&logoColor=white)](#install)
[![Runtime dependencies](https://img.shields.io/badge/npm_runtime_dependencies-0-52a788)](packages/npm/lycheedev/package.json)
[![Agent skill](https://img.shields.io/badge/Agent_skill-included-8563bd)](#for-agents)

**English** · [简体中文](README_zhCN.md)

[For developers](#for-developers) · [For Agents](#for-agents) · [Install](#install) · [Development](#development) · [Regression evidence](docs/toolkit/business-regression-2026-09-26.md)
</div>

---

## For developers

Find where Blizzard defines an API, query the records behind a spell or item, export a texture, and investigate your addon in the running client. Lychee Dev brings these tasks into one workspace with fixed source commits, explicit game builds, and saved evidence.

**Source, data, and asset workflows do not require a running game or the addon.** Install the in-game workbench when you need live investigation.

### What you can do

| Workflow | Capabilities | Typical question |
| --- | --- | --- |
| **Source research** | Sync Blizzard UI source and supported addon repositories; index Lua/XML/TOC; precise or exploratory search; inspect symbols and lines; compare pinned revisions | Where is this API used, and what changed between clients? |
| **Addon validation** | Check the TOC/XML load closure, syntax and source-name references; run an explicit client matrix | Does this addon have missing load files or incompatible interface declarations? |
| **DB2 & SQL** | Typed schemas and rows, field search, foreign-key selection, pagination, JSONL streams, read-only SQL and CSV export | Which records match this condition? |
| **Game records** | Spell references, auras and summons; item models, geosets and textures; creature displays/models; encounters; housing decor | What data and assets are associated with this ID? |
| **Hotfixes** | Explicit Wago, local DBCache and Raidbots inputs; filtering, bounded pagination and captured results | What does this provider report for this build and scope? |
| **Assets** | Community listfile search; local CASC or CDN inspection; raw export; BLP2 → PNG/lossless WebP; bounded VP9 AVI demux | Find this icon and export its actual pixels. |
| **In-game workbench** | `/dev` Run, Diagnostics, Object Inspector, Events, Function Trace and export tools | Inspect this object, observe an event, or investigate an addon error. |
| **Live automation** | Identify a client, load a bounded Lua probe, verify its report, acknowledge it and clear the receipt; recover interrupted operations | Run this check on the selected character and bring back the result. |
| **Evidence & projects** | Fixed project references, archived captures, verification and bundles; explicit cache budgets and cleanup | Can I reproduce this answer and inspect its original evidence? |


### Requirements

- **Windows 10/11 x64.** Windows amd64 is the only shipped product platform.
- **Node.js ≥22.14.0** for npm distribution; the CLI itself is native Go. Native ZIPs are also available in [Releases](https://github.com/Follen/Lychee-Dev/releases).
- **Git** for source preparation. Local game data or network access is needed for the selected data source.
- A running, logged-in client and the installed addon for live investigation.

| Client | Interface baseline | Acceptance scope |
| --- | --- | --- |
| Retail | `120100` | Supported matrix |
| Classic / Mists | `50504` | Supported matrix |
| Titan / 时光服 | `38002` | Supported matrix |
| Forever / 无限服 | `16001` | Retained experimental path; limited recorded observations, outside full acceptance |

Client identity comes from installation metadata and build evidence, not the folder name. See [verified status](docs/toolkit/implementation-status.md) for the exact baselines and limits.

### Install

PowerShell:

```powershell
npm install --global lycheedev --ignore-scripts
lycheedev version --format json
lycheedev describe --format json
```

The package includes the CLI, addon and skill payloads. npm installation does **not** deploy the addon or skill automatically:

```powershell
$release = Join-Path (npm root -g) 'lycheedev'
$client = 'D:\Game\World of Warcraft\_retail_'  # use your client directory
$skill = Join-Path $env:USERPROFILE '.agents\skills\lycheedev'

lycheedev addon install --release $release --installation $client
lycheedev skill install --release $release --path $skill
```

These are first-install commands. For an existing installation, inspect `addon status` / `skill status` first. Managed upgrades use `--output <recovery-directory>` on the same drive, outside the live AddOns/skills directory. Modified or unmanaged copies require an explicit recovery decision; do not overlay files onto a managed installation.

Load the addon in the client after installation. Open `/dev` to use the workbench. A successful disk installation does not prove the running client has loaded that version.

### Research an API

```powershell
$source = lycheedev source sync --source wow-ui-source --product retail --ref refs/heads/live --format json | ConvertFrom-Json
$sourcePin = $source.result.id
lycheedev source index --snapshot $sourcePin --format json
lycheedev source query C_Spell.GetSpellInfo --snapshot $sourcePin --mode precise --topic api --limit 10 --format json
```

The branch is resolved once; subsequent commands reuse the returned immutable pin. To investigate a historical release, supply its exact commit or full tag ref. Use `source inspect` for the original lines and `source diff --from <pin-a> --to <pin-b>` for revision comparisons.

### Query real game data

```powershell
# Choose the region and locale appropriate to your investigation.
$target = lycheedev target resolve --installation $client --region us --locale enUS --format json | ConvertFrom-Json
$dataPin = $target.result.id
lycheedev data db2 schema ChrClasses --snapshot $dataPin --installation $client --format json
lycheedev data sql --snapshot $dataPin --installation $client --sql 'SELECT ID, Name_lang FROM ChrClasses LIMIT 5' --format json
```

Use `--cdn` instead of `--installation` to select remote content explicitly. `--offline` uses verified cached content and fails when it is missing; it never substitutes another build.

### Export an icon from a record

```powershell
$classes = lycheedev data db2 --snapshot $dataPin --installation $client --table ChrClasses --limit 1 --format json | ConvertFrom-Json
$iconId = $classes.result.page.rows[0].IconFileDataID
lycheedev asset export --snapshot $dataPin --installation $client --file-id $iconId --encoding png --output '.\class-icon.png' --format json
```

`asset search` finds candidate file IDs by name, text or extension. `asset inspect` verifies the selected CASC content. Choose `raw`, `png` or `webp` explicitly when exporting; the filename extension does not select an encoder.

> **Know the limits.** Some tables/assets contain encrypted sections whose keys are unavailable to the CLI. A blocked read is not an empty table. Hotfix pages can be incomplete; static validation is not proof of runtime or taint safety. The [business regression report](docs/toolkit/business-regression-2026-09-26.md) separates real-input results, fixture coverage and blocked cases.

## For Agents

Install the [lycheedev skill](skills/lycheedev/SKILL.md) to make the workflows discoverable. Its automatic invocation policy is enabled. The skill routes source, data, asset and live questions; the native CLI owns transport, persistence, integrity and recovery.

Examples of requests:

- “Find the declaration and callers of this API in the pinned Retail source.”
- “Query these spell records for this build; preserve missing fields and export the result.”
- “Find this texture and export it as PNG with its source identity.”
- “On this character, run a bounded probe, read the report, ACK it and clear the QR.”

### Discover, pin, execute, finish

1. Use `lycheedev describe --format json` for the actual installed command surface. Read only the relevant [skill reference](skills/lycheedev/references/commands.md).
2. Resolve the source/data identity needed by the task; carry the same pin through related calls. Do not silently replace an exact reference with `latest`.
3. Keep live input within authorization and bind it to the selected window and character. Preserve operation IDs after interruption.
4. Read the result, captures, warnings and completion fields. Finish the authorized operation before delivering the final answer.

### A QR card is a checkpoint, not completion

```text
connect → put probe → load → run → read verified report → finish
                         │                              │
                         └──── recover by operation ID ─┘
```

`live run` returns after report verification. **The Agent must continue** with `live finish` for the same operation: the CLI acknowledges the report, retires its queue entry, clears the receipt and verifies the clear. A failed clear keeps the verified report usable; retry the same operation to finish cleanup. Successful finish retains evidence and repeated calls send no game input. Use atomic `live ack` when the user explicitly wants to retain the display. Do not ask the user to scan the card or approve routine cleanup.

| Evidence | What it establishes |
| --- | --- |
| A visible QR, a sent command, or a loaded probe | An intermediate step; not the investigation result |
| `report.state: verified` | The report is usable; cleanup may still be pending |
| `cleanup: complete` | The operation has been acknowledged and its ownership released |
| `live finish`: `display.state: cleared`, `complete: true` | Report, ACK and final display cleanup are verified |
| JSONL with a legal `end` frame and exit 0 | The stream completed according to its reported scope |

On pending or uncertain input, inspect/resume the original operation. Do not create a replacement probe, switch characters, blindly reload, or automatically abandon ownership. If progress requires an external action, report the exact blocker, verified findings and retained IDs as an **incomplete handoff**. See [live orchestration](skills/lycheedev/references/live-investigation.md).

CLI results use `lycheedev.result.v1`. Exit codes distinguish argument errors (`2`), capability limits (`3`), invalid input (`4`), external failure (`5`), pending (`6`), cancellation (`7`) and internal failure (`8`). Success still requires checking scope and completeness.

## Development

```powershell
go build ./...
go vet ./...
$env:LYCHEEDEV_REQUIRE_LUA51 = '1'
go test -parallel=4 -count=1 ./...
node --test tools/*.test.mjs
node tools/version.mjs --check
node tools/skill-contract.mjs
```

Lua suites require Lua 5.1; `node tests/tools/build-lua.mjs` builds the pinned interpreter. The Windows CI also checks process locks, installation/package behavior and races. It does not impersonate a real game acceptance run.

| Area | Source |
| --- | --- |
| CLI and modules | [`cmd/lycheedev/`](cmd/lycheedev/) · [`internal/`](internal/) |
| Game workbench and bridge | [`addon/`](addon/) |
| Agent workflow | [`skills/lycheedev/`](skills/lycheedev/) |
| npm distribution | [`packages/npm/lycheedev/`](packages/npm/lycheedev/) |
| Contracts and verification | [Design](docs/toolkit/design.md) · [Status](docs/toolkit/implementation-status.md) · [Regression matrix](docs/toolkit/regression.md) |
| Release | [Release contract](docs/toolkit/release-2.0.6.md) · [GitHub Releases](https://github.com/Follen/Lychee-Dev/releases) |

## License

Lychee Dev Toolkit is licensed under [MIT](LICENSE). See [third-party notices](THIRD_PARTY_NOTICES.md) for dependency licenses and attribution.
