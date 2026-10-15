# Native incoming caller identity, R1

Base: `351fcfdaceb4c064179bad3839b5dc1e01aa2043`. Behavior oracle remains Go 3.2.2 `0bec60e4406f5d5dc155ee25cef44f9a710e37e8`, with approved native/XML enhancements. This slice neither executes studied Lua nor changes outgoing relations, XML observation/copy rows, source-map inventory, semantic resolution, limits, or coverage.

## Public row contract

Only an ordinary native syntax row already classified `kind=call` and selected as incoming rather than outgoing receives these changes:

- `source` is the nearest enclosing callable's proven syntax binding name, or an explicit placeholder. It is never derived from `target`, the callee token, a graph value, an outer named function, line proximity, or a separately extracted structural edge.
- `referenceExpression` retains exactly the previous `source` expression. Alias spelling therefore remains independent of both caller and selected target.
- `caller.kind` is `named`, `anonymous`, `file`, `xml_handler`, `xml_binding`, or `unknown`. Only `named` has `caller.name`. Placeholder sources are `<anonymous>`, `<file>`, `<xml-handler:TAG>`, `<xml-binding:TAG>`, and `<unknown>`; they are not real symbol identities.
- `caller.position` uses the existing authenticated original-source projection. A named caller points to its exact declaration/binding expression; anonymous callers point to the closure start; file scope points to the admitted file's start. XML handler/binding roots point to the original script element. An unknown owner includes an explicit reason instead of inventing a position.
- `caller.syntaxClosure`, where present, identifies the full enclosing closure in the admitted syntax input. For XML this is explicitly a generated Lua site; `caller.position` is the original XML location. It is not a substitute for the original row `site`/`position`.

A named callable means syntax ownership, not proof that a runtime dispatch happened or that a selected value resolves. Existing `state`, `lexical`, `observations`, proof, load scope, and analysis fields retain their separate meaning. Real recursion still has source equal to target. Noncall `reference`/`write`/`capture` rows retain their previous expression-based source in this narrow slice. Fixed Go's LuaLS incoming `kind=reference` instead uses a file path; the two contracts are not falsely described as universally caller-based.

## Bounded implementation

The existing first file-local Emmy AST traversal records closure-to-binding sites; the existing second traversal retains its already-computed nearest closure on each reference. The map lives only with that queried file's `SourceFileSyntax`, not for the retained session. No graph/AST clone or second whole-corpus index is introduced.

Named function statements and exact one-to-one assignment slots qualify. Parenthesized closure values are recognized. Local/global/static-field assignment names qualify; dynamic keys, call-return owners and table-literal fields do not invent names. A nested anonymous closure always stops ownership lookup, even inside a named function. Each statement's assignment list is visited once, avoiding per-closure sibling rescans. New loops retain the original cancellation/deadline checks; logical row charges and budget ceilings are unchanged.

The existing `xml_inline::Script` body boundaries authenticate synthetic handler/binding wrappers. A wrapper must span both sides of the decoded body. Nested real closures cannot match those boundaries. Original entity/Unicode/BOM/CRLF maps and all mapping manifests are unchanged. Caller locations deliberately do not copy the full closure's potentially large segmented map into each call row.

## Ordering, cursor, and session identity

Research rules advance from `source-native-research-v7-xml-script-ancestry` to `source-native-research-v8-incoming-callers`. Corrected source labels can change order, so old pages are not continued under the old key.

Actual consumers inspected:

1. `relations::read` supplies `RESEARCH_RULES` to `relation_cursor`; the native prepared path uses the same constant in `validate_prepared_cursor`. The key includes runtime rules, direction, limit, selected symbol, source, and environment. `batch.rs` calls the real prepared validator.
2. `context::cursor_key` already includes native `RESEARCH_RULES`. Its nested relation cursor uses the relation validator; both ordinary and prepared/batch context paths inherit the new rule.
3. `research_material::identity` already includes research rules, implementation digest, index, environment, and other engine/map rules; `load_or_build` hashes that identity and validates it on reuse.
4. `ResearchSession::prepare_observed` includes research rules in admitted `source_identity`. `SourceInputs` hashes this evidence. Research plan source identity, graph identity, syntax proofs, and execution digests therefore change consistently where their identity material changes. This is not a promise that historical v7 proof strings remain equal; proof content and attribution remain present.
5. **Previously missing direct rule binding:** `SnapshotRequest::cache_key` only streamed the snapshot. It now streams `(RESEARCH_RULES, snapshot)` into the existing checked SHA256 writer, without materializing a second full JSON input. Actual client `native_session::analyze` and owner `execute_cached_query` both call this method. Old keys miss; unchanged full snapshots remain valid and can reprepare. The original executable-hash handshake and process identity checks remain unchanged. This slice does not call a cache miss an invalid source snapshot.
6. Session IPC acceptance obtains its opaque reuse key from real status; no test/client separately computes the obsolete raw-snapshot hash.

`project_relation_page` contains the original sort-and-slice body byte for byte (SHA256 `4bde3fc9f8de9eb1edc449a3471ca370c74d42a01d1bc7d546565c7f208c81db`). Its only production caller is the old location in relation assembly. Stable path/line/kind/source/target sort still precedes the row limit; independent structural and per-load-scope proofs are not deduplicated. The new Rust regression exercises the actual cursor and page helper for old-v7 rejection, new-v8 continuation, 11 proof rows over six two-row pages, stable equal-key ordering, overflow, and invalid offsets. It is declared, not executed here.

## Evidence boundaries

- Original historical output: candidate `62d53b228e1aeb837424d6cc65d8dc53934aa0dd`, ordinary incoming refs SHA256 `fdadb79dc85ac459b3a172d72d0392bbd30988abbfa3adff685497a992644e8d`. Its 11 rows / 4 sites include six native false caller-like self-edges across the three real callers and two scopes. That failure remains unchanged historical evidence.
- Two complete original Retail files in `tests/rust-source-callers/` are extracted from authenticated sealed Git blob output at source commit `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b`; fixture metadata records SHA256, Git SHA1, size, and path. They are data-only `include_str!` inputs, never executed.
- The old full-scan oracle file is byte-identical (SHA256 `c2c65c124f4fe955ce2bf7b55694ab4d1625215521248d1e8c45f51ec3f70ca8`); the old eager/test-reference tail is byte-identical (SHA256 `2e6e3242f157a26a103b7f7318749882fdc30c3f6537300bd6ab874e83178cfd`). Existing comparisons apply only the explicit inverse of added incoming labels, with no sorting or other normalization, then compare full JSON and logical charges. These native projections are in traversal order; public sorting is separately tested.
- Actual local execution: the two existing Node checker suites passed **65/65**, zero skipped. They validate checkers and synthetic file fixtures, not the CLI or Rust behavior. All 2,618 stage inputs were unchanged before/after that run. A first wrong-working-directory preparation attempt failed before Node launched; its failed preflight receipt is retained separately.
- The generic whitespace check reports nine original trailing-whitespace lines in the authenticated Keybindings fixture. Those exact source bytes are deliberately preserved; owned changes have no whitespace errors.
- `rustfmt --emit stdout` parsed all nine changed Rust files successfully without editing them. This is not compilation, type checking, or a passing Rust test.
- Eight new Rust tests plus affected existing differential/retention/limit tests remain **NOT_RUN**. No Cargo, CI, game operation, release, branch push, or studied-source execution occurred in this worker.
- Existing Windows groups cover `native_research::`, `native::`, and `native_session::`. The coordinator must add the distinct `relations::incoming_caller_cursor_tests` filter to its current workflow edits; this worker does not overwrite the coordinator's workflow.
- New-candidate actual whole-Retail refs/context/session under the original 90-second budget and independent output review are still required. No memory/speed improvement or complete semantic coverage is claimed.
