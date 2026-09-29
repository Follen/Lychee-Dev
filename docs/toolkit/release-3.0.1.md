# Lychee Dev Toolkit 3.0.1 release contract

Date: 2026-09-29. The owner requested npm **3.0.1**, while keeping npm
`latest` on **2.5.1**. Publish 3.0.1 under **`next`**. Do not temporarily move
`latest`, promote the new package after publication, or change existing bytes.
The version remains 3.0.1, not an `-rc` version. The GitHub release must not
replace the repository's latest release when publishing this channel.

## Scope and evidence

This release includes the main branch's Source/Data improvements, revised
Skill entry and live workflows, native connection recovery, hybrid input
readiness, memory lookup improvements, and 200-slot forward allocation for
instances sharing one installation. Earlier main-branch work is retained.

The [200-slot acceptance record](live-slot-routing-acceptance-2026-09-29.md)
records Retail 12.1.0.69933 dual-instance testing: reserved-slot skipping,
original-request recovery, 94 actor-tagged tasks, capacity reload in both
instances, peer isolation, and zero outstanding reservations/window owners
after disconnect. The complete pre-version-bump offline baseline passed.
That real-client run used the preceding 3.0.0 development candidate; it is
not a claim that the final 3.0.1 tag was independently retested in game.
Classic, Titan and Forever's new 200-slot live cases remain `not_run`.

The version-synchronized local baseline also passed at
`.tmp/release-3.0.1/offline/report.json`: all eight checks, 42 Go packages,
2,572 Go/Lua entries, zero failures (conditional skips remain recorded), and
the Node/version/Skill checks. It used mandatory Lua 5.1 and the pinned LuaLS
runtime with `GOFLAGS=-p=2`; only this evidence note was added afterwards.
Remote CI and registry publication are separate gates, not implied by this run.

Versioned Skill sources and payload are included. Updating this release does
not authorize uncertain input replay, clearing foreign reservations, or
treating on-disk installation as proof of game runtime activation.

## Immutable publication sequence

1. Synchronize `release/version.json` through `tools/version.mjs --write`;
   keep command/Skill contracts and generated references consistent.
2. Pass build/vet, full uncached mandatory Lua 5.1 Go tests and the offline
   functional baseline with pinned LuaLS. Commit the exact release tree.
3. Push immutable `v3.0.1`. The official workflow must pass all required
   Windows CI jobs, process/race/addon coverage and CGO=0 checks at that commit.
4. Assemble once, compare the corresponding-source rebuild, test the isolated
   `--ignore-scripts` install and Windows binary, and seal digests.
5. Verify the channel policy and current registry `latest=2.5.1`, then publish
   the sealed tarball to `next` using OIDC, without any token fallback.
6. Verify registry name/version/integrity/provenance and isolated installation;
   verify `next=3.0.1` and `latest=2.5.1`, then create the GitHub release and
   upload the sealed artifacts. A triggered workflow alone is not completion.

If the version already exists with different bytes, or the protected tag has
changed, stop rather than overwriting or repairing it automatically. Published
tarballs and tags are immutable. Keep the historical 3.0.0 and older release
contracts as records of their original publication decisions.
