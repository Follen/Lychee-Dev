# Pinned DBCD differential adapter

Developer-only oracle, not a product runtime dependency. Use wowdev/DBCD commit
`e732093f8864240fc5884bd1bba6b02f3dfc0d56` and a .NET 10 SDK. Check out that
exact upstream commit outside tracked product source. Keep fixture DB2 and DBD
files in one directory under their table names, e.g. `SpellMisc.db2` and
`SpellMisc.dbd`. Record the full build, definition commit and input hashes.

Example PowerShell from the repository root (replace paths with explicit local
fixture and checkout paths):

```powershell
dotnet build tests/dbcd/Oracle.csproj -c Release -p:DBCDSource=C:/fixtures/DBCD -p:TargetFrameworks=net10.0
dotnet .tmp/dbcd-oracle-bin/Release/net10.0/Oracle.dll C:/fixtures/tables SpellMisc 12.1.0.69933 .tmp/oracle.json
go run ./tests/dbcd/read -db2 C:/fixtures/tables/SpellMisc.db2 -dbd C:/fixtures/tables/SpellMisc.dbd -build 12.1.0.69933 -skip 1,7,26,27,30,34,35 -output .tmp/lychee.json
go run ./tests/dbcd/compare .tmp/oracle.json .tmp/lychee.json
```

`-skip` is optional. Specify it only from authenticated missing-range/partition
evidence. The example indices apply to the documented retail fixtures, never
to arbitrary input. This adapter does not decrypt BLTE or establish missing-key
provenance; that belongs to the production reader's capture chain. It consumes
the already decoded (possibly explicitly partial) DB2 fixture.

To compare specific records, pass a fifth comma-separated ID argument to the
oracle and the same `-ids` list to the Go reader (at most 200). Both reject
unavailable requested IDs; full readable-ID and encrypted-ID checks still run.

Each adapter outputs the readable row count, SHA-256 of sorted readable IDs
(decimal plus newline), encrypted-ID sets and first 200 readable rows. Lychee
scans/decodes all readable rows. The comparator compares exact JSON numbers and
every sampled field, ignoring only the oracle's sourceCommit label. A mismatch
requires investigation of both implementations, not automatic upstream trust.

The committed C# source does not enforce checkout identity: verify the upstream
commit before building. No upstream all-DB2 test-suite pass is claimed. The
2026-09-27 run compares SpellMisc and Spell from fixed retail CN zhCN build
12.1.0.69933; see the [verification ledger](../../docs/toolkit/data-improvements-2026-09-27.md).
