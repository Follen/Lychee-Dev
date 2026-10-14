# Frozen Go 3.2.0 → Rust static differential foundation

This is **test-only, source-derived evidence**, not a passing migration gate or a
release-binary behavior capture. The active Rust candidate currently fails the
static comparison. Missing commands, extra commands, legacy product names and
addon drift are not normalized, allowlisted, or accepted as parity.

## Run

Requires Node.js 20+ and Git; no npm dependencies, Cargo, Go compiler, Lua runtime,
network, game, downloaded release, or missing Emmy source is required. Run from
the checkout root:

```sh
node --test tests/rust-parity/oracle.test.mjs
node tests/rust-parity/run.mjs --verify-fixture
node tests/rust-parity/run.mjs
node tests/rust-parity/run.mjs --json
```

The unit tests verify the **runner**, including controlled equal and unequal
candidates. Passing unit tests do not make the real candidate pass. The normal
runner checks the frozen manifest against Git objects before comparing the
current worktree, including untracked addon files.

Exit codes:

- `0`: static equality only, or an explicitly requested fixture operation passed
- `1`: actual static mismatches; currently expected but still **FAIL**
- `2`: invalid invocation, missing objects, unreadable/oversized input, unsupported
  syntax, fixture corruption, or an otherwise unverified static extraction

An extraction marked unverified never passes. If another static check already
fails, the overall result stays fail (exit 1), with the unverified item retained.
Behavioral parity, source-family resolution, product validation, runtime,
performance, Windows and game acceptance are not established by this runner.

## Immutable inputs

The manifest `go-3.2.0.manifest.json` is derived from peeled commit
`3057a9666ab078270b63c7f06de748069b0d1f4b`, never the working Go files, a tag that
could move, or `main`. This commit must already exist in the local object store;
the runner does not fetch. It uses `git cat-file blob` and `git ls-tree` (the
object-reading equivalent of `git show <fixed-commit>:<path>`) without external
text conversion, shell expansion, or executing repository hooks.

The manifest contains:

- All 84 exact source-derived public definitions: path, summary, mutates,
  positional arguments, ordered flag display strings and explicit flag arities
- All seven data-product profiles, including folder, build series, reused-slot
  flag and regional-product → source-family mapping; both Forever beta tracks
  map to `forever` without merging their regional data identities
- All 55 `addon/` paths, byte lengths and SHA-256 hashes from that exact Go tree
- All 359 tracked `*_test.go` paths, byte lengths and SHA-256 hashes, with the
  Go→Rust test mapping explicitly `unreviewed`
- Input source paths, byte lengths and SHA-256 hashes

The command adapter statically reproduces the reviewed Go
`publicDefinition`, `acceptedFlagDisplays`, `sharedFlags`, `projectSelection`,
`dataSourceFlags`, `liveDoctorTargetFlags` and `dataProductFlag` behavior.
The complete command-contract and product-source hashes are hard-coded guards.
Any byte change requires explicit adapter review, not automatic acceptance.
The source-derived definition does not replace future describe/help captures
from a verified Go release binary or behavioral/default/exit-code fixtures.

For an intentional regeneration from **the same fixed commit**:

```sh
node tests/rust-parity/run.mjs --write-fixture
node tests/rust-parity/run.mjs --verify-fixture
```

This writes only the test manifest. A baseline change requires a separately
reviewed authority decision, commit/hash adapter change, and fixture diff. Do not
regenerate a fixture from the candidate or suppress an expected failure.

## Separate report dimensions

1. `commands`: the complete Rust `commands.json` inventory vs Go names and exact
   summaries/mutation/argument metadata. Inventory does not establish dispatch.
2. `flags`: exact ordered flag displays per command, including defaults described
   there. `parserFlagArity` separately compares Go arities with Rust's
   `Flag::takes_value` interpretation, guarded by the Rust contract-source hash.
3. `advertised`: static reconstruction of `contract::implemented()` using the
   current literal business-crate `commands()` slices. The filter is hash-locked;
   if it changes, this dimension becomes unverified until reviewed. Missing
   advertising does not prove no internal handler exists, and advertising does
   not prove a handler works.
4. `products`: canonical IDs, delivery product codes and build series from Rust's
   literal `PRODUCTS` catalog. `forever` and `cn-beta` remain extra legacy IDs,
   not substitutes for the expected canonical regional names. Folder discovery,
   reused-slot checks and API eligibility remain explicitly unverified.
5. `sourceFamilyMapping`: expected Go mapping and observed Rust UI-source tracks
   are displayed separately. A source track's presence is not proof that
   regional input resolves correctly or that cross-region isolation is enforced.
6. `addon`: missing, extra and byte-changed paths, with no line-ending or version
   normalization. The runner never copies oracle Lua into the active payload.

`--json` gives exact expected/actual field values, source hashes and the verified
fixture hash. Text output gives counts and affected paths. Neither representation
claims that registered APIs, old tests, or static equality prove runtime parity.

## Go test migration reference

The Go test inventory is an important behavioral reference, not a requirement to
mechanically copy Go implementation-specific, obsolete or platform fixtures.
For each migration module, the next bounded batch should map individual Go test
functions to Rust tests or record a reviewed non-applicability reason. A useful
mapping row must contain:

- Fixed Go file/hash and test function, tested behavior and fixture dependencies
- Platform/automation requirements (offline, automated Windows, or actual game)
- Rust test path/function and candidate source identity, or an explicit missing
  coverage item; language/engine-specific replacements need equivalent business
  assertions, not a blanket exemption
- `mapped-unverified`, `pass`, `fail`, `not_run`, or reviewed `not_applicable`, with
  exact execution evidence or a concrete rationale

Start with command/parser, selection/product, storage/delivery, Source business,
then Data and complete Live protocol/recovery. Pull transitive non-Go fixtures
only when the corresponding test needs them; do not copy them to active addon
payloads. Existing Rust tests and old passes must be re-evaluated against these
fixed Go business assertions. The current inventory does **not** claim any of the
359 files has been mapped or its tests run. Source-derived static equality also
does not satisfy those behavioral assertions.

## Safety and boundedness

Only literal syntax is parsed. Helpers are explicit test-code functions;
repository Lua, Go, Rust, configuration and scripts are never evaluated. Unknown
expressions, duplicate identities and unsupported declarations fail closed.
Limits are 2 MiB per file/Git blob, 500,000 lexer tokens, depth 64, 2,048 addon
filesystem entries, 64 MiB total addon bytes, and a separate 64 MiB Go-test-source
read budget (359 expected files). Each Git read has a 10-second
wall timeout and bounded output. Symlinks, special files, path traversal and
Windows drive/path separators are refused in input-relative paths. This is a
static test harness, not a security boundary against concurrent hostile
filesystem replacement.

Tests use fresh owned temporary directories and remove only those directories.
The symlink-rejection test requires symlink creation permission (on Windows use
a suitably configured automated runner); inability to create the fixture is a
visible test failure, not a skipped security check. The source catalog/parser
extractors are intentionally narrow: changed syntax requires review rather than
best-effort guesses or silently empty results.

## Retained run evidence

The branch-specific `Rust parity foundation (Windows)` workflow runs on pushes
to `rust-refactor`, even while its draft PR cannot produce a merge ref. Each exact
commit uploads `rust-static-parity-<commit>` with the actual comparison report.
The parity step fails when gaps remain; runner unit-test success is not product
parity. Local initial workspace evidence is retained separately because another
worker's uncommitted product migration was present during capture; it is not
presented as evidence for a tests-only commit. Rerun after each relevant batch.
