# Evidence record protocol

Saved records use the `lychee.evidence.v1` envelope. A Ticket is the stable lookup key and is also stored inside the record so an Agent can verify that it found the intended evidence.

## Lookup

Search the account SavedVariables for the exact Ticket, then read:

```text
LycheeDevDB.exports.records[TICKET]
```

The complete exported text is stored once at:

```text
LycheeDevDB.exports.records[TICKET].payload.content
```

## Shape

```lua
{
    schema = "lychee.evidence.v1",
    ticket = "LYCHEE-YYYYMMDD-HHMMSS-NNNN",
    createdAt = 0,
    source = {
        kind = "run_result",
        title = "...",
    },
    payload = {
        mediaType = "text/plain",
        encoding = "utf-8",
        content = "...",
        byteCount = 0,
    },
    environment = {
        addonName = "Lychee Dev",
        clientId = "retail",
        version = "12.1.0",
        build = "...",
        buildDate = "...",
        interface = 120100,
        locale = "zhCN",
    },
    metadata = {},
}
```

`source.kind` is a stable machine identifier. `source.title` and the payload may be localized display content. `metadata` contains bounded feature-specific context and must not be treated as a complete copy of the payload.

Automation results commit through the same envelope: `source.kind` is `automation_result` (or `error_log` for the built-in bug snapshot), `payload.mediaType` is `application/json`, and the payload holds one complete `lychee.automation.result.v1` report. `metadata` then carries `taskId`, `executionId`, `revision`, `resultSchema`, `status`, `complete`, `checksumAlgorithm`, and `contentChecksum` (Adler-32 over the exact UTF-8 payload bytes). Automation records created in the current session are additionally protected from pruning, deletion and cache clears until the output-side reload writes them to disk.

## Persistence

Creating a record updates the in-memory SavedVariables table. World of Warcraft writes it to disk only on `/reload`, logout, or normal client exit. The UI marks records created in the current session as pending until one of those lifecycle events occurs.

## Migration

Database schema 7 upgrades export storage to version 2. Existing version 1 records are migrated after `ADDON_LOADED`:

- Ticket, creation time, order, metadata, client information, and unknown fields are preserved.
- `kind`, `title`, and `metadata.path` move under `source`.
- `content` and `byteCount` move under `payload`.
- `client` moves under `environment`.
- Legacy duplicate top-level fields are removed after migration.

Migration is idempotent. Records already using an evidence envelope are normalized without duplicating their payload.

## Historical performance records

Removing the performance tools does not change the database schema or delete existing evidence. The saved-records page retains labels for legacy `performance_snapshot`, `performance_health`, `performance_capture`, `performance_benchmark`, and `performance_storage` kinds. Their complete payload remains at the same Ticket path; normal storage limits and explicit user deletion still apply. No performance runtime is required to read these records.
