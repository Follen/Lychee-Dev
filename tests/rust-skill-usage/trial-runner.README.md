# Historical blind-trial protocol

This fixed-product experiment is retained for its original commit only. It is
not a current Agent trial, and its old SQL/DB2 allowlist must not be widened or
its results relabeled. Current execution uses the new owned Agent consumer
contracts; a new blind trial, if scheduled after unified validation, needs its
own exact product/schema/identity and scoring plan.

# One-shot independent-agent Skill trial

This runner is test-only. It neither builds a product/exporter nor publishes,
installs a replacement product, accesses games, or changes the original scripted
42-command Data workflow. Node tests validate guards and coordination only;
Windows native execution and independent-agent quality remain **not_run** until
separately performed and scored.

## Fixed identities and prerequisites

Use one already sealed Windows amd64 3.5.0 candidate at product commit
`86f2bbb0f65234bba1bc0454d916e85fc6855ad9` for both guide conditions. Run from that
product's original root. Its current sourceDigest must equal the original
artifact inputHashes.sourceTree. The candidate report, complete installed
package, launcher chain and EXE must authenticate through the existing helpers.
The coordinator must also independently check the original product checkout's
fixed Git HEAD and clean status; sourceDigest is the existing product-source
witness, not a whole-Git-tree digest. The original report must explicitly have workspaceDirty=false.
Deploy the complete runner checkout (the full baseline tree plus all seven new
files), or a separately verified complete runtime dependency closure. Copying
only the seven new files into an empty directory is not runnable: imported
existing helpers, fixed Skill manifests and data fixtures are required. Invoke
the entry by its absolute runnerRoot path while keeping cwd at productRoot.
The newer runner directory is separate and its complete input hashes are saved.
The six shared identity/ownership tools must remain byte-identical to the product
root. A runner change is never a product rebuild or a relaxation of that gate.

Node24 with node:sqlite, installed npm, existing disk admission, the owned
Windows process wrapper, and enough scratch/evidence capacity are required.
Data requires an already sealed same-source exporter receipt accepted by
loadExporters; only the chosen catalog/partial exporter runs. The runner does
not compile missing or stale exporters. Source uses `-` for the exporter receipt.

Supply a separate directory containing either complete authenticated guide tree:
`5edd025cae923fb2235086f250862696db7a2483` or
`231818eb48f81cf927542bb19d9a8964dfbe4baf`. The guide is copied outside the installed
package to a neutral `guide` directory. Its files are read-only *evaluation
inputs*: grant the trial agent read access only; this runner does not alter ACLs.
Its script is never executed. The authenticated current packaged launcher stays
identical for both conditions. Guide bytes are verified before and after use.

## Coordinator operation

From the fixed product root, start the newer runner by absolute path:

    node <runner>/tools/rust-skill-trial.mjs --historical-oracle-only start <artifact.json> <candidate-report.json> <sealed-exporter-receipt.json|-> <guide-directory> <catalog|partial|source>

The command remains running under the existing outer owned Job. Its first line
identifies `session` and `inputs` paths. Wait for session.json to appear before
submitting anything. Startup performs only authenticated npm offline
ignore-scripts installation, one real CLI init, and (for Data) the selected
exporter, loadFixture, and seedDataHome. It never queries a task answer, runs the
42-command script, restores a key, or performs source sync automatically.

Each invocation is one trial/arm, with a new home shared by all its calls.
Never reuse a home, session, or retired owner for another task/guide condition.

Use a second coordinator process to submit exactly one outstanding request:

    node <runner>/tools/rust-skill-trial.mjs --historical-oracle-only call <session.json> 1 '["describe"]'
    node <runner>/tools/rust-skill-trial.mjs --historical-oracle-only call <session.json> 2 '<JSON argv array chosen by the independent agent>'
    node <runner>/tools/rust-skill-trial.mjs --historical-oracle-only finish <session.json> 3

These are illustrative POSIX quoting examples; on Windows pass the JSON string
as one argument using the invoking tool's argv support. No shell command is
accepted *inside* a trial request. Sequences are contiguous, start at 1, and
cannot be reused. The coordinator must keep the original trial process alive.
A per-call response includes actual stdout, stderr, exit and raw-byte hashes.
The call interface requires machine JSON output (`--format json`); other formats
are rejected before invocation so returned PIN/CAP identities cannot be silently
lost. Every actual invocation, including preparatory calls, has full raw stdout/stderr,
argv, cwd, start/end and outcome in sealed evidence. Business nonzero exits are
returned without stopping the trial, so the agent can correct them. A scope
rejection says `rejected`; it is never presented as actual CLI output.

A bounded JSONL relay is also available:

    node <runner>/tools/rust-skill-trial.mjs --historical-oracle-only relay <session.json>

Feed objects such as `{"op":"call","argv":["version"]}` or
`{"op":"finish"}`. The relay assigns sequences and prints one response per line.
EOF, malformed input, or input-stream cancellation requests cancellation if no
terminal request was submitted. An uncertain call is never retried. Explicit
cancellation is `cancel <session.json> <next-sequence>`. When a call is already running, EOF/input cancellation is observed after that
call returns (at most the 210-second command timeout); it is not an immediate
in-flight interrupt. A lost coordinator or unpublished request ends through the fixed idle/total timeout; the existing Job
retains process custody and orphan handling. No heartbeat or daemon is added.

For a partial task, supply the synthetic recovery key only after the independent
agent asks for it. The coordinator records that request using:

    node <runner>/tools/rust-skill-trial.mjs --historical-oracle-only restore-key <session.json> <next-sequence> '<the agent request>'

This changes only that owner's sample.keys, once. It does not run a recovery
query, remove previous captures, or imply recovery succeeded. No key is restored
by normal setup, finish, or a query. Do not expose the key/op proactively.

`finish_requested` acknowledges the request, not successful archival or quality.
Wait for the outer command's terminal owned-report.json. `completed=true` means
normal trial termination plus product/runner integrity and process custody;
`scored=false` and `completeSkillAcceptance=false` remain explicit. Cancel,
EOF and deadlines are not completed trials. Responses are atomically preserved
outside scratch, so final acknowledgments survive successful cleanup.

## Blind input and independent scoring

Give each fresh independent agent only its identical user task, inputs.json,
the neutral guide tree, and a tool that takes a JSON argv array and forwards it
through `call`. Do not give it the coordinator shell, general filesystem writes,
configuration, exporter descriptors, archive/evidence directory, or oracle.
The runner is a narrowly scoped CLI tool, **not an OS sandbox** for an agent
that has separate unrestricted shell or filesystem tools.

The coordinator must independently preflight actual target resolve/show against
the authenticated Data fixture and retain that evidence privately; the seeded
PIN alone is not a claim that these CLI operations ran. Likewise, perform the
required fixed Source corpus authentication separately and retain the actual
4,049-file/32,665,129-byte inventory check with the scorer. These prerequisite
checks do not count as the independent agent's calls or answers. If missing,
record the trial inputs as blocked rather than claiming formal acceptance.

The coordinator adds any task-specific colleague command to the task's separate
input handoff; do not modify this runner's inputs.json or select expected answers.
For the stale-flag task, preflight actual describe independently to confirm the
chosen stale token is absent; `--page-size` is deliberately allowed through this
scope guard so the real parser can reject it. Preserve that error and correction.
For Source, permitted sync is exactly wow-ui-source, retail, commit
31c7f7b9cc79e56c986b365c06a6afbcf3c9177b. The agent must actually discover/sync and
use its returned PIN. The fixed full-corpus oracle stays with the scorer;
source sync identity alone is not a full-corpus or API-answer pass.

The scorer separately reads actual outputs and all archived home bytes, checks
fixed-input/source identities, captures, partial coverage, mutation claims and
unchanged fixtures, and scores the agent's answer. A full-home chunk manifest
preserves source/cache/CAP/SQLite bytes before cleanup; fixture and complete
guide bytes are preserved separately. No synthetic result is a native result.
Task correctness, usefulness and comparative guide quality are never assigned
by the controller. Original failures and unexecuted tasks remain distinct.

## Limits and termination

- 40 agent CLI calls, at most 42 protocol requests (including optional key and terminal)
- Each request <=32 KiB; total accepted requests <=1 MiB; <=64 argv elements
- Each stdout/stderr stream <=4 MiB; total raw process output <=32 MiB
- Serialized response <=12 MiB; archive files split into <=8 MiB evidence chunks
- 20-minute total controller wall, 3-minute idle wait, <=210 seconds per command
- Existing outer Job has a further fixed 60 seconds for shutdown/archive
- One bounded fresh home snapshot, <=512 MiB, <=20,000 files, <=128 MiB per input file; any archival failure is explicit
- Before copying archives, existing disk admission checks measured home/guide/inbox bytes plus 16 MiB metadata reserve while preserving the normal free-space reserve

There are no user-selectable budget, executable, environment, cwd, home, output
path, arbitrary input file, or product-reference overrides. Allowed read/query
commands cover the four specified tasks. Live/game/addon/install, arbitrary
shell, deletion/cache cleanup, batch files and arbitrary filesystem targets are
rejected. SQL is passed to the actual product read-only parser; an attempted
UPDATE may produce an actual business error but cannot be reinterpreted as a
supported mutation by this runner.

Request publication uses exclusive temporary-file creation plus same-directory
rename, one publisher and one outstanding sequence. Duplicate, out-of-order,
foreign/reparse/hardlinked, malformed and oversized requests fail closed.
Partial temporary publication is never consumed and expires under the limits.
The outer owner independently authenticates every archive manifest and chunk,
compares them with the final live scratch inventory, and verifies archived
fixtures before allowing cleanup. Every owner seals evidence before cleanup. Failed/cancelled trials are also
retired only when process handles are reaped and the full archive was completed;
uncertain process custody or failed archival retains scratch for explicit
recovery. Do not delete retained scratch manually or label it completed.

## Tests

    node --test tests/rust-skill-usage/trial-runner.test.mjs tests/rust-skill-usage/data-runner.test.mjs

The original Data suite still exercises its scripted 42-command contract.
Windows native preparation, process-wrapper behavior, same-source fixtures and
blind trials require the existing authorized Windows test coordinator; none
were executed as part of creating this runner.
