# Changelog

All notable changes to the Lychee Dev Toolkit are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/) with
version entries matching the single version source `release/version.json`.
Detailed acceptance records and evidence live in
[docs/toolkit/implementation-status.md](docs/toolkit/implementation-status.md);
per-release publishing contracts live in `docs/toolkit/release-*.md`.

## [Unreleased]

### Fixed

- Recognize China Forever installations in `_cn_beta_` with `wow_cn_beta`
  metadata and matching 1.60.1 builds. Keep China and international launcher
  slots separate during installation discovery and local target resolution.
  Existing international Live connection records remain recoverable when their
  older metadata omits the launcher slot; regional or other identity changes
  still fail closed.

- Local target resolution accepts game roots and client directories with optional
  product constraints. Shared installation discovery reports candidates and
  identity conflicts, ignores catalog-only remnants, and never falls back to a
  remote target. Addon selection and live path/PID filtering reuse the same reader.
- Reused data slots reject foreign build series. Availability reports total rows
  and truncation instead of silently shortening the version list.
- CDN delivery routes refresh once from the selected version service on
  availability failure, including configuration preparation; fixed keys and
  offline behavior are preserved. Alternate content encodings can recover HTTP
  failures; exhausted remote copies retain remote errors.
- Static data commands have a shared total timeout (`--timeout-seconds`, default
  300, range 1–3600), covering source preparation, recovery and query execution.
- Forever CDN queries and target preparation use the configured publishing slot
  for endpoint selection and build-uid validation, while preserving the exact pin.
- Availability discovery requests the publishing slot it reports, so Forever
  releases are discoverable through the actual HTTP request as well as its locator.
- Local CASC reads accept fully zeroed preambles only with full BLTE EKey
  authentication; nonzero mismatches and corrupt payloads remain errors.
- CDN archive lookup continues after an unavailable loose mirror, retaining
  uncertainty when no authenticated copy is found. Local missing objects and
  archive integrity failures now have structured query errors.
- Data investigation skill distinguishes transport, decoding and coverage
  failures, shared installations, and workspace health from data readiness.
  It documents product discovery and name-to-ID lookup without requiring callers
  to recreate the CLI's product routing or archive validation.
- Data skill now starts with a short decision flow and loads target, table,
  Hotfix and recovery details on demand. Bounded retry and cursorless-search
  handling are explicit; a CLI scenario covers duplicate-name discovery through
  ordered SQL continuation to distinct related effects.
- Source skill separates short declaration lookup from revision preparation,
  relationships and value-flow analysis. Repository tracks and client API
  environments stay distinct; CLI regression rejects cross-query/cross-command
  cursor reuse, with scenario acceptance recorded separately from agent behavior.

## [3.0.0] — 2026-09-28

### Added

- Native live connections use verified process-memory records and 64 managed
  load-on-demand input slots. Project-local journals retain target identity,
  requests, nonces and result evidence across interrupted CLI runs.
- Current-process scans support eight workers, SIMD matching, coverage accounting
  and optional address hints; invalid hints fall back to discovery and never
  authorize a result without runtime and record validation.
- Shared-installation coordination allows two game processes to use the same
  slot pool while enforcing one connection owner per process. Short publication
  locks, durable reservations and exact pre-input payload checks preserve
  independent progress and prevent stale receipts from retiring new work.
- Input readiness is observed from addon memory before sending keys or reload.
  Ordinary probes leave input free; explicit protected phases retain their
  execution budget and release protection on completion or error.

### Fixed

- Removing an edited managed addon with input slots reports an installation
  conflict consistently and leaves the addon, slot pool and recovery paths intact.
- Resume retains the original request after publication contention, runtime
  replacement or interrupted input. Proven process exit can retire ownership
  without falsely reporting unknown execution as successful.
- Automation history persists in the workbench and distinguishes execution
  success or failure from transport acknowledgement. Shared controls, report
  areas and scrollbars are aligned; obsolete settings are removed.
- The small bouncing Lychee distinguishes Agent execution from input protection;
  the bounded reload beacon remains a visual hint, not execution evidence.
- Skill workflows now describe native connections, slot rotation, recovery,
  multi-instance selection and managed installation activation.

### Verification and limits

- Recorded real-client coverage includes Retail shared-installation races,
  Classic, Titan and specified Forever build experiments; see the
  [release contract](docs/toolkit/release-3.0.0.md) for exact evidence and limits.
- Historical OP/BTP recovery remains available. Complete loss of the project
  journal is not transparent recovery, and successful tests do not imply zero
  errors or stable low latency.

## [2.5.1] — 2026-09-28

### Added

- `lycheedev update` updates the global npm CLI, bundled LuaLS, selected managed
  Agent skills and in-game addons together. It supports explicit multi-client
  targets, read-only planning and offline payload synchronization from a release.
- Update preflight checks every target before replacement. Exact npm bytes and
  target identities survive interruption; native delivery resumes incomplete
  replacements and removes old managed files after verification. Recovery stays
  on each target's drive, outside addon/skill discovery directories.
- Isolated update regression uses real npm, a loopback registry, temporary
  prefixes, two simulated clients and two CLI versions. It checks replacement,
  obsolete-file removal, conflicts, concurrent ownership, retry, JSONL and
  SavedVariables preservation without changing the user's installation.

### Fixed

- Skill guidance now matches read-only shortcut settings, bounded pre-commit
  bootstrap recovery, connection input blocking and the probe activity indicator.
- Skill command checks derive all command groups from the CLI contract and
  validate packaged reference links. The generated command guide can be checked
  for drift instead of silently regenerated.
- Update results distinguish verified files from game reload and Agent context
  activation, and retain structured recovery information on failure.

## [2.5.0] — 2026-09-28

- Rebuilt source research around pinned Git worktrees and bundled LuaLS, with
  semantic references, context, validation and bounded value-flow evidence.
- Added complete Hotfix paging, explicit key handling, coverage/provenance
  reporting, effective record overlays and SQL query improvements.
- Reworked Agent live execution around correlated input, immutable budgets,
  durable ownership, verified reports and explicit display cleanup.
- Refined the workbench controls and page layouts. A bouncing Lychee indicates
  connecting/running state; input is blocked only during command reception.

## Historical 2.0.2 candidate notes

These notes preserve the earlier candidate's scope and observations; they are
not the current release status. See its [historical contract](docs/toolkit/release-2.0.2.md).

### Added

- Unified single-manifest addon architecture (the Ellesmere pattern): one
  flat `Lychee Dev.toc` declaring every supported interface
  (`120100, 50504, 38002, 16001`) with `Core/ClientGate.lua` as the first
  load selecting the product profile from the running build at load time —
  replacing the four per-client TOC files and the Clients profile files.
  Real-machine acceptance on WoW Forever (Auto—Forever, 1.60.1.70009):
  connect, probe load/run(sum=55)/ack and hide all passed; retail untouched.
- Atomic live operations with a journaled state machine
  (`live probe put/load`, `live run`, `live ack`): every stage requires a
  nonce-correlated receipt, persists intent before effects and resumes by the
  original operation ID without replaying submitted input.
- `live abandon`: explicitly stop cleanup of a verified probe before ACK;
  preserves the report, retires only the exact disk queue entry and releases
  window ownership without game input (`cleanup=abandoned`, never claimed as
  ACK success).
- `live reload`: standalone correlated UI reload with runtime verification
  through a nonce-matched new receipt.
- `live bugs`: bounded capture of existing addon errors as a verified report.
- `live hide`: dismiss a displayed bridge receipt after its evidence is
  archived; refuses windows owned by in-flight operations and verifies the
  clear from valid frames.
- `live reset`: third fixed bootstrap command breaking the deadlock where an
  in-memory queue blocks identity after abandon or a lost receipt; actor-scoped
  tombstoning only, disk-owned windows refused, nonce-correlated receipt
  required.
- `live probe list/show/remove` registry for immutable, content-addressed
  bounded Lua probes.
- Desktop lifetime regression proving the WGC capture runtime stays loaded
  between sessions (`LYCHEEDEV_TEST_DESKTOP=1`).

### Fixed

- Byte fidelity across the QR boundary: `desktop.DecodeSymbols` preserves
  transmitted bytes by reading byte mode with ISO-8859-1, and the single
  conversion back to bytes now happens once, in
  `desktop.BytesFromSymbolText`. A non-ASCII identity (Chinese character or
  realm name) was previously re-encoded twice and every receipt comparison
  failed, which is what made Classic `50504` live runs stall after dispatch.
- Transport compression removed from `Bridge/CaptureWriter`: a raw DEFLATE
  stream is not valid UTF-8 and the host rejects a SavedVariables document that
  is not valid UTF-8, so one compressed receipt made the whole
  `LycheeToolkitDB` unreadable and forced the command output to print binary
  bytes into chat. The host still reads the archived `0x1F` transport, so a
  receipt drawn by an earlier build stays verifiable.
- `live abandon` releases a probe stalled at `flush_requested` when the client
  never persisted a report, using the same durable dispatch-input proof it
  already required at `dispatch_requested`; a window is no longer wedged with
  no legal recovery path.
- Native `0xc0000005` crash in capture sessions: the process now retains one
  MTA reference for its lifetime so `GraphicsCapture.dll` is never unloaded
  while a capture worker is still returning.
- Background input pacing restored to the proven 150 ms open / 50 ms per
  UTF-16 unit cadence, with per-message identity rechecks and reentry that
  waits for both load-end events in any order.
- ACK after a faults cleanup reload no longer fails in a fresh process: ack
  input preparation now observes current input readiness anchored on the
  archived reentry evidence, and a persisted zero-message receipt authorizes a
  safe resend.
- Load operations stop at `loaded` instead of overrunning into execution;
  report archival no longer depends on transient reentry displays.

### Changed

- The single flat manifest is the only manifest: the delivery contract requires
  one `Lychee Dev.toc` declaring every supported interface, and a per-client TOC
  variant is rejected by `tools/addon-package.mjs` and
  `delivery.InspectAddonRelease`. `tools/addon-package.mjs`, `tools/release.mjs`
  and `tools/version.test.mjs` were still built around four TOC files and are
  realigned.
- Release scope is Windows amd64 only; Retail `120100`, Classic `50504` and
  Titan `38002` are the acceptance clients. Forever `16001` source stays in
  the tree unverified.
- Skill orchestration documents `identity_busy` as the stuck-queue signature
  with `live reset` as the first-line recovery before any UI maintenance, and
  receipt dismissal as post-archive tidiness.

## [2.0.1] — 2026-09-24

Published as `lycheedev@2.0.1` (tag `v2.0.1`). Contract:
[release-2.0.1.md](docs/toolkit/release-2.0.1.md).

- First npm-published release on the 2.0 architecture: one Go CLI, one
  in-game Lua workbench, one agent skill, zero runtime npm dependencies.
- Windows amd64 only; non-Windows launcher refuses to run.
- `lycheedev describe --format json` as the only command directory, generated
  skill references checked against the contract table in CI.
- Results as `lycheedev.result.v1` envelopes with documented exit codes 0/2-8.

## [2.0.0] — 2026-09-21

Published (tag `v2.0.0`). Superseded publishing assumptions — five platforms,
interactive-desktop CI gates and four-client machine gates — were retired by
the 2.0.1 contract and never shipped.

- Initial 2.0 architecture: Go CLI + workbench addon + skill + npm
  distribution with a single version source in `release/version.json`.
- Legacy Python-delegation implementation (1.x) retired; its history remains
  in git and its user data is never imported.
