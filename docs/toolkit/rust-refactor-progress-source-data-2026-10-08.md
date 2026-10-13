# Source / Data Rust migration progress

Baseline: frozen Go v3.1.2. No Go business code is invoked by product Rust.
This is a development ledger; it does not certify complete migration or manual acceptance.

Local performance acceptance (owner update): each equivalent Source/Data workload
must take no more than 50% of the frozen Go wall-clock time, excluding network.
Pin, inputs, cold/warm state, coverage, complete recall and result quality must be
comparable. Added luac/LuaLS checks are measured separately. This gate is not met.

## Inventory and implementation order

Source inventory (`internal/codebase`, `internal/luals`): repository catalog and exact Git
resolution; authenticated immutable blob/tree reads; rebuildable facts/index/cache;
Lua 5.1/XML/TOC syntax and generated API descriptors; precise/exploratory/body/asset
search, stable symbol IDs and cursors; inspect/refs/context and bounded closures;
diff; compatibility/secret flow analysis; addon validation and multi-client matrix;
fixed environments; verified bundled LuaLS check/LSP and optional cross-process broker;
leased worktrees and explicit prune. Current command contract is the public checklist.

Data inventory (`internal/records`): installation identity/config and local/remote build
resolution; content-key-verified CASC local idx/data and CDN TACT indices/ranges;
encoding/root/content variants; BLTE raw/zlib/frame/Salsa20 and unavailable ranges;
query resource admission; pinned DBD/manifest binding; WDC1..WDC5 regular/sparse/copy/
relationship/compressed/partition views; strict typed SQL/joins/subqueries/aggregates/
recursive queries and JSON/CSV/JSONL; domain spell/item/creature/decor relationships;
physical/effective local XFTH versions 1..9, scans, Wago/Raidbots providers; authenticated
listfiles; raw/PNG/WebP export and BLP2 conversion; video AVI/MPEG demux; complete
provenance, evidence, cache authentication and atomic publication.

First implementation batch: independent bounded format cores (DBD, XFTH, BLTE),
immutable Git reader and AST facts. Follow with end-to-end command composition and
oracle differential fixtures; then remaining query/data/semantic functionality.

## Dependencies

Official Rust crate API documentation checked 2026-10-08:
tree-sitter 0.26.3 https://docs.rs/tree-sitter/0.26.3/tree_sitter/ and
tree-sitter-lua 0.5.0 https://docs.rs/tree-sitter-lua/0.5.0/tree_sitter_lua/.
Queries use tree-sitter structural facts, scopes and static aliases. The obsolete
full_moon comparison implementation/dependency was removed after retaining all
four existing behavior tests. Real bundled luac 5.1.5 provides independent syntax
compilation; LuaLS provides semantic check/navigation. Structural parsing does not
execute generated descriptors or establish semantic binding correctness.
Lua compiler `-p` behavior/license checked against https://www.lua.org/manual/5.1/luac.html
and https://www.lua.org/license.html; foundation owns pinned rebuild/distribution.
flate2 1.1.10 https://docs.rs/flate2/1.1.10/flate2/ (bounded zlib decoding;
consumed-input verification required), RustCrypto md-5/sha1
https://docs.rs/md-5/0.10.6/md5/ and https://docs.rs/sha1/0.10.6/sha1/.
License metadata must be retained in the candidate dependency audit; source parser
MIT, RustCrypto and flate2 MIT/Apache-2.0. Ported algorithms retain baseline MIT
attribution. No registry dependency is called a verified release until lockfile and
packaged corresponding source gates pass.

## Verification ledger

- Inventory: read baseline design/regression/status and implementation files.
- Rust Data regressions: latest `cargo test -p lycheedev-data -p lycheedev-source
  --lib --tests --locked`, **44 Data passed**,
  2026-10-08 (9 format unit + 6 CASC + 2 installed metadata/target + 4 range + 5 SQL
  + 15 independently owned WDC + 3 definition cache/revision/manifest). Four explicit
  ignored entries are actual named-client library/dispatch, network transport and
  performance experiment; they do not count as fixture passes. Durable log:
  `.tmp/rust-data-public-cli/module-tests.log` (Source13 passed separately).
  Implemented strict DBD parser/selection; physical XFTH v1..9 reader including v8
  framing disambiguation, negative signed pushes/latest batches, whole-tail validation;
  BLTE N/Z/F/E decode, 16/32-byte Salsa20, checksums/budgets and unavailable-range
  protection. New general local index/object authentication, Encoding CKey/EKey
  pages, Root projections, build configuration identity, lazy framed BLTE ranges
  and single-table typed SQL are library implementations. Data public commands
  now include limited DBCache hotfix, genuine local static `data sql` and raw
  `asset inspect` handlers. Their full profiles are not accepted: joins, aggregate,
  CTE, complete function set, CDN, provider hotfix, key sources, effective/meta
  SQL, JSON query documents/CSV, image export and domain composition remain open.
- Source catalog, immutable Git reader/transport, TOC/XML/generated literal API
  descriptor facts, tree-sitter incremental recovery and scoped alias facts implemented.
  `cargo test -p lycheedev-source --lib`: **13 passed** (one process-test helper),
  2026-10-08. Includes actual isolated Git batch/hash/proof/cold-hot cache/schema
  rebuild, ordered TOC/XML duplicate/cycle/CJK/escape, timeout/output/cancel with
  blocked stdin writer and child reaping. Source command coverage remains partial.
- Independent fixed Blizzard source differential evidence (foundation):
  `.tmp/rust-source-independent/third-batch/report.json`, approved retail commit
  `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b`; original excerpt eight fields and
  C_Timer.After symbol ID/path/excerpt/hash passed. Go query 703ms vs Rust cold
  8597ms / warm 9713ms, payload 2640B vs 3952B: **performance failed**. Follow-up
  persistent authenticated complete index and lazy selected original-byte reads
  implemented and unit verified. Fourth report: inspect Go264/Rust697ms;
  query Go837/Rust cold13596/warm2523ms, still **performance failed**.
- Source's experimental SQLite serialized projection was discarded before acceptance:
  design §8 requires a rebuildable bounded file cache. Source has no direct
  rusqlite dependency. Current `LYSPIDX2` file uses interned string slices, fixed
  records and sorted name postings; malformed ranges/UTF-8/identity are rejected.
  Projection tests retain exact suffix/prefix/indexed/body/assets and cursor output.
- Independent 20 alternating release samples (root),
  `.tmp/rust-supervision/source-release-20/report.json`: retained-hot exact query
  complete legacy rows/count/coverage parity passed; Go median625.686/p95663.879ms,
  Rust126.872/p95151.420ms, ratio0.20277. Only that selected hot query meets the
  50% gate; controlled cold, inspect multi-sample and complete quality/perf matrix
  remain open. Fresh skill forward evaluation made 15 real commands and verified
  two captures, but exposed advertised `source list --snapshot` unsupported.
  The readonly readiness handler is fixed, exercised against actual immutable
  cache and Go legacy fields; proof `.tmp/rust-source-readiness-proof/{go,rust}.json`.
  Fresh precision findings remain open: per-part complete explanation, noisy
  exploratory literal ranking, relation expansion and response bytes. Source
  refs/context/diff/prune/session/matrix remain unimplemented, not registered.
- Selected local release-profile cases, five alternating samples per engine:
  `.tmp/rust-source-file-perf/comparison-fixed/report.json`. Original inspect eight
  fields, exact/prefix/indexed returned legacy row fields, relations, cursors and
  coverage all passed. Real differential testing exposed and fixed non-exact
  excerpt starting one line too late. Median Go/Rust ms: inspect255.57/137.43
  (**50% gate failed**, ratio0.538), exact599.52/111.68, prefix596.89/114.25,
  indexed602.26/114.29 (selected hot cases pass). The first release inspect was
  517ms and is retained. New query projection initial debug format build took
  76.2s, 58,317,474B index; this is not a fair uncached-Go comparison or a pass.
  Complete cold/body/exploratory/refs/context/matrix gates remain open. Rust
  payloads still larger (exact4446 vsGo2640B; indexed6263 vs3043B); structural
  precision additions do not certify output effectiveness without forward tests.
- `cargo clippy -p lycheedev-data --lib -- -D warnings` passed for the new local
  metadata/ranges/SQL batch. `go build ./...` and `go vet ./...` passed including
  the test-only oracle adapter; Rust product never delegates to this Go program.
- Actual readonly local Retail `wow/cn/12.1.0.69933` metadata verified from explicit
  `D:/Game/World of Warcraft/_retail_` flavor/common parent catalog. Build config
  `dcfc90fffd79ba00406ae46f5f657592`, CDN config `f39eaf0d23ad9a0f85fdbfc2713d3ed3`.
  First eager Encoding run failed `container.resource_limit`, exit1 test process:
  unrelated large chunks exceeded64MiB before any bounded page query. This was
  an implementation difference, not environmental unavailability. General BLTE
  range admission now checks complete framing but applies chunk budgets only to
  visited chunks; independent Encoding page MD5 can validate a bounded direct
  range in a large N segment. Tests include untouched large segments, unavailable
  encryption and damaged unvisited page. No raised default chunk budget.
- Explicit real test run passed separately in release profile:
  `cargo test -p lycheedev-data --release --test installed_parity
  real_local_authenticated_encoding_root -- --ignored --nocapture`, with explicit
  client/build/IDs/definition directory/report environment. Fixed WoWDBDefs
  commit83057bdc0cbe13062850ebf8ad530031e128a1cd manifest SHA
  4314266fe055e30207e1c39498a7a31058a1aa2ab6cd674726491b313d135265 and Map.dbd SHA
  972819af8ae37fcdcf77c1e6fa0b349852199d96f5b991651587ff68d57575c0 prepared from
  two exact official resources. Map FDID1349477 came from this manifest.
  `.tmp/rust-data-real-local/map-chain-report.json`: config/EKey/Encoding pages ->
  Root whole CKey -> explicit zhCN/standard -> Map whole CKey -> WDC/DBD/row/SQL.
  Map decoded128845B SHA f4914e00483fec93f634cff52682518eefaf1921226f6e679929551a49a49a40,
  1184 available rows, no unavailable partitions. Encoding whole decoded CKey
  is **not authenticated**; framing EKey and requested pages are authenticated.
  Initial reader materialized complete encoded187MB. The subsequent general
  `casc::LocalReader` implements deadline/cancel-aware physical section reads;
  `blte::Ranges::open_reader` reads the framing directory and requested ranges,
  not the whole encoded file. Actual Encoding14reads/1,440,623B, header SHA
  0bde9dfb5f6fa298e985c408e3067189c8a22b6f198830e56869c7b729d4b289.
  Source/total query budget accounting, index session/verification and complete
  ReaderAt matrix are still open. Region/locale/DBD are never inferred from folder.
- Test-only Go oracle independently reads the same actual client/config/Encoding/
  Root/Map bytes and authenticates complete Root/Map CKeys before typed decoding.
  `.tmp/rust-data-real-local/differential-report.json`: exact metadata, all requested
  Root variants, whole Map SHA, every Map[0] field including arrays and SQL passed.
  Go431.682/Rust478ms single nonalternating sample is **not** 50% gate acceptance.
  Expanded `.tmp/rust-data-real-local/full-differential-first-report.json`:
  six SQL cases/full IDs/order-offset/array scalar/alias/distinct/Unicode passed,
  but complete1184-row public JSON has **109 differing rows** from f32 fields
  widening to f64 before serialization. First example Corpse -230.989 Go versus
  -230.98899841308594 Rust. Foundation corrected source Float32 plus global JSON
  `arbitrary_precision` to preserve the f32 shortest representation through Value
  envelopes; SQL still explicitly widens provider f32 to f64 as Go does.
  `.tmp/rust-data-real-local/full-range-exact-integer-differential-report.json`:
  complete1184 rows/allfields/arrays and all six SQL workloads **passed**, zero
  differences, same metadata/variants/whole Map SHA. Comparator preserves JSON
  integer lexical values above2^53 as BigInt rather than JS Number rounding.
  Original failed reports remain retained. Single Rust402/Go402.831ms remains
  **not** 50% acceptance. Zlib backend candidate `zlib-rs0.6.8` (actual official
  package source/API and Zlib LICENSE verified) selected via flate2's official
  documented feature. New source/license closure is foundation-owned and pending.
  Actual subsequent profile `.tmp/rust-data-real-local/map-profile-chain-report.json`
  is404ms: cumulative identity3.3/Encoding open15.8/directory20.0/Root decoded320.0/
  Root lookup323.6/table decoded350.8/bound352.3/queriesFinished369.9ms. This is
  hotspot evidence, not performance acceptance. Windows CNG MD5 was independently
  measured and was slower than RustCrypto (64MiB84.1vs77.4ms); it was not adopted.
  Explicit framed decoder preallocation and lazy physical EKey directory are
  implemented with bounds; headerless output does not reserve arbitrary allowance.
  Full pagination/provenance/cancel/ReaderAt/20AB/performance matrix remains open.
- Real luac process regression: **1 passed**, 2026-10-08,
  `cargo test -p lycheedev-source --test luac_real -- --ignored --nocapture`,
  `LYCHEEDEV_LUAC_STAGE=.tmp/luac-5.1.5-final-a` (fixed release manifest).
  `.tmp/rust-source-luac-real/real-luac-report.json`: Unicode/space path, valid 5.1,
  goto/bitops/EOF rejection, early error plus 4MiB tail correctly mapped to source
  path/line; no bytecode and staging cleanup. Line-only compiler diagnostics now
  explicitly retain legacy column0 as unknown sentinel (`columnKnown=false`,
  `positionAccuracy=line`). Validate archives raw compiler stderr separately and
  returns concise original-path diagnostics plus audit capture reference.
  `source validate --toc` handler uses
  archived closure + luac + TS + pinned source presence; matrix and complete
  ClientPin semantic catalog remain open. Foundation installed npm CLI syntax smoke
  passed: `.tmp/rust-npm-luac-syntax-smoke/report.json`.
- Real LuaLS process regression: **1 passed**, 2026-10-08,
  `cargo test -p lycheedev-source --test luals_real -- --ignored --nocapture`, with
  `LYCHEEDEV_LUALS_ZIP=.tmp/rust-supervision/luals-3.19.1.zip` (fixed manifest digest).
  Report: `.tmp/rust-source-luals-real/real-luals-report.json`. Real clean/undefined-global
  checks and alias definition/references/hover passed. CLI semantic composition,
  compatibility matrices and optional persistent broker remain open.
- Real-client manual acceptance: not_run, deliberately final phase.
- Full Source/Data parity: incomplete; all unimplemented inventory entries remain open.

## Public local Data adapter batch

- `installed::resolve_target` returns canonical toolkit DataPin product and preserves
  observed catalog product in metadata. Explicit region/language/exact commit are
  required at this lower-level seam. Four approved series come from core baselines;
  reused `wow_cn_beta`/`wow_classic_beta` slots bind only to Forever1.60.1. Selected
  CN client never borrows the international slot. Existing version.txt must agree
  with the exact active catalog build. Common root with multiple matching slots is
  ambiguous. Regional no-borrow/wrong-series/version contradiction fixture passed;
  this is not actual four-client acceptance or network release resolution.
- Definitions use exact official URL resources, original `definition/<URL-SHA256>`
  metadata + authenticated blobs. Duplicate fields/unknown/null IDs/duplicate names
  and FDIDs are rejected; hash collisions remain legal. Corrupt cache is a failure,
  never auto-replacement. `resolve_revision` preserves exact-commit no-I/O behavior,
  default fullref `refs/heads/master`, strict fullrefs, online refresh and offline
  last verified observation (`definition-ref/<ref-SHA256>`). Concurrent resolvers
  retain their own network answer and validate a competing stored observation.
  Three cache/revision/manifest tests passed. Actual HTTPS fixed83057 manifest/Map
  hashes and fresh master/offline observation passed as one explicit ignored run:
  `.tmp/rust-data-public-network/definition-transport-report.json`. Network does
  not enter local performance evidence.
- New HTTP dependency exact `ureq3.1.2`, native-tls only, explicit Windows native
  TLS/platform roots/no redirects/30-second global bound and 4MiB resource read
  bound. Official API https://docs.rs/ureq/3.1.2/ureq/ and packaged sources checked;
  ureq/native-tls MIT OR Apache-2.0, schannel MIT, actual license texts present.
  Symbolic commit body is bounded128B. Public query deadline limits definition
  requests. Cargo lock closure changed; complete new release license/source audit
  remains foundation-owned, no reuse of older105-package acceptance.
- `real_pinned_public_sql_and_asset_dispatch` explicit release run passed (0.95s):
  fixed canonical Retail pin -> actual local authenticated Encoding pages ->
  complete Root CKey -> exact locale/variant selection -> complete Map CKey ->
  actual manifest/WDC/DBD typed SQL, plus FDID raw asset capture. Both captures
  were reread/verified. `.tmp/rust-data-public-cli/public-dispatch-report.json`.
  No fixture shortcut or Go product backend. Ordinary suite leaves this ignored.
- Actual frozen Go and Rust **public CLI** `data sql` and `asset inspect` each
  exit0 on the same pin/local inputs. Original envelopes saved under
  `.tmp/rust-data-public-cli/{go,rust}-{sql,asset}.json`; exact comparator script
  `tests/rust-source-data/compare-public-data.mjs`, report
  `.tmp/rust-data-public-cli/public-differential-report.json`. SQL request/result,
  schema/bundle/layout and all fixed FileReading identity/content fields match;
  raw asset Map128845B SHA/root/config/variant/entry/pin match. Overall report
  **failed parity** because decodedCache/keySource/resources legacy fields are
  missing. Actual module resource limits are additive and explicitly not complete
  whole-query accounting; they do not silently pretend to be old resources.
  Cache/key-source/resource admission/proof still need implementation. Local
  wall-time50% gate is not accepted from these individual commands.
- Current `cargo clippy -p lycheedev-data --lib --locked -- -D warnings` passed;
  `go build ./...` and `go vet ./...` passed. No active build/test handles remain.
  Full decoded materialization currently hard-caps256MiB; bounded range metadata
  can be larger. Go's streaming larger-content coverage is still a migration gap,
  not represented as complete file support. Source remaining refs/context/diff/
  prune/session/matrix and stronger query-quality/full50% matrices remain open.

## Shared control, cache/key and XML syntax follow-up

- Data decoded reuse now hashes the current selected encoded source, rehashes
  stored SHA256 bytes and verifies the complete decoded CKey on every hit.
  Missing blobs are restored from the same authenticated source; changed cache
  associations/corrupt bytes fail. Explicit KEYFILE input is bounded, duplicate
  checked and represented by request identity without publishing private keys.
  Public keys are lazy and bind to the fixed TACTKeys commit/hash, with precise
  offline missing-key state. Three cache/key fixtures passed; root independently
  ran the same three (locked/offline, 0 ignored), log
  `.tmp/rust-supervision/data-cache-keys-independent-04.log`. Encrypted public-key
  availability profiles and every legacy cache corner are not yet accepted.
- One `Invocation.control` now reaches Data SQL/rawasset admission, CASC reads,
  Encoding/Root loops and SQL evaluation. Shared conservative counters reserve
  physical index representations once and charge every actual repeat validation,
  range read, selected decoded allocation, definition/key document, HTTP request
  and SQL work/retained row. Untouched logical Encoding extent is not billed as
  fetched/decoded content. Three resource fixtures passed, including atomic
  multidimensional rejection and cancellation before public handlers open inputs.
  These are caller-token fixtures, not public-console process acceptance.
- Explicit real fixed Retail run with the new ledger passed: **1 passed**, 0.97s,
  `cargo test -p lycheedev-data --release --locked --offline --test cli_local_parity
  real_pinned_public_sql_and_asset_dispatch -- --ignored --nocapture`, isolated
  `.tmp/rust-data-budget/home`. Report
  `.tmp/rust-data-budget/public-dispatch-report.json`: cold SQL plus warm rawasset,
  original complete Root/Map content verification retained. Cold SQL admission
  reported retained321529821B, metadata78695624B, work4138883, network0/0. These
  counters measure conservative admitted allocations/work, not RSS or wall-time.
  **wholeQueryAccountingComplete remains false**: WDC transient allocations,
  installed identity parser allocations, evidence serialization, blocking stdin
  and single uninterruptible codec calls still need shared hooks. Legacy `resources`
  JSON parity and the whole Data50% performance matrix remain open.
- Full affected ordinary module run passed after these changes:
  `cargo test -p lycheedev-data -p lycheedev-source --locked --offline`, log
  `.tmp/rust-data-budget/module-tests.log`: Data50 ordinary passed/4 ignored,
  Source14 unit+6 foundation diff/prune passed/3 ignored. Later goto compatibility
  added one separately passed unit and one explicitly run real-process test;
  they are not silently included in that earlier log. Strict Source/Data lib
  clippy passed before the final Git/control+goto extension; final lint remains
  to rerun for that extension. Go build/vet passed during this batch.
- Source diff/prune modules were implemented and tested by foundation, then
  registered by this owner. Root independently retains their tests/public parity
  evidence. Readiness now has explicit `index::is_ready`; it does not auto-build
  a missing index. Complete orphan/broker-recovery profiles remain open.
- Git transport now uses the same bounded owned-process runner as syntax tools,
  with stdout/stderr independent caps, Job termination and child/writer/reader
  reap details. `build_with_control`, tree/regular_tree/blob/visit_blobs/verify_file
  controlled variants are available and public Source query/inspect/index/validate
  use the caller's control. Hot projection and bounded serde/CPU regions presently
  check before/after; sync and full public-console cancellation acceptance remain
  open. Spawn-to-Job-attach race is still explicitly not a closed-lifecycle proof.
- Bundled luac verification now holds Windows deny-write/delete read handles for
  the verified executable and license through child launch and wait/reap. LuaLS
  full runtime hold coverage remains open. No PATH/default installed compiler is
  used; `-p -` input still produces no bytecode and executes no addon program.
- XML On* handlers now enter actual luac compilation after text/CDATA/entity
  decoding, using a nonexecuted `function(self, ...)` wrapper and original XML
  line mapping. `checkedLua` still counts Lua files; additive `checkedInlineScripts`
  separates handlers. Diagnostics retain original path/line/engine/content hash,
  handler and audit capture; raw compiler stderr stays in the audit capture.
  Real public dispatcher regression **1 passed** (0.83s), report
  `.tmp/rust-source-inline-real/inline-public-report.json`; root independently
  passed it (0.71s), log
  `.tmp/rust-supervision/source-inline-luac-independent-01.log`. Legal varargs,
  CJK/space paths and entities passed; goto/bitops failed on original line4 and
  malformed EOF on line5. TOC cycles/duplicate loads now remain legacy warnings
  and loadOrder starts at0. Independent Go/Rust TOC CLI differential remains open.
- This real test found quick_xml0.38 emits a separate GeneralRef event; ignoring
  it had made legal `<` comparisons fail structural analysis. Fixed the event
  handling, with an explicit derived-facts generator identity bump.
- Lua5.1 `goto` identifiers were verified against the bundled real compiler.
  Original TS extraction failed despite compilation passing; preserved first
  observation `.tmp/rust-source-luac-real/goto-first-report.json`. Structural
  compatibility now adapts only exact identifier tokens with same-width grammar
  input, skips all strings/comments and always extracts names/ranges/hash from
  untouched source bytes. Incremental parsing falls back to full parsing when
  a delimiter edit changes token classification outside the edited extent.
  Actual compiler+TS test **1 passed**, current report
  `.tmp/rust-source-luac-real/goto-current-report.json`; separate trivia/ranges/
  incremental-equivalence unit passed. Generator-v5 prevents reuse of prior
  incompatible derived facts without touching pins/captures. `goto label` remains
  a Lua5.1 compiler error. Full4-client syntax/semantic/query quality matrices
  and all local performance gates beyond the retained-hot exact case remain open.

## Public DB2 profiles and Unicode simple-mapping follow-up

- Data now registers seven real handlers: hotfix, SQL, DB2 record/page, DB2
  schema, search, foreign-key, and raw asset inspect. The new four DB2 entries
  reuse the fixed manifest/DBD plus authenticated local CASC/Encoding/Root/WDC
  chain. They do not implement CDN or incomplete encrypted-table profiles yet.
  Real dispatcher test `real_db2_record_page_schema_search_foreign_key_profiles`
  explicitly ran (1 passed, 3.27s), isolated `.tmp/rust-data-db2/home`; eight
  success cases and one rejection archived in `db2-dispatch-report.json`.
- Reproducible public CLI comparator:
  `node tests/rust-source-data/db2-public-differential.mjs . <report-directory>`.
  First report `.tmp/rust-data-db2/public-first/report.json` **failed**: two
  search warnings missing, string foreign-key error/exit mismatch, missing record
  error/exit mismatch. Preserved this observation; fixed the actual handlers.
  Second report `public-second/report.json` passed all ten cases, then root
  independently passed all ten in
  `.tmp/rust-supervision/data-db2-public-independent-01/report.json` with stable
  Rust SHA256 b8ad9695653dd856b1451218c18453b7db5abbaf712287f7cd457304fa18cc53.
  All Go result fields, exit/errors/warnings and capture provenance were exact.
  Resource/cache/timing observations are recorded separately, not claimed equal;
  new additive fields are excluded from the legacy-field comparison. This is one
  fixed Retail/local complete-table profile, not full Data or performance proof.
- Foundation's WDC `inspect_budgeted` and `open_budgeted` now receive the same
  public query Arc budget. Inspection and opening each charge their own actual
  conservative admission; caller does not repeat parser metadata charges.
  WDC scalar/string/row temporary work is hooked; accounting still explicitly
  false for the full query (identity, evidence, SQL plan/order-container and
  single codec regions remain). Counters describe cumulative admission, not RSS.
- Root independently found SQL `UPPER('ß')` / `LOWER('İ')` differences in
  `.tmp/rust-supervision/sql-simple-case-01`. Fixed SQL and DB2 substring search
  using Unicode17 **simple** one-scalar mappings, without full/contextual case
  expansion. Source data is official UnicodeData17 at fixed SHA256
  2e1efc1dcb59c575eedf5ccae60f95229f706ee6d031835247d843c11d96470c;
  reproducible generator `tests/rust-source-data/generate-unicode-simple.mjs`,
  original Unicode License V3 retained at `crates/lycheedev-data/UNICODE-LICENSE`.
  No new crate/runtime Go dependency. Frozen test-only Go adapter emits the
  independently expected mappings: every 1,112,064 valid Unicode scalar passed
  upper/lower checks. Also passed German/Turkish/Greek/contextual-sigma/ligature
  SQL fixtures; preserves `ᾀ` -> `ᾈ`, not an expansion or unchanged heuristic.
- String literal/parameter/column clones, concat result allocation, scalar
  function argument containers, casing destination, and bounded rational
  temporary representations now admit memory before allocation. SQL case/length
  functions charge input-byte work; casing loops observe cancellation. A new
  test initially expected zero admission but correctly observed the already
  admitted 24-byte argument container; corrected the assertion, then all seven
  SQL fixtures passed including pre-clone/pre-concat budget rejection.
- Fresh release a4651dea0c6931a86c5395f1e01aee7cf2a6da36d22bf5c51ec2b87d6f73f726:
  `sql-simple-public.mjs` same-pin public query matched the Go rows/query exactly,
  report `.tmp/rust-data-db2/sql-simple-fixed/report.json`; DB2 public-third ten
  cases still passed. `.tmp/rust-data-db2/module-tests-after-unicode.log` records
  `cargo test -p lycheedev-data -p lycheedev-source --locked --offline`:
  Data54 ordinary passed/5 ignored; Source15 unit +6 diff/prune passed/4 ignored.
  Strict all-target Source/Data clippy and Go build/vet passed. Ignored real
  luac/LuaLS/client/performance cases are not folded into these counts.
- Foundation's actual Source console CTRL_C proof is retained separately at
  `.tmp/rust-public-console-control/cancel-01/report.json` (owned Git child,
  exit7, child/writer/readers reaped, old artifact6f976…); actual Data SQL public
  timeout proof `.tmp/rust-data-public-deadline/report.json` (exit5,1052.877ms).
  Neither older artifact proof silently certifies every new path/version.
- Lua5.1 real negative checks include both `goto done` and `::done::`; the prior
  identifier false-positive repair never enables goto jump statements. All
  remaining Source refs/context/session/semantic/matrix and Data joins/aggregate/
  CTE/meta/effective/typed-stream/partial/network/domain/image/export profiles
  remain migration work. No whole50% Data gate or full Source matrix accepted.

## Typed DB2 stream provider (public CLI integration pending)

- Root independently verified the original Unicode query239f… against the new
  a465… CLI: both Go/Rust exit0 and `[ß,i]` exact, evidence
  `.tmp/rust-supervision/sql-simple-case-fixed-independent-01/report.json`.
  The Greek/sigma/ligature and full-scalar tests above remain owner evidence,
  not falsely labeled independent root evidence.
- `stream::stream(inv, sink) -> Result<Outcome>` now reuses `db2::prepare`, the
  authenticated fixed local source and one shared WDC/control budget. It decodes
  and yields one typed record at a time; no whole row array is materialized by
  the provider. Before-begin preparation/field errors yield no typed frame.
  After-begin walk/sink/cancellation failures attempt an error marker and yield
  no end; successful bounded scans archive the begin/end manifest. The library
  exports the API, but `commands()` does **not** advertise it until CLI owns the
  actual writer and suppresses envelopes after yielded frames.
- Explicit real SDK test:
  `cargo test -p lycheedev-data --release --test stream_real --locked --offline
  -- --ignored --nocapture`, with `LYCHEEDEV_RUST_DATA_CLI_HOME` set to the
  isolated `.tmp/rust-data-db2/home`: **1 passed**,2.36s. Report
  `.tmp/rust-data-db2/stream-sdk/report.json` covers full1184rows, projection,
  typed filter, limit truncation, caller cancellation (begin/record/error, noend),
  persistent failed sink (only begin accepted), unknown-field refusal beforebegin,
  and capture re-verification. These sink/token cases are SDK fixtures, **not**
  public Windows pipe/console proof.
- `node tests/rust-source-data/stream-sdk-differential.mjs` compared all emitted
  Rust SDK frames to the frozen Go public CLI: four complete exact sequences
  passed, 0 differences — full stream1186frames, truncated5, projected5,
  filtered3; all1184 complete typed rows/arrays/floats and begin/end identities
  included. `.tmp/rust-data-db2/stream-sdk/differential-report.json` records the
  boundary; full public Rust CLI streaming/error/brokenpipe/control integration
  is pending foundation. New stream unit and Data alltarget strict lint passed.

## Public streaming, SQL function binding and runtime hold

- Foundation connected the actual CLI writer; Data now registers eight handlers
  including typed DB2 stream. Public complete1186frames, truncated/projected5,
  filtered3 sequences matched all Go frames exactly. The first error comparator
  accidentally ignored `envelope.error` in generic JSONL refusal frames; its
  apparent pass is **invalid error coverage**, retained in stream-public-first.
  Corrected the reader and preserved stream-public-second actual failure for
  malformed-filter stage (`dispatch` vs `query`). Fixed the provider; third
  report `.tmp/rust-data-db2/stream-public-third/report.json` passed all eight
  public success/refusal cases. Physical pipe/console acceptance remains owned
  by foundation, not inferred from SDK sink/caller-token fixtures.
- Root's real18scalar targeted difference exposed function arity checked after
  NULL propagation and unsupported ABS accepted; initial evidence
  `.tmp/rust-supervision/sql-scalar-differential-01/report.json` has8pass/10fail,
  actual stable dcad3466… artifact (not earlier a465…). Shared bind/eval arity
  checks now precede NULL and bind unreachable/empty-source branches. Removed
  ABS from the supported scalar set to retain Go behavior. Public SQL faults
  explicitly map legacy `query.*` codes/messages/exits and query stage; CLI
  removes only internal stage routing from details, keeping real business data.
- `sql-scalar-public.mjs` exact normalized results/full error objects passed
  original18 +unreachable-arity case (19/19), report
  `.tmp/rust-data-db2/sql-scalar-fixed/report.json`, artifactStable=true;
  corresponding8SQL library fixtures passed. Stable CLI SHA256
  4ae069dd449547744cf91cdb80a785c9eaf69f1f4ea32dbda1821362e8656709.
  Harnesses now accept `LYCHEEDEV_RUST_BIN` for frozen copies, record start/end
  digests, and exit1 on failed comparison. Node syntax checks passed; root will
  use an independent frozen executable instead of testing a changing target.
- LuaLS verification now holds deny-write/delete read handles for every verified
  runtime inventory member through actual check/LSP process termination. Windows
  fixture verified writes/deletes refused while held and tampering refused after
  release; explicit pinned real LuaLS check+alias navigation passed1/5.14s.
  `check_with_control` exposes the caller control and bounded report read. RPC
  analysis/session still uses its separate deadline and has not completed public
  shared-control/session acceptance. LuaLS is not a Lua5.1 compilation substitute.
- Full ordinary affected module run exited0, log
  `.tmp/rust-data-db2/module-tests-after-stream.log`; Data56 ordinary/6 ignored,
  Source16unit+6diff/prune/4ignored. Go build/vet also exited0. These ignored
  counts include actual process/client/performance tests kept separate above;
  no full SQL, network/partial/domain/assets or Source semantic/matrix/performance
  completion is claimed.

### Fixed API environment and semantic validation batch

- Root independently froze release `4ae069dd449547744cf91cdb80a785c9eaf69f1f4ea32dbda1821362e8656709`:
  SQL19 normalized public cases passed with no differences in
  `.tmp/rust-supervision/sql-scalar-fixed-independent-02/report.json`.
  Streaming8 public cases then passed sequentially in
  `.tmp/rust-supervision/data-stream-independent-02/report.json`, including all
  1184 rows/1186 frames, projections, filters, truncation and pre-begin errors.
  Root's first parallel same-home stream failure was a real workspace.busy6;
  its original `data-stream-independent-01` evidence remains preserved.
- Root's new frozen4ae generator-v5/shared-control retained-hot Source20AB
  passed legacy excerpt/result quality and this exact-query performance case:
  Go median632.3534/p95692.6868ms, Rust137.10685/p95144.0611ms, ratio0.21682.
  `.tmp/rust-supervision/source-hot-control-generator5-01/report.json`.
  First Rust query24811.19ms rebuilt a retained home's derived generation;
  this is not a controlled cold benchmark or a full Source50% acceptance.
- New `environment::{build,documents_from_pin,from_pin}` uses the existing
  Lua5.1-aware TS tree and authenticated batched immutable Git reads. It never
  executes source Lua. Full named/array literal metadata, exact API slots,
  defaults/secret flags, duplicate/dynamic gaps, alias/mutation/escape refusal,
  enum/structure/callback declarations and LuaLS-to-source mappings are retained.
  Fixed identity/generator framing and Go JSON hash escaping are explicit.
- Small frozen-Go suite:16 cases match all old facts/coverage/definitions bytes/
  mappings/hashes exactly; case17 separately proves the standard5.1 legitimate
  `goto` identifier improvement while retaining the frozen gopher parser's
  actual rejection. It does not enable goto jump statements or labels.
  First two failed runs are retained in
  `.tmp/rust-source-environment-{first,second}-tests.log`; corrected tests passed
  in `rust-source-environment-third-tests.log`. Frozen Go table.LastLine was
  unset, so old endLine=line is preserved; additional sourceSpans expose the
  full TS extent, original UTF8 byte range/hash and line-only legacy accuracy.
- Actual fixed Retail31c7 corpus:612 documents,9851 facts,5873 mappings,
  1039688 declaration bytes,3449 gaps. All legacy fields/definition bytes/hashes
  match frozen Go Build with the same explicit `environment-ts-v1` identity;
  `.tmp/rust-source-environment-real/differential-report.json`. Input and both
  complete output artifacts are retained alongside it. Catalog complete=false
  is intentional: unknown/dynamic metadata has not been invented or hidden.
- Real pinned LuaLS catalog test first failed because the test incorrectly
  omitted UnitHealth's observed second boolean argument. The first log
  `rust-source-environment-real-test.log` remains unchanged. Corrected actual
  argument and table-type negative passed1/7.40s in `...real-test-02.log`;
  `.tmp/rust-source-environment-real/report.json` retains both diagnoses.
- LuaLS check now returns bounded original report/configuration artifacts for
  evidence. RPC `analyze_with_control` shares the caller control, has a bounded
  stdin writer/ack queue,20ms cancellation observation,64MiB total output,
  bounded headers/frames, and joins writer/readers after owned child termination.
  Authentic runtime read locks remain held through termination. Exact ZIP
  length is checked before bounded allocation. Real prior alias defs/refs/hover
  regression passed1/5.33s (`rust-source-luals-controlled-rpc-real.log`); public
  LSP console cancellation/session acceptance is still not_run.
- Public Source dispatcher now composes `--semantic` with fixed ClientPin API
  definitions, bundled luac5.1 and real LuaLS. Non-Blizzard source requires an
  explicit `--environment` ClientPin. Actual SDK dispatcher real test passed1/
  14.44s, `.tmp/rust-source-semantic-dispatch-real.log` and
  `.tmp/rust-source-semantic-real/report.json`: legal Unicode/space XML passes,
  Lua5.1 compilation passes for a semantic type error, LuaLS reports that error
  at original XML line3 with unknown column=null, and all three engine artifacts
  verify through evidence. LuaLS diagnostics preserve generated UTF16 ranges
  separately; luac syntax success does not imply semantic success. Packaged CLI
  semantic Go/Rust acceptance,4client matrix, definitions/refs mapping of all API
  families and complete session lifecycle are still not_run/incomplete.
- Ordinary affected suites passed in
  `.tmp/rust-source-environment-semantic-module-tests.log`: Data56/6ignored,
  Source16unit+6diff/prune+2environment/6ignored. A subsequent BOM/hash/span case
  is checked separately rather than counted in that earlier log. Strict Source/
  Data all-target clippy passed `rust-source-environment-final-clippy.log`;
  Go build/vet exited0. No new dependency or full business/performance gate
  completion is claimed. Data SQL joins/aggregates/CTEs, overlays/domain/assets,
  partial/network and remaining allocation/evidence budget gaps remain scope.

### 2026-10-08 current SDK semantic and bounded JOIN batch

- Root independently ran the new actual SDK Source semantic dispatcher test:
  1 passed/0 ignored,14.19s, exit0 in
  `.tmp/rust-supervision/source-semantic-sdk-independent-01.log`. This proves
  the exact authenticated LuaLS3.19.1 ZIP + bundled luac5.1.5 + fixed ClientPin
  composition, original XML semantic mapping and captures for this fixture.
  It does not certify packaged CLI/runtime closure or the complete client matrix.
- JOIN frozen-Go fixtures are saved in `tests/rust-source-data/joins-cases.json`
  and `joins-go.json`; the test-only Go adapter never participates in product
  dispatch. All14 cases match legacy columns/rows/errors exactly, including
  duplicate-key order, left NULLs, reverse equality, exact u64 keys, chained
  joins, heterogeneous error timing and empty-source binding. Root independently
  passed the first2 tests in
  `.tmp/rust-supervision/data-joins-fixture-independent-01.log`.
- JOIN no longer materializes intermediate products. A bounded reusable working
  row retains source order and string capacities; optional equality indexing
  falls back before exceeding half the remaining local allocation allowance.
  Necessary-column copies avoid unrelated text work, after full provider-row
  validation, so invalid unused/nonfinite values still fail. New 40,000-pair
  filtered cross test passes with16KiB query allocation budget. Current3 JOIN
  tests +8 retained scalar tests passed in the owner batch; strict Data all-target
  clippy and Go build/vet passed. Whole allocation accounting remains false:
  plan/order containers, installed identity, evidence and codec gaps remain.
- First actual public batch failed1/8: residual self-left Go succeeded while
  Rust copied all unrelated strings and exhausted work. Raw failed envelopes
  remain `.tmp/rust-data-joins-public-01/`; it was not an environment failure.
  Bounded visitor/projection correction passed8/8 in
  `.tmp/rust-data-joins-public-02/report.json`; expanded key-source identity and
  actual6 capture verification passed8/8 in
  `.tmp/rust-data-joins-public-03/report.json`.
- Public proof artifact SHA256:
  `6b0603eb4128941032754414f9826a501fd08a06193579f3ef8020e97db11b92`.
  Map self-inner/self-left/qualified-static and Map/ChrClasses inner/left/cross
  have exact typed rows, complete/partial/query, full schema/layout/DBD and
  retained content + key-source identity parity; ambiguity/duplicate alias have
  exact public errors/exits. Map aliases share one physical source; cross-table
  queries have two authenticated local CASC/WDC sources. Exact ChrClasses DBD
  preparation was separated from the subsequent offline comparison. Mutable
  cache/resource/timing and additive Encoding verification fields are excluded
  explicitly; both original complete envelopes remain available.
- Stable runner:
  `LYCHEEDEV_RUST_BIN=<frozen exe> LYCHEEDEV_JOIN_OFFLINE_ONLY=1 node tests/rust-source-data/joins-public.mjs <task output>`.
  These8 cases and synthetic14 fixtures are not full JOIN/SQL or a performance
  gate. Aggregates/CTEs/subqueries/compound queries, SQL LSP/CSV/overlays,
  partial/CDN/assets/domain commands, Source refs/context/session/matrix and
  full50% workload matrices remain required work.
- Root independently froze that6b060 artifact and passed the same8 sequential
  offline public cases, with artifactStable=true and all6 success captures
  verified: `.tmp/rust-supervision/data-joins-public-independent-01/report.json`.
  The added bounded40k/bad-unused fixture remains owner-tested separately;
  Root's earlier2-test library run does not cover that later third test.

### 2026-10-08 Source matrix composition and authenticated load facts

- `matrix::{read_config,merge,validate_with_resolver,dispatch}` is real Source
  composition, using the same invocation control through fixed target resolution,
  index/TOC closure, bundled luac and optional LuaLS. Config is strict1MiB text,
 32-target count and16MiB combined target-output admission; relative roots are
  cleaned relative to the config path. Mixed single-source flags are refused.
  Targets preserve requested refs and independently resolved commits/order;
  matrices keep shared and target-only API/event/XML/file/diagnostic/unresolved
  columns. Unknown catalog and semantic coverage remain per target rather than
  being collapsed into a success sentinel. Resolver identity mismatches fail.
- Config+merge tests passed2/0ignored, including3 complete frozen Go merge
  objects and strict unknown/nested/missing/duplicate/size/Unicode/control cases:
  `.tmp/rust-source-matrix-parity-01.log`. Root independently passed2/0ignored
  `.tmp/rust-supervision/source-matrix-config-merge-independent-01.log`.
- Actual SDK matrix with two independently validated TOCs and one fixed Retail
  ClientPin passed1/25.84s in
  `.tmp/rust-source-matrix-two-target-real-02.log`; full report is
  `.tmp/rust-source-matrix-real/two-target-report.json`. Both actual luac and
  LuaLS run; the second invocation preserves valid Lua target while XML type
  error makes only that target and the matrix invalid. Original XML line mapping
  and actual matrix capture verify are checked. First attempt's wrong archive
  path external.io5 remains in `...real-01.log`; no product failure was hidden.
  Root independently passed the same SDK fixture1/26.08s in
  `.tmp/rust-supervision/source-matrix-two-target-independent-01.log`.
  This is not a multi-product or packaged CLI acceptance.
- Source sync now uses `git::resolve_source_with_control`; the SDK default
  wrapper remains compatible. Ref validation no longer swallows cancellation,
  and fetch/init/resolve/update share the public invocation deadline/token.
  Strict Source/Data all-target lint passed
  `.tmp/rust-source-data-matrix-joins-clippy-01.log`.
- Test-only `prepare_matrix` first observed four moving refs and pinned their
  actual commits. The first SDK run failed because three new observations did
  not match the documented ClientPin interface baselines; no interface was
  guessed or relabeled. Evidence remains `rust-source-matrix-four-sdk-01.log`
  and `.tmp/rust-source-matrix-four-01/prepared-pins.json`.
- A new isolated baseline home uses four distinct authoritative exact commits:
  Retail31c7f7b9, Classic1028c1e6, Titan825d29d3, Forever4d5d706b.
  Full40-digit pins and preparation captures are in
  `.tmp/rust-source-matrix-four-baseline-01/prepared-pins.json`;
  preparation exited0 in `rust-source-matrix-four-baseline-prepare-01.log`.
  `matrix_four_real` then passed1/0ignored/36.17s with the actual default
  dispatcher and Git exact-ref resolver, four authenticated luac5.1.5 executions
  and four actual LuaLS3.19.1 semantic executions. Actual catalog signatures
  selected the fixture calls; all four partial catalog gaps remain visible.
  Matrix capture verification passed; nested engine captures are present but
  not all independently verified by this fixture. Full report:
  `.tmp/rust-source-matrix-four-baseline-01/four-product-sdk-report.json`, log
  `.tmp/rust-source-matrix-four-sdk-02.log`. This is SDK evidence, not packaged
  CLI, full Go parity, client gameplay acceptance or a performance gate.
  Forever remains source-only. Root independently passed1/0ignored/36.00s in
  `.tmp/rust-supervision/source-four-products-independent-01.log`, compiled
  before the following persistent-session refactor. This evidence is retained
  against its actual SDK version, not reused to certify later session changes.
- `index::load_facts_with_control` supplies authenticated TOC/XML load candidates
  to Source context without treating skipped warm-cache parser facts as absence.
  It authenticates Git raw blobs and checks SHA256/length against the selected
  Index before parsing with existing format readers, under the caller control
  and128MiB/100k-fact bounds. Actual warm Retail test passed1/31.19s:
 4169 facts match original Index content identities; tampered Index SHA is
  refused with source_integrity exit4. Evidence:
  `.tmp/rust-source-load-facts-real-01.log` and
  `.tmp/rust-source-matrix-four-baseline-01/load-facts-report.json`.
  These are parser-observed load paths, not proof of runtime loading. This
  full-scan provider is not a context performance acceptance; compact load
  postings, actual context CLI quality, broker/session and complete Source/Data
  command/profile and50% workload matrices remain required work.

### 2026-10-08 retained LuaLS SDK session (broker not registered)

- `Runtime::open_session_with_control` owns one initialized real worker;
  `WorkspaceSession::analyze_with_control` retains each batch's caller deadline
  and cancellation token. It admits at most256 batches/10minutes and retires on
  every error. Runtime and known Lua/definition/config input files remain read
  locked through the child lifetime; pre/post member checks reject added input.
  Explicit close/drop terminate the job, reap the child, join pipe threads,
  release file handles and remove the private frozen workspace immediately.
- Actual same-PID2-batch definition/references/hover (including generated
  `@environment/api.d.lua`) passed, followed by real SDK cancellation exit7 and
  verified process death. A second initialized worker refuses a known-input
  write, detects an added Lua file before the next batch and retires/reaps.
  Test1passed/0ignored/5.20s, `.tmp/rust-source-retained-session-sdk-02.log`;
  `.tmp/rust-source-session-real/session-report.json` stores results and scope.
  First01 failed because close reaped but retained Frozen cleanup until Drop;
  immediate retirement cleanup fixed that lifecycle gap. Original failure log
  remains `rust-source-retained-session-sdk-01.log`.
- Strict Source/Data all-target clippy passed
  `.tmp/rust-source-data-retained-session-clippy-01.log`. No CLI session command
  is registered by this batch. SDK frozen input bounds remain2048files/32MiB;
  complete Source workspace admission20kfiles/1GiB, continuous input/runtime
  directory monitoring, same-logon named-pipe/peer authentication, aggregate
  custody and public status/close/launch/cancel still require integration and
  real tests. Pre/post checks alone do not prove absence of transient input
  insertion during a request. Windows facilities are assigned to the Windows
  owner; Source owns broker framing, context identity and lifecycle composition.
- Complete-source SDK provider is now actual tested:
  `Runtime::open_source_session_with_control(home,pin,index,definitions,control)`
  batch-authenticates original Git blobs and Index SHA256/length before streaming
  the entire admitted Lua set into a private disk workspace. It holds the exact
  `worktree:<repo>:<commit>` kernel lease until worker reaping and input cleanup;
  capacity is20k Lua files/1GiB logical source, with explicit larger-than1MiB
  exclusions and partial semantic coverage. It does not load every source file
  into one permanent in-memory byte map. Runtime/file locks and inherited
  per-request controls remain the same SDK lifecycle as above.
- Actual fixed Retail31c7 workspace contains2551 Lua files/23,354,176bytes with
  no exclusion. CallbackRegistryMixin RegisterCallback declaration at original
  `Interface/AddOns/Blizzard_SharedXMLBase/CallbackRegistry.lua:112` returns
  one definition, two references and hover text; actual lease acquisition is
  refused6 while live and succeeds after close/reaping. Test1passed/24.09s,
  startup5.043s, `.tmp/rust-source-whole-workspace-session-sdk-02.log` and
  `.tmp/rust-source-whole-session-real/whole-session-report.json`. First01 is
  a preserved test adapter compile error (blob API tuple), not a runtime pass.
  This closes the SDK small-workspace admission gap for this real source but
  not continuous-watch, public broker, full precision or50% performance gates.
- Root independently ran both retained and complete-source SDK fixtures before
  watcher integration:1passed4.25s +1passed23.02s,0ignored, in
  `.tmp/rust-supervision/source-retained-whole-session-independent-01.log`.
  This does not certify the later watcher or broker changes.
- Windows armed event watches now guard the frozen Lua/library and runtime
  directories during retained LSP requests. SDK query loops check the mutation
  token and actual watch completion, not just pre/post file snapshots. A new
  actual fixture inserts then deletes an extra Lua file before another request;
  although the file is already absent, the session rejects it and reaps.
  The extended same-worker/cancel/input/transient suite passed1/4.90s in
  `.tmp/rust-source-retained-session-watch-sdk-01.log`. Root independent watcher
  regression is pending. Config files remain read locked; parent identity checks
  and packaged/continuous full closure acceptance are still distinct gates.
- `session.rs` real same-logon pipe framing/identity/lease/server composition
  compiles; strict bounded frame and deadline tests passed2 in
  `.tmp/rust-source-broker-wire-unit-01.log`. No public command is registered by
  these unit tests. Hidden launch proof, per-request disconnect cancellation,
  physical cross-process reuse/status/close/peer negatives and complete broker
  acceptance are still pending; no stub response is advertised as implemented.

### 2026-10-08 actual installed npm four-source semantic matrix

- Foundation's fresh isolated ignore-scripts offline npm installation contains
  the complete fixed LuaLS archive inventory, authenticated luac5.1.5 and current
  notices. Actual installed CLI SHA8735cf912db4f85663db4ca2c1e0bcc2a08d4ac767d328b61fd43dc6e5704f06
  and release manifest SHA354464af320a507b683bc7722f611da3da17aeb1170567bb91603c5cfb8952d8
  remain identical before/after the public `source validate --matrix --semantic
  --release` run. Four distinct authoritative baseline commits execute four
  actual luac and four actual LuaLS checks, all valid, exit0/28.69s.
- All embedded captures are recursively verified, including outer matrix,
  syntax audits, semantic inputs and three semantic artifacts per target:
 21/21 actual evidence verify true. Partial catalog coverage remains
  Retail3449/Classic2471/Titan2472/Forever3553 gaps, complete=false. Original
  stdout and first report are preserved in
  `.tmp/rust-source-matrix-packaged-public-01/`; `report-with-catalog-coverage.json`
  corrects only the first report's wrong catalog projection path from the same
  original stdout, explicitly without rerunning a command. The runner now reads
  and asserts the actual catalog coverage path.
- Root independently reran installed8735 into a new output directory and passed
  exit0 with stable binary/release, four syntax/semantic targets and21 verified
  captures: `.tmp/rust-supervision/source-matrix-packaged-independent-01/report.json`.
  `tests/rust-source-data/matrix-public.mjs` supports explicit frozen binary,
  release/home/config environment overrides and nonzero exit on failed checks.
  This is packaged CLI/runtime closure and the stated matrix fixture, not full
  Go differential, gameplay acceptance, global Source quality/performance or
  later Source session/watch/broker artifacts. Forever remains source-only.


### 2026-10-08 broker launcher and bounded LuaLS response batch

- Source adds `dispatch_with_services` without changing existing SourceTools or
  direct SDK signatures. Explicit verified BrokerIdentity feeds session status/
  close and the independently owned refs/context modules. No new describe paths
  are registered before the actual public lifecycle test.
- Opt-in analysis launches only when the authenticated scoped endpoint is absent.
  It creates a random 128-bit single-use metadata intent, records actual parent
  PID/creation and the exact verified executable/release identity, launches the
  exact hidden argv with no visible console, and keeps its identity read locks
  through bounded readiness. Hidden CLI composition now consumes the exact intent
  transactionally before serve. Failed startup kills/reaps only the Child this
  invocation actually spawned and removes only its unchanged intent. Five-second
  startup admission inherits the public operation deadline/cancellation.
- Disconnect observation uses the real duplicated named-pipe handle and cancels
  only the request control. Public physical lifecycle, replay, foreign peer and
  actual caller cancellation tests remain pending; compile is not acceptance.
- LuaLS hover now decodes string/marked-string arrays into plain text rather than
  dumping unknown JSON, retains a UTF-8-safe prefix of at most16KiB, and exposes
  truncation/incomplete. Locations reject negative/missing/inverted ranges and
  avoid duplicating the whole response list. Source library20 tests passed and
  strict Source all-targets clippy passed after two owned slice-clone fixes.
- The actual retained LuaLS watcher fixture was rerun after the decoder change:
  1passed/0ignored/5.00s, `.tmp/rust-source-retained-session-watch-sdk-02.log`.
  It proves retained SDK PID reuse, cancellation/input retirement and transient
  insertion/deletion observation; it does not certify public broker lifecycle.
- Root independently passed three Windows pipe fixtures in
  `.tmp/rust-supervision/windows-broker-independent-01.log`. This is the Windows
  transport layer, not Source broker acceptance. New public runner
  `tests/rust-source-data/broker-public.mjs` supports frozen binary/release/home
  overrides, records start/end artifact hashes, verifies captures and exits
  nonzero on failed checks. Its actual packaged run is pending.


### 2026-10-08 first actual broker failure and retained independent proofs

- Actual packaged stage04 broker lifecycle run is a preserved failure, not a
  unit-test substitute: `.tmp/rust-source-broker-public-01/report.json`, exit1,
  binary262c2c88c0ff8c8ed3615f1ca52ba2d653e45fb44862ac0d9b7c752374360a58
  and manifest9a7ddcb004137de0d709ed72326f5f5b301c0a002fb2e0d00c39675e42b258e5
  stable throughout. Absent status and owned cleanup passed, but first semantic
  request exited5/command.deadline after15.33s. Repeated full release/runtime
  verification prevented readiness within the five-second broker startup bound.
- CLI composition now shares its inspected release instead of scanning it again.
  Source `prepare_inspected_release` constructs a non-executing runtime provider;
  actual check/open still hashes every runtime member and retains verified file
  handles before spawn. Existing eager constructor behavior remains available.
  This removes duplicate startup scans without weakening authenticated execution
  or increasing five-second startup admission. A local startup timeout now maps
  to unavailable3 while total public deadline/cancellation retain5/7.
- `prepare_broker` actually creates a new isolated Git/catalog input matching the
  original Go broker fixture: First/Second/Third plus one generated FixtureAPI.
  Exactcommit4a7f7b43e6f5284fb6b188a57e91b17d5f31d8fe,
  PIN-9e101198396228da82d822287f9ada729e75e3669384bac22b8ceb750b04d281,
  `.tmp/rust-source-broker-complete-input-01/prepared.json`. Two Lua files and
  five declarations index complete; one API document/emitted record is complete.
  This provides complete cache-test inputs, not a real-client baseline. Cache-hit
  batch preservation, pruning, cancellation and replay negatives remain pending.
- Root independently passed the current retained watcher SDK fixture1/4.98s,
  `.tmp/rust-supervision/source-retained-watch-independent-01.log`, including
  samePID, cancelled worker reaping, new-input retirement and transient added/
  deleted input. Root also passed armed Windows watcher1 fixture separately in
  `.tmp/rust-supervision/windows-watch-independent-01.log`. Neither is public
  Source broker acceptance.
- Root independently reran the actual fixed luac5.1 goto boundary:1passed,
  `.tmp/rust-supervision/lua51-goto-boundary-independent-02.log`. `goto done` and
  `::done::` remain rejected; `goto` as an ordinary Lua5.1 identifier remains
  accepted with original names/ranges. No goto jump support is introduced.
- Next Data batch now has28 frozen Go aggregate result/error cases in
  `tests/rust-source-data/aggregates-cases.json` and `aggregates-go.json`, generated
  by the test-only relational adapter. Grouping/HAVING/DISTINCT/empty inputs,
  exact wide sums and invalid constructs are included. Rust aggregation is not
  implemented or certified by preparing these inputs; CTE/subquery/domain/assets,
  partial/network and full local50% performance coverage remain required.


### 2026-10-08 second packaged broker failure and actual Data aggregates

- Broker02 is terminal exit1, not an in-flight success. Stable installed stage05
  SHA953ef5209e409a677f0e87353cb126a11fd7d6dd93f16445446f599d4f2f6fbb
  removes duplicate startup scans: absent status1.15s and owned cleanup1.09s.
  Actual first semantic request failed5/codebase.git_failed because the hidden
  child passed a canonical Windows verbatim home into Git, after76.64s. Original
  report remains `.tmp/rust-source-broker-public-02/report.json`; no new Source
  describe paths are certified by this run.
- Git now converts DOS/UNC spelling only at the external argv boundary. Canonical
  identities and containment remain unchanged. Actual canonical-home versus
  ordinary-home tree/object/blob/hash regression1passed in
  `.tmp/rust-source-canonical-git-argv-02.log`; first01 compile error is preserved.
  Root independently passed1/1.00s in
  `.tmp/rust-supervision/source-canonical-git-independent-01.log`. Subsequent
  packaged broker process proof remains pending on a new artifact.
- Rust aggregation is now implemented in the real relational Program pipeline,
  replacing the preceding historical not-implemented status: streaming keys and
  COUNT/MIN/MAX/SUM/AVG state, DISTINCT, GROUP BY/HAVING, empty global aggregate,
  nested/WHERE aggregate rejection, typed numeric equality and exact rational
  sums with finite/bit bounds. JOIN visitor feeds groups without retaining its
  whole product; only group keys/state and final output rows survive. Partial
  source coverage remains partial and unused invalid values remain checked.
-28 frozen Go aggregate cases and budget/cancel/partial/unused-value cases passed
  2 tests. First failed fixture log records a legal JSON number notation
  difference2.0 versus2; the comparison now canonicalizes decimal digits/exponent
  exactly without f64, epsilon or lost wide integers. Original result lexemes
  remain in the fixture. Aggregate2+priorJOIN3+scalar8 all passed in
  `.tmp/rust-data-aggregate-joins-regression-01.log`; strict Data all-targets
  clippy passed. Root independently aggregate2passed in
  `.tmp/rust-supervision/data-aggregate-fixture-independent-01.log`.
- Actual fixed local Retail/CN/zhCN Map and Map×ChrClasses public queries passed
  8/8 exact normalized business results/errors with6 actual CAP verify=true:
  `.tmp/rust-data-aggregates-public-01/report.json`. Frozen release executable
  `.tmp/rust-data-aggregate-binary-01/lycheedev.exe` SHAaa9816990fbfc63a7faa41f73e32c5b297a5ea5e5256c014182a177bc3c7c397,
  same authoritative pin/DBD/layout/whole-content identities retained. Only
  mutable decoded-cache/resource/IO counters and ephemeral captures are excluded
  from differential comparison; original complete envelopes remain beside it.
  This is actual local CASC/WDC SQL aggregation, not full SQL or50% performance.
- Root independently reran frozenaa981 eight public cases:8/8 equal,6 CAP verified,
  stable artifact, terminal0 in6.41s (not a performance sample),
  `.tmp/rust-supervision/data-aggregates-public-independent-01/report.json`.
- After that artifact, per-state numeric admission was tightened before additions
  while COUNT states retain the original512-byte aggregate allowance. A single
  bounded rational scratch allowance serves sequential arithmetic instead of
  reserving4KiB for every COUNT group. New40000-group COUNT case plus28 old cases
  and budget controls passed3 in `.tmp/rust-data-aggregate-state-admission-01.log`.
  This later resource change is owner fixture proof, not included in frozenaa981.
  Whole query/RSS accounting remains incomplete; CTE/subquery/compound/CSV/domain
  and asset/network/partial profiles and cold/warm50% matrix remain required.


### 2026-10-08 CTE scope progress and broker endpoint diagnosis

- Packaged broker03, immutable stage06 native SHA9312c13b5b3d665e8ebfaecd3cfaa3265de023d7af6d75fba17a0ddfb0ded205,
  is terminal failed1 in `.tmp/rust-source-broker-public-03/report.json`.
  Its first actual Retail refs request succeeded0 in35.77s: complete semantic
  whole2551 Lua documents/23.35MB, two queries/one batch and verified CAP.
  The next public status reported closed. This proves an absent endpoint only:
  no original broker OS PID/creation/exit observation was retained by that run.
  It does not certify retained-worker behavior or establish a guessed cause.
- A bounded replaceable lifecycle diagnostic now retains actual broker PID and
  creation identity, last phase and terminal typed error, without token or
  business content. It is never an authorization source. The public fixture
  checks actual OS creation time separately. Source check/clippy passed; actual
  stage08 lifecycle run remains a separate pending proof until terminal.
- Nonrecursive CTEs are now compiled as lexical scopes. All local names are
  reserved before definitions run, so self/forward references cannot fall through
  to physical or outer sources; qualified static table references remain physical.
  Physical row sets are borrowed, computed CTE rows move into the local scope,
  and every definition and outer query shares the original Work/resource budget.
  Partial CTEs remain partial, including unused definitions. Recursive/compound
  UNION/derived-table/expression-subquery profiles remain required and unsupported.
- Fourteen frozen-Go CTE rows/errors passed, plus budget/cancellation/partial,
  exact dependency identities, borrowed900KB unused input and invalid-unused-value
  assertions:2 tests in `.tmp/rust-data-cte-budget-tests.log`. The first differential
  found duplicate CTE names mapped to binding rather than syntax; the original
  failure remains `.tmp/rust-data-cte-first-tests.log`, fixed regression in second.
  A preceding incorrect test-adapter mode produced no Go result and was corrected
  to the explicit relational mode before claiming any differential success.
  Affected aggregate3 and JOIN3 passed, and strict Data all-targets clippy passed.
  Real public CASC CTE cases and performance are not certified by these fixtures.
- The aggregate/JOIN public runners now separately assert Rust's additive
  verification declarations rather than describe them as timing fields.
  Preserved aggregate raw envelopes were read-only checked:all7 physical sources
  declare contentVerified, whole selected payload CKey, whole Root CKey,
  Encoding framing EKey and requested-page MD5 true; whole Encoding decoded CKey
  is explicitly false. Companion `.tmp/rust-data-aggregates-public-01/verification-claims-companion.json`
  passed without rerunning or changing the original8-case artifact/report.

- Broker04 stage08 native SHA96a7e9b9c3d6eeb18c0e6369a1fb835acf74f63ee001ac8d69628e0c61b3cf6c
  is terminal failed1, original `.tmp/rust-source-broker-public-04/report.json`.
  First semantic result and CAP succeeded, but spawnSync returned only95.55s
  later. Actual lifecycle records serverPID72848/creation134359002530812522,
  normal terminal at accept with no error. Separate escalated OS lookup confirms
  the original PID absent; the in-harness PowerShell absent lookup returned a
  nonzero status, now corrected to explicit zero observation status.
  The normal60s accept-idle expiry after semantic processing suggests caller
  pipe EOF was retained by inherited handles, not a transport accept failure.
  Official Rust1.99 Windows Command defaults to inheriting handles; its
  inherit_handles(false) and raw attribute APIs remain nightly-only:
  https://doc.rust-lang.org/std/os/windows/process/trait.CommandExt.html#tymethod.inherit_handles .
  A narrow native launch with no handle inheritance and an actual owned pipe
  fixture is being coordinated with Windows; no idle/deadline budget is enlarged.
- Real public CTE01 preserved first failure:7/8 equal; self-reference had no
  physical table dependencies and CASC empty-file-ID admission masked SQL's
  unresolved binding. Original `.tmp/rust-data-cte-public-01/report.json` remains.
  The handler now skips CASC admission for zero physical inputs and lets the
  lexical evaluator report the true binding error. A fresh artifact and second
  actual differential are pending; no full public CTE pass claimed yet.

### 2026-10-08 public CTE, derived tables and broker lifecycle correction

- CTE public02 is now terminal passed:8/8 exact normalized Go business results,
  six verified captures and separately asserted additive verification claims.
  Frozen executable SHA f26133615befdfe599fba4e3217a23338bf2fe1e6977f85125b7116d2a839b08
  is recorded in `.tmp/rust-data-cte-public-02/report.json`. Root independently
  passed the same eight cases in
  `.tmp/rust-supervision/data-cte-public-independent-01/report.json` (6.09s is a
  functional-run duration, not a performance result). First01 failure remains.
- Derived FROM/JOIN tables are now compiled and evaluated in the same lexical
  scope and original Work budget. Physical rows are borrowed; computed rows move
  into bounded local sources. Sixteen frozen-Go cases passed in
  `.tmp/rust-data-derived-first-tests.log`, alongside CTE2, aggregate3 and JOIN3.
  Actual public derived-table cases, recursive CTE, UNION/compound, expression
  subqueries, CSV, domain/asset/network/partial profiles and whole performance
  matrices remain unverified or unimplemented as applicable.
- Environment generator identity intentionally differs at
  `environment.manifest.identity.generatorVersion`: Go `environment-v1`, Rust
  `environment-ts-v1`. Root approved preserving the truthful tree-sitter and
  restricted-descriptor derived identity/cache generation. This single explicit
  difference does not exempt repository/product/commit, definition content,
  source mappings, coverage or the remaining identity fields from comparison.
- Inheritance observation01 contained a synchronous control call inside the
  Node exit callback and therefore did not independently prove EOF timing.
  Corrected asynchronous observation02 waited a real two seconds before controls:
  parent exit12.36s, output still open while the authenticated broker remained
  alive, then output EOF18.23s after retirement. Original observations remain in
  `.tmp/rust-source-broker-inheritance-observe-01` and `-02`.
- Native launch now uses Windows CreateProcessW with handle inheritance disabled,
  exact executable/UTF16 argv and scoped environment; startup ownership and
  authenticated parent/creation/one-use credential checks remain. Root separately
  passed two actual Windows fixtures in
  `.tmp/rust-supervision/windows-noinherit-independent-01.log`.
  Fixed packaged observation03 used immutable stage09 native SHA
  0f7c99899d77d17669b32febc4307b073f4c06f6778967cabd39d537e4ca4d88:
  output EOF11.740s and parent close11.741s preceded the true two-second yield;
  the original broker was still OS-identity matched and authenticated ready.
  `.tmp/rust-source-broker-inheritance-observe-fixed-03/report.json` passed.
- Complete-cache public02 remains terminal FAILED overall in
  `.tmp/rust-source-broker-complete-public-02/report.json`. Its First/Second/Third
  requests were complete, CAP verified, retained one worker and batches1/2/3;
  authenticated First cache hit retained batches3. Active exact worktree lease
  correctly refused6. Immediate prune then failed6 because the broker's Retire
  response preceded final owner-lock release. Cleanup retired the original broker.
  The cache portion is proven; the complete cache/prune lifecycle is not yet passed.
- The latest client now holds the authenticated broker's original process handle
  before requesting Retire and waits for its actual kernel process exit under
  the same deadline/cancellation control before returning closed. It never
  terminates an unrelated PID or weakens prune locks. CLI check and Source/Data
  library strict clippy passed. A fresh packaged artifact and public lifecycle
  rerun are pending; stage09 does not contain this later correction. Public
  startup/active Ctrl+C, sibling isolation, replay/foreign peer and configuration
  retirement matrices remain required.

### 2026-10-08 cache/prune owner pass, independent startup failure and SQL compound

- Immutable packaged stage10 native SHA
  ebf4f1399d84b0efa2bc78c909e9ac565b157f47f2caab276b6e4a6c852d681d
  passed owner complete-cache/prune public03, terminal0:
  `.tmp/rust-source-broker-complete-public-03/report.json`. All four semantic
  captures verified; First/Second/Third retained the original worker and batches3,
  First cache hit kept3, active lease refused6, prune completed0, released lease
  acquired0, status/close were idempotently closed and the worker PID was absent.
  This closes the owner-observed retirement race for that complete fixture.
- Root's independent fresh complete01 using the same immutable stage10 FAILED
  startup capability3, not passed:
  `.tmp/rust-supervision/source-broker-complete-independent-01/results/report.json`.
  The child had reached accept but failed the unchanged five-second readiness
  window. Exact original PID49048 was subsequently OS-confirmed absent. Do not
  count owner03 as independent/full broker acceptance.
- Retail public05 likewise remains failed1: first whole2551-Lua semantic result
  and CAP succeeded, next status failed5/external.io with early EOF. Original
  OS creation identity matched before the failure; subsequent original PID36900
  was absent. `.tmp/rust-source-broker-public-05/report.json` preserves it.
  Exit observer01 saw status ready followed by retirement EOF, but its harness
  initially failed to save/wait observer frames on that error branch. Corrected
  observer02 instead failed at startup before it could hold a successful broker.
  Both `.tmp/rust-source-broker-retail-exit-observe-01` and `-02` remain failed;
  neither proves the unexplained process exit code or a guessed cause.
- Startup now has a bounded token-free diagnostic of identity/spawn/readiness
  phase times and exact child creation identity. Lifecycle diagnostics also
  include timestamps. No identity algorithm, five-second budget, idle budget or
  cache admissibility has been weakened. CLI check0; a new optimized package is
  pending to measure these phases. Public cancellation and startup reliability
  remain incomplete. A same-logon aggregate broker lock also prevents treating
  different homes as permission to run independent brokers concurrently.
- Derived public01 retained its actual7/8 failure: the unresolved correlated
  field error lacked Go's original column/candidates details and message.
  The shared missing-column diagnostic now computes the bounded top8 candidates
  using simple Unicode scalar case mapping and bounded Levenshtein arrays;
  public mapping retains those details. Derived public02 passed8/8 exact
  results/errors and all successful CAP/verification claims:
  `.tmp/rust-data-derived-public-02/report.json`. Frozen executable is
  `.tmp/rust-data-derived-compound-binary-01/lycheedev.exe`; its hash is in both
  reports. Root separately passed16 frozen derived cases in
  `.tmp/rust-supervision/data-derived-fixture-independent-01.log`; this does not
  itself certify public02 or all derived/SQL profiles.
- UNION ALL now executes every branch under one original lexical scope and Work
  budget, moves retained rows without cloning scalar payloads, keeps first-branch
  column names and applies the outer ordering/limit once after tail validation.
  Sixteen frozen-Go cases plus partial/shared work/memory/cancel/bad-tail cases
  passed2, with affected CTE2, aggregate3 and derived tests also passed. Root
  independently passed2 in `.tmp/rust-supervision/data-union-fixture-independent-01.log`.
  Actual local compound public01 remains7/8 FAILED overall:
  `.tmp/rust-data-compound-public-01/report.json`. Non-ALL syntax failure has the
  correct exit/code but still lacks the precise Go syntax location/message.
  Recursive CTE, expression subqueries, domain/asset/network/partial surfaces,
  CSV and the complete cold/warm50% performance matrix remain required.

### 2026-10-08 optimized broker and independent SQL public results

- Root independently passed optimized stage11 complete cache/prune in
  `.tmp/rust-supervision/source-broker-complete-release-independent-01/results/report.json`,
  native b726772edbb2b59e7db25d54fd69651c7e05f780c59ac9b9463bd9d878576ad4:
  same worker/batches1/2/3 and authenticated hit keeps3, four verified CAPs,
  active lease6, prune0, released lease0 and actual worker reaped. This is a new
  optimized artifact; failed debug stage10 independent startup remains failed.
- Owner held-kernel observer03 on that optimized artifact passed first Retail
  semantic request, ready status and authenticated retirement with exact original
  broker exit0. Startup diagnostic measured identity9ms/spawn76ms and readiness
 1466ms under the unchanged5s window;35 readiness probes were counted, phase
  details beyond the bounded24-entry allowance were explicitly truncated.
  `.tmp/rust-source-broker-retail-exit-observe-03/report.json` is a single-request
  lifecycle proof, not evidence that the older debug failures are explained.
- Optimized Retail public06 FAILED second semantic request after the first result,
  CAP and ready status succeeded: external.io/early EOF at3.07s, owner cleanup0.
  `.tmp/rust-source-broker-public-06-release/report.json` is retained. A separate
  kernel-handle observer is being applied before the second request to identify
  the actual original process exit; no guessed memory/transport fix is certified.
- Root independently passed derived public8/8 on frozen e326... in
  `.tmp/rust-supervision/data-derived-public-independent-01/report.json`, including
  the original correlated-field error's column/candidates/message. Six successful
  CAPs and additive verification claims were checked separately.
- UNION public02 fixed the first failed non-ALL case by validating actual lexer
  tokens, preserving UTF-8 byte offset and one-based Unicode scalar line/column,
  and projecting the structured location into the public error. Three compound
  library tests passed, including comments, quoted/literal content and Unicode.
  Public02 passed8/8 with preserved whole local file identities and CAPs on SHA
  8368a7079932d43199c812195919a00c5d9b6a3aad589a9d51328eed0e73f589.
  Root independently passed the eight cases in
  `.tmp/rust-supervision/data-compound-public-independent-01/report.json`.
  Original compound public01 failure remains. These are selected local SQL
  profiles, not recursive/subquery/full SQL, whole Data accounting or50% proof.
- Doctor-facing runtime verify_with_control APIs now authenticate actual files
  without spawning tools. Git mirror_health_with_control is a real offline
  contained regular-directory/bare/SHA-1 format probe, explicitly objectsVerified
  false; it never claims whole object integrity, fetches, writes refs or invents
  an origin binding (the existing mirror fetch path does not set origin).
  The actual isolated Git test passed1; CLI check passed. Full doctor acceptance
  belongs to its separate Core/CLI provider integration and is not implied here.

- The second-request kernel observer04 is terminal FAILED overall and now gives
  causal process evidence: original authenticated broker PID27220/creation
  134359040219327134 was held before the second request; that request failed EOF5,
  and the original kernel process handle reported exit -1073741819 (0xC0000005,
  access violation). `.tmp/rust-source-broker-retail-exit-observe-04/report.json`
  and `observer.stdout.jsonl` preserve the exact identity/exit observation with
  stage11 stable hashes. This is not normal idle expiry, a guessed memory limit
  or proof that the named pipe caused the crash. Further exception module/stack
  evidence is required before changing the unsafe boundary. Source remains
  unregistered for these incomplete broker/research acceptance profiles.

### 2026-10-08 exception diagnosis and selected warm performance

- The old debugger01 report's passed=true was an insufficient harness verdict:
  original held process exited C0000354 and the second CLI created a replacement
  broker. Preserve its raw report, but treat it as FAILED retained-session proof.
  The harness now requires the same original PID/creation and kernel exit0;
  CLI calls are asynchronous so Node keeps draining debugger output, and attach
  readiness requires the actual complete marker line rather than command echo.
- Debugger02 passed those strengthened identity/exit assertions but its second
  request took61s under attach and did not reproduce the fault. This is debugger
  perturbation, not a fix or an explanation of the uninstrumented C0000005.
- Debugger03 and04 FAILED with the strict original-kernel assertion on the
  unchanged optimized b726... stage11. Microsoft CDB's second-chance command
  (-c2) was needed to capture C0000409 FAST_FAIL_FATAL_APP_EXIT subcode7. In
  `.tmp/rust-source-broker-av-debug-04/debugger.log`, allocation failures
  C000012D precede that stack: ownPE+a3a8e1/a3849f/a4dc5b/a3b048 then caller
  +4bf541/+52d0f2 and OS thread entry. Nearest exported tree_sitter_lua labels
  are not authenticated function attribution. The112485-byte local minidump
  remains private; no token, arguments or business memory are published.
  Stage11 has no retained matching PDB; a new same-build exe/PDB pair is needed
  for precise functions. This changed failure shape does not establish the cause
  of the original uninstrumented access violation, and2GiB is unchanged.
- A not_run minimisation seam now exists:
  source_session_repeated_real::actual_two_full_workspace_batches_under_aggregate_commit_limit.
  It preserves actual full-source/canonical catalog and production2GiB, removes
  only public CLI/pipe callers, repeats references+hover twice on one actual
  WorkspaceSession, and asserts same PID/results. Its eventual result cannot
  replace the original full public retained-session regression.
- Root's40 process samples and four selected legacy-field comparisons passed
  on stable sealed b726... versus frozen Go in
  `.tmp/rust-supervision/source-release11-selected-warm-performance-01/report.json`.
  Warm medians(ms)/Rust:Go ratios: inspect258.35/155.60=.60227 FAILED50%gate;
  exact737.69/172.07=.2333, prefix692.86/160.98=.2323 and indexed676.59/161.46=.2386
  passed their selected warm gates. Report.partial_failed is authoritative even
  though the older runner exited0. First Go exact10.29s was derived-cache rebuild,
  not controlled cold. Full cold/warm, broad/context/semantic/product matrices
  and overall Source/Data/Live50% acceptance remain incomplete.
- Data availability::fetch_versions now supplies the Core provider's narrow
  actual transport: one fixed regional HTTPS endpoint, raw slot validated,
  no redirects/no state writes,1MiB UTF-8 body, timeout min(30s,callerremaining).
  Existing ureq/platform TLS is reused without a new dependency. Cancellation
  is checked around each blocking read; AtomicBool cannot instantly wake a
  blocked socket and that limitation is explicit. Two offline admission tests
  passed (cargo test -p lycheedev-data availability::tests --locked --offline,
  original handle15452 exit0); strict Data lib clippy also passed. Actual remote
  HTTP/full target.available public behavior remains not_run until provider
  integration and separate network fixtures, and no command is advertised here.
### 2026-10-08 precise thread-admission seam and recursive SQL

- Root independently passed the two offline availability admission fixtures in
  `.tmp/rust-supervision/data-availability-admission-independent-01.log`.
  They do not prove actual HTTP or the full target.available public workflow.
- Stage12 is a new optimized symbol-rich artifact, not the old b726... bytes:
  native SHA256112b412ac7ed6d344f43709383fae263ec908524f19a47ed609ce04827607fcc,
  matched PDB SHA256fd011e3d10ccc5994d73c5be4c7bdb99f6799b1d26f00a5d65d936970f971774.
  `.tmp/rust-source-broker-symbol-debug-02/debugger.log` identifies
  core::result::unwrap_failed -> DisconnectObserver::start -> session::serve.
  Implicit std::thread::spawn unwraps an actual native thread admission failure.
  This precisely attributes the new panic; it does not identify the cause of the
  older uninstrumented C0000005. Allocation failures under the debugger remain
  distinct observations, and the aggregate commit contract stays2GiB.
- The full-source SDK minimisation was actually run (original handle43942,
  terminal1), not not_run:2551 Lua files/full catalog, same2GiB owner Job,
  no CLI/pipe/DisconnectObserver. First semantic batch succeeded; second failed
  with LuaLS -32603 / not enough memory after32.18s. The load-bearing memory
  failure therefore also exists without the pipe seam. Full retained public
  quality is still FAILED; no file omission, limit raise or tiny-input pass is
  used to claim support. Actual Job/process commit telemetry is pending the
  Windows owner's narrow read-only API.
- DisconnectObserver now uses fallible Builder::spawn with the ordinary
  production stack, returns bounded luals.input_budget/exit3 with OS code, and
  serve reaps the worker and retires the owner before returning that failure.
  The actual owned-pipe/native thread rejection test passed1, proving the caller
  token remains uncancelled and the same pipe can still carry the typed response.
  Root independently passed the same test in
  `.tmp/rust-supervision/source-observer-admission-independent-01.log`.
  This removes the proven panic seam; it is not a full2GiB support or AV fix.
- Recursive SQL now implements breadth-first previous-delta UNION ALL under the
  same work/memory/cancel control, preserves exact wide integers and scope,
  validates an empty seed's step, and rejects multiple/self seed references.
  Blocks own results without deep cloning scalar payloads. Extra unused scalar
  parameters follow the frozen Go behavior (ignored); referenced parameters
  remain required. The initial unsupported and binding failures remain recorded.
  Sixteen frozen-Go cases and the shared budget/cycle/partial/BADcell fixture
  passed2; Root independently passed2 in
  `.tmp/rust-supervision/data-recursive-fixture-independent-01.log`.
- The first combined SQL suite exposed two old implementation-specific memory
  assertions that omitted newly admitted scope metadata. They were replaced by
  actual allocation-boundary assertions, preserving rejection before large clone
  or concatenation. The guarded original handle70271 finished0: recursive,
  nonrecursive CTE, derived, compound and relational suites17 tests passed;
  Source/Data all-target strict clippy passed. Recursive public CASC/CLI cases
  are pending a fresh frozen build. Expression subqueries, remaining SQL/Data
  profiles and whole accounting/performance are not implied by these fixtures.
- Fresh static Skill forward evidence is `.tmp/rust-skill-forward-current-01/report.md`:
 33 calls/15 verified CAPs on stable hashes, no broker/semantic/game testing.
  Independent definition completeness, literal binding/basis, duplicate match
  projection and inspect CAP-to-source-blob traceability remain concrete output
  improvements; overall complete semantics and full recall must be preserved.

- Fresh optimized recursive public01 completed0 in6.41s (not a performance
  sample): eight sequential fixed Retail Map/ChrClasses cases matched frozen Go
  exactly; six successful CAPs verified and additive whole-file/requested-page
  verification declarations passed separately. The runner preserves full rows,
  schemas, source identity and public errors, and requires stable binary hashes.
  `.tmp/rust-data-recursive-public-01/report.json` and frozen
  `.tmp/rust-data-recursive-binary-01/lycheedev.exe` bind the actual artifact.
  This is selected recursive local CASC/CLI proof, not all expression subqueries,
  network/partial profiles, whole Data accounting or the50% performance gate.

- Root independently passed the eight public recursive cases on frozen
  d8c91d64552101f747feeddd6c04197021f6552212fe0b943d07a9faeed95bdf in
  `.tmp/rust-supervision/data-recursive-public-independent-01/report.json`.
  All retained fields/errors, six successful CAPs and additive verification
  matched; full SQL/network/partial/accounting/performance remain separate.
- Spell aura/summon navigation now consumes the same authenticated typed WDC
  provider, exact SpellID integer matching, effect6/effect28, first-array NPC,
  full summon rows, ordered relation provenance and65536+1 truncation probing.
  Two library fixtures passed; Root independently passed2 in
  `.tmp/rust-supervision/data-spell-navigation-fixture-independent-01.log`.
  New paths remain unregistered pending full providers/profiles evidence.
- Initial broad cargo test mistakenly compiled all Data integration artifacts;
  original94996 ended101 with real rustc allocation failures and fast-fail.
  Precise --lib -j1 then passed both navigation fixtures and strict lib clippy.
  This compiler event is not the product2GiB Job result.
- Real fixed Retail SpellEffect1459 first oracle attempt failed3 with
  container.key_unavailable / key provider failed for chunk112. A premature
  old-Rust artifact invocation returned unsupported and is retained as stale
  artifact evidence, not a parity result. The actual fresh Rust first profile
  failed3 with retained-byte budget instead, so the refusal shape is NOT parity.
  `.tmp/rust-data-spell-public-01/` preserves those raw results and current frozen
  binary01. No empty/unsupported input is labelled a successful spell baseline.
- Data decode failure/retry ownership now uses an exact temporary retained-byte
  reservation; failed output/scratch drops release that admission before retry.
  Successful ownership keeps the charge; shared permanent metadata/work/IO
  charges never reset. One reservation unit and four cache/key regressions
  passed, including actual nested BLTE/key loading failure, integrity and cancel.
  Initial test-only empty key document was correctly rejected; it was replaced
  by a valid owned nonempty document. Data all-target strict clippy passed.
  Real SpellEffect refusal retest is pending the fresh artifact; no default
  budget has been raised and no complete whole-query accounting is claimed.
- SDK original80518 now sampled public-SDK Job/held-process counters and passed
  two identical full-source batches on one PID in50.25s under unchanged2GiB.
  `.tmp/rust-source-repeat-memory-01/report.json` and artifact.json preserve this
  run separately from43942's second LuaLS not-enough-memory failure. Before
  worker owner private commit288276480B; after first owner290676736B and worker
 1855508480B. Job peak2147758080B then2150699008B is the native reported peak,
  not current aggregate commit (unsupportedPublicSdk/null). The reported limit
  remained2147483648B. Process private commit, working set and peaks are distinct.
  This edge-pressure/variable outcome is not a causal AV fix or public acceptance.
  The SDK keeps live full Index/environment/raw declaration objects through the
  two batches; release of already-consumed derived objects is the next single
  minimisation variable, preserving full source/definitions/query content.

- The subsequent terminal release artifact for temporary-reservation ownership
  now reports the actual unavailable key14f4b11d7b067aa2 for fixed Retail
  SpellEffect1459, rather than the earlier retained-byte budget refusal.
  `.tmp/rust-data-spell-binary-02/lycheedev.exe` and
  `.tmp/rust-data-spell-public-01/temporary-fixed-rust.json` preserve it.
  Go's chunk112/provider message, error details and query stage still differ;
  this is not full public refusal parity and these paths remain unregistered.
  The separate Go offline observation is a metadata-budget refusal and must not
  be conflated with the online missing-key result.
- `tests/local_domain_parity.rs` now prepares bounded genuine local archive/index,
  framed Encoding, Root, WDC3 and matching cached DBD inputs, with explicit
  catalog/flavor/version identity and a fixed DataPin. It exercises the real
  domain dispatch without a substituted provider. Six success profiles and a
  corrupted original object despite a decoded cache are drafted, not_run until
  build admission returns. This synthetic installed-data fixture is neither
  actual-client nor partial/CDN acceptance.
- Disk governance update: no Source/Data compiler, CLI, debugger or broker
  handles are active. New cargo work is paused for disk admission (current
  target exceeds the initial20GiB allowance), while source/fixture work continues.
  No new Cargo target directory or whole-target copy was created. Immutable
  stage11/12 artifacts, matched12 PDB/EXE identity, original failures/private dump,
  frozen domain/recursive inputs and the protected memory report remain evidence
  that terminal scratch cleanup must preserve.

### 2026-10-08 resumed Source/Data ownership and key retry preparation

- The resumed owner confirmed no active Source/Data compiler, CLI, debugger or
  broker processes and no held build-channel leases. Cargo remains paused until
  disk admission is restored. Existing optimized failures and memory evidence
  remain protected; no new runtime acceptance is claimed.
- The full-workspace SDK diagnostic already has the optional
  `LYCHEEDEV_SOURCE_RELEASE_DERIVED=1` variable and exact source/definition/config
  identity fields. Its off/on comparison is still not_run. Production
  `session::analyze_request` already drops the initial Index/environment at the
  end of worker creation, so a successful SDK drop experiment cannot itself
  establish the cause or repair of the original broker access violation.
- The tiny local domain report now writes passed only after both all six
  positive cases and corrupted-original rejection pass, and restores original
  archive bytes before the negative assertion. The fixture remains not_run.
- Local file key recovery now checks current public-key state and the precise
  missingKey fact. A content decoder that has already loaded the fixed public
  document must not cause an outer replay of Encoding/Root/content; provider
  failure also remains distinct from a missing key. The permanent metadata/work
  ledger is not reset and no budget was raised. A genuine local encrypted CASC
  fixture plus the authenticated pinned public-key document and a lazy-public
  versus explicit-identical-document ledger comparison are drafted, not_run.
  This is not parity with the earlier Go provider/network failure.
- After admission returned, the first precise Data integration invocation
  (original exec handle24595) compiled successfully in1m04s but exited101:
  `local_domain_parity` failed during fixture target resolution because its
  `.flavor.info` lacked the required `Product Flavor!STRING:0` header. Receipt
  `.tmp/rust-disk/evidence/69fdf4e7-c596-4e32-a9d7-f5af43e2dd84/receipt.json`
  and `.tmp/rust-data-local-domain-input-astra01` preserve the actual failure.
  No query/provider parity passed. The ordinary channel was released for Live
  before the fixture repair/retest; the key-retry lib fixture remains not_run.
- Correcting that fixture header produced all six authenticated domain results
  plus restored-original integrity rejection in0.36s, but the new owned-process
  runner forced the remaining Job after Cargo exit0 and returned overall1.
  `.tmp/rust-disk/evidence/f1854791-ada4-49d5-8ac9-b32b6c3c67f1/receipt.json`
  retains this distinct process-management failure. The fixed test EXE is sealed
  at `.tmp/rust-data-domain-fixed-artifact02/local_domain_parity.exe`, SHA256
  `aab23de2b054632c636fdfd7b4e16f13d4be729c999ab923ae6e7d1c9b23f666`.
  The original receipt does not identify the remaining descendants or their exit
  timing, so it alone does not prove a bookkeeping race or leaked child cause.
  Foundation's subsequent wrapper investigation and raw replay remain separate
  from the failed overall Cargo run; it is not silently relabeled successful.
- The unchanged exact integration Cargo command subsequently completed0 through
  the repaired owned runner; receipt `d9ae01c7-306e-416f-9cd3-a3ac604c37cc` binds
  the same source-tree digest as the failed wrapper run. This is runner repair
  verification, not extra domain coverage. The genuine encrypted fixture was
  then prepared with the existing fixed983550-byte public key document;
  receipt `b24ad790-540f-4147-acdb-07b02fa31c63` completed0 without a download.
- The exact key-retry lib fixture passed1 in0.29s: lazy-public and explicit
  identical-document paths both permanently charged metadata7877224B and
  retained7890546B, with decode work306 versus305 and zero network. It returned
  the same actual missing-key42 fact. However its compiler Job retained a live
  `VCTIP.EXE` at the30-second drain boundary, so runner receipt
  `8d55b129-517c-4170-b537-64abca5106cf` remains overall failed despite Cargo0.
  This new receipt contains the actual Job PID/image; it does not retroactively
  explain the older receipt lacking that observation. Current installed tool
  identity/signature and the observing wrapper source are preserved beside it.
  The business test EXE is sealed in `.tmp/rust-data-key-retry-artifact03`, SHA
  `d6b4a717a64fd6a34c94465828edd0e3b11e0a42d1e02b6163a9268eecc168bf`.
  Cargo is paused again for compiler-process ownership handling; no repeat of
  the passing business test is used to erase this true remaining-child failure.
- Public local-domain differential coverage is now verified for the fixed older
  CLI SHA256 `456ed952406eacbbd99881883eb7ff8952083a6a5c701e2a21014894f3d8cb6e`
  against frozen Go `5738418756ee57c536518ff71223f2e7843ac3495a846b4f063d3104d83aae87`:
  six valid aura/summon profiles and four admission errors all match the full
  retained legacy result/error fields. Additive table-authentication declarations
  are separately asserted, and each of the six Rust captures passes public
  `evidence verify`. `.tmp/rust-data-local-domain-public-host02/report.json`
  binds the real authenticated synthetic CASC/DBD/WDC fixture `astra03`; its
  `process-proof.json` records all 26 owned processes terminal and fully reaped.
  Binary hashes are stable before/after. This is narrow public-command coverage,
  not a new candidate, encrypted-key retry, CDN, full domain or real-game proof.
- The initial restricted-token run is retained at
  `.tmp/rust-data-local-domain-public-astra01`: four admission errors matched,
  while all six valid Rust requests returned external.io/access denied and Go
  succeeded. One identical request under the host token then completed0 with
  owned cleanup (`.tmp/rust-data-domain-access-host01`), followed by the complete
  ten-profile host-token comparison above. Permissions affected this run; the
  specific denied filesystem/API operation has not been identified. This does
  not establish a product fix, and the initial failure has not been overwritten.
  Returning to the restricted token after all successful host runs again gives
  the same access-denied error (`.tmp/rust-data-domain-access-restricted03`),
  ruling out a simple first-use versus already-populated-cache explanation.
- A separate offline envelope audit found that seven of those ten profiles
  (six successful queries and missing `--spell-id`) lack Go's
  `context.timeoutSeconds=300`; three parser-level errors match with empty
  context. The original v1 report remains unchanged and proves only the stated
  result/error subset. `envelope-audit.json` beside it is therefore failed for
  full envelope parity. The public runner now compares every outer envelope
  field, validates each capture's hash/size reference and captured result bytes,
  then normalizes only capture IDs/timestamps and the previously declared
  resource/additive fields. Core envelope repair and a new-artifact rerun remain
  pending; this is not full public-command acceptance.
- The complete Retail SDK off/on experiment is now terminal0 on one newly sealed
  test EXE `8468e91b5ccae94c5e096d159c8997207aabc5dbc2421a96cc2af9da7313a762`
  and matching PDB `bdba1d316efa5c6f2deacfe4c136d0916e5b575837d96753ca8edac7401a9576`.
  Compile receipt `526520db-32e7-470f-9089-99970cec5bf7` completed0 with source
  digest `17af5ac87d93213dc9f4bce66a960361bdfa44e9157116aebc234b4d60cf79c6`.
  `.tmp/rust-source-independent-retail-01/input/manifest.json` independently
  authenticates the pinned Git tree and every framed blob: all4036 source
  documents, including2551 Lua, bind digest
  `88d68f2c4bdb352245cbcb53cd41ec906b294e2ea599ee6374e00be169be7928`.
  Both runs assert this exact set before queries; actual locked1039688-byte
  environment library SHA256 is
  `d5c3071e9c696cc19b02f609cae3fe8be19089ee5e434537bdf81f3f81b7c103`.
  Full inputs/configuration match, EXE/ZIP/manifest stay stable, both workers
  complete two same-PID identical-result batches and owned trees fully reap.
- `.tmp/rust-source-repeat-memory-astra02/report.json` shows the actual effect:
  retaining derived objects leaves owner private commit290693120B and LuaLS
 1855504384B after first batch; releasing only already-consumed objects drops
  owner291266560B to7368704B before queries, but LuaLS then grows2137399296B
  with owner8810496B after first batch. Both still approach the unchanged2GiB
  aggregate limit. Reported historical Job peaks remain distinct from unknown
  current aggregate commit. Observed test durations77.13s versus27.91s are one
  diagnostic pair, not performance acceptance. Production broker already
  scope-drops the same objects, so this test-only switch is not a production
  fix or an explanation of the old AV. Worker memory pressure, original AV,
  repeated full public session/cancel/reopen/prune acceptance remain unresolved.
- Foundation's new sealed symbols CLI `d15b8c886b34b2a087193c0298f64ac3f21660dcc702969619254145ec6b1cf4`
  now passes the strict v2 local-domain public comparison at
  `.tmp/rust-cli-timeout-public-01/report.json`: six genuine authenticated
  CASC/DBD/WDC success cases and four argument cases, including complete stable
  envelopes and timeout context. All six capture blob references match their
  stored byte length/hash and full returned body; public evidence verification
  passes. All 26 owned native processes terminate and reap. This supersedes
  only the earlier missing-context gap for those ten fixture profiles; it does
  not establish real-game, CDN/partial-provider, remaining domains or performance
  acceptance. Earlier v1 and failure evidence remains untouched.
- Prepared `.tmp/rust-source-symbols-diagnostic-astra03` for the next public
  AV observation: the exact sealed EXE/PDB above plus all 1307 original stage11
  tool resources (22457122 bytes), individually checked against the original
  release manifest. This is a diagnostic release, not a complete package or
  candidate acceptance. The new `broker-retail-diagnostic.mjs` owns all CLI,
  broker, LuaLS and kernel-handle observer children within one Job and checks
  source manifest and original/copied resource identities before and after.
- The first no-debugger public execution is terminal at
  `.tmp/rust-source-public-retail-astra03/report.json`. Both requests return0
  with the full pinned2551 Lua files,23354176 bytes and zero exclusions; source
  manifest, EXE/PDB and all original/copied tool resources remain unchanged.
  Nevertheless this is a failed retained-session result: the held original
  broker PID65920, creation FILETIME string134359111291704583, terminates with
  kernel exit `0xC0000409` (-1073740791), and the second request silently creates
  broker PID50292 with a new creation identity; status reports only one batch.
  Exact exit is established, but the exception parameters/fast-fail subcode and
  cause are not yet known. Do not equate this with original stage11's observed
  `0xC0000005` or claim success from two public exit0 envelopes. The outer Job
  exits1, forced=false, all handles close and active process count is zero.
- Controlled CDB observation on the same sealed EXE/PDB, tool resources and
  complete input is retained at
  `.tmp/rust-source-public-retail-cdb-astra04/exception-evidence.json`.
  Broker PID51284/creation134359114023456909 exits with `0xC0000409`, parameter0
  `7` (`FAST_FAIL_FATAL_APP_EXIT`). Matching private PDB stacks show
  `std::alloc::rust_oom` reached from `serde_json::Value` BTreeMap insertion in
  the LuaLS stdout reader thread, while main is in `BrokerListener::accept`.
  The log also records heap commit failures `0xC000012D`. This establishes the
  allocation-failure/abort path for this execution. The102483-byte minidump SHA
  is `596d9be0c84b32a000864ebdfa6a4712b577dadf556e18366380b008688df706`.
  `.ecxr` reported HRESULT0x8000FFFF; the debugger-selected fault-thread stack
  nevertheless reaches `RtlUserThreadStart`, and all thread stacks were saved.
  Debugger timing perturbation and the absence of simultaneous global-versus-Job
  commit measurements remain explicit. This does not establish the historical
  stage11 `0xC0000005` cause. This diagnostic returns1 with no forced cleanup,
  all process handles reaped, and all input/resource hashes stable.
- The first focused implementation now validates frames without retaining JSON
  trees and discards only notifications the previous consumer already ignored.
  Requests/responses retain raw bytes under one-slot backpressure, and the
  consumer takes the result instead of cloning it. Destruction disconnects the
  receiver before killing/reaping the child and joining pipe threads. The
  existing8MiB frame,64MiB cumulative input and10000-frame limits remain.
  `JobGuard` has an optional child aggregate commit limit; its default behavior
  is unchanged. LuaLS uses1.75GiB inside the original2GiB broker aggregate Job.
- The governed Source lib run, receipt
  `fa5cc09b-60f3-449a-a9fc-cad72edeed2f`, is terminal0 with29/29 tests. New tests
  cover escaped/duplicate keys, id:null, number/private-key/depth/invalid-JSON
  handling and actual Node pipe peers: full-queue close, partial-frame close,
  pending-response cancellation, and300 ignored notifications followed by a
  null-ID server request and its legitimate response. Every child is observed
  reaped through its original process handle. Source digest is
  `18ec3a809aa1ad928e3d4499282080b7201a524260754eb4cee1eadc221bdde1`.
- This is not yet a complete owner-memory proof or full Retail repair result.
  Up to three raw frames coexist (reader, channel and consumer), plus parser
  scratch, consumer Value, accumulated transformed results and serialization.
  An8MiB syntactically valid response composed of small nested arrays can expand
  beyond256MiB because each Vec may reserve capacity4; that candidate reserve
  is therefore not a worst-case bound. Bounded consumer/result admission and
  actual child-limit/descendant verification remain follow-up obligations.
  Child Job attachment still occurs after spawn, retaining the documented
  pre-attachment descendant race; no race-free tree claim is made. Next use the
  sealed first-patch CLI on the unchanged full Retail workload to determine
  whether the child partition preserves normal supported behavior.

### 2026-10-08 Sol ownership handoff and unchanged-load runner preparation

- The user explicitly changed every subagent to `gpt-6.1-sol` with `high`
  reasoning. This overrides the older Astra preference in the requirements
  document; Source/Data ownership continues in this existing worktree.
- Rust/Cargo/release/vendor inputs remain frozen while the root owner resolves
  the interrupted symbols build's original process/receipt terminal state.
  Source/Data has neither restarted that unknown invocation nor started any
  compiler, CLI, LuaLS, debugger or actual-load experiment in this handoff.
- The existing full Retail diagnostic retains the unchanged incoming
  `CallbackRegistryMixin:RegisterCallback` request and limits. Its JS observer
  now explicitly requires both requests to have `cacheHit=false` and final
  retained-session `batches=2`, in addition to the original held-process
  identity, ready state and kernel exit0 checks. `node --check` passed; the
  strengthened actual-load experiment remains `not_run` on the first-patch
  artifact. Historical reports were not modified.
- Read-only follow-up confirms that raw queue backpressure alone does not bound
  `Rpc::request`'s `serde_json::Value` expansion or the retained transformed
  results from128 requests. Consumer-tree admission, accumulated result and
  serialization admission remain open; no256MiB worst-case proof is claimed.

- Root independently completed the new controlled symbols build with terminal0,
  receipt `22619f37-88d4-4210-a82b-02e03f69e5a1`, source digest `18ec3a...`.
  The sealed first-patch EXE is
  `12e20bb40589ff975bd129f2f569e624f898c49a02e3dcd7c3695cb46d064812`,
  with matching PDB
  `1ba0b25be3a784bea298625ce684d31a8367274502eec183ef787fb8fb2e5552`.
  `.tmp/rust-source-symbols-diagnostic-sol01` copies all1307 original stage11
  tool resources/22457122 bytes without modification; its release manifest is
  `3ea9e93583ee0392051fd42cdd378f7c192e0887414be6197e2892b8931cc4e1`.
- The first restricted-token invocation at
  `.tmp/rust-source-public-retail-sol01/report.json` terminated1 before any
  semantic request: initial `source session status` returned5/external.io
  access denied. Its own Job fully reaped, active0, forced=false; all sealed
  inputs remained stable. This environmental failure remains preserved.
- Root then granted an exclusive full Retail load interval after its267 public
  parser differentials completed. The identical no-debugger host-token run,
  original exec handle27123, is terminal1 at
  `.tmp/rust-source-public-retail-sol02/report.json`. First query returned0 in
 18110.93ms with all2551 Lua files/23354176 bytes, zero exclusions and
  `cacheHit=false`. The second identical query returned5 in6391.98ms with
  LuaLS JSON-RPC error `-32603`, `not enough memory`; it is an actual normal-load
  failure under the new1.75GiB child partition, not an AV or EOF diagnosis.
- The held original broker PID29632/creation FILETIME134359137135216467 was
  not replaced. It retired the failed worker and reported idle/batches0;
  authenticated close returned0 and the original kernel handle observed exit0.
  The outer Job proof reports full tree/handles/tool-lock reaping, active0,
  forced=false, no deadline hit; EXE/PDB/manifest and all original/copied
  resources remain stable. This proves clean refusal and cleanup for this run,
  while retained two-batch normal support remains failed. The earlier broker
  C0000409 allocation abort and original stage11 C0000005 remain separate.

### 2026-10-08 typed RawRPC and pre-spawn derived-object release

- Source now separates full authenticated workspace preparation from LuaLS
  initialization. The production broker's Index/environment/initial declaration
  objects leave a real inner scope before any LuaLS spawn. The prepared object
  retains the complete frozen files, read locks, byte/hash checks, source identity,
  coverage and lease; initialization reloads the already locked declaration file
  and checks its original size/hash. The existing public SDK wrapper composes
  the two stages without changing its signature. This is a separate variable
  from the earlier SDK after-open release experiment and typed RPC parsing.
- Production RPC retains a fully validated raw frame, never an arbitrary result
  Value tree. Validation also implements the pinned serde arbitrary-precision
  first-key Number semantics before dropping unused notifications, including
  floating-point notifications. Method-specific consumers keep the first1000
  locations plus an extra-item truncation fact, stream hover to16KiB while
  validating all content and discard unused initialization extensions only after
  complete JSON/UTF-8/depth/private-number validation. Broker results are typed
  and directly serialized under the unchanged8MiB output allowance; raw frame,
  cumulative64MiB input, query counts and90-second limits remain unchanged.
- Two previous accidental Rust differences are explicitly corrected to the
  frozen Go typed contracts: Location/LocationLink select URI and range together
  when the primary URI is empty/null, reject wrong URI types and retain zero
  defaults for absent coordinates; only the four range coordinates are output.
  RPC errors retain code/message, reject wrong types and render the frozen Go
  inner `server error <code>: <message>` wording. These fields do not retain
  unused extension DOMs. Prior JSON-formatted Rust error failure evidence stays
  unchanged; this does not certify complete outer Source error-envelope parity.
- Current allocation controls use fallible raw/string/typed-vector/output
  growth, admission before query-descriptor cloning and a shared cumulative
  result allowance across both broker batches. The after-scope-drop owner
  PrivateUsage32MiB startup admission is bound to the already-held owner PID,
  creation FILETIME, sequence and timestamp in a non-authoritative diagnostic.
  It is neither RSS nor a claimed current aggregate Job counter. Its normal-load
  feasibility and the complete worst-case commit inventory remain obligations.
  The child1.75GiB and original aggregate2GiB limits were not changed, and no
  diagnostics configuration change was mixed into this batch.
- Root's first controlled Source+CLI check completed0 without warnings:
  receipt `734f1ec4-d7a9-4375-a78d-86f9e35f26f5`, source
  `b44b2e706c99bd93f5516fa3cd2fe9cd87df4985292f6b036c05856ced61282d`.
  Source lib39/39 then passed at
  `d3b5e29c-a3fe-4b52-8d0b-4f1dbfa9f334` on the same source. Fixtures include
  a7.3MiB valid discarded location tail, malformed JSON/private-number tails,
  long Unicode hover and late invalid contents, large unused initialization
  extensions, duplicate/escaped keys, linked locations and exact output limits.
- First strict Source clippy genuinely failed101 with20 lints, preserved at
  `21075101-b856-4dd6-bd2e-d854e0106fe6`: three new projection conditions and
  existing compact flow formatting/argument-count issues. Source conditions
  were merged, flow formatting normalized and register's existing scope fields
  grouped into DefinitionScope; no lint suppression or intended behavior change.
  Root then passed39 lib plus4 frozen-Go flow goldens at
  `c965c7c4-4d03-4fdb-82e7-3859acd69d1b`, and strict Source lib clippy at
  `876cbca5-50d7-476c-9273-48518baddbde`, both on source
  `411d8092f3ccd9e894d6d8e029ec03907730781cbd19d597fb78ff890863c907`.
  Root also reports16 CLI unit checks green. These are module checks, not a
  successful complete Retail retained-session repair.
- Root prepared two read-only Retail diagnostic improvements, both syntax0:
  bounded optional64KiB resource-diagnostic snapshots before/after first, after
  second and after cleanup, and sealed copies/SHA256 of8 observer/supervisor/
  policy inputs with before/after identity checks. Neither workload, query,
  budget nor pass criteria changed. The scripts and new actual full-load run
  remain pending the new sealed CLI; historical reports are untouched.
- Source compilation inputs are frozen for the new symbols seal. Its first
  admission stopped before Cargo for disk.cache_quota:19365161681 cached bytes
  plus2147483648 reservation exceeded21474836480 by37808849 bytes. Root is
  reviewing only owned generated incremental cache for safe reclamation;
  quotas and protected EXE/PDB/log/input evidence were not enlarged or discarded.
  No new CLI artifact or full normal-load pass is claimed from that refusal.

### 2026-10-08 Sol symbols02 diagnostic staging and unregistered quota fixtures

- Root recovered the interrupted build as not started, preserved the original
  cache-reclaim and read-only recovery evidence, then independently sealed a
  fresh symbols build with terminal0, receipt
  `7a1c53bf-9080-4243-8530-3e63772b813c`. Source digest remains
  `411d8092f3ccd9e894d6d8e029ec03907730781cbd19d597fb78ff890863c907`.
- Source/Data executed only the authorized diagnostic staging command, terminal0.
  `.tmp/rust-source-symbols-diagnostic-sol02` contains EXE
  `b748a02beaff2b8c42d79ae8e0282f0aa070e46e3477f2e1f5ab919ec240e370`,
  matching PDB
  `3156dc3644343e89ff827908387cf5d9d34947bd70aec548c213392def1e98a9`
  and release manifest
  `e1b47965482e81618a2fc17e316ec905aeb3d70e46c7088f1a28fecb538f5bd2`.
  All1307 original stage11 resources/22457122 bytes were copied and checked
  byte/hash exact. This is a diagnostic release, not candidate acceptance.
- Three unregistered fixture drafts remain outside crates under
  `tests/rust-source-data`: `staged-budget-tests.rs`,
  `staged-native-quota-tests.rs` and `native-quota-peer.rs`. They prepare actual
  128-query pipe batches,129-query pre-RPC rejection, cross-batch result quota,
  descriptor admission, native committed-memory refusal and held-process tree
  reaping. Only the embedded Node peer's parse-only syntax and expected result
  serialization sizes were checked. No Rust draft was compiled, registered or
  run, and no CLI/LuaLS/game input was launched by Source/Data.
- Native allocation refusal attribution remains explicitly open: the current
  JobGuard exposes no exact child Job telemetry or completion notification.
  The draft distinguishes near-budget allocation refusal from proof of its
  cause, and its descendant starts after attachment, leaving the pre-attachment
  race unclaimed. The normal full Retail result and further fixture execution
  remain pending the root owner's exclusive interval and freeze release.

### 2026-10-08 symbols02 full Retail unchanged-load diagnostic passed once

- Root's actual `.tmp/rust-source-public-retail-sol03/report.json` is terminal0
  and passed on the sealed symbols02 EXE/PDB/release above. This is one full
  Retail normal two-batch diagnostic under the unchanged2GiB aggregate and
  unchanged1.75GiB child partition, with the original diagnostics configuration.
- First query completed in21825.536ms; the second identical public semantic
  query completed in39925.0771ms. Both had `cacheHit=false`, complete semantic
  state, all2551 Lua files/23354176 bytes and zero exclusions, retaining the
  exact fixed source pin and identical index identity.
- Original broker PID22596/creation string`134359227097928633` was held by its
  original kernel handle. Final status was ready with batches2 and retained
  worker33376; authenticated close returned0 and the original broker's kernel
  exit was0. The outer supervisor also returned0 with every process/handle/tool
  lock reaped, active0, forced=false and all observed sealed input/resource hashes
  unchanged. Parser267 and SDK904 differential checks on this exact EXE also
  passed before the normal load, as independently recorded by root.
- The optional telemetry file was absent before this run, then sequence1 with
  fresh timestamp and top-level owner PID/creation string matching the held
  broker. PrivateUsage fell from289243136 bytes before real derived-object Drop
  to8069120 bytes afterward, below the32MiB admission before LuaLS spawn. The
 400138240-byte process peak is startup history, not a steady-state bound;
  observer first process PrivateUsage was7262208 bytes.
- Nested ProcessMemory identity creation is a JSON number and is rounded by
  JavaScript JSON parsing. It is not used as authorization/identity evidence.
  Raw diagnostic copies and the top-level owner creation string preserve the
  exact binding; held OS process handles remain authoritative.
- This establishes one supported normal-load pass after the typed projections
  and pre-spawn derived-object release. It does not establish repeated stability,
  complete lifecycle/resource worst-case proof, or causal repair of the original
  stage11 C0000005 and later broker C0000409 failures. Rpc's three remaining
  infallible thread-spawn sites need focused admission/partial-start cleanup;
  staged128-query/result/actual child quota tests remain unregistered/not_run.
  Source compilation inputs remain frozen during the root-granted LiveP3 window.

### 2026-10-08 focused Rpc thread admission and child Job observation batch

- After root's first LiveP3 matrix reached terminal0, Source's narrow resource
  batch replaced the three product Rpc `thread::spawn` calls with named,
  fallible Builder admissions. Handle-vector capacity is admitted before child
  spawn; Rpc owns cleanup before the first thread. Failure at any boundary
  disconnects the raw receiver, terminates/reaps the exact child Job, releases
  the writer and joins only the threads already admitted. Typed refusal is
  `luals.input_budget`/exit3 with resource name and OS code. Existing budgets,
  runtime resources, query workload and LuaLS diagnostics configuration remain
  unchanged.
- Windows JobGuard gained read-only memory accounting against its own held Job
  handle and a bounded64-PID membership snapshot with explicit overflow refusal.
  Existing BrokerJob memory accounting was factored without changing meaning;
  current aggregate commit still reports unavailable. PID lists are observations,
  not identities or authorization, and the fixtures hold original process handles
  and export creation times as strings.
- Five light resource fixtures are registered in `rpc_budget_tests.rs`: actual
 128+128 pipe batches on one worker,129-query refusal before RPC,64+64 shared
  result admission, SDK-only descriptor admission and actual OS impossible-stack
  thread rejection at each partial-start boundary. Two ignored native commit
  fixtures are registered in `rpc_native_quota_tests.rs`, retaining the original
 2GiB enclosing Job and production1.75GiB child partition; they require a sealed,
  SHA256-checked/read-locked standalone helper and an exclusive interval.
  Modules were moved inside `crates/lycheedev-source/src` before registration so
  their actual bytes enter the controlled build's source digest.
- Actual formatting is terminal0. The initial restricted rustfmt shim attempt
  returned access denied before formatting; the authorized host invocation then
  succeeded. Cargo/CLI/LuaLS and all new Rust fixtures remain `not_run` pending
  root's controlled checks. Native near-quota cases stay separate from light
  resource checks, normal-load repetition, lifecycle and performance evidence.

- Root's governed Source+CLI+Windows check completed0, receipt
  `487b0122-3f74-43d3-a963-06a744a3bb71`, source digest
  `95e3ee3a7318d62983622dc571bbf027b22aabd2456f9498bfcefddb073607a0`.
  Source lib then passed44 with zero failures and two ignored native quota
  cases in5.87s, receipt `8ceec605-c035-4be8-ab68-639b1e03c3fb`, same source.
  Root independently inspected all three original admission reports: actual
  OS code87 from invalid stack reservation, typed exit3, held child kernel
  exit7 and24/179/45ms cleanup timings. This proves generic Builder-failure
  cleanup, not a resource-pressure failure cause or historical AV causality.
- Strict Source+Windows lib/tests clippy then genuinely failed101 with one
  old integration-test `unwrap_err` after `is_ok` lint, receipt
  `0f519064-0543-4d8e-aabf-5878900f727b`. An equivalent match replaced it without
  suppression. The authorized fixture-only follow-up also binds light case
  directories to held owner PID/creation string/counter, requires create_dir,
  and writes admission reports with create_new. Native ignored cases require
  a fresh report case directory and create_new. No old directory was deleted
  or overlaid. Formatting is0; follow-up checks remain pending root supervision.
- The standalone native allocation helper remains uncompiled/unrun. Its
  governed Cargo test-target build/seal plan is proposed to root separately;
  no ad-hoc rustc, native allocation or assumed helper artifact was used.

- Root's fixture-only follow-up Source lib run passed44/zero failures/two
  ignored in5.55s, receipt `a2103d45-395e-4e78-ae48-6cf1e04fc824`; strict
  Source+Windows lib/tests clippy then passed0, receipt
  `512f2e02-26c1-45bf-8b9c-54d57ce0bc9f`. Both bind source
  `e1b67f3f69bc76d1fa5a3cf8188b0d585f1bb657e498e9cd78112bdcd59fe4ba`.
- Root then authorized one governed native-helper fixture addition. The helper
  moved to `crates/lycheedev-source/tests/native_quota_peer.rs`, and Source's
  Cargo.toml adds a harness=false test target without dependencies/lock/product
  crate changes. Absent AUTH explicitly reports inactive/allocation=false and
  exits0 before stdin, spawn or commit. Active cases use a fresh32-lowercase-hex
  fixture nonce echoed by both replies; the exact Job limits/flag and held-bound
  PID membership are checked before any allocation request. Header/body inputs
  are bounded8192/<4096 bytes. Actual helper build, sealing, inactive smoke and
  both exclusive native quota cases remain `not_run`; no ad-hoc compiler or
  allocation was invoked. The nonce-updated lib-test EXE must also be rebuilt
  and sealed under this new source identity before either ignored case.

- Root's actual governed helper+lib compile/seal completed0, receipt
  `f3c403ff-418e-47f3-a901-3a092f487555`, source
  `72e20b90774c1ea9e67bf376e1aa2f3fa01a3eccf320d7432c55c972ed287925`.
  `.tmp/rust-source-native-quota-sealed-01/artifact.json` binds helper EXE
  `b1c5d13fcacf99366f1e531724dd75627bc2814f7a258efd4884f845e808bcf6`,
  lib-test EXE
  `db4b0483976c3e4b4d772e7d2012ec74e8ad4857b8d4025b16647266d3cccbaa`,
  both corresponding PDBs and the helper source hash. The inactive smoke is0
  and performed no allocation. First runner01 then hit an evidence-parent
  ENOENT before execution/allocation; its outcome stays preserved.
- Runner02 repeated inactive0 and actually reached direct preallocation, then
  failed101 at exact Job membership admission: observed32940/39672, expected
  only helper32940. The original lib-root924 and all tree/handles reaped with
  active0/forced=false. No allocation request was sent, no quota result was
  produced, and aggregate case remains `not_run`. PID39672 is unknown; it is
  not labelled conhost or allowed through the gate without evidence.
- Root authorized only an observation enhancement. Before the unchanged gate,
  the fixture now saves raw/parsed prepare result, mode/nonce, exact held owner/
  helper/member identities as creation strings, image paths/parent PIDs, limits,
  flags and PID snapshot or explicit open errors in a create_new preallocation
  observation. No product/helper/cap/config behavior changed. Microsoft documents
  nested Job process lists as including associated and child-job processes; that
  does not identify this extra PID. The changed lib rebuild and identical direct
  preallocation reproduction remain pending root supervision.

- Root's identical observation03 direct run actually failed101 before allocation
  again. `.tmp/rust-source-native-quota-run-03/direct/new-native-case/`
  `preallocation-observation.json` identifies the extra as held Windows system
  conhost38220/creation string`134359269297121419`, exact image
  `C:\Windows\System32\conhost.exe`, parent5688. Held helper5688 was created at
  `134359269297071220`, reported descendantPid0 and matched the unchangedb1c5
  helper; owner was36012. Queried child limit was1.75GiB/flags8704/active2/
  peak1826816, with enclosing2GiB Job active1 before spawn. allocationSent=false;
  all outer processes/handles reaped, active0/forced=false. Earlier failures stay
  failures and are not rewritten as passes.
- With that actual evidence, root approved a fixture-only typed OS console role.
  The CUI helper, product Job behavior, creation flags and budgets remain unchanged.
  Bounded test-only GetSystemDirectoryW derives the exact canonical system conhost
  path; its file is read-locked and hashed before helper spawn. Additional members
  must match that full path, a held known-peer parent, creation no earlier than
  that parent and max one console role per parent. Unknown paths/parents/open
  failures still refuse before allocation; path/hash do not claim Authenticode.
- Partial pre/post observations are saved before gated assertions. Exact members
  must equal known peers plus explicitly admitted console roles; a same-parent
  identity replacement refuses. Every observed admitted handle remains held and
  its kernel retirement is recorded; peer/explicit descendant require7, while
  console exit is recorded as observed. Enclosing Job must retain only the owner.
  Formatting is0; strict checks, resealed lib and separate exclusive direct/
  aggregate native quota execution remain pending root. No allocation was started
  by Source/Data.

- Root's actual run04 admitted the typed direct console role and sent the native
  allocation request, then failed at a fixture assertion that historical Job
  peak must not exceed its configured limit. The preserved postallocation
  observation reports successful retained native bytes1872756736, refusal1455,
  configured child limit1879048192 and historical Job peak1884520448. Held helper
  current PrivateUsage1876955136 and console1261568 were separately sampled;
  those non-atomic samples are not current aggregate telemetry. Helper peak
  PrivateUsage1883258880 also differs from current usage. Root's supervisor
  reaped the failed101 run completely, active0/forced=false; aggregate case is
  still `not_run`.
- The peak<=limit assertion was unsupported by the API contract and has been
  removed as the sole authorized fixture correction. Raw historical peaks
  remain in every report, current aggregate stays unavailable, and retained
  successful allocation bytes must still remain within the configured limit,
  reach its bounded near-limit region and encounter refusal. Queried flags/
  exact members/all original held exits/owner-only retirement remain required.
  The helper stays unchangedb1c5 and budgets are unchanged. A tentative charge/
  rollback explanation is only an inference, not a proved cause; run04 remains
  failed evidence. Strict checks/reseal and both exclusive cases remain pending.

### 2026-10-08 exclusive native quota run05 passed with retained failure chain

- Root's actual `.tmp/rust-source-native-quota-run-05/report.json` is
  `passed_scoped_native_quota`: inactive, direct and aggregate each exit0,
  every outer tree/handle/tool lock reaped, active0, forced=false and sealed
  inputs stable. Source digest is
  `e15fe5c05ac458670b00ee0d788d0bce65a385a804836510ad0450aff8c51a3c`.
  Source/Data independently read the supervisor and both native reports.
- `.tmp/rust-source-native-quota-sealed-04/artifact.json` retains unchanged helper
  EXEb1c5d13fcacf99366f1e531724dd75627bc2814f7a258efd4884f845e808bcf6,
  matching helper PDB7730f051437e192fb9170a04d0e066717bece03f31218ffde4f5993c23b667a2,
  lib-test EXE540c00c96e5c45356d23c69d869fdc7bc94823bfa5a508d4ff06c44cc9436e5e
  and PDB6d7e911b38a8736740a3ae3d27ffab34d5c882a88ce528077be1aa304f768845,
  with root's governed receipt7be743d4-a36a-445f-adb6-d75bddf470e5 and exact helper source hash.
- Direct retained1872756736 native committed bytes before refusal1455. Helper
 11012 and typed system console37308 remained in the queried1.75GiB Job with
  flags8704, inside the original2GiB enclosing Job. Both held kernel exits were7;
  enclosing retirement retained only owneractive1 before outer termination.
- Aggregate retained1736441856 helper bytes plus134217728 explicit descendant
  bytes, total1870659584 before refusal1455. Helper8964, descendant21348 and
  consoles39904/38412 were identified by full canonical system path, locked
  file SHA256e449bce01f275cd08f3d4e64bb73b3b43ae845a0dbdb3e6131426e66537705e5,
  parent and creation strings. The descendant console appeared only in the
  postallocation snapshot and was explicitly admitted by its known parent.
  All four original held handles observed kernel7; parent-onlyactive1 and
  outeractive0 retirement passed.
- Raw historical child peaks1884528640/direct and1886437376/aggregate remain
  in the reports, even above the configured limit. Current aggregate commit
  remains null/unsupported; no non-atomic process sample sum, historical peak
  substitution, exclusive refusal cause or pre-attachment inheritance proof
  is claimed. This is a scoped native retained-allocation/Job membership/tree
  cleanup pass, not normal LuaLS repetition, lifecycle/performance or AV closure.
- The original sequence remains intact: runner01 evidence-parent ENOENT before
  allocation; direct02 unknown-member admission failure before allocation;
  direct03 observation proved the system console but the unchanged original
  assertion still failed before allocation; direct04 actually allocated and
  exposed the unsupported peak<=limit assertion; run05 passed after that sole
  contract correction. Earlier failures were not relabelled or overwritten.
  Compilation inputs are now held for root's next LiveP3/Foundation intervals.

### 2026-10-08 LuaLS batch document lifecycle parity: red, fix, scoped green

- The owner explicitly returned the task to LuaLS. Source used diagnosing-bugs
  and the existing actual Rpc/WorkspaceSession seam to establish a precise wire
  regression before changing production behavior. Root's red command, receipt
  `2351202c-4ec6-4474-9f3f-e6d1992b7d31`, source
  `c251f772b8f8e4d07d739f806edd01ba8a6ea3b4c53636b7f26e26f8d4421f8c`,
  actually returned Cargo101: one cancellation case passed, two lifecycle cases
  failed in0.21s. Two batches left API/probe/caller documents open without
  didClose; the external API query hit the delayed-scope peer's explicit-32008
  preload error. The wrapper's unreviewed VCTIP4728 forced cleanup is separately
  retained; it does not erase those actual regression failures.
- Frozen Go opens its first usable worktree Lua and awaits definition(0,0) before
  each batch, then closes that batch's documents; error/cancel retires the worker.
  Rust's generic SDK previously preopened every file/API, while prepared Source
  preopened the API and retained opened paths across batches. The minimal fix
  covers both constructors: initialize/validate without eager didOpen, perform
  the worktree scope barrier, open queried documents on demand, didClose all
  successful batch documents, and clear state when error/cancel retires Rpc.
  Library input remains8MiB and worktree files1MiB; all Job/query/lifetime/wire
  budgets, input set, runtime resources and diagnostics configuration are unchanged.
- Regression fixtures preserve actual semantic-query counts separately from the
  additional scope barrier. Each128-query batch still issues128 user queries;
 129-query rejection issues no barrier or user query. The delayed peer locks
  down protocol ordering/lifecycle, not actual LuaLS semantic or OOM causality.
  No provider, Assets or Live protocol code was changed in this focused batch.
- Root's controlled sequential lib suite passed47/zero failures/two ignored in
 7.89s, receipt `24e944ef-fa71-40ee-a3c3-c65b46ef39c7`. Actual pinned LuaLS
  `luals_real` and `session_real` passed one each in4.93/5.39s, receipt
  `1d4f1a39-de48-4543-80e2-2cd9a54e4484`. Strict Source lib/tests clippy passed0,
  receipt `ac5e0efa-f1ea-420e-9a4a-d368a2ff56f0`. All bind source
  `46ef915ed29dfb0b707ab142c566a6b912678b043748b8ed9b20cf06d92548a2`.
  A prior cache admission refusal and owned Source incremental111284189-byte
  reclamation remain separate owner records; checks were not run outside custody.
- Root sealed the same-build CLI/PDB with receipt
  `55c7b935-2344-410a-b91a-a0b6b1690361`: EXE
  `cebd059b5b64f89e040768429a09ec66269d80ef245ecc2e0e36a4a90d355c56`, PDB
  `50eb2c00d829c06dead3db84d67ea9a64a5fc5de79533cc0d9ea30b1f46ef651`.
  Diagnostic release manifest
  `ff133be5a86f8e3612192635da50abfa172dafebfd6d0cd337a4150ac1a7e32a`
  retains all1307 original resources. This is a diagnostic candidate identity,
  not npm/candidate-release acceptance.
- `.tmp/rust-luals-document-public-01/report.json` actually passed the unchanged
  full Retail public two-batch workload: first22659.417ms, second61152.1629ms;
  both semantic complete/cacheHit=false, all2551 Lua/23354176 bytes, excluded0,
  same source pin/index identity. Original held broker38408/creation string
  `134359343930993936` remained ready/batches2 with worker3592, then observed
  kernel exit0 after authenticated close. All outer processes/handles reaped,
  active0/forced=false, sealed inputs stable. Source independently read those
  reports. The before-phase optional telemetry belonged to an older broker and
  is not fresh evidence; after-first owner/string matched this held broker and
  reported7540736-byte post-drop PrivateUsage.
- Go comparison is still incomplete: an initial full attempt stopped at
  git_output_limit before semantic completion; a short-HOME first request later
  completed in about26.8s with matching generated definitions, but a logon-SID
  observer bug prevented the second observation. Neither condition is labelled
  a Go LuaLS failure, nor does one successful Go request prove retained stability.
  This Rust pass does not close original C0000005 causality, repeated stability,
  full cancellation/reopen/prune acceptance or the50% performance requirement.
  The next suspended-before-Job startup change is a separate lane/artifact;
  this lifecycle result remains bound to46ef/cebd.

- A later actual Go03 run removes the earlier observer defect and changes the
  comparison boundary. `.tmp/rust-luals-go-baseline-03/observation.json` is failed
  retained-session evidence on frozen Go
  `5738418756ee57c536518ff71223f2e7843ac3495a846b4f063d3104d83aae87`:
  first17910.0101ms/exit0 with semantic complete, exact31c7 pin, matching d5c307
  definitions, all4036 documents/2551 Lua/23354176 bytes. Root archived only the
  newly derived semantic cache to make the second request actual; source input
  was not pruned. Second2312.8718ms returned CLI0 but semantic partial, reason
  `LuaLS relation batch failed: luals.failed: broker response: EOF`.
- The original held Go broker1728/creation string`134359345798159870` exited2,
  and held worker41576/creation`134359345829379385` exited0 before cleanup;
  authenticated cleanup returned closed and the outer tree reaped. Pre-second
  sampled broker PrivateUsage347394048 and worker1799086080 are separate,
  non-atomic process samples, not current aggregate Job telemetry or a proved
  OOM cause. No allocation/exception stack has yet attributed that Go failure.
  Thus the earlier assumption that complete repeated Go load was already stable
  is unsupported; semantic cache hits can conceal whether another LuaLS batch
  actually executes. Current Rust46ef/cebd passed one actual full two-batch cycle
  at22.66/61.15s, while this actual Go cycle failed; neither constitutes repeated
  stability or the fixed20-sample performance acceptance.

### 2026-10-08 guarded LuaLS startup and final public reopen verification

- Windows LuaLS startup now uses `CREATE_SUSPENDED`, attaches the original child
  to its bounded Job, verifies the held primary thread, then resumes it. Failure
  before resume terminates/reaps that exact child and Job; cancellation/deadline
  remain distinct from spawn and preparation errors. The existing1.75GiB worker
  limit, complete source input and LuaLS runtime resources are unchanged.
- Actual Windows tests cover payload remaining suspended before attachment,
  stdio/Unicode arguments/environment/cwd, descendant Job membership and exact
  held-process cleanup, plus cancellation/injected preparation failure without
  payload execution. The first membership assertion failed because Windows also
  created console hosts; the corrected test authenticates their system path and
  bytes and rejects unknown members. The failure remains recorded separately.
- Final source digest is
  `060d81fa0beabd6a1e277a7159236c553f7877a7f39ac2f17065796412cd8ae6`.
  Strict Windows+Source lib/tests clippy passed with receipt
  `bd15f84e-8a0a-4309-9d3a-48a2533c8cb2`; final actual Windows tests passed2
  with receipt `d2fbd1eb-00b6-4f76-8af7-184a843348b4`. Before equivalent lint
  corrections, full Source lib passed47 with2 ignored and pinned real LuaLS
  tests passed2 (receipts `0df4109a-ebcb-4034-afb9-602002a2c15d` and
  `5b9de175-900c-40d4-8dfa-5e00e17537c6`). Final CLI/PDB build passed with
  receipt `e1514ce5-ca77-44db-913f-7cf46a7f2517`.
- Diagnostic release `.tmp/rust-luals-suspended-release-01` binds EXE
  `9d7079235d8fb45def66364e8d4d9b0f4f784554d13c3919bfa7ce77f1a63502`,
  PDB `744e5b09d1ade4d2967e2dbb284015685b1f6426aae4cd4baff303c4479f6cd1`,
  release manifest
  `036a2f87194b7804d98d92160f1ce9c145d19f19e3bb8318da5e035d437e7f04`.
  All1307 original resources remain unchanged. No installation or publication.
- Final public attempts01/02 are retained as harness failures:01 selected the
  sealed EXE outside the required release-native path;02 completed the first
  actual query, then its Go-oriented observer was rejected by Rust before Hello.
  The observer now reads Rust's bounded nonterminal lifecycle record after an
  authenticated CLI status, then holds and verifies the broker creation string
  and executable bytes. Worker identity comes from that status and its held
  process/image; this does not claim independent parent-chain authentication.
  Product caller authentication was not relaxed and no Hello was fabricated.
- `.tmp/rust-luals-final-public-03/report.json` and
  `.tmp/rust-luals-final-public-04/report.json` both passed on this final EXE.
  Each runs two actual public queries with one retained LuaLS worker, closes it,
  observes the original held broker/worker exiting, checks closed status and
  repeats close. Run03 took23.206/5.581s; run04 took22.882/5.375s. All four
  semantic analyses are complete, cacheHit=false,2551 Lua/23354176 bytes,
  excluded0, identical resolved reference output and generated definitions.
  API-environment extraction still reports partial, so overall complete=false
  is preserved rather than mislabelled as complete product coverage.
- Run03 held broker11036/creation`134359364981618871`, worker41588/creation
  `134359365051590541`; run04 held broker35736/creation`134359365579899382`,
  worker45656/creation`134359365649578301`. Different creation identities prove
  reopening. Both brokers exited0; workers exited7 only after authenticated
  close began, matching explicit Rust Job retirement. Both outer owners reaped
  all handles, and before/after binary/runtime/input/runner identities matched.
- This closes the scoped document-lifecycle/startup and two-cycle public reopen
  verification. It does not establish original C0000005 crash causality,
 20-sample performance acceptance, all quota/cancellation cases or full rewrite
  acceptance. Historical failures and the Go03 actual second-batch failure remain.
