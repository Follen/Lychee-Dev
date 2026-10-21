# Current installed-client Data expectations

The coordinator prepares fresh expectations with the authenticated Go 3.2.4
package at `29bdbdaff5f064ec212ff2064380b1ccac652eda`. This is a read-only game
input task, with a new evidence-retained owner/home and isolated user directories.
It does not build Rust/Go, install anything, operate the game, or read an existing
user workspace. Resolving definitions master and fetching its exact manifest/DBD
are explicit network preparation. No command is executed by importing the module.

From the current product checkout on Windows, supply an existing request JSON:

```json
{
  "goArtifact": "<existing Go324 artifact.json>",
  "installation": "<selected current Retail client directory>",
  "luaLSArchive": "<retained pinned LuaLS ZIP for package authentication>"
}
```

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

The shared manifest schema is `lycheedev.local-data-go-oracle.v1`. Set
`LYCHEEDEV_RUST_CURRENT_DATA_FIXTURE` to the resulting `fixture.json` for the four
explicit Rust business tests. Each exact case must receive its own nonexistent
`LYCHEEDEV_RUST_DATA_CLI_HOME`; consumers initialize/seed only that new home and
read the shared fixture without modifying it. The installed-reader case writes
to `LYCHEEDEV_RUST_DATA_REPORT`, with its decoded output beside that new report.
File descriptors contain paths relative to that
manifest plus byte length and SHA256. `map.rows` is `{ids, rows}` in original
ascending source order, not a sample. `map.schema` and `expected` descriptors
reference original Go `lycheedev.result.v1` envelopes, whose business output is
under `result`. `expected.installedSqlQueries` contains `{sql, response}` entries;
`probes` contains the actual `recordId`, `searchQuery`, `truncatedSearchQuery`.
Stream tests compare their frames against these complete independently decoded
rows. They do not substitute a count-only check or an invented Go stream result.

The existing owned process wrapper retains original command output/reaping
receipts. Preparation shares a 600-second deadline, 64 calls, 128MiB command-output
budget, 200 rows/page and 32 pages. Disk limits and product query budgets remain
unchanged. Failed/incomplete preparation is not an accepted fixture. Older build
fixtures and their reports retain their original identities.
