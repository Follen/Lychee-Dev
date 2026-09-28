# Lychee Dev Toolkit 3.0.0 release contract

Date: 2026-09-28. The owner requested merging `codex/nonce-memory-transport`,
publishing npm and updating the local installation. Published 2.5.1 is immutable;
3.0.0 synchronizes the CLI, addon and npm package through `release/version.json`.

The owner explicitly selected major version 3.0.0 after the unpublished 2.5.2
candidate. Its package smoke exposed modified-addon removal returning invalid
input instead of an installation conflict. The error contract is restored and
covered by a regression that also verifies no removal side effects.

## Scope

Native memory/slot connections, 64 managed slots, project-local recovery,
input-state coordination, shared-installation race fixes, automation history,
workbench revisions and the corresponding Skill workflows. The architecture
review's three proposed future refactors are not part of this release.

## Acceptance

- Pass build/vet and the uncached full Go suite with mandatory Lua 5.1; run the
  functional baseline with the pinned real LuaLS runtime and all Node tooling
  tests, including isolated npm update tests. Skill contracts and generated
  references, source inputs and version consistency must pass.
- The exact tagged commit must pass all required Windows CI jobs, including
  race detection, OS-process ownership, addon fixtures and package smoke.
- Retain the real-client evidence in
  [shared-installation races](live-input-shared-installation-2026-09-28.md),
  [Classic and Forever input coordination](live-input-multiclient-2026-09-28.md),
  [Classic history](classic-input-history-2026-09-28.md),
  [Titan/Forever experiments](live-memory-slot-clients-2026-09-28.md) and
  [workbench audit](workbench-audit-2026-09-28.md). These records describe the
  specified candidate code/builds, not new runs on the release tag. Local
  installation verification is recorded separately after registry publication.

## Explicit limits

Historical OP/BTP compatibility remains. Total project-history loss is not
transparent recovery and must not authorize uncertain input replay. Forever is
an experimental specified-build result, not an expansion of the supported
acceptance matrix. Long-running/arbitrary-concurrency cases remain unverified.
The observed 9.8-105.4 second paired-query range is not a latency guarantee.
The historical bootstrap test's transient EOF is retained in its evidence;
passing a rerun does not establish its root cause.

## Publication and installation

Use the immutable Windows-amd64 pipeline from [2.5.0](release-2.5.0.md):
clean tagged commit `v3.0.0`, required CI, single assembly and pack,
corresponding-source rebuild comparison, isolated `--ignore-scripts` package
smoke, sealed digests, OIDC publication without token fallback, registry
integrity/provenance read-back, and GitHub Release assets. npm dist-tag is
`latest`. Never replace a published package or move a published tag.

After publication, use the managed `update` workflow for the local CLI, Skill
and discovered addon targets. Verify version, commit and target receipts.
Filesystem completion and game runtime activation remain separate facts;
do not claim that an online client has loaded the update from disk checks alone.
