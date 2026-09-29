# Lychee Dev Toolkit 3.0.2 release contract

Date: 2026-09-29. The owner requested merging the content-variant fix into
main, publishing npm **3.0.2**, preserving npm **latest=2.5.1**, then updating
this machine from the official registry. Publish under **next**; never move
latest temporarily. Keep the GitHub latest release unchanged. AGENTS.md
records this as the continuing policy unless the owner explicitly changes it.

## Scope and evidence

Retain every existing main-branch change. Add explicit
`--content-variant standard|low-violence` to asset inspect/export, filtering
only Root LOW_VIOLENCE (`0x80`). Default selection remains strict; missing
variants do not fall back, and remaining ambiguity is still rejected.
Selection evidence and all normal CASC/output integrity checks remain intact.
The versioned Skill documents the new argument and its boundaries.

[Content-variant acceptance](content-variants-2026-09-29.md) records the
fixed Forever US/zhCN 1.60.1.70009 PIN: 153 IDs, 306 verified exports, zero
export errors, with independent CKey and SHA256 checks. The navigation task
separately found 102 identical collision pairs and 51 different pairs.
The active client's overrideArchive remains unknown; locale is not proof.

The pre-version-bump complete offline baseline passed at
`.tmp/model-variants-20260929/offline/report.json`: 42 Go packages, mandatory
Lua 5.1, 94 Node tests, version and Skill checks, with conditional skips
recorded separately. Version-synchronized acceptance must also pass before
committing the release tree; its output is `.tmp/release-3.0.2/offline/`.
No static model export or fixture result is claimed as new live-game acceptance.
Existing live acceptance retains the client/version limits in the 3.0.1 contract.

## Publication and local installation

1. Synchronize release/version.json using tools/version.mjs; update the
   version-bound npm-channel.json to next with preserveLatest=2.5.1.
2. Pass build/vet, full uncached mandatory Lua Go tests and the complete offline
   baseline. Commit the exact tree and merge into main while retaining main's work.
3. Push main and immutable v3.0.2. The official tag workflow gates publication
   on Windows CI, addon/process/race coverage, and pure-Go capture/decode checks.
4. Assemble once, compare corresponding-source rebuilds, verify isolated
   --ignore-scripts install and Windows execution, seal and recheck digests.
5. Verify latest=2.5.1, then publish the sealed tarball to next using OIDC.
   No token fallback, manual re-pack, or replacement of published bytes/tags.
6. Verify registry version/integrity/provenance, next=3.0.2 and latest=2.5.1,
   then complete the GitHub release with sealed assets and --latest=false.
7. Install exact lycheedev@3.0.2 from https://registry.npmjs.org with scripts
   disabled. Verify the installed release identity and synchronize managed
   Skill/addon targets using that official package's offline update path.
   Do not invoke the unqualified online updater, which follows protected latest.

Filesystem installation does not prove that running games or agent contexts
loaded the new payload. Report runtime activation separately; do not send game
input as part of this installation-only request.
