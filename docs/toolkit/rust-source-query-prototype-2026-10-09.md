# Source query facade: fixed prototype evidence

Status: optional direct-library prototype, verified on the fixed cases below.
The engine is not finally selected and default Source commands are not migrated.
This is not complete G01–G16, flow, full Retail or public CLI acceptance.

`PreparedSnapshot::source_queries` reads the same Emmy AST/FileFacts as the
native analysis. Its syntax-only queries remain available for code whose
abstract evaluator does not yet support loops. `with_execution` attaches an
immutable graph with an explicit load, source-template or invocation selector.
Multiple occurrences require a selector; conditional observations retain
uncertainty. No raw global/member resolver supplies a missing managed value.

Definitions expose two independent facts: a lexical declaration and the
selected value's original source. A storage assignment does not change a copied
function value. References distinguish Read, Write, Capture and Call. Captures
identify a mutable cell without claiming that creation read its current value.
Typed storage/capture/use metadata is emitted by the existing evaluator events,
not reconstructed from opaque cell strings. Calls distinguish syntax, symbolic
templates and explicit-input abstract observations; none claims game execution.

Reference/call pages preserve input-file order, original ranges, occurrence,
role and fact identity. In-process cursors bind owner, target, filter, selector,
options, identity and rules. Context walks the same declarations, values,
writes, copies, calls and captures with bounded depth/nodes/bytes. Full origin
ranges, displayed ranges, exact UTF-8 snippets and truncation remain separate.
Load-occurrence metadata uses an explicit zero-width file anchor and no source
execution claim.

## A real upstream defect and its repair

The original G01 lexical-negative input contains a parameter named `value`,
then a loop variable with the same name. After the loop, `return value` must
refer to the parameter `[31,36)`. The first actual query run incorrectly chose
the loop variable `[74,79)`: 69 checks passed and one failed. Native raw
`resolve_name` returned the same wrong declaration. The diagnostic artifact
records ForStat scope `[70,104)`, child Block `[86,101)` and return `[113,118)`.

The pinned upstream's `visibility_end` omitted ForStat/ForRangeStat, extending
loop declarations to the enclosing function block. Its `visible_from` also
made iteration variables visible inside their own initializer expressions.
Numeric-for declaration ownership incorrectly retained the previous statement.
The private `semantic_db/facts.rs` repair uses the actual for/body scopes for
both boundaries and records the correct owning statement. All dependent
lexical lookup/reference/completion facts share that helper; no query facade
name or range workaround was added.

Original failure: `.tmp/rust-emmy-source-query-red-01/report.json`.
Raw scope proof: `.tmp/rust-emmy-source-query-diagnostic-01/report.json`.
Repair proof: `.tmp/rust-emmy-source-query-scope-fixed-02/report.json`.
Fourteen additional fixed numeric/generic header/body/post-loop, nested-loop,
recursive-local-function and upvalue checks pass. Initial diagnostic output
compilation and test-isolation failures remain preserved separately.

## Actual current artifact identities

All four artifacts and strict checks below bind Source tree
`5fb8755358596c26452d863b3d7dcffb6e8bbde7591e950ce1ae6c8d5f2469f4`
and private Emmy patch
`9cb0e13982358106b1e9aa7b7950014d963aa2ec26a6db3c943d7d6c20d8074b`.
The pristine pinned 848-file upstream remains unchanged; the private experiment
has 849 files and 19 changed paths. Sealed builds preserve full inputs, original
and modified patch bytes, compile environment, EXE and matching PDB.

| Gate | Build/run evidence | Result |
| --- | --- | --- |
| Query definitions, inverse references/calls, capture identity, load selectors, pagination, UTF-8 context, cancellation and loop scopes | `.tmp/rust-emmy-source-query-scope-fixed-build-02/artifact.json`, receipt `e18389d0-e3bc-4d47-934e-f273359c60cb`; `.tmp/rust-emmy-source-query-scope-fixed-02/report.json` | 84 assertions; 496 independent checks; 8 actual Lua 5.1 stdin syntax checks |
| Original native core/object/audit/lifecycle/return-slot cases | `.tmp/rust-emmy-source-queries-native-build-01/artifact.json`, receipt `c96f7b82-8896-46f7-ba56-4928ac3420eb`; `.tmp/rust-emmy-context-queries-*-01` | 363 assertions; 2,524 independent checks |
| Input/map/identity/control cases | `.tmp/rust-emmy-source-queries-input-build-01/artifact.json`, receipt `570e0e46-ee62-4851-8f99-c486868b285e`; `.tmp/rust-emmy-source-queries-input-check-01/report.json` | 36 assertions; 53 total custody/check records |
| Legacy fixture behavior/provenance | `.tmp/rust-emmy-source-queries-adapter-build-01/artifact.json`, receipt `ef0fbb58-8f66-4704-8dd1-b04c34173708`; `.tmp/rust-emmy-source-queries-adapter-check-01` | Original 86 assertions; 1,261 independent checks |
| Strict Clippy on all affected examples | `.tmp/rust-emmy-context-clippy-09/report.json`, receipt `4c3c8c08-aadb-4ddd-b4d0-308c161786fc` | Pass |

Combined fixed assertions are 569; independent graph/source checks are 4,281.
Counts do not establish broader completeness. All owned runs confirm root/tree
reaping and closed handles. Related freeze32/33/34 records differ by added test
contracts/verifiers; their Source tree identities match.

## Remaining work

The facade covers the fixed subset, including original G01 inputs and added
counterexamples. Complete command projections, all business goldens, common-fact
secret/guard flow, bounded execution of loops, segmented XML, full Retail,
cross-request/cache/fault lifecycle, packaging, performance and final independent
engine choice remain required. Low-level raw Emmy access remains documented as
non-authoritative for managed selected values. The prototype's single-operation
control and existing resource ceilings have not been expanded to claim these
gates passed.
