# Execute source byte contract candidate

Oracle: Go v3.2.0 `3057a9666ab078270b63c7f06de748069b0d1f4b`.
All Rust, compilation, and Windows checks in this batch are **NOT_RUN**.
The referenced Go source was inspected; no Go product execution is claimed.

## API and ownership

`connection::ExecuteRequest.code` is now `&[u8]`. Public CLI source/probe/file
bytes must be passed unchanged. Never call `from_utf8_lossy` before request
lookup or admission. The CLI owner applies its own adapter; this batch does not
edit CLI files, activation/doctor files, or the typed public JSON writer.

Only `Operation.code` and shared `mailbox::SlotEnvelope.code` use the new
`CodeBytes(Vec<u8>)` value. Identities, tokens, request keys and other strings
retain their existing types and validation. `CodeBytes` has exact-byte equality,
`as_bytes`, `into_bytes`, `len`, `is_empty`, `clear`, `push_byte`, and conversions
from `Vec<u8>`, `&[u8]`, `String`, and `&str`. Its JSON representation remains a
string. No new wire field, digest, command or protocol is introduced.

Driver's `prepare_request` and `historical_request` accept `impl AsRef<[u8]>`;
existing UTF-8 call sites remain source-compatible. `Operation::prepared` retains
its existing String signature, delegating to the new `prepared_bytes` constructor.
Only three existing core test lines and one Delivery test line require mechanical
API adaptation. Those are owned and frozen; their original boundary assertions
are retained. Publication production code is unchanged.

## Fixed-Go stages

1. Command `[]byte` to Go `string` preserves all bytes. `PrepareRequest` checks
   the original 1..262144-byte size and rejects initial byte 27, with the same
   budget/policy rules. Both historical request checks compare exact bytes.
2. In-memory State/Operation and the actual Driver-created prepare envelope keep
   those bytes. `bridge.SlotPayload` measures/checksums them and decimal-escapes
   every non-printable/non-ASCII byte for Lua 5.1. It never converts through UTF-8.
3. `channel/artifacts.go:snapshot` JSON-marshals and unmarshals a COPY before
   extracting artifacts. Go JSON encoding replaces each invalid UTF-8 byte with
   U+FFFD; a truncated two-byte prefix produces TWO replacements. Rust's
   `from_utf8_lossy` is unsuitable because it groups some invalid prefixes.
   `CodeBytes` serialization performs the per-byte Go replacement only at this
   JSON boundary. Valid U+FFFD stays valid U+FFFD; invalid bytes are emitted as
   `\ufffd`. The original in-memory bytes do not change.
4. The artifact hashes and content describe the JSON-cloned bytes. Hydration
   assigns authenticated artifact bytes directly to `CodeBytes`, matching Go's
   `string(data)`; it does not reject or normalize non-UTF8 artifacts.
5. A later invocation compares recovered bytes with the new original argument.
   Thus after storing `[ff fe]`, retrying `[ff fe]` conflicts with the recovered
   two UTF-8 U+FFFD characters; retrying those two valid characters matches.
   This behavior comes from the fixed Go implementation, not a new identity rule.

The regression exercises actual FileJournal/admission files and actual Driver
preparation/history/publication payload construction. OS observation/input is
modeled, and this is not public CLI/game/Windows acceptance evidence.

## Asymmetric artifact limits retained from Go

The original source has at most 262144 bytes. After JSON cloning, each source
byte contributes at most the 3-byte UTF-8 U+FFFD, so the largest decoded code
artifact is 786432 bytes. Although JSON may encode a control byte as six ASCII
characters (`\u0000`), JSON decoding restores that byte before artifact storage.
Result artifacts retain their original base64-decoded bytes (at most 512 KiB).

Go `snapshot.put` permits an absent artifact's first write above its 512-KiB
READ limit. Rust's dedicated artifact writer now uses the real decoded size,
bounded at 786432, while checking every existing artifact with the original
512-KiB limit. It preserves existing identical files, rejects different content,
and never repairs or replaces conflicting evidence. A maximal invalid-byte source
can save once but then fail restoration or a repeated existing-artifact write.
A smaller expansion may fit the artifact read limit but fail State's 262144-byte
code limit after hydration. The tests preserve both fixed-Go asymmetries.
Result-file and archive `immutable` boundaries are unchanged. No limit is
increased on report acceptance, report bytes, artifact reads or request admission.

## Explicit next byte boundary: raw reports

This batch does not fix report JSON containing non-UTF8 string bytes. Go
`observations.go` and `state.go` use `json.Valid`; the valid fixture
`7b2273223a22ff227d` is accepted and RawMessage output retains byte ff.
Rust observations/state currently validate via `serde_json::RawValue` and the
public RawReport serializer requires a UTF-8 `str`. Both therefore reject this
fixture. The new generic typed writer also accepts raw JSON as `&str` only.

The follow-on needs one bounded byte-level Go JSON validation/compaction path
(maximum 512 KiB report payload), preserving exact body bytes/checksum, and a
trusted raw-byte serializer/output boundary. Converting to Value, replacing the
invalid byte, or inventing a base64 public report changes the contract. The
consumer scope is observations, State result validation, RawReport, and the
public typed writer/CLI projection; it must be separately owned and reviewed.
This gap is recorded for completion, not waived.

## Independent review revision 2

Two P2 findings were repaired in this candidate. `write_result` formerly compared
an already-normalized durable Operation with the still-raw live Operation. It
now compares the complete decoded persisted representation, preserving every
other identity/result field, and reuses those exact encoded bytes for the
standalone result file. The live Operation is never rewritten or normalized.
The new real Driver/FileJournal regression seeds validated ResultVerified facts,
then exercises standalone result persistence, `result_durable`, ReleaseReady,
and the exact release intent. It preserves raw live source and exact report
bytes/checksum, and verifies normalized history after reopening. Eleven distinct
identity/report differences remain rejected and poison the failed writer as
before.

The prepare-publication regression no longer uses the recovery-only Peer, which
intentionally rejects business preparation. A dedicated recording backend admits
only its selected action, validates and records its exact envelope, and returns a
controlled stop before any input. The original recovery fixture is unchanged.
All 18 new declarations remain NOT_RUN; this revision awaits independent review.
