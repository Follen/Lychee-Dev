> Historical reference for the Rust migration (2026-10-14): current decisions, baseline and acceptance status are in [handover.md](../../handover.md). This document preserves its original scope and evidence; conflicting requirements, protocol designs, performance targets and release instructions are superseded. Historical test results are not current-candidate passes.

# Data and asset implementation inventory

This is a source audit, not acceptance evidence. It supplements the shared
89-command ledger without changing its verification claims. The initial
inventory below is updated as implementations land; registration and local
fixtures do not establish full command acceptance. The frozen Go contract has
23 Data/Asset commands. Current proof and remaining acceptance are recorded in
[the current batch progress](rust-host-and-runtime-progress-2026-10-12.md).

| Command | Actual Rust state | Go implementation and essential behavior |
| --- | --- | --- |
| data spell auras | Handler exists in `navigate.rs`; six shared fixture profiles with summons; full envelope/provider profiles pending | `internal/records/navigate_spell.go`: exact SpellID, effect6, bounded rows and relation provenance |
| data spell summons | Handler exists in `navigate.rs`; same evidence boundary | `navigate_spell.go`: effect28, first-array NPC, optional NPC filter, full original row |
| data spell info | Seven-table closure/detail handler and six integration tests passed; public success/capture profiles and focused error-projection recheck passed | `navigate_spell.go`: seven tables; bounded trigger/description-reference closure, cycles, max depth, spell detail and unresolved relations |
| data item get | Shared catalog handler and local fixture regression; complete provider/public acceptance pending | `navigate_item.go`: Item+ItemSparse, required item, slot name, optional class row and partial facts |
| data item models | Seven-table handler compiled; six integration tests and 64 public JSON/JSONL profiles passed | `navigate_item.go`: seven tables; ordered appearance choice, model/material resources, race/gender, ambiguity and missing relations |
| data item geosets | Five-table handler compiled; six integration tests and 48 public JSON/JSONL profiles passed | `navigate_item.go`: five tables; appearance/display, geoset groups, helmet visibility and hide relations |
| data item textures | Six-table handler compiled; six integration tests and 66 public JSON/JSONL profiles passed | `navigate_item.go`: six tables; display sections, material and texture file selection |
| data creature display | Handler, 14 shared integration tests and 168 shared public JSON/JSONL profiles passed; broader provider/client acceptance pending | `navigate_creature.go`: exactly one display/model-file key, three tables, ordered candidate choice and ambiguity |
| data creature model | Handler passed the same 14-test/168-profile creature batch; broader provider/client acceptance pending | `navigate_creature.go`: all displays for one model, texture variants and geoset values |
| data encounter get | Handler/dynamic provider and eight local encounter tests passed in runtime-data-batch-01; complete public differential/provider/client acceptance pending | `navigate_encounter.go`: section tree with child/sibling/parent fallback, cycle rejection, depth/section bounds, related spell/icon facts |
| data decor list | Shared catalog handler and local fixture regression; complete provider/public acceptance pending | `navigate_decor.go`: HouseDecor, explicit1–5000 bound, ID order, rows with model0 retained, truncation |
| data decor get | Shared catalog handler and local fixture regression; complete provider/public acceptance pending | `navigate_decor.go`: exactly one decor/item/model key, absent error, ambiguous candidates and source relations |
| asset search | Local listfile handler exists; HTTP and complete provider/public acceptance pending | `listfile.go`, `listfile_source.go`, `listfile_http.go`: three listfile formats, query/extension/name/ID modes, verified cached input, explicit network, bounds |
| asset export | Local/CASC raw and BLP2 PNG/WebP routes are implemented; codec integration and 86 public image profiles passed; complete provider/client acceptance pending | `query.go`, `image_export.go`, `texture/`: authenticated raw export or BLP2 PNG/WebP; mip/channel/pixel limits; safe output replacement and captured original/output |
| asset demux | Bounded VP9 AVI handler exists; complete provider/public acceptance pending | `video_demux.go`, `video/`: local file or pinned CASC, bounded VP9-in-AVI demux, frame metadata/digests, partial policy, output transaction and captures |

The Go domain fixtures are in `internal/records/navigatetest`. They create real
bounded CASC/DBD/table inputs; they are distinct from real-game acceptance. Asset
fixtures include `internal/testkit/asset_fixture.go`, `internal/records/*export*_test.go`,
`video_demux_test.go`, `listfile*_test.go`, and `internal/command/asset*_test.go`.
Use the frozen Go program as the public oracle; do not call retired entrypoints
or import legacy user data mentioned in historical fixture comments.

## Implementation order and independent ownership

1. Complete a shared multi-table navigator on the authenticated table provider.
   Current `db2::prepare` prepares one table; SQL already has multi-file reading.
   A domain request must bind one pin, source and budget; reuse metadata and table
   preparation rather than replaying Encoding/Root independently for each table.
   Preserve row typing, unavailable data, ordered provenance, relation bounds,
   cancellation and all Go partial/truncation distinctions.
2. Add decor list/get and item get as initial consumers, then creature display/model,
   item models/geosets/textures, spell info and encounter get. This order exercises
   one-table selection, multi-table joins, ambiguity, then graph/tree recursion.
   It is an implementation dependency order, not permission to omit hard profiles.
   Extend the genuine local fixture to each table set and compare complete public
   envelopes, captured bodies and references against frozen Go.
3. In parallel, a separate owner can implement pure asset formats and filesystem
   publication in new `lycheedev-data` asset modules: listfile parsing/indexing,
   VP9 AVI demux, raw export, BLP2 decode and PNG/WebP encoding. Leave shared
   `local_query`, key/decode accounting, domain navigation and `lib.rs` dispatch
   integration to the Data owner until a small interface is agreed. At the initial
   audit there were no image/PNG/WebP codec dependencies in the Data Cargo manifest;
   adding them needs offline/license/source-rebuild closure and must not introduce
   an external executable or runtime DLL. Each format needs malformed, truncated,
   oversized, cancellation and protected-output profiles, not just valid samples.
4. Fill cross-cutting providers and existing-command gaps alongside those
   implementations. CDN transport, partial encrypted table reads and key-provider
   failures affect DB2/SQL/domains/assets together. Hotfix still needs named-table
   binding, resumable scan, Wago/Raidbots providers and exports. SQL still needs
   expression/correlated subqueries, complete meta/effective overlay behavior and
   CSV/output profiles. Stream still needs actual Ctrl+C and permanently blocked
   writer cancellation. The15-command inventory does not include these gaps.
5. Register only supported profiles honestly, then bind full regressions and
   performance evidence to the same sealed candidate. Required cold/warm/large
   load cases, Go/Rust alternating20-sample gates, final five-client and multiple
   instance acceptance remain separate from all local fixture results.

Current narrow domain public evidence: original
`.tmp/rust-data-local-domain-public-host02/report.json` proves the v1 result/error
subset on fixed older binaries. Its preserved `envelope-audit.json` fails seven
profiles because Rust omitted `context.timeoutSeconds`. Foundation owns that
Core envelope correction. Its new-artifact v2 rerun now passes all ten profiles
at `.tmp/rust-cli-timeout-public-01/report.json` on sealed symbols EXE
`d15b8c886b34b2a087193c0298f64ac3f21660dcc702969619254145ec6b1cf4`.
The v2 runner compares complete stable envelopes and validates actual capture
blob references, size/hash and full body equality; all six public evidence
verification calls pass. All 26 owned processes reap. This fixes the specific
envelope gap without expanding the fixture's provider or domain coverage.

## Content allowance and unresolved maximum-size acceptance (2026-10-12)

The public encounter differential at
`.tmp/rust-encounter-public-02/report.json` retains the original two failures:
the JSON and JSONL maximum-content-budget profiles supplied 512 MiB for a small
file, but the Rust adapter forwarded that decoded-content allowance as the CASC
encoded-storage limit. CASC rejected the limit itself before reading the small
file. The narrow correction in `local_query.rs` keeps `--max-bytes` on the
selected decoded length and retains the independent CASC limits. Root owns the
new-build differential and validation; this source change alone is not a pass.

Current implementation bounds, not new product contracts:

- `--max-bytes`: default 128 MiB, accepted range 1 byte through 512 MiB, per
  selected decoded content in the current Rust adapter. Readonly comparison
  subsequently confirmed frozen Go `v3.1.2` checks both selected decoded size
  and selected physical encoded size against this flag in
  `internal/records/reader.go::extractContentWithCoverage`. The current Rust
  exact-decoded-size test with larger BLTE framing therefore demonstrates a
  Rust behavior difference, not Go parity. Restore the selected encoded bound
  without forwarding an unsupported 512 MiB configuration to the CASC reader;
  metadata transport remains independently bounded. Execute an additional
  public differential for that boundary before claiming parity.
- CASC encoded object: 256 MiB; one index: 64 MiB; directory entries: 4096.
  These limits are not increased by the flag. Encoding uses authenticated range
  reads, with logical size checked against config; range reads/chunks are at
  most 64 MiB. The old whole-Encoding helper's 256 MiB decoded cap is distinct
  from this active range path. Root is still fully materialized, capped at
  256 MiB.
- Full content decode/cache currently refuses decoded content above 256 MiB.
  `local_query` additionally admits at most 512 MiB aggregate Root plus selected
  content. The accepted 512 MiB flag alone does not prove that either Go or Rust
  can complete an exact 512 MiB public query under the independent total budget.
- The unchanged shared `Cost::defaults()` admits 512 MiB retained bytes,
  128 MiB metadata and 100,000,000 work units. These are conservative admission
  counters, not measured RSS: full CASC reads charge twice the physical extent;
  cold decode reserves twice decoded length and charges 64 KiB work units;
  successful reservations remain charged, failed temporary reservations release
  retained bytes only. Warm decoded reuse still hashes selected encoded bytes,
  admits decoded allocation, and rehashes the cached bytes/CKey. Encoding range
  reads, metadata, key preparation and other consumers share the same ledger.

`tests/content_budget_parity.rs` adds small-file cold/warm 512 MiB flag checks,
exact decoded-size allowance with larger encoded framing/metadata, warm-cache
rejection below decoded size, invalid flags, cancellation, and changed-source
integrity checks. Its ignored `actual_maximum_content_requires_full_verified_decode`
constructs a valid compressed 512 MiB logical object and expects full verified
success. It remains an explicit unaccepted low-level boundary, not a passing test
that blesses the 256 MiB restriction or proves frozen Go success at that size.
The eventual fix also needs the full installed/CASC/public JSON/JSONL paths,
cold/warm cache, malformed lengths/chunks, cancellation during work, and evidence
publication/recovery on large content accepted by the frozen Go business route.
Go's public asset capture charges retained content in addition to metadata:
exact 512 MiB success must be established by execution or explicitly recorded
as a correctness improvement with a compatible resource model; do not assume
that the flag guarantees success regardless of the total budget.

Closing this gap requires bounded streaming content and evidence ownership,
including chunk work/cancellation and releasing allocations when their owners
drop. It must preserve full CKey/source authentication and unchanged query
budgets. Raising global budgets, trimming content, or treating the small-file
512 MiB flag profile as actual maximum-size acceptance is not sufficient.

### 2026-10-12 selected asset streaming update

The preceding materialized-content bounds describe the earlier adapter. The
selected `asset inspect` and raw `asset export` routes now stream authenticated
CASC/BLTE chunks into a verified held blob; other consumers still use the
materialized adapter. The encoded and decoded selected lengths both obey
`--max-bytes`. Global retained, metadata, work and chunk limits are unchanged.

Root's actual public JSON/JSONL cold/warm boundary calls passed all 20 cases in
`.tmp/rust-content-boundary-public-01/report.json`, including complete Go error
envelopes. A real compressed 320 MiB, 40-chunk fixture was then exercised through
both executables in `.tmp/rust-large-content-public-01`. Frozen Go cold inspect
succeeded; its legacy envelope and blob identity matched Rust. Go warm inspect
and raw export failed with `records.query_resource_budget: retained_bytes`.
Rust cold inspect, warm inspect and raw export succeeded, with the complete blob
and exported body SHA256 checked in that directory's `verification.json`.

The warm/export difference is an explicit resource-model improvement: streaming
retains bounded live chunks and an immutable blob handle instead of charging a
full selected body again. It is not warm Go parity, a budget increase, or a
reduction of the required output. Exact 512 MiB public behavior remains unrun;
the 320 MiB observation does not imply a result at every larger size.

The current fault-test additions cover changed warm encoded input, framed EKey
and chunk digest failures, whole decoded CKey failure before publication,
cancellation after a stream block, staged-publication failure cleanup, and raw
output conflict/overwrite behavior. These additions await Root's next targeted
test run; they are not recorded here as passed.

### 2026-10-12 PNG/WebP business connection

The asset exporter now connects the existing BLP2 palette/BC1/BC2/BC3/BGRA8
decoder to lossless PNG and WebP. It reads the header, palette and selected mip
from the already verified immutable source blob, rather than materializing all
mips or rehashing the source at each layer. Original RGBA alpha values, sparse
mip numbering, channel canonicalization and single-channel opaque grayscale
follow the frozen Go implementation. Source capture, encoded artifact capture
and export manifest retain their separate roles and Go ordering.

The exact new codec dependencies are [png 0.18.1](https://docs.rs/crate/png/0.18.1)
and [image-webp 0.2.4](https://docs.rs/image-webp/latest/image_webp/struct.WebPEncoder.html),
both MIT OR Apache-2.0 pure Rust packages. Root owns the lockfile, dependency
license inventory and controlled build. No external encoder executable or DLL
is used. Mip, RGBA, PNG row/state, WebP copied RGBA/frame capacity, and output
buffer allocations share the unchanged request budget and release with their
owners. PNG feeding and output copies check control between blocks; the WebP
crate's internal frame encode has no control callback, so cancellation is
observed before/after that codec call and during its publication writes.

`image_export_business` exercises authenticated CASC inputs through actual
dispatch, then decodes the generated files and compares full RGBA, image
metadata, blob hashes and the three capture bodies. It includes malformed,
unsupported, missing mip, pixel bounds, conflict, overwrite/hardlink and cancel
profiles. Both integration tests passed in
`.tmp/rust-native-semantic-image-run-02/image/stdout.bin` after the
`pixelsSHA256` wire-field correction. `prepare_image_export_public_fixture`
prepares the same BLP formats and expected pixels for real CLI comparison.
Codec-compressed bytes are not promised to match Go's encoders: lossless pixels
and provenance semantics are the comparison boundary, and each emitted artifact
must have its own exact captured byte identity.

Actual public execution is recorded in
`.tmp/rust-image-export-public-01/path-normalized-report.json`: 86 comparison
profiles passed, from 172 owned Go/Rust CLI processes, all reaped. The successful
matrix covers BGRA8, palette, BC1/BC2/BC3 and a sparse mip, PNG and WebP,
`rgba`/`a`/`abgr`, and JSON/JSONL. Additional profiles cover malformed and
unsupported input, missing mip, pixel bounds, duplicate channels, output
conflict, and overwrite retaining an old hardlink's bytes. The existing bundled
sharp/libvips decoder independently checked full RGBA and dimensions; it is a
verification dependency only. Each emitted source/artifact/manifest blob's full
hash, byte length, metadata record and references were checked independently.
The Go and Rust image/source fields and complete stable envelopes agree after
removing explicitly scoped codec-dependent identities and existing diagnostic
extensions.

The original `report.json` and process outputs are retained: its only two failed
comparisons were conflict messages containing their respective output paths.
The corrected interpretation first asserts each exact path-bearing message and
unchanged destination, then normalizes that path. No product change or CLI rerun
was needed. The reproducible runner is
`tests/rust-source-data/image-export-public.mjs`; its fourth argument selects the
existing sharp module used for independent decoding. The tested binary SHA256s
are Go `5738418756ee57c536518ff71223f2e7843ac3495a846b4f063d3104d83aae87`
and Rust `30a6f56ac5c720149ee9b4e857adc1e5958fc75b5905cf92b65e0cfdaeb1c48d`.
These authenticated synthetic inputs do not claim real-client acceptance or
complete CDN/provider coverage.

### 2026-10-12 spell-info business connection

`navigate/catalog/spell.rs` now implements `data spell info` against frozen
`v3.1.2:internal/records/navigate_spell.go`. The shared `db2::prepare_following`
path opens SpellName, Spell, SpellEffect, SpellMisc, SpellCastTimes,
SpellDuration and SpellRange sequentially in one pinned CASC session. Each
table is opened before resolving the next table's definition, preserving Go's
failure precedence. Existing shared query, row, relation and evidence bounds
remain in place.

The handler keeps breadth-first trigger/description traversal, effect-64 misc
children, cycle edges with visited-node termination, duplicate source relations,
sorted/deduplicated `triggeredBy`, default depth 5 and maximum depth 32. Open
frontiers report `max_depth`, and known leaves still receive full detail. The
description scanner preserves the exact ASCII spell-reference patterns and
uint32 parsing boundary. Full effect arrays and zeros, nullable names/periods,
truncated float presentation, the first SpellMisc row and icon provenance are
preserved. A missing referenced spell returns nullable detail and empty effects;
the frozen Go spell contract explicitly considers this complete, unlike the
encounter domain's unresolved-section behavior. No extra unresolved field or
partial flag is invented.

`spell_info_parity` adds six actual-dispatch fixture tests covering full detail,
cycles and duplicate references, missing rows, depth-bound leaf detail,
65,537-row/relation probes, shared source metadata, cancellation/deadline,
invalid arguments, and ordered/corrupt table failures with no result capture.
Its ignored `prepare_spell_info_public_fixtures` prepares retained public inputs
under `LYCHEEDEV_SPELL_INFO_FIXTURE`; the seven variants include baseline,
long chain, missing/invalid tables, nonmatching corrupt effects, and both
row/relation bounds. The common fixture gained float32 and signed two-element
array encoding to exercise the actual typed table decoder. Root's controlled
build `1f12111a-3ee0-40ed-9a97-3fcb8ab6aced` compiled this batch; all six
integration tests passed in
`.tmp/rust-file-xml-spell-tests-01/spell_info_parity/stdout.bin`.

Actual public differential `.tmp/rust-spell-info-public-01/report.json`
completed 50 JSON/JSONL profiles and 156 owned processes, all reaped. All
28 successful-result profiles passed complete stable-envelope, relation,
detail, exact capture body/metadata and public `evidence verify` comparisons.
That includes actual 65,537-row and 65,537-reference inputs with visible row
and relation truncation, and full JSONL output. Large JSONL duplicates the
result in record/end frames; a minimal Node parent writes the ordinary CLI's
exact stdout to the case file inside the existing owned Job. The wrapper's
16 MiB log cap is unchanged, and neither output nor compared fields are cut.

Twelve argument-error profiles also passed. Ten profiles exposed a public
projection gap: `data spell info` was absent from the CLI's catalog-command
error mapping. Go maps malformed WDC failures to exit 5 / `command.failed`,
and missing definitions / depth overflow to stage `query`; Rust still emitted
the internal typed error or stage `dispatch`. The original failed report and
outputs remain evidence. Root owns this CLI seam repair; no Data algorithm
change is indicated by this run. The focused runner
`tests/rust-source-data/spell-info-public.mjs` now uses the observed Go exit
codes and accepts an optional comma-separated profile list for the affected
recheck. After Root added this command to the CLI mapping, producer
`e6067d69-8fd9-4273-ad13-18e908325693` supplied the updated CLI. The focused
`.tmp/rust-spell-info-public-02/report.json` passed 12/12 JSON/JSONL profiles
with 28/28 owned processes reaped: the ten previously failing profiles and
two normal seed-100/capture checks. CLI SHA256 was
`87c2365820dfb9cbefc47c656c16dd0020274c8e0700e848d7abc2f8fbfb2e0d`.
The original 40/50 RED report remains unchanged; the large row/relation
profiles were not repeated for this mapping-only correction.

## Item models implementation and public verification (2026-10-13)

`navigate/catalog/item_models.rs` implements frozen Go v3.1.2's complete
`data item models` path. The request opens ItemSparse, ItemModifiedAppearance,
ItemAppearance, ItemDisplayInfo, ModelFileData, TextureFileData, then
ComponentModelFileData in that order through one authenticated source session.
Each table is opened before the next is resolved, preserving failure order.
The item must exist in ItemSparse, including when no appearance exists.

Appearance candidates are scanned in ascending row order. The lowest row ID
is selected; nonselected candidate links precede its link, and ambiguity is
visible. A missing selected appearance or display reports relation_unresolved
and displayID zero; no appearance returns a complete empty result. Resource
arrays omit zero and invalid identities but preserve duplicates. Model
candidates retain every relation, default to the first file, and optionally
select the first ComponentModelFileData match for the signed race/gender
request. An unavailable variant falls back to that first file with one
variant_unavailable reason. FileDataID zero falls back to the row ID.
Material resolution uses the first qualifying UsageType-zero row. Missing
resource candidates do not invent an unresolved flag. Geoset arrays preserve
their shape, zeros, and signed values. Existing 65,536-row/relation bounds and
capture/resource budgets remain unchanged.

`item_models_parity` contains six actual-dispatch tests for these choices,
ordered links, duplicates, variants, missing identity/relations, scalar versus
array fields, unused component rows, invalid nonmatching rows, required-table
failure order, shared cold/warm metadata, request cancellation/deadline, and
actual 65,536/65,537 model-row bounds. The ignored
`prepare_item_models_public_fixtures` creates seven authenticated synthetic
variants under `LYCHEEDEV_ITEM_MODELS_FIXTURE` for ordinary Go/Rust CLI
comparison. Root included the CLI error mapping and signed race/gender parser
in controlled build `31ed2bb3-3f3f-4292-9000-25c3a4a92aa7`. All six item-model
integration tests passed in
`.tmp/rust-checker-item-models-tests-01/item_models_parity/stdout.bin`; this
is the Data test result, not a claim that other Source tests in that batch passed.

The ignored fixture preparation completed separately with exit 0 and verified
process reaping. `tests/rust-source-data/item-models-public.mjs` then compared
all seven variants against the immutable Go oracle in
`.tmp/rust-item-models-public-01/report.json`: 64/64 JSON/JSONL profiles passed,
with 200/200 owned calls reaped. The Rust CLI SHA256 was
`f15aeb4d85810a0596ee637c20ee89426a5695001e4cd9aecbe666cf6d983419`;
the Go oracle remained
`5738418756ee57c536518ff71223f2e7843ac3495a846b4f063d3104d83aae87`.
Both hashes were unchanged after the run. All 36 successful profiles compared
the complete stable result, all relations, exact capture body, SHA256 and
metadata, and public `evidence verify`. The 28 error profiles compared full
error envelopes, including exit code, message and stage. Signed i64 limits
were also checked as exact original decimal tokens in each output frame and
capture, before JavaScript numeric parsing could round them.

The real 65,536-row relation-limit and 65,537-row scan-limit inputs both
passed without reduced outputs. Their JSONL file sinks retained respectively
15,191,772 / 15,194,520 Go/Rust bytes and 15,191,762 / 15,194,510 Go/Rust bytes;
the existing owned-job log limit was unchanged. Resource counters were audited
against their fixed budgets separately from semantic equality. The Rust
accounting gaps remain declared; there is no real-client, CDN, performance or
complete resource-accounting acceptance claim.

## Item geosets implementation and public verification (2026-10-13)

`navigate/catalog/item_geosets.rs` implements `data item geosets` over the
five required tables in frozen Go order: ItemSparse, ItemModifiedAppearance,
ItemAppearance, ItemDisplayInfo, HelmetGeosetData. The existing item appearance
and bounded row helpers are shared with item models, with catalog-local
visibility. Integer array projection is shared as well; the models algorithm
and its already-passed public cases are unchanged.

The result preserves complete geoset and helmet-visibility arrays. Each
positive visibility value scans matching helmet rows in ascending ID order,
emits the original visibility as a relation target, and appends every signed
HideGeosetGroup value, including zeros and duplicates. Nonpositive visibility
values remain in the result but do not trigger scans. The uint32 scan selector
cast follows Go even for a larger positive array value, while its relation
retains the original value. Missing hide fields project to zero. Missing
helmet rows produce an empty hide list without inventing a partial flag.
Appearance ambiguity and unresolved chains retain the shared semantics.
The relation cap stops new links but keeps all already-admitted hide values;
the row cap retains its first truncation reason.

`item_geosets_parity` supplies six actual-dispatch tests, plus ignored
`prepare_item_geosets_public_fixtures` under `LYCHEEDEV_ITEM_GEOSETS_FIXTURE`.
Its ten authenticated synthetic variants exercise the normal chain,
scalar/negative/wide visibility, missing hide fields, required-table failures,
nonmatching corrupt rows, and actual 65,536/65,537 helmet-row boundaries.
The shared fixture adds float32 two-element arrays for the selector-cast case.
`tests/rust-source-data/item-geosets-public.mjs` statically prepares 48
JSON/JSONL profiles with complete result, relation and capture comparisons,
public capture verification and the existing large-output file sink. Its
JavaScript syntax check passed. Root added the CLI mapping and compiled this
batch in producer `7c200505-6f5e-457f-95ba-f1f2113f4d2b`. The Data targets in
`.tmp/rust-empty-geosets-tests-01/report.json` each passed six active tests:
item_geosets_parity and the shared-helper item_models_parity regression.
Both test processes exited 0 and were reaped; this does not claim that every
Source target in the same report passed.

The ten-fixture preparation completed separately with exit 0 and verified
process reaping. `.tmp/rust-item-geosets-public-01/report.json` passed all
48 JSON/JSONL profiles with 152/152 owned calls reaped. Its 28 successful
profiles retained and compared complete results, all relations and capture
bodies/metadata/public verification; the 20 failure profiles retained complete
error envelopes and exit codes. The original positive visibility values above
uint32, selector casts, signed hide values and duplicate expansion all matched
the frozen Go oracle. CLI SHA256 was
`d64704f2541868842b509d6be16a5f694fd781e56970532dd8cbb42314e9d2cb`,
unchanged before and after both public runs.

The actual relation-bound JSONL output retained 18,182,040 Go / 18,184,172 Rust
bytes; the row-bound output retained 18,182,030 / 18,184,162 bytes. These exceed
the existing 16 MiB process-log cap and were kept in full through the existing
owned file sink, with its child CLI trees reaped and the cap unchanged.
`.tmp/rust-item-models-public-02/report.json` additionally passed four profiles
from the original normal-item and ambiguous-selected-item cases, with 16/16
owned calls reaped. The prior 64-profile model result remains untouched, and
its large-boundary cases were not repeated for the shared-helper change.
All resource-accounting, synthetic-source and real-client scope limits above
still apply.

## Item textures implementation and public verification (2026-10-13)

`navigate/catalog/item_textures.rs` implements the frozen Go six-table path:
ItemSparse, ItemModifiedAppearance, ItemAppearance, ItemDisplayInfo,
ItemDisplayInfoMaterialRes, TextureFileData. Each required table opens before
the next resolves, then the item identity is checked. The existing appearance,
bounded-row and texture-file helpers are reused; the texture helper's only
change is catalog-local visibility.

Sections retain ascending source-row order, signed component values and
duplicates. Zero/invalid material identities are skipped. Texture selection
preserves the shared bounded `rowUint32` conversion, qualifying UsageType zero
and FileDataID-to-row-ID fallback; it adds only the selected texture relation.
No new relation from the section table is invented. Missing section rows or
usable texture candidates produce complete empty results. A zero display ID
returns immediately even when that display row exists. Appearance ambiguity
and missing-chain partial flags remain shared with models/geosets.

`item_textures_parity` adds six integration tests and ignored fixture creation
through `LYCHEEDEV_ITEM_TEXTURES_FIXTURE`. Its eleven authenticated variants
include signed/default/float field projection, zero-display early return,
missing fifth/sixth tables, corrupt nonmatching rows, and real 65,536/65,537
section and texture-candidate bounds. The full admitted section result remains
available after relation truncation. The independent
`tests/rust-source-data/item-textures-public.mjs` has 66 JSON/JSONL profiles,
full envelope/capture/metadata/public verification and the unchanged large
JSONL file sink. Producer `b61aa5e4-db41-4764-9c50-87188f97ead9` compiled the
batch. The three Data targets in
`.tmp/rust-material-return-textures-tests-01/report.json` each passed six active
tests: item_textures_parity, item_models_parity and item_geosets_parity. Their
processes exited 0 and were reaped; the Source target failure in that report
is not represented as a passing batch.

The eleven-fixture preparation completed separately with exit 0 and verified
process reaping. [The public report](../../.tmp/rust-item-textures-public-01/report.json)
passed all 66 JSON/JSONL profiles with 200/200 owned calls reaped and no
comparison differences. Its 34 successful profiles compared complete results,
relations, capture bodies, SHA256/byte lengths, metadata and public evidence
verification; its 32 failure profiles compared complete stable error envelopes.
The CLI SHA256 was
`f04687ebf3dbb6c618a347a5a1c248b7047944df1d2be5210e89e375eb0455e2`,
unchanged before and after execution.

The relation-bound JSONL retained 20,564,564 Go / 20,567,005 Rust bytes; the
section-row-bound JSONL retained 20,564,555 / 20,566,995 bytes. All four outputs
were retained completely through the owned file sink, with child trees reaped
and the existing process-log cap unchanged. The texture-candidate row boundary
also passed. Shared-helper regressions in
`.tmp/rust-item-models-public-03/report.json` and
`.tmp/rust-item-geosets-public-02/report.json` each passed four normal/ambiguous
JSON/JSONL profiles with 16/16 owned calls reaped. Earlier public matrices and
reports remain unchanged; their large-boundary cases were not repeated.

These results cover authenticated synthetic local inputs. Complete resource
accounting, CDN/provider coverage, real-client and performance acceptance
remain open; the existing resource-accounting gaps remain declared.

## Creature display/model public verification (2026-10-13)

Producer `336c130d-f9ed-4a47-8898-377e6d3d52dc`, source
`6e5c791d90cb6728d7f3810ad89537686378891a2b508528e59414c742d04605`,
passed all 14 active creature integration tests, with two fixture-preparation
entries ignored, in `.tmp/rust-nominal-creature-regression-tests-01/report.json`.
The process exited 0 and was reaped.

[The public report](../../.tmp/rust-creature-public-01/report.json) passed all
168 JSON/JSONL profiles from 31 authenticated synthetic fixtures, with 496/496
owned calls reaped and no Go/Rust comparison differences. The original 17
scenarios remain intact. Coverage includes both display selectors, all model
displays, first-model selection without fallback, zero/missing model data,
array/scalar/signed/float projection, actual signed 64-bit geoset arithmetic,
required-table error order, corrupt nonmatching rows, exact match-field types,
65,536/65,537 row and relation boundaries, and capture overflow without a new
capture. Complete stable envelopes, JSONL frames, relation arrays, raw i64
tokens, capture bodies/SHA256/metadata and public evidence verification were
checked. The Rust CLI SHA256 was
`b530e24ecc482b720a0dc7cf391a4ec0978a1d1c3222bb7b1202cedb044df7f1`.

The largest retained stdout was 16,215,545 bytes, below 16 MiB. Large JSONL
profiles used the owned file sink and preserved full output; the existing
process-log cap was unchanged. Resource counters remain conservative admission
counters, not measured RSS, CPU cost or performance evidence. All three declared
accounting gaps remain open. Full resource accounting, CDN/provider coverage,
real-client and performance acceptance are not established by this matrix.
