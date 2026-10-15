# SQL resource accounting slice R1

Oracle: Go 3.2.0 commit `3057a9666ab078270b63c7f06de748069b0d1f4b`.
Status: implemented candidate; Rust/Go/Windows/performance execution **NOT_RUN**.
This does not revise or invalidate the immutable semantic SQL R2 receipt.

## Ledger contract

`resources` is the logical lifetime usage snapshot after SQL and authenticated
source postcheck, before public-number preparation, evidence serialization,
request hashing, reference verification and capture commit. Publication failure
still returns an error and cannot publish a success result. The additional
`resourceAccounting` diagnostic remains an incomplete conservative Rust ledger,
not RSS and not the Go usage object. Its `logicalProviderParityComplete` is true
only for source-free/metadata-only queries; physical/effective providers remain
unaudited, not silently claimed equivalent.

The two ledgers share a mutex and admit split charges atomically. Both preserve
512 MiB retained, 128 MiB metadata, 100M decode work, 4096 requests and 1 GiB network
limits. Safety-only views share the same underlying safety ledger, cancellation,
deadline and ownership. A logical release cannot release Rust allocations.
Temporary Rust reservations release only their exact safety-retained ownership;
Go logical HTTP scratch is independently held/released and excluded from usage.

Definitions charge Go's four times actual bytes to logical retained/metadata and
ceil(bytes/65536) decode work. Rust keeps eight times actual bytes as conservative
retained admission; the redundant second four times bytes is no longer mislabeled
as metadata. Cold loads account network attempts/bytes and hold Go's 8 MiB logical
HTTP scratch until actual-byte admission completes. Semantic parser byte and
fixed row charges match Go; extra cloned strings remain safety retained charges.
Distinct URLs sharing identical blobs still incur distinct provider charges.

SQL reserves 64 MiB logical retained once before providers. The Rust executor
still enforces its independent 64 MiB Work limit and charges actual allocations
and work to the shared safety view, without charging the logical allowance twice
or classifying executor operations as Go decode work. The safety ledger does not
precharge another 64 MiB in addition to actual allocations. A safety failure can
still reject a logically admitted operation; neither ledger claims actual RSS.

## Charge-site audit matrix

| Callsite family | Logical treatment | Safety treatment / status |
| --- | --- | --- |
| SQL request::read/admit_encoded; compile estimate | excluded by safety view | original bounded charges unchanged |
| definitions::load cached | 4× bytes + ceil/65536 | 8× retained; 4× metadata |
| definitions::load HTTP | request/actual network; 8 MiB transient scratch; 4× bytes + ceil/65536 | incremental 8× retained, actual network, 4× metadata |
| semantic parse text/mapping/value | same Go byte/256/160 estimates | shared cap and cancellation |
| semantic output row | Go 256/256/1 | 256 + owned strings retained |
| SQL execute_budgeted / relational Work | excluded, prepaid 64 MiB logical | original 64 MiB execution and 100M shared safety work limits |
| public_query_numbers, semantic/effective report serialization | excluded by safety view after snapshot | unchanged bounded serialization admission |
| capture bytes/request hash/reference verification | excluded by safety view after snapshot | original producer budget and cancellation retained |
| CASC/Encoding/cache/WDC/effective provider charges | default mirrored, NOT parity verified | existing limits remain active |
| other Budget users / charge_once | default mirrored | unchanged safety admission; no global safety bypass |

Physical providers can have different Go allocation/work estimates and snapshot
scope. The new `resources` field for those paths is **not yet exact Go parity**.
No CSV/EXPLAIN or error taxonomy expansion is included. No claim of full module
completion, executable success, exact memory measurement or performance gain.
