# Native report raw-byte parity

This is a source-derived implementation contract, not an execution or acceptance
claim. Fixed application oracle: Go v3.2.0 commit
`3057a9666ab078270b63c7f06de748069b0d1f4b`; standard library: Go 1.27.1.

## Boundary and architecture

Native `observations.go` uses `json.Valid(record.Payload)` after BODY identity,
length and Adler32 checks; `state.go` repeats JSON, 512 KiB and checksum checks.
Go's byte scanner accepts any unescaped string byte >= 0x20, including 0xff.
Its escaped Unicode code units need not form surrogate pairs. Object keys may
repeat. Numeric tokens retain their original lexemes and may exceed binary64.
The separate legacy `bridge/report.go` UTF-8 gate is not used by this native path.

The new `raw_json` module implements that grammar iteratively, with Go's 10000
container depth and no document tree, numeric conversion, UTF-8 fabrication,
unsafe code, or report-sized copy. RawJsonBytes holds a validated borrowed slice.
The existing native 512 KiB cap stays at consumers; this generic scanner invents
no smaller public output limit. Only observations, driver result validation,
State result validation and RawReport presentation change.

The typed public writer recognizes one explicit private newtype marker. Its
body must call `serialize_bytes` directly: strings, sequences, maps, options and
nested newtypes are rejected. A local scoped Cell flag enables the formatter's
byte-array hook for that call only; ordinary bytes still encode as JSON arrays.
The flag is reset on errors and unwinding, with independent state for preflight
and real output. No RefCell, global registry, thread-local storage or writer
aliasing is involved. The payload's local `is_human_readable=false` handshake is
an adapter signal, not authentication; marker spoofing still cannot skip grammar
validation. Other binary serializers are outside this presentation API's contract.

RawReport retains its public Vec<u8> type. UTF-8 reports still use the existing
borrowed RawValue path. Non-UTF-8 reports use RawJsonBytes and ordinary serde_json
serialization fails closed rather than emitting an array or replacement text.
Native public consumers must retain the typed ProjectResult through the writer.
Raw strings retain every non-whitespace byte, escape, HTML character, separator,
duplicate key and number spelling. Pretty output counts enclosing typed depth;
compact RawMessage validation counts its own depth, matching Go.

Existing Value/NumberLayout production code is unchanged. Typed finite-float
checks remain active before and after raw fields. Deterministic two-pass
serialization preflights to a sink before output; real IO failures retain
external.io/exit 5 and can leave partial bytes. Arbitrary stateful custom
Serialize implementations can still change between passes; no unconditional
OS-atomic guarantee or performance improvement is claimed.

## Consumer audit

- HEAD/BODY record reader preserves payload bytes and qualifies length/checksum
  before the observations byte-JSON check. Driver simulation requalification
  calls the same scanner and keeps target, checksum and length checks.
- State validation calls the scanner only for report-bearing stages. ResultBytes
  remains standard padded base64 on disk. Its decoder and all report capacities
  remain unchanged.
- Journal snapshot clones through that base64 representation, extracts exact
  result bytes into hash-addressed artifacts, and hydrates bytes before State
  validation. Journal `go_json_bytes` and MemoryEvent RawValue process snapshot
  metadata, never unescaped native report payloads. Their UTF-8 gates stay intact.
- Durable result files serialize Operation/ResultBytes, not RawReport. History
  loads/hydrates the same State. No recovery budget, unknown-input replay,
  request identity, capacity, or cleanup behavior changes.
- Connection status/report/history produce ProjectResult; its only raw field is
  RawReport. The new service fixture confirms exact payload and no log mutation.
- Formal CLI R2 snapshot retains typed LiveResult into Envelope/Record/End and
  uses the same compact/pretty writer APIs for JSON, JSONL and text. No new CLI
  API or source edit is required. Actual CLI byte-fixture tests and executable
  JSON/JSONL/text differential remain a follow-on integration gate; this batch
  does not claim a CLI runtime pass.
- New activation/doctor work uses ProjectResult inside typed structures and is
  not edited here. Its independent freeze and final integrated build remain
  separate gates.

## Evidence and open gates

`native-report-bytes-freeze.json` identifies exact owned and dependency bytes;
`native-report-bytes-test-map.json` maps 15 new declared tests. Formatting and
source/hash checks do not establish Rust type correctness or product behavior.
Cargo/rustc/Go/Windows/game/performance executions are NOT_RUN because the local
controlled build lease is invalid. Fresh independent review, controlled exact
candidate builds/tests, fixed-Go byte differential, old writer regressions and
same-machine performance checks remain required.

The temporary executor transport outage interrupted a planned fixture edit,
not a product run. The actual saved tree was inspected after reconnect; the
fixture was then applied and formatted. No unobserved command was counted as
successful or used to clear a process lease.
