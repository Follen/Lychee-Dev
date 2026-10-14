# Independent real Skill workflow acceptance

The evaluator read the fixed Go 3.2.0 Skill at
`3057a9666ab078270b63c7f06de748069b0d1f4b` before designing this controller.
`workflow.json` freezes the full original 23-file Skill inventory (entry,
references, agent metadata and launcher) and three representative user tasks.
The selected candidate can add necessary Emmy migration notes; it cannot reduce
the fixed Skill's business contract to make a test pass.

## Execute on Windows CI

After the sealed CLI and unpublished candidate package have been assembled:

```text
node --test tests/rust-skill-usage/runner.test.mjs
node tools/rust-skill-usage.mjs .tmp/ci-cli-artifact/artifact.json .tmp/ci-source-candidate/report.json
```

The outer tool checks artifact/report/binary/source-tree/manifest/resource and
current packaged Skill identities. It starts exactly one controlled Windows Job
and gives its controller a fresh registered scratch directory. The controller
stages an exact package copy into a space-and-Chinese-character npm-prefix
layout. An unexecuted `.cmd` marker lets the real packaged Skill launcher follow
its ordinary npm-prefix discovery branch into the real package launcher, which
verifies and launches the actual candidate EXE. This is a resolution fixture,
not a claim that npm installation has been tested. No native override is used.

Actual commands use that chain, including describe/help, init/catalog, exact
Retail `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b` sync and target inspection.
Queries prepare their own mapping, as the Skill says; no index warm-up is imposed.
API and implementation tasks use returned paths and symbol IDs, inspect originals,
then request static and ordinary semantic refs/context. Literal Chinese and
no-result queries retain coverage, and a large result is continued once in the
same scope. Wrong-scope cursors, invalid limits and line ranges must fail with
structured errors before the original fixed query succeeds again.

The API answer requires byte-for-byte equality with the authenticated archive's
exact lines 10–21, including reported first/last/total lines and full-file/window
hashes. Merely returning the relevant strings with shifted line coordinates fails.
The same authenticated archive fixes the original line-141 call token's exact
byte range, 3940–3960, in ActionButton.lua.

Ordinary native checks require the native engine and rules identity, consistent
analysis/input identity and coverage, full fixed Lua input cardinality, and
`source_owned_process` / `one_shot` / `retired=true`. They also require the exact
original callsite's native basis, byte site, proof, lexical result and observations.
Production projects native rows into the public paginated `relations` array and
removes the duplicate `nativeResearch.relations`; the checker follows that shape.
A structural-only row cannot satisfy this check. Honest partial/static-candidate
native facts are accepted without asserting resolved bindings or complete whole-
repository semantics. The ordinary context page is bounded to 50 rows so the
checked callsite remains available; its existing excerpt/depth budgets are intact.

The batch workflow first consults actual describe. A missing command is a failed
required capability with preserved supported answers, not an invented invocation
or permission to weaken the Skill. Once advertised, the documented three-item
static batch must retain order, pins, individual results and partial semantics.

There is no silent static fallback for failed ordinary semantic calls. Source
sync is a new bounded download to avoid adopting another task's mutable home,
cache or process. It is recorded as network-and-local input preparation, never
called controllable benchmark time. The existing independent full-corpus runner
remains the all-file authentication gate; this run is actual selected task use.

## Evidence and bounds

- Whole controller: 19-minute admission budget inside a 20-minute owned Job
- Each public call: at most 210 seconds, close: at most 30 seconds
- At most 60 commands; per-stream response buffer 12 MiB; retained output budget
  192 MiB plus small reports, with original stdout/stderr hashes
- Staged package: at most 512 MiB, 10,000 files, bounded entries and depth;
  scratch admission reserves 1 GiB
- After timeout, signal or output/process failure: no more child processes;
  outer Job must prove root/tree/handle/readers reaped before scratch cleanup
- All terminal results and failure logs sealed before cleanup; uncertain cleanup
  retains scratch with an interrupted receipt

`report.json` preserves actual commands, elapsed time, captures, answers and
per-case outcomes. `candidate-identity.json` binds all package files and test
inputs. `owned-report.json` and the process/disk receipts are necessary before
claiming even selected-workflow success. Source captures are scoped to the
retired isolated home; sealed stdout contains the actual returned evidence.

The Node tests run on Linux and exercise checker/fault behavior only. They do
not run the Windows CLI, execute the Skill, or prove product acceptance. Only a
real Windows run followed by independent review can supply actual usage evidence.
This selected workflow also does not establish Data/Live/addon branches, complete
Go parity, native lifecycle, performance or real-game acceptance.
