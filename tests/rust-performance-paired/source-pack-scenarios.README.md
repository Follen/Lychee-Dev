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
the current invocation stops without replacement samples or a successful outer
result. Reports preserve the exact stopping point and blocker. The sole explicit
continuation below first revalidates and seals the fixed, already completed cold
calls; it never changes the original failed status.

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

## Restricted continuation of the 7d8 empty-WAL stop

The real [7d8 cold attempt](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6236649962)
completed both measured invocations, then stopped before complete Facts comparison
because the Go metadata directory retained an empty WAL and a 32,768-byte SHM.
Those old owners and their reports remain **failed**. Their cold calls are not
rerun, relabeled as fresh, or replaced. This continuation is limited to:

- Outer owner: `2f0bcd33-f0f4-46b6-9bc5-5539dbc5b422`
- Healthy-base owner: `058035ab-29e8-43c7-bbeb-ee54472f8147`
- Go cold run: `bfae709f-783a-4eb0-ae9e-70e949163034`
- Rust cold run: `0ee84792-5d7e-4f59-b0a2-44d2e6779c61`

Export the revised 25-file runtime closure separately; keep the fixed product
checkout untouched. Use the original exact pack2 configuration and a current
machine-conditions file. First inspect the retained evidence and create a new,
small reference file (the destination must not exist):

```powershell
node <reviewed-runner-stage>/tools/rust-source-pack-diagnostics.mjs <pack2-config.json> <machine-conditions.json> --inspect-cold <NEW-retained-cold.json>
```

This invokes no product and creates no scratch owner. It reads the two fixed
sealed owners, all original logs, exact original 25-file runner identity, raw
requests/proofs/stdout/stderr, sync-only seed provenance, and the two current
home inventories. It writes only the new reference JSON, containing the two
receipt SHA-256 values and the two current inventory SHA-256 values. The original
failure occurred before a post-cold inventory was captured; these inventory
hashes therefore explicitly describe the retained state at inspection, rather
than asserting a nonexistent historical post-call inventory. The subsequent
full input/Facts/pin checks establish whether that state is usable.

Then run the existing owned controller through the formal continuation mode:

```powershell
node <reviewed-runner-stage>/tools/rust-source-pack-diagnostics.mjs <pack2-config.json> <machine-conditions.json> --resume-cold <retained-cold.json>
```

It rejects foreign/active/unreaped owners, altered/unsealed evidence, another
runner or product identity, an extra prior scenario, changed homes, another
hostname/platform, or a changed power plan. It revalidates the original native
clock and cold 31-phase profile, preserves original memory observations/gaps and
conditions, and compares the complete five Facts arrays and common summaries.
Only a copy of the main SQLite DB is opened. The source DB, absent-or-zero-length
WAL and bounded SHM are authenticated before and after copying/reading, including
file identity and timestamps; nonempty WAL, any rollback journal, symlinks and
concurrent changes fail. No original sidecar is deleted or opened by SQLite.
An empty WAL contains no committed frames; SHM is nonpersistent coordination
state ([SQLite WAL format](https://sqlite.org/walformat.html),
[SQLite file format](https://sqlite.org/fileformat2.html)). This does not implement
a backup/recovery path for a nonempty WAL.

A new read-only custody owner seals complete cold indexes, metadata snapshots,
all cold Go/Rust Facts files and input inventories before warm is allowed. Large
files use the existing 8 MiB parts. Only comparison/reconstruction material is
copied; no whole home, mirror or unrelated cache is moved to evidence. Original
failed receipts/logs are repeatedly authenticated and never changed.

A separate continuation owner records the exact two borrowed home paths,
original receipt hashes, sealed cold custody receipt, and before/after inventories
in its own inputs/evidence. Only same-home warm writes to these borrowed homes;
changed scenarios still clone their independent healthy base into fresh owners.
The old scratch directories are never adopted or cleaned. A single-use claim at
`.tmp/rust-disk/source-pack2-cold-058035ab-29e8-43c7-bbeb-ee54472f8147.json`
is created before warm and is retained even on failure. A second attempt cannot
automatically repeat or replace warm/changed observations; stop and review the
existing evidence. No general owner-reopening or automatic retry system exists.

The resumed report marks `originalCallsReused: 2`, `freshColdCalls: 0`, and the
original failed lineage. Fresh six-scenario reports cannot pass as continuations
or vice versa. No performance-acceptance conclusion is added.

Admission still uses the unchanged 5 GiB scratch / 2 GiB free-reserve policy.
Existing homes are already charged. Read-only custody reserves 1 MiB new scratch;
free-space demand is `2 * INDEX_BYTES + actual cold Facts bytes + 65,330,258 +
64 MiB`. Warm reserves the larger measured cold index plus 8 MiB for sequential
temporary replacement/metadata growth; its free-space demand additionally covers
two bounded index copies plus 64 MiB. These are documented estimates, not hard
allocation caps. Actual usage is rechecked; changed engine reservations remain
based on actual healthy inventories. A quota or integrity failure stops with
completed cold evidence preserved. It does not raise the cap, delete another
owner, rebuild the product, or silently take replacement samples.

## Continue only the four unexecuted variants after be168

The fixed [be168 continuation](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6237209753)
completed the original cold revalidation and new warm comparison, then hit
`scratch_quota` before the first variant owner existed. Its outer
`d1989b3b-573a-423e-9185-86b89fc6d2f4` and continuation
`bff2649b-3f34-4e15-b14e-48855cff42fe` stay failed/sealed/reaped. The complete
[cold custody](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6237188747)
`786d460f-cfd9-4a21-9322-6b79d5e5a08d` stays retired. The original cold claim
(SHA `514fb250a2d885f9f154609e658f5969f621c71c08ad49d65f93f64580f77a10`)
is retained unchanged. Do not rerun `--resume-cold` or delete that claim.

The exact remaining-task entry is:

```powershell
node <reviewed-runner-stage>/tools/rust-source-pack-diagnostics.mjs <pack2-config.json> <machine-conditions.json> --inspect-remaining <NEW-remaining-reference.json>
node <reviewed-runner-stage>/tools/rust-source-pack-diagnostics.mjs <pack2-config.json> <machine-conditions.json> --resume-remaining <remaining-reference.json> --scratch-6gib
```

Inspection writes only a new four-hash reference. Authentication checks the
exact be168 runner, both original cold/warm native proofs and raw outputs,
31-phase profiles, saved/reloaded warm reuse identities, complete comparisons
and summaries, cold custody and all its bounded parts, every original failed
owner's sealed evidence, unchanged claim, and exact healthy post-warm inventory.
It accepts only the recorded first-variant admission stop with zero variant
owners/calls. The old raw `running` row remains in its original evidence and is
identified as an unstarted admission stop in the new lineage.

A new owner references the two healthy homes read-only; no baseline home is
copied or rewritten. The existing four-case loop alone executes, still making
independent per-engine variant clones, sealing Go comparison custody before
cleaning that own clone, and preserving the complete comparison and timing
contract. New output explicitly records four reused original calls, zero new
cold/warm calls, and the original conditions. The original two failed owners
and all their evidence are never changed or cleaned.

A new remaining-only claim is created only after the first variant owner passes
admission, before any variant preparation or product call. Therefore a pure
pre-admission rejection makes no invocation claim; once claimed, failure cannot
automatically restart or substitute any variant. Reports explicitly list
`notRunCases` and `notRunInvocations`; a launch attempt is not mislabeled as
not-run merely because its terminal result is unavailable.

`--scratch-6gib` is the user's specifically approved one-task allowance, bound to
`Sentinel_a57db18d476081919e647f3195a645a1` and this fixed be168 remaining-task
scope. It sets only the explicit `limits` arguments for this wrapper/controller's
admissions and owners: scratch exactly 6 GiB, cache still 20 GiB, free reserve
still 2 GiB. The flag is rejected on fresh/cold/inspection modes; omitting it
retains 5 GiB. Arbitrary amounts, authorization references, scopes or environment
overrides are rejected. No machine environment, policy file, product runtime
budget or old evidence is changed. First Go admission previously needed
380,939,478 B above 5,005,736,596 B already used, a 17,966,954 B gap at 5 GiB;
actual reservations and usage checks remain unchanged under the approved cap.
