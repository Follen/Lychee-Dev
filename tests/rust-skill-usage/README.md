# Independent installed Skill workflow qualification

The current workflow uses the fixed Go 3.2.3 semantic contract at
`3766f9cacb52e93a803a626187e2a1d54a445468`. Earlier Go320/321/322 fixtures,
reports and source manifests remain immutable historical evidence. New Rust
Skill documentation has its own independently fixed 25-file manifest and real
Git source anchor; it is not byte-identical Go documentation or a relabeled old
Rust guide. See `skill-identity.mjs` and `baselines/` for explicit identity lanes.

## Current entry points

- `tools/rust-skill-usage.mjs <artifact> <candidate>` retains the historical
  staged package mode: it copies authenticated candidate bytes to an isolated
  synthetic npm prefix and runs the Skill launcher discovery path. This is
  not an npm installation acceptance result.
- `tools/rust-skill-usage.mjs --installed <artifact> <candidate>` reads
  `npmSmoke: passed` and `installedRoot` from the authenticated candidate
  report produced by `tools/rust-package-smoke.mjs`. It verifies the existing
  installation equals the candidate package, EXE and independently fixed
  25-file Skill, then runs the same three full-Retail Source tasks directly
  through `installedRoot/bin/lycheedev.mjs`. It does not copy, install, download
  or build a package. The Source workflows retain their existing fixed-source
  sync and all original window, native-callsite and semantic assertions.
  Reports identify the installation path, candidate-report digest and
  `entryKind: installed-npm-package-launcher`; the Skill prefix discovery path
  is explicitly `not_run` in this mode. Its static Skill identity check and the
  separate staged discovery result do not become one complete Skill launch
  acceptance. Both modes retain the six no-home discovery checks. Old reports
  remain historical and are not relabeled as installed runs.
- `tools/rust-skill-usage.mjs --data ...` is the actual supervised typed Agent
  Data workflow. See [the current Data protocol](data-README.md) for its explicit
  three-target fixture preparation, production importer, exact values/CAP and
  owner-backed replay. It never defaults to the retired SQL CLI.
- `tools/rust-skill-trial.mjs --historical-oracle-only ...` is deliberately the
  fixed old-product 86f trial. It is **not** a current Agent blind trial or
  current acceptance route. Its old inputs/results cannot qualify this product.

No tests, compilation, checking, native operations or CI were executed during
reconstruction. These commands are preparation documentation, not permission to
run before final unified authorization. Actual Windows work remains in the
single owner-coordinated queue, default stack, 600-second per owned group,
locked/offline Cargo and `-j 1`, with no one-shot and no ad-hoc child cleanup.

## Acceptance boundaries

Source and Data controllers verify matching current artifacts/package/source,
installed native and launcher bytes, complete Skill inventory, explicit owned
process receipts and their original stdout/stderr. Source passing does not imply
Data, Live, memory-pressure or full Skill acceptance. Static documentation and
checker negative tests are not real business observations. A skipped required
feature target or historical oracle result is not a new-architecture pass.

The four Go323 incremental obligations remain separate: Live held-key recovery,
Live slot/reload continuity, Data exact numeric/query evidence and real pressure
recovery. Pure discovery must not read caches, initialize workers or create an
explicit/default home. Live interactions still need an authorized actor and
actual INPUT/CON proof; this harness never operates the game implicitly.

Performance follows correctness. Current paired collectors preserve complete
semantic/CAP/asset comparisons and explicit timing boundaries; CSV has a genuine
Agent Export correctness path, not a fabricated Go JSON-versus-CSV ratio. Fixed
old reports, benchmark contracts and blind-trial outputs remain historical.
