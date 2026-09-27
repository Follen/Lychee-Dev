# DB2 encrypted-section recovery — 2026-09-27

Status: working-tree fix; not an npm release. No game input or process-memory
access was needed. Static DB2 and Hotfix remain separate sources.

## Cause and change

The BLTE decoder already implemented Salsa20, but the CLI supplied no key
provider. One unavailable chunk then rejected the entire DB2 file. The reader
now loads a pinned public key snapshot on demand, or uses an explicit
`--key-file <WoW.txt|keys.json>`. Offline never downloads. Private key material
is not archived; provenance retains only its source kind and document digest.

DB2 preparation can retain authenticated chunks around missing-key ranges.
Those ranges must lie entirely inside encrypted WDC partitions with matching
key identities. Shared metadata, integrity faults, provider failures and
cancellation are never downgraded to partial success. Missing-byte reads are
rejected, including string dereferences. Original partition indices/counts
remain intact. Raw asset reads/exports still require the complete file.

Partial bytes are represented by `partialContent` and `missing`, separately
from CKey-verified `content`. Queries, aggregates, streams, CSV manifests and
captures propagate incomplete coverage. `unavailablePartitions` records
physical rows and copy aliases separately; schema `rowCount` counts readable
logical IDs. A requested absent ID in a partial table is unavailable, not
proof that the record does not exist.

Real Spell row decoding also exposed an existing sparse-record check: it
rejected the zero bytes used to align records to four-byte boundaries. The
fix permits bounded zero padding only; nonzero, misaligned and excessive
trailing data still fail. Regression includes sparse copies.

## Fixed references

- Data pin: `PIN-844e56206c15b8a81da4ca386208008d855bf69b077af8b0cbcbd6adeb14657a`
- Retail / CN / zhCN / `12.1.0.69933`
- DBD: `e989e99e6f5f97c57b2e138d4d28b16066b4ee9c`
- Public TACTKeys: `71b75360752840dd62a412013a38b5a972524a50`, 983550 bytes,
  SHA-256 `6d83241ed776f9e74e27ed65d1ec52f1a1464a4e9b16bed497258fdd61020b52`
- Upstream comparison: DBCD `e732093f8864240fc5884bd1bba6b02f3dfc0d56`
  (`DBCD.IO/Readers/WDC5Reader.cs`); TACTSharp
  `a507ff7b485f7afce45e0a01dd8a15cfb2818728` (`BLTE.cs`, `Utils/KeyService.cs`).

## Real data observations

| Query | Verified observation |
| --- | --- |
| SpellMisc schema, installation and CDN | 417635 readable logical IDs; same layout `434B3607` and CKey `4204a82d7b05fe2b67a0080a9fb83f12` |
| SpellMisc rows and SQL COUNT, offline installation | Rows decode; count 417635; `complete=false` |
| Spell schema, rows and SQL COUNT, offline installation | 414027 readable logical IDs; count 414027; `complete=false` |
| Spell ID 133 | Correct readable Chinese description beginning `投出一枚火球`; incomplete table provenance retained |
| ChrClasses schema, offline installation | 15 logical IDs; `complete=true` |
| Explicit public key file, offline CLI | SpellMisc count remains 417635; `keySource.kind=file` |
| SpellMisc JSONL and SQL CSV | End frame, CSV manifest and both captures retain `complete=false` |

Both SpellMisc and Spell still lack seven partitions and 55 logical IDs.
SpellMisc has 55 physical rows and no copies there; Spell has 18 physical rows
and 37 copies. Missing key identities:
`14f4b11d7b067aa2`, `583c5b29bf208655`, `bbae9630be1c18e3`,
`bbd411bea4522afe`, `cf9022a9913b1b4d`, `fbbf041f980ce0dc`,
`fc852d42866ce038`. None is claimed decrypted. Missing sections cannot support
an exhaustive all-spells conclusion or a Hotfix overlay conclusion.

Local command artifacts are `.tmp/db2-*.json`. They retain fixed identities,
capture IDs and exact completeness; they are not shipped game data.

## Automated regression

The offline authenticated CDN fixture reproduces the original error with one
readable section and one missing-key section. It now verifies readable rows,
schema, paging, SQL aggregation, JSONL end, CSV manifest/captures and strict raw
asset failure. An independent Salsa20 ciphertext plus explicit key file restores
both rows and the complete CKey check. Corrupted chunks, inaccessible metadata,
mismatched section keys, failed providers and cancellation still fail.
Key-provider tests cover text/JSON input, immutable request snapshots, invalid
documents and uncached offline behavior. Sparse tests cover valid alignment
and reject nonzero, excessive and misaligned padding.

Final validation passed:

- `go build ./...` and `go vet ./...`.
- `LYCHEEDEV_REQUIRE_LUA51=1 go test -count=1 ./...` using Lua 5.1;
  final log `.tmp/db2-final-tests.log` (all packages passed).
- 46 tooling Node tests and 5 npm launcher tests (51 total).
- Skill validation and command contract: 80 commands, 192 references,
  zero violations; generated command reference includes the new key flag.
- `git diff --check`.
