# Lychee Dev Toolkit · Rust migration

[简体中文](README_zhCN.md)

This branch, `rust-refactor`, is an **unfinished Rust implementation of the released Go 3.2.0 product contract**. It is not a release or a claim of functional/performance parity.

## Goal and fixed baseline

The behavioral oracle is **v3.2.0**, peeled release commit **`3057a9666ab078270b63c7f06de748069b0d1f4b`** ([release source](https://github.com/Follen/Lychee-Dev/blob/3057a9666ab078270b63c7f06de748069b0d1f4b/README.md)). Do not use a moving `main`, Go 3.1.2, an arbitrary development binary, or later fixes as the oracle.

Reuse the existing Rust work where differential evidence supports it. Keep, replace or complete each module against Go 3.2.0; do not start over indiscriminately or preserve an obsolete Rust protocol merely because it already exists.

Only the host implementation language and Source analysis engine change: Rust plus **EmmyLua's direct analysis libraries, native worker and session ownership**, replacing LuaLS. All other business behavior, the same Lua addon, public CLI, versioned Skill, delivery and complete **Lycheedev mailbox protocol v1** remain equivalent to the pinned release.

Performance acceptance means at least **70% less controllable elapsed time: Rust ≤ 30% of Go** on every agreed required path, with equivalent results on the same machine/data. Download time and uncontrollable external waiting are reported separately. Cold/warm samples are independent. This is a target, not a measured result.

## Read these first

- [handover.md](handover.md): the only current migration status/decision/evidence source; human-assisted tests are its final section.
- [DESIGN.md](DESIGN.md): target architecture and compatibility rules, derived from that authority.
- [todo.md](todo.md): ordered implementation/acceptance checklist, no independent status narrative.
- [Pinned Go protocol](https://github.com/Follen/Lychee-Dev/blob/3057a9666ab078270b63c7f06de748069b0d1f4b/docs/live-protocol.md): the full wire, recovery, input and diagnostic contract.

Older `docs/toolkit/` plans and test reports are historical evidence. Their Go 3.1.2 goals, 50% threshold, four-slot exact-COMMIT redesign and old input assumptions do not override this migration.

## Current implementation and build boundary

Seven crates exist under `crates/`: cli, core, source, data, live, delivery and windows. Source defaults to `emmy-prototype` and links the EmmyLua analysis libraries directly. Its dependency paths currently refer to an ignored `.tmp/rust-source-binding-context-01/upstream` tree. The tracked checkout alone does not contain that modified dependency's complete bytes; a reproducible build requires recovering and pinning its source and patch inventory first. This does not imply the owner's local dependency files are lost.

Do not advertise a clean checkout build, full Source success, Live readiness, or a validated replacement package yet. See the exact gaps in handover. Windows amd64 remains the product platform. Source/Data/assets do not require a running game. A running selected client is needed only for the final Live acceptance.

## Work policy

This first change is documentation only; the user subsequently authorized full implementation of the plan. No game input, publication or main merge is authorized. Implementation proceeds through offline tests and automated Windows checks first; tests needing the user, a real game, or a specific interactive machine run last. Each authorized batch updates the same dedicated draft PR. After implementation, perform the four sequential convergent audit stages specified in handover. Preserve historical failures and artifacts, and delete dead code only after call-site, feature, package and regression evidence proves it is unused.

Released installation instructions belong to the [pinned Go README](https://github.com/Follen/Lychee-Dev/blob/3057a9666ab078270b63c7f06de748069b0d1f4b/README.md). This branch must not present an npm install as installation of the unfinished Rust candidate. Prior branch README and UI design text are preserved in [docs/history](docs/history/).
