# Lychee Dev Toolkit 2.5.0 release contract

Date: 2026-09-28. The owner requested npm version **2.5.0**, including the
in-game addon. This release includes the accumulated Source, Data, live bridge,
workbench and Agent skill changes since 2.0.6. Published tags and packages remain
immutable; the earlier release contracts are retained as historical records.

## Scope

- Source research uses pinned Git worktrees and a bundled, checksum-pinned
  Lua Language Server. Semantic context, references, relations, validation and
  bounded flow analysis share explicit source/environment identities.
- Data investigation adds complete Hotfix paging, explicit key handling,
  partial-coverage reporting, effective-record provenance and SQL improvements.
- Agent investigations use correlated receiver transactions, immutable probe
  budgets, durable ownership, character-scoped reports and verified completion.
  Proven pre-commit or zero-message input can recover within the original
  operation; uncertain business input is never replayed.
- The workbench shares rounded controls, readable page layouts and fixed
  shortcut displays. Connecting blocks game input; running probes release input
  and retain a small Lychee activity indicator until final cleanup.
- Versioned Agent guidance, offline regression baselines and real-client probes
  are included with the corresponding source.

## Version and platform

`release/version.json` is the sole version source. `node tools/version.mjs
--write` synchronizes npm package/lock, CLI build info, addon TOC/runtime and
version-sensitive protocol fixtures to 2.5.0. The release tag is `v2.5.0` and
the npm dist-tag is `latest`. The addon ZIP and npm payload use identical bytes.

Windows amd64 is the only shipped platform. Retail 120100, Classic 50504 and
Titan 38002 remain the acceptance matrix; retained Forever paths are unverified.
The current Retail UI/bridge evidence is documented in
[UI acceptance](ui-polish-2026-09-28.md). It does not establish unrun multi-client,
multi-instance, IME, scaling or long-running acceptance.

## Required publication sequence

1. Synchronize versions, generated command references and skill contracts. Pass
   build/vet, the full Go suite with mandatory Lua 5.1, Node tooling and source
   input/license checks. Freeze all changes in a clean Git commit.
2. Push the immutable tag. The release workflow runs all required Windows jobs,
   race detection and the pure-Go capture/decode pre-step for that exact commit.
3. Assemble once: native CLI, bundled LuaLS, addon, skill, release manifest,
   corresponding-source archive and offline rebuild comparison, addon ZIP and
   the npm tarball. Verify a clean isolated install and Windows execution.
4. Seal and recheck digests, publish that same tarball with OIDC and no token
   fallback, verify npm name/version/integrity/provenance, perform a registry
   install check, and create the GitHub Release with the sealed assets.

The existing workflow uses `--ignore-scripts` only for isolated distribution
verification. This is not required in user-facing installation commands.
Publishing succeeds only when the workflow and registry read-back succeed;
triggering CI or pushing the tag is not completion.

The immediately preceding worktree baseline is
`.tmp/ui-polish-offline-d/report.json`: 2,099 Go test entries, 78 Node tests,
20 fixed regression groups and real LuaLS integration passed, with 31 optional
environment/helper entries skipped. The versioned release reruns the full
baseline; local release evidence is in `.tmp/release-2.5.0-final/report.json`.
