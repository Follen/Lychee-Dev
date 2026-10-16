# Separate current contract for the two measured object-call receipts

This entry does not change `run.mjs`, its Rust helpers, the historical expected
rows, `allTraceFieldsMatch`, or either Cargo exit. It is a **read-only parallel
report** for the exact ac8 run described in
[PR evidence 6219800482](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6219800482).
It is not a general exception registry or permission to accept future deltas.

## Why these two receipts are necessary

The unchanged original object-carrier fixture has four expression comparisons,
then the two calls in these literal inputs:

```lua
local function f() end
return f()
```

```lua
local function f() end
local first,second=f()
return second
```

The original fixtures admit the actual callee with `typed-call-v1`, provide an
authenticated observed return pack of known Nil and an empty complete object,
and query twice to exercise the cache. The CallExpr sites are file 0, `30..33`
and `42..45`. The historical reference records the native engine's independently
evaluated values and producer rows, not the current owned output.

The current production contract is independently visible in:

- `crates/lycheedev-emmy-adapter/src/session/query.rs`: the validated Bound gate
  calls `permitted_call_inner`; only an actual projected value reaches
  `finish_call_projection`. The observed pack is projected before completion is
  recorded and before any scalar transport. Denied or invalid gates return
  earlier, with no result-completion receipt.
- `.../session/backend/call_quality.rs` and `check_gate.rs`: the completed owned
  value is recursively checked for Unknown/Any, with the existing bounded walk.
  A known Nil/object pack is `native_projected`; Unknown remains unavailable.
  The original permit proof is reused without another provider observation.
- `.../session/evidence.rs`: this public state means completion quality. It
  does not falsely claim execution by a patched upstream checker.
- `crates/lycheedev-source/src/native_semantic.rs`: the production semantic
  response emits the session coverage ledger, preserving operation, state,
  proof, reason and source mapping.
- `crates/lycheedev-source/examples/support/native_context.rs`: the actual
  contract-probe query consumer requires the same call site's `call_result`
  receipt, and reports missing result coverage as unavailable. The independent
  `tests/rust-source-engine/coverage-independent-verify.mjs` already requires
  exactly one matching result receipt and proof.
- `crates/lycheedev-emmy-adapter/tests/object_carrier_contract.rs` already
  asserts every field of the complete five-entry ledger for the declaration
  transport, together with object identity and cached declaration results. This
  change does not duplicate that Rust test or claim to have rerun it.

Therefore dropping these positive receipts would regress the current completion
contract. Adding receipts to a denied gate would also be wrong. The current
checker accepts neither omission nor arbitrary additions.

## Exact comparison and custody

`current-contract.json` binds the measured commit
`ac8bfbe656e81501e64ec572fc2c5c2be909a758`, its original sourceTree
`73ebb0f797b20187dd8c135812e3d64c26a94410e39b3a4828014f31adf7b5b4`, the original
three owner IDs, receipt/report SHA-256 pins, the unchanged clean/historical
Emmy trees and the reviewed source subset. A later checkout may run the checker
only while that reviewed source subset is byte-identical. Its HEAD is never
substituted for the measured commit. Any other measured sourceTree is rejected.
The four root hashes were independently read back from the original machine,
without rerunning or changing the owner, and published in
[custody evidence 6220009285](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6220009285).

The entry authenticates every log sealed by the three original owners, the
original failed report, both exact 36-test inventories, all 76 physical parts
(38 per side), all headers/complete terminals, all 4,137 rows per side and the
original report's per-part bytes/hashes and raw difference list. The existing
five pure parsing/comparison functions are statically mirrored, unchanged, in
`current-contract-traces.mjs`; tests require their function source to be exactly
equal to `run.mjs` and compare actual multipart parsing. This avoids importing
the runner's mutable Cargo/process tool chain. It also checks
the original Cargo command, source identities, reference exit 0 and candidate
exit 101. A forced-cleanup error is retained as a separate execution fact.

Only `object_carrier_native_oracle` /
`actual_unary_tuple_logical_declaration_and_observed_return_transports_keep_nested_carriers`
at zero-based ordinals 18 and 22 may differ, and each must equal a complete
literal expected old/new Debug value. Both old ledgers contain, in order:

1. `call_return / known_absent / nil-slot`
2. `object_shape / bound / shape:object-1`
3. `call_return / bound / selected:object-1`
4. `call / bound / permit`

Each new ledger must preserve those exact four entries and append exactly
`call_result / native_projected / permit` at that same exact site. For all five
entries, reason, parameter, parameter_index and argument are None. No fields,
strings, whitespace, sites, ordering, provider events, types or other rows are
normalized or ignored. All other 4,135 rows must compare equal.

## Run without Cargo or changed original evidence

From a checkout containing this checker and the identical reviewed source subset:

```powershell
node --test tests/rust-source-engine/fixed-emmy-oracle/current-contract.test.mjs tests/rust-source-engine/fixed-emmy-oracle/run.test.mjs tests/rust-source-engine/fixed-emmy-oracle/controlled-run.test.mjs
node tests/rust-source-engine/fixed-emmy-oracle/current-contract.mjs --evidence-root C:\path\to\original-repo\.tmp\rust-disk\evidence --output C:\path\to\original-repo\.tmp\ac8-current-object-contract-r1
```

The exact existing-machine invocation is:

```powershell
node tests/rust-source-engine/fixed-emmy-oracle/current-contract.mjs --evidence-root "D:\Code\wow\addons\Lychee Dev\.worktrees\rust-refactor\.tmp\rust-disk\evidence" --output "D:\Code\wow\addons\Lychee Dev\.worktrees\rust-refactor\.tmp\ac8-current-object-contract-r1"
```

If running from a different checker checkout, `--contract-root` can name the
preserved ac8 source root. The checker authenticates that root's reviewed
source subset and separately authenticates the original `run.mjs` beside the
checker. The new checker imports only its
Node-only trace helper; it does not run that source or change its identity.

`--output` must be a new directory outside the original evidence root, with an
existing parent directory. It is created exclusively and cannot overwrite an
earlier result. The executable has no Cargo/process/lease/rebuild operation and
does not update the original owner or report. Relocated evidence is supported
only as intact directories for the same three original owner IDs, preserving
every sealed file. No embedded arbitrary absolute path is followed.

The new JSON report preserves `historicalRaw.state = failed` and
`historicalRaw.allTraceFieldsMatch = false`, both original Cargo results and any
forced-cleanup error. It separately reports `currentContract.state`. Exit 0
means only this narrowly defined current-contract check passed. It does not
mean historical equality, successful candidate Cargo, later-HEAD validation,
full Source/Retail parity, a performance result or a completed audit loop.
Missing, corrupt, incomplete, changed, foreign or unauthenticated evidence fails
closed. Node unit fixtures are explicitly synthetic; their pass is not a Rust
runtime or real-artifact verification result.

The complete executable is additionally tested against a private copied
manifest and explicitly synthetic three-owner/36-trace/38-part-per-side chain.
Those tests cover the narrow pass, later missing-child failure, malformed
reported trace metadata and extra physical parts, retaining the authenticated
historical failed verdict in each later failure. They add no production CLI
pin override and never modify or stand in for the real ac8 artifacts.
