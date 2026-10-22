# Current installed-client Data expectations

The coordinator prepares fresh expectations with the explicit external fixed Go
3.2.4 reference artifact at
`29bdbdaff5f064ec212ff2064380b1ccac652eda`. This is a read-only game
input task, with a new evidence-retained owner/home and isolated user directories.
It does not build Rust/Go, install anything, operate the game, or read an existing
user workspace. Resolving definitions master and fetching its exact manifest/DBD
are explicit network preparation. No command is executed by importing the module.

From the current product checkout on Windows, supply an existing request JSON:

```json
{
  "goArtifact": "<existing fixed Go 3.2.4 reference artifact.json>",
  "installation": "<selected current Retail client directory>"
}
```

The artifact directory (`dirname(goArtifact)`) must contain the external fixed
reference layout (`dev-npm-stage/` and `lycheedev-local-3.2.4.tgz`); its package,
binary and tarball bytes are re-derived and compared with the record.

```text
node tests/rust-data-local-current/prepare.mjs --execute-main <request.json>
```

The producer runs version/help/describe before initializing its fresh home.
`target resolve` reads the current active installed build and fixes
`refs/heads/master` to the returned exact definition commit. Every subsequent
Data/asset command uses that same pin and local installation. Map's entire
verified DB2 body, all ordered IDs/rows, schema, record/page/search/foreign-key,
seven installed SQL queries, parameter SQL and raw asset results come from Go.
Raw command envelopes and capture blobs remain available; no Rust-derived value
is used as its own expected result. A changed installation stops preparation.

The shared manifest schema is `lycheedev.local-data-go-oracle.v1`. The current
consumer is `crates/lycheedev-data/tests/agent_current_local.rs`, through the
production Agent Session and DataFusion engine. Set
`LYCHEEDEV_RUST_CURRENT_DATA_FIXTURE` to the resulting `fixture.json` and
`LYCHEEDEV_RUST_DATA_AGENT_HOME` to a nonexistent home in the current owned
scratch directory. Run the already-built `agent_current_local` test executable:

```text
<agent_current_local.exe> current_installed_map_uses_production_session_and_datafusion --exact --ignored --nocapture --test-threads=1
```

Prepare that executable in the normal owned build stage when it is absent;
consumers do not invoke Cargo or the retired migration test targets. The test
initializes only its fresh home, compares the entire fixed Map row set and
schema, exercises typed queries and related records, and checks cancellation
and recovery after an invalid query. It writes `agent-current-local-report.json`
beside the fresh home. This verifies the library business path; installed-package
CLI validation remains separate.

File descriptors contain paths relative to the fixture
manifest plus byte length and SHA256. `map.rows` is `{ids, rows}` in original
ascending source order, not a sample. `map.schema` and `expected` descriptors
reference original Go `lycheedev.result.v1` envelopes, whose business output is
under `result`. `expected.installedSqlQueries` contains `{sql, response}` entries;
`probes` contains the actual `recordId`, `searchQuery`, `truncatedSearchQuery`.
Row comparisons use the complete independently decoded values and exact numeric
tokens; a matching row count alone is insufficient.

The existing owned process wrapper retains original command output/reaping
receipts. Preparation shares a 600-second deadline, 64 calls, 128MiB command-output
budget, 200 rows/page and 32 pages. Disk limits and product query budgets remain
unchanged. Failed/incomplete preparation is not an accepted fixture. Older build
fixtures and their reports retain their original identities.
