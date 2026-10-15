# SQL resource accounting slice R2

Go oracle remains `3057a9666ab078270b63c7f06de748069b0d1f4b`.
R1 contract, maps, receipt and snapshot are immutable historical evidence.
R2 preserves the R1 ledger design and physical/effective provider exclusions.
All Rust, Go, Windows and performance execution remains **NOT_RUN**.

## Independent-review finding and correction

R1's cold-definition generation-conflict handling compared document references
without repeating Go's cached `read()` charges, blob verification and semantic
validation. Inspection also found Rust Metadata::put(None) accepts identical
existing documents idempotently, hiding that race from the conflict branch.

R2 adds separate `Metadata::create` and `create_controlled` methods. Both insert
only if the key is absent inside the existing SQLite immediate transaction.
Any existing key, including identical bytes, returns vault.generation_conflict.
The original put, put_controlled and put_serialized callers retain their byte-
idempotent behavior. Controlled create shares the original cancellation/deadline,
lock retry, COMMIT, rollback and connection restoration paths; errors retain
original causes. No precheck/nonce/cache-format workaround is used.

Definition publication uses strict creation. On conflict it calls the very same
cached-read helper used by initial warm loads: URL validation, four-times actual
logical retained/metadata plus ceil(bytes/65536) decode charge, eight-times Rust
retained accounting, verified blob read, and caller semantic validation. Only
then is the winning blob compared with the attempted blob. A repeated charge or
validation error therefore precedes comparison, matching the fixed Go source.

## Deterministic regressions

Five tests use a bounded loopback HTTP server. The first validation callback
publishes the competing record synchronously, forcing the subsequent production
strict insertion to conflict without scheduler timing. They cover controlled and
ordinary identical winners, second provider charge failure, changed valid winner,
and second validation failure before blob comparison. The 4-byte comment fixture
charges 40 logical retained/metadata, 2 decode, 1 request and 4 network bytes on a
successful race; Rust retained is 72 bytes. Metadata limit 35 accepts the first
20-byte load/parse then atomically rejects the next 16-byte provider charge.

Two core tests cover absent insertion, identical/different strict conflicts,
legacy idempotence, already-cancelled/expired control, cancellation at an observed
SQLite lock wait, and subsequent successful use of the same connection.
The existing batch-definition regression was corrected from stale 8× metadata
to 4× metadata, and now separately asserts 8× retained safety accounting.

The R2 map contains 14 declarations: six R1 additions, seven new R2 tests and one
corrected existing test. Declaration/static checks do not execute those tests.
