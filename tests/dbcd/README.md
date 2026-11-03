# Pinned DBCD differential adapter

Developer-only oracle, never a product runtime dependency. Build against clean
wowdev/DBCD commit `c0aeb5f6be0e9fbffc8eadb4f8b9b4e36b42af7c` using .NET 10 SDK.
The project checks the actual Git HEAD and rejects modified/untracked inputs;
the verified commit is embedded in the assembly. The comparator requires this
exact identity before examining results.

Keep matching DB2 and DBD files together, named after the table. Both adapters
record full build and SHA-256 of the actual DB2/DBD bytes. The C# adapter accepts
an optional sixth argument for one XFTH Hotfix cache and records its SHA-256.
Use `-` as the fifth argument to retain the default sample selection.

```powershell
dotnet build tests/dbcd/Oracle.csproj -c Release -p:DBCDSource=C:/fixtures/DBCD -p:TargetFrameworks=net10.0
go run ./tests/dbcd/fixture -out C:/fixtures/tables
dotnet .tmp/dbcd-oracle-bin/Release/net10.0/Oracle.dll C:/fixtures/tables CommonBoth 12.1.0.69875 .tmp/oracle.json
go run ./tests/dbcd/read -db2 C:/fixtures/tables/CommonBoth.db2 -dbd C:/fixtures/tables/CommonBoth.dbd -build 12.1.0.69875 -output .tmp/lychee.json
go run ./tests/dbcd/compare .tmp/oracle.json .tmp/lychee.json
```

Run each generated table: `CommonBoth`, `CommonOrigin`, `CommonDefault`,
`FullFields`, `Numeric`. The Common fixtures exercise logical copy IDs with
origin/target overrides and defaults. FullFields covers 301 rows; Numeric also
covers uint64 high bits, negative int64 and non-integral float32 values.

Both adapters hash **every field of every readable row**, in sorted ID and
ordinal column order. The field stream uses decimal integer tokens (no float64
conversion), float64 bits (float32 widened exactly), length-prefixed UTF-8
strings and recursive arrays. It emits one complete digest and a digest per
256 rows, so a change beyond the 200-row display sample is observable and
locatable to a block. The count, complete ID digest, encrypted IDs, input
identities and exact JSON sample numbers must also match. Use a fifth
comma-separated ID argument and the same Go `-ids` option to inspect a specific
block (at most 200 IDs). Missing requested IDs fail.

Go `-skip` is optional and must come from authenticated unavailable-partition
evidence. This adapter consumes decoded bytes; it does not decrypt BLTE or
establish missing-key provenance. Those belong to production capture handling.

For the independent Hotfix comparison, set absolute paths before running tests:

```powershell
$env:LYCHEEDEV_DOTNET = 'C:/path/to/dotnet.exe'
$env:LYCHEEDEV_DBCD_ORACLE = 'C:/repo/.tmp/dbcd-oracle-bin/Release/net10.0/Oracle.dll'
$env:LYCHEEDEV_DBCD_FIXTURES = 'C:/fixtures/tables'
$env:LYCHEEDEV_REQUIRE_DBCD = '1'
go test -p=2 -parallel=4 -count=1 -timeout=0 ./internal/records -run '^TestPinnedDBCDHotfixDifferential$'
```

`TestPinnedDBCDHotfixDifferential` runs actual XFTH bytes through DBCD and the
production Go Hotfix parser/overlay for v7, v8, v9: base→NotPublic,
Valid→NotPublic, Delete→NotPublic and NotPublic→Valid. It verifies the oracle
commit/cache digest, final row count and full-field digest. Required mode fails
when any tool/fixture path is absent. CI must require all 12 subtests rather than
accepting a parent pass or skip. The test-only Go read adapter handles base DB2;
Hotfix equivalence is checked against the production overlay in this test.

DBD selection intentionally differs from upstream: an explicit layout selects
that layout and never falls back to a build-matching but incompatible layout.
The schema package tests matching/conflicting builds, layout-only, build-only,
missing layout and ambiguity. The production reader supports WDC2–WDC5; support
for older upstream formats requires a concrete current-product input.

These synthetic fixtures verify reader semantics and comparison coverage. They
do not establish six-product real-data coverage. Historical retail CN zhCN
12.1.0.69933 SpellMisc/Spell comparisons retain their original scope; current
real-data and client coverage remains in [todo.md](../../todo.md).
