# Fixed full Retail public CLI acceptance

Consume a sealed Windows CLI artifact without compiling or operating a game:

```text
node --test tests/rust-source-full-corpus/*.test.mjs
node tools/rust-source-full-corpus.mjs <artifact.json>
```

The first command checks the runner, not the product. The second authenticates
artifact/source identity and runs all CLI/Git children in one owned Job, retaining
terminal evidence before scratch cleanup. Failure/timeout stops further launches
until the owner retires the tree. Current results belong in [handover](../../handover.md).

## Independent inputs and cases

The CLI initializes a fresh home and syncs Gethe/wow-ui-source commit
`31c7f7b9cc79e56c986b365c06a6afbcf3c9177b`. An independent Git reader verifies
all 4,049 blobs and 4,036 Lua/XML/TOC documents (32,269,051 bytes), including
paths, SHA-1/SHA-256, size and fixed business anchors. The archive oracle is
`d4d6e468636d6205d94b912fd96ac469b66ae606e6422d7e3849afbff7a376df`:

```text
node tests/rust-source-full-corpus/verify-oracle.mjs <fixed-retail.tar.gz>
```

Cases cover application-derived cold/warm indexing and readiness, original
bytes/lines/hashes, declarations/API queries, repeatability, bounded continuation,
cursor scope rejection, structured failures and same-pin recovery, static refs
and original context. This corpus has no Han source text; Chinese queries check
literal no-result behavior. Static relation coverage remains explicitly partial.
Corpus gaps are failures rather than silently skipped files.

Only `passed_selected_static_cases` is produced. Native semantics, session reuse
and lifecycle limits are separate [native cases](../rust-source-native-acceptance/README.md).
Sync time includes network/local preparation; no pure-download subtraction or
performance claim is inferred from this correctness run.

## Optional Windows memory observation

The Source acceptance owner uses the existing bounded 100 ms Job-member sampler.
Read `owned-report.json.memory` and its hashed JSONL evidence. WorkingSetSize and
PrivateUsage are Windows counters, not Linux RSS or resident-private memory.
Group peaks are maxima of per-sweep sums, not sums of process lifetime peaks;
shared pages may be counted more than once. Sweeps are synchronized, not atomic.

Coverage records command windows, creation identities, exact image roles,
missed/short-lived children, read errors, polling gaps and shutdown. Short peaks
can fall between samples. Correctness and observation completeness are separate;
unjoined observers or unclosed handles prevent successful cleanup. No process-
name discovery or game access occurs. Synthetic reducer and owned-child tests
live in `tools/rust-owned-memory.test.mjs`.
