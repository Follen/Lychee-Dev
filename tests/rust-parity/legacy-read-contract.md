# Fixed-Go historical Live recovery map

Oracle: Go 3.2.0 `3057a9666ab078270b63c7f06de748069b0d1f4b`.
This first slice implements only retired non-CON resume admission and BTP status.
It does not restore historical input, execution, confirmation, or release routes.
All new Rust tests are **NOT_RUN**. Rustfmt parsing and diff checks are static
checks, not compilation, native acceptance, or measured latency evidence.

| Contract | Fixed Go source | First slice |
| --- | --- | --- |
| Missing legacy resume ID precedes workspace open; any non-CON resume rejects exit 2 with the exact retirement text | `internal/command/entry.go:705-715` | Implemented in formal CLI admission; tests NOT_RUN |
| CON routing remains ahead of the legacy workspace branch | `internal/command/live_channel_windows.go:24-42` | Existing formal route retained |
| BTP status reads `bootstrap/attempt/<id>`, validates typed schema and identity, does not capture or send input | `internal/command/entry.go:726-733`; `internal/live/history_bootstrap.go:160-175` | Implemented; errors retain requested operation ID and no result; tests NOT_RUN |
| Historical DTO includes unknown/partial receiver state without granting execution authority; ignores extra typed output fields, merges repeated nested objects, preserves exact quoted window identities | `internal/live/history_bootstrap.go:19-45,178-194`; `internal/desktop/receiver_bindings.go:11-15`; `internal/bridge/signal.go:23-65` | Implemented; existing admission Go object decoder reused; tests NOT_RUN |
| OP status verifies work generation and available report/error/reload/display evidence; report available plus cleanup pending stays usable; raw report JSON retained | `internal/live/journal/book.go:162-175`; `internal/live/outcome.go:99-193`; `internal/live/history_records.go:51-80,155-259`; `internal/live/history_display.go:34-123` | OPEN, migration_incomplete remains |
| Historical SESSION projection requires record hash, bindings, pin, capture hash/provenance, canonical payload, and ready identity; never authorizes fresh input | `internal/live/history_session.go:47-151`; `internal/command/entry.go:756-764` | OPEN, migration_incomplete remains |
| Cancel only prepared+pending+null observation; no game effects | `internal/live/history_retire.go:47-126`; `internal/live/journal/book.go:177-239` | OPEN; needs atomic work/ownership CAS and exact retained owner lease |
| OP abandon advances abandoning then abandoned, preserves request/observation byte content and unknown results, exact owner retirement only; idempotent terminal retry | `internal/live/history_retire.go:47-126`; `internal/live/history_retire_test.go:20-116`; `internal/live/journal/window_run.go:39-115` | OPEN; sequential independent metadata puts are prohibited |
| BTP abandon checks original claimed owner, current and window indices, refuses confirmed; changes only phase/time while preserving unknown persisted JSON fields | `internal/live/history_bootstrap.go:47-157` | OPEN; no mutation is implemented in this slice |
| There are no public historical confirm/release/result commands; current CON report confirmation/release remains separate | `internal/command/command_contract.go:237-242` | No invented command or replay path |
| Historical flags are the frozen command contract, including workspace `--home`; CON flags do not justify legacy input | `internal/command/command_contract.go:237-242,334-341` | Existing parser retained; no new flags |

## Bounded read and remaining dependent limitation

The BTP route uses the new `Metadata::read_raw_controlled`, never the existing
five-second SQLite busy handler. Read-only open, schema inspection and raw-byte
read use the original operation control through the existing bounded retry
helper. The SQL row read is an atomic snapshot, and rejects >1MiB before byte
allocation. This bound is fixed Go `internal/vault/metadata.go:126,167`, not a
new BTP limit. The connection is explicitly closed on success and failure;
primary failure remains primary if close also fails. No metadata mutation API
has changed and no multi-document write has been added.

Independent static review found and fixed repeated-history slice backing reuse,
raw invalid UTF-8 before normalization, raw timestamp escapes/RFC3339 fallback,
and missing-record error text. These fixes and all other new behavior remain
NOT_RUN. Runtime latency, OS locks and checked-close paths need execution.

The inherited `workspace::open` identity/path failures are not yet fully mapped
to fixed-Go errors (for example, a missing workspace.json has different legacy
workspace/OS-error classification). This slice does not claim those diagnostics
or complete historical recovery parity. The fixed-Go 10,000 nesting limit and render-only invalid time-offset failures
now have explicit NOT_RUN regressions; their differential execution remains open.

The new DTOs are persistence projections only. They introduce no native service,
physical input, game capture, current readiness checks, or mutable ownership.
