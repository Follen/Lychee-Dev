# Preserve and validate an interrupted Source pack observation

These two read-only-input entry points preserve the original b3d observations.
They do not rerun a product, rebuild a binary/index, change a cache, repair the
old owner receipt, or turn the interrupted observation into paired acceptance.
The historical bbfe/ab4 cold-only validator remains unchanged.

## Export and Node verification

Use one reviewed saved runner commit, independently of the b3d artifact source.
Export **all** paths in `RUNNER_FILES` from
`tests/rust-performance-paired/source-candidates.mjs`, plus:

- `tools/rust-source-pack-evidence.mjs`
- `tests/rust-performance-paired/source-pack-resume.mjs`
- `tools/rust-source-pack-evidence.test.mjs`
- this document

The current runner paths include `tools/rust-build.mjs`, `rust-disk.mjs`,
`rust-emmy-identity.mjs`, `rust-owned-process.mjs/.ps1`,
`rust-owned-memory.mjs`, `rust-build-auxiliary.json`, `zip.mjs`,
`release/rust-disk-policy.json`, the paired `source-candidates.mjs`,
`read-fact-delta.mjs`, `inputs.mjs`, `contract.mjs`,
`tests/rust-source-full-corpus/assertions.mjs`, `retail-oracle.json`,
`tests/rust-performance-paired/owned-proof.mjs`, and
`tools/rust-source-candidate-diagnostics.mjs`. Preserve their relative paths.
No package installation or build is needed. Use the already selected Node 24.
The export must be byte-checked against the chosen commit before invocation.

```powershell
node --test "$runner/tools/rust-source-pack-evidence.test.mjs" "$runner/tests/rust-performance-paired/source-pack-custody.test.mjs"
```

Tests use synthetic bytes and native-proof fixtures. The 82,228,560-byte test
matches the real file's length only; it does not claim the historical hash or
Windows product results were reproduced in cloud.

## 1. Capture the original large index without adopting its owner

The original directory is:

`D:/Code/wow/addons/Lychee Dev/.worktrees/rust-refactor/.tmp/rust-disk/evidence/6d9d5ad5-01ec-4a9c-a248-85020e1e62cb`

Save this exact JSON as a new UTF-8/no-BOM `index-custody-config.json` in the
coordinator's new preparation directory. Configuration is data, not a local
replacement for either formal entry point.

```json
{
  "schema": "lycheedev.source-index-custody-config.v1",
  "root": "D:/Code/wow/addons/Lychee Dev/.worktrees/rust-refactor",
  "source": {
    "path": "D:/Code/wow/addons/Lychee Dev/.worktrees/rust-refactor/.tmp/rust-disk/evidence/6d9d5ad5-01ec-4a9c-a248-85020e1e62cb/cold-index.json",
    "bytes": 82228560,
    "sha256": "1bd8039cb3b003a27392b8f138ee7a1340f888aa4d34211e4fb7b97d508ce520"
  },
  "priorOwner": {
    "id": "6d9d5ad5-01ec-4a9c-a248-85020e1e62cb",
    "receiptSHA256": "85f0012af1d209b9d1fbaa76140fc889b5738974cf0aad86633aeb0057bd74c9"
  }
}
```

```powershell
node "$runner/tools/rust-source-pack-evidence.mjs" capture "$preparation/index-custody-config.json"
```

The tool authenticates the original regular file and receipt before admission.
It reserves original file bytes + 1 MiB under the existing scratch/free-space
policy, creates a new `source-index-evidence-custody` owner with preserve
retention, and streams 64 KiB buffers into ten raw 8 MiB-or-smaller parts.
The original 128 MiB product index cap and 16 MiB/log sealing cap remain intact.
A manifest records original path/size/SHA and each part's ordered name, offset,
size and SHA. A separate streamed read authenticates each part and the complete
ordered byte sequence. Original file and receipt identities are checked again.
Only the parts, manifest and small custody report are sealed; the large original
is never passed to `beginScratch.seal`.

The stdout result identifies the new receipt, custody report and manifest. The
new owner performs synchronous file work and starts no child, so its own
`handlesReaped=true` is independent of the old process. Original owner state
`active` remains exactly that. No cleanup or old-owner terminal claim is made.
On an error, partial new parts and a failure report are retained; a missing
manifest or failed new receipt is not a complete authenticated copy. A seal
failure is propagated, not reported as success.

Independently verify the returned manifest with:

```powershell
node "$runner/tools/rust-source-pack-evidence.mjs" verify "$manifestPath"
```

Original receipt: 2062 bytes, SHA shown above. Original `owned-report.json`:
2524 bytes, SHA `efbeaebb00bdd1be0193e14bc68b6c70765df31ecebf988177ff14d2b2d1d466`.
These identify retained history; neither is rewritten.

## 2. Authenticate existing Rust warm result and its original native timing

Save this JSON as a new `warm-observation-config.json`:

```json
{
  "schema": "lycheedev.source-pack-observation-config.v1",
  "root": "D:/Code/wow/addons/Lychee Dev/.worktrees/rust-refactor",
  "mode": "warm",
  "documents": 4036,
  "runId": "43ceb1ad-4e62-49cf-8058-505343a07f4f",
  "artifact": {
    "root": "D:/Code/wow/addons/Lychee Dev/.worktrees/rust-refactor",
    "path": "D:/Code/wow/addons/Lychee Dev/.worktrees/rust-refactor/.tmp/pr14-b3d2446-release-artifact/artifact.json",
    "sha256": "f7d6cbd39f6e5952c449c25c5a708e4a044f9e4071bb446e221641a80b1b59fc"
  },
  "files": {
    "stdout": {
      "path": "D:/Code/wow/addons/Lychee Dev/.worktrees/rust-refactor/.tmp/rust-disk/evidence/6d9d5ad5-01ec-4a9c-a248-85020e1e62cb/process-43ceb1ad-4e62-49cf-8058-505343a07f4f.stdout.bin",
      "bytes": 1530,
      "sha256": "2a8c79c16d06c1826dec563126733b529cbde27a09e3ac84968b56276b6e159e"
    },
    "stderr": {
      "path": "D:/Code/wow/addons/Lychee Dev/.worktrees/rust-refactor/.tmp/rust-disk/evidence/6d9d5ad5-01ec-4a9c-a248-85020e1e62cb/process-43ceb1ad-4e62-49cf-8058-505343a07f4f.stderr.bin",
      "bytes": 2559,
      "sha256": "707480e6ccf577c6baa3a0f9c044627fac6dfa927bb6146dde580e1952bb1844"
    },
    "receipt": {
      "path": "D:/Code/wow/addons/Lychee Dev/.worktrees/rust-refactor/.tmp/rust-disk/evidence/6d9d5ad5-01ec-4a9c-a248-85020e1e62cb/process-43ceb1ad-4e62-49cf-8058-505343a07f4f.receipt.json",
      "bytes": 2774,
      "sha256": "b0a71b7545597422bcba4b67932909db0943a0ac5c040e5c1a1d356cd6488d97"
    }
  }
}
```

```powershell
node "$runner/tests/rust-performance-paired/source-pack-resume.mjs" "$preparation/warm-observation-config.json"
```

The helper verifies all raw sizes/hashes, same original process filenames/runId,
original artifact record and binary through `verifyArtifact`, native binary path,
exit0, complete reap/close, no forced/deadline/output-limit, and the unchanged
`elapsed()` native timing. It parses all 31 profile fields in fixed order, exact
compact JSON and no unknown stderr. Warm requires only metadata/cached_index
calls, all other phase calls/times zero, cachedFacts=4036, parsedDocuments=0,
Git processes=0, sourceBytesRead=0 and indexBytesRead in the original cap.
It never claims full-Facts equivalence from this cache-hit flag. Its output says
`fullFactsEquivalent: "not_evaluated"`, `productCalls: 0`, `importedSamples: 0`.
The raw native receipt stores a binary path, not its launch-time SHA, and does
not contain the outer wrapper exit. Accordingly `historicalLaunchIdentity` is
`not_independently_reauthenticated`, `wrapperOutcome` is `not_evaluated`, and
`completeInvocationVerified` is false. The existing original owned report
continues to supply that chain; this reader does not reconstruct or replace it.
Original launch/ownership conclusions remain attached to
[6233513933](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6233513933)
and the matching release record
[6233470665](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6233470665).
Capture its stdout in a fresh coordinator evidence file, retaining the original
failure and the original 509.5789ms sample's identity. Do not overwrite old logs.

For the already-executed Rust cold sample, the same config format uses mode
`cold`, runId `5b2ba0a5-7732-4139-88cb-e29cca88a464`, and its matching filenames:

| File | Bytes | SHA256 |
| --- | ---: | --- |
| stdout.bin | 1444 | b0e9134c963e7987909230899384b091f9c3998bc3de137825c290a7b316afb6 |
| stderr.bin | 2676 | 6c4803e1860355e828417ad4998608b874509cf50f69be26bc5742a21661da15 |
| receipt.json | 2777 | 81de5cf62d8cad0b88c6886509565be2d3fd88ea445b02f7ce9d55e1dcea6eff |

Cold requires zero cached Facts, complete parse/serialization denominator and
uncached-index phases. Pack read/publish counts are deliberately not equated to
4036: group opens, frame writes and group lifecycle have their own observations.
`changed` is limited to this observation's content-changing single-change,
insertion and deletion scenarios with a rebuilt corpus and positive Git blob
work. It applies the same strict schema and result checks but permits cached
cohorts and speculative workers whose prepared results were discarded on hits.
It is not a validator for every incremental case, such as a new commit with no
content changes.
This is a mode distinction, not a relaxed schema or normalized output.

## 3. Remaining continuation, kept separate

No product continuation is performed by these two helpers. Existing cold Go/
Rust and Rust warm samples are never rerun. Go warm, single-change, insertion
and deletion remain not_run until executed under a **new** existing owned
process/paired owner, with original 180s product / 210s invocation limits.
The original owner remains preserved and is not adopted through an invented
recovery interface.

Before continuing, authenticate the original Go/Rust homes, complete pin and
mirror identity, index manifest/blob, Go records/manifest, and byte-exact cache
copy inventory. Use isolated new writable copies, retaining the interrupted
origin and cache-copy provenance. Keep the original mutation plan: append one LF
to the first eligible document, insert `000_LycheePackInserted.lua`, then delete
the specified first document. Exact inserted bytes, actual chosen paths,
commit/tree/blob identities and scenario starting-cache states must be recorded
before execution; they are not yet supplied by the public report. Do not choose
more favorable group positions. A single change may reparse its 64-document
cohort, and insertion/deletion may shift all later cohorts.

After each real call, use the mode-aware validator above and the existing
`read-fact-delta.mjs` `v11-pack1` reader bound to the same-run index path/SHA for
all documents and fields/order, with summary totals checked separately. Go warm
has no Rust phase record. Only actual full comparisons can close the remaining
warm/scenario equivalence. Retain failures and no-sample-import provenance;
interrupted continuation is not an uninterrupted adjacent pair or 20-pair
performance acceptance.
