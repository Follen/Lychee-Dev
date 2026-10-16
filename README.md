# Lychee Dev Toolkit · Rust migration

[简体中文](README_zhCN.md)

This branch, `rust-refactor`, is an **unfinished Rust implementation of the tagged Go 3.2.3 product contract**. It is not a release or a claim of functional/performance parity.

The unpublished Rust candidate version is **3.5.0**, with **beta** distribution configuration. This is a package identity separate from the fixed Go 3.2.3 behavior oracle; no release or npm dist-tag operation has been performed.

Final acceptance now requires **unmodified Emmy upstream source**: all current patches must be replaced by equivalent owned-crate/public-API behavior while retaining the complete XML/Lua enhancement. The current source candidate now selects the unmodified upstream tree and runs contextual queries and diagnostics in the owned adapter. Compilation, complete semantic differentials, full Retail and performance acceptance remain pending.

## Goal and fixed baseline

The behavioral oracle is **v3.2.3**, peeled tag commit **`3766f9cacb52e93a803a626187e2a1d54a445468`** ([release source](https://github.com/Follen/Lychee-Dev/blob/3766f9cacb52e93a803a626187e2a1d54a445468/README.md)). Do not use a moving `main`, Go 3.1.2, an arbitrary development binary, or later fixes as the oracle.

Prior v3.2.2 (`0bec60e4406f5d5dc155ee25cef44f9a710e37e8`), v3.2.1 (`10b561db5706702cd42e047eaeed4ccb5e825351`) and v3.2.0 (`3057a9666ab078270b63c7f06de748069b0d1f4b`) fixtures and results retain their identities. The cumulative [3.2.1](docs/toolkit/rust-go-3.2.1-increment.md) and [3.2.2](docs/toolkit/rust-go-3.2.2-increment.md) requirements remain. The new [3.2.3 increment](docs/toolkit/rust-go-3.2.3-increment.md) covers admission-scoped adaptive Data memory, compact WDC records and safe indexed IN selection, with updated Data recovery guidance. Its Rust acceptance is pending. The exact tag/source is verified; release binaries/assets are not verified.

Reuse the existing Rust work where differential evidence supports it. Keep, replace or complete each module against Go 3.2.3; do not start over indiscriminately or preserve an obsolete Rust protocol merely because it already exists.

Only the host implementation language and Source analysis engine change: Rust plus **EmmyLua's direct analysis libraries, native worker and session ownership**, replacing LuaLS. All other business behavior, the same Lua addon, public CLI, versioned Skill, delivery and complete **Lycheedev mailbox protocol v1** remain equivalent to the pinned release.

Performance acceptance means at least **70% less controllable elapsed time: Rust ≤ 30% of Go** on every agreed required path, with equivalent results on the same machine/data. Download time and uncontrollable external waiting are reported separately. Cold/warm samples are independent. This is a target, not a measured result.

## Read these first

- [handover.md](handover.md): the only current migration status/decision/evidence source; human-assisted tests are its final section.
- [DESIGN.md](DESIGN.md): target architecture and compatibility rules, derived from that authority.
- [todo.md](todo.md): ordered implementation/acceptance checklist, no independent status narrative.
- [Pinned Go protocol](https://github.com/Follen/Lychee-Dev/blob/3766f9cacb52e93a803a626187e2a1d54a445468/docs/live-protocol.md): the full wire, recovery, input and diagnostic contract.

Older `docs/toolkit/` plans and test reports are historical evidence. Their Go 3.1.2 goals, 50% threshold, four-slot exact-COMMIT redesign and old input assumptions do not override this migration.

## Current implementation and build boundary

Nine crates exist under `crates/`: cli, core, source, data, live, delivery, windows, emmy-adapter and binding-contracts. Source uses the owned adapter and engine-neutral contracts; the fixed upstream engine does not depend on those contracts. Source defaults to `emmy-prototype` and links the unmodified EmmyLua commit `08b0873962e83bb8307e9a81052e3f41578fbc78` directly. The verified tree is exactly 848 upstream files; [third_party/emmy](third_party/emmy/README.md) records source, archive and license identity with no production patches. Owned algorithms retain the upstream MIT attribution. The preparation recipe materializes the ignored dependency directory. A separately fixed historical patched tree is retained only for reference tests, never as the production dependency. Source identity checks are not compilation or behavioral acceptance.

Successful component runs do not establish full Source, default Live, package or performance acceptance. Full Retail native analysis has exposed real generated-API failures; new fixes require execution on their exact candidate. Formal runtime/input/backend components are connected to the default public lifecycle; exact-candidate acceptance remains open. Use [handover.md](handover.md) for exact passed/failed/not-run evidence and [the current module queue](todo.md#current-module-work-queue-derived-from-handover-no-percentage-claims) for remaining work. Independent reproducible-build and final package gates remain open.

Windows amd64 remains the product platform. Source/Data/assets do not require a running game. A running selected client is needed only for final Live acceptance.

## Work policy

The initial change was documentation only; full implementation of the plan is now in progress. No game input, publication or main merge is authorized. Implementation proceeds through offline tests and automated Windows checks first; tests needing the user, a real game, or a specific interactive machine run last. Each authorized batch updates the same dedicated draft PR. After implementation, perform the five sequential convergent audit stages specified in handover. Preserve historical failures and artifacts, and delete dead code only after call-site, feature, package and regression evidence proves it is unused.

Released installation instructions belong to the [pinned Go README](https://github.com/Follen/Lychee-Dev/blob/3766f9cacb52e93a803a626187e2a1d54a445468/README.md). This branch must not present an npm install as installation of the unfinished Rust candidate. Prior branch README and UI design text are preserved in [docs/history](docs/history/).
