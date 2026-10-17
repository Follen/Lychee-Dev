# Formal pack2 six-scenario diagnostic

This is an opt-in Windows **observation**, not the 20-pair performance gate or
complete B01–B07 acceptance. No result is implied by the Node tests. The product
is fixed to `00d215a937513609e482c1ef65f3b43f0288beee`; the controller reuses its
existing sealed release/symbols artifact. There is no Cargo/rustc invocation,
CLI rebuild, game interaction, package installation, publication or historical
sample import. The historical b3d/pack1 and bbfe/ab4/v10 results stay unchanged.

## Inputs and execution

Keep the product checkout at its original clean commit. A separately exported
runner stage is allowed; export every path in the new controller's
`RUNNER_FILES` together from the reviewed runner commit. Do not copy these files
into the fixed product checkout or re-identify its artifact as the runner commit.
The wrapper hashes the full runtime file closure before and after execution.

Invoke on the selected Windows test environment:

```powershell
node <reviewed-runner-stage>/tools/rust-source-pack-diagnostics.mjs <pack2-config.json> <machine-conditions.json>
```

Exact runtime export list (25 files; this isolated closure has been imported in
a fresh cloud directory):

```text
tools/rust-source-candidate-diagnostics.mjs
tests/rust-performance-paired/source-candidates.mjs
tests/rust-performance-paired/read-fact-delta.mjs
tests/rust-performance-paired/inputs.mjs
tests/rust-performance-paired/contract.mjs
tests/rust-source-full-corpus/assertions.mjs
tests/rust-source-full-corpus/retail-oracle.json
tests/rust-parity/describe-contract.mjs
tests/rust-parity/oracle.mjs
tools/rust-build.mjs
tools/rust-disk.mjs
tools/rust-emmy-identity.mjs
tools/rust-owned-process.mjs
tools/rust-owned-process.ps1
tools/rust-owned-memory.mjs
tools/rust-build-auxiliary.json
tools/zip.mjs
release/rust-disk-policy.json
tools/rust-source-pack-diagnostics.mjs
tools/rust-source-pack-evidence.mjs
tests/rust-performance-paired/source-pack-scenarios.mjs
tests/rust-performance-paired/source-pack-resume.mjs
tests/rust-performance-paired/source-pack-custody.mjs
tests/rust-source-native-acceptance/assertions.mjs
tests/rust-skill-usage/assertions.mjs
```

Configuration (all filesystem paths absolute; replace every placeholder with
actual authenticated values, including the complete Go package inventory SHA):

```json
{
  "schema": "lycheedev.source-pack-scenarios-config.v1",
  "ownerRoot": "C:/owned-project",
  "candidate": {
    "sourceRoot": "C:/fixed-00d-product-checkout",
    "artifactRoot": "C:/original-artifact-owner",
    "artifact": "C:/original-artifact-owner/.tmp/pr14-00d215a-release-artifact/artifact.json",
    "commit": "00d215a937513609e482c1ef65f3b43f0288beee",
    "artifactSHA256": "02ebe1eb57d552d1c8ebf6c86697fdbc77d6eba4ee847edd33c4d7d19b6253ad",
    "binarySHA256": "3169c2187b966b40ed3622fe14cfb81527e8dd3a52a6d99a888e8ab482c1b1fb",
    "sourceTreeSHA256": "06a5a0e39c46c786d946b8c429c90c0eb4e9e1291028375e329fc9457776b4e3"
  },
  "go": {
    "sourceRoot": "C:/fixed-Go323-checkout",
    "packageRoot": "C:/existing-Go323-package",
    "luaLSArchive": "C:/official-pinned-LuaLS.zip",
    "packageSHA256": "<64 lowercase hex>"
  }
}
```

These fixed artifact/EXE/source hashes bind the existing [00d release build](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6236213281) and [assembly](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6236225116). The EXE is 44,717,568 bytes; substitute only the owner-local absolute paths, not new artifact identities.

The existing machine-conditions format requires nonempty `machine`,
`powerAndThermal`, `competingLoad`, `disk`, and `operator` strings. State unknowns
honestly. Go source must be clean at
`3766f9cacb52e93a803a626187e2a1d54a445468`, with its complete existing package,
pinned LuaLS 3.19.1 archive, and actual `go version -m` identity. Rust source,
artifact receipt, optimized EXE and optional matching PDB, source digest,
unmodified Emmy identity and 3.5.0/beta configuration are reauthenticated.

## Exact cases and preparation

1. Cold: each executable actually performs init and fixed Retail source sync in
   its own fresh home. Both independently authenticate every Gethe Git blob,
   complete metadata schema and CLI-created pin at
   `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b`. No analysis is seeded. These homes
   themselves become the healthy bases after their successful cold indexes.
2. Warm: the identical command repeats on those same successful base homes.
   Complete derived hashes/file identities/timestamps and Rust's actual cache
   hit are required. No OS cache flush or OS-cold claim is made.
3. Single content change: append exactly one LF to the first eligible document,
   `Interface/AddOns/Blizzard_APIDocumentation/BaseAPIMixin.lua`.
4. Root insertion: create `000_LycheePackInserted.lua` with exactly the 34 bytes
   `function LycheePackInserted() end` followed by LF. SHA-256:
   `a1fb32c0408f44f38dec357b31f1268ea84a12f1c33c01ffda14948ec3d9e330`.
5. First deletion: remove that same original first document.
6. Cross-prefix rename: move its unchanged original bytes to the previously
   absent root path `ZZZ_LycheePackRenamed.lua`. This is a new pack2 case, not a
   historical pack1 measurement.

The original first document is 2,533 bytes, Git blob
`0083f8b1a5957be88807f2eb60642929b3712012`, SHA-256
`5c48438fa06252e7eacb5dc752f4959cb6e2b20cbc639fc474229fe6fdbe4053`.
The first three mutation recipes retain [the original report](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6235614924).

Every changed engine starts from a new byte-checked copy of its original healthy
base, never the prior variant. Fixed author/time local commits, trees and blobs
are constructed only inside that owner, with an isolated Git index and disabled
hooks/config prompting. Complete modified corpus identities and the exact
project-lock pin are prepared and recorded before the timed call. The timed
public index performs its own normal project-selection/pin admission. No fixture
source or configuration code is executed and no Git ref is pushed.

Cold runs Go then Rust; warm runs Rust then Go. Each changed case runs Go then
Rust in separate sequential owners. Their independent preparation occurs between
samples, explicitly recorded as nonadjacent diagnostic observations. This is
not an alternating repeated-performance acceptance.

## Timing, equality and custody

Every CLI stays in an owned non-breakaway Windows Job inside the controller's
outer Job. The original native total CreateProcessW-to-descendant-reap/output-
drain clock is retained, with wrapper wall separate. Product index budget stays
180,000 ms and invocation cap 210,000 ms. Rust's exact 31-phase profile is checked
with the existing cold/warm/changed validator; overlapping phase values are
neither summed nor subtracted from total time. The healthy pack2 changed parse
bounds are 64 / 513 / 512 / 1,025 respectively. A bound failure is retained.
Memory observation keeps sampled aggregate working-set/private-usage and gaps;
it is not complete or unique physical RSS. CPU/I/O remain unmeasured.

The complete five Facts arrays, every row field, duplicate and array order,
document order/identity, and full common summary are compared. Extra Facts keys
are rejected. The same-run Rust index comes only from the exact v14/pack2
metadata key and authenticated blob SHA; selecting a file by age or scanning
for an attractive cache is forbidden. Both engines' full persisted selection
and closed metadata schema are captured. No enhanced semantic result is removed
to manufacture equality; any difference fails and remains available for review.

For changed cases, Go's authenticated raw records, original manifest and every
actually compared raw per-file Facts envelope are copied into immutable evidence,
then that successful Go owner is sealed and cleaned. The subsequent Rust owner
uses the original healthy Rust base. The small `source-pack-custody.mjs` helper
and optional authenticated stream argument to the existing `readFactDelta`
allow the identical comparison after Go scratch removal. No full home, mirror,
mutable cache or unrelated file is moved into evidence. This is comparison data,
not a new product catalog.

All aggregate indexes/records and Go Facts evidence are streamed in 64 KiB I/O
chunks into ordered 8 MiB-or-smaller parts, each with a whole-file SHA, per-part
SHA, offset/size and authenticated manifest. The catalog also binds the original
manifest and every parts manifest; final verification rehashes them and all
bodies. The 16 MiB disk-sealing file cap is unchanged. No large index JSON is
inlined into the report. Missing, reordered, duplicated, escaping, corrupted,
over-budget or changed-after-open evidence prevents comparison success.

## Disk and terminal ownership

The original fixed policy remains 20 GiB cache / 5 GiB scratch / 2 GiB free-space
reserve. All `LYCHEEDEV_DISK_*` environment overrides are rejected before the
first owner; no machine environment is changed. No existing active, preserved,
foreign or unregistered owner is reclaimed.

The initial cold reservation is explicitly a planning estimate: historical
complete b3d home bytes 257,058,598 + 131,416,885 plus 64 MiB setup/growth allowance.
It is not a measured 00d size or an OS allocation cap. Current cold home sizes and
admission are recorded before proceeding. Each changed engine then reserves its
measured complete healthy home, a complete new measured index, Facts growth and
8 MiB fixture/metadata allowance. Go's exact one-file mutation recipes use its
largest existing fact plus 1 MiB allowance; Rust conservatively reserves a full
additional Facts generation rather than predicted hits. Complete retained output
copies, all four cold/warm indexes, raw full-corpus Git audit outputs and bounded
metadata/report allowances are included separately in free-space admission. Actual post-run usage is
also checked and retained; that check is not presented as a preallocation cap.
The run may still reject admission under the unchanged quota. It must not lower
an estimate or omit ownership to fit a desired run.

Only successful, fully reaped, sealed owners are cleaned through their existing
`beginScratch` handle. A failed/interrupted owner is retained. Go success before
Rust is explicitly “awaiting independent Rust” and is not a completed comparison.
If any sample, custody, identity, profile, cleanup or power-policy check fails,
there is no replacement sample, no later warm-after-failed-cold label and no
successful outer result. Reports preserve the exact stopping point and blocker.

## Cloud-safe checks

```sh
node --check tools/rust-source-pack-diagnostics.mjs
node --check tests/rust-performance-paired/source-pack-scenarios.mjs
node --check tests/rust-performance-paired/source-pack-custody.mjs
node --test tests/rust-performance-paired/source-pack-scenarios.test.mjs tests/rust-performance-paired/source-pack-custody.test.mjs tests/rust-performance-paired/source-pack-resume.test.mjs tests/rust-performance-paired/read-fact-delta.test.mjs tests/rust-performance-paired/source-candidates.test.mjs tools/rust-source-pack-evidence.test.mjs
```

These tests validate configuration, exact mutation identities, shared reader
contracts, streamed custody, metadata/index lookup, scheduler order, process
failures, same-base lineage, quota rejection and terminal cleanup. They do not
execute Windows product samples or establish performance acceptance.
