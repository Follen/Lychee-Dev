# Fixed full Retail public CLI acceptance

This runner consumes a sealed real Windows CLI artifact. It does not compile,
execute studied Lua/configuration, use a game, publish or reuse an unresolved
build lease. It is a new acceptance execution, not a reuse of Retail19 results.

```text
node --test tests/rust-source-full-corpus/*.test.mjs
node tools/rust-source-full-corpus.mjs .tmp/ci-cli-artifact/artifact.json
```

The first command tests the acceptance runner itself; it is not product proof.
The second must run on Windows. `verifyArtifact`, managed scratch, and one outer
owned process Job bind the exact EXE and reap every controller/CLI/Git child.
The fresh home is reclaimed only after terminal logs are sealed and process
handles are verified reaped. Evidence remains in `.tmp/rust-disk/evidence/`.

The CLI performs `init`, repository discovery, and ordinary `source sync` at
Gethe/wow-ui-source commit `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b`.
An independent Git reader authenticates the commit, all 4,049 tracked blobs and
their SHA-1 plus SHA-256, then compares path/hash/size digests against the fixed
archive oracle. It verifies all 4,036 Lua/XML/TOC documents (32,269,051 bytes),
not just the selected query files. No managed source files or pins are patched.

The oracle was obtained from archive SHA-256
`d4d6e468636d6205d94b912fd96ac469b66ae606e6422d7e3849afbff7a376df`.
Independently recheck that archive and every anchor with:

```text
node tests/rust-source-full-corpus/verify-oracle.mjs <fixed-retail.tar.gz>
```

Selected public cases cover full derived-cold/warm indexing and readiness,
original bytes/lines and hashes, exact declarations and API lookup, stable warm
query repetition, Chinese UTF-8 no-result behavior, ordinary no results,
large-result continuation and cursor scope rejection, structured errors,
recovery after errors, static incoming refs and original context snippets.
The source has no Han characters in Lua/XML/TOC: the Chinese case checks honest
literal no-result behavior, not natural-language understanding. Static relation
coverage must remain visibly semantically unavailable/incomplete.

Every CLI command retains args, exit, time, stdout/stderr and hashes. Failed
assertions retain useful independent successes; a timed-out child stops further
launches until the outer Job retires its tree. Full corpus and zero-skipped
coverage are mandatory; parser gaps are reported as failures, never hidden.

Only `passed_selected_static_cases` is possible here. Semantic refs/context/flow,
native continuous-session reuse and close, fixed Go 3.2.0 differential, and
20-repeat independent cold/warm performance groups are still separate gates.
Sync wall time is a separate network-and-local-preparation phase. Because the
CLI does not expose a pure download boundary, its time is not falsely labeled
pure download or subtracted from a controllable-time ratio. This correctness
run reports no 70% performance success, CPU/RSS/IO or complete Source acceptance.
