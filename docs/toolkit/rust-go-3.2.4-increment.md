# Tagged Go 3.2.4 migration increment

[handover](../../handover.md) records current implementation and verification. This document maps the changed reference contract; it is not a passing report.

The user selected Go 3.2.4 on 2026-10-21 and explicitly required the existing Rust optimizations to remain. The immutable reference is annotated tag `v3.2.4` (`acc75ae8fd0fe4eaaf454556129e8a0591d3d7dc`), peeled commit `29bdbdaff5f064ec212ff2064380b1ccac652eda`. At inspection, main was `c9bd90ea2b504c0bf72df0e16876ce5828361298`; its two later file changes are handover/todo documentation. Tests pin the release commit rather than moving main.

The [3.2.3 → 3.2.4 diff](https://github.com/Follen/Lychee-Dev/compare/3766f9cacb52e93a803a626187e2a1d54a445468...29bdbdaff5f064ec212ff2064380b1ccac652eda) changes 20 files. Go Source, Data and command behavior are unchanged; `internal/buildinfo/version.go` changes the release label. The Skill tree and command-contract bytes are unchanged. The [3.2.3 contracts](rust-go-3.2.3-increment.md) remain cumulative. Rust stays version 3.5.0 with beta distribution configuration; this reference update does not authorize publication or game operation.

| Scope | Required implementation and verification |
| --- | --- |
| Addon combat/focus | Port the exact `addon/Bridge/FocusBindings.lua` and `InputState.lua` fixes. Use the fixed 3.2.4 `t_focus_bindings.lua` cases, including entering combat with an existing editor, combat editor changes, and post-combat cleanup. Keep the complete formal addon suite and bridge paths. |
| Release fields | Keep Rust Runtime/TOC and candidate test expectations at 3.5.0. Add a separate 3.2.4 formal oracle and exact release-field projection; preserve older oracle bytes and outcomes. |
| CLI and Skill | Authenticate unchanged command and Go Skill bytes from the new commit. Update active baseline declarations and add a new candidate Skill identity after its changed files are committed. Historical manifests remain independently pinned. |
| Performance | Prepare an isolated Go 3.2.4 package and require its exact commit/version for current paired runs. Preserve twenty pairs, complete tasks, cold/warm measurement, resource limits and the Rust ≤30% target. Old 3.2.3 packages and reports retain their original identities. |

Previously passed Rust tests whose implementation and inputs are unaffected remain recorded at their actual commits. Re-run affected addon, bridge, packaging and identity checks; complete the remaining correctness and performance work. No historical report is relabeled as 3.2.4 acceptance. Real-client tests remain in the final handover section until separately authorized and executed.
