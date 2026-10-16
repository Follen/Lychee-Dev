# Real native Source acceptance

Status is candidate-specific. Historical Windows runs include real quota/idle/lifetime passes and full-Retail deadline failures; see [handover](../../handover.md) for exact trees and evidence. A new sealed optimized candidate still must execute the applicable groups. A Node checker pass is never a product pass. These commands do not execute studied source/configuration, operate games or publish a release.

## Invocation and independent groups

```text
node --test tests/rust-source-native-acceptance/*.test.mjs
node tools/rust-source-native-acceptance.mjs <artifact.json> <candidate-report.json> full
node tools/rust-source-native-acceptance.mjs <artifact.json> <candidate-report.json> quota
node tools/rust-source-native-acceptance.mjs <artifact.json> <candidate-report.json> idle
node tools/rust-source-native-acceptance.mjs <artifact.json> <candidate-report.json> lifetime
```

Use separate Windows amd64 CI steps/jobs for the four groups. The outer wall
caps are 20 minutes, 10 minutes, 150 seconds and 12 minutes respectively. The
controller reserves 15 seconds inside its cap for cleanup; these are failure
ceilings, not estimated performance. `idle` deliberately observes 60 seconds;
`lifetime` deliberately observes the actual 600-second owner lifetime. Neither
long group is replaced with a shortened constant or a preloaded counter.

The wrapper requires the current `sourceDigest`, sealed EXE, artifact record,
candidate report and every candidate payload/resource to agree. It verifies
them again afterward and hashes the exact test inputs, including the reused
full-corpus runner. The candidate is development-only. Every process, including
PowerShell peers, remains inside one existing owned-process Job without a
helper exception. Scratch is removed only after terminal evidence sealing and
complete root/tree/handle/reader reaping. A fatal process/transport failure, including any failed nested full-corpus
controller or its child timeout/signal evidence, stops new launches and leaves retirement to that Job.

## Current workflow and separately sealed runner

Both wrapper and controller call `loadCurrentWorkflow`, authenticating the fixed
Go3.2.3 contract, original historical workflow, both exact Skill manifests and
all four cumulative reference fixtures. The old `workflow.json` remains the
unchanged Go3.2.0 evidence; it cannot be passed to current candidate validation.
The owner hashes every current workflow input before and after execution.

A runner-only repair can test an already sealed candidate without changing its
product source identity. Export the selected runner commit into a new directory
with this fixed dependency set (retain commit/archive/file hashes):

```powershell
$RunnerCommit = '<exact selected runner commit>'
$RunnerStage = '<new runner directory>'
$Archive = '<new archive path>'
git archive --format=tar --output=$Archive $RunnerCommit -- tools/rust-source-native-acceptance.mjs tools/rust-build.mjs tools/rust-disk.mjs tools/rust-emmy-identity.mjs tools/rust-owned-process.mjs tools/rust-owned-process.ps1 tools/rust-owned-memory.mjs tools/rust-build-auxiliary.json tools/zip.mjs release/rust-disk-policy.json tests/rust-source-native-acceptance tests/rust-source-full-corpus/run.mjs tests/rust-source-full-corpus/assertions.mjs tests/rust-source-full-corpus/retail-oracle.json tests/rust-skill-usage/assertions.mjs tests/rust-skill-usage/workflow.json tests/rust-skill-usage/workflow-go323.json tests/rust-skill-usage/baselines
if ($LASTEXITCODE -ne 0) { throw "runner archive failed" }
New-Item -ItemType Directory -Path $RunnerStage -ErrorAction Stop | Out-Null
tar -xf $Archive -C $RunnerStage
if ($LASTEXITCODE -ne 0) { throw "runner extraction failed" }
node "$RunnerStage/tools/rust-source-native-acceptance.mjs" '<sealed-artifact.json>' '<sealed-candidate-report.json>' full
```

Run the last command from the unchanged product checkout that matches its
sealed artifact. Do not overwrite its tools, source or old evidence. Product
`sourceDigest`, EXE, candidate report, launcher, packaged Skill and resource
hashes remain verified against that checkout. Executing runner and workflow
hashes are separately recorded and checked at retirement; both reports record
`runnerRoot` and the current Go contract. The controller launches its full-corpus
dependency from the same authenticated runner tree. This changes no product
budget, case, corpus, cleanup requirement or acceptance criterion. A repaired
runner does not turn an old failed report into a pass.

## Full Retail group

The existing full-corpus controller is run again, in the same newly owned home
and Job, rather than borrowing its earlier report. That controller does not
clean up its home. It authenticates Gethe/wow-ui-source commit
`31c7f7b9cc79e56c986b365c06a6afbcf3c9177b`, archive oracle SHA256
`d4d6e468636d6205d94b912fd96ac469b66ae606e6422d7e3849afbff7a376df`,
all 4,049 Git blobs and all 4,036 Lua/XML/TOC documents (32,269,051 bytes).
It performs cold/warm full indexing, exact original declaration/inspect lines,
UTF-8 Chinese and ordinary no-result searches, bounded large-result paging,
cursor/argument errors and same-pin recovery. These original static cases
remain explicitly static and cannot satisfy the native checks below.

The same home then runs actual ordinary native refs/context, followed by
explicit `--session-reuse` across fresh CLI processes. Checks require:

- Ordinary one-shot lifecycle retirement; material-cache cold build and warm
  authenticated admission are separate from a retained semantic analysis hit.
- A real C_Timer.After API Type operation in the independently rebuilt API
  model, with executed/bound coverage, known inferred type and the exact
  authenticated original declaration mapping. A descriptor is insufficient.
- The native engine and actual plan/execution receipts, every Lua input plus
  an independently rebuilt complete XML mapping inventory. A bounded Node
  byte tokenizer reads all authenticated original XML, reconstructs script
  bodies, entity/newline/CDATA spans and synthetic wrappers, then compares
  every coordinate and the complete inventory digest. Neither a returned
  count nor a historical script count supplies this oracle. Every TOC and
  the authenticated original API environment are checked separately.
- Both ordinary and session context paths check every returned snippet against
  authenticated original bytes: exact file hash, first/last lines, numbered
  text, definition range and total line/byte/count budgets. Depth-zero context
  intentionally has no relation rows; refs supplies the exact callsite check.
  Context additionally requires an actually executed References answer bound
  to the complete plan hash and exact original declaration token, UTF-16
  position, file hash and admitted load scope. Partial completed-batch counts
  may remain zero; a plan/metadata shell plus static snippets cannot pass.
- The original GetActionButtonForID call at ActionButton.lua:141, including its
  exact UTF-8 byte range. Native-origin evidence must exist independently of
  a structural row at that location. Partial/unavailable/uncertain binding
  stays partial; it is not promoted to resolved or complete semantics.
- Independent reading of the actual persisted native research material. Every
  original document is rehashed against the complete oracle. The compressed
  material bytes are retained in bounded chunks, with raw and chunk hashes,
  so the source-file counter is not the only evidence of input coverage.
- A cold full snapshot transfer, warm identity-only transfer, identical held
  owner PID/creation identity and native input identity, and admitted-batch
  accounting preserved across different CLI invocations.
- A local synthetic descendant of the fixed commit with one non-source note.
  Its 4,036 original documents are unchanged and independently rechecked.
  It must force a full analysis rebuild, then switching back to the exact
  upstream pin must rebuild the original identity. This derivative is clearly
  labeled test material, never another upstream Retail revision.
- Public close, closed status, a new authenticated owner identity after reopen,
  final close and the independent outer-Job retirement proof.

## Real IPC groups

Quota and time-based groups use a small isolated local Git/project fixture.
The public CLI validates its normal project-lock identity and starts the real
native owner; the harness does not modify the metadata database, use a private
server-start bypass, or inject a starting batch count. This fixture is separate
from full Retail and cannot satisfy any full-corpus claim.

The committed fixture contains `a.lua`, `Main.toc`, and one literal
`Interface/AddOns/Blizzard_APIDocumentationGenerated/NativeSessionDocumentation.lua`.
The documentation is an analysis input, not an executed TOC entry. Public
`source index --project` registers the exact project lock; its returned snapshot
must match that lock. Public `source refs --snapshot ... --symbol Caller
--direction incoming --limit 25 --static-only` then runs the production API
extraction/authentication path. The preflight requires one unambiguous source
declaration, ready environment coverage, the exact source/client/commit, one API
document, and the independently computed original-input digest. Its semantic
coverage remains unavailable and its overall completeness remains false.

Only afterward does the existing `source refs --session-reuse --release ...`
open the native owner. The public and worker environment manifests must agree
with the preflight, and both committed Lua documents must reach the native
analysis. Raw source inputs, their file hashes, and the observed environment
manifest are retained alongside the command logs. No generated API, cache,
metadata database, startup token, or batch counter is injected. The direct
`tinySnapshot` wire fixtures retain their original empty API environment and
unknown load-scope inputs; their quota and timing expectations are unchanged.

Offline Node tests run the JavaScript bootstrap with real local Git commits and
mirrors but injected CLI observations. They check input custody, command order,
and rejection behavior only. They do not prove Rust API extraction, actual
Windows owner startup, named-pipe traffic, quota, idle, or lifetime acceptance.
Actual product outcomes require a sealed Windows execution of each corresponding group; preserve historical pass/fail identities separately from any newly changed candidate.

The authenticated peer holds the named pipe and corresponding process handles,
checks the operating-system pipe server PID, exact process creation time,
image path and held executable SHA256 against Hello, and records requests,
responses and diagnostics. FILETIME values retain exact integer precision.

Quota cases execute zero-, one- and two-batch logical plans through the actual
pipe, accumulate to exactly 256, verify analysis retirement, test 255+2
identity-only rejection before execution and full resend, and test the single
full-input replacement path. Plan/admitted/executed counts remain separate;
partial execution never refunds admitted work. No preloaded-counter fixture
is accepted as proof of these runs.

Idle/lifetime groups measure real elapsed time on the original authenticated
owner. Malformed envelopes must not renew idle; admitted status requests must
not extend the fixed lifetime. Their timing and original process-exit evidence
are saved separately from the quota and full-corpus results.

## Evidence and limits

`owned-report.json` binds the input identities and owned-process proof.
`report.json` contains command arguments, exact exit state, wall time, hashes,
per-phase profile events, checks and remaining gaps. Nested prerequisite and
peer evidence is recursively sealed. Large stdout or material bytes are
split into ordered bounded chunks; concatenate in listed order (then gunzip
material) and verify both chunk and whole-byte hashes. Raw diagnostic files
preserve production memory telemetry; process working set/private commit and
Job peak commit have different meanings and are not summed into an RSS claim.

Only `passed_native_<group>` is available, not complete Source acceptance.
Full semantic resolution, every Source/Skill business branch and all other
product gates remain separate. Go 3.2.0 differential is explicitly `not_run`
without a verified same-machine Go candidate. Single correctness wall samples,
phase timings and owner memory observations do not establish a 70% reduction.
No CPU/I/O/aggregate-RSS metric is invented, no download time is guessed or
subtracted, and no independent cold/warm repeated performance gate is marked
passed. Required paired, equivalent-work Go/Rust measurements remain open.

### Full Retail memory evidence

The `full` group enables the same exact-Job sampler described in
[the full-corpus runner](../rust-source-full-corpus/README.md#windows-memory-observation).
Its `owned-report.json` → `memory.phases` includes the prerequisite cold index
and warm queries, `full-native-semantic`, retained cold preparation, retained
warm reuse, and a dedicated 1-second `retained-session-steady` window after
successful warm-owner authentication. The idle window records individual
samples and medians, requires at least three complete sweeps with the exact
retained PID/creation identity, and checks that public session status is still
ready afterward. This window is an observation, not proof of convergence or
absence of a leak. Its one-second delay is part of correctness observation time
and cannot be used in performance comparisons without equivalent instrumentation.

Each command's phase marker and spawned PID are recorded. The nested prerequisite
controller shares the original owner; native workers stay inside its Job even
when they have their own nested Job. Authenticated existing native diagnostics
establish the retained-owner role. Other descendants remain visible by exact
image and identity; a matching candidate image alone does not prove a one-shot
worker role. Whole-Job and candidate-image-only simultaneous sums are separate.
Individual kernel lifetime peaks are never added together or attributed to the
current phase. No actual Retail memory value is established by these source
changes or by the synthetic tests. The next authorized Windows full run must
supply its exact candidate, input and evidence hashes, correctness outcome and
observation gaps before reporting an actual full Retail measurement.
