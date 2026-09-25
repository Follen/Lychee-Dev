# Toolkit 2.0 capability inventory

Status: source inventory for the Toolkit 2.0 capability surface. This file records
the migrated behavior, its current command owner and the contracts in
`docs/toolkit/design.md`. It is not an implementation plan; current availability
is checked against `lycheedev describe --format json` and the generated skill
command reference.

The design-convergence revision supersedes module/type names in this mapping:
application use cases now belong directly to codebase, records, delivery and live;
there is no actions package or shared jobs framework. Game coordination lives in
live/journal, while Hotfix and assets share records' data access. This inventory
still defines the capability scope; removing wrappers does not remove features.
Use implementation-status.md and CLI describe for current availability.

The current shared content reader backs DB2, SQL, asset inspection and raw/BLP2
image exports from either an installation or CDN. Source selection is explicit;
offline mode uses verified workspace caches. Historical build discovery, full
warmup, domain queries and video demux are represented in the inventory and
covered by the parity ledger at the level supported by current fixtures and
adapters; real-source coverage remains a separate acceptance concern.

Revision 2026-09-23: the legacy `add-on/` workbench (v1.2.0) is first-class
inventory scope (see "Legacy addon workbench" below); the wowdata/wowdoc rows
carry source-audit corrections; the mapping questions at the end of this file are
replaced by resolved owner decisions; and `live connect` joins the public `live`
surface by owner request.

## Reading rules

- A mapping marked `direct` has a named 2.0 command and an obvious owner module.
- A mapping marked `split` keeps the behavior, but the old command is divided
  between multiple 2.0 commands or private application stages.
- A mapping marked `change` is an intentional 2.0 behavior change. It must not be
  implemented as a legacy alias.
- A mapping marked `question` has no sufficiently precise 2.0 public owner yet.
- The old command names in this document are provenance only. Design section 2
  explicitly forbids compatibility aliases, old private protocols, and old helper
  names.
- The parity status of a row is tracked separately in
  `tests/parity/coverage.json`. A command mapping is not proof of complete legacy
  behavior. `passed` means current automated assertions passed;
  `fixture-backed` and `fixture-backed-partial` retain their source/coverage
  limits; `intentional-change` records a deliberate 2.0 contract change.

The current module names below are the design names: `selection`, `codebase`,
`records`, `assets`, `bridge`, `evidence`, `vault`, `desktop`, `delivery`, `live`,
and `command`. Coordination state belongs to `live/journal`; there is no
separate `actions`, `changes` or shared `jobs` assembly layer.

## wowdoc/wowdata parity ledger

The migrated wowdoc and wowdata inventory is tracked as 55 business cases in
`tests/parity/coverage.json` rather than as a single “command exists” checklist:
19 wowdoc cases and 36 wowdata cases. On 2026-09-24 the offline ledger contains
25 `passed`, 6 `fixture-backed`, 8 `fixture-backed-partial`, 16
`intentional-change`, and 0 `not_run` cases. The parity test checks that every
case has a current command mapping and repository evidence, but it intentionally
does not invoke retired binaries or import their user data.

This is enough to make every migrated area reviewable and to expose deliberate
2.0 changes. It is not a claim that every historical CDN Build, Hotfix provider
record, DB2 table, locale, or client has been exercised. Those boundaries remain
explicit in the case notes and in the network/client sections of
`docs/toolkit/regression.md`.

## Target command surface from the design

| 2.0 group | Public actions | Primary modules |
| --- | --- | --- |
| `describe`, `version`, `project` | machine-readable contract, version, workspace/project identity | `command`, `selection`, `vault` |
| `target` | `list`, `add`, `show`, `resolve`, `remove` | `selection`, `vault` |
| `project` | `init`, `lock`, `status` | `selection`, `vault` |
| `source` | `list`, `sync`, `index`, `query`, `inspect`, `diff`, `validate` | `codebase`, `selection`, `vault` |
| `data` | `sql`, `db2`, `hotfix`, `spell`, `item`, `creature`, `encounter`, `decor` | `records`, `selection` |
| `asset` | `search`, `inspect`, `export`, `demux` | `assets`, `records`, `selection` |
| `live` | `instances`, `connect`, `probe put|list|show|remove|load`, `run`, `ack`, `bugs`, `reload`, `status`, `resume`, `cancel` | `bridge`, `desktop`, `live`, `evidence` |
| `evidence` | `list`, `show`, `verify`, `bundle`, `keep`, `remove` | `evidence`, `vault` |
| `cache` | `status`, `verify`, `prune` | `vault` resource admission |
| `addon`, `skill` | `install`, `status`, `remove` | `delivery`, `selection` |

The design also makes `--home` the complete workspace root, `--snapshot` the
explicit `PinnedSet` selector, `--session` the explicit `WindowBinding` selector,
`--output` an explicit artifact path, and `--format` one shared output selector.
These replace source-specific implicit roots and last-used-target behavior.

## Source provenance

The inventory was read at these repository HEADs. No source repository was edited.

| Source | HEAD | Branch |
| --- | --- | --- |
| Lychee Dev (`packages/cli/bin/lycheedev.js`, `Lychee Dev skill/scripts/automation.py`) | `87efeed83cb3f90b69ccd9a8765c28a6925b5aae` | `codex/toolkit-2.0.0` |
| Lychee Dev addon (`add-on/` workbench, v1.2.0) | `87efeed83cb3f90b69ccd9a8765c28a6925b5aae` | `codex/toolkit-2.0.0` |
| wowdoc (`internal/app`) | `bd1fa8a010cb2b8d7cea8f21cb971705f54a1423` | `codex/search-validation-upgrade` |
| wowdata (`internal/app/commands.go`) | `6191d3dc567966b7a474849f3a11e7411390091c` | `main` |

## Lychee Dev npm wrapper: `packages/cli/bin/lycheedev.js`

The wrapper handles the first six names locally, then resolves a client/window
and forwards the remaining names to the bundled Python helper. `help` is a local
help answer; `--help` and `--version` are global answers, not subcommands.

| Current command | Current behavior | 2.0 mapping |
| --- | --- | --- |
| `install` | Installs Python dependencies, the skill, and the addon; records client configuration. | `addon install` + `skill install` and workspace setup; **change**: no Python bootstrap or old npm lifecycle implementation. |
| `update` | Re-runs install with `force`. | **change**: no `lycheedev update` alias; npm updates the npm distribution and release artifacts use the 2.0 delivery flow. |
| `doctor` | Reports installed/missing addon, skill, and client state. | `doctor` with capability checks owned by `command`/`delivery`; it must not read legacy roots or bootstrap Python. |
| `clients` | Scans detected installations and reports product/build/addon state. | `live instances` (installed + running candidates with identity states) per the resolved client-enumeration decision; named data targets stay `target list`/`add`/`show`/`remove`. Do not silently make this the current target. |
| `instances` | Lists running clients, PID, HWND, product/build, and optional identity markers. | `live instances` (`bridge.Binder`/`desktop`). Preserve product metadata and Build resolution. |
| `use [index]`, `use --character <name>`, `use --clear` | Pins a running window/build for later commands, or clears that pin. | `live bind` with an explicit `WindowBinding`; **change**: no implicit current window, last-used fallback, or `use` alias. |
| `help` | Prints the hand-written wrapper help text. | Generated `--help` plus `describe`; **change**: one generated 2.0 contract is authoritative. |
| `send <text>` | Sends one slash command to a resolved live window. | **change** (resolved): no public arbitrary-send command; raw slash send stays a private protocol stage, and supported work goes through `live run`/bridge protocol. |
| `reload` | Sends reload, decodes the nonce-bound readiness notice, and locally confirms readiness. | `live reload`; interrupted reloads use `live status`/`live resume` without duplicating the send. |
| `capture` | Polls a live window and decodes a completion notice. | **split**: private bridge signal handling plus `evidence` capture/archive; no public QR-only capture operation is specified. |
| `run --task <id>` | Delivers a registered task, decodes its notice, reloads once, reads the matching SavedVariables ticket, and forwards the result. | **split**: `live probe put` registers immutable source, `live probe load` loads without execution, and `live run <operation-id>` advances only that loaded operation to verified. |
| `bugs --count <n>` | Requests a bounded recent-error snapshot and reads the result ticket. | `live bugs` (`bridge.Executor.CollectFaults` + `evidence`). Preserve the bound and do not manufacture errors. |
| `ack --ticket <t> --status received|failed` | Reports ticket-read status to the game and waits for matching receipt and marker cleanup. | **change**: `live ack <operation-id>` acknowledges only the exact verified operation, retires its exact queue entry and releases ownership without a cleanup reload. |
| `task upsert|list|remove` | Owns the installed `auto.lua` task registry, including revision, expiry, interface, and output limits. | **change**: immutable `live probe put/show/list/remove` revisions plus `live probe load`; there is no executable mutable task registry or compatibility alias. |
| `sv find` | Finds account-level `Lychee Dev.lua` candidates under the configured client track. | **change**: no legacy SavedVariables discovery command; evidence is selected by `CaptureID`/workspace records. Client identity still follows product metadata, Build, then folder evidence. |
| `sv read --ticket <t>` | Reads and verifies a ticket from an exact SavedVariables path, including task/request/revision checks. | `evidence show`/`verify` as an internal archive operation; **change**: no direct old SV path or `LycheeDevDB`/old addon namespace import. |
| `profile` | Forwards profile management to `automation.py`. | **change** (resolved): old automation profiles are not 2.0 targets or sessions; use `target` for immutable selection and `live bind` for a live window. |
| `status` | Prints host session-log events. | `live status` and/or `project status`; choose based on whether the record is a live binding or a workspace operation. |
| `recover` | Lists unresolved reload requests and handshake cleanup instructions. | `live status`/`live resume` backed by `live/journal`; no separate legacy recovery alias is specified. |

Wrapper options that need explicit regression coverage are `--wow-root`,
`--client`, `--instance`, `--character`, `--hwnd`, `--pid`, `--python`,
`--force`, `--reset-registry`, `--json`, `--data-dir`, and `--installation`.
The first six become explicit selection/binding inputs where still needed;
`--data-dir` and `--installation` are not 2.0 workspace controls. `--python`
and `--reset-registry` are intentional removals from the native 2.0 release.

## Lychee Dev skill helper: `Lychee Dev skill/scripts/automation.py`

These are the helper's actual argparse commands, including commands that the JS
wrapper does not forward (`identify`). Shared helper options are
`--data-dir` and `--installation`; live commands also expose `--hwnd`, optional
`--pid`/`--exe-path`, `--mode foreground|messages`, `--timeout`, and `--interval`.

| Current command | Current behavior | 2.0 mapping |
| --- | --- | --- |
| `task upsert` | Writes or replaces one owned task block in `Modules/Automation/auto/auto.lua`, with ID/source/request/revision/expiry/interface/output-limit checks. | **change**: `live probe put` creates content-addressed immutable revisions under mutable names; load/run use the revision frozen into `live/journal`. No executable mutable task registry is restored. |
| `task remove` | Removes one task block by ID. | Same **change** (resolved) as `task upsert`; there is no registry to remove from. |
| `task list` | Lists task ID, request ID, revision, expiry, and interface metadata. | Same **change** (resolved); queued/executed work is inspected through `live/journal` operation records and the workbench Automation page. |
| `profile set` | Saves a named client, WoW root, addon directory, and exact SavedVariables path. | **change**: replace with `target add`/`target show` for a user target and `live bind` for a verified window; no old profile file import. |
| `profile show` | Reads one saved automation profile. | `target show` for named data targets, `live status`/`live bind` for window state (resolved); no `profile` alias. |
| `sv find` | Finds account-level SavedVariables candidates for a named profile. | **change**: no public legacy SV finder; evidence/archive lookup must be explicit. |
| `sv read` | Reads one exact `Lychee Dev.lua` ticket, retries torn/replaced files, validates identity, and prints report metadata/content. | `evidence show` + `evidence verify`; preserve retry and identity checks in the archive reader. |
| `send` | Sends exactly one slash command through foreground clipboard mode or background `messages` mode. | **change** (resolved): bridge operations are explicit and protocol-scoped; arbitrary slash injection is not a 2.0 command and raw slash send stays a private protocol stage. |
| `reload` | Sends a nonce-bound reload, waits for readiness, and clears the marker; `--resume` continues an existing nonce without another reload. | `live reload`/`live resume`. Preserve no-duplicate-reload, marker identity, cleanup, and unresolved-stage reporting. |
| `ack` | Sends received/failed acknowledgement, validates a matching receipt, then observes marker cleanup. | `live ack <operation-id>` is the public atomic closure action; it uses archived report identity and does not accept a raw ticket. |
| `capture` | Polls a window and decodes a nonce/task/ticket/status completion notice. | Private `bridge` signal decoder feeding `evidence`; QR/signal is never the complete report. |
| `identify` | Reads character/build identity markers from one or more windows or a JSON instance list. | `live instances`/`live bind`; preserve ambiguity refusal and verified identity binding. |
| `run` | Sends a task command, rejects stale/mismatched notices, reloads once, waits for SV write, reads and verifies the ticket, and saves an artifact. | `live probe load` + `live run` + `live/journal` + `evidence.Archive`; loading and execution are separate atomic actions and preserve the staged outcome. |
| `bugs` | Sends a bounded error snapshot request, resolves the notice, reloads once, reads the report, and saves an artifact. | `live bugs` + `evidence`; preserve `1..100` bounds and existing-error semantics. |
| `status` | Prints recent host log events, optionally filtered by event and installation scope. | `live status`/`project status`; records are owned by `live/journal` and `vault`. |
| `recover` | Finds reload requests and handshakes without confirmed receipt/cleanup and tells the operator to use `reload --resume`. | `live status` + `live resume`; no old `recover` command. |

The helper's documented default `--mode messages` is a behavior regression to
preserve in the Windows bridge where applicable: it posts to the target HWND
without foreground focus. Foreground mode remains explicit, and failure must not
silently fall back between modes. The helper's `--data-dir` default, named
`default` installation scope, and Python entry point are not 2.0 compatibility
defaults.

## Legacy addon workbench (`add-on/` UI and modules)

The legacy in-game workbench (`add-on/`, v1.2.0) is first-class inventory scope as
of the 2026-09-23 revision: eight capability classes plus the shared game-side
lifecycle. Module paths below are relative to `add-on/`, and the legacy entry
points are provenance names only.

| Current entry point | Current behavior | 2.0 mapping |
| --- | --- | --- |
| Interactive workbench, Lua editor/executor, result text/tree views, bounded run history: `UI/MainWindow.lua`, `Core/Bootstrap.lua`, `Core/Database.lua`, `Core/Inspector.lua`, `Core/Serializer.lua` | `/dev` opens the window (1040×720, 8 tabs, Esc close, lazy creation, combat refusal/shutdown). The executor strips the `/run` and `/script` prefixes, captures `print`/`dump`, returns compile and runtime errors as data, and caps output at 44 000 B. The serializer bounds depth 6 / 200 entries per table / 44 000 B (streaming: depth 32 / 10 000 entries / 16 MB). Value trees page 200 entries with load-more; stored snapshots are depth 8 / 4 000 nodes / 512 B values. Run history has a 16 MiB budget (12 KB code / 48 KB result entries, ≈1 024 entries) with oldest-first prune. | `addon/` workbench modules (re-implementation in progress). **change**: no `LycheeDevDB`/`DumperDB` import — history and exports are NEW data in `LycheeToolkitDB`. |
| Object inspection + result browsing: `Modules/ObjectInspector.lua`, `UI/Pages/Object.lua` | Path grammar with resolver; incremental result text in 44 KB chunks; mouse picker dock (F/Esc); ranked search (exact→prefix→contains) bounded to ≤200 results / 10 000 scans / 2 000 nodes / depth 6; node text popup with per-node export. | `addon/` object browser over the shared game-side inspector; preserve path grammar, resolver error semantics, and every bound. |
| Event catalog + filtering + bounded event listening: `Modules/Events/` (per-client generated catalogs: 1782/1483/1486/1802 entries with payload signatures), `UI/Features.lua` | Ranked search (limit 8, `ALL`/“全部” alias); selection semantics (ALL swaps); event monitor ring 500 records / 16 args / 256 B per arg; `RegisterAllEvents` mode. | `addon/` event catalog and monitor; per-client generated catalogs stay per-client; preserve search, selection and ring bounds. |
| Function tracing + trace results: `Modules/FunctionTrace.lua`, `UI/Pages/Trace.lua` | `hooksecurefunc` per traced path (never removed, only disabled); one active path at a time; trace ring 300 records / 16 args / 180 B per arg. | `addon/` trace module; preserve disable-only hook ownership, the single active path, and ring bounds. |
| Error diagnostics: collection, filter, detail: `Modules/Diagnostics.lua`, `UI/Pages/Diagnostics.lua` | !BugGrabber as collection provider; current-session/all scopes; keyword filter; Agent Report formatting with a 48 KB field cap; machine snapshot `SnapshotRecentErrors(1..100)` with `missingFields`/`incomplete` semantics; live refresh on `BugGrabber.BugGrabbed`. | `live bugs` evidence path + `addon/` diagnostics page. **change**: soft !BugGrabber integration — no hard TOC dependency, explicit unavailable state. |
| Result export, export records, view/copy interaction: `UI/Export.lua`, `UI/Pages/ExportRecords.lua`, `Core/Database.lua` | Evidence envelope `lychee.evidence.v1`; tickets `LYCHEE-YYYYMMDD-HHMMSS-%04d`, monotonic and never reused; 16 MB / 200-record budget with protected + pending lifecycle; post-save ticket popup (select + Ctrl+C copy); records page with list/detail/two-click delete/clear/reload. | `evidence` + `addon/` export records. **change**: envelope becomes `lycheedev.export.v1`, and history/exports are NEW data in `LycheeToolkitDB` (no legacy namespace import). |
| Automation tasks: in-game view, execute, history: `UI/Pages/Automation.lua`, `Modules/Automation/` (Controller/Report/Reload/auto registry) | In-game view of queued and executed automation work plus history against the `auto.lua` task registry. | The workbench view remains over `ProbeQueue\|ProbeRunner\|ReportStore\|Reentry`. **change**: no `auto.lua` registry or `/dev auto` grammar; CLI delivery is immutable `live probe put/load`, then atomic `live run` and `live ack`. |
| About/version page, bilingual UI, complete navigation: `UI/Pages/About.lua`, `Core/Locale.lua`, `Core/Locale_enUS.lua` | About/version page and the complete 8-tab navigation; zhCN base + enUS overlay (~300 keys); zhTW falls back to zhCN, all other locales get English; placeholder parity. | `addon/` About page and locale modules; preserve key-count/placeholder parity and the fallback order. |

## wowdoc: `D:/Code/wow/wowdoc/internal/app`

`wowdoc` currently constructs the following public paths. Its `update`, `clean`,
and `uninstall` commands are lifecycle commands, not source-query capabilities.

| Current command | Current behavior | 2.0 mapping |
| --- | --- | --- |
| `wowdoc --version` | Prints the embedded wowdoc version from the Cobra root. | `version`; preserve build provenance, but do not keep the `wowdoc` executable or add a legacy alias. |
| `init` | Creates the wowdoc home, installs/ensures Git, synchronizes source mirrors, discovers refs/tags (`--hot-tags`, default 10), builds indexes, and writes readiness state. | **split**: workspace creation is 2.0 `init`; mirror sync/index publication is `source sync` + `source index`. Preserve resumable partial initialization, but do not make old home layout the new home. |
| `update` | Runs `npm install -g @follenfang/wowdoc@latest`; `--dry-run` only prints it. | **change**: no `wowdoc update`; package/update delivery belongs to the single Toolkit distribution and `addon`/`skill` delivery. |
| `clean` | Lists temporary files (Temp-only scope) and deletes them only with `--yes`, preserving snapshots. | **resolved**: `cache prune` plus the explicit fresh/archive workspace flow; do not alias `clean`. |
| `uninstall` | Deletes wowdoc data and skill, and optionally the global npm package, after `--yes`. | **change**: use explicit `addon remove`/`skill remove` for installed components; workspace and old data require the design's explicit fresh/archive flow, not broad deletion. |
| `doctor` | Checks Git, writable home, and initialization state without changing it. | `doctor`; preserve read-only diagnostics, but report Toolkit capabilities and avoid legacy paths. |
| `source` | Source repository command group. | `source`; module boundary is `codebase` plus `selection`. |
| `source list` | Lists catalog sources and, when initialized, refs and selected product tags. | `source list` (`codebase` + `selection`). Preserve source/product identity and observable refs. |
| `source check` | Requires source/product and checks the local/remote source mirror; fields `{sourceId, product, branch, localCommit, remoteCommit, updateAvailable, initialized}`. | **resolved**: `doctor` capability check (mirror freshness, bounded network, `unknown` offline); NOT `source validate`, which keeps its TOC/addon validation meaning. |
| `source sync` | Synchronizes selected mirrors concurrently (hot tags hard-coded to 50, unlike `init --hot-tags`) and publishes resolved commits/tags. | `source sync` (`codebase`/`selection`/`vault`). Preserve bounded concurrency and partial failure details. |
| `index` | Immutable source-index command group. | `source index`; module boundary is `codebase`/`vault`. |
| `index build` | Builds an immutable index for an explicit source/product/ref, or from `--source-path` fixture input with a deterministic FNV synthetic commit. | `source index` (`codebase`); preserve exact commit/tag evidence and immutable snapshot IDs. |
| `index refresh` | Uses the same index builder under a refresh verb. | `source index`; **change**: one new action, no old `refresh` alias unless its distinct semantics are specified. |
| `index status` | Reports index status for required source/product: `{activeSnapshot, readySnapshots, snapshotFiles, astFiles, parserSchema, indexSchema, database, contentDatabase, journalMode}`. | **resolved**: a read model surfaced by `source list` (prepared snapshot readiness); no separate verb. Preserve snapshot readiness and commit identity. |
| `query` | Searches indexed source text with `--topic api\|lua\|xml\|toc\|asset` and a result limit; one evidence-tier-ranked engine with role penalties (vendor 15; locale\|generated-data\|tool 20); Search stops at the strongest matching tier. | `source query` / `codebase.Browser.FindSymbols`; preserve topic filtering, role penalties and bounded results. **resolved**: `explore` is a search MODE of this same engine. |
| `explore` | Runs the broader mode of the same indexed engine with the same text/topic/limit inputs: Explore always includes weaker evidence tiers and OR-joins FTS, while Search stops at the strongest tier. | **split** (resolved): retained as a search MODE of `source query` (exact vs exploratory tiers of one engine); not a second command, not removed, not an alias. |
| `inspect` | Inspects a qualified symbol or repository path in an indexed snapshot; symbol lookup falls back to `search(25)` and asset paths yield asset metadata rows. | `source inspect` / `codebase.Browser.ReadSpan`; preserve symbol/path requirement, the search fallback, and exact source pin. |
| `diff` | Compares two selected source refs/snapshots. | `source diff` / `codebase.Comparer.CompareTrees`; preserve explicit `from` and `to`, with no latest fallback. |
| `validate` | Validates an addon directory's Lua files, or a selected TOC closure against a source/product/ref; five stable diagnostic codes `api_not_found, event_not_found, xml_template_not_found, toc_interface_mismatch, compatibility_reference_not_found`. | `source validate` / `codebase.Checker`; preserve ordered TOC closure, the five diagnostic codes, and source evidence. |
| `validate-matrix` | Validates every target in a strict JSON matrix (`--config`: per-target `id,toc,product,ref`, optional `source` defaulting to `wow-ui-source`, top-level `path`, duplicate-id rejection) and merges the results. | `source validate --matrix <file>`; the 2.0 flag name is `--matrix` (the old `--config` name is not ported). Preserve target uniqueness, explicit source/product/ref, and merged diagnostics. |

The old wowdoc home resolver and `latest` defaults are specifically not portable
2.0 selection behavior. 2.0 requires an explicit `PinnedSet`/snapshot whenever a
source operation needs a fixed reference.

## wowdata: `D:/Code/wow/wowdata/internal/app/commands.go`

The following table includes every command spec and child path declared in
`registerCommands`. The group names are public Cobra commands even when an
action is normally expected below them.

| Current command | Current behavior | 2.0 mapping |
| --- | --- | --- |
| `sql [query]` | Executes read-only `wowdata-sql-v1` against static DB2, with file/stdin, named `--param` parameters (type inference), an exactly-one-source rule for inline text/file/stdin, JSON/JSONL/CSV output, and plan/metrics output. | `data sql` / `records.Reader.SelectRows`; preserve read-only enforcement, parameter typing, the exactly-one-source rule, plan/metrics, limits, and output completeness. |
| `hotfix query` | Reads independent Hotfix records from Wago, DBCache, or Raidbots using product/build/region/locale/table and optional raw/decoded/latest filters. Providers: Wago (Inertia parser, identity-keyed cache receipts, MaxPages 2048), DBCache (`HFS1` sidecar index), Raidbots (30-day-bounded snapshot). Filters `--table-hash/--record/--push/--status/--from/--to/--search/--page/--dbcache/--raidbots/--dbd`; CSV columns `id,push_id,record_id,table_name,status,build,region_id,locale,payload_length`. | `data hotfix`; Wago, local DBCache and bounded Raidbots snapshots are implemented with explicit provider/identity/coverage, cursor drift checks and verified cache pages. Providers never fall back to one another and never overlay static DB2 rows. |
| `warmup` | Prepares a complete local or remote target, including selected tables, listfile, DBD manifest, and cache. Every data command auto-prepares with a per-command table/listfile dependency merge (warmup is not a separate step for queries). Defaults: table list `SpellName,Spell,SpellEffect,SpellMisc,SpellCastTimes,SpellDuration,SpellRange,JournalEncounterSection`, `listfile=true`, `listfile-format=binary`, `dbd-manifest=true`, TACT-key warm; post-warm prune keeps only the current build when invoked as `warmup`; persists BuildSnapshot with RecentBuilds cap 2. | `target resolve` prepares local installation or current remote release identity, verifies configurations and pins definitions. **split** (resolved): identity preparation stays `target resolve`; bulk content preparation becomes on-demand query preparation plus the cache maintenance surface (`cache status\|verify\|prune` + workspace `config.json`); there is no `warmup` verb. Historical release selection is only via exact `--build`. No implicit current target. |
| `db2` | DB2 query group. | `data db2` / `records.Reader`. |
| `db2 schema <table>` | Prints parsed table schema metadata. | `data db2` schema operation; preserve table identity and DataPin. |
| `db2 rows <table>` | Fetches rows by ID(s) (`--ids`), fields, filter, and limit. | `data db2` rows; preserve bounded read-only selection. |
| `db2 search <table>` | Case-insensitive field search with limit. | `data db2` search; preserve field/query semantics and bounds. |
| `db2 foreign-key <table>` | Queries rows by a foreign-key field/value, unbounded by default (limit 0). | `data db2` relation operation; preserve relationship semantics and the explicit unbounded mode. |
| `db2 stream <table>` | Streams large row sets as JSONL or JSON with field/filter/limit; JSONL is per-row envelopes without begin/end frames. | `data db2` streaming result; **change**: 2.0 upgrades to a typed begin/record/end contract while preserving incomplete-result reporting. |
| `spell` | Spell relationship group. | `data spell` / `records.Navigator`. |
| `spell info` | Traverses trigger chains and description references to a bounded depth. | `data spell`; preserve max-depth and relation provenance. |
| `spell auras` | Detects aura presence for a spell. | `data spell`; preserve absence/presence result semantics. |
| `spell summons` | Detects NPC summons from spell effects, optionally filtered by NPC ID. | `data spell`; preserve optional NPC filter. |
| `encounter` | Journal encounter group. | `data encounter` / `records.Navigator`, with `asset` for exported icons. |
| `encounter get` | Returns section tree and related spell IDs. | `data encounter`; preserve section relationships and bounded traversal. |
| `encounter export` | Exports encounter skills and PNG icons in one process. | **split**: `data encounter` plus `asset export`; preserve one-request relation and manifest. |
| `file` | CASC file lookup/export group. | `asset` / `assets.Exporter` and `records` file metadata. |
| `file lookup` | Resolves fileDataID to filename. | `asset inspect`; preserve ID/name mapping. |
| `file search` | Searches listfile entries with a limit. | `asset search`; preserve listfile source and bounds. |
| `file extension` | Lists files by extension with a limit. | `asset search`; preserve extension filtering. |
| `file get` | Fetches a raw CASC file by ID or name to an output path, silently overwriting an existing output. | `asset export --file-id --output` implements verified raw local/CDN export with manifest/evidence and explicit overwrite. **change**: silent overwrite is NOT ported — 2.0 refuses existing outputs by default and requires explicit `--overwrite`. Name/listfile selection remains required. |
| `file exists` | Checks whether a CASC file exists by ID or name. | `asset inspect`; preserve non-exporting existence check. |
| `file encoding` | Inspects content-key and encoding-key metadata. | `asset inspect`; preserve CASC metadata rather than treating it as decoded content. |
| `file export` | Writes a raw CASC file to disk, silently overwriting an existing output. | `asset export` implements explicit raw output, fixed-source manifest/hash and atomic publication without default overwrite. **change**: silent overwrite is NOT ported — default refuse plus explicit `--overwrite`. |
| `icon` | BLP texture group. | `asset` / `assets.Exporter`. |
| `icon export` | Converts BLP to PNG or lossless WebP with mipmap/mask options; the `--mask` flag is echo-only (recorded, never affects conversion) and is NOT a real capability. | `asset export --encoding png\|webp --mipmap --channels` implements BLP2 palette/BC1/BC2/BC3/BGRA8 conversion, selected-level budgets, actual channel transformation and source/artifact manifests. The echo-only `--mask` flag is not ported. Full real-build coverage remains required. |
| `casc` | CASC source-state group. | **split** (resolved): `target`, `cache`, and `doctor` cover the pieces; there is no `casc` group. |
| `casc info` | Shows current Build and cache state; reports `{source,region,product,locale,buildName,buildKey,cachePath,cdnHost,archiveCount,rootEntryCount,encodingEntryCount,tactKeyCount}`. | `target show` (prepared identity) + `cache status` (cache accounting). |
| `casc products` | Lists available products and Builds from a local or remote source. | `target list` (remote manifest listing, bounded); preserve product and Build identity, not a folder-only guess. Historical remote builds resolve only via exact `--build` while the CDN still serves the configs (else `target.build_unavailable`); listing shows current manifest builds. |
| `casc diagnose` | Inspects CDN, archive, root, encoding, cache, and TACT state via checks `source,region,product,cache_path,go_runtime,cache_dir,build_key,archives,root,encoding,tact_keys`. | `doctor` + `cache verify` (resolved split); preserve read-only diagnostic detail. |
| `item` | Item metadata/assets group. | `data item` / `records.Navigator`, with `asset` for referenced files. |
| `item get` | Returns item summary and slot information. | `data item`; preserve DataPin and item ID. |
| `item models` | Returns model fileDataIDs and textures for race/gender. | `data item` plus `asset inspect`; preserve race/gender inputs. |
| `item geosets` | Returns geoset and helmet-hide data. | `data item`; preserve model relationship data. |
| `item textures` | Returns character texture fileDataIDs. | `data item` plus `asset inspect`; preserve IDs and source references. |
| `creature` | Creature display/model group. | `data creature` / `records.Navigator`. |
| `creature display` | Queries creature display metadata by display ID or model fileDataID. | `data creature`; preserve either lookup key. |
| `creature model` | Queries creature model fileDataIDs and variants. | `data creature` plus `asset inspect`; preserve variants. |
| `decor` | Decor data group. | `data decor` / `records.Navigator`. |
| `decor list` | Lists decor entries with a limit. | `data decor`; preserve bounded listing. |
| `decor get` | Gets decor by item ID or model fileDataID. | `data decor`; preserve either lookup key. |
| `video` | WoW video-container group. | `asset` / `assets.Exporter`. |
| `video demux` | Inspects VP9 AVI container metadata only: at HEAD it writes NO files, and `outputDir` is merely echoed. | `asset demux`; intentional completion, not a regression — 2.0 performs real bounded frame export plus manifest. Preserve frame bounds and explicit output. |
| `profile` | Named complete data-target profile group. | **split**: `target list/add/show/remove`; preserve explicit target fields, but no old profile store or implicit current profile. |
| `profile list` | Lists named profiles. | `target list`; no legacy profile import. |
| `profile show <name>` | Shows one named profile. | `target show <name>`. |
| `profile set <name>` | Creates/replaces source/region/product/Build/locale profile. | `target add` (and explicit replacement semantics); resolve to a `DataPin` before query. |
| `profile remove <name>` | Removes one named profile. | `target remove <name>`; preserve explicit name and reference protection. |
| `cache` | Managed cache group. | `cache` / `vault`. |
| `cache status` | Shows cache size and configured limit with usage taxonomy `{total,payload,unique,duplicate,resume,derived,amplificationRatio,withinMax,withinAmplification}`. | `cache status`; preserve protected-object visibility and the usage taxonomy. |
| `cache verify` | Verifies cached object integrity. | `cache verify`; preserve content hashes and no destructive repair by default. |
| `cache prune` | Prunes old unprotected Build caches in phases: 24h resume-TTL GC, unreferenced object GC, then oldest unprotected builds down to ≤maxBytes (protection = profile RecentBuilds). | `cache prune`; preserve reference protection and atomic coordination. |
| `cache clear` | Deletes all rebuildable cache data. | **change** (resolved): NOT ported — destructive; `cache prune` is policy-driven, and any broader removal is an explicit workspace operation, never an alias. |
| `cache config` | Shows/updates cache capacity and worker settings; writes `config/config.json` with schema `wowdata.config.v1` `{cacheMaxBytes, downloadWorkers, workerMode auto\|fixed}`. | Workspace `config.json` under `--home` for budgets (resolved); no `cache config` verb. |
| `doctor` | Diagnoses installation, target, cache, and network basics. | `doctor`; preserve read-only checks and actionable capability results. |
| `update` | Updates wowdata through npm. | **change**: no `wowdata update`; use the single Toolkit distribution/update path. |
| `uninstall` | Removes wowdata, its Skill, and managed data, optionally keeping data. | **change**: explicit `addon`/`skill remove` and workspace/archive policy; no broad old uninstall alias. |
| `golden` | Golden-fixture group for command capture/comparison. | **resolved (decision 14)**: retained as development regression tooling under `tests`/`tools`; not a public 2.0 command. |
| `golden capture` | Captures Go command output into a named fixture and manifest. | Test tooling only; no public 2.0 command mapping. |
| `golden compare` | Compares actual Go JSON output with one/all fixtures. | Test tooling only; no public 2.0 command mapping. |

The wowdata source uses defaults that must not silently become Toolkit defaults:
`hotfix --source wago`, `warmup --cache ~/.wowdata/cache`, its preload table
list, `listfile=true`, `listfile-format=binary`, `dbd-manifest=true`, and default
profile/current-target selection. 2.0 must require or resolve these through an
explicit `DataPin`, project lock, or documented capability configuration. Shared
flag facts: `--build` accepts `latest|version|VersionsName|BuildConfig|BuildKey|trailing build id`;
environment knobs `WOWDATA_TIMING`, `WOWDATA_PROCESS_START_PROBE`,
`WOWDATA_METADATA_WORKERS`, `WOWDATA_LARGE_RANGE_WORKERS`,
`WOWDATA_RANGE_CHUNK_MIB`, `WOWDATA_ARCHIVE_TAIL_KIB`. Cache-format v2
auto-wipes the cache when the format version changes; **change**: that is NOT
ported — 2.0 refuses unsafe writes instead.

## Behavior regressions that must remain

These are observable behaviors from the current sources and the Lychee Dev skill
that the rewrite must retain unless the table above explicitly marks a change.

### Selection and identity

- A client folder is only a location. Resolve product identity from `.flavor.info`
  and Build from `version.txt`; if those authoritative metadata files are absent
  or contradictory, return an identity error instead of using the folder name as
  a product fallback. This matters especially for `_classic_beta_` carrying WoW:
  Forever.
- Multiple live windows remain ambiguous unless a verified character/build marker
  distinguishes them. Refuse to send to an uncertain window.
- A pinned binding may survive a restart by immutable identity (Build/install
  evidence), but PID/HWND are live facts and must be revalidated before use.
- Source Commit, game Build, addon version, and Hotfix timestamp remain separate
  fields with `exact`, `compatible`, `mismatch`, or `unknown` relationships.
- No query silently selects the last window, last profile, latest source, or a
  shared `current-target`. 2.0 selection is explicit: snapshot, target config,
  then project lock, with missing fields reported.

### Live execution and recovery

- Background Windows input remains the default where supported: post messages to
  the bound HWND without foreground focus. Foreground/clipboard mode is explicit;
  an error in one mode must not silently switch to the other.
- A QR/notice proves only a signal. Complete game work requires matching task,
  request, Ticket, client identity, Build, source revision, SavedVariables read,
  host archive verification, ACK, and marker cleanup.
- A reload is sent once per logical request. `resume` waits/cleans up an existing
  nonce and never sends the reload again. Unresolved stages remain resumable.
- Stale notices are rejected by task/request/Ticket/status identity. A timeout or
  closed capture session is unresolved, not success.
- `bugs` requests at most the requested bounded number of existing errors; it does
  not create errors to fill a count. Large report bodies remain in SavedVariables
  and the host archive, not in the QR payload.
- A game probe error can still produce a valid capture and completed cleanup; the
  result must distinguish operation completion from `probeStatus`.
- SavedVariables are read as data with a restricted parser. Unknown/new fields are
  preserved, and old records are not rewritten into the 2.0 namespace.

### Source, data, assets, and output

- SQL remains read-only. Static DB2 and independent Hotfix data remain separate;
  Hotfix results must not silently overwrite static DB2 results.
- DB2, relationship, CASC, BLP, video, and encounter operations remain bounded,
  source-attributed, and explicit about partial results and output paths.
- Streaming output must use a valid begin/record/end/error contract; a broken pipe
  or truncated result must not be presented as complete.
- 2.0 JSON keeps the design envelope (`schema`, `ok`, `operationId`, `context`,
  `result`, `captures`, `warnings`, `error`) and stable error stages/codes. Logs
  and progress stay off stdout.

### Safety and lifecycle

- No legacy default or fallback is part of 2.0: no old home resolver, old
  `~/.wowdoc`, `~/.wowdata`, or automation directory; no old environment-variable
  meaning; no implicit Python/Node bootstrap; no old SavedVariables import.
- The new `~/.lycheedev` workspace is a complete root selected by `--home`, then
  `LYCHEEDEV_HOME`, then the documented default. A legacy-looking occupied path
  is detected and handled by an explicit archive/fresh flow, not merged or
  silently deleted.
- Destructive cache/workspace operations protect snapshots, unfinished operations,
  evidence, and pinned objects; external output refuses overwrite unless explicit.
- The addon side continues to require zero cost while disabled, event-driven work
  while enabled, WoW Lua 5.1 syntax, secret-value checks, and no tainting of
  protected Blizzard UI.

### Legacy workbench invariants

The `add-on/` workbench adds these invariants on top of the addon rules above:

- Zero frames, events and hooks before the first `/dev`; the workbench is created
  lazily and never pre-created.
- Combat refusal plus `PLAYER_REGEN_DISABLED` shutdown: no workbench work in
  combat, and owned work shuts down on the combat-end signal.
- `issecretvalue` checks before any compare, format, index or branch on values
  that may be secret; the secret-dependent path is refused, and the feature
  degrades to a harmless visible state rather than throwing or leaving stale
  hidden state (AGENTS.md wording).
- No `SetScript` on Blizzard frames, no custom fields on Blizzard tables, and no
  `OnUpdate` or wall-clock state.
- Stable layouts across labels, values and representative UI scales.
- Per-client TOC closure stays ordered: client profile → compatibility → locale →
  core engines → feature modules → UI.
- Bare `/dev` opens the workbench while the `connect`/`disconnect`/`bridge …`
  machine verbs coexist on the same slash surface.
- Local inspection and interaction must never require connecting to the external
  CLI.
- The workbench and the CLI share ONE game-side capability implementation: the UI
  must not re-implement execution/diagnostics logic, and the CLI must not fake
  these features through a generic Lua entry point.

## Intentional 2.0 changes

These are deliberate contract changes, not missing compatibility work:

1. The product has one native Go CLI and one workspace. The release path does not
   invoke `wowdoc`, `wowdata`, or `automation.py`, and npm is only a distribution
   channel with no runtime dependency or Python installer.
2. Old names (`install`, `update`, `profile`, `task`, `sv`, `send`, `capture`,
   `ack`, `recover`, `warmup`, `casc`, `golden`, and the old lifecycle commands)
   are not aliases merely because a similarly named 2.0 action exists.
3. `--home` changes meaning to the full Toolkit root. Old `--data-dir`, old
   installation scopes, old home-directory composition, and old environment
   variables are not silently accepted as equivalent configuration.
4. There is no automatic latest-source fallback, last-window fallback, current
   profile fallback, or folder-name-only product detection. Callers supply or
   obtain a real `PinnedSet`, `DataPin`, or `WindowBinding`.
5. Old user data and old addon SavedVariables are not migrated or imported into
   `LycheeToolkitDB`. A fresh/archival transition is explicit and recoverable.
6. Installation is split into explicit `addon` and `skill` capabilities; data
   cache maintenance is `cache status|verify|prune`; package updates are not
   hidden inside an old tool's `update` command.
7. Live work reports durable stages and evidence references. A submitted command,
   observed signal, game completion, report read, archive commit, ACK, and cleanup
   are separate states rather than one optimistic success message.

## Resolved mapping decisions (owner, 2026-09-23)

The former open mapping questions are resolved owner decisions as of the
2026-09-23 revision. Each is marked resolved; the tables above keep the
source-grounded provenance facts and now cite these decisions.

1. **`source check` — resolved:** becomes a `doctor` capability check (mirror
   freshness, bounded network, `unknown` offline); NOT `source validate`, which
   keeps its TOC/addon validation meaning.
2. **`index status` — resolved:** a read model surfaced by `source list`
   (prepared snapshot readiness); no separate verb.
3. **`query` versus `explore` — resolved:** `explore` is retained as a search
   MODE of `source query` (exact vs exploratory tiers of one engine); not a
   second command, not removed.
4. **`warmup` — resolved:** split explicitly: identity preparation stays
   `target resolve`; bulk content preparation becomes on-demand query
   preparation plus an explicit cache maintenance surface
   (`cache status|verify|prune` + workspace `config.json`); there is no `warmup`
   verb.
5. **`casc` group — resolved:** `casc info` → `target show` (prepared identity) +
   `cache status` (cache accounting); `casc products` → `target list` (remote
   manifest listing, bounded); `casc diagnose` → `doctor` + `cache verify`.
6. **Automation task blocks — resolved:** NOT retained; replaced by
   immutable `live probe put/load` + the probe queue (`lycheedev.queue.v1`) + `live/journal`
   operation records. No task-registry compatibility surface.
7. **SavedVariables reading — resolved:** internal to `evidence show|verify`
   (archived captures); no public raw-SV command, no legacy namespace import.
8. **Public live surface — resolved:** beyond `live bind|run|reload|resume`,
   `live instances` (discovery + identity) and `live connect` (whole-task
   automatic connection) are ADDED by the 2026-09-23 revision (user-requested).
   Raw slash send, QR capture and ACK submission stay private protocol stages;
   `live bugs` remains the bounded error-snapshot entry.
9. **Addon install detection / client enumeration — resolved:** `live instances`
   (installed + running candidates with identity states); named data targets are
   `target list/add/show/remove`. The exact `SelectionSpec` field set remains
   open at implementation time.
10. **Cache and lifecycle commands — resolved:** `cache config` → workspace
    `config.json` (budgets); `cache clear` → NOT ported (destructive;
    `cache prune` is policy-driven); wowdoc/wowdata `clean`/`uninstall` →
    `cache prune` + explicit `addon remove`/`skill remove` + the explicit
    fresh/archive workspace flow.
11. **Output encodings — resolved:** `text|json|jsonl` (typed
    begin/record/end frames) + `--encoding csv` as an EXPORT encoding with a
    machine-readable manifest for tabular results (sql/hotfix/db2 rows); asset
    conversion options stay on `asset export`
    (`--encoding raw|png|webp --mipmap --channels`), manifests recorded in
    `CaptureRef`/export manifests.
12. **Snapshot policy — resolved:** data/asset/source reads accept `--snapshot`
    explicitly, else `--project`/nearest project lock (already implemented);
    `project lock` itself requires an explicit resolved snapshot; `live run`/
    `resume`, two-sided `source diff`, and evidence reads are never redirected
    by project defaults.
13. **Historical remote builds — resolved:** only via exact `--build` while the
    CDN still serves the configs (else `target.build_unavailable`); remote
    listing shows current manifest builds. No third-party historical source in
    2.0.

14. **Golden capture/compare tooling — resolved:** retained as development
    regression tooling under `tests`/`tools` per roadmap §1 (fixture capture,
    semantic comparison incl. `remote-products-v1`/`cache-state-v1`/
    `profile-state-v1` normalization and `requiredGroups` enforcement); it is
    NOT a public 2.0 command surface. No capability is dropped: the comparison
    semantics move with the tooling.

No mapping question remains open. `SelectionSpec`'s exact field set is decided
at implementation time, as noted in decision 9.
