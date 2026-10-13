# Isolated WoW binding/value graph experiment

`emmy_wow_binding_probe golden` is a bounded direct-library example. It is not
used by the product CLI. The owner builds and runs it in the existing native
process budget; this directory does not add a runner or execute Lua.

The original seven immutable embedded Lua fixtures feed six baseline analyses: future
scripts A/B with unknown execution phase, A/B after loading, B/A after loading,
the two mixin copies, a locally shadowed helper, and unsupported accesses. Each
scenario key includes all input byte hashes, ordered load occurrences, the
execution phase, and the explicit fixture profile. It also includes the pinned
engine commit/archive hash and engine package versions, adapter package/rules
versions, compiled dependency-lock hash, source-map profile, workspace root,
and full effective Emmyrc configuration. There is no result cache.
Every embedded file emits its original byte count and SHA-256 for comparison
with the independent owner contract and fixed Lua 5.1 syntax check.

A preserved RED-only batch added four fixtures and four scenarios without changing the
Builder or Facade: an ordinary function mutates a table field, a parallel
assignment changes the variable used by another left-hand address, and a colon
call has unsupported invocation semantics. A fourth fixture puts a mutating
mixin call inside a branch controlled by `local Flag = ...`, an unknown chunk
argument. This local input is independent of the closed initial global
environment; an undeclared global Flag would instead be known nil and would
not establish this counterexample. Four compound assertions preserve
the exact expected selection/type/diagnostic or coverage facts. The first two
require the actual Lua results, not an unavailable result substituted to make
the test pass. The colon case requires unavailable check coverage to agree with
its already-unavailable expression type. The conditional call requires the
later field binding, type, and call-check coverage all to be unavailable.
The resulting run has eleven input
identities and ten scenarios, with 86 assertions total. The owner reproduced
all four failures: 82 assertions passed, 11 actual Lua syntax checks passed,
and all processes were reaped. An independent 989-check audit found the same
four failures. That RED artifact remains the before evidence.

The next focused change freezes every assignment destination before applying
any assignment write. A destination stores its local/global cell or object
identity plus static field key and source site. Field writes include the
captured receiver use in their input provenance; they never re-evaluate that
receiver after a sibling write. Thus `a, a.run = b, F1` writes F1 into the old
table, while `a` becomes b, following
[Lua 5.1 section 2.4.3](https://www.lua.org/manual/5.1/manual.html#2.4.3).
All 86 expectations and all 11 fixture byte sequences remain unchanged.
The owner verified this focused change: 83 of 86 assertions passed, all eleven
Lua checks passed, all processes were reaped, and strict Clippy passed. Its
independent 987-check audit retained only the ordinary mutation, colon
coverage, and conditional-call failures.

The next focused change gives every call a `resultFactId`, assigned after its
result use is recorded; graphs with unfinished call results cannot be frozen.
The call facade reports `declaredReturnType`, `argumentCheckCoverage`,
`invocationCoverage`, `resultType`, `resultCoverage`, and `effectCoverage`.
`resultType` and `resultCoverage` are taken directly from the associated result
fact's `type_view`, also emitted as `resultTypeView` with its fact/proof IDs.
Invocation and effect coverage have four explicit classes: `profile_contract`,
`bounded_proved_pure_subset`, `bounded_parameter_field_writes`, and `unavailable`. Argument checking independently
uses the chosen callee signature and the invocation's supported argument
mapping. Unsupported colon calls retain their declared signature but have
unavailable argument checks, invocation, effects, and expression result.

The legacy `returnType` field is an alias for `declaredReturnType`, not a claim
about the expression result; `checkCoverage` aliases `argumentCheckCoverage`.
The same explanation is emitted in each call projection. This call-boundary
change initially compiled and passed strict Clippy, but the actual owner run
reported eight failures among 86 assertions: six newly exposed regressions and
the two remaining effect failures. All eleven Lua checks passed and all
processes were reaped. The pinned parser's `get_colon_token()` returns the
index token for both dot and colon syntax; testing only its presence incorrectly
marked ordinary dot calls unsupported. Earlier declaration-only `returnType`
output had hidden their unavailable expression results.

The next change uses `call.is_colon_call()` to test the actual token kind.
Nothing else in the invocation or effect logic changes. All 86 expectations,
all eleven input byte sequences, and all resource limits remain unchanged.
The owner verified V5: 84 of 86 assertions passed, all eleven Lua checks
passed, all processes were reaped, and strict Clippy passed. The independent
1,169-check audit verified exact result-fact/type/proof links and the actual
expression results of seven normal calls, retaining only the ordinary mutation
and conditional-call failures. The failed V4 run is retained as evidence.

The next focused change adds a bounded symbolic `FunctionEffectSummary` for
known functions whose entire body is a sequence of single assignments to
`parameter.staticField`. Each target must resolve through Emmy lexical identity
to that exact function's parameter declaration. Right-hand expressions are
restricted to names and primitive literals. The summary contains original body
sites, parameter declarations, fields, and symbolic RHS expressions; it does
not execute Lua. Neither function names nor fixture names select this behavior.

At the call site, actual argument values bind those parameter declarations.
Every written parameter must be a known object without unknown receiver
behavior. The summary reads RHS names through the same value graph at the
call's phase, using the callee's lexical load occurrence for captured locals
and the call-time global state. It adds body receiver/value use facts and field
write versions linked to the selected callee and actual argument facts. The
call exposes `parameterBindings`, `effectWriteVersions`, and the source summary.
A completed straight-line body without return has a proved nil expression
result even when its declaration has no annotated signature. Its invocation
and effects are `bounded_parameter_field_writes`; unavailable parameter type
annotations do not falsely become a checked argument signature.

The summary accepts at most 32 parameters and 32 writes and shares the existing
construction budget without resetting it. Branches, nested calls, local/global
assignments, computed targets, general RHS expressions, and other bodies remain
outside this summary. Their call coverage remains unavailable. Conservative
invalidation of objects/globals affected by unsupported calls is still missing,
so the prototype must not be used as a generally sound whole-program analyzer.
Deferred function-body return inspection is a query at a checkpoint, not proof
of an actual invocation. Such a query retains actual parameter-binding evidence
but reports `deferred_effect_application_not_analyzed` and applies no writes,
even at `after_load`; it cannot contaminate the graph used to check other
functions. The owner verified V6: 85 of 86 assertions passed, all eleven Lua
checks passed, all processes were reaped, and strict Clippy passed. Its
independent 1,195-check audit verified the exact parameter/body/actual-object/
argument/RHS/caller-write chain; only the conditional-call RED remained.

The next focused change collects bounded conditional effect footprints.
`graph.branches` records the branch source and condition sites/facts, possible
call facts, unavailable write versions, per-call affected destinations and
source versions, and explicit coverage gaps. Calls carry their branch ID and
`executionState=conditional_possible`; they do not run intrinsic copies or
ordinary effect summaries. Their invocation, parameter-check and result
coverage remain unavailable. Condition expressions and branch RHS expressions
are traversed for possible calls, including inside unsupported expression
forms; nested closure bodies are not visited as executed code.

A possible `Mixin` invalidates only the known source fields on its actual
target object. A known ordinary summary invalidates only its parameter fields.
An unknown source-field set or possible metatable change makes only that target
object's behavior unavailable. Direct conditional assignments retain
`conditional_write`; conditional function assignments record
`conditional_function_write` without creating a definitely installed function.
Writes link back to the branch, callee, arguments, and available condition
facts. No possible call is recorded as a confirmed copy or applied summary.
Normal objects and fields outside the footprint remain intact.

Calls without a summary conservatively invalidate known argument objects and
record a gap for hidden captures/global effects. Conditional local bindings,
unsupported statements, unresolved targets, and unsupported colon argument
mapping also record gaps. This is a bounded conservative subset, not a general
branch join/fixpoint engine. Deferred body inspection never applies these
branch writes. The existing construction/depth budgets remain shared and are
not reset. All 86 expectations and all eleven fixture byte sequences remain
fixed; owner regression of this final fixture gap is pending.

Each graph exposes `snapshot_inputs` and the exact compact
`snapshot_inputs_json` string. Hash that string's UTF-8 bytes, without adding a
newline, to reproduce `snapshot_key`. `effectiveEmmyrcJson` and
`effectiveEmmyrcSHA256` similarly expose the exact configuration hash input.
Parsing either string must reproduce its companion JSON object. The config
comes from the same instance passed to `analysis.update_config`, with no later
mutation. Phase and load order remain inside the snapshot identity. The
adapter rules version is `wow-binding-value-graph-v7-conditional-footprints`; the source map explicitly
preserves all original UTF-8 bytes without transformation. The owner separately
checks the pinned archive's 848 source files; the version metadata alone does
not establish binary build provenance.

Emmy supplies the Lua AST, lexical local declaration identity, and ownership of
explicit primitive signature annotations. The prototype separates lexical
declarations, runtime cells, immutable values, object identities, field write
versions, script load occurrences, and uses in execution contexts. It records
the selected value when processing a use. A copied function keeps its value
identity when the source field is overwritten or another mixin copy occurs.

After construction, one facade reads the frozen graph for definition, type,
call checks, return checks, and inverse references. These views share fact and
proof IDs. Call-target inverse references are also emitted separately from all
function-value references. Every site uses original UTF-8 byte ranges. Raw
Emmy name-resolution identities are recorded separately and are not consulted
by the facade. Managed unavailable decisions cannot fall through to another
resolver. There is no fallback provider in this experiment.

`Mixin` and `CreateFromMixins` use a deliberately assumed, versioned fixture
contract: static shallow fields copied in argument order. The intrinsic table
contains exact declaration identities harvested from `profile.lua`; local
functions with the same name retain ordinary function behavior. This is not
evidence of the native WoW helper implementation. The initial global
environment is explicitly closed in these fixtures, so a global not yet
written can be known nil. Real WoW ambient globals require a separate contract.

Unknown deferred phases leave global and field reads unavailable. Immutable
captured local values remain identifiable; a local with multiple write
versions is unavailable without an execution phase. Calls use only the
selected function's explicit primitive signature. Proved-pure function bodies
are limited to a single return of names/literals; mutating summaries are limited
to the parameter-field assignments described above. Function-body checking covers
return expressions only; bodies with other statements report unavailable
coverage. Dynamic keys, metatables, conditional writes, unsupported expression
forms, and missing signatures preserve specific unavailable reasons.

The output contains complete bounded graph, copy, call, return, and inverse
reference facts, followed by assertion records and one terminal record.
Internal limits are 16 files, 64 KiB input, 6,000 AST nodes, 10,000 construction
steps, 2,000 values/writes, 5,000 use facts, expression depth 64, and 2 MiB JSONL.
The caller still owns the hard deadline, process reaping, and memory limit.
No executable configuration loader, preprocessing, source execution, native
helper, or file discovery is called.

The owner verified V1 with 74 example assertions, seven actual fixed Lua 5.1
syntax checks, and process reaping. A separate owner audit passed 596 checks of
the emitted facts, including exact call-target partitions. V2 adds the complete
snapshot identity and fixes four strict-Clippy findings without changing any
of the 74 business expectations. Subsequent RED and assignment-fix status is
recorded above. The author does not build or run outside that controlled
workflow. Passing this small vertical experiment
does not complete the original Source business matrix, the 36 flow cases, or
the remaining 16 matrix cases. Full Source acceptance always remains false.
