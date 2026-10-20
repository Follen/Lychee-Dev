# Current Agent consumers, recovery identity 2026-10-20

Status: implementation and source review only. No tests, Node execution, Cargo,
format/check command, native queue, CI or performance collection has been run.
The user requires the entire migration to finish before any of those are dispatched.
This is newly reconstructed source based on b42e5b676adc20b28ee435d1eb08cc6e552755db,
not a recovery assertion for the lost r1–r5/a853 bytes or results.

## Entry mapping

| Entry | Current Rust execution | Reference and acceptance |
| --- | --- | --- |
| rust-data-performance-paired | catalog Query/Page/Capture/Release through Agent; real existing asset inspect/export for BLTE and BLP | authenticated Go323, one explicitly selected case per bounded group; twenty alternating timed pairs and a separate preflight for performance; catalog is correctness-only |
| rust-data-generation-paired | typed Project/Sort/Scan through Agent; explicit production cache prepare; fresh WDC, prepare-inclusive first, conditional first, subsequent IPC consumption | authenticated Go323 full SQL results/CAP; twenty alternating pairs and separate preflight; prepare cost included, never subtracted |
| rust-data-csv-diagnostics | actual Export for every page; strict quoted cells, doubled quotes, CRLF, full concatenated CSV; complete query CAP | Go323 SQL JSON and full query CAP are only the semantic baseline. CSV is correctness-only: no claim Go supports CSV, no JSON-versus-CSV performance ratio or --acceptance |
| historical-data-run / historical-generation-run | original frozen SQL collector source | historical analysis only, never current Agent acceptance |
| historical-data-csv-diagnostics | original Rust 63d47f29/a8604d0d contracts and tests | explicit historical wrapper; existing reports, fixtures, hashes and assertions unchanged |

Current tools accept one JSON configuration and optional --acceptance except CSV.
The configuration schema is lycheedev.paired-agent-request-inputs.recovered.v1.
Required fields: mode (data/generation/csv), artifact, goSource, goPackage,
luaLSArchive, importerReceipt, fixtures, conditions. All supplied artifact paths
are absolute. The tools run from the matching product source root. Data also
requires caseId and sharpModule. Conditions retain machine, powerAndThermal,
competingLoad, disk, operator. Data fixtures are catalog/content/images; the other
two modes use numeric. Each fixture entry contains directory, preparation and
preparationSHA256, referring to the newly sealed current producer report.
Historical preparation receipts cannot be relabeled for this schema.

Fixture production is explicit via agent-prepare-fixtures.mjs (or --csv-numeric).
A separate explicit --prepare-importer creates a compiler-authenticated receipt.
Neither happens implicitly during a measurement. The ignored Rust importer target
agent_fixture_import runs import_owned_agent_fixture with LYCHEEDEV_AGENT_IMPORT.
Its bounded request contains schema lycheedev.paired-agent-seed.recovered.v1,
owner_receipt, home, fixture_home, snapshot, pin, documents. Each document is a
DBD definition cache key, original value and original blob_path. The importer
uses production pin_selection, shared-blob publication, and DataCacheMetadata
strict first creation. No Node private-database image or fake metadata migration.
Only active paired-agent and skill-data-usage owner identities are permitted.
The parent Skill exporter may build this same target in its own sealed build.

## Evidence and budgets

- Reuse existing rust-owned-process Windows request/start/root/receipt/wrapper/
  stdout/stderr chain. Do not replace the Job with a Node child PID assertion.
- Authenticate the actual helper request, binary, arguments, coordinator,
  run ID, wrapper hash and native PID/creation identity. Reject duplicate IDs,
  borrowed commands, missing receipts, uncertain tree cleanup or clock escape.
- Replay every pair from its retired owner's sealed pair.json and every original
  log, not sample.valid or copied durations. Cold/warm share precisely their own
  home; owners and homes are distinct between pairs and sides.
- Original Agent frame bytes, full CAP bytes and provenance are retained.
  CAP blobs must be <=16 MiB. Query pages alone cannot satisfy CSV.
- Full Go CAP/source blobs and full asset outputs survive home cleanup. The 17 MiB
  BLTE source is a legacy asset blob, not an increased Agent CAP limit. Large
  legacy buffers use <=8 MiB owner-sealed chunks, each hash and length plus a
  complete <=64 MiB whole-buffer hash. Missing or changed chunks fail closed.
- Independent replay reopens the original authenticated fixture inventory,
  CKey/EKey/source pin/definition identity, whole ordered rows and exact values,
  complete CSV, and all RGBA pixels. libvips runs within an owned replay helper.
- Agent sample timing includes the Node JSONL helper startup, actual CLI and worker tree, output drain and reap; no helper cost is subtracted. Memory retains whole-Job observations and filters source-executable observations by the real Rust product path, not the Node helper.
- Collector lifetime is at most 570 seconds. Collector and independent replay
  share one 600-second outer budget; replay never receives a fresh 600 seconds.
  Expiry is a failure, not a partial accepted sample. Twenty repetitions are not
  silently reduced to fit the budget. Performance remains postponed.
- Disk defaults, default thread stack and production artifact identity remain
  enforced. No profiler output from old SQL is presented as actual DF profiling.

## Source dependency readback and integration boundary

The parent-owned rust-agent-client.mjs API is AgentClient(command,args,options),
request/success/close. Each response is checked again using its original raw bytes.
The current strict fixture parser rejects missing numeric source context, unsafe
integers and lossy decimal spelling; historical parsers remain byte-identical.

The Core demand source must preserve original sourceReading/sourceVerification,
StaticIdentity, full sourceDefinition/sourceLayout, and the nested
 typedGenerationCache representationSelection/representationReason. A hit does
not by itself prove IPC. The chosen full Project/Sort/Scan generation fixture
requires actual authenticated_arrow_session_snapshot consumption; arbitrary
finite requests may honestly select demand_work_boundary and are not covered by
this fixture. Initial/fresh source reads are not mistaken for session reuse. The full-scan fixture
strictly requires sessionSnapshot.coverage=whole_physical_source; an ID-set or
finite-descriptor snapshot cannot stand in for this proof.

The restored Core files remain owned by their author and are not copied here.
Core sealed-r1 manifest e469e598b20ee5e1dd33d9803d1b966e45b236bf1914921383acb588e0323120
was read back for the exact nested cache fields, source identity, snapshot coverage
and resource boundary. The full Project/Sort/Scan shape is explicitly eligible
for whole-source IPC consumption. This is source review, not runtime validation.
The parent-owned common client and final combined source still require independent
integration review before the eventual unified test queue.

## Recovered r3 custody corrections (source only)

Requested bare tool names remain exact request fields; git/go/powercfg terminal
identities are separately resolved to absolute executable paths and byte hashes
before ownership begins, then checked around execution and independent replay.
Cargo uses its existing held-file helper admission and fixed auxiliary policy.
Only non-measured Cargo preparation permits successful known auxiliary cleanup;
ordinary and measured processes still reject any forced termination. Unknown
helpers, failed signatures, failed reap, deadlines and output limits fail closed.

The explicit fixture producer admits the same default stack/compiler/disk policy
before creating an owner, freezes its complete JS tool closure, and clamps the
actual execute options.timeout to the remaining shared 600-second group budget.
No ignored runCargo timeout option is relied upon. Tool hashes and the actual
native timeout are rechecked against the sealed preparation on fixture loading.

Image proof requires an explicit absolute Sharp entry within a dedicated,
non-symlink node_modules deployment. Before collection the entire bounded
package tree is frozen (20,000 files, 1 GiB total, 256 MiB/file maximum), including
package versions, transitive JS and packaged native binaries. Collector and
independent replay recheck the same bytes before/after, disable Sharp caches,
record actual loaded JS/native identities and versions, and require identical
observations. External native dependencies are restricted to Windows System32
and are individually hash-bound. No image decoder is run by the launcher.
All original encoded-image and full RGBA comparisons remain mandatory.
These are source contracts, not executed acceptance results.
