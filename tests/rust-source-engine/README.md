# Direct EmmyLua library baseline probe

This is an opt-in example in the existing `lycheedev-source` crate. It does not
replace a public command, start a language server, or exercise the old broker.
The example directly owns one `EmmyLuaAnalysis`, loads explicit byte-verified
inputs, runs library queries on one thread, and drops the analysis before exit.
The normal product feature set does not enable this example.

Upstream identity: EmmyLua Analyzer Rust commit
`08b0873962e83bb8307e9a81052e3f41578fbc78`; parser and analysis packages report
`0.30.0`. Root controls dependency admission, lockfile changes, and compilation.
The locally admitted source is under `.tmp/rust-source-engine-research-01/upstream`.
The example's declared revision is metadata; the governed build's input identity
must separately prove which source actually entered the executable.

## Build and process contract

The root-owned build uses the existing workspace, toolchain, build cache, and
governed build channel with package `lycheedev-source`, example
`emmy_library_probe`, and feature `emmy-prototype`. There is no eighth product
crate or separate Cargo target directory. New direct dependencies are the two
admitted EmmyLua crates, `rowan = 0.17.0`, and `tokio-util = 0.7.19`; the example
also uses existing `serde`, `serde_json`, and `sha2` dependencies. It needs no
Tokio runtime, Rayon, custom allocator, or extra executable.

Always run the built example under the existing governed owned-process runner
with an explicit total deadline and process/job commit limit. The library's
token only detects cancellation before entering diagnostics; this example does
not claim cooperative cancellation during parsing, indexing, or type inference.
Do not confuse external termination, malformed output, or EOF with a successful
query or a known OOM cause. The external observer owns memory/handle measurements
and verifies that the process is reaped after success or failure.

Arguments passed to the built executable:

```text
golden [hold-ms]
retail <absolute-manifest.json> <absolute-source-root> <absolute-definitions.lua> [repeats] [hold-ms]
retail-cycles <absolute-manifest.json> <absolute-source-root> <absolute-definitions.lua> <cycles 2..3> [repeats 2..20] [hold-ms 0..250]
```

- In `retail`, `repeats` defaults to 3 and accepts 1–100. Acceptance of true repeated queries
  requires at least 2, regardless of the argument's diagnostic lower bound.
- In `golden` and `retail`, `hold-ms` defaults to 0 and accepts 0–5000. It pauses only at flushed resource
  milestones, allowing external sampling before/after Drop. Holds are excluded
  from individual load/query timings and remain within the outer total budget.
- In `retail-cycles`, the cycle count is required and accepts only 2–3.
  Repeats default to 3 and accept 2–20; holds default to 0 and accept 0–250 ms.
  A normal resource test uses `3 3 100` under one external 90-second deadline
  and one 2 GiB Job limit for the entire process. No cycle resets or extends
  those limits. The existing 1 MiB stdout cap also applies to the entire run.
- Inputs are read only. No Git, Go, Lua, luac, LSP, subprocess, network, or package
  installation is invoked by this probe. Lua source is parsed, never executed.
- Stdout is flushed JSONL with a hard 1 MiB byte limit. The terminal `end` record
  is required for completed execution. Exit 0 means this baseline's assertions
  passed; exit 1 means one or more precise assertions failed; exit 2 means input,
  API availability, I/O, or output-budget failure. An abnormal process exit is
  distinct from those states. No exit means full Source acceptance.

Events include configuration and embedded-resource hashes, input hashes,
assertions, input coverage, true-query timings, bounded samples, and resource
milestones (`before_load`, `after_load`, `after_actual_query`, `before_drop`,
`after_drop`). Query sample truncation is explicitly separate from the full
reference/diagnostic counts and hashes. The example does not implement pagination.
Every event has a `cycle` field: zero-based for `retail-cycles` work and null
for the existing single-snapshot modes or process-wide events. In particular,
resource samples can be matched to the exact cycle's milestone without relying
on reused phase names alone.

## Fixed small cases

The fifteen checked-in Lua fixtures are embedded by `include_str!`, except the
BOM fixture, which uses `include_bytes!` to preserve its leading three bytes. The probe
prints their actual bytes and SHA256 hashes. Expected declaration/reference
positions come from fixed source markers, not from the engine under test.
The assertions compare exact semantic IDs, byte ranges, complete reference sets,
call sets, diagnostic code/location, or the exact expected type.

The suite tests:

1. Lua 5.1 accepts `goto` as a variable and rejects a goto statement and label.
2. Lexical scope, block shadowing, outer-variable capture and visibility after
   leaving the inner block.
3. A global definition in one file referenced from another, excluding string
   and comment lookalikes from the complete expected reference set.
4. Plain value references are not call facts. Calls are collected only from
   actual `LuaCallExpr` callee nodes and then semantically bound.
5. A narrowly qualified initializer alias origin before a write; after a
   preceding assignment, this adapter returns `unknown_reassigned_before_call`.
   It does not claim full points-to or execution-order analysis. The library's
   lexical binding of both calls to the same local alias is asserted separately.
6. A table method's exact declaration and read/call references.
7. `---@type number` inference and an exact assignment-type-mismatch diagnostic.
8. A pre-cancelled diagnostic query returns unavailable. This does not exercise
   cancellation after query entry.
9. Unicode preceding an ASCII identifier preserves byte ranges and round-trips
   to the original source text. Scalar and UTF-16 columns are shown separately.
10. Ordinary assignment occurrences stay in references. Two same-name local
    declarations retain their own exact, disjoint write/read reference sets.
11. A BOM plus Chinese text and a supplementary Unicode character preserves
    original definition, reference, callee, and diagnostic byte positions.
12. Three alias call-value regressions exercise the same
    `initializer_origin_before` helper used by the earlier alias assertions:
    function-statement assignment to a visible local must invalidate the old
    origin; a call inside a deferred closure must return
    `unknown_call_execution_order`; copying a function into an alias must keep
    that value when the original variable is subsequently reassigned.
13. A two-edge copy chain retains its original function despite subsequent
    writes to both copied-from variables. A loop call returns execution-order
    unknown because later iterations can reach the same source position with a
    different function value.

These three new expectations concern the value at the selected call. Reporting
the historical initializer alone does not satisfy them. The first expects
`unknown_reassigned_before_call` with no origin. The deferred call also expects
no origin, since its lexical position does not prove execution order. The copy
case expects `initializer_function_origin` identifying the original function
declaration: the straight-line copy occurs before the source variable changes.
Each case records exact lexical bindings, initializer ranges, both assignment
and function-statement writes, all actual callee nodes, and the raw reference
partition. Run 03 records the three original regressions as failures, before
the helper was changed; those results and all original expectations remain.

The prototype helper now checks both assignment syntax and function-statement
writes to the exact visible local. While following initializer aliases, it
checks each source variable only up to the time that value was copied, then
continues through that initializer. A write to the source after copying does
not invalidate the captured value. Calls inside functions, conditionals or
loops return `unknown_call_execution_order`; the helper does not infer their
execution from textual order. Known results cover these bounded top-level,
same-file, straight-line initializer copies. General reassignment propagation,
interprocedural effects, closure execution order and points-to analysis remain
unsupported; a relevant prior write still gives an explicit unknown result.

Reference projection verifies the target's exact declaration/member identity
against its file facts. It excludes only that identity's exact declaration
range from the deduplicated raw occurrence set. It does not remove every write
or every occurrence of the same spelling. Every tested reference set retains
the sorted raw index multiset, count, SHA256 and bounded sample as evidence;
the declaration/reference partition must be disjoint and exhaustive. The
original global and method expected reference sets are unchanged.

The input adapter removes only an initial UTF-8 BOM from analyzed text. Raw
input SHA256 and byte count remain authoritative. Each changed file records
its analyzed SHA256 and the three-byte prefix map. Unchanged files use an
identity map. Definitions, references, callees, parser errors and diagnostics
are mapped to original UTF-8 bytes, with exact source-slice round-trip checks.
Diagnostic line/character fields explicitly use original Unicode-scalar
columns, matching the library's actual column unit; they do not claim UTF-16.

The first governed run confirmed that the upstream Lua 5.1 parser accepts
`::target::`: its lexer and label grammar have no Goto-feature gate. The test
deliberately expects rejection and remains a raw-engine capability failure. The suite continues
after a failed assertion so other results remain visible. This is evidence of
a compatibility gap, not permission to relax the requirement. Pinned luac 5.1.5
remains the required independent syntax authority and is not run here.
Terminal results separate `businessBaselinePassed` from
`rawEngineSyntaxCapability`. The overall `baselinePassed` and exit status
continue to include raw-engine failures; a passing business subset cannot hide
the label incompatibility or establish full Source acceptance.

## Complete fixed Retail input

Pass the existing independent input manifest:

```text
.tmp/rust-source-independent-retail-01/input/manifest.json
```

The argument itself must be absolute. Its required identity is:

| Field | Required value |
| --- | --- |
| schema | `lycheedev.source-memory-input.v1` |
| commit | `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b` |
| documents | 4036 |
| Lua files | 2551 |
| Lua bytes | 23354176 |
| document path/SHA256/size digest | `88d68f2c4bdb352245cbcb53cd41ec906b294e2ea599ee6374e00be169be7928` |
| environment bytes | 1039688 |
| environment SHA256 | `d5c3071e9c696cc19b02f609cae3fe8be19089ee5e434537bdf81f3f81b7c103` |

`source-root` is the root of an already materialized fixed source tree.
`definitions.lua` contains the exact existing environment declaration text,
extracted by the root's preparation step from the fixed environment package.
This program does not locate or generate either input automatically.

All 4036 manifest files are checked for their exact byte count and SHA256.
The manifest digest is recomputed in listed order using UTF-8 path, NUL, ASCII
SHA256, NUL, and little-endian u64 size. Duplicate Windows-case-folded paths,
non-relative paths, unsupported modes, symlinks/reparse points under the source
root, oversized files and changed bytes are rejected. Input caps are 4 MiB for
the manifest, 8 MiB per document, and 128 MiB for all documents. All 2551 Lua
original texts and all 1039688 original environment bytes are accounted for in
one bulk library load (only leading BOM bytes are omitted from analyzed text); no file is
removed because its parse has errors. The other documents are hash-verified but
TOC/XML semantics are explicitly unimplemented in this baseline.

The registered source, environment, and standard-library roots use fixed logical
paths beneath `C:/lycheedev-emmy-probe`. These paths are not created or read.
The explicit `Emmyrc` has Lua51 selected before any text is inserted; the entire
effective configuration and its hash are recorded. No configuration-discovery
function or Lua configuration loader is called. The 13 non-JIT standard-library
Lua resource files come from the compiled `include_dir` resources and have their
individual identities recorded. User-level EmmyLua resource caches are unused.

Each Retail iteration creates a fresh semantic model, looks up the exact
`CallbackRegistryMixin:RegisterCallback`, `UnregisterCallback`, and `TriggerEvent`
definition identities, queries the workspace reference index and inferred member
types, queries all real callees in CallbackRegistry.lua, and runs that file's
diagnostics. It compares full result hashes between iterations, while printing
only bounded samples. These are actual repeated library queries, not returned
result-cache entries. Immutable index caches are naturally retained by the
analysis owner. Full-workspace diagnostics are not claimed.

The fixed Retail set contains three BOM inputs: Blizzard_CharacterCustomize.lua,
ClubFinderApplicantList.lua and Constants.lua. The probe asserts that exact
changed-file set, unchanged raw identities and zero parser errors in those
files. It checks all their declaration/member, name/member occurrence and
callee ranges against original bytes, verifies declaration/name spelling, and
maps their diagnostics. Bounded samples, full range hashes and counts make
these checks auditable. Coverage retains the original 23354176-byte total and
separately reports the analyzed byte total.

## Outstanding acceptance

The separate opt-in `emmy_wow_binding_probe` now exercises one immutable graph
and common binding/type/call/check facade on seven fixed fixtures. It does not
alter this original library probe. Root's `wow-build.mjs` seals its EXE/PDB while
holding the governed Cargo channel, verifies all 848 pinned upstream files, and
retains the exact fixture identities. `wow-owned-probe.mjs` runs the sealed
artifact and seven real bundled-luac stdin compilations under one 30-second,
2-GiB native Job. `wow-independent-verify.mjs` checks executed graph facts by
original byte sites rather than trusting the example's own assertion names.

V2 `.tmp/rust-emmy-wow-02/` passed all 74 example assertions, seven syntax checks,
700 independent checks, exact phase/order key recomputation and native resource
retirement. Its source/binary/symbol identities are in
`.tmp/rust-emmy-wow-build-02/artifact.json`; strict Clippy passed under receipt
`6b40f461-990d-41a7-8d8b-973c7b059f59`. See `wow/README.md` for the bounded
primitive-signature scope. General type/flow/effects, real load/environment
integration and final engine selection remain open; a fixture profile assumes
shallow Mixin copy behavior without claiming native-client proof.

This baseline does not close the new Source goal. TOC/XML and XML source-map
adaptation, WoW Mixin behavior, general alias mutations, the full 16-case business
matrix, cross-CLI reuse, persistent cache invalidation, in-flight cancellation,
deadline/OOM recovery, performance acceptance, process-resource stability,
offline distribution, and corresponding-source rebuilds remain separate work.
The inherited API environment still has 3449 recorded extraction gaps; loading
every environment byte does not change its partial completeness status.

The first governed build succeeded. Its golden run passed 32 of 35 assertions;
the failures were label acceptance and raw global/member occurrence indexes
including their declarations. The first complete Retail run finished under
the root's 2 GiB/90-second limits and retained all 2551 Lua inputs; its six parser
errors were confined to the three leading BOMs. Those original reports remain
in `.tmp/rust-emmy-golden-01/report.json` and `.tmp/rust-emmy-retail-01/report.json`.
The second governed run (`.tmp/rust-emmy-golden-02/report.json`) passed all 54
business assertions and two raw syntax assertions; the label rejection
assertion remains failed (56/57 total). The complete second Retail run
(`.tmp/rust-emmy-retail-02/report.json`) passed all 17 assertions, including the
three BOM files' zero parser errors and original-byte readback checks.
Run 03 (`.tmp/rust-emmy-golden-03/report.json`) completed 69 assertions: all three
new alias call-value assertions failed, alongside the known raw label failure.
It returned the old original function for both the function-statement write
and deferred call, and incorrectly invalidated the copied function after the
source variable changed. That red evidence is retained. Run 04 passed all 74
business assertions after the helper correction and two supplementary cases;
the raw label assertion remains failed (76/77 total, exit 1). Its build receipt
is `57d90c02-6601-481a-938e-234baacd3664`. The same artifact's Retail03 baseline
passed all 17 assertions, with zero parser errors over all 2551 Lua files and
three fresh queries returning the same complete fact hash. These facts do not
close the remaining WoW binding and execution-order requirements.

## Native resource and fault measurements

`owned-resource-probe.mjs` seals the Retail03 EXE/PDB and derives a test-only
instrumented custody wrapper. It samples the original held native process
handle, retains the creation FILETIME as a decimal string, and reads kernel Job
peak commit, process CPU and I/O counters before releasing custody. There is no
PID reopen or source execution. The normal Job limit stays 2 GiB and the total
test deadline stays 90 seconds. Sampling is bounded to 3000 records at 40 ms.
Milestone intervals include intervening work; `holdSamples` in later reports
selects a conservative initial observation window during a declared pause.

The first normal measurement in `.tmp/rust-emmy-resource-01/report.json` passed
20 fresh semantic-model queries, all with fact hash
`80eab9870f39e1ca9de46bffdf42b45207709214ff3baf0c25040f5fbd7b77cd`.
Kernel peak Job commit was 1592569856 bytes. Query-phase private bytes stayed at
1573617664; after Drop they were 24342528. Handles stayed within 54–55; process,
tree, reader tasks and native handles were reaped. This is one full load and
one Drop, not proof that repeated snapshot rebuilds cannot leak. It is also not
the required alternating optimized Go/Rust performance matrix.

The private `retail-cycles` mode exercises repeated snapshot construction and
Drop in one process. Each cycle calls the complete existing Retail path: it
re-reads and verifies all 4036 documents, loads all 2551 Lua files plus the
complete fixed API environment into a new analysis, checks the BOM mappings,
runs fresh semantic queries and explicitly drops that snapshot. Only small
hash vectors survive between cycles; source snapshots and query results are
not reused. Existing within-cycle assertions remain, and each query hash in
later cycles is compared against the corresponding actual query in cycle 0.

`cycle_plan` records the requested counts and shared resource-boundary policy.
Each `cycle_start` is followed by cycle-indexed verification, query and resource
events. After `after_drop`, `cycle_end` records all ordered `queryFactsSHA256`
values, plus `bodyFactsSHA256`, the SHA256 of that vector's compact JSON. It
also records this cycle's assertion/failure counts and elapsed time including
holds. The final process `end` retains the existing assertion and acceptance
semantics. A `cycle_end` means queries returned and Drop ran; it does not claim
that the allocator released every byte or that no resources accumulated.
The external held-native-handle observer must compare the successive after-Drop
private-byte and handle baselines and verify final custody cleanup. This new
mode completed under build `89b7e361-2a97-4d7a-9f9d-683392dee743`.
`.tmp/rust-emmy-cycles-01/report.json` records three full constructions,
nine real queries, 57 passing assertions and a legal end at 78196 ms under the
unchanged 90-second/2-GiB limits. Each ordered query hash vector was independently
recomputed and matched the cycle report. Native after-Drop private bytes were
25423872, 25743360 and 25452544; handles remained 55 in all three observation
windows. Peak Job commit was 1596735488 bytes and all held resources were
reaped. These three repetitions show no continuing accumulation in that tested
sequence; they are not proof for unlimited runs or future cache/session designs.

The same sealed artifact was interrupted during actual index construction by
a test-only five-second deadline (`.tmp/rust-emmy-deadline-01/report.json`). The
owner recorded deadline retirement, native exit 8, no business end frame and
complete resource cleanup. A fresh normal process then loaded the full input
and completed 20 real queries (`.tmp/rust-emmy-post-deadline-01/report.json`).
This proves the prototype owner boundary, not cooperative library cancellation
or a public CLI typed-error contract.

A separate test lowered only the fault-injection Job limit to 512 MiB. In
`.tmp/rust-emmy-memory-limit-01/report.json`, Windows delivered the Job memory
limit message for the held root PID; stderr recorded a 40960-byte allocation
failure and the root exited with `0xC0000409`, without deadline/owner termination.
No end frame was emitted and all resources were reaped. The following fresh
normal 2 GiB run (`.tmp/rust-emmy-post-memory-limit-01/report.json`) completed
20 real queries. The abnormal exit alone would not prove allocation failure;
the positive native notification and stderr are separate evidence. A missing
notification would remain inconclusive. This intentionally reduced-limit test
does not show that the supported workload exceeds its normal 2 GiB limit.


### Private BindingContext/native typed-call experiment (implementation, not yet verified)

The immutable admitted Emmy source stays at `rust-source-engine-research-01`.
A separate copy under `.tmp/rust-source-binding-context-01/upstream` adds the
optional `BindingContext` entry point. The default model remains context-free.
`BindingDecision` has four authoritative states; only `NotManaged` takes the
ordinary database lookup path. The provider stores frozen graph decisions,
function/source identities, object epochs, and proofs. It cannot call a model,
supply a `LuaType`, or generate a diagnostic. Mutable query coverage lives in a
separate session, shared only by models of the same immutable context.

`emmy_wow_binding_probe` is now a thin wrapper over
`examples/support/wow_binding.rs`. Its V7 profile, 11 original fixtures,
10 scenarios, 86 expected assertions, and snapshot inputs remain intact.
The new `emmy_native_context_probe golden` shares that graph builder but chooses
`typed-call-v1`: generic and method closure identities are admitted independently
of V7's primitive-signature adapter. Three new files in `wow/native/` plus the
existing `wow/profile.lua` form the native input set. These source files are
parsed by Emmy; neither Lua source nor executable configuration is run.

The native path projects the selected foreign-file closure through Emmy's
`type_of_signature_in_file`, exact-closure overload candidates, native callable
selection and unification, and native parameter/return checkers. Explicit
parameter and return annotations are required in this first slice. Name/index
VM instructions retain the use syntax; call gates run before native builtin-name
dispatch. Model creation, foreign model creation, flow-sensitive expression
queries, and checker candidate fast paths preserve the context. Unknown/Any
results have unavailable coverage; empty diagnostics never imply checked input.
Other checker families are disabled in the serialized effective config.
References use the frozen value-reference overlay and retain execution contexts.
They are exposed as a separate value API, not merged with raw declaration refs.

The new probe holds one analysis database and three context models for
`after-a`, `unknown-a`, and `after-b`. Queries interleave those models in order
0,1,2,0,2,1 and retain original UTF-8 source ranges, selected closure proofs,
native LuaType/candidate data, and native diagnostic code/range evidence. The
cases cover cross-file generic Identity calls, a copied generic function after
source-field replacement, method colon/dot self mapping, wrong return types,
wrong argument types, unknown phase, and swapped script load order. This is
still a bounded experiment: general control flow/effects, higher-order foreign
callback inference, other diagnostic families, and full Source acceptance are
not implemented.

Both examples emit a `build_identity` event with the compile-time
`LYCHEEDEV_EMMY_CONTEXT_PATCH_SHA256`. The native probe refuses a missing or
invalid value and includes that exact patch identity in every snapshot key.
The old probe permits null for historical non-fork builds. Controlled build/run
and independent proof validation are owned by the root agent; this section is
not evidence of a successful compile or run.

The first sealed native run completed all 193 assertions: 186 passed and 7
failed; all four Lua 5.1 parses and process cleanup passed. Five failures came
from native overload matching discarding the selected `string` return when an
ordinary argument had the wrong type. Two came from the upstream return
checker's `Integer` versus `Number` compatibility route. Evidence is preserved
under `.tmp/rust-emmy-context-native-check-01/`.

Focused repair A now preserves a native candidate after argument-match failure
only when the call has a matching managed permit, the selected value is a
closure, there is exactly one non-generic native candidate, and its return is a
fixed primitive (`number`, `integer`, `string`, `boolean`, or `nil`). It enters
the existing native substitution and return-processing path; the native
parameter checker still reports the argument mismatch. Unmanaged calls,
unavailable bindings, ambiguous overloads, generic returns, and nested helper
calls cannot use this recovery. This repair has not yet been compiled or run.
The separate numeric compatibility failure and unknown-argument coverage
counterexample remain unchanged and unresolved.

Focused repair A was verified by the sealed second native run:
`.tmp/rust-emmy-context-native-check-02/report.json` records 191 passes and the
two existing numeric-compatibility failures out of the unchanged 193 checks.
All four real Lua 5.1 parses, transport/source/compiled-patch checks, and process
cleanup passed.

Focused repair B adds only `Integer` to the existing native return-checker
`IntegerConst`/`DocIntegerConst` to `Number` scalar compatibility rule when a
binding context is attached. The native generic solver widens numeric literals
to `Integer`, which remains within Lua 5.1's number domain. The added branch is
directional and does not add string-to-number or number-to-integer compatibility.
The context-free native compatibility path and its existing tests are unchanged.
B has not yet been compiled or run; all 193 expected outcomes and all Lua input
bytes are unchanged. The separate unknown-argument coverage counterexample and
broader object-version/resolver/reference integration remain unresolved.

The focused A/B native baseline has since passed all 193 fixed checks; the
original adapter baseline also passed all 86. These results do not establish
coverage beyond the documented slice.

A separate `emmy_native_context_probe coverage-red` command now tests the
remaining fixed-return/unknown-argument coverage inconsistency. It reuses the
same graph builder, `NativeInput`, immutable provider, and native checker path.
Its explicit input set is the original four files plus the 127-byte ASCII/LF
`wow/native/coverage-unknown-argument.lua`; its snapshot includes all five inputs
and a separate validation scope. `golden` retains its original four-file input
set and all 193 expected outcomes. No semantic-library code changes in this
red-test preparation.

The new command has 12 fixed assertions. It requires native `String` and
`native_projected` result coverage, unavailable argument checking, checked
return checking, and exactly one native `return-type-mismatch` over original
bytes `[98,122)`. The selected native candidate, unknown argument, graph facts,
and coverage proofs are emitted for independent inspection. Before a semantic
fix, the result-coverage assertion is expected to fail because the library
combines argument and result availability. This new command has not yet been
compiled or run; the expected red outcome is not verification evidence.

The coverage red run is now preserved at
`.tmp/rust-emmy-context-coverage-red-01/report.json`: all 12 fixed assertions ran,
11 passed, and only `native_result_coverage` failed as expected. All five real
Lua parses, the exact diagnostic range, argument/return coverage and proof
links, and process cleanup passed. The same binary also passed the ordinary
193-check golden baseline.

Focused repair C now records result availability solely from the native result:
`Unknown` or `Any` remains unavailable, while a known result remains projected
even if an argument cannot be checked. The native argument checker and its
unavailable coverage are unchanged. The ordinary query projection reads only
the same call site's `call_result` coverage and proof; it no longer combines
unavailable states from unrelated argument-check operations. Missing result
coverage is still unavailable. This repair has not yet been compiled or run;
the 193 golden and 12 coverage expectations and all five Lua inputs are unchanged.


The focused C run passed all 193 native checks and all 12 coverage checks;
the original 86-check adapter baseline also passed. This closes the result versus
argument coverage case, without completing object structure integration.

`emmy_native_context_probe object-red` is a separate test-only object-version
specification. It uses the original four native inputs plus the new LF fixture
`wow/native/object-versions.lua`, with explicit load order
profile/replace/defs/calls/object so `Identity` is the generic closure when the
new scenario starts. The snapshot adds validation scope
`object-version-structure-red-v1`. The existing golden and coverage commands,
expectations, and Lua bytes are unchanged; no semantic library changes were made
for this red preparation.

The fixture covers number-valued Source, shallow Copied, Source field overwrites
(including a static string key), a second Mixin into Copied, a same-shaped Twin,
Identity object round trips, and a dynamic-key write. The probe calls the actual
five public member APIs and public compatibility query, interleaves old/new/old
observations in one model, repeats member queries through a foreign model, and
saves direct-index facts, real provider identities/epochs, native type equality,
and independent reference sets. Expected diagnostics are exactly two native
parameter mismatches, on bad Source and copiedBad; the dynamic argument requires
unavailable checking rather than a definite mismatch.

There are 101 fixed assertions. Missing serialized object observations
(`UseFact.object_view` / `Graph.object_views`) and missing public structure-check
coverage are emitted as missing facts; the probe does not reconstruct shapes,
manufacture epochs, or supply native types. These missing capabilities and the
current raw member/compatibility paths are expected to produce RED results.
This command has not yet been compiled or run. Later stages must add actual
observation facts, native object identity and member projection, shared structure
checking, and generic conversion preservation while retaining these expectations.
Cycles, inheritance, metatables, general dynamic-key resolution and full Source
acceptance remain separate outstanding work.
