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

### Windows memory observation

The existing outer owned Job now opts into an evidence-only 100 ms poller. The
controller marks `cold-index`, `warm-index` and `warm-query` command windows;
network/input preparation and independent Git auditing remain distinguishable.
Read `owned-report.json` → `memory`, and its hashed raw JSONL chunks. Existing
`peakRSS: not_measured` stays accurate: this is Windows WorkingSetSize and
PrivateUsage (committed private bytes), not Linux RSS or resident private bytes.

Only current counters from authenticated members of that exact Job are summed
within one bounded sweep. Each sweep records monotonic start/end timestamps;
it is synchronized but not atomic. Reported group peaks are maxima of these
sums, never sums of individual process peaks. Lifetime kernel counters are kept
per process only. Working-set sums can double-count shared physical pages.
Process PID, exact decimal creation FILETIME, image and evidence-based role are
retained. `sourceExecutableOnly` filters the exact candidate image, while the
whole-Job totals include controllers, Git, native children and any compiler
children. A compilation/preparation peak is not a Source-command peak.

A missed command window, process exit/creation race, failed observation,
unobserved assigned child, long polling gap, evidence cap or unverified sampler
shutdown remains explicit. Short-lived children/peaks between polls can be
missed even in an otherwise clean sample set. Correctness success is recorded
separately from observation coverage; failed/incomplete execution is never a
successful full-corpus memory result. Sampling failures do not kill/retry the
business command. An unjoined observer or unclosed handle still prevents the
existing scratch cleanup gate from claiming all handles are closed.

The poller is bounded to 128 simultaneous members, 15,000 sweeps and 64 MiB of
raw evidence, split into at most 8 MiB files. It has no process-name discovery,
process mutation, game access or new native execution route. Only the two Source
acceptance launchers enable it; existing callers retain their disabled default.
Synthetic reducer checks: `node --test tools/rust-owned-memory.test.mjs`.
Windows-only tests in that file exercise actual owned children and observation
failure isolation; a non-Windows skip is not runtime validation.
