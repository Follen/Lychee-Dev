> Historical reference for the Rust migration (2026-10-14): current decisions, baseline and acceptance status are in [handover.md](../../handover.md). This document preserves its original scope and evidence; conflicting requirements, protocol designs, performance targets and release instructions are superseded. Historical test results are not current-candidate passes.

# Source redesign: business contracts and first direct-library evidence

Status: design and engine evaluation in progress. This is not a final engine
decision or completed Source migration. The user-edited goal from 2026-10-09
supersedes the earlier requirement to use tree-sitter and LuaLS together.

The optional owned-input library seam is now implemented and verified on fixed
cases. Its exact artifact identities, 485 assertions, independent checks and
unaccepted boundaries are recorded in
[the owned-input prototype report](rust-source-inputs-prototype-2026-10-09.md).
That evidence does not complete the business, resource or migration gates here.
The subsequent common-fact query facade and the real loop-scope defect/fix are
recorded in [the query prototype report](rust-source-query-prototype-2026-10-09.md).
The common control run, numeric projection repair and final fixed-case evidence are
recorded in [the control prototype report](rust-source-control-prototype-2026-10-09.md).
Authenticated host values, common argument/return slots, conditional return
fan-in and capture-cell references now have fixed-case execution evidence in
[the 2026-10-12 progress record](rust-host-and-runtime-progress-2026-10-12.md).
The subsequent explicit-entry/load-plan batch remains pending compilation and
execution; these milestones do not complete flow, full Retail or migration.

## Business interface and facts

The frozen Go v3.1.2 catalog contains 89 commands, including 12 Source commands:
list, sync, index, query, inspect, refs, context, session status, session close,
diff, prune and validate. Search, definitions, references, calls, source context
and flow are business capabilities projected through those actual commands.
Do not invent separate public commands from capability names.

The proposed Source module interface is `prepare(fixed inputs, control)`,
`query(snapshot, query, control)` and `manage(request, control)`. It owns input
verification, analysis, pagination, evidence publication and resource retirement.
Callers should not manage document-open ordering or analysis process internals.
Existing storage and synchronization can be reused when they satisfy this
interface; the old LSP and broker structure is not an architectural prerequisite.

Every snapshot binds exact source bytes and paths, source commit, TOC/XML load
closure and ordering, client API/environment identity, data-only configuration,
engine commit, rules, Lua dialect, source-map version and syntax checker identity.
The investigated source and repository configuration are never executed.

File positions, declarations, lexical bindings, references, call candidates and
flow facts form one auditable fact set. Keep these distinctions explicit:

| Axis | Required meaning |
| --- | --- |
| Input coverage | Which exact inputs were admitted and analyzed; no silently excluded files |
| Analysis completion | Whether the requested work completed under its budget |
| Output completeness | Pagination and truncation, independently of input coverage |
| Certainty | Text, syntax, semantic binding, supported inference or runtime unknown |

An incomplete API catalog affects conclusions that depend on its missing facts;
it does not automatically invalidate complete byte search or an independent
local binding. Conversely, missing API declarations cannot prove nonexistence.
Normal resolvable cases cannot pass by returning permanent partial or unknown.

Internally use original UTF-8 byte ranges and explicit mappings. Public legacy
positions require a documented projection; scalar columns returned by EmmyLua
must not be interpreted as UTF-16. XML needs decoded-byte to original-byte
segments for entities, CDATA, CRLF and synthetic handler binders. Existing
line-only mappings are insufficient for exact semantic locations.

WoW adaptation must model shared globals and file locals, ordered TOC/XML loads,
deferred function execution, Mixin composition and overriding. A shadowed helper
or dynamic source cannot receive a deterministic Mixin binding. Calls must be
actual callee expressions; ordinary references and lexical proximity are not
call or control-flow proofs. Cross-snapshot symbols require explicit matching.

### Binding integration decision still under evaluation

Independent read-only review of 12 hash-verified upstream files found that a
name/member resolver patch alone cannot establish coherent WoW semantics.
The inference VM falls back from unresolved names to global types, accesses
members through a separate type path, and merges multiple global writes into
overload candidates. Reference indexes are built through lower-level database
queries. Parameter checks can choose callable candidates from prefix types;
return checks use expression inference. Foreign semantic models and their caches
also lack a WoW execution-context identity. These paths can disagree even when a
definition projection appears correct. `Instr::IndexMember` does not carry the
use-site or object-write version needed by two reads around a Mixin overwrite.

Two routes remain under comparison. A separate adapter can reuse Emmy lexical
facts while owning a load/value graph and every affected type/check projection.
Its cost is additional expression and diagnostic logic. An upstream context
patch can reuse more inference logic, but must propagate authoritative binding
results through the VM, callable candidates, references, foreign models and
caches. Neither route may return corrected definitions alongside unadapted raw
types or diagnostics. Unavailable bindings must not fall back to future globals.
The next isolated example evaluates the first route without upstream changes;
this choice of prototype is not final engine selection or public migration.

The graph contract separates lexical declaration, storage cell, saved function
value, object field/write version, script load occurrence and execution context.
Each call has one selected value and signature, argument/return type decisions,
diagnostics and original-source evidence. Definitions, references, calls and
type display must consume that same fact. Signature reuse is initially limited
to explicit primitive annotations proved independent of the changed binding.
Unknown/Any must not silently turn an unsupported check into a passing check.

The initial independent business expectations are:

| Case | Required result |
| --- | --- |
| Top-level read before a later file writes `Later` | No binding to that future write; Nil requires a closed fixture environment |
| Deferred `Later(v)` | Unknown without an execution phase; an explicit, supported after-load phase may select the later function |
| Copy F1 into `x.run`, overwrite source with F2, then copy again | First reads retain F1; reads after the second copy select F2; a previously captured local still retains F1 |
| Locally shadowed `Mixin` | Bind the ordinary local helper and produce no intrinsic copy edge |

F1 has a number-to-number signature and F2 a string-to-string signature. Correct
and wrong arguments, return-type mismatches in captured/deferred functions,
inverse reference sets and source ranges must distinguish these values together.
Changing load order or execution context must change graph/query identity even
if file bytes are unchanged. Initial scenarios rebuild immutable graphs rather
than switching a phase on an existing cached semantic model. Runtime Mixin
behavior remains a source-supported profile assumption until the final client
acceptance; closed-fixture expectations are not native runtime proof.

The isolated implementation now has first native evidence for these expectations.
V1 build `7d3cd183-4383-4f98-b28e-4c135f47a490` completed six scenarios over seven
1466-byte fixed Lua fixtures. `.tmp/rust-emmy-wow-01/report.json` records all 74
prototype assertions and seven actual pinned-luac stdin checks passing under one
30-second/2-GiB native owner, with every process/handle reaped. Root independently
verified selected original declarations, correct/wrong argument types, captured
return mismatches, exact callee partitions and inverse graph projections.
The first independent verifier omitted source spelling in its own normalized
position record, yielding five comparison failures; its original code/report are
retained. After reading the original bytes into that record, the unchanged native
facts passed all 602 checks. No business expectation or source fixture changed.

Review of V1 found that its graph key omitted engine and effective-configuration
identity. V2 includes the pinned engine commit/archive/package versions, exact
dependency lock, full effective Emmyrc JSON/hash, rules version, original-byte
map profile, source identities, ordered load occurrences and execution phase.
The exact serialized hash input is retained in every graph for independent
recomputation. Build `ae83b5af-e4c8-49d6-b89f-14ed718db5e0`, source digest
`7d00e427bd185586d0733ef08f3a4bc0639e9ed13fb60eba2a3442414cf40005`, and
`.tmp/rust-emmy-wow-02/report.json` passed the same 74 assertions and seven actual
syntax checks. Root's `.tmp/rust-emmy-wow-02/independent.json` passed 700 checks,
including exact configuration/key recomputation and phase-only/TOC-only identity
changes. Swapping future-a/b changes the top-level read from Nil to F2/function;
the explicitly after-load deferred calls retain F2/string. Unknown-phase calls
retain unavailable checks and undetermined return mismatch decisions.

The build custodian hashes all 848 upstream files before and after compilation
and seals EXE/PDB plus fixtures inside the held Cargo lease. The execution runner
uses that sealed output rather than adopting whatever currently occupies target.
Input identities, frame schema, terminal state and assertion counts are checked
alongside independent semantic facts. Strict Clippy first rejected V1's reserved
NotManaged variant and three nested conditionals. V2 corrected them without
changing expectations; strict Clippy passed at
`6b40f461-990d-41a7-8d8b-973c7b059f59`. Its first attempt was refused by cache
admission before Cargo. Only this example's known retired incremental outputs
(31690460 bytes, 291 files) were subsequently inventoried and reclaimed under
both build leases; sealed/main binaries and symbols remained unchanged.

This proves the bounded fixture adapter slice, not a complete WoW analyzer.
Primitive signature/check logic is owned by the graph; upstream inference does
not independently reselect those callees. General type inference, control-flow
fixpoints, ordinary function effects, multi-return propagation, complete syntax
references in uncertain branches, real TOC/XML/API integration and native helper
semantics still require expansion and route-cost comparison. Return-expression
checks explicitly do not claim whole-function checking. No public Source command,
default engine, cache, session or release dependency has migrated from this pass.

Fresh independent implementation review then found four additional counterexamples
inside the accepted syntax subset. A red-only revision preserved all seven old
fixture bytes and original behavior expectations, adding four inputs/scenarios:

| Counterexample | Required result | Actual red result |
| --- | --- | --- |
| `mutate(t)` writes F2 into `t.run`, then `t.run(7)` | F2/string and a wrong-number argument diagnostic | Retained F1/number with no argument mismatch |
| `a, a.run = b, F1` with `old = a` | `old.run` selects F1; new `a.run` is Nil | Wrote F1 into the new table; old call had no function |
| Unsupported `t:run(7)` | Unavailable invocation/argument checking, consistent with the unavailable expression | Call check reported checked while the expression reported unsupported |
| A Mixin call inside an unknown branch | Subsequent member proof/check unavailable | Retained the old F1 and a checked call |

The assignment expectation follows [Lua5.1 assignment semantics](https://www.lua.org/manual/5.1/manual.html#2.4.3).
The conditional fixture uses `local Flag = ...`: an unknown chunk input is
distinct from an absent global in the closed fixture environment. Treating an
undeclared global as unknown would not establish this counterexample.

Build `cf28fa03-8eba-4907-970a-6d6b4f33da7c`, source
`064e77bc1983a8ced85cfbab826225377dbe1100e1c705253fd74c9c19cc906e`,
actually reproduced all four failures in `.tmp/rust-emmy-wow-red-01/report.json`.
All 11 real luac checks and native retirement passed; the 86 assertions contain
82 passes and four failures. `expectedRedVerified=true` is successful fault
reproduction, while `businessPassed=false` remains explicit. Root's independent
989-check report contains the same four exact failures and retains all earlier
supported expectations. These failures prevent accepting this adapter as a
complete Source engine, despite the previous bounded fixture pass.

Next work fixes these boundaries without lowering the concrete F2 or assignment
expectations. Calls need distinct declared-signature, argument-check, result and
effect coverage, linked to one call-result fact. The adapter's generic types,
flow and effects would require substantial additional analysis machinery.
After the proven correctness gaps are addressed, compare a real BindingContext
patch using the same business contract before committing to an expanded adapter.
The patch route still needs VM, references, foreign models and cache context;
it is not a two-resolver change and is not yet proved superior.

The first focused repair now freezes all assignment targets before writes.
Field targets retain the original ObjectId, static key and receiver fact; RHS
evaluation and later writes do not resolve the target through a newly assigned
variable. Original 11 fixture bytes and all 86 expectations remain. Build
`7d228fe6-870c-4e35-ba9b-aa154a8e340f`, source
`8866503626990647fb258f30141e87e5d570d058bc29b4554612030407426879`,
passed that exact assignment expectation in
`.tmp/rust-emmy-wow-assignment-fixed-01/report.json`. The complete matrix has
83 passes and the other three known failures; all 11 syntax compilations and
native retirement passed. Root independently confirmed only those same three
failures among 987 checks. Strict Clippy passed at
`189485ac-219b-4989-8554-89ade0ee5efe` after another receipt-verified reclaim of
33621089 bytes of this example's rebuildable incremental output. This is a
partial repair, with `businessPassed=false`, not final adapter acceptance.

The call-result contract now links each call to its exact result UseFact and
separates declaredReturnType, argumentCheckCoverage, invocationCoverage,
resultType/resultCoverage and effectCoverage. Legacy returnType is explicitly
the declared-signature alias, not a runtime-return claim. The first actual run
of this change (`.tmp/rust-emmy-wow-call-fixed-01/`) produced eight failures,
including six regressions in ordinary dot calls. Root read the pinned parser:
`get_colon_token` also returns a dot index token; only `is_colon_call()` checks
the token kind. That pre-existing expression bug had been hidden by checking the
old declaration-return projection without inspecting each call-expression result.
The exact failed build/output and matching symbols are retained; earlier passes
do not prove the newly strengthened result contract.

Using the actual colon predicate restored ordinary dot results while leaving
unsupported colon invocation/argument checks explicitly unavailable. Build
`8b473fb1-712a-40cc-a39d-d66af8221eb9`, source
`c2a379ec3d523cb8dcb0fa95fc0763e2fffaaa8645820fc6ed479b8e6fc74455`,
and `.tmp/rust-emmy-wow-call-fixed-02/report.json` record 84 passes and the two
remaining ordinary/conditional effect failures among all 86 assertions. All 11
syntax compilations and held native resources retired. Root's 1169 independent
checks include result-site/proof/type projection and actual normal call-expression
types; only those same two effect expectations fail. Strict Clippy passed at
`e4e2ff4b-4861-4d05-a3c8-dec115e986ad`. Ordinary effect summarization is the next
focused repair; no failed F2 expectation has been replaced by an Unknown pass.

That focused parameter-field summary now selects F2 after the actual mutator call.
It derives parameter declaration identity from the same Emmy lexical facts,
binds the actual argument's ObjectId, and records body receiver/RHS uses and the
field write with caller, parameter and source evidence. The bounded summary
accepts straight-line static parameter-field assignments only; it never executes
Lua. Deferred return-expression inspection rejects applying such effects to the
shared loading graph. Build `c40a6e45-85bf-429e-8f2f-3d7b5c1eb7f0`, source
`4b732e49c3fa2a08bafa56002270dccd4907c6a5c68a9df1f4b77edde869254b`,
passed 85 of 86 assertions and all 11 actual syntax checks. Root's 1195 checks
retain only the conditional-call failure; exact body/argument/object/write links
pass. Strict Clippy passed at `027a172f-b8ad-4e3c-b211-295b688fd217`.

The subsequent bounded branch footprint records condition facts, possible calls,
affected destinations, source versions and unavailable writes. It marks those
calls `conditional_possible` and does not execute copies or ordinary summaries.
The Mixin case invalidates only its destination's potentially copied fields;
other fields retain their existing facts. Build
`f855cd69-46a3-44c9-93fa-9415f0a33e9b`, source
`f935e51d0099ebc97517846164b4247cea8757b5759d05cf20e44d303fb2bbd1`,
passed the complete 86-assertion/11-syntax matrix in
`.tmp/rust-emmy-wow-branch-fixed-01/report.json`, with all native resources reaped.
Root's `independent-02.json` passed 1241 checks, including the exact condition,
possible-call, source/destination and later unavailable-read chain. The first
independent report contains only a Root CLI spelling error in the expected rules
version and is retained. Strict Clippy passed at
`a104e248-1b62-4181-af6e-fff4c4ce02a5`. Both Clippy admission retries reclaimed
only known retired example incremental output under both build leases; cache
quotas, test budgets, source fixture bytes and behavioral expectations did not
increase or shrink.

### Native type-system comparison now starting

The bounded adapter remains a comparison implementation. It supplies primitive
signature strings and excludes generic/method typing, general effects and full
flow; passing the above matrix does not select it as the final Source engine.
The next experiment lets the immutable value graph choose closure/object versions
while Emmy's native signature, overload/unification and checkers compute types
and diagnostics. Providers must not feed precomputed returns or diagnostics into
that experiment. Four-state authority, coverage gaps, foreign models, reference
overlays and immutable context-specific caches are part of the same slice.

V7's primitive-signature prerequisite and unsupported-colon policy remain its
original capability profile. A separately versioned typed-call profile will
exercise cross-file and copied generics plus normal generic methods; native
enhancements must not be mislabeled as unchanged V7 behavior.

Root admitted `.tmp/rust-source-binding-context-01/upstream` as an exact private
copy of all 848 pinned files/8382174 bytes, preserving the original tree. Optional
evaluation dependencies now point to this copy. Locked/offline Cargo metadata
under receipt `ee79e3b5-ef9f-4425-a910-9d98233337ca` confirms the same seven product
workspace members and unchanged Cargo.lock; no compilation occurred at admission.
The old original-engine build sealer rejects this fork path until the actual
patch inventory is frozen and attested by a context-aware sealer. Engine/package
version metadata cannot substitute for that patch identity. Product default
Source, public commands, session/cache management and release packaging remain
unmigrated pending the complete design/prototype gates.

The context-aware custody scripts now inventory every actual fork file and
every difference against the immutable 848-file base. Both freeze and verification
enforce the first-slice semantic-Rust path boundary, all shared example Rust
modules, original fixture hashes, Cargo inputs and the custody scripts themselves.
The pinned V7 contract contains the exact 86 assertion names/expected values and
11 input hashes from the actual passing sealed V7 report. The normal runner now
rejects missing or changed assertions, as well as altered fixture bytes.

An independent static review found substring Cargo-wiring checks, missing
verification admission rules, omitted shared modules, reduced green assertion
sets and incomplete compilation-environment identity. The scripts now reject
unsupported TOML shapes (including multiline strings and quoted tables), require
the exact optional pinned dependencies and feature boundary, and bind a separate
artifact execution identity to the frozen patch, EXE/PDB and actual invocation
environment digest. The new context execution also requires a binary-emitted
`build_identity` patch digest; this adds provenance without changing the V7 graph
or its primitive-capability expectations.

Actual `.tmp/rust-context-custody-checks-04/report.json` records four owned Node
process refusals and eleven admission/contract checks, all passing with all
handles reaped and no new Cargo receipt. These are custody checks, not a frozen
BindingContext build or native generic/method acceptance. The semantic fork and
typed-call probe remain under implementation; their patch inventory has not yet
been frozen, compiled or accepted.

`context-native-owned.mjs` is prepared and syntax-checked for the separate native
profile. It requires a fixed expectation contract included in the pre-build
freeze, the actual native compilation receipt, compiler-patch identity emitted by
the binary, exact input identity, bundled luac stdin checks and a bounded 2 GiB /
30-second owned process tree. No execution of this new runner has been accepted;
the typed fixtures and expected contract still need their final independent
source review before freezing. This was the custody-preparation state; the
actual first-slice results below supersede that execution status.

### First native BindingContext slice: actual result and boundaries

The private patch changes ten semantic Rust files, preserving the original
848-file upstream. The parser, overload/unification implementation and default
non-context type compatibility remain pinned. The two examples share one graph
builder; the native provider supplies frozen value/closure/parameter identities,
call permissions and reference overlays, never precomputed types or diagnostics.

The first actual context build failed on two denied unused qualifications in
`lib.rs`; its source, before/after patch bytes and logs remain in
`.tmp/rust-emmy-context-build-fail-01`. Removing the redundant qualifiers allowed
the build. The first native run completed all 193 checks with 186 passing and
seven failing, while compilation identity, input hashes, four real bundled-luac
stdin checks and process cleanup passed. The selected closures were correct.
Five failures came from coupling fixed return inference to argument matching;
two came from rejecting a native `Integer` result against `number` in return
checking. The independent review found no evidence of context-cache mixing.

The first focused repair retains a single selected native candidate with a fixed
primitive return after parameter matching fails, only for an exact managed call
permission and bound closure. It continues the existing native return inference
and parameter diagnostics, with no fallback for multiple candidates, generics or
unavailable bindings. Its actual run passed 191/193, retaining the two numeric
failures. The second repair adds `Integer` to the existing numeric return rule
only in context-bound models; the no-context compatibility contract stays intact.

Patch `8718763075d0110a0b72e64e9f83aec740e767735a78c971e289b31c06475bff`
under the pre-style-fix source seal passed all 193 native checks in
`.tmp/rust-emmy-context-native-check-03/report.json`. All four syntax inputs passed
real Lua 5.1.5 compilation, and the owned process tree was reaped. Root additionally
verified 1365 independent checks for all 54 query sites, selected function/value
identities, native types/candidates, callee/result proof links, exact diagnostic
code/range sets, foreign parameter identity and value-overlay references. The
first independent verifier run used incorrect JSON field names; that report and
runner snapshot remain preserved, and correcting schema reads passed the same
semantic expectations.

The same patch also passed the unchanged original 86 checks and eleven actual
Lua syntax checks in `.tmp/rust-emmy-context-adapter-check-02`, plus Root's 1241
independent graph checks. All checks use the fixed original fixture hashes and
assertion contracts. These are bounded capability-profile results, not full
Source acceptance or an engine-selection decision.

Strict Clippy then failed on a native-only constant left in the shared module.
The constant has been moved to the native module; this style-only source update
is sealed under `.tmp/rust-emmy-context-freeze-05`. Its affected recompilation and
strict check are pending cache admission, so the current edited source is not
yet recorded as fully verified. The prior successful binaries and matching PDBs,
all failures and exact source inputs remain preserved.

That style repair was subsequently verified under source
`b15943b014e5fd5d8a9b9a175d5b86b3f9820137cabf1bd8cfd6d8303dfc45dc`:
strict Clippy receipt `6d0bfbe8-d40c-4f33-8ad8-8ed3d61d8511` passed, and
`.tmp/rust-emmy-context-native-check-04` passed the original 193 checks and
Root's 1365 independent checks. Its separate five-input coverage reproduction
completed twelve checks with exactly one expected failure: the result coverage
was unavailable despite native `String`, a fixed return mismatch at `[98,122)`,
an independently unavailable argument check and a checked return. The red report
is `.tmp/rust-emmy-context-coverage-red-01`; its reproduction success does not
mean passing business behavior.

The focused coverage repair separates native result availability from argument
availability. Ordinary query projection now consumes the same `call_result`
record and proof rather than aggregating unrelated argument-check failures.
Unknown/Any native results and absent result records remain unavailable.
Patch `bf069c91c05d0df2fe775591400fd500d51cf79f0d1955d1f7e2330ee71bb6c0`
under source `77d1d5ef2cd2fd4669b4402f22a4e2e1f0f6172f3f1ed59f4628798fe69d58dd`
passed all twelve original coverage expectations in
`.tmp/rust-emmy-context-coverage-fixed-01`, plus 44 Root checks of native types,
source bytes, exact diagnostics and argument/result/return proof links. The same
binary passed all 193 original checks and 1365 independent checks in
`.tmp/rust-emmy-context-native-check-05`. All five coverage inputs were actually
compiled by bundled luac5.1.5; all owned processes and handles retired.

The unchanged original adapter contract also passed 86 checks, eleven real
syntax compilations and 1241 independent checks on that same patch in
`.tmp/rust-emmy-context-adapter-check-03`. Strict Clippy receipt
`5e9aa36f-faf6-4278-98df-bd6bbdff44ad` passed with the complete frozen private
inventory verified before and after checking. All previous failures, source
inputs and matching EXE/PDB pairs remain preserved.

The result/argument coverage case above is now closed for its fixed scope.
Object epochs currently select graph fields but
are not retained in native structural table/member caches. Public raw resolver
and reference-index entry points are not fully context-aware; the tested
references use the session overlay. General effects, full flow, the complete
business matrix, full-Retail context/resource behavior and default migration
remain unaccepted.

Further inspection also found that the first object epoch digest used the
variable cell write list, which does not change when the object's fields change.
The graph does not yet freeze complete object/heap observation versions.
Structural member/parameter work must first preserve those versions and shallow
copy value identities, then integrate context-owned member projection and native
field checking. Merely forwarding the current epoch into `LuaType` would not
repair that fact-layer gap; permissive native object compatibility must not be
misreported as an actually checked structural argument.

## First engine candidate and source custody

Candidate: EmmyLua Analyzer Rust parser and analysis libraries, commit
`08b0873962e83bb8307e9a81052e3f41578fbc78`, package version 0.30.0, upstream MIT.
The release tag 0.25.1 points to a different commit and is not this input.

Root acquired the fixed official HTTPS source archive, verified its commit PAX
record, refused links and escaping paths, and hashed all848 files/8382174 bytes.
Archive SHA256:
`ae529a06b551dbb301744bea2b58ba94cb3e30340f63a8c83f9335be9086fdb7`.
The immutable file inventory is
`.tmp/rust-source-engine-research-01/source-identity.json`.

The evaluation feature `emmy-prototype` in the existing Source crate calls the
library directly. Product default commands remain on their current path until
design and prototype gates close. The workspace still has seven product crates.
Upstream source is unmodified; its separate workspace is explicitly excluded
from automatic membership in the product workspace.

Dependency preparation01 failed because the nested path package inherited the
wrong workspace. Preparation02 resolved the closure without compiling, saved
before/after lockfiles and verified the848 upstream hashes unchanged:
`.tmp/rust-emmy-dependency-prepare-02/report.json`. The updated Cargo.lock also
changes shared dependency versions; earlier module passes are not automatically
acceptance for a later candidate.

## Verified first execution

Build receipt `8f45beaf-e86b-46d9-812b-1db54d0c24c0` succeeded under the controlled
ordinary Cargo channel, Rust1.99 Windows MSVC, locked/offline. Source digest:
`70a888ed3d530113174c04c053ca9142324f7089f063b744612ee537bff654f4`.
The example, matching PDB and the execution custody wrapper are sealed in each
execution directory. The wrapper sets an actual2GiB Job commit ceiling before
resuming the child; no analysis server or LuaLS child runs.

`.tmp/rust-emmy-golden-01/report.json` reached a legal terminal record:
35 exact assertions,32 passed/3 failed, exit1, about38ms of probe execution.
All original process handles and descendants were reaped. Passed cases include
scope/shadow/upvalue, cross-file definition binding, excluding non-callee reads
from calls, a bounded initializer-alias inference with later reassignment marked
unknown, member binding, type/diagnostic checks and exact Unicode byte ranges.

The three failures are retained:

- Lua51 parser accepts a label. This is a real compatibility gap; fixed
  luac5.1.5 remains the syntax authority and must be integrated without executing
  investigated code.
- Global and member raw reference indexes include their declaration locations.
  The business projection must classify an exact semantic identity and range as
  declaration or reference. Do not loosen the original expected reference sets
  or remove matches merely by name, line or overlapping range.

`.tmp/rust-emmy-retail-01/report.json` completed a separate baseline, exit0,
about24.520s in an unoptimized build under the same2GiB/90s limits. It verified
all4036 source files (2551 Lua,1091 XML,394 TOC), admitted all2551 Lua files and
23354176 Lua bytes plus the complete1039688-byte API declaration input. The API
input SHA256 is `d5c3071e9c696cc19b02f609cae3fe8be19089ee5e434537bdf81f3f81b7c103`.
Three actual semantic queries took52.621/49.909/51.684ms and returned identical
full fact hashes:
`db5051c851b55441de894dc504bcd75c629d3b12766ed769164fb937c138a26c`.
These queries bypassed result caches. Drop and process termination completed.

This baseline is intentionally narrower than full Source acceptance. Three
files produced six parse errors at byte0..3. Root independently read the original
prefixes of all three files and confirmed UTF-8 BOM bytes EF-BB-BF. A BOM adapter
must preserve original bytes and translate analyzed ranges back by the recorded
prefix mapping; no file can be excluded to hide this gap. TOC/XML files were
verified but not semantically adapted, inherited API
extraction still has gaps, and raw library reference counts are not yet the final
public reference contract. No optimized performance, peak-memory measurement,
cross-CLI session, cancellation-in-flight or recovery claim follows from this run.

## Resources, faults and migration obligations

### Subsequent verified prototype evidence

The exact-identity reference projection and BOM adapter are implemented in the
isolated example. Golden02 passed 54 business assertions; original occurrence
multisets, hashes and exact declaration partitions remain visible. Retail02 and
Retail03 retained all 2551 Lua files and original byte hashes, with zero parser
errors after only the three leading BOM prefixes were mapped. Diagnostics use
the upstream's actual Unicode-scalar column unit and project to original bytes.
The independent fixed luac 5.1.5 stdin suite passed seven compatibility cases,
including legal goto identifiers, rejected goto/label syntax, BOM+Unicode and a
source that would throw if executed. luac has not yet been integrated into the
whole program or XML analysis gate.

Independent review found three call-value counterexamples in the alias helper.
Golden03 preserved their failures; Golden04 passed all 74 business assertions
after function-statement writes, delayed execution and copy-time observations
were corrected. All old expectations remained. The raw engine label rejection
still fails; Golden04's overall exit is 1, not a full green result. A subsequent
review found multi-assignment return-slot handling missing. Golden05 preserved
two exact failures (89 business assertions, two failed); Golden06 passed all 94
business assertions after nonzero single-value slots and a nil-copy chain were
handled. Its total remains 96/97 with the raw label failure retained. No final engine selection
or public-command migration follows from these prototype passes.

Native held-handle resource sampling over the same Retail03 artifact completed
20 fresh queries under the unchanged 2 GiB/90-second normal limits. Kernel peak
Job commit was 1592569856 bytes; query private bytes stayed at 1573617664 and
fell to 24342528 after Drop. Original process/tree/reader/handle cleanup passed.
The exact evidence is `.tmp/rust-emmy-resource-01/report.json`. These are one
construction and one release, not repeated-rebuild or cross-CLI cache evidence.

Test-only five-second hard-deadline retirement during full index construction
passed, followed by a full normal load and 20 true queries. Separate 512 MiB
allocation fault injection produced a positive native Job memory-limit message,
an allocation-failure stderr and `0xC0000409` without owner termination; a fresh
normal 2 GiB load then completed 20 true queries. Both failure processes emitted
no business end frame and all held resources were retired. Reports are
`.tmp/rust-emmy-deadline-01/`, `.tmp/rust-emmy-post-deadline-01/`,
`.tmp/rust-emmy-memory-limit-01/` and `.tmp/rust-emmy-post-memory-limit-01/`.
These establish prototype fault isolation, not public typed-error handling or
cooperative in-flight cancellation. The Windows Job allocation limit and
positive-notification meaning were checked against the installed SDK and
[Microsoft's Job limit documentation](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_limit_information).
Notification absence is not a negative proof, per
[the completion-port contract](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_associate_completion_port).

### XML extraction evidence and remaining gates

The independent XML example uses the exact Retail UI.xsd companion hash
`3addc00906cd293304c48c5c2642bdd9962b4f6402b2605e21f4975c4254771c`.
Original source input manifest bytes remain unchanged. XML01/02 passed 23
checks and six real pinned-luac stdin compilations, including expected invalid
Lua rejection. Names and calls are parser facts only; implicit parameters,
bindings, types and WoW execution phases are not implemented.

Actual `.tmp/rust-emmy-xml-retail-01/report.json` verifies all 4036 original
files and attempts all 1091 XML files in one owner: 999 extracted, 84 unsupported
and eight failed. All file identities, complete program/map/fact hashes and
bounded failure samples are retained. Unsupported counts comprise 76 missing
handler-catalog files and eight Bindings roots. Independent readback confirmed
the 76 have names from omitted animation script types in the same pinned XSD;
no input can be dropped to hide this gap. Bindings roots contain actual Lua
business bodies and require a dedicated mapped adapter, not just root-name
acceptance. The eight failures are all eight BOM-prefixed XML files in this
corpus. A reader cursor excluding the BOM was treated as a raw byte offset,
causing wrong self-closing/body/stack classification. Their source hashes and
direct trigger ranges were independently checked; this is an adapter defect,
not evidence that those original structures are malformed.

Six independent XML well-formedness negatives were actually accepted by the
sealed XML01 extractor (`.tmp/rust-emmy-xml-wellformed-red-01/report.json`). They
cover a raw less-than sign in an attribute, a forbidden CDATA close in ordinary
text, BOM/NBSP after the root, and numeric-leading element/attribute names. These
are basic [XML 1.0 lexical rules](https://www.w3.org/TR/2008/REC-xml-20081126/),
not a request to add full XSD validation. Transactional rejection remains a
required repair; the existing profile label cannot excuse accepting bad input.

Actual entity-CR diagnostics reproduced two mapping failures in
`.tmp/rust-emmy-xml-cr-red-02/report.json`: luac line 2 was unavailable for a
top-level chunk, and handler line 3 mapped to synthetic `end`. The earlier
CR-red01 was a test-harness filename error and remains separate. XML03 now
passes 31 golden checks, six real compilations and both original CR regressions
after matching fixed Lua5.1.5's CR/LF/CRLF/LFCR byte-line rules. Entity text is
unchanged; columns remain unknown. `xml-map-v2` participates in snapshot identity.
The failed first large JSON macro build, matching source and diagnostics are
preserved in `.tmp/rust-xml-retail-build-fail-01/`; grouping serialization fields
fixed compilation without increasing a compiler or operation budget. Strict
Clippy subsequently found two collapsible conditionals in the XML example;
their correction passed strict Clippy under receipt
`f905be79-4adb-4e91-8deb-ff507e216b88`.

The focused BOM repair preserves `Event::Empty` and maps every numeric reader
cursor through the original three-byte prefix. Borrowed raw slices are already
original coordinates and receive no second shift. XML04 passed 40 golden checks
and the six actual compilations, under build
`32da4dc6-e111-4267-a5f0-89dfa4b49262` and source hash
`91f248d4cd3607ad28bf2e237b3439fa656337f1b0e6aad3040ef897bc704b64`.
The second full Retail run attempted all 1091 XML files, extracted 1003, and
reported zero structural failures. Eighty files still report an unsupported
handler and eight report Bindings roots; four of the repaired BOM files now
reach the catalog gap that their earlier structural error had hidden. The
overall test remains failed. `xml-map-v3` identifies the repaired mapping rules.
Full input coverage and removal of those eight adapter errors are separate
from complete XML support.

The catalogue gap subsequently closed against the same pinned XSD's four
script types, with owner/type, extension and substitution provenance retained.
XML05 passed all 52 golden checks and six actual compilations; strict Clippy
passed. Build receipt is `7181c606-6b84-4618-ac95-5ba3067f2d14`, source hash
`48cd1a1886c8b58409ad81bddbc23ff04a812ca45dcde2f89c2185b98e5346e6`.
The third full run (`.tmp/rust-emmy-xml-retail-03/report.json`) attempted all
1091 XML files, extracted 1083, and retained eight unsupported Bindings roots
with zero structural failures. Its overall exit remains 1. Owner provenance
is not full XSD validation or an implicit handler-parameter signature.
`xml-map-v4` covers these catalogue rules. All actual semantic bindings remain
separate work.

XML06 adds the XML1.0 Unicode name ranges, exact XML whitespace outside the
root, raw attribute less-than and plain-text CDATA-close checks. It passed 65
golden assertions, 25 actual pinned-luac compilations and the original six
well-formedness negatives under build `89b7e361-2a97-4d7a-9f9d-683392dee743`,
source `46e937a5d56b2f2f0d20cdea9e24a5aacc32d7955e8807caf9c8171e7703c2bc`.
The explicit byte-slice type compile repair and first failure are preserved in
`.tmp/rust-xml-lexical-build-fail-01/`. The full fourth XML run still extracts
1083 and reports only the eight unsupported Bindings roots. A subsequent fresh
independent review found attribute separator and full XML declaration grammar
checks still missing. Six new exact negatives were actually accepted by the
sealed XML06 artifact (`.tmp/rust-emmy-xml-declaration-red-02/report.json`).
This defect remains open; six earlier negative passes did not prove all XML
well-formedness.

Bindings v6 adds source-supported declarations and mapped Lua bodies, preserving
all attributes, duplicates, empty declarations and ModifiedClick entries rather
than treating names as Lua globals. XML07 passed 85 checks and 27 real luac
compilations; strict Clippy passed under source
`d271a81c0afe837110f85d684e42d862e4b72c64cfa33e263fafbbe9a4e163d3`.
The fifth full XML run attempted all 1091 files and extracted 1090, reporting
one profile failure in Commentator/Bindings.xml. Independent original-byte
readback found `</Binding>>`, with an extra greater-than character at
`[1132,1133)`. That character is legal XML data inside the root. It must be
preserved as a profile diagnostic rather than repaired or converted to Lua;
discarding the file also discards 57 valid Binding bodies. The next focused
adapter revision separates complete extraction from unresolved profile
validation. No native-loader behavior has been proved for that source anomaly.

That v7 revision completed extraction of all 1091 XML files with zero excluded,
unsupported or unprocessed files. It retains all 1033 Binding declarations,
1025 Binding bodies and 77 ModifiedClick declarations across the exact eight
184481-byte inputs. Commentator's complete root text `[1132,1136)` and typed
diagnostic `[1132,1133)` are independently asserted by
`.tmp/rust-emmy-xml-retail-06/report.json`. The resulting state is
`extracted_with_profile_diagnostics`; `profileValidationPassed=false` and
native behavior remain unresolved. The test's passing expected-diagnostic
contract is not an all-green validation claim. XML08 passed 92 golden checks
and 29 real luac compilations under build
`46933388-9e2a-4507-803f-08cfcf9b0dec`, source
`ab02076334ccad7da483bbdb4df087f60f3046d4de753f8d1893e3364df71fd5`.

The attribute/declaration grammar repair subsequently passed all original six
new negatives, 104 golden checks, 30 real luac compilations and strict Clippy.
Build is `9673b6b6-e2cf-49bd-8248-eb600e6862f2`, source
`b71b2a697d6213e4a8034c93fc9c3a3da473446f26d87e4e66b5fc935ae213d5`.
XML09/full-Retail07 retains all the same declarations and the same unresolved
Commentator diagnostic. No expected negative was relaxed in this batch.
Only the earlier v7 root-text expectation changed, to distinguish valid XML
character data from profile-valid Lua placement; the exact input and old
failure reports are retained.

Actual full syntax authority was also exercised over the fixed complete input,
not just small fixtures. `.tmp/rust-emmy-full-syntax-01/report.json` authenticated
all 4036 files and original 32269051 bytes, then invoked bundled luac5.1.5 via
stdin for all 2551 Lua files and all 4018 mapped XML compiler sources. All 6569
compiled without a syntax failure in 70488 ms, under one native 2-GiB/90-second
custody owner, with a bounded four-child compiler queue and no result cache.
The test worker's total budget is 85 seconds, leaving owner retirement time;
budgets are not restarted per file. Investigated code was never executed.
The one existing profile diagnostic remains distinct from syntax results.
XML extraction and syntax compilation still do not establish WoW name/type
binding, load-phase semantics or performance against Go. The latest lexical
parser change passed a separate all-4018-XML compilation regression in 57408 ms
(`.tmp/rust-emmy-full-xml-syntax-02/report.json`); original Lua bytes and their
full syntax evidence remain unchanged.

The direct-library owner also completed three same-process full Retail
constructions, queries and releases (`.tmp/rust-emmy-cycles-01/report.json`):
57 assertions, nine uncached queries with the same exact facts, 78196 ms total.
After-Drop private bytes were 25423872, 25743360 and 25452544; all three native
handle baselines were 55. Kernel peak Job commit was 1596735488 bytes, under
the unchanged 2-GiB limit, with all custody resources retired. This finite
sequence shows no continuing accumulation. It does not replace cross-CLI,
persistent-cache, total-budget cancellation or optimized Go/Rust acceptance.

Independent pinned-source review and root readback confirmed the selected
`Emmyrc -> VFS -> semantic_model/type/diagnose` path parses and infers without
executing investigated Lua or annotations. The Rust `InferVm` operates on
`LuaType` values; it is not the luars execution VM. This guarantee depends on
the API boundary: public `load_configs(_raw)` can execute Lua configuration in
a sandbox exposing os/package/require, and `pre_process_emmyrc` can launch
`luarocks`. Neither API is called by the prototype, and neither may enter the
new input owner. All 848 pinned Emmy files and the 137 locked luars files were
checked against their source identities; a sandbox label is not a substitute
for this explicit no-execution boundary.

The next BOM build was initially refused before compilation by the configured
cache admission reserve. Under both build-channel leases, verified retired
owners' incremental caches for the two examples (78223719 bytes) and the pinned
analysis library (212951072 bytes) were reclaimed after full file inventories,
source hashes, idle-process checks and protection of main/sealed EXE/PDB pairs.
No dependency output, symbol output or unknown cache was removed, and the
20 GiB cap was unchanged. Reports are `.tmp/rust-emmy-incremental-reclaim-01/`
and `.tmp/rust-emmy-incremental-reclaim-02/`. A preparatory Node import-path
failure occurred before reclamation and is separate from both completed runs.

First evaluate one controlled process owning one analysis on one thread. Avoid
the upstream per-file OS-thread helper and do not treat unsafe Send/Sync as proof
of concurrent safety. Library cancellation tokens are only checked at diagnostic
entry; hard deadline and memory enforcement remain an external ownership duty.
Request, analysis, cache, process, handles and leases must share a bounded lifecycle.
Error return and retirement must fit the defined total budget; retries cannot
restart it. Configuration changes or unbounded VFS tombstones require a fresh
snapshot rather than silently reusing incompatible state.

Old failures remain evidence, including the unclassified stage11 AV, separately
proved allocation abort, Go EOF and the latest full-load timeout/EOF. Removing a
path does not identify its historical root cause. The latest old-chain red tests
proved two defects: observer-read expiry can forge caller cancellation, and an
expired request pipe can discard its typed error and surface EOF. Their same-build
EXE/PDB, source and raw output are preserved in
`.tmp/rust-luals-request-boundary-red-sealed-01/` and receipt
`ac3d1ad2-91d6-4404-a8ed-b7fd87494425` (3pass/2fail, actual Cargo101).
These behavioral risks become engine-neutral fault tests in the new design;
the old pipe implementation is not a required module to keep repairing.

Next gates are exact reference projection, Lua51/BOM and original-position
adaptation, TOC/XML/Mixin and API coverage, the complete business golden matrix,
complete Retail repetition and resource measurements, cancellation/timeout/crash
followed by normal queries, cache invalidation and close/reopen/prune. Only after
those establish an acceptable engine/interface should public commands, session
management, packaging and Skill migrate. Fixed luac stdin, offline dependency
closure, all licenses/resources, two independent source rebuilds and the20-sample
performance matrix remain required. The current package still has old runtime
dependencies; final removal is a migration task, not a completed claim.

The complete finite-object vertical slice was then exercised with an independent
101-assertion source contract. `.tmp/rust-emmy-context-object-red-01/report.json`
records 42 passing and 59 failing business assertions. Build receipt is
`25ee54dd-4f4d-4fe3-a0a5-0d9908c0293b`, source tree
`2e13619b9b96ee280d467c2291d6b919f474c3d14b04dabfeef34897dff9dda9`,
EXE `37188f1fc10a3796a6d5dab8e07d2edcfa2d6230f0f5eb8673804182fdcc40ef`
and matching PDB `bc17690744712cc378dc4ea53ffe0e0108a0aae03e242c71a22fd6a75c8ea45c`.
The pinned five inputs, compiled private patch, exact expected assertions,
Lua5.1.5 syntax checks and native Job ownership all verified. This is a semantic
failure rather than a transport or artifact failure; the first report and
all matching source/symbol bytes remain preserved.

The failing slice exposes three separate gaps: frozen graph facts do not retain
object shapes at each read; native member APIs merge incompatible static
contributions or omit copied members; and public structural compatibility plus
variable-argument checks do not diagnose the two source-proved bad arguments.
Dynamic mutation is also incorrectly reported as checked. Direct field reads
already succeeding do not prove whole-object checking or member enumeration.
The first repair stage therefore records immutable bounded object observations,
keeping alias identity and shallow-copy provenance. Native member projection
and common structural checking are subsequent stages, each retested against the
same 101 expectations. Native rule identity advances to
`wow-native-binding-context-v2-object-observations`; historical v1 contracts
remain unchanged, and versioned contracts change only that rule identity.
No Source default migration or final engine selection is authorized by this
partial slice.

The first immutable-observation implementation then passed 53 of the same 101
expectations, closing the eight missing views and three unchanged epochs.
`.tmp/rust-emmy-context-object-facts-01/independent.json` independently passed
689 source/provenance checks. Native build02 is receipt
`c476e051-911e-46e9-aef0-075f6b034979`, source tree
`17e83f17935163a5266c808615ff29997dae2235cbeffc40239199f06246a147`.
The same source passed native193/independent1365, coverage12/independent44,
primitive86/independent1241 and strict Clippy. Those results cover the fixed
inputs; nil, nested, cyclic and unknown-effect counterexamples still need their
own actual execution. Their new fixture is not evidence until executed.

Native object identity/member projection next reached 87/101 under build03,
receipt `4c511ec2-3796-4e6b-82a8-c1e7969b727f`, source tree
`eb30c196cad3a9af20bd3e1ff6311b253711d9545faeaa35736f8b1cb79fc684`.
All remaining fourteen failures are structural compatibility, expected two
argument diagnostics and related unknown coverage. Raw native output exposed
an additional library gap: the fixed anonymous `value:number` parameter was
projected as plain `Table`, including in the initial RED evidence. Native
signature facts deliberately use a coarse shell; cross-file signature queries
were not performing the rich DocAST recovery already used by local signatures.
The repair unifies source-backed parameter projection for local, cross-file and
public parameter queries. It does not substitute a probe-generated target type.
The same original source bytes and 101 expectations remain authoritative.

That annotation repair actually passed all 101 original object checks and
692 independent shape/source checks, alongside native193/1365 and coverage12/44.
Build04 is receipt `777a06d7-d213-44aa-b975-7c478a39f84f`, source tree
`1507e49c372ee2db3afd484b6e62b48dd374bb0a825644b52bc95ffc4b313acd`.
This is finite object/member/structure acceptance, not complete Source acceptance.
Latest-patch strict Clippy and the primitive baseline still need execution;
their earlier passes cannot automatically apply to this changed patch.

Fresh independent review subsequently found five concrete gaps outside that
fixed scenario. Root preserved and actually ran their exact six input files in
`.tmp/rust-emmy-context-audit-red-01/report.json`: 11 checks, 2 passing and 9
failing, with complete transport, fixed-expectation verification, pinned Lua51
compilation and retired ownership. Audit build01 is receipt
`681ee3d7-5d40-46d4-89c2-f971ff7dfb26`, source tree
`21ac6f75bdf8a78c9de3b194e2bf7cab64a3995f020e745a6645fc6496e6cff5`.
Global rebinding incorrectly contaminates an old alias with Boolean; colon
receiver, function-valued object fields and a call inside `or` leave stale
Number where the fixed source requires String. Union/nullable anonymous records
still accept the bad table and incorrectly reject nil, producing neither of the
two required argument diagnostics. Original failure bytes and matching symbols
remain preserved. These are unclosed correctness defects, not accepted unknowns.
Known straightline calls require proper invocation/capture-cell identities and
bounded abstract advancement; merely marking them permanently unavailable would
not satisfy the complete business contract. Invocation selection must also
resolve multiple facts at one source syntax without choosing first or last.

The record-wrapper/target-union repair was then actually run against the same
eleven expectations: `.tmp/rust-emmy-context-audit-record-fixed-01/report.json`
now has seven passes and the same four invocation failures. Bad records reject,
good records and nil accept, and both original argument ranges diagnose. Build
audit02 is receipt `20e21c19-ed4b-4cf3-b65c-442a69d9a1b6`, private patch
`869aef8b7d33b055fe150660d6cc9f95bdfdb27fcc5e0e71906018df201f85ec`.
Object101/independent692, native193/1365 and coverage12/44 all passed again on
that same sealed pair. Strict Clippy and primitive86 for this newest patch are
still required. The source-backed invocation design is recorded separately in
`rust-source-invocation-design-2026-10-09.md`; implementation is authorized but
has not yet produced accepted results. Version3 contracts retain all original
193/12/101/11 expectations and change only the rule identity. No historical
version1/version2 contract or first failure is overwritten.
