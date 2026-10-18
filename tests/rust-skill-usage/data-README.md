# Offline packaged-Skill Data workflow (new revision v2)

This is a thin, opt-in companion to the unchanged default Source workflow.
It exercises the **actual npm-installed packaged Skill launcher** with two
fresh, synthetic, authenticated local CASC inputs. It is not a new product
command, a general test framework, a live-client result or a performance gate.

The previous unpushed files were lost. These are newly implemented bytes and
must receive fresh review and tests. Historical Node/Windows results are not
inherited, relabeled or edited. The fixed semantic oracle remains Go 3.2.3
commit `3766f9cacb52e93a803a626187e2a1d54a445468`, full 23-file Skill tree
`b9e162ace7abe7ea4a2feecef822283767eb1210`.

## Formal entry and prerequisites

Only the single owner-coordinated Windows queue runs the following commands.
Node 24 with `node:sqlite`, installed npm, Windows amd64, the existing managed
Cargo cache/lease and ordinary Windows Job runner are required. The cloud
checker tests need Node 24 and installed npm but never execute a product EXE.

```powershell
node --test tests/rust-skill-usage/runner.test.mjs tests/rust-skill-usage/data-runner.test.mjs
node tools/rust-skill-usage.mjs --prepare-data-exporters
node tools/rust-skill-usage.mjs --data <current-artifact.json> <current-candidate-report.json> <fresh-exporter-buildEvidence-receipt.json>
```

The three angle-bracket paths are explicit paths returned by the current
owned preparations, not historical local paths to guess or reuse blindly.
The artifact and candidate must bind the current `sourceDigest`, identical
native EXE, release manifest and full package inventory. The exporter build
must have that same source identity. If a fixture is absent, the Data runner
creates it under its own new owner. It never adopts a previous fixture root,
workspace SQLite database, cache or capture.

`--prepare-data-exporters` is an explicit build-only preparation entry. It
uses the existing `runCargo`, one-shot mode and ordinary `target` lease:

```text
cargo test --locked --offline -j 1 -p lycheedev-data --test catalog_navigation_parity --test skill_data_fixture --no-run --message-format=json
```

Its fresh sealed build receipt authenticates the compiler stdout, exact two
integration-test target source paths, same-invocation executable bytes/SHA and
`skill-data-exporters.json`. That manifest is bound in the invocation's
`command.json` before sealing. Existing build receipts are never amended.
Later cache overwrites, another target, stale source, unsealed or failed owner
are rejected. The `--data` entry itself does not compile or fetch.

## Real launch and ownership

`data-owned.mjs` uses `beginScratch` and one existing 20-minute owned Windows
Job. The 19-minute controller has a maximum of 80 child starts, 2 minutes per
child, 12 MiB per stdout/stderr buffer and 128 MiB total recorded output.
After a process failure no dependent child is started. Failed or unreaped
runs retain scratch. Success first seals all evidence and reaps the tree,
then retires only its own scratch.

The isolated npm invocation uses separate empty `user.npmrc` and
`global.npmrc`, its own cache and prefix, `--offline --ignore-scripts`, and
no audit/fund/update request. It packs the fixed candidate, verifies tgz
SHA1/SHA512 against npm's result, and performs an actual global install to the
owned prefix. Every installed package file must match the verified candidate.
The npm-generated `lycheedev.cmd` is archived. No marker shim is fabricated.

Every public call uses the installed
`payload/skill/scripts/lycheedev.mjs`; that Skill discovers the npm prefix and
hands off through `bin/lycheedev.mjs` to the exact native EXE. Inherited
`LYCHEEDEV_*`, npm overrides, Node options/path, HOME and temporary directories
are isolated. `LYCHEEDEV_BIN` is never used. Source Skill behavior and public
product CLI remain unchanged.

## Inputs and selected calls

Both inputs use retail / cn / zhCN / `12.1.0.69933`, definitions commit
`bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb`.

1. `prepare_catalog_public_fixture` exports Item, ItemSparse and HouseDecor
   under `<new-owner>/catalog-fixture`.
2. Dedicated ignored test `prepare_skill_partial_fixture` verifies the active
   `skill-data-usage` owner and exact caller-owned, absent
   `<new-owner>/partial-fixture`. It invokes the shared
   `encrypted_sections(201)` / `encrypted_frame` factory. It exports original
   Sample bytes and the authenticated descriptor, not a new product consumer.
   The shared factory refuses any existing file/directory/symlink before
   writing and does not delete non-owned roots.

The existing `loadDataFixture` is reused for catalog authentication;
`seedDataHome` is reused for both fresh initialized homes. Only the exact PIN
and definition documents plus their content-addressed blobs are inserted.
No SQLite file, decoded cache or previous CAP is copied into the tested home.
All fixture bytes, the original descriptor, read-only seed SQLite and the
CASC archive/index are independently archived. The bounded reader checks
PIN identity, DBD blob hashes, EKey/index/chunk hashes, config MD5, Encoding
page MD5 and CKey/EKey mappings, Root mapping and complete selected bytes.
Encrypted Sample is selected by its authenticated EKey. Header length 60 is
not a type discriminator: the normal Encoding object also has that header.

The public sequence includes version, describe, help, SQL help; actual init;
actual target resolve/show; then Data calls through the same installed Skill.
HouseDecor is never replaced by Sample:

```sql
SELECT ID, Name_lang, ModelFileDataID FROM HouseDecor
WHERE ID IN (:third,:first,:duplicate,:second) ORDER BY ID
```

JSON parameters are `third=30, first=10, duplicate=10, second=20`. Exact rows:

```json
[[10,"无模型🙂",0],[20,"第二",501],[30,"第三",501]]
```

The unordered companion preserves 30,10,20. EXPLAIN ANALYZE must report the
actual normalized table `{catalog:"static", name:"housedecor"}` and exactly
3 calls, 3 lookups, 3 input rows and 3 output rows. CSV is exactly 76 UTF-8
bytes, SHA256
`b304aa35231bb210d013f57cbd9e4e70b18a74f6f5853377db9c704ed3c8bf5e`.

The independent Sample fixture has clear ID1/Value42 and encrypted ID2/Value99.
With an unrelated synthetic key file, schema/SQL/CSV must preserve the one
readable row, missing-key and unavailable-partition proof, placeholder blob,
`partial=true`, `complete=false` and incomplete captures. Replacing only that
key file restores two rows in the same home and PIN. A new target show, schema,
SQL and CSV must be complete and omit stale partial/missing/partition fields.
The synthetic key/vector is fixed test material, not a user credential.

## Exact accounting and capture custody

A full successful run has **26 checks, 42 recorded children, 38 public CLI
calls and 16 captures**. Children comprise 2 exporter test executables, 2 npm
commands, 22 public discovery/init/target/Data calls and 16 public
`evidence verify` calls. Capture sources are:

- catalog target resolve: 1
- catalog schema, ordered JSON, unordered JSON, EXPLAIN ANALYZE: 4
- catalog CSV query capture + CSV capture: 2
- Sample target resolve: 1
- partial Sample schema, SQL, CSV query + CSV: 4
- restored Sample schema, SQL, CSV query + CSV: 4

Every returned capture is verified by the real public `evidence verify` call.
Its public result is the CAP itself; the internal library's
`{capture, verified:true}` wrapper is not the CLI result and is rejected.
Independent verification preserves and rechecks:

- canonical CAP identity and complete/truncated/provenance fields
- actual SQLite capture metadata key/generation/full raw value
- full content-addressed capture bytes and source blobs
- exact JSON result/query, sorted parameter request digest and PIN
- CSV bytes, sidecar manifest, own queryCapture/csvCapture and dependency edge
- complete target capture, catalog digest and actual published config blobs
- original queries, both key-file states, fresh workspace identity and seed docs

No `metadataVerified=true` flag substitutes for the archived metadata bytes.
The source/placeholder blobs remain replayable after successful home cleanup.

All observed logical resource dimensions must be nonnegative exact integers,
with network bytes/requests zero. Actual `windows_adaptive` memoryBudget must
have a positive admitted concurrency, observed ceiling, the current initial
retained/metadata minima, zero growth for these tiny inputs, and usage plus
peak scratch within its limit. Whole-query accounting stays explicitly
incomplete. This does not measure packet traffic, RSS, peak OS memory or
real OS Job memory-pressure rejection/release.

## Read-only evidence replay and status boundaries

`assertDataReport(report, workflow, {evidenceDirectory})` accepts an explicit
read-only relocated archive directory. It opens only that directory's archived
files while preserving the original Windows Node, npm, launcher, home,
fixture, output and command paths from the report. Command reconstruction uses
original Windows path semantics, never the replay host's `process.execPath`.
All 42 commands and stdout/stderr identities, fixture manifest inventory,
workspace/seed inputs, all 16 CAPs and every verify result are rechecked.
The outer live owner still requires exact equality with its original config
and current source/package/native/npm/test inputs before sealing success.

A standalone replay proves archive consistency; native acceptance additionally
requires that run's actual sealed owned-process receipt and matching outer
report. Synthetic unit-test reports are checker fixtures only.

Only a fully successful independently replayed run promotes
`g323-data-query`. `completeSkillAcceptance` always remains false. The two
Live increments and `g323-data-pressure-recovery` remain NOT_RUN, with their
original requirements. No pressure helper, general runner extension, new
Windows FFI, mocked native pressure, game interaction or historical report
promotion is included. Actual Windows packaged execution and Rust compilation
must remain NOT_RUN until the single authorized queue runs these exact bytes.

## Fixed runner with an unchanged sealed product

The Data runner can execute from a different fixed runner checkout while its current working directory remains the clean product source witness. This is intended for runner-only repairs: never authenticate an older EXE against a newer product checkout.

From the verified product checkout, use the absolute path of the selected runner entry:

```text
node <fixed-runner-root>/tools/rust-skill-usage.mjs --data <product-artifact.json> <product-candidate-report.json> <same-product-exporter-buildEvidence-receipt.json>
```

For the original0002 product, retain its already sealed artifact, candidate and exporter receipt. The runner still verifies their sourceDigest, complete package inventory, EXE, recipe and upstream identities. Shared build/identity/ownership tools must match the product witness byte-for-byte; runner inputs are separately hashed and recorded. Do not copy a later product's crate files into that witness. Each invocation gets a new owner and new fixtures; the original failed owner is unchanged.

The installed Skill is authenticated against the complete reviewed Rust-candidate25-file tree in `baselines/rust-candidate-skill.json`, including Git-blob SHA and SHA256 for each file. The fixed Go23-file baseline remains independent:15 originals are unchanged,8 files contain documented engine/candidate adaptations,2 candidate references are added. Full installed and archived bytes, inventory and the unchanged command contract are checked; neither arbitrary candidate modifications nor merely self-consistent changed hashes are accepted. Node checker success does not substitute for actual installed CLI, OS-pressure or full Skill acceptance.
