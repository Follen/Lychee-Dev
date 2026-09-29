# Local installation selection acceptance

2026-09-29, working tree, unpublished. The reported original caller path is still
unknown. An isolated regression independently reproduced the same empty-product
error by passing a game root instead of its sole Titan client directory.

## Contract and implementation

- `records.DiscoverClientInstallations` observes one client or bounded immediate
  children of a root; `SelectClientInstallation` applies the requested product.
  Existing `InspectClientInstallation` callers use that same selector.
- Active catalog/version contradictions fail. Catalog rows without an actual
  client directory do not create candidates. Explicit client selection does not
  search siblings. Root discovery cannot select a redirected client outside it.
- Known foreign flavors can be excluded by a product constraint. Unknown identity
  and matching-product conflicts remain blocking; a parameter never overrides
  observed metadata. This matters on the real multi-product root, which also has
  unsupported Beta/Anniversary/Era clients.
- `target resolve --installation <root-or-client> --product titan ...` is local;
  product-only remains remote. Named local targets pass their product/build
  constraints through the same resolver. Local failures never switch transport.
- Structured errors distinguish `selection.installation_missing`,
  `selection.installation_ambiguous`, `selection.installation_product_mismatch`
  and `selection.installation_metadata_conflict`; selection observations are in
  `context.installationSelection`. Identity error compatibility via `errors.Is`
  is retained for internal callers.
- Addon deployment resolves to the selected client path. Live window selection
  uses the shared candidates plus actual executable directory and optional PID;
  multiple processes remain separate. Runtime actor/nonce checks are unchanged.

## Regression mapping

| Case | Evidence |
| --- | --- |
| Root and child identify the same Titan | `TestInstallationRootAndClientResolveSameTitan`; failed before fix with `unsupported product ""`, now passes |
| Multiple products and explicit mismatch | `TestInstallationDiscoveryDoesNotInventOrOverrideClients/multiple`; candidates retained, child cannot retarget to sibling |
| Catalog remnants / empty directory | `catalog-only`, `empty-folder`; no fabricated installation |
| Missing flavor or version | `missing-flavor`, `missing-version`; use remaining verified evidence |
| Active catalog/version contradiction | `conflict`; does not override observed build |
| Unsupported sibling vs unidentified sibling | `foreign-unsupported`, `unknown-sibling`; only known unrelated flavor excluded |
| Local root/child return same pin; no remote fallback | `TestTargetResolveFromClientReturnsReusablePinAndEvidence`, including product mismatch and missing local path |
| Shared Data and changed selected client | `TestClientDataReadChecksSelectedClientBeforeArchives` |
| Product-only remote selection remains remote | `TestRemoteTargetOfflineNeedsObservation` and existing remote target suite |
| Addon identity errors stay actionable | `TestAddonStatusUsesClientIdentity`; metadata failure includes structured observations |
| Root plus PID selects exact client; no PID stays ambiguous | `TestDiscoveredClientWindowGameRootUsesPIDWithoutRetargeting` and existing window-selection suite |
| Root must not select a redirected foreign client | `TestInstallationDiscoveryDoesNotFollowForeignClient`; **skipped on this machine** because Windows denied directory-symlink creation; do not count as passed |

## Real local observation

Candidate `.tmp/lycheedev-installation.exe`; isolated home
`.tmp/install-selection-real-20260929`. Both offline target commands used product
Titan, region cn, locale zhCN and explicit definitions commit
`61b81d1e518fd276aa879e662c53393ec135681f`:

- Game root: `D:/Game/World of Warcraft`.
- Client: `D:/Game/World of Warcraft/_classic_titan_`.
- Both returned `wow_classic_titan`, `3.80.2.69874`, shared game root and
  `PIN-a9e5a7b57f2b9f6bdcf4e0f19d90a92700f921000b73946dc1f9129fd01fc18e`.
- Raw envelopes: `.tmp/installation-titan-root.json`,
  `.tmp/installation-titan-child.json`.

No game input, installation writes, project-lock edits or publication occurred.
This verifies local identity/configuration resolution, not DB2 payload availability,
actual live connection, or independent Agent behavior. Those are separate scopes.

Final checks passed: `go build ./...`, `go vet ./...`, full
`LYCHEEDEV_REQUIRE_LUA51=1 go test -p 4 -count=1 ./...`, targeted installation/
command/live/delivery tests, generated command reference, skill validation,
87-command/210-reference skill contract, both Node contract-tool tests and
`git diff --check`. Final full log: `.tmp/installation-selection-final.log`.
The symlink test and other environment-gated integrations retain their skips.
