# Installed Skill Agent Data workflow

This is the current opt-in Data companion to the Source workflow. It exercises
the actual npm-installed launcher and supervised `data agent serve` protocol,
not the retired SQL CLI. Reconstruction has new source identities. No prior
Node/native result, old `skill-data-usage.v2` report or lost candidate is a pass.
All new declarations remain NOT_RUN until unified authorization.

## Explicit preparation and execution

Windows amd64, Node 24 with `node:sqlite`, npm, the ordinary managed Cargo
channel and existing owned Job supervisor are prerequisites. A matching current
Release artifact and candidate package must already exist. No importer build or
fixture preparation is hidden inside a measurement.

After the final unified gate authorizes execution:

```text
node --test tests/rust-skill-usage/agent-client.test.mjs tests/rust-skill-usage/agent-data-contract.test.mjs
node tools/rust-skill-usage.mjs --prepare-data-exporters
node tools/rust-skill-usage.mjs --data <artifact.json> <candidate-report.json> <exporter-buildEvidence-receipt.json>
```

The explicit exporter preparation uses the existing runner, default stack and
ordinary channel, `--locked --offline -j 1`, **no one-shot**, and three targets:
`catalog_navigation_parity`, `skill_data_fixture`, `agent_fixture_import`, with
`--features migration-oracle --no-run --message-format=json`. The legacy feature
is restricted to authenticated fixture construction/comparison; it is forbidden
in the production Release artifact. Every target source, compiler artifact,
executable bytes and same-invocation build receipt are bound. Missing/stale
executables fail; the Data workflow never compiles or fetches implicitly.

The shared ignored importer uses production pin selection, blob publication and
DataCacheMetadata strict creation. It receives original authenticated DBD cache
documents and original blob paths, not a copied SQLite/cache image. Shared pin
metadata and private `state/data` definition metadata are inspected separately.

## Actual business sequence

The outer owner has 600 seconds; its controller has 570 seconds. Installed Node,
npm, package inventory, native executable, Skill bytes, fixtures, source and full
assertion/tool closure are bound before and after. Parent and controller use the
same sanitized actual environment; wrapper-only PowerShell module/policy values
are omitted, not changed as system settings. Success requires native tree-reap
receipts, never a child PID or JSON `passed` field.

Metadata calls include version, describe, bounded help, exact Agent command
schema, search and authoritative Data schema. Every call observes both explicit
and default home paths before and after, proving it did not create a workspace.

Three fresh supervised sessions cover:

- HouseDecor: authenticated Fields, typed sorted projection, exact three rows,
  full Page and quoted-CRLF CSV Export, explicit complete CAP, Release and Close.
  Actual verbose Analyze returns its own handle and four native metric rows,
  including Output Rows = 3; echoing `analyze:true` is not execution evidence.
- Partial Sample: original encrypted section remains unavailable, one exact
  row and complete=false; missing interval and partition identity are retained.
- Restored Sample: only the admitted key file changes; the same pin/home now
  yields both exact rows and complete=true, without stale missing metadata.

Query is typed, not SQL. Query does not implicitly publish CAP: the explicit
Capture task captures the **full owned result**, not just the latest page.
Domains have their separately defined automatic CAP behavior. Agent JSONL is one
matching-ID response per request, not the old begin/record/end result envelope.

## Independent evidence

Retain original JSONL frames and request IDs, private Data CAP metadata, full
CAP blob, original fixture catalog/CASC objects and DBD blobs. Replay verifies
all ordered values, exact wide numeric tokens, original typed request digest,
CAP identity, completeness and source proofs. It independently reconstructs the
entire fixed WDC3 layout, including storage/placement/partition metadata, from
authenticated raw fixture bytes. It does not trust duplicate returned layout
objects or self-reported verification flags.

Source identity hashing preserves real Rust struct field order and nested Value
serialization order. It is not a new globally sorted JSON canonicalization.
The fixture request digest helper is explicitly limited to its safe-integer,
ASCII-key request domain; it is not a generic floating-point/large-integer
JavaScript serializer for arbitrary typed requests.

`provenance.resources` binds actual applicable limits and logical admission
ledgers, with local network counters zero. It is not RSS, OS peak or proof of
whole native allocator accounting. Analyze binds full-request timing/work
scope, excludes transport/subsequent handle operations, and checks actual DF
output rows. It does not independently prove every metric inside plan text.

Only `assertAgentDataReport` plus the outer owner and sealed receipt chain may
qualify this workflow. Standalone archive consistency is not native acceptance.
The fixed Go323 semantic baseline and earlier 0002/6187/3ae Skill/report
identities remain separate. Old SQL report checkers remain historical helpers,
not current product runners. Original historical documentation is available at
base b42, not silently relabeled as this protocol.

Passing selected Data tasks does not set completeSkillAcceptance=true. Live game
operations require separate authorization and actual INPUT/CON evidence; true
memory-pressure recovery requires its own real observation. Performance and
blind trials remain after correctness, never inferred from this fixture run.
