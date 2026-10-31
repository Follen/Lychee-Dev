# Changelog

Notable changes to Lychee Dev. Version entries match the single version source
[release/version.json](release/version.json). The current development baseline is
**3.5.0**.

## [3.5.0]

Current product baseline: Rust CLI, existing Lua addon, first-party Agent Skill
and npm package for Windows amd64. This entry describes the current baseline; it
is not a published-release acceptance record.

### Changed

- The native CLI is implemented in Rust. A single command catalog
  (`crates/lycheedev-cli/src/commands.json`) drives parsing, `--help`, `describe`,
  `schema`, `cli search`, examples and execution, so discovery and behavior share
  one contract.
- Source analysis runs on the fixed, unmodified EmmyLua Analyzer Rust upstream
  plus the owned adapter. Managed Lua 5.1 `luac` performs syntax checks; binary
  Facts/Index caches and content-addressed captures preserve fixed source identity
  and evidence.
- Data has one Engine → DataFusion execution path behind a caller-owned
  `data agent serve` session. Typed tasks and generic queries share the data
  source, exact numeric semantics, result handles, paging, export and CAP; asset
  search/inspect/export/demux and independent Hotfix sources use the same public
  command catalog.
- Live uses the existing Lua addon and Lycheedev mailbox protocol v1: verified
  process/runtime binding, the existing 500-slot managed installation, durable
  request and recovery journals, verified reports and cleanup, and no replay of
  submitted or unknown input.
- Delivery manages addon and Skill installation, update transactions and removal
  from a release package, and hands npm CLI updates to the packaged launcher;
  `update` synchronizes the npm CLI and managed targets.
- The Agent surface is a minimal first-party Skill with the current product/build
  quick reference and versioned CLI discovery; command parameters, schemas and
  examples come from the CLI instead of a second prose manual.
- Update discovery, registry selection and candidate admission use the embedded
  channel authority. A newer release in the same channel remains eligible;
  the Skill carries no toolkit version or channel selection table.
- Command errors expose the recognized command's correction metadata, required
  flags match typed admission, and Data Agent discovery describes actual response
  frames, result handles, exact values, paging, exports and captures.
- Distribution is the npm package plus the native Windows amd64 payload. The npm
  package contains the native CLI, the addon and the Skill; the version and npm
  channel configuration are defined by `release/version.json` and
  `release/npm-channel.json`.

### Verification and limits

- This baseline does not imply release, package-install, performance or
  real-client acceptance. Actual verification results are recorded in
  [handover.md](handover.md); open work is tracked in [todo.md](todo.md), and test
  coverage in [test.md](test.md).
