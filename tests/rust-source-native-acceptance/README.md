# Native Source acceptance

These Windows amd64 cases consume a sealed optimized artifact and matching
candidate report. They do not compile, execute studied Lua/configuration, operate
a game or publish. Results are candidate-specific; record actual outcomes in
[handover](../../handover.md). Runner unit tests are separate from product proof.

```text
node --test tests/rust-source-native-acceptance/*.test.mjs
node tools/rust-source-native-acceptance.mjs <artifact.json> <candidate-report.json> full
node tools/rust-source-native-acceptance.mjs <artifact.json> <candidate-report.json> quota
node tools/rust-source-native-acceptance.mjs <artifact.json> <candidate-report.json> idle
node tools/rust-source-native-acceptance.mjs <artifact.json> <candidate-report.json> lifetime
```

## Inputs, ownership and limits

The wrapper verifies current sourceDigest, original EXE, candidate manifest,
launcher, managed compiler, packaged Skill/resources and fixed runner inputs
before/after execution. `runner-inputs.mjs` and `native-contract.mjs` own current
inputs and independent checks. Every controller, CLI, Git and peer process is in
one owned Job; no helper bypasses retirement. A fatal process/transport failure
stops new launches. Scratch cleanup requires sealed terminal evidence and full
root/tree/reader/handle reaping. Reuse existing compiled artifacts and caches.

Outer limits are 15 minutes (`full`), 10 minutes (`quota`), 150 seconds (`idle`)
and 12 minutes (`lifetime`), including 15 seconds reserved for cleanup. Idle
observes the actual 60-second behavior; lifetime observes the actual 600-second
owner lifetime. Shortened constants or preloaded counters do not verify them.
These limits are failure ceilings, not performance targets.

## What each group establishes

`full` first executes the fixed [Retail corpus](../rust-source-full-corpus/README.md)
with its independent archive/blob/document oracle in the same home and Job.
It then exercises ordinary native refs/context and explicit session reuse across
CLI processes: actual API type/resolution and original locations, material/cache
identity, source/environment separation, owner reuse and explicit session closure.
Static-only results cannot satisfy the native assertions. Project identity cases
use real `source index --project` admission before querying the fixed snapshot.

`quota` exercises the configured session boundary and failures. `idle` and
`lifetime` verify actual owner retirement at their respective boundaries. Read
the per-case reports for requested work, coverage, partial results and teardown;
preparation or a descriptor alone cannot count as execution.

The optional existing Windows memory observation has the same scopes and limits
as the full-corpus owner. Reports distinguish business correctness, source/
runner stability and process/observer retirement. Measurements do not establish
unmeasured paths or runtime game behavior.
