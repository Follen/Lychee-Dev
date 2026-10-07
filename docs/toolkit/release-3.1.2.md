# Lychee Dev Toolkit 3.1.2

On 2026-10-08 the owner authorized publishing the merged PR #11 fixes and
updating all local Lychee Dev addons. Publish 3.1.2 under npm `next`, preserving
`latest=2.5.1` and GitHub latest `v3.0.0`. Published 3.1.1 remains immutable.

This patch recognizes China Forever installations through `_cn_beta_`,
`wow_cn_beta` metadata and matching 1.60.1 builds. Local target resolution
retains the observed regional launcher slot. Existing international live
connections without catalogProduct retain their original known slot identity;
regional and other identity changes remain rejected. No addon behavior or live
protocol changes are added; release identities and fixtures advance together.

Local installation preflight also exposed an activation cleanup defect:
`live disconnect` returned `live.channel_journal_missing` for a prepared,
unbound activation with recorded `not_sent` input. The patch adds input-free
cancellation through the normal disconnect path, preserving evidence and exact
driver/owner guards. Submitted, uncertain and runtime-selected activations
remain protected; cancellation must be durable and repeatable.

The release starts at merged main commit
`1b8fb7a220654d96a3a059563b72e5f9cb0b8988`. Uncommitted work in the owner's
primary checkout is excluded. Run build, vet and the complete uncached offline
baseline with mandatory Lua 5.1 and pinned LuaLS before the release commit.
Prior PR #11 local discovery results are recorded in implementation-status.md;
they do not establish new game activation or input acceptance.

The initial version-only baseline passed (43 Go packages, 3,065 named results,
40 explicit optional/helper skips, 100 Node tests). The activation cleanup fix
requires its own regression coverage and a fresh complete baseline before
commit; the earlier result does not validate the later code change.

The corrected CLI was exercised on the original local activation
`CON-a5cc5c5ecb2ab154d4ca7b809771e526` in its original ipc-test project.
Disconnect returned `activation_cancelled`, `closed=true`, `complete=false`
and an unavailable business report, preserving the honest activation outcome.
No game input was sent. This verifies host-side cancellation only, not gameplay,
new runtime readiness or general cross-client live behavior.

Push the verified release commit to main, wait for required Windows CI, then
push immutable `v3.1.2` at that clean commit. The existing toolkit-release.yml
pipeline owns required CI, CGO verification, single assembly, corresponding
source rebuild comparison, isolated --ignore-scripts installation, Windows
execution evidence, sealed digests, OIDC publication, registry read-back and
GitHub Release with latest=false. No token fallback or local repacking.

Verify registry identity, integrity and provenance, next=3.1.2, latest=2.5.1,
and unchanged GitHub latest after publication. Install the exact official
registry package with scripts disabled, then use its matching release root
and managed addon installer for all discovered local installations. Check
each target's identity, ownership, claims and file integrity before replacing
it; retain unresolved or foreign ownership rather than clearing it.

Filesystem completion is separate from runtime activation. This request does
not require game input, a client restart or a character switch. Report updated
files and any still-loaded older runtime separately.
