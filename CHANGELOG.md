# Changelog

All notable changes to the Lychee Dev Toolkit are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/) with
version entries matching the single version source `release/version.json`.
Detailed acceptance records and evidence live in
[docs/toolkit/implementation-status.md](docs/toolkit/implementation-status.md);
per-release publishing contracts live in `docs/toolkit/release-*.md`.

## [Unreleased] — 2.0.2 candidate

Nothing here is published yet. 2.0.2 must not be claimed releasable before the
static gates, Windows CI, release assembly and registry read-back in
[release-2.0.2.md](docs/toolkit/release-2.0.2.md) are complete.

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
