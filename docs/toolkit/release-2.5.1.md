# Lychee Dev Toolkit 2.5.1 release contract

Date: 2026-09-28. The owner requested 2.5.1 with the unified updater, a complete
Skill orchestration audit, changelog and isolated tests before npm publication.
The CLI, npm package and in-game addon share `release/version.json`.

## Required acceptance

- Update real npm packages in an isolated prefix against a loopback registry,
  using two native versions, a managed Skill and two simulated game clients.
  Verify conflict preflight, same-version retry, obsolete-file removal,
  interrupted coordinator recovery, prefix ownership and SavedVariables
  preservation. Never use the developer's global prefix or actual game files.
- Native replacement tests must cover read-only planning, all-target preflight,
  interrupted old-tree movement, modified/unmanaged rejection, path aliases and
  transaction placement outside discovery on the same target volume.
- Full Go tests require Lua 5.1; build, vet, race, Node tests, generated Skill
  command/link checks and the real bundled LuaLS baseline must pass.
- Audit every shipped Skill reference against the command contracts and current
  runtime. Static scenario review is not independent Agent execution or new
  real-client acceptance; retain those limits explicitly.

## Publish

Use the same sealed Windows-amd64-only pipeline as
[2.5.0](release-2.5.0.md): clean immutable tagged commit, required Windows CI,
single assembly/pack, corresponding-source rebuild comparison, isolated package
smoke, sealed digests, OIDC publish without token fallback, registry integrity
and provenance read-back, then GitHub Release assets. Tag `v2.5.1` maps to npm
`lycheedev@2.5.1` on `latest`. Published artifacts and tags are never replaced.

The updater has filesystem acceptance only. Existing Retail evidence remains
in [UI acceptance](ui-polish-2026-09-28.md); this release does not upgrade unrun
multi-client, multi-window, IME, relogin or manual visual cases to passed.
