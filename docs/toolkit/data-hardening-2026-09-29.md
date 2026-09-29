# Data recovery hardening — 2026-09-29

Working-tree changes after the [Forever investigation](forever-data-recovery-2026-09-29.md).
No npm publication, global installation replacement or game input. The original
`mewo` project lock remains unchanged.

## Fixed contracts

1. Reused publishing slots pass the unified product/build-series guard during
   discovery, remote resolution and pinned reads. A foreign series in the Forever
   slot is rejected before downloading configurations or creating a pin. The
   guard currently accepts the verified 1.60.1 series; a new series needs an
   explicit catalog update. Stable product endpoints retain their existing policy.
2. Static data commands have one total deadline covering preparation, I/O,
   recovery and execution: 300 seconds by default, configurable with
   `--timeout-seconds <1-3600>`. A shorter caller deadline wins. Timeout is exit 5,
   `command.deadline`; it is not an empty successful result. Hotfix keeps its
   separate scan/request budgets and checkpoints.
3. The selected product/region's version-service `cdns` manifest supplies delivery
   hosts and paths. Cached routes are delivery hints. Availability failure can
   refresh that manifest once per prepared source, including configuration reads.
   The same BuildConfig, CDNConfig and content keys are retained; `versions` is
   not consulted for recovery. Offline mode never refreshes. A changed path uses
   a separate fragment-cache namespace; concurrent route writes use generations.
4. Content extraction tries other indexed encodings after HTTP/missing-object
   failures. All candidates still pass normal content verification. Cancellation,
   deadlines, malformed data, integrity and budget failures stop immediately.
   Exhaustion preserves remote/local source and HTTP uncertainty.
5. Availability exposes `totalRows` before its output bound and `truncated`.
   Rejected foreign-series rows contribute to rejection/error reporting and are
   never advertised as another product's valid builds.

These are CLI contracts for humans, scripts and agents alike. Skill guidance
documents the behavior; it does not implement recovery outside the program.

## Regression evidence

Red-before-fix tests reproduced foreign-slot acceptance, invisible truncation,
missing timeout flags, stale-route failure, premature encoding failure and
remote errors mislabeled as local. Added boundary checks cover:

- Remote identity rejection before config download and pin publication.
- Route refresh at config/content stages, changed host/path, one refresh maximum,
  persisted route reuse, offline no-network behavior, invalid ranges and deadlines.
- HTTP-to-valid alternative encoding, all remote copies missing, retained HTTP
  uncertainty, corruption and cancellation stopping after the first candidate.
- Whole CLI timeout while waiting on an owned metadata lease, no success result,
  and successful reuse after releasing the lease.
- Actual US discovery, Forever enUS Claw name search and three effect records
  through the rebuilt candidate, with fixed snapshots and partial coverage.

Targeted log: `.tmp/forever-hardening-targeted.log`.
Full Go/Lua log: `.tmp/forever-hardening-full.log`.
`go build ./...`, `go vet ./...` and the full
`LYCHEEDEV_REQUIRE_LUA51=1 go test -count=1 ./...` completed successfully.
Generated command reference, skill contract (87 commands, 212 references),
skill validation, two Node contract-tool tests, version consistency and
`git diff --check` passed.
Candidate: `.tmp/lycheedev-data-hardened.exe`.
Real-command envelopes: `.tmp/forever-hardening-{availability,name-search,effect}.json`.

## DBCD differential comparison

Oracle: `wowdev/DBCD@e732093f8864240fc5884bd1bba6b02f3dfc0d56`, rebuilt with
.NET 10. All 71 upstream C# source files match that commit's archive. Input
DB2/DBD SHA-256 values are checked against the earlier captured provenance;
missing partitions come from authenticated source evidence, not guessed zeros.

| Product/build | Table | Readable logical IDs | Result |
| --- | --- | ---: | --- |
| Forever 1.60.1.70009 | SpellEffect | 42,365 | Match |
| Forever 1.60.1.70009 | SpellName | 31,716 | Match |
| Retail 12.1.0.69933 | Spell | 414,027 | Match |
| Retail 12.1.0.69933 | SpellMisc | 417,635 | Match |

For every table, the complete readable-ID digest, encrypted-ID sets and every
field of the first 200 readable rows match. Additional targeted comparisons
match three Claw effect records and four Claw name/rank records. Both readers
reject SpellName 1313392, which belongs to an unavailable encrypted partition.

Reproduction script: `.tmp/run-forever-hardening-dbcd.py`; result manifest with
input hashes and case outcomes: `.tmp/forever-hardening-dbcd-results.json`.
The shipping CLI gains no .NET dependency. This is a four-table, two-build
comparison, not verification of every DB2 format or unavailable encrypted data.
Static values do not verify server-side damage coefficients.
