# Lychee Dev

[简体中文](README_zhCN.md)

[![CI](https://github.com/Follen/Lychee-Dev/actions/workflows/ci.yml/badge.svg?branch=rust-refactor)](https://github.com/Follen/Lychee-Dev/actions/workflows/ci.yml)
[![npm](https://img.shields.io/npm/v/lycheedev)](https://www.npmjs.com/package/lycheedev)
[![platform](https://img.shields.io/badge/platform-Windows_amd64-0078d4)](https://github.com/Follen/Lychee-Dev)
[![runtime dependencies](https://img.shields.io/badge/npm_runtime_dependencies-0-52a788)](packages/npm/lycheedev/package.json)
[![license](https://img.shields.io/badge/license-MIT-52a788)](LICENSE)

Lychee Dev is a World of Warcraft investigation toolkit for developers and
agents: a native Rust CLI, an in-game Lua workbench addon, a first-party Agent
Skill and an npm package. It turns a WoW question into fixed-source evidence,
typed game data, exported assets or an authorized observation on a running
client, and keeps the result inspectable.

The released platform is **Windows amd64**. The product version is defined only
by [release/version.json](release/version.json) and is currently **3.5.0**.

## Capabilities

| Area | What it answers |
| --- | --- |
| Source | Fixed Blizzard UI or third-party addon revisions: declarations, callers, context, load closure, validation, secret/value flow and revision differences. |
| Data | Typed DB2 records and metadata: catalog, fields, foreign keys, filters, joins, aggregates, paging, export and Hotfix records; spell, item, creature, encounter and decor reports. |
| Assets | Listfile search, CASC file inspect, raw export and BLP2-to-PNG/lossless-WebP conversion; local installation or explicit CDN delivery. |
| Live | Behavior in one verified running client through the existing addon and Lycheedev mailbox protocol v1: bounded Lua probes, verified reports, recovery and cleanup. |
| Evidence | Fixed pins and project locks, content-addressed captures, verification, bundles and bounded cache management. |
| Delivery | Managed addon and Skill installation, updates and removal from a release package; npm CLI update handoff. |

Source, data and asset work do not require a running game. Live work requires a
logged-in client with the managed addon loaded.

## Requirements

- Windows amd64 (`x86_64-pc-windows-msvc`); this is the only released target.
- [Node.js](https://nodejs.org/) **22.14.0 or newer** for the npm launcher and
  package tooling. The CLI itself is a native executable and has no runtime npm
  dependencies.
- Git on `PATH` for Source repository preparation.
- Game data work needs an installed client or an explicitly selected CDN
  source; some content is encrypted or unavailable and is reported as such.
- Live work needs a supported client, a running game process and the managed
  addon. Client support in code is not a claim of interactive acceptance.

## Install and deploy

```powershell
npm install --global lycheedev
lycheedev version --format json
```

The npm package contains the native CLI, the addon and the Agent Skill. Installing
the package does **not** deploy the addon or Skill automatically:

```powershell
$release = Join-Path (npm root -g) 'lycheedev'
$client  = 'D:\Game\World of Warcraft\_retail_'   # your client directory
$skill   = Join-Path $env:USERPROFILE '.agents\skills\lycheedev'

lycheedev addon install --release $release --installation $client
lycheedev skill install --release $release --path $skill
```

Inspect targets first and apply later updates through the managed updater:

```powershell
lycheedev update --plan --format json
lycheedev update
```

`update` synchronizes the npm CLI and managed Skills/addons. `--release <root>`
applies an already installed release offline; repeated `--path` / `--installation`
options scope the targets explicitly. Filesystem completion does not activate a
running game or an existing agent context: the client still needs a reload.

## Discover capabilities

The CLI is self-describing. Start from the task, then read only the contracts that
are needed:

```powershell
lycheedev cli search "spell cooldown" --format json
lycheedev schema target resolve
lycheedev data agent schema
lycheedev describe --format json
lycheedev <command> --help
```

Discovery reads command metadata only: it does not create a workspace, open a
cache, start a worker, use the network or connect to the game. The full installed
command list is in `lycheedev describe`; the authoritative Agent task and session
contract for Data is `lycheedev data agent schema`.

## Examples

### Source

```powershell
lycheedev init
$source = lycheedev source sync --source wow-ui-source --product retail --ref refs/heads/live --format json | ConvertFrom-Json
$sourcePin = $source.result.id

lycheedev source query C_Spell.GetSpellInfo --snapshot $sourcePin --mode precise --topic api --limit 10 --format json
lycheedev source context --snapshot $sourcePin --symbol-id <symbolId> --format json
```

A branch is resolved once into an immutable pin. Reuse that pin, or the project
lock, for follow-up reads. Use `source inspect`, `source refs`, `source diff` and
`source validate` for exact files, relationships, revision comparison and addon
load closures.

### Data

```powershell
$target  = lycheedev target resolve --installation $client --region cn --locale zhCN --format json | ConvertFrom-Json
$dataPin = $target.result.id

lycheedev data agent serve --snapshot $dataPin --installation $client --format jsonl
```

Open one caller-owned `data agent serve` process and send the typed JSONL tasks
declared by `lycheedev schema data agent serve`. The session owns a fixed data
identity and result handles for paging, export and capture; closing or a deadline
ends that ownership. Use `target available` for advertised releases and
`target add` / `target show` / `target list` for named target configurations.

### Assets

```powershell
lycheedev asset search --snapshot $dataPin --listfile community-csv --query "interface/icons" --format json
lycheedev asset export --snapshot $dataPin --installation $client --file-id 123456 --encoding png --output .\icon.png
```

Raw bytes are the default export. An explicit encoding selects image conversion;
a missing name, ambiguous variant, missing key or size limit is an error, not an
empty success.

### Live (authorized tasks only)

```powershell
lycheedev live instances --format json
lycheedev doctor --pid <pid> --format json             # read-only readiness
lycheedev live connect --installation $client --format json
lycheedev live probe put --name <name> --file <probe.lua>
lycheedev live execute --session <CON-id> --probe <name> --request <key> --format json
lycheedev live disconnect <CON-id>
```

Live work binds one verified process/runtime and keeps durable ownership, request
identity and recovery journals. `doctor --fix` repairs within the authorized
installation/target scope; when no current input protocol is loaded, first
activation can require one manual reload. Submitted or unknown input is never
replayed. `live status`, `live resume` and `live session` inspect and recover the
original work.

### Workspace, cache and evidence

```powershell
lycheedev doctor --format json
lycheedev cache status --data --format json
lycheedev evidence list --limit 20 --format json
lycheedev evidence verify <CAP-id>
```

`cache prune` reclaims only ordinary rebuildable cache; Data-derived pruning is an
explicit `--data-derived` operation. Captures and committed evidence are kept
until explicitly removed. The default Data capacity budget is decimal 5 GB
(5,000,000,000 bytes) and is configurable with `lycheedev config set`.

## Results and limits

- Ordinary commands return the versioned `lycheedev.result.v1` envelope; JSONL
  output uses begin/record/end frames with a legal end frame required for success.
  Data Agent session output uses its own versioned protocol.
- `complete`, `partial`, `pending`, truncated and unknown values are reported
  separately. An empty partial result never proves absence.
- Exit codes: `0` complete, `2` arguments/selection, `3` capability/environment,
  `4` input/protocol, `5` external/business failure, `6` pending, `7` cancelled,
  `8` internal.
- Session result handles are owned by the session; CAPs are committed evidence and
  outlive it. Pagination and export consume a retained result instead of
  re-running the query.
- Missing content, keys, coverage and quota are reported as errors or explicit
  gaps; they are not substituted with another build, region, product or client.

## Repository layout

| Path | Contents |
| --- | --- |
| [crates/lycheedev-cli](crates/lycheedev-cli/) | Command catalog, parsing, discovery, application orchestration and public output. |
| [crates/lycheedev-source](crates/lycheedev-source/) | Fixed source identity, load closure, analysis, query, relation and validation. |
| [crates/lycheedev-emmy-adapter](crates/lycheedev-emmy-adapter/), [crates/lycheedev-binding-contracts](crates/lycheedev-binding-contracts/) | Narrow adapter and shared types around the fixed Emmy upstream engine. |
| [crates/lycheedev-data](crates/lycheedev-data/) | CASC/WDC/BLTE input, Engine, DataFusion, Data Agent session, assets and Hotfix. |
| [crates/lycheedev-live](crates/lycheedev-live/) | Game instance binding, Mailbox, input, recovery, reports and cleanup. |
| [crates/lycheedev-delivery](crates/lycheedev-delivery/) | Managed installation, update transactions, addon/Skill delivery and npm handoff. |
| [crates/lycheedev-core](crates/lycheedev-core/) | Shared identities, errors, control, workspace, storage and evidence. |
| [crates/lycheedev-windows](crates/lycheedev-windows/) | Windows handles, files, processes, jobs, IPC and input boundaries. |
| [addon/](addon/) | Existing in-game Lua workbench addon, client profiles and Mailbox v1. |
| [skills/lycheedev](skills/lycheedev/) | First-party Agent Skill and its task-specific references. |
| [packages/npm/lycheedev](packages/npm/lycheedev/) | npm package and native CLI launcher. |
| [tools/](tools/), [tests/](tests/) | Build, packaging, CI and test tooling; fixtures and test targets. |
| [release/](release/) | Version source, release tool identities, license supplements and data. |

## Documentation

- [DESIGN.md](DESIGN.md): business goals, architecture boundaries, public
  contracts and the Skill/CLI usage surface.
- [AGENTS.md](AGENTS.md): repository working rules, documentation ownership and
  verification policy.
- [todo.md](todo.md): open implementation, cleanup, CI and verification work.
- [test.md](test.md): current test purpose, entry points and coverage.
- [handover.md](handover.md): current branch, collaboration boundaries, actual
  verification results and blockers.
- [skills/lycheedev/SKILL.md](skills/lycheedev/SKILL.md): the installed Agent
  skill.
- [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md): third-party components and
  attribution.

## License

Project-owned code is distributed under the [MIT license](LICENSE). Third-party
components retain their own licenses; see
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
