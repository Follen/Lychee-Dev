# Native incoming caller identity, R2 review correction

R2 extends the immutable R1 candidate based on `351fcfdaceb4c064179bad3839b5dc1e01aa2043`. The Go 3.2.2 oracle, incoming-only scope, original source fixtures, v8 identity, field contract, cursor/session changes and unchanged full-scan/eager oracles remain as documented in [R1](source-caller-identity-r1-contract.md). R1 was never integrated or delivered as product v8, so this pre-integration correction does not create another rule version.

## Finding and actual reachability

Independent R1 review correctly found that the pinned parser can retain `FuncStat` and a closure after malformed headers: `function Obj.()`, `function Obj:()`, dynamic `function Obj[key]()`, and call-return-looking `function Obj().Run()` / `function Obj()()`. R1 trusted `get_func_name` unconditionally for function declarations. Missing member keys and recovered parameter syntax cannot prove a caller name. The same recovery path applies to `LocalFuncStat`.

However, this finding is a **latent helper robustness risk, not an observed product output failure for malformed inputs**. The actual production route is `ResearchSession::prepare_observed` → `native::prepare_observed` → `PreparedSnapshot::prepare_inner`. `native/context.rs` rejects every nonempty `parse_errors()` result before syntax indexing. That rejection remains byte-for-byte unchanged. No malformed snapshot is made valid here, and no test bypasses admission to fabricate successful relation output. This reachability correction does not alter the separately authenticated historical false-self caller bug on valid Retail source.

Pinned parser source evidence (read, never executed locally):

- `grammar/lua/stat.rs` SHA256 `b900fd6993d019d33fe64fa15e48355b0b3715991496d480c3f9bf2563cb52a4`, function-name recovery and local-function recovery.
- `grammar/lua/expr.rs` SHA256 `73aef77f97234a537346020caaa2f30f2f9392094e0e02e41a4965b505190aa1`, closure/parameter recovery continues after a missing opening parenthesis.
- `parser_error/mod.rs` SHA256 `23ccedaf4f603a5f788ffe80bc53181be961d1df0eaa63febc47f60ffa8f2c4f`, syntax errors are distinguishable from documentation diagnostics and retain source ranges.

## Minimal production delta

1. `research_static_name` now requires the base `NameExpr` to have an actual name token. Existing static index key/prefix validation remains; no target-derived inference is added.
2. A shared `research_function_binding` predicate handles global/member and local function declarations. It requires the static complete name and a genuine parameter list with both parentheses, and conservatively withholds the name when any parser `SyntaxError` starts within the declaration range (including its end boundary). The error check covers suffixes recovered into parameters or the body, not just the AST name shape.
3. The existing file-local traversal builds a `BTreeSet` of distinct syntax-error positions once, checking cancellation/deadline for each item. It borrows the existing tree, does not duplicate error text, does not assume diagnostic order, and does not retain this set in a session or graph. Per-declaration lookup is bounded by tree lookup rather than rescanning all diagnostics. Currently admitted valid input has an empty set.
4. An unproven declaration leaves the nearest closure's original `None` owner entry. It cannot inherit an outer callable. If recovered trees are ever admitted by a future separately reviewed change, these owners use the existing explicit anonymous placeholder rather than a false known name. This intentionally also withholds the name when a declaration's **body** has a syntax error; it is a conservative known-name capability boundary, not deletion of a call. Healthy declarations outside the affected range and healthy bodies keep their names.

All changes operate on naming metadata. Relation selection, row emission, proof, observations, source maps, logical charges, sorting, page limits, coverage and cancellation result handling are unchanged. Assignment-based ownership retains its R1 contract. No production admission policy is changed.

## Regression and execution boundaries

Three new Rust tests supplement the eight R1 tests, all **NOT_RUN** locally:

- `native::sharing_tests::incoming_function_binding_rejects_parser_recovery_names`: actual pinned `LuaParser` plus the production binding predicate. Eleven malformed cases cover missing dot/colon keys, dynamic keys, call-return suffixes, missing parentheses, local declarations, and body errors. Tests require all three original `Target` call syntax sites to remain identical, healthy adjacent declarations to remain named, malformed/containing declarations to remain unbound, and no borrowing of an outer owner. Separate assertions test name/header shape rejection without relying on diagnostics. These are parser/helper assertions, not product relation output claims.
- `native::sharing_tests::incoming_function_binding_accepts_complete_headers_and_honors_control`: valid simple/member/method/local/vararg/comment-separated headers and cancellation.
- `native_research::projection_tests::incoming_malformed_function_documents_keep_real_session_admission_rejection`: real `ResearchSession::prepare` must retain the original parse-error rejection for all eleven malformed cases. No synthetic snapshot, fake Core harness, or parser stub is used.

Existing R1 real native projection tests retain exact old-oracle inverse-label comparisons, logical-charge comparisons, valid Retail caller/recursion/alias/anonymous/XML coverage and real cursor/session tests. None ran locally. The two existing pure Node checker suites are rerun against R2 with all inputs and output sealed; their result proves only checker behavior. `rustfmt --emit stdout` is used only for syntax parsing, not type checking or compilation. No Rust test, Cargo, CI, game, studied Lua source, or repository configuration is executed.

R1 files and evidence remain immutable. R2 records its own complete candidate patch, R1→R2 delta, file hashes, source evidence, checker output and validation receipts. The coordinator still owns integration and the separate `relations::incoming_caller_cursor_tests` workflow filter.
