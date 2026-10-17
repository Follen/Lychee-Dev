# Complete generated Lua Facts and bounded scan regression

Status: implementation and regression inputs prepared; Rust execution, new Go
observations, full-corpus comparison and performance are **NOT_RUN** for this
batch. Node identity tests and rustfmt parsing are static checks only.

## Complete algorithm and unchanged contract

Production files are `src/lua_tree/generated.rs`, `src/lua_tree.rs`,
`src/facts/worker.rs`, `src/facts.rs` and `src/index/facts_pipeline.rs` in
`crates/lycheedev-source`. Generated Lua now uses the same optional worker guard
as ordinary Lua/XML/TOC, including the complete generated documentation pass.
An abandoned worker publishes no partial Facts; the ordered coordinator keeps
the complete original fallback. The original input/owned/scratch/in-flight
limits remain 64 KiB / 4 MiB / 32 MiB / 256 MiB, with a 2 MiB worker stack.

Fixed-key views borrow nodes and retain the first matching field even when its
value is invalid or nonliteral. They do not use XML's last-value-wins rule.
Each descriptor/parameter table is scanned once for its requested fields;
the ordinary AST pass still precedes every API observation. The ordinary
root-Type probe remains separate. This removes repeated direct-child walks,
key decoding and child vectors, not repeated whole-file parses. Old descriptor
lookups number ten for System, twelve for callable, eleven/twelve for event
(with repeated Payload for an alias); each parameter formerly has three.
The actual AST denominator for the real corpus has not been measured.

An explicit DFS continuation stack preserves the seven section order, root
source order and repeated nested-System roots. Parenthesized field values are
unwrapped; collection entries retain the original Rust behavior. Only Systems
own namespaces; children borrow live ancestors by stack index. Signatures
append directly into one buffer, preserving all punctuation and dynamic,
Nilable, return and event-alias behavior. No input Lua is executed.

### Requested temporary capacity bound

Let N be the complete source byte length. The descriptor-local reservation is
64N + 4096 bytes, taken before decoding and released before descending into a
child. This is a conservative reservation within the existing scratch limit,
not an allocation of 64N or an increased allowance.

- Literal decoding requests exactly its source length and emits no more bytes:
  quote/delimiter removal, newline folding and escapes only reduce or preserve
  length. UTF-8 replacement requests 3 times the decoded length; each invalid
  byte becomes one three-byte U+FFFD. Both buffers can coexist, at most 4N.
- Display, Type and LiteralName each request at most 3N. A qualified name requests
  exactly namespace + dot + display. For a nonsystem descriptor, its namespace
  literal belongs to a disjoint ancestor, so this is at most 3N + 1. A parameter's
  Name and Type use disjoint source literals, together at most 3N.
- For a parameter with raw table length R, output including its leading comma
  is at most 11R: the shortest `{}` emits at most 22 bytes; populated literals
  expand by at most 3 times their own bytes and Nilable requires additional
  source bytes. Arguments and Returns are disjoint tables. Consequently one
  signature has length at most 14N + 7, conservatively including qualified
  name and punctuation. The tighter disjoint-span bound is 11N + 7.
- `SignatureBuffer` chooses and records its own capacity request, doubling or
  selecting the exact required length, and uses `reserve_exact` before push.
  This does not assume std's unspecified amortized growth. At a growth boundary,
  old + new requested buffers total at most 3 times the required length:
  at most 42N + 21 using the conservative signature bound.
- During signature growth, the non-signature buckets above are bounded by
  3N Type + 3N display + (3N+1) qualified + 3N LiteralName + 3N parameter strings
  + 4N active decoder = 19N + 1. Some are mutually exclusive; counting all still
  gives 61N + 22. A pre-call name copy is at most 3N + 1, yielding 64N + 23.
  The 4096 constant exceeds these fixed terms. A `link` can own two name copies,
  but runs after its signature has moved into Facts, so it cannot overlap the
  42N signature-growth bucket. A System emits only its name copies.

Persistent namespaces are separately charged by actual capacity before the
local reservation is released. Continuation/root vectors are separately
precharged before growth and keep conservative reservations until document
completion. `Walker::declare` and `Walker::link` separately admit retained
name/signature/scope/category capacities and Facts vector growth. The temporary
reservation covers their pre-call name copies; retained Facts remain in the
original 4 MiB ledger. This describes requested Rust storage, not allocator
excess, tree-sitter's opaque C parser/tree/cursor storage, process RSS or a
proof of performance.

## Linear scanning and cancellation boundary

The independent `ByteScan` change applies to the owned Lua grammar/lexical and
XML grammar/text/character/entity scans. A new 256-byte observation granularity
allows at most 255 admitted byte-work steps between real control observations.
Nested delimiter/name/escape/masking loops share their scanner; delimiter
window comparisons account for all compared bytes. Text expansion copies are
counted and perform an unthrottled check before each possible Vec growth.
Separate scans check on entry and exit; their counters do not combine into a
whole-parser byte or wall-clock bound. Constant lookahead, library parsing,
UTF-8 validation, formatting and allocator time are not a latency guarantee.

AST-node, event and all existing allocation charges remain unthrottled.
Deadline comparisons are unchanged. Every scanner return, including empty,
short and error returns, performs a real check in Drop and sets sticky
abandonment. Drop cannot change a precomputed return value, so publication is
also checked explicitly: Lua grammar -> `SyntaxDocument::new_with_guard`
checks before parsing; lexical -> `facts_with_guard` checks before retention;
XML grammar -> `xml_guarded` checks immediately; `facts::for_worker` checks
again after the complete mode dispatch and before returning any Facts.
The deterministic last-byte hooks exercise this boundary after earlier facts
already exist. No scanner exit can turn cancellation into accepted partial data.

## Two independent comparison axes

The 20 inputs remain unchanged, including `parenthesized_field_and_entry` and
`unsupported_clears_all`. Possible old Rust/Go differences are unknown until
executed; no case is removed, normalized or exempted to make a result pass.

1. `lua_tree::generated_tests` retains the original generated field/documentation/
   signature algorithms from Rust commit
   `5766dccc1fec7d20f5d8120bca7dcd79eed726c4` as test-only reference. Both full
   candidate and guarded worker compare every serialized Facts field, array
   order and private raw-target byte/ordinal against it. Ordinary parsing is
   shared; this reference isolates the generated algorithm, not an independent
   full old binary. The 20-case test emits complete old-reference rows with
   `SOURCE_FACT_RUST_REFERENCE_JSON`, preserved in the test log.
2. `facts::tests::fixed_go_generated_observations` compares complete candidate
   Facts/error rows against a separately sealed fixed-Go report. Any mismatch
   fails honestly. Compare its row with the corresponding old-reference row
   when classifying a historical difference; this batch does not repair it.

The source fixture has SHA256
`f26bddac5eeb449da21f748e9dda2c1acee626223974098298cfc8994458aa36`.
The old 52 and XML 32 inputs and original Go overlay remain byte-identical.
The new cases cover ordinary/API ordering, all sections, three first-invalid
masking forms, literal keys, duplicate semantic keys, nested roots/namespace
reset, case handling, parentheses, signatures, event aliases, unknown sections,
raw literal targets, BOM/CRLF/long literals, malformed/unsupported syntax,
minimal Type-first shape and fallback namespace.

## Exact consolidated local entry

Use the existing owned-process/scratch route described in the
[original fixture instructions](../source-facts-malformed/README.md).
Authenticate the unmodified Go checkout at
`3766f9cacb52e93a803a626187e2a1d54a445468`, official Go1.27.1, go.mod/go.sum and
its existing module cache. Do not run a bare compiler or modify that checkout.
Create one owned overlay entry from the nonexistent absolute Go path
`internal/addoncheck/source_facts_malformed_oracle_test.go` to the absolute Rust
candidate path `tests/rust-parity/source-facts-malformed/oracle_test.go`.
This is the unchanged six-source-hash-authenticating test; it already accepts
arbitrary fixture paths/counts through environment variables.

With cwd at the Rust candidate, resolve and pass to the existing wrapper:

```powershell
$env:LYCHEEDEV_SOURCE_FACTS_CASES = (Resolve-Path 'tests/rust-parity/source-facts-generated/cases.json').Path
$env:LYCHEEDEV_SOURCE_FACTS_REPORT = '<absolute-nonexistent-owned-generated-20-report.json>'
```

Use private GOCACHE/GOTMPDIR, GOTOOLCHAIN=local, GOPROXY=off, GOSUMDB=off and
GOWORK=off. Through `ownedSpawnSync`, with cwd at the authenticated Go checkout
and a bounded timeout, run exactly:

```text
go test -overlay <owned-overlay.json> -count=1 -run ^TestFixedGoPublicFactsMalformedOracle$ -v ./internal/addoncheck
```

Seal the complete report/logs, fixture/overlay/source hashes, process exit and
reaping receipts. Go test success produces observations, not Rust parity.
Set all three independently sealed reports and use the existing Cargo owner:

```powershell
$env:LYCHEEDEV_SOURCE_FACTS_GO_REPORT = '<absolute-old-52-report.json>'
$env:LYCHEEDEV_SOURCE_FACTS_XML_GO_REPORT = '<absolute-xml-32-report.json>'
$env:LYCHEEDEV_SOURCE_FACTS_GENERATED_GO_REPORT = '<absolute-generated-20-report.json>'
node tools/rust-cargo.mjs --owner source-generated-regression -- test -p lycheedev-source --lib --locked --offline -- --nocapture
```

For focused reruns, append one test filter before the final `-- --nocapture`:
`lua_tree::generated_tests::`, `facts::worker_tests::`,
`facts::worker::tests::`, `facts::tests::fixed_go_` or
`index::facts_pipeline::tests::`. The full Source lib batch includes all groups.
Without its report, each Go observation test prints
`SOURCE_FACT_ORACLE_NOT_COMPARED`; that is not a differential pass.

In particular retain these regressions in the consolidated batch:

- `lua_tree::generated_tests::generated_guard_and_linear_views_match_all_20_complete_reference_cases`
- `lua_tree::generated_tests::generated_guard_keeps_original_29_lua_boundary_outputs`
- `lua_tree::generated_tests::generated_fixed_views_scan_each_direct_field_once_and_keep_invalid_first_match`
- `lua_tree::generated_tests::generated_nested_roots_are_repeated_after_ordinary_observations_in_exact_order`
- `lua_tree::generated_tests::generated_deep_documentation_uses_heap_continuations_on_default_stack`
- `lua_tree::generated_tests::generated_large_namespace_amplification_abandons_all_partial_worker_facts`
- `lua_tree::generated_tests::generated_signature_dynamic_and_replacement_expansion_keep_exact_bytes`
- `lua_tree::generated_tests::generated_shared_scratch_and_cancelled_guards_never_publish_partial_facts`
- `lua_tree::generated_tests::lua_linear_grammar_and_lexical_scans_bound_mid_scan_cancellation`
- `lua_tree::generated_tests::lua_scanner_last_byte_cancellation_cannot_escape_as_complete_facts`
- `facts::worker::tests::linear_scan_uses_bounded_real_clock_observations_and_unthrottled_charges`
- `facts::worker::tests::linear_scan_nested_loops_observe_callback_cancellation_within_255_steps`
- `facts::worker::tests::linear_scan_exit_checks_short_empty_and_error_paths`
- `facts::worker_tests::xml_linear_scans_keep_bytes_and_bound_mid_scan_cancellation`
- `facts::worker_tests::xml_long_named_and_numeric_entities_share_the_text_scan_guard`
- `facts::worker_tests::xml_numeric_entity_scan_retains_original_strict_radix_interpretation`
- `facts::worker_tests::xml_scanner_last_byte_cancellation_discards_already_observed_facts`
- `facts::worker_tests::xml_text_expansion_copies_share_the_scan_counter_and_check_before_growth`
- `facts::worker_tests::worker_matches_all_84_fixed_malformed_and_xml_diagnostic_cases`
- `facts::tests::fixed_go_generated_observations`
- `facts::worker_tests::worker_refuses_oversized_non_utf8_and_exhausted_facts_inputs`
- `index::facts_pipeline::tests::generated_lua_worker_preserves_complete_facts_and_order_among_lua_inputs`

The deep test runs on a 2 MiB stack with 1800 nested documentation collections;
ordinary guarded traversal may abandon and the complete fallback must remain
stack-safe. Namespace/signature amplification tests force original budget
fallback without returning partial observations. Full real 4036-document
Facts equality, source-order failures/cancellation, native lifecycle, cold/warm
whole-walltime and memory measurements remain necessary. No per-format timing
or performance benefit is inferred from serial parse totals. Profile schema
and all 31 existing fields are unchanged by this production batch.

Static identity checks only:

```text
node --test tests/rust-parity/source-facts-generated/fixture.test.mjs tests/rust-parity/source-facts-xml-diagnostics/fixture.test.mjs
```
