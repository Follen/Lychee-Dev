# Native source invocation facts: design after independent audit

Status: implemented and verified in the isolated, optional native library
experiment. It is not the selected product engine and has not migrated public
commands. The pristine pinned Emmy source remains unchanged. The first
independent audit, `.tmp/rust-emmy-context-audit-red-01/report.json`, retains
its original 2 passing and 9 failing checks; the failures are not overwritten.

The latest tested native build is
`.tmp/rust-emmy-context-call-observation-build-03/artifact.json`
(receipt `56737d46-0897-4d05-a193-cd7ed2fa2c08`, source tree
`28fa78f3ebaa56de6c021235069c78aa0e4e33d3bdc38d10f648594d3109410b`).
Its private semantic-library patch is
`10c55cf4563ea3d2909443898638c61e7d327aed5b9386c9ae996922dbb7930c`.
EXE and matching PDB, all original/modified patch bytes, fixtures and compile
environment are sealed with freeze21. On that exact build, the unchanged
317 core checks, 28 lifecycle checks, 4 isolated callsite checks and 14 return
slot checks all pass. The separate native, coverage, object and lifecycle
provenance verifiers pass 1,365, 44, 719 and 391 checks respectively. These
are fixed source-only cases, not full Source business or real-client acceptance.

Freeze21 includes the test-only type-alias cleanup found by strict Clippy.
Strict Clippy passes under receipt `d3b38338-2b7b-4969-940d-4c0bb00a477d`.
The unchanged legacy experiment passes 86 checks plus 1,241 independent checks
on adapter-build-06 (receipt `06d570c8-741e-4e4a-be13-ece2f1d3a116`), with the
same source tree and private patch as the native build. Earlier build-02,
compile-failure, lifecycle/callsite failures and initial metadata mismatch are
retained separately. No historical passing result is silently attached to a
later source identity.

## Observed gaps

The original example facts builder was
`crates/lycheedev-source/examples/support/wow_binding.rs` with
`native_profile=true`. Its straight-line parameter-field summary could not
represent the following supported source behaviors:

| Case | Required fact | Missing mechanism |
| --- | --- | --- |
| Global rebinding inside a known closure | A later read selects the newly assigned value; saved aliases retain the old object | Execute ordinary body assignments against shared global cells |
| Colon call to an anonymous function | Evaluate the receiver once and bind it to positional parameter 0 | Normalize call arguments independently of declaration syntax |
| Known function stored in an object field | Follow the selected closure and update the captured local | Capture mutable storage cells and transitively invoke known closures |
| Call inside a larger expression | Preserve its effects before computing the enclosing expression | Evaluate expression children in language order even if the result is unavailable |

The former `Context.parameters` override was also insufficient for parameter
reassignment: writes to a cell cannot supersede a separate parameter lookup map.
The local key `local:<declaration>:load:<occurrence>` conflates separate calls to
the same closure. Both mechanisms must change before extending execution.

## Identity and state

Keep the identities below separate and include their relevant roots in the
snapshot/provider identity. A source location is never an invocation identity.

| Identity | Meaning |
| --- | --- |
| Function definition | File plus closure AST identity; owner of the native signature |
| Closure instance | Definition plus creation occurrence plus captured cell bindings |
| Invocation/frame | One evaluated call, selected closure instance and normalized arguments |
| Cell | A mutable storage location; global cell or frame plus lexical declaration |
| Value | Primitive, object identity, closure instance, nil, or reasoned unavailable value |
| Object observation | Existing immutable heap observation of one object identity |
| Use occurrence | Source site plus load/frame context and expression evaluation occurrence |

Create a fresh frame for each invocation. Allocate parameter and local cells in
that frame. All reads and writes resolve through cells; parameters have no
second read-only value map. Lexical resolution comes from Emmy declarations,
not spelling. A closure stores declaration-to-cell references for its free
locals at creation time, rather than their current values. Escaped cells outlive
the frame while reachable. A captured `x` sees later writes to its cell; a saved
object value continues to refer to the same object despite rebinding `x`.

Global bindings use the snapshot's shared environment cells. A global assignment
changes the selected cell value without mutating the previously selected object.
Object field writes retain the existing object identity/write-version mechanism.
Function definitions continue to reuse native Emmy signatures. A native callable
reference may identify the source definition, while graph invocation provenance
identifies the particular closure instance and captures.

## Bounded abstract evaluation

Replace the native-profile summary dispatch with one bounded evaluator of the
supported AST subset. Do not execute investigated Lua, dynamically load code or
invoke the actual WoW APIs. Retain the current non-native experiment separately.
An evaluator result carries selected value(s), control completion, and explicit
coverage; it is not inferred from an effect-summary pattern match.

1. Evaluate statements in order: local declarations, assignments, calls, return
   and nested do blocks. Handle the existing multiple-assignment evaluation
   rules before committing writes. Entering a lexical block allocates local
   cells in the current frame and exposes only declarations actually in scope.
2. Evaluate the callee once, then arguments. For colon syntax evaluate the
   receiver once as part of the member selection and insert that same value at
   argument index 0. Bind against formal parameter order even when the function
   was assigned as `obj.f = function(self, x) ... end`.
3. Select the exact closure value from the callee fact, create a frame, initialize
   parameter cells and execute its body. Nested calls use the same evaluator.
   A return stops the current body and returns the actual selected value(s).
   Falling off the body returns zero slots; scalar adjustment produces nil.
   Preserve existing explicit handling for
   trusted, unshadowed intrinsic profile operations such as Mixin.
4. Evaluate operands, computed keys, table values and nested calls in Lua order
   before calculating an expression result. An unsupported result operator must
   not erase effects of children that were evaluated. Preserve boolean literal
   truthiness instead of reducing true and false to the same primitive tag.
5. For `and` and `or`, evaluate the left operand first, then evaluate the
   right only when truthiness determines that Lua does so. For unknown truthiness,
   use conditional state/join semantics with visible uncertainty; do not execute
   the right branch as definite. An unsupported control structure cannot be
   treated as an empty body or a successful exact invocation.

All recursive evaluation shares one builder budget. The current step budget must
not reset per call. Add bounded call depth, frame count, closure count, captured
cell edges, return arity and occurrence counts; retain existing object/field/
observation bounds. Recursive calls and invalid state fail with explicit
coverage/budget reasons. No unbounded fixed-point iteration and no retry after
partial execution. If construction fails, do not publish the partially mutated
graph as a complete result. The owner/governor remains responsible for native
process resource bounds; these analysis budgets are complementary.

## Unknown-call effects

A call to a known closure uses the evaluator and its actual body. An unresolved
call uses an explicit conservative effect path. Compute reachable state with a
bounded worklist over value, object, closure and cell identities:

- Object fields lead to referenced objects and closure values.
- Closure captures lead to cells; cells lead to their selected values.
- Cycles terminate through visited identities, and every edge consumes budget.

An object passed by value may have reachable fields changed; its caller variable
cannot be rebound merely because it points to that object. A cell captured by a
passed closure may be rebound, and the values reachable through it may change.
These are separate invalidations. Retain escaped object, closure and cell
identities across calls. A later unknown call with no new arguments must revisit
these roots. A captured cell is revisited using its current selected value,
including an object assigned after the original escape. The actual argument
and selected-callee roots
are explicit. An unknown global environment requires an explicit open-global
coverage effect; it is not proof that every lexical cell in the analysis is
reachable. A function stored in a table must not disappear from the reachable
closure/capture graph. Uncertainty carries its originating call and reason.
An assignment whose receiver identity is unavailable records a source-backed
uncertain effect and makes its invocation's effect coverage unavailable. It
does not recover a former object from a previous write, mutate that object,
or abort unrelated later source facts.

The actual snapshot records shared construction steps (10,000), call depth
(32), frames/closures/call observations (256 each), captures per closure (128),
capture edges (2,048), parameters/return slots (32 each), reachable nodes,
retained escape roots, escape events and uncertain effects (4,096 each), plus
128 checkpoints/restores and 4,096 checkpoint slots. Object and native shape
limits are separately recorded. Construction failure is not a complete graph.

The complete-source research profile has a separate admitted input/AST/work
budget. Its first module-level binding of each source definition per load
occurrence, and its single static template per definition, are source facts;
they do not consume the 256 dynamic closure/frame allowance. Their aggregate
counts may exceed 256. Additional creations of that definition, closures made
inside calls, actual invocation frames and their capture edges still consume
the original invocation limits. The bounded prototype and flow profile retain
their original total counters. This accounting correction addresses the real
Retail load stopping at its 257th declaration; it does not raise the 2 GiB
worker, request deadline, input/AST/work or response budgets.

The same distinction applies to common host facts: one static call template and
the read/write/call-slot projections of existing common facts are bounded by
their admitted source graph and shared work ledger. The 256 host-call allowance
counts actual calls; the 4,096 transfer allowance counts new invocation joins.
The small prototype still counts all calls and transfers against its original
limits. No projected read, write, call slot or capture reference is omitted.

Heap checkpoints share unchanged object versions and retain cell-map deltas.
The 4,096 slot allowance counts newly retained state, including removed-cell
markers; serialization and restoration still expose complete maps and states.
Continuation predicates reduce exact complementary alternatives using
`(P and C) or (P and not C) = P`. Original branch edges and join inputs remain
as evidence, and distinct condition facts are never equated. These changes
do not increase checkpoint, restore or active-path counts.

Research object views use the latest mutation of the object and its reachable
children as their version. Unrelated heap writes reuse the existing immutable
view. Alias writes, checkpoint restoration and reachable cycles participate in
version calculation; field provenance and the original 512-view/4096-field-slot
limits remain intact. A use retains its own execution context even when its
object shape is shared with an earlier use.

## Native query context and static diagnostics

The provider in `examples/support/native_context.rs` keys decisions
by `BindingSite(file, syntax)` and rejects duplicate execution occurrences.
Repeated body evaluation uses an explicit immutable context selector. Retain
the duplicate-occurrence rejection; do not remove it and
silently choose the first or last fact.

Keep distinct provider views:

- A concrete invocation view is keyed by graph/snapshot, load occurrence,
  invocation and use occurrence. A native model bound to this view resolves body
  uses from that invocation only. Caches and object-view keys retain this context.
- A static declaration view supplies symbolic native parameters for function
  body diagnostics. It does not invoke functions or apply writes to the concrete
  after-load heap. Captured runtime values may remain unavailable when no
  execution phase is selected; native annotation checks continue to use native
  types and retain their own coverage.
- A top-level load view selects the exact load occurrence. A source query lacking
  a selector when several occurrences exist returns an explicit collection or
  ambiguity result. It never invents one execution by merging all occurrences.

References, callee selection, parameter checks, inferred values and diagnostics
must consume the same selected occurrence. Foreign-file native models carry the
same provider context. The syntax-to-definition map stays source based, while
use-to-value and call-to-effect projections stay occurrence based. If the native
API requires a smaller initial seam, expose separate immutable provider/model
instances per selected view; do not encode invocation choice in mutable global
state or overwrite one provider map during queries.

For ordinary calls, `CallObservation` identifies the selected source function,
callee value, concrete closure, invocation frame, ordered return source values
and proof. It contains no `LuaType`, diagnostic or compatibility answer. Native
inference checks those identities and projects source descriptors. Explicit
return, overload, cast, function and member annotations retain priority. Empty
and multiple return sequences undergo the Lua scalar/list-tail/parenthesis
adjustments at the actual callsite. Concrete return observations never replace
an explicit native annotation with a fixture-computed type.

## Implementation and verification sequence

1. Introduce frame/cell/closure identities and explicit provider view selection.
   Freeze those contracts before editing the facts builder and native provider.
2. Route known native-profile calls through the bounded evaluator. Preserve
   ordinary source diagnostics through their isolated static view.
3. Add the reachable unknown-effect worklist and conditional expression behavior.
   Check that no path replays a call after recording partial effects.
4. Root builds and runs the frozen independent audit and existing full object
   context acceptance under the existing governor. No change to an expectation
   may be used to make an implementation pass.

Additional acceptance must distinguish two invocations of one closure with
separate local cells, two closures from one factory, shared captured-cell writes,
parameter reassignment, saved aliases across global rebinding, nested known
calls, colon receivers with visible side effects, short-circuit calls, returns
before later statements, closure cycles and bounded recursion. Test exact
invocation and static views independently, including cross-file context/cache
identity. Confirm the true unknown-call path invalidates reachable captured
cells while retaining an unrelated local object. A closed-fixture proof remains
separate from real WoW runtime acceptance.

The evaluated subset now handles local/assignment/function/call/return/do/if
statements, known nested calls, mutable captured cells, receiver normalization,
short-circuit effects and explicit view selection. Loops, vararg formals and
unknown-if return/continue divergence remain rejected. The full 16 business
goldens and 36 flow cases, arbitrary immutable input store, definition/reference
projection through selected bindings, full Retail reuse/fault matrix and final
public migration remain unaccepted. All candidate compilation and execution
belongs to Root's controlled build and owned-run workflow.
