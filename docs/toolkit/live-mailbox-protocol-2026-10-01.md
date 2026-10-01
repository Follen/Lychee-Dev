# Native Mailbox protocol (3.1.0 candidate)

The owner requested one complete protocol cutover, with no old wire compatibility
and with identity, challenge and execution validation retained. This contract
supersedes native CON memory discovery in the September 28/29 contracts. Receiver
OP/BTP documents remain historical; their keys and ACK rules are not native input.

## One generation

| Surface | Required value |
| --- | --- |
| CLI and managed addon release | 3.1.0 |
| Binary header / trailer | LYCMEM06 / LYCEND06 |
| Slot envelope / receipt | lycheedev.slot.v3 |
| Identity descriptor | lycheedev.slot.identity.v2 |
| Input observation | lycheedev.input.v2 |
| Slot addon transport | memory-slot-v3 |
| Public publication | lycheedev.mailbox.v1 |
| Runtime replacement proof | lycheedev.runtime-replacement.mailbox.v1 |

The optical pairing remains `lycheedev.input.hybrid.v1`: its encoding did not
change. It accompanies a v2 memory input observation and does not replace it.
Old slot envelopes, native identities, input records and connection state are
rejected. The installer may replace an inert managed installation using its
old installation receipt; that is filesystem replacement, not execution of an
old protocol. Existing unresolved ownership is preserved, never silently erased.

## Published data

`LycheeDevInternal.Mailbox` contains `schema`, `release`, `runtime`, `identity`,
`input`, `receipts[nonce]`, and `bodies[ticket]`. Values are immutable binary
records, not decoded execution state. The receipt/body maps are independent
mirrors of the private protocol state. They expose no executable callbacks.

Private state still authorizes every operation: actor/runtime/owner/fence,
fresh nonce, prepared nonce, generated challenge, commit identity and exact
result digest. Modifying the public table cannot authorize execution or release.
The CLI requires a validated HEAD before reading a BODY, with the exact nonce,
runtime, ticket, length and checksum. Header/trailer integrity is corruption
detection, not execution authorization.

The existing retention bounds remain 200 receipts and 46 operations. Successful
release removes that task's BODY; input stop/hide clears the current input field.
Reload replaces the publication table and runtime. Explicit bridge-off creates
no new mailbox or input sampling work.

## Reading the current publication

The host obtains module base and executable hash from the selected OS process.
Its module read capability seals the OS-inventoried executable range and a
bounded plan of the actual `MEM_IMAGE` or `MEM_MAPPED` segments within that range.
WoW's current main module uses multiple mapped allocations. Every module read
must be covered by both the original issued eligible segment and the current
mapping before and after IO. Each current query starts at the actual requested
location, rather than an old run origin outside that location. Allocation,
mapping type, committed state and exact protection must match the original
eligible segment. Coverage is composed from each requested/issued/current
intersection, advancing only to their smallest end. VirtualQuery run boundaries
are page geometry, not object identity: changes before or after the requested
bytes do not alone invalidate those bytes. Changes within the request are
checked at the next piece and rejected if eligibility or attributes differ.
An expanded current run cannot authorize an originally ineligible hole or extend
the issued range. Newly readable holes, foreign modules and fabricated metadata
grant no access. Ordinary heap reads
remain private-only.
The mapping inventory has its own bound: at most one region per system page
inside an OS-inventoried module of at most 512 MiB. This is separate from the
4,096 loader module-entry limit. It validates aligned, strictly advancing
geometry and searches only the sealed ordered mapping plan with binary search.
The same read-locked file handle supplies both the executable SHA-256 and its
standard PE section layout. An opaque layout capability binds those sections to
the hash and module size. Runtime MZ/NT signatures, table-location fields and the
complete section table must match that file and remain unchanged through root
resolution. Other COFF/optional-header fields can be transformed by the loader;
they never provide section bounds. There is no repaired header or runtime-only
layout fallback.
The current researched binding is Retail 12.1.0.69933, exe SHA256
`d41f11de411f6fdb280a7c1c6380ba3d1b17614f03e5aca8fabce2cc715cd7dd`, Lua root
RVA `0x79c0c18`. A second independently researched recipe follows Forever's
RunScript state load: exact build 1.60.1.70124, executable SHA-256
`3d2fbfb0a20567097fa9cedbeb55a8fed895cead6c1ff86103c89ee96f5ff58f`,
anchor `0x75a69d`, root `0x7b780b8`. Its fast path verifies all 49 original
runtime code bytes and the derived root, not a bare numeric RVA. This bounded
semantic site was readable even though other text pages were protected; it does
not assert unique coverage of those pages. Known hashes verify their complete
runtime anchor first. Other executable hashes require exactly one match across
both the 58-byte and 49-byte recipes over the complete runtime `.text` section
(at most 128 MiB). Matching both recipes or multiple sites is ambiguous, and a
shorter match crossing a chunk boundary must be counted once. All RIP global
targets must be readable/writable non-executable image data; call targets must
be executable text. Missing/ambiguous matches, gaps, invalid targets or changed
metadata/anchor/root guards fail closed. A derived root must still pass the
complete Lua publication checks; no old numeric RVA or unverified layout is
inherited. Synthetic relocation and old-dump replay are not real cross-build
acceptance.

Read path: module base + RVA → current Lua state → globals →
`LycheeDevInternal` → `Mailbox` → named field/map key → immutable wire record.
Hash lookup follows bounded Lua table collision links. Every lookup re-resolves
heap pointers and rechecks the selected path, type/secret bits, table node
layout, key bytes, current value, schema/release/runtime and OS process identity.
The read budget is 512 calls and 1 MiB, including guards. No cached heap address
or hints participate, including `--no-cache`.

Coverage means completion of this selected named-field lookup, not coverage of
the client's heap. Missing publication cannot prove absence of unrelated data.
Layout/path changes and malformed records fail closed; they do not invoke a
scanner. Before physical input the CLI resolves the current input field again,
requires the same qualified sample/address, checks freshness, optical pairing,
selected target/ownership and the exact slot publication.

The current INPUT may also contain `inputAttempt`, a latest-only diagnostic
snapshot (`lycheedev.input-attempt.v1`). A private monotonic callback counter,
runtime, slot, fixed stage/reason and Receive-entry flag explain whether a
callback was observed and where it stopped. The snapshot is detached from
private state; public mutation cannot advance its counter. Disabled mode
publishes none and adds no events, hooks or timer. Optional malformed diagnostic
data is ignored without hiding the main input's readiness error.
After an exact receipt miss the host permits at most one additional named INPUT
lookup per exchange per invocation, within the ordinary read/trace budget. It
may journal a fresh matching-route counter increment, but it cannot advance a
transaction, authorize input, acknowledge execution or replay an uncertain key.
No diagnostic increment does not prove non-delivery, and a callback observed
on the same route does not prove the host key caused it. The callback cannot
know a nonce before loading the corresponding envelope.

Protocol `Snapshot` exposes only bounded detached facts: at most 200 receipts
and consumed bits and 46 operation metadata objects. It never returns execution
functions, callbacks, source code or writable private table aliases. A prepared
envelope is copied into private state before compilation and observation;
later mutation of the caller's table or a snapshot cannot change its challenge,
prepared nonce, target or execution budget. Immutable wire strings can be
shared. These boundaries do not sandbox other addons sharing the Lua runtime.

Runtime retirement requires two current input publications from one new runtime,
fresh at observation time, with increasing sample and sequence and matching
current identity. Retained immutable strings, arbitrary addresses and discovery
alone cannot establish replacement. No region snapshots are used.

## Installation and acceptance

Install from the matching development/release root through `addon install`.
Activate using the journaled native CLI route on an explicitly selected clean
installation and PID. First installation may use its fixed reload sequence;
this does not decode old wire records or replay unknown business input.

Required verification includes disabled/enabled Lua fixtures, public mutation
negative tests, Go read-path/migration/ownership tests, mandatory Lua full tests,
normal and large real-client requests, read-only retry, cache-off, reload and
disconnect. Record actual evidence and `not_run` separately. A fixture pass is
not real-client acceptance. The wowdump root/complete-path recipes are immutable;
save the new complete Mailbox path as a new recipe, with independent read,
semantic and reuse confirmations.
