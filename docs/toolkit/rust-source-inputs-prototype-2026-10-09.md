> Historical reference for the Rust migration (2026-10-14): current decisions, baseline and acceptance status are in [handover.md](../../handover.md). This document preserves its original scope and evidence; conflicting requirements, protocol designs, performance targets and release instructions are superseded. Historical test results are not current-candidate passes.

# Owned Source inputs: optional direct-library prototype

Status: verified fixed prototype subset; not final engine selection, complete
Source acceptance or default command migration.

The shared fact engine, abstract invocation evaluator and immutable provider
now live in `crates/lycheedev-source/src/native/`. The examples register fixture
inputs and expected results; they use the same library implementation as the
dynamic input entry. There is no mutable global source registry or second copy
of the binder. The module remains behind `emmy-prototype`.

`SourceInputs::admit` verifies owned original UTF-8 bytes, content hashes,
analysis bytes and exact Identity/BOM transforms. It rejects path collisions,
unadmitted load files, invalid ranges, configuration that loads unverified
external files, unsupported Lua versions and resource-limit violations.
Preparation checks actual Emmy FileId/URI uniqueness and stored analysis text.
Original ranges remain separate from analysis ranges; XML segment maps are
not yet admitted.

The actual input identity includes source material, original/analysis hashes,
mapping, input admission order, repeated ordered load occurrences, phase,
effective configuration, trusted host material and exact intrinsic declaration
sites, engine base/private patch, rules, dependency lock, luac and limits. It
enters the graph, provider, proof and cache identity. A filename or investigated
annotation cannot grant intrinsic privileges. Luac identity expresses a pending
syntax-check requirement; it does not invent a successful compilation.

The original admission deadline and cancellation token belong to the operation.
Prepare, build, freeze, query and serialization reject a replacement control.
Construction steps are shared by the admission owner, including repeated
preparation and phase views. This prototype supports one operation; separate
request lifetimes, shorter child controls and cross-CLI reuse remain unaccepted.
Cooperative checks cannot interrupt Emmy's synchronous parse/index call; its
hard deadline continues to require the owned process governor.

## Actual evidence

All current artifacts below bind source tree
`9cb639b561ced2f3320bf88baceb641cd32fd17f1a6df9cc9d181a0aa9930eeb`
and private patch
`10c55cf4563ea3d2909443898638c61e7d327aed5b9386c9ae996922dbb7930c`.
The pristine 848-file upstream remains unchanged. Every build preserves its
matching EXE/PDB pair, full input closure and original/modified patch bytes.

| Gate | Actual artifact | Result |
| --- | --- | --- |
| Dynamic input, BOM/UTF-8 maps, identities and control | `.tmp/rust-emmy-source-input-build-03/artifact.json`, receipt `b4ac040c-90a6-4209-99ab-ee8906f760d2`; `.tmp/rust-emmy-source-input-check-02/report.json` | 36 assertions and 53 total custody/check records pass |
| Native core, object, audit, lifecycle and return slots | `.tmp/rust-emmy-source-input-native-build-03/artifact.json`, receipt `4d78c0ac-914f-4d8d-9a93-857fa35de764`; `.tmp/rust-emmy-context-owned-*-02/report.json` | 363 unchanged semantic assertions pass |
| Independent native/coverage/object/lifecycle provenance | Four `independent.json` reports under the preceding native outputs | 1,365 + 44 + 724 + 391 pass |
| Legacy fixture behavior | `.tmp/rust-emmy-source-input-adapter-build-01/artifact.json`, receipt `8666cfa5-7fb1-446c-9a75-1ff930317d52`; `.tmp/rust-emmy-source-input-adapter-check-01/report.json` | Original 86 assertions pass |
| Legacy provenance with explicit new identity | `.tmp/rust-emmy-source-input-adapter-check-01/independent-02.json` | 1,261 checks pass |
| Strict Clippy, all affected examples | `.tmp/rust-emmy-context-clippy-07/report.json`, receipt `f342667f-2532-42cd-99c4-d1127b641c92` | Pass |

The native and input artifacts seal freeze27 (129 input files); strict and
legacy artifacts seal freeze26 (128). The sole additional file is the new
independent legacy verifier, not a source change. Both source identities match.
The original verifier's 52 metadata mismatches remain preserved: the new
snapshot has explicit trusted host material, full admission identity and renamed
engine fields. Its semantic checks did not fail. The replacement verifier
retains those checks and additionally verifies the owned admission digest and
legacy evaluator identity. Initial compile and strict-check failures are also
retained separately.

## Remaining acceptance

The prototype ceiling remains 16 files, 64 KiB input, 6,000 AST nodes, 10,000
construction steps, 2 MiB serialized output and 30 seconds; configuration can
lower these limits. This is a fixed-case prototype ceiling, not a claim that
the complete Retail workload fits it.

Mandatory public definition/reference projection, 16 complete business goldens,
36 flow cases, finite loops, secret/guard transfer, XML integration, full Retail,
cross-request/cache/fault lifecycle, distribution and performance remain
unaccepted. The default Source path is unchanged. Total goal completion also
requires the separate protocol, command, locator, package and real-client gates.
