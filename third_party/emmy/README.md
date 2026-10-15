# EmmyLua reconstruction v1

This is a **new** dependency patch series based on the public EmmyLua commit
`08b0873962e83bb8307e9a81052e3f41578fbc78` (MIT). It does not recover or claim
byte equivalence with the missing historical private BindingContext fork.

The complete archive, including upstream license and sources, is authenticated
before extraction. Tracked patches are applied in listed order. Preparation
verifies every final file against the tracked inventory. No upstream programs
or configuration are executed by preparation. The generated source is a build
input and must be included in corresponding-source distributions with its
identity receipt and this patch series; a network URL alone is not sufficient.

Preparation: `python3 third_party/emmy/prepare.py --archive /path/base.tar.gz`.
The default destination is `.tmp/emmy-reconstruction-v1/upstream`. An existing
destination is verified, never silently overwritten. `--download` explicitly
fetches the pinned archive to the supplied archive path when it is absent.

Implementation status and integration gates are recorded by the project's sole
current authority, `handover.md`. Availability of these APIs is not a claim that
all historical binding semantics or full-source acceptance are restored.

## Owned binding contracts

`crates/lycheedev-binding-contracts` contains the engine-independent evidence
records formerly defined inside the patch. It depends only on serde/json/std;
Emmy re-exports its exact types, while the owned adapter depends on it directly.
Engine syntax IDs, SemanticId, LuaType, session isolation and inference/checker
hooks remain in Emmy. This does not remove the enhanced Lua/XML semantics.

From `upstream/crates/emmylua_code_analysis`, the dependency path climbs exactly
five parents to the source snapshot root, then enters
`crates/lycheedev-binding-contracts`. It never points outside that snapshot.
The recipe identity authenticates every owned-crate file as `ownedFiles` plus
`ownedTreeSHA256`; Python preparation and JS build/stage verification reject
missing, changed or unexpected files. Sealing updates both inventories.
Standalone staging includes those files and a minimal local workspace manifest;
full corresponding-source staging preserves the product manifest and includes
the same crate. Both root and upstream Cargo.lock record this path dependency.
The new crate is covered by the owned unsafe-code gate.
