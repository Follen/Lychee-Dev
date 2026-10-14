> Historical reference for the Rust migration (2026-10-14): current decisions, baseline and acceptance status are in [handover.md](../../handover.md). This document preserves its original scope and evidence; conflicting requirements, protocol designs, performance targets and release instructions are superseded. Historical test results are not current-candidate passes.

# Common Source control facts: fixed prototype evidence

Status: optional direct-library prototype; fixed control cases now pass. The default Source
engine and public commands are not migrated. This prerequisite does not establish
acceptance of the 36 flow cases, complete Retail, or the overall Rust replacement.

The native graph now records statement visits, conditions, control edges, scope
activations, per-iteration cells, exits, return alternatives and explicit stopping
boundaries. Numeric loops and repeat loops advance within fixed bounds. Break
exits the selected loop; returned paths do not continue. Unknown iterators,
unproved while termination, zero-step refusal and exhausted loop budgets retain
prefix evidence. An incomplete graph cannot produce a complete native provider.

Occurrence selectors distinguish repeated visits to the same original syntax.
Function templates and explicit abstract invocations remain separate; none is
evidence that the game executed source. The analyzer never runs investigated Lua.

Limits remain 16 files, 64 KiB input, 6000 AST nodes, 10000 shared operation steps,
2 MiB output and the original 30-second admission deadline. Control additionally
bounds each loop to 64 iterations, total loop visits to 256, active paths to 32,
terminal paths to 128, nodes to 4096 and edges to 8192. Prefix knowledge boundaries
and budget truncation are distinct; cancellation returns exit 7.

## First actual run

Sealed build: `.tmp/rust-emmy-source-control-build-01/artifact.json`.
Receipt: `475b0750-0fac-468b-bec6-3866262a8921`.
Source tree: `38750983d0b8cce1804e64284f6360e4b258c421b34426d5dd21d9182e5d49e6`.
Private patch: `9cb0e13982358106b1e9aa7b7950014d963aa2ec26a6db3c943d7d6c20d8074b`.
Rules: `wow-native-binding-context-v6-control-occurrences`.
EXE: `9c0cfa71ff0abebc4a8561ffcd9e739a88c3b9f7fae861da1c63987b03761141`.
Matching PDB: `dacd95e20b1727c6b669ddec474fd72aba4ccb0fde5a889d9c74ad0948152a1a`.
Freeze35 binds 166 input files; the archived build retains all those bytes and
the complete private patch with original counterparts.

`.tmp/rust-emmy-source-control-red-01/report.json` contains ten scenarios and
2347 external checks, with eight failures. All ten fixed files passed the bundled
Lua 5.1.5 compiler through stdin. The owned Node/analysis/compiler process tree
used a 2 GiB Job limit and 30-second total deadline; all processes and handles
were verified retired. Analysis output was 1,315,423 bytes.

The fixed call counts, numeric arguments, captured closure results, per-iteration
cells, mixed return paths, break exits and cancellation checks passed. Top-level
and function-internal budget/unknown-iterator cases stopped before following
statements and retained incomplete graphs with absent unfinished call results.

Five exact numeric occurrence providers, numeric Load, break Load and return
Load failed to freeze. Code diagnosis identified a missing projection contract:
generated loop values have a genuine variable declaration span, while the old
primitive projection requires an expression syntax identity. The exact numeric
values in the common graph were correct. Repair must retain derived-value proof;
rebinding iteration 2 to the header's literal 1 would invent source evidence.

The initial external contract also expected numeric Load to freeze despite two
or three visits at the same Mark(i) syntax. That expectation is corrected to an
explicit ambiguity rejection; all five exact occurrence selectors must still
freeze. Original source bytes, value/count expectations and first failure records
remain unchanged. The first report did not expose the freeze error message;
the subsequent probe records it for diagnosis.

## Derived-value repair and actual verification

The graph now stores `derived_numbers` with the original declaration and numeric
for statement, header use IDs, iteration, predecessor, occurrence, IEEE754 bits,
rule and proof. The immutable provider transmits this numeric data and source
evidence; private Emmy verifies the actual for AST and derives `FloatConst`.
The five selected argument queries independently returned exact 1, 2, 3, 2, 1
constants through the library. The facade constructs no Lua type.

A Root probe compilation error, matching `LuaType` as `Option<LuaType>`, was
preserved with all inputs and patch bytes in
`.tmp/rust-source-control-compile-failure-01`; receipt
`eeac34b7-1e80-408c-8734-abf821f7975e`. It produced no accepted new executable.

Corrected build03: `.tmp/rust-emmy-source-control-build-03/artifact.json`.
Receipt: `578e4ec2-e66b-4019-ba7f-5151caebd405`.
Source tree: `1d711466c4b66bf9be2291f40b713f2144a6d29aebe7872c8753a4d0c045e9b0`.
Private patch: `e7471e12f5954c2be94121e1d86483c66b10dea13f2acb28e9bd5a05556e4026`.
EXE: `37ae5ebca158c4ac47097f944e4e6ab8b2d0b212432673c92e565c77fd73c04d`.
PDB: `8ac69508adc1150700582457763d0efdcbfde6b260ac5ee8fba879c827a5b010`.

`.tmp/rust-emmy-source-control-fixed-01/report.json` passes all ten scenarios
and 4280 checks, including 4271 source/provenance checks in `independent.json`.
All ten actual luac stdin checks and all owned process/handle retirements pass.
The expanded proof checker still stays within the original resource ceilings.

On the same source tree and private patch, all 363 existing native assertions
pass, including objects, recorded audit counterexamples, lifecycle, return slots
and callsite capture. Independent native1365, coverage44, lifecycle391 and
object724 checks also pass. The first object checker retained the old selector
profile label and failed exactly that metadata check; its original checker and
report are retained. The V6 checker now requires the actual V2 occurrence-selector
profile. No object/source/value expectation changed.

Query facade84 and independent496 checks also pass on this same source tree,
under build receipt `a576a5a2-bf4e-4c1b-b174-0176a570b793`.

## Final source revision for this batch

Strict Clippy first found three library style/interface warnings, then one probe
statement-layout warning. Both failures retain receipts and complete frozen input
bytes in `.tmp/rust-emmy-context-clippy-10` and `-11`. Parameter grouping and
formatting were corrected, with no change to fixed source expectations.

The final batch source is
`c4af47404b6b28a599c839a1c50a416a035290de9d75eb0d386c3cd7f222055a`;
private patch remains `e7471e12f5954c2be94121e1d86483c66b10dea13f2acb28e9bd5a05556e4026`.
Freeze41 seals 175 inputs. The final control EXE is
`fc8ee8320ddc22f4eacdbb96136326be429757fde547f8196d34c6e92d17b2bc`,
with matching PDB `12410f70164eaad70878467848edd9e1efdc26d830ea033a1f5240a36b844cd5`;
receipt `942e5c45-6cb6-47f9-a04c-a7eaf5a02a90`.

`.tmp/rust-emmy-source-control-stage-01/report.json` binds all latest results:
ten control scenarios/4280 checks, the existing 569 assertions and 4281 independent
checks, and strict Clippy. Control checks include 4271 independent source/proof
checks; do not double-count those as additional execution scenarios. All runs
completed under their fixed budgets and retired their owned processes/handles.

Native receipt: `3b3798be-e1f6-49b4-86a8-7086a3b43603`.
Query receipt: `5b1779c3-41ce-4013-ab37-fca88037f229`.
Input receipt: `6d502e09-836b-4732-aae9-bab23c260c87`.
Primitive receipt: `bd7f3be2-114b-4974-846f-fd947b9fdef4`.
Strict Clippy receipt: `c5c08865-ca09-4d3c-aff0-10fa046ba903`.
The stage report contains the corresponding EXE/PDB and report hashes. Earlier
successful revisions remain historical and are not substituted for this revision.

## Remaining work

Trusted API callable identities, return/argument slots, transfer edges, guard
dominance and the complete 36-case flow projection are subsequent required work.
